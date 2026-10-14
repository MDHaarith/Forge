use serde_json::{json, Value};

#[derive(Debug)]
enum Node {
    Atom(String),
    List(Vec<Node>),
}
impl Node {
    fn atom(&self) -> &str { if let Self::Atom(s) = self { s } else { "" } }
    fn list(&self) -> &[Node] { if let Self::List(v) = self { v } else { &[] } }
    fn field(&self, name: &str) -> Option<&Node> {
        self.list().iter().find(|n| n.list().first().is_some_and(|n| n.atom() == name))
    }
    fn number(&self, i: usize) -> Result<f64, String> {
        self.list().get(i).and_then(|n| n.atom().parse::<f64>().ok()).filter(|v| v.is_finite())
            .ok_or_else(|| "kicad: invalid number".into())
    }
    fn point(&self) -> Result<[f64; 2], String> { Ok([self.number(1)?, -self.number(2)?]) }
}

fn read(text: &str) -> Result<Node, String> {
    let mut stack: Vec<Vec<Node>> = vec![vec![]];
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '(' => {
                // shortcut: 128 nested expressions, raise only with an iterative node destructor for deeper foreign files.
                if stack.len() >= 128 { return Err("kicad: nesting is too deep".into()); }
                stack.push(vec![]);
            }
            ')' => {
                if stack.len() == 1 { return Err("kicad: unbalanced parentheses".into()); }
                let list = stack.pop().unwrap();
                stack.last_mut().unwrap().push(Node::List(list));
            }
            ';' => { for c in chars.by_ref() { if c == '\n' { break; } } }
            c if c.is_whitespace() => {}
            '"' => {
                let mut atom = String::new();
                let mut closed = false;
                while let Some(c) = chars.next() {
                    match c {
                        '"' => { closed = true; break; }
                        '\\' => atom.push(chars.next().ok_or("kicad: unbalanced parentheses")?),
                        _ => atom.push(c),
                    }
                }
                if !closed { return Err("kicad: unbalanced parentheses".into()); }
                stack.last_mut().unwrap().push(Node::Atom(atom));
            }
            c => {
                let mut atom = c.to_string();
                while chars.peek().is_some_and(|c| !c.is_whitespace() && *c != '(' && *c != ')') {
                    atom.push(chars.next().unwrap());
                }
                stack.last_mut().unwrap().push(Node::Atom(atom));
            }
        }
    }
    if stack.len() != 1 { return Err("kicad: unbalanced parentheses".into()); }
    let mut root = stack.pop().unwrap();
    if root.len() != 1 { return Err("kicad: not a footprint file".into()); }
    Ok(root.pop().unwrap())
}

fn geometry(node: &Node) -> Result<Vec<[f64; 2]>, String> {
    let mut points = vec![];
    for name in ["start", "end"] {
        if let Some(p) = node.field(name) { points.push(p.point()?); }
    }
    if let Some(pts) = node.field("pts") {
        for xy in pts.list().iter().filter(|n| n.list().first().is_some_and(|n| n.atom() == "xy")) {
            points.push(xy.point()?);
        }
    }
    if let (Some(center), Some(end)) = (node.field("center"), node.field("end")) {
        let c = center.point()?;
        let e = end.point()?;
        let r = (e[0] - c[0]).hypot(e[1] - c[1]);
        points.extend([[c[0] - r, c[1] - r], [c[0] + r, c[1] + r]]);
    }
    Ok(points)
}

fn bounds(points: &[[f64; 2]]) -> [f64; 4] {
    points.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |mut b, p| {
        for i in 0..2 { b[i] = b[i].min(p[i]); b[i + 2] = b[i + 2].max(p[i]); } b
    })
}
fn polygon(points: &[[f64; 2]]) -> Value {
    let [x0, y0, x1, y1] = bounds(points);
    json!([[x0,y0], [x1,y0], [x1,y1], [x0,y1]])
}

/// Read modern and legacy KiCad footprints without an installed KiCad library.
pub fn parse(text: &str) -> Result<(String, Value), String> {
    let root = read(text)?;
    let header = root.list();
    if !header.first().is_some_and(|n| matches!(n.atom(), "footprint" | "module")) || header.len() < 2 {
        return Err("kicad: not a footprint file".into());
    }
    let name = header[1].atom().to_owned();
    let mut pads = vec![];
    let mut courtyard = vec![];
    let mut body = vec![];
    for node in header.iter().skip(2) {
        let fields = node.list();
        let tag = fields.first().map(Node::atom).unwrap_or("");
        if tag == "pad" && fields.len() >= 4 {
            let number = fields[1].atom();
            let kind = fields[2].atom();
            if !matches!(kind, "smd" | "thru_hole" | "np_thru_hole") { continue; }
            let size = node.field("size").ok_or_else(|| format!("kicad: pad {number} has no size"))?;
            let mut size = [size.number(1)?, size.number(2)?];
            let mut at = node.field("at").map(Node::point).transpose()?.unwrap_or([0.; 2]);
            let rotation = node.field("at").filter(|n| n.list().len() > 3).map(|n| n.number(3)).transpose()?.unwrap_or(0.);
            let mut shape = fields[3].atom();
            if shape == "custom" {
                // shortcut: custom pad primitives become their bounding rectangle, add exact regions before importing RF-shaped lands.
                let mut points = vec![[-size[0]/2., -size[1]/2.], [size[0]/2., size[1]/2.]];
                if let Some(primitives) = node.field("primitives") {
                    for primitive in primitives.list().iter().skip(1) {
                        let width = primitive.field("width").map(|n| n.number(1)).transpose()?.unwrap_or(0.) / 2.;
                        for p in geometry(primitive)? {
                            points.extend([[p[0]-width,p[1]-width], [p[0]+width,p[1]+width]]);
                        }
                    }
                }
                let [x0,y0,x1,y1] = bounds(&points);
                let (s,c) = rotation.to_radians().sin_cos();
                let (x,y) = ((x0+x1)/2., (y0+y1)/2.);
                at = [at[0]+c*x-s*y, at[1]+s*x+c*y];
                size = [x1-x0,y1-y0];
                shape = "rect";
            }
            let drill = if let Some(d) = node.field("drill") {
                if d.list().get(1).is_some_and(|n| n.atom() == "oval") { json!([d.number(2)?,d.number(3)?]) }
                else { json!(d.number(1)?) }
            } else { Value::Null };
            let bottom = node.field("layers").is_some_and(|n| n.list().iter().any(|n| n.atom() == "B.Cu"));
            let mut pad = json!({"number":number,"at":at,"size":size,"shape":shape,"drill":drill,
                "layers":if kind != "smd" { "through" } else if bottom { "bottom" } else { "top" },"rotation":rotation});
            if shape == "roundrect" {
                pad["roundrect_rratio"] = json!(node.field("roundrect_rratio").map(|n| n.number(1)).transpose()?.unwrap_or(0.25));
            }
            if kind == "np_thru_hole" { pad["plated"] = json!(false); }
            pads.push(pad);
        } else if matches!(tag, "fp_line" | "fp_rect" | "fp_poly") {
            let layer = node.field("layer").and_then(|n| n.list().get(1)).map(Node::atom);
            match layer {
                Some("F.CrtYd") => courtyard.extend(geometry(node)?),
                Some("F.Fab") => body.extend(geometry(node)?),
                _ => {}
            }
        }
    }
    let mut value = json!({"pads":pads,"height":1});
    if !courtyard.is_empty() { value["courtyard"] = polygon(&courtyard); }
    if !body.is_empty() { value["body"] = polygon(&body); }
    super::footprint::native(&value)?;
    crate::jsfmt::normalize_json(&mut value);
    Ok((name, value))
}
