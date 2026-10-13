mod common;
#[path = "common/tools.rs"]
mod support;

use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use support::{project, run};

// =============================================================================
// Helper: SVG element parser
// =============================================================================

#[derive(Debug, Clone)]
struct SvgElement {
    tag: String,
    attrs: HashMap<String, String>,
}

fn parse_svg_elements(svg: &str) -> Vec<SvgElement> {
    let mut elements = Vec::new();
    let mut cursor = 0;
    while let Some(start) = svg[cursor..].find('<') {
        let tag_start = cursor + start;
        if let Some(end) = svg[tag_start..].find('>') {
            let tag_content = &svg[tag_start + 1..tag_start + end];
            if !tag_content.starts_with('/')
                && !tag_content.starts_with('!')
                && !tag_content.starts_with('?')
            {
                let tag_name = tag_content
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_end_matches('/');
                let mut attrs = HashMap::new();
                let mut rest = &tag_content[tag_name.len()..];
                while let Some(eq_idx) = rest.find('=') {
                    let attr_name = rest[..eq_idx]
                        .trim()
                        .rsplit(|c: char| c.is_whitespace() || c == '/')
                        .next()
                        .unwrap_or("")
                        .trim();
                    let after_eq = rest[eq_idx + 1..].trim_start();
                    if let Some(quote_char) = after_eq.chars().next() {
                        if quote_char == '"' || quote_char == '\'' {
                            let val_start = 1;
                            if let Some(val_end) = after_eq[val_start..].find(quote_char) {
                                let val = &after_eq[val_start..val_start + val_end];
                                attrs.insert(attr_name.to_string(), val.to_string());
                                rest = &after_eq[val_start + val_end + 1..];
                                continue;
                            }
                        }
                    }
                    rest = &rest[eq_idx + 1..];
                }
                elements.push(SvgElement {
                    tag: tag_name.to_string(),
                    attrs,
                });
            }
            cursor = tag_start + end + 1;
        } else {
            break;
        }
    }
    elements
}

fn is_on_grid(v: f64) -> bool {
    ((v / 2.54) - (v / 2.54).round()).abs() < 1e-4
}

fn is_power_net(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    match upper.as_str() {
        "GND" | "AGND" | "DGND" | "PGND" | "GNDA" | "GNDD" | "VSS" | "VSSA" => true,
        "VCC" | "VDD" | "VDDA" | "VDDIO" | "VBAT" | "VBUS" | "VIN" | "VSYS" => true,
        _ => {
            let voltage = upper.strip_prefix('+').unwrap_or(&upper);
            if let Some((a, b)) = voltage.split_once('V') {
                !a.is_empty()
                    && a.bytes().all(|c| c.is_ascii_digit())
                    && b.bytes().all(|c| c.is_ascii_digit())
            } else {
                false
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Rect {
    r#ref: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[derive(Debug, Clone)]
struct Pin {
    pin: String,
    x: f64,
    y: f64,
}

#[derive(Debug, Clone)]
struct WireSegment {
    net: String,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

impl WireSegment {
    fn new(net: &str, p1: (f64, f64), p2: (f64, f64)) -> Self {
        Self {
            net: net.to_string(),
            x1: p1.0,
            y1: p1.1,
            x2: p2.0,
            y2: p2.1,
            min_x: p1.0.min(p2.0),
            max_x: p1.0.max(p2.0),
            min_y: p1.1.min(p2.1),
            max_y: p1.1.max(p2.1),
        }
    }
    fn is_horizontal(&self) -> bool {
        (self.y1 - self.y2).abs() < 1e-6
    }
    fn is_vertical(&self) -> bool {
        (self.x1 - self.x2).abs() < 1e-6
    }
}

fn resolve_endpoint(spec: &Value, endpoint: &str) -> Vec<String> {
    let (part_ref, pin_name) = endpoint.rsplit_once('.').unwrap();
    let spec_parts = spec["parts"].as_array().unwrap();
    let part = spec_parts
        .iter()
        .find(|p| p["ref"].as_str() == Some(part_ref))
        .unwrap();
    let kind = part["kind"].as_str().unwrap_or("");
    if kind == "led" || kind == "diode" {
        if pin_name == "A" {
            return vec![format!("{part_ref}.1")];
        } else if pin_name == "K" {
            return vec![format!("{part_ref}.2")];
        }
    }
    if let Some(pins_obj) = part.get("pins").and_then(|p| p.as_object()) {
        let matching: Vec<String> = pins_obj
            .iter()
            .filter(|(_, label)| label.as_str() == Some(pin_name))
            .map(|(num, _)| format!("{part_ref}.{num}"))
            .collect();
        if !matching.is_empty() {
            return matching;
        }
    }
    vec![endpoint.to_string()]
}

fn assert_schematic_contract(svg: &str, spec: &Value) {
    let elements = parse_svg_elements(svg);

    // 1. Every part: one <g class="part" data-ref="REF"> and exactly one <rect class="body" data-ref="REF" x y width height>
    let spec_parts = spec["parts"].as_array().expect("spec.parts array");
    let mut part_refs = HashSet::new();
    for p in spec_parts {
        let r = p["ref"].as_str().expect("part ref");
        part_refs.insert(r.to_string());
    }

    let mut g_parts = Vec::new();
    let mut body_rects = Vec::new();
    for el in &elements {
        if el.tag == "g" && el.attrs.get("class").map(|s| s.as_str()) == Some("part") {
            let r = el.attrs.get("data-ref").expect("g.part data-ref");
            g_parts.push(r.clone());
        }
        if el.tag == "rect" && el.attrs.get("class").map(|s| s.as_str()) == Some("body") {
            let r = el.attrs.get("data-ref").expect("rect.body data-ref");
            let x: f64 = el.attrs.get("x").expect("rect.body x").parse().unwrap();
            let y: f64 = el.attrs.get("y").expect("rect.body y").parse().unwrap();
            let w: f64 = el
                .attrs
                .get("width")
                .expect("rect.body width")
                .parse()
                .unwrap();
            let h: f64 = el
                .attrs
                .get("height")
                .expect("rect.body height")
                .parse()
                .unwrap();
            body_rects.push(Rect {
                r#ref: r.clone(),
                x,
                y,
                w,
                h,
            });
        }
    }

    assert_eq!(
        g_parts.len(),
        spec_parts.len(),
        "Count of <g class=\"part\"> must match parts count"
    );
    assert_eq!(
        body_rects.len(),
        spec_parts.len(),
        "Count of <rect class=\"body\"> must match parts count"
    );
    for r in &part_refs {
        assert_eq!(
            g_parts.iter().filter(|g| *g == r).count(),
            1,
            "Part {r} must have exactly one <g class=\"part\">"
        );
        assert_eq!(
            body_rects.iter().filter(|b| &b.r#ref == r).count(),
            1,
            "Part {r} must have exactly one <rect class=\"body\">"
        );
    }

    // 2. Every pad of every part: <path class="pin" data-pin="REF.NUM" data-at="X,Y" …>; data-at on grid; data-at strictly OUTSIDE its part's body rect.
    let mut pins = Vec::new();
    let rect_by_ref: HashMap<String, Rect> = body_rects
        .iter()
        .map(|r| (r.r#ref.clone(), r.clone()))
        .collect();

    for el in &elements {
        if el.tag == "path" && el.attrs.get("class").map(|s| s.as_str()) == Some("pin") {
            let pin_name = el.attrs.get("data-pin").expect("pin data-pin");
            let data_at = el.attrs.get("data-at").expect("pin data-at");
            let coords: Vec<f64> = data_at.split(',').map(|s| s.parse().unwrap()).collect();
            assert_eq!(coords.len(), 2, "pin data-at must be X,Y");
            let (px, py) = (coords[0], coords[1]);
            assert!(
                is_on_grid(px),
                "pin {pin_name} X coordinate {px} must be on grid"
            );
            assert!(
                is_on_grid(py),
                "pin {pin_name} Y coordinate {py} must be on grid"
            );

            let part_ref = pin_name.split('.').next().unwrap();
            let rect = rect_by_ref
                .get(part_ref)
                .unwrap_or_else(|| panic!("body rect for part {part_ref}"));
            // data-at strictly OUTSIDE its part's body rect
            let inside = px > rect.x + 1e-6
                && px < rect.x + rect.w - 1e-6
                && py > rect.y + 1e-6
                && py < rect.y + rect.h - 1e-6;
            assert!(
                !inside,
                "pin {pin_name} at ({px},{py}) is strictly inside body rect of {part_ref} [{},{},{},{}]",
                rect.x, rect.y, rect.w, rect.h
            );
            pins.push(Pin {
                pin: pin_name.clone(),
                x: px,
                y: py,
            });
        }
    }

    // 3. Body rects of different parts do not overlap (allow touching = no; require a gap ≥ 2·G = 5.08 between any two body rects – check axis-separated distance ≥ 5.08 − 1e-6).
    for i in 0..body_rects.len() {
        for j in i + 1..body_rects.len() {
            let r1 = &body_rects[i];
            let r2 = &body_rects[j];
            let dx = if r1.x + r1.w <= r2.x {
                r2.x - (r1.x + r1.w)
            } else if r2.x + r2.w <= r1.x {
                r1.x - (r2.x + r2.w)
            } else {
                0.0
            };
            let dy = if r1.y + r1.h <= r2.y {
                r2.y - (r1.y + r1.h)
            } else if r2.y + r2.h <= r1.y {
                r1.y - (r2.y + r2.h)
            } else {
                0.0
            };
            assert!(
                dx >= 5.08 - 1e-6 || dy >= 5.08 - 1e-6,
                "Body rect gap violation between {} and {}: dx={dx}, dy={dy} (gap must be >= 5.08 mm)",
                r1.r#ref,
                r2.r#ref
            );
        }
    }

    // 4. Wires: <polyline class="wire" data-net="NET" points="x1,y1 x2,y2 …">; consecutive points share x or y (|dx|<1e-6 or |dy|<1e-6) and are not identical; all points on grid.
    let mut polylines_by_net: HashMap<String, Vec<Vec<(f64, f64)>>> = HashMap::new();
    let mut wire_segments: Vec<WireSegment> = Vec::new();

    for el in &elements {
        if el.tag == "polyline" && el.attrs.get("class").map(|s| s.as_str()) == Some("wire") {
            let net = el.attrs.get("data-net").expect("wire data-net");
            let points_str = el.attrs.get("points").expect("wire points");
            let pts: Vec<(f64, f64)> = points_str
                .split_whitespace()
                .map(|p| {
                    let parts: Vec<f64> = p.split(',').map(|s| s.parse().unwrap()).collect();
                    (parts[0], parts[1])
                })
                .collect();
            for pt in &pts {
                assert!(is_on_grid(pt.0), "wire point x {} not on grid", pt.0);
                assert!(is_on_grid(pt.1), "wire point y {} not on grid", pt.1);
            }
            for w in pts.windows(2) {
                let dx = (w[0].0 - w[1].0).abs();
                let dy = (w[0].1 - w[1].1).abs();
                assert!(
                    dx >= 1e-6 || dy >= 1e-6,
                    "wire consecutive points identical: {:?}",
                    w
                );
                assert!(
                    dx < 1e-6 || dy < 1e-6,
                    "wire segment not orthogonal: ({},{}) -> ({},{})",
                    w[0].0,
                    w[0].1,
                    w[1].0,
                    w[1].1
                );
                let seg = WireSegment::new(net, w[0], w[1]);
                wire_segments.push(seg);
            }
            polylines_by_net.entry(net.clone()).or_default().push(pts);
        }
    }

    // 5. No wire point and no wire segment lies strictly inside any body rect
    for rect in &body_rects {
        for seg in &wire_segments {
            // Check endpoints
            for (px, py) in [(seg.x1, seg.y1), (seg.x2, seg.y2)] {
                let inside = px > rect.x + 1e-6
                    && px < rect.x + rect.w - 1e-6
                    && py > rect.y + 1e-6
                    && py < rect.y + rect.h - 1e-6;
                assert!(
                    !inside,
                    "wire point ({px},{py}) on net {} lies strictly inside body rect of {}",
                    seg.net, rect.r#ref
                );
            }
            // Check interior intersection
            if seg.is_horizontal() {
                let y = seg.y1;
                if y > rect.y + 1e-6 && y < rect.y + rect.h - 1e-6 {
                    let overlap =
                        (seg.min_x < rect.x + rect.w - 1e-6) && (seg.max_x > rect.x + 1e-6);
                    assert!(
                        !overlap,
                        "wire horizontal segment ({},{})-({},{}) on net {} intersects body rect of {}",
                        seg.x1, seg.y1, seg.x2, seg.y2, seg.net, rect.r#ref
                    );
                }
            } else if seg.is_vertical() {
                let x = seg.x1;
                if x > rect.x + 1e-6 && x < rect.x + rect.w - 1e-6 {
                    let overlap =
                        (seg.min_y < rect.y + rect.h - 1e-6) && (seg.max_y > rect.y + 1e-6);
                    assert!(
                        !overlap,
                        "wire vertical segment ({},{})-({},{}) on net {} intersects body rect of {}",
                        seg.x1, seg.y1, seg.x2, seg.y2, seg.net, rect.r#ref
                    );
                }
            }
        }
    }

    // 6. For two different nets, no collinear overlap between segments (overlap length > 1e-6) and no vertex of one net lies on a segment of the other.
    for i in 0..wire_segments.len() {
        for j in i + 1..wire_segments.len() {
            let s1 = &wire_segments[i];
            let s2 = &wire_segments[j];
            if s1.net == s2.net {
                continue;
            }
            // Collinear overlap
            if s1.is_horizontal() && s2.is_horizontal() && (s1.y1 - s2.y1).abs() < 1e-6 {
                let overlap = s1.max_x.min(s2.max_x) - s1.min_x.max(s2.min_x);
                assert!(
                    overlap <= 1e-6,
                    "Collinear overlap between nets {} and {}: length {overlap}",
                    s1.net,
                    s2.net
                );
            }
            if s1.is_vertical() && s2.is_vertical() && (s1.x1 - s2.x1).abs() < 1e-6 {
                let overlap = s1.max_y.min(s2.max_y) - s1.min_y.max(s2.min_y);
                assert!(
                    overlap <= 1e-6,
                    "Collinear overlap between nets {} and {}: length {overlap}",
                    s1.net,
                    s2.net
                );
            }
        }
    }
    // Vertex of one net lies on a segment of another net
    for (net1, polys1) in &polylines_by_net {
        for pts in polys1 {
            for v in pts {
                for s2 in &wire_segments {
                    if &s2.net == net1 {
                        continue;
                    }
                    if s2.is_horizontal() {
                        if (v.1 - s2.y1).abs() < 1e-6
                            && v.0 >= s2.min_x - 1e-6
                            && v.0 <= s2.max_x + 1e-6
                        {
                            panic!(
                                "Vertex ({},{}) of net {} lies on segment ({},{})-({},{}) of net {}",
                                v.0, v.1, net1, s2.x1, s2.y1, s2.x2, s2.y2, s2.net
                            );
                        }
                    } else if s2.is_vertical() {
                        if (v.0 - s2.x1).abs() < 1e-6
                            && v.1 >= s2.min_y - 1e-6
                            && v.1 <= s2.max_y + 1e-6
                        {
                            panic!(
                                "Vertex ({},{}) of net {} lies on segment ({},{})-({},{}) of net {}",
                                v.0, v.1, net1, s2.x1, s2.y1, s2.x2, s2.y2, s2.net
                            );
                        }
                    }
                }
            }
        }
    }

    // Map pin to net from spec: explicit nets take precedence over part.pins labels
    let mut pin_to_net: HashMap<String, String> = HashMap::new();
    for p in spec_parts {
        let r = p["ref"].as_str().unwrap();
        if let Some(pins_obj) = p.get("pins").and_then(|po| po.as_object()) {
            for (pad_num, net_name) in pins_obj {
                if let Some(ns) = net_name.as_str() {
                    pin_to_net.insert(format!("{r}.{pad_num}"), ns.to_string());
                }
            }
        }
    }
    if let Some(nets_obj) = spec.get("nets").and_then(|n| n.as_object()) {
        for (net_name, pin_list) in nets_obj {
            if let Some(pins) = pin_list.as_array() {
                for p in pins {
                    if let Some(ps) = p.as_str() {
                        for actual_pin in resolve_endpoint(spec, ps) {
                            pin_to_net.insert(actual_pin, net_name.clone());
                        }
                    }
                }
            }
        }
    }

    // 7. A wire of net A never passes through (contains) the data-at point of a pin that is not on net A.
    for pin in &pins {
        let pin_net = pin_to_net.get(&pin.pin);
        for seg in &wire_segments {
            if pin_net == Some(&seg.net) {
                continue;
            }
            if seg.is_horizontal() {
                if (pin.y - seg.y1).abs() < 1e-6
                    && pin.x >= seg.min_x - 1e-6
                    && pin.x <= seg.max_x + 1e-6
                {
                    panic!(
                        "Wire of net {} passes through pin {} at ({},{}) which is not on net {}",
                        seg.net, pin.pin, pin.x, pin.y, seg.net
                    );
                }
            } else if seg.is_vertical() {
                if (pin.x - seg.x1).abs() < 1e-6
                    && pin.y >= seg.min_y - 1e-6
                    && pin.y <= seg.max_y + 1e-6
                {
                    panic!(
                        "Wire of net {} passes through pin {} at ({},{}) which is not on net {}",
                        seg.net, pin.pin, pin.x, pin.y, seg.net
                    );
                }
            }
        }
    }

    // 8. Junctions <circle class="junction" data-net="NET" cx cy r>: exactly at points where ≥3 wire-segment directions of the same net meet
    let mut actual_junctions: HashMap<String, HashSet<(i32, i32)>> = HashMap::new();
    for el in &elements {
        if el.tag == "circle" && el.attrs.get("class").map(|s| s.as_str()) == Some("junction") {
            let net = el.attrs.get("data-net").expect("junction data-net");
            let cx: f64 = el.attrs.get("cx").expect("junction cx").parse().unwrap();
            let cy: f64 = el.attrs.get("cy").expect("junction cy").parse().unwrap();
            let gx = (cx / 2.54).round() as i32;
            let gy = (cy / 2.54).round() as i32;
            assert!(
                ((cx / 2.54) - gx as f64).abs() < 1e-4,
                "junction cx not on grid"
            );
            assert!(
                ((cy / 2.54) - gy as f64).abs() < 1e-4,
                "junction cy not on grid"
            );
            assert!(
                actual_junctions
                    .entry(net.clone())
                    .or_default()
                    .insert((gx, gy)),
                "Duplicate junction for net {net} at ({gx},{gy})"
            );
        }
    }

    for (net, polys) in &polylines_by_net {
        let mut arms: HashMap<(i32, i32), HashSet<usize>> = HashMap::new();
        for pts in polys {
            for w in pts.windows(2) {
                let gx1 = (w[0].0 / 2.54).round() as i32;
                let gy1 = (w[0].1 / 2.54).round() as i32;
                let gx2 = (w[1].0 / 2.54).round() as i32;
                let gy2 = (w[1].1 / 2.54).round() as i32;
                if gx1 == gx2 {
                    let dy = if gy2 > gy1 { 1 } else { -1 };
                    let mut y = gy1;
                    while y != gy2 {
                        let next_y = y + dy;
                        arms.entry((gx1, y))
                            .or_default()
                            .insert(if dy > 0 { 2 } else { 3 });
                        arms.entry((gx1, next_y))
                            .or_default()
                            .insert(if dy > 0 { 3 } else { 2 });
                        y = next_y;
                    }
                } else if gy1 == gy2 {
                    let dx = if gx2 > gx1 { 1 } else { -1 };
                    let mut x = gx1;
                    while x != gx2 {
                        let next_x = x + dx;
                        arms.entry((x, gy1))
                            .or_default()
                            .insert(if dx > 0 { 0 } else { 1 });
                        arms.entry((next_x, gy1))
                            .or_default()
                            .insert(if dx > 0 { 1 } else { 0 });
                        x = next_x;
                    }
                }
            }
        }
        let net_junctions = actual_junctions.get(net).cloned().unwrap_or_default();
        for (&pt, dirs) in &arms {
            if dirs.len() >= 3 {
                assert!(
                    net_junctions.contains(&pt),
                    "Missing junction at grid {:?} for net {net} with {} arms",
                    pt,
                    dirs.len()
                );
            } else {
                assert!(
                    !net_junctions.contains(&pt),
                    "Extra junction at grid {:?} for net {net} with {} arms",
                    pt,
                    dirs.len()
                );
            }
        }
        for pt in &net_junctions {
            let arm_count = arms.get(pt).map_or(0, |s| s.len());
            assert!(
                arm_count >= 3,
                "Junction exists at {:?} for net {net} but arm count is {arm_count}",
                pt
            );
        }
    }

    // 9. Power rule (case-insensitive, whole name):
    // ground = GND AGND DGND PGND GNDA GNDD VSS VSSA;
    // supply = VCC VDD VDDA VDDIO VBAT VBUS VIN VSYS, or +?<digits>V<digits?> (5V, +12V) or <digits>V<digits> (3V3, 1V8).
    // Power nets have NO class="wire" elements; each pin on a power net has exactly one <g class="power" data-net="NET" data-at="X,Y"> with the same data-at as the pin.
    let mut power_symbols: HashMap<String, Vec<(f64, f64)>> = HashMap::new();
    for el in &elements {
        if el.tag == "g" && el.attrs.get("class").map(|s| s.as_str()) == Some("power") {
            let net = el.attrs.get("data-net").expect("power data-net");
            let data_at = el.attrs.get("data-at").expect("power data-at");
            let coords: Vec<f64> = data_at.split(',').map(|s| s.parse().unwrap()).collect();
            power_symbols
                .entry(net.clone())
                .or_default()
                .push((coords[0], coords[1]));
        }
    }

    // 10. Non-power nets with more than 8 pins have no wires; each of their pins gets <text class="label" data-net="NET" data-at="X,Y"> matching the pin's data-at.
    let mut labels: HashMap<String, Vec<(f64, f64)>> = HashMap::new();
    for el in &elements {
        if el.tag == "text" && el.attrs.get("class").map(|s| s.as_str()) == Some("label") {
            let net = el.attrs.get("data-net").expect("label data-net");
            let data_at = el.attrs.get("data-at").expect("label data-at");
            let coords: Vec<f64> = data_at.split(',').map(|s| s.parse().unwrap()).collect();
            labels
                .entry(net.clone())
                .or_default()
                .push((coords[0], coords[1]));
        }
    }

    // 11. Unconnected pads get <path class="nc" data-pin="REF.NUM" …>.
    let mut nc_pads = HashSet::new();
    for el in &elements {
        if el.tag == "path" && el.attrs.get("class").map(|s| s.as_str()) == Some("nc") {
            let pin_name = el.attrs.get("data-pin").expect("nc data-pin");
            nc_pads.insert(pin_name.clone());
        }
    }

    // Group pins by net
    let mut net_pins: HashMap<String, Vec<Pin>> = HashMap::new();
    for pin in &pins {
        if let Some(net) = pin_to_net.get(&pin.pin) {
            net_pins.entry(net.clone()).or_default().push(pin.clone());
        } else {
            // Unconnected pin
            assert!(
                nc_pads.contains(&pin.pin),
                "Unconnected pin {} missing <path class=\"nc\">",
                pin.pin
            );
        }
    }

    // Check each connected net
    for (net_name, pins_in_net) in &net_pins {
        let is_pwr = is_power_net(net_name);
        if is_pwr {
            // Power net: NO class="wire" elements
            assert!(
                !polylines_by_net.contains_key(net_name),
                "Power net {net_name} must have NO wires"
            );
            // Each pin has exactly one <g class="power" data-net="NET" data-at="X,Y"> with same data-at
            let pwr_list = power_symbols.get(net_name).cloned().unwrap_or_default();
            for pin in pins_in_net {
                let matches = pwr_list
                    .iter()
                    .filter(|(x, y)| (x - pin.x).abs() < 1e-4 && (y - pin.y).abs() < 1e-4)
                    .count();
                assert_eq!(
                    matches, 1,
                    "Pin {} on power net {net_name} must have exactly one power symbol at ({},{})",
                    pin.pin, pin.x, pin.y
                );
            }
        } else if pins_in_net.len() > 8 {
            // Non-power net with > 8 pins: no wires, each pin gets label
            assert!(
                !polylines_by_net.contains_key(net_name),
                "Non-power net {net_name} with >8 pins ({}) must have NO wires",
                pins_in_net.len()
            );
            let lbl_list = labels.get(net_name).cloned().unwrap_or_default();
            for pin in pins_in_net {
                let matches = lbl_list
                    .iter()
                    .filter(|(x, y)| (x - pin.x).abs() < 1e-4 && (y - pin.y).abs() < 1e-4)
                    .count();
                assert_eq!(
                    matches, 1,
                    "Pin {} on label net {net_name} must have exactly one label at ({},{})",
                    pin.pin, pin.x, pin.y
                );
            }
        } else {
            // Wired net: polylines exist
            let polys = polylines_by_net
                .get(net_name)
                .unwrap_or_else(|| panic!("Missing wires for net {net_name}"));
            // 12. Each connected pin is covered by wire vertex at its data-at
            let mut wire_vertices = HashSet::new();
            for pts in polys {
                for pt in pts {
                    let gx = (pt.0 / 2.54).round() as i32;
                    let gy = (pt.1 / 2.54).round() as i32;
                    wire_vertices.insert((gx, gy));
                }
            }
            for pin in pins_in_net {
                let gx = (pin.x / 2.54).round() as i32;
                let gy = (pin.y / 2.54).round() as i32;
                assert!(
                    wire_vertices.contains(&(gx, gy)),
                    "Pin {} at ({},{}) not touched by wire vertices of net {net_name}",
                    pin.pin,
                    pin.x,
                    pin.y
                );
            }
            // Check connected graph touching every pin
            let mut adj: HashMap<(i32, i32), Vec<(i32, i32)>> = HashMap::new();
            for pts in polys {
                for w in pts.windows(2) {
                    let gx1 = (w[0].0 / 2.54).round() as i32;
                    let gy1 = (w[0].1 / 2.54).round() as i32;
                    let gx2 = (w[1].0 / 2.54).round() as i32;
                    let gy2 = (w[1].1 / 2.54).round() as i32;
                    let dx = (gx2 - gx1).signum();
                    let dy = (gy2 - gy1).signum();
                    let (mut cx, mut cy) = (gx1, gy1);
                    while (cx, cy) != (gx2, gy2) {
                        let nx = cx + dx;
                        let ny = cy + dy;
                        adj.entry((cx, cy)).or_default().push((nx, ny));
                        adj.entry((nx, ny)).or_default().push((cx, cy));
                        cx = nx;
                        cy = ny;
                    }
                }
            }
            if !pins_in_net.is_empty() {
                let start = (
                    (pins_in_net[0].x / 2.54).round() as i32,
                    (pins_in_net[0].y / 2.54).round() as i32,
                );
                let mut visited = HashSet::new();
                let mut q = vec![start];
                visited.insert(start);
                while let Some(node) = q.pop() {
                    if let Some(neighbors) = adj.get(&node) {
                        for nxt in neighbors {
                            if visited.insert(*nxt) {
                                q.push(*nxt);
                            }
                        }
                    }
                }
                for pin in pins_in_net {
                    let p_node = ((pin.x / 2.54).round() as i32, (pin.y / 2.54).round() as i32);
                    assert!(
                        visited.contains(&p_node),
                        "Net {net_name} polylines do not form a single connected graph touching pin {}",
                        pin.pin
                    );
                }
            }
        }
    }
}

// =============================================================================
// Contract A: Schematic SVG Tests
// =============================================================================

const BLINKER_555_BOARD_JSON: &str = r#"{
  "board": {"width": 40, "height": 30, "layers": 2, "thickness": 1.6},
  "rules": {"trace": 0.2, "clearance": 0.2, "via": [0.6, 0.3], "edge": 0.3},
  "parts": [
    {"ref": "U1", "kind": "chip", "footprint": "soic8", "at": [0, 0], "pins": {"1": "GND", "2": "TRIG", "3": "OUT", "4": "RESET", "5": "CTRL", "6": "THRES", "7": "DISCH", "8": "VCC"}},
    {"ref": "R1", "kind": "resistor", "value": "1k", "footprint": "0603", "at": [-10, 5]},
    {"ref": "R2", "kind": "resistor", "value": "10k", "footprint": "0603", "at": [-10, -5]},
    {"ref": "R3", "kind": "resistor", "value": "330", "footprint": "0603", "at": [10, 5]},
    {"ref": "C1", "kind": "capacitor", "value": "10uF", "footprint": "0603", "at": [-10, -10]},
    {"ref": "C2", "kind": "capacitor", "value": "10nF", "footprint": "0603", "at": [10, -5]},
    {"ref": "LED1", "kind": "led", "value": "red", "footprint": "0603", "at": [15, 5]},
    {"ref": "J1", "kind": "header", "footprint": "pinheader_1x2", "at": [0, -10]}
  ],
  "nets": {
    "VCC": ["J1.1", "U1.8", "U1.4", "R1.1"],
    "GND": ["J1.2", "U1.1", "C1.2", "C2.2", "LED1.K"],
    "DISCH": ["R1.2", "R2.1", "U1.7"],
    "TIMING": ["R2.2", "C1.1", "U1.6", "U1.2"],
    "CTRL": ["U1.5", "C2.1"],
    "OUT": ["U1.3", "R3.1"],
    "LED_NET": ["R3.2", "LED1.A"]
  }
}"#;

#[tokio::test]
async fn test_schematic_555_blinker() {
    let dir = project("sch_555", &[("pcb/b555.pcb.json", BLINKER_555_BOARD_JSON)]);
    run(
        "pcb_build",
        json!({"source": "pcb/b555.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    let svg = std::fs::read_to_string(dir.path.join("pcb/b555-schematic.svg")).unwrap();
    let spec: Value = serde_json::from_str(BLINKER_555_BOARD_JSON).unwrap();
    assert_schematic_contract(&svg, &spec);
}

#[tokio::test]
async fn test_schematic_9pin_bus_label() {
    // Board (b): 9 resistors each with pin 1 on "BUS", pin 2 on "GND" to check labels
    let board_json = r#"{
      "board": {"width": 50, "height": 30, "layers": 2},
      "parts": [
        {"ref": "R1", "kind": "resistor", "value": "10k", "footprint": "0603"},
        {"ref": "R2", "kind": "resistor", "value": "10k", "footprint": "0603"},
        {"ref": "R3", "kind": "resistor", "value": "10k", "footprint": "0603"},
        {"ref": "R4", "kind": "resistor", "value": "10k", "footprint": "0603"},
        {"ref": "R5", "kind": "resistor", "value": "10k", "footprint": "0603"},
        {"ref": "R6", "kind": "resistor", "value": "10k", "footprint": "0603"},
        {"ref": "R7", "kind": "resistor", "value": "10k", "footprint": "0603"},
        {"ref": "R8", "kind": "resistor", "value": "10k", "footprint": "0603"},
        {"ref": "R9", "kind": "resistor", "value": "10k", "footprint": "0603"}
      ],
      "nets": {
        "BUS": ["R1.1", "R2.1", "R3.1", "R4.1", "R5.1", "R6.1", "R7.1", "R8.1", "R9.1"],
        "GND": ["R1.2", "R2.2", "R3.2", "R4.2", "R5.2", "R6.2", "R7.2", "R8.2", "R9.2"]
      }
    }"#;
    let dir = project("sch_bus", &[("pcb/bus.pcb.json", board_json)]);
    run(
        "pcb_build",
        json!({"source": "pcb/bus.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    let svg = std::fs::read_to_string(dir.path.join("pcb/bus-schematic.svg")).unwrap();
    let spec: Value = serde_json::from_str(board_json).unwrap();
    assert_schematic_contract(&svg, &spec);
}

#[tokio::test]
async fn test_schematic_power_vs_non_power_net_names() {
    // Board (c): nets named "3V3", "+5V", "gnd", "VSSA" (power) and "VREF", "GNDX", "A3V" (NOT power)
    let board_json = r#"{
      "board": {"width": 40, "height": 30, "layers": 2},
      "parts": [
        {"ref": "R1", "kind": "resistor", "value": "1k", "footprint": "0603"},
        {"ref": "R2", "kind": "resistor", "value": "1k", "footprint": "0603"},
        {"ref": "R3", "kind": "resistor", "value": "1k", "footprint": "0603"},
        {"ref": "R4", "kind": "resistor", "value": "1k", "footprint": "0603"}
      ],
      "nets": {
        "3V3": ["R1.1"],
        "gnd": ["R1.2"],
        "+5V": ["R2.1"],
        "VSSA": ["R2.2"],
        "VREF": ["R3.1", "R4.1"],
        "GNDX": ["R3.2"],
        "A3V": ["R4.2"]
      }
    }"#;
    let dir = project("sch_pwr", &[("pcb/pwr.pcb.json", board_json)]);
    run(
        "pcb_build",
        json!({"source": "pcb/pwr.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    let svg = std::fs::read_to_string(dir.path.join("pcb/pwr-schematic.svg")).unwrap();
    let spec: Value = serde_json::from_str(board_json).unwrap();
    assert_schematic_contract(&svg, &spec);
}

#[tokio::test]
async fn test_schematic_geometric_crossing_rules() {
    // Board (d): ~12-part board with a few crossing nets for the geometric rules
    let board_json = r#"{
      "board": {"width": 80, "height": 60, "layers": 2},
      "parts": [
        {"ref": "R1", "kind": "resistor", "value": "1k", "footprint": "0603"},
        {"ref": "R2", "kind": "resistor", "value": "1k", "footprint": "0603"},
        {"ref": "R3", "kind": "resistor", "value": "1k", "footprint": "0603"},
        {"ref": "R4", "kind": "resistor", "value": "1k", "footprint": "0603"},
        {"ref": "R5", "kind": "resistor", "value": "1k", "footprint": "0603"},
        {"ref": "R6", "kind": "resistor", "value": "1k", "footprint": "0603"},
        {"ref": "R7", "kind": "resistor", "value": "1k", "footprint": "0603"},
        {"ref": "R8", "kind": "resistor", "value": "1k", "footprint": "0603"},
        {"ref": "R9", "kind": "resistor", "value": "1k", "footprint": "0603"},
        {"ref": "R10", "kind": "resistor", "value": "1k", "footprint": "0603"},
        {"ref": "R11", "kind": "resistor", "value": "1k", "footprint": "0603"},
        {"ref": "R12", "kind": "resistor", "value": "1k", "footprint": "0603"}
      ],
      "nets": {
        "N1": ["R1.1", "R6.1"],
        "N2": ["R2.1", "R5.1"],
        "N3": ["R3.1", "R8.1"],
        "N4": ["R4.1", "R7.1"],
        "N5": ["R9.1", "R12.1"],
        "N6": ["R10.1", "R11.1"],
        "GND": ["R1.2", "R2.2", "R3.2", "R4.2", "R5.2", "R6.2", "R7.2", "R8.2", "R9.2", "R10.2", "R11.2", "R12.2"]
      }
    }"#;
    let dir = project("sch_geom", &[("pcb/geom.pcb.json", board_json)]);
    run(
        "pcb_build",
        json!({"source": "pcb/geom.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    let svg = std::fs::read_to_string(dir.path.join("pcb/geom-schematic.svg")).unwrap();
    let spec: Value = serde_json::from_str(board_json).unwrap();
    assert_schematic_contract(&svg, &spec);
}

#[tokio::test]
async fn test_schematic_determinism() {
    let dir1 = project("sch_det1", &[("pcb/det.pcb.json", BLINKER_555_BOARD_JSON)]);
    let dir2 = project("sch_det2", &[("pcb/det.pcb.json", BLINKER_555_BOARD_JSON)]);
    run(
        "pcb_build",
        json!({"source": "pcb/det.pcb.json"}),
        &dir1.path,
    )
    .await
    .unwrap();
    run(
        "pcb_build",
        json!({"source": "pcb/det.pcb.json"}),
        &dir2.path,
    )
    .await
    .unwrap();
    let svg1 = std::fs::read_to_string(dir1.path.join("pcb/det-schematic.svg")).unwrap();
    let svg2 = std::fs::read_to_string(dir2.path.join("pcb/det-schematic.svg")).unwrap();
    assert_eq!(
        svg1, svg2,
        "Schematic SVG builds must be byte-identical across separate projects"
    );
}

// =============================================================================
// Contract B: ERC lines in pcb_drc output
// =============================================================================

#[tokio::test]
async fn test_erc_trigger_all_rules_and_format() {
    let board_json = r#"{
      "board": {"width": 40, "height": 30, "layers": 2},
      "parts": [
        {"ref": "R1", "kind": "resistor", "value": "1k", "footprint": "0402", "at": [-10, 0], "pins": {"1": "SINGLE"}},
        {"ref": "D1", "kind": "diode", "footprint": "0603", "at": [0, 0], "pins": {"1": "LOOP", "2": "LOOP"}},
        {"ref": "C1", "kind": "capacitor", "value": "100n", "footprint": "0603", "at": [10, 0]},
        {"ref": "J1", "kind": "header", "footprint": "pinheader_1x2", "at": [0, -10], "pins": {"1": "CONN"}},
        {"ref": "R2", "kind": "resistor", "value": "10k", "footprint": "0402", "at": [10, -10], "pins": {"1": "CONN", "2": "CONN2"}},
        {"ref": "R3", "kind": "resistor", "value": "10k", "footprint": "0402", "at": [10, 10], "pins": {"1": "CONN2", "2": "CONN"}}
      ]
    }"#;
    let dir = project("erc_all", &[("pcb/erc.pcb.json", board_json)]);
    run(
        "pcb_build",
        json!({"source": "pcb/erc.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    let drc = run("pcb_drc", json!({"board": "erc"}), &dir.path)
        .await
        .unwrap();
    let lines: Vec<&str> = drc.text.lines().collect();

    // Check presence of required ERC lines:
    // 1. erc: net SINGLE has one pin (R1.1)
    assert!(lines.contains(&"erc: net SINGLE has one pin (R1.1)"));
    // 2. erc: R1.2 is not connected
    assert!(lines.contains(&"erc: R1.2 is not connected"));
    // 3. erc: D1 has both pins on net LOOP
    assert!(lines.contains(&"erc: D1 has both pins on net LOOP"));
    // 4. erc: C1 has no connected pins
    assert!(lines.contains(&"erc: C1 has no connected pins"));
    // J1 has an unused pin J1.2, but headers must NOT have erc: J1.2 is not connected
    assert!(!lines.iter().any(|l| l.contains("J1.2")));

    // Filter ERC lines
    let erc_lines: Vec<&&str> = lines.iter().filter(|l| l.starts_with("erc:")).collect();
    // Verify ERC lines are sorted
    for w in erc_lines.windows(2) {
        assert!(
            w[0] <= w[1],
            "ERC lines must be sorted: {} > {}",
            w[0],
            w[1]
        );
    }
    // Verify warning count line
    let warning_summary = format!("{} ERC warning(s).", erc_lines.len());
    assert!(
        lines.contains(&warning_summary.as_str()),
        "Missing or mismatched ERC warning count line: expected '{warning_summary}' in: {:?}",
        lines
    );
    // Last line is DRC summary
    let last = lines.last().unwrap();
    assert!(
        *last == "No DRC errors." || last.ends_with("DRC error(s)."),
        "Last line must be DRC summary: {last}"
    );
}

#[tokio::test]
async fn test_erc_header_unused_pins_produce_no_warning() {
    let board_json = r#"{
      "board": {"width": 30, "height": 20, "layers": 2},
      "parts": [
        {"ref": "J1", "kind": "header", "footprint": "pinheader_1x2", "at": [-5, 0]},
        {"ref": "R1", "kind": "resistor", "value": "1k", "footprint": "0402", "at": [5, 0]}
      ],
      "nets": {
        "SIG": ["J1.1", "R1.1"],
        "GND": ["R1.2"]
      }
    }"#;
    let dir = project("erc_hdr", &[("pcb/hdr.pcb.json", board_json)]);
    run(
        "pcb_build",
        json!({"source": "pcb/hdr.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    let drc = run("pcb_drc", json!({"board": "hdr"}), &dir.path)
        .await
        .unwrap();
    // J1.2 is unused; it should produce no warning for J1.2
    assert!(
        !drc.text.contains("J1.2 is not connected"),
        "Headers must never produce 'is not connected' warnings: {}",
        drc.text
    );
    assert!(
        !drc.text.contains("J1 has no connected pins"),
        "Header with connected pin must not report no connected pins: {}",
        drc.text
    );
}

#[tokio::test]
async fn test_erc_clean_board_has_no_erc_lines() {
    let board_json = r#"{
      "board": {"width": 30, "height": 20, "layers": 2},
      "parts": [
        {"ref": "R1", "kind": "resistor", "value": "1k", "footprint": "0402", "at": [-5, 0]},
        {"ref": "R2", "kind": "resistor", "value": "1k", "footprint": "0402", "at": [5, 0]}
      ],
      "nets": {
        "A": ["R1.1", "R2.1"],
        "B": ["R1.2", "R2.2"]
      }
    }"#;
    let dir = project("erc_clean", &[("pcb/clean.pcb.json", board_json)]);
    run(
        "pcb_build",
        json!({"source": "pcb/clean.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    let drc = run("pcb_drc", json!({"board": "clean"}), &dir.path)
        .await
        .unwrap();
    assert!(
        !drc.text.contains("erc:"),
        "Clean board must not contain any erc: lines: {}",
        drc.text
    );
    assert!(
        !drc.text.contains("ERC warning"),
        "Clean board must not contain ERC warning summary: {}",
        drc.text
    );
    assert_eq!(drc.text.trim(), "No DRC errors.");
}

#[tokio::test]
async fn test_erc_ordering_with_drc_faults() {
    // Board with deliberate DRC fault (unrouted net) AND ERC warning (single-pin net and unconnected pad)
    let board_json = r#"{
      "board": {"width": 30, "height": 20, "layers": 2},
      "parts": [
        {"ref": "R1", "kind": "resistor", "value": "1k", "footprint": "0402", "at": [-5, 0], "pins": {"1": "SINGLE"}},
        {"ref": "R2", "kind": "resistor", "value": "1k", "footprint": "0402", "at": [5, 0]}
      ]
    }"#;
    let dir = project("erc_order", &[("pcb/order.pcb.json", board_json)]);
    run(
        "pcb_build",
        json!({"source": "pcb/order.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    let drc = run("pcb_drc", json!({"board": "order"}), &dir.path)
        .await
        .unwrap();
    let lines: Vec<&str> = drc.text.lines().collect();
    let erc_indices: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.starts_with("erc:"))
        .map(|(i, _)| i)
        .collect();
    assert!(!erc_indices.is_empty(), "Must have ERC warnings");

    // All DRC faults must come before first ERC line
    for i in 0..erc_indices[0] {
        assert!(
            !lines[i].starts_with("erc:"),
            "DRC faults must precede ERC lines"
        );
    }
    // Final line is DRC summary
    let last = lines.last().unwrap();
    assert!(
        *last == "No DRC errors." || last.ends_with("DRC error(s)."),
        "Last line must be DRC summary: {last}"
    );
}

// =============================================================================
// Contract C: Rounded SMD pads
// =============================================================================

#[tokio::test]
async fn test_rounded_smd_pads() {
    let board_json = r#"{
      "board": {"width": 30, "height": 20, "layers": 2},
      "rules": {"trace": 0.2, "clearance": 0.2, "via": [0.6, 0.3], "edge": 0.3},
      "parts": [
        {"ref": "R1", "kind": "resistor", "value": "1k", "footprint": "0603", "at": [-5, 0], "rotation": 0},
        {"ref": "J1", "kind": "header", "footprint": "pinheader_1x2", "at": [5, 0], "rotation": 0}
      ],
      "nets": {
        "N1": ["R1.1", "J1.1"],
        "N2": ["R1.2", "J1.2"]
      }
    }"#;
    let dir = project("round_pad", &[("pcb/rnd.pcb.json", board_json)]);
    let build_res = run(
        "pcb_build",
        json!({"source": "pcb/rnd.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert!(
        build_res
            .text
            .contains("No errors: every connection is routed."),
        "Board should route cleanly: {}",
        build_res.text
    );
    let drc_res = run("pcb_drc", json!({"board": "rnd"}), &dir.path)
        .await
        .unwrap();
    assert_eq!(drc_res.text.trim(), "No DRC errors.");

    // 1. Circuit JSON check
    let raw_circuit = std::fs::read_to_string(dir.path.join("pcb/rnd.circuit.json")).unwrap();
    let circuit: Value = serde_json::from_str(&raw_circuit).unwrap();
    let smt_pads: Vec<&Value> = circuit
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["type"] == "pcb_smtpad")
        .collect();
    assert_eq!(smt_pads.len(), 2, "0603 resistor must have 2 SMD pads");

    let p0 = smt_pads[0];
    let w = p0["width"].as_f64().unwrap();
    let h = p0["height"].as_f64().unwrap();
    let expected_r = (0.25 * w.min(h)).min(0.25);
    let actual_r = p0["rect_border_radius"].as_f64().unwrap();
    assert!(
        (actual_r - expected_r).abs() < 1e-4,
        "Corner radius {} != expected {}",
        actual_r,
        expected_r
    );

    // Key order check in raw JSON: rect_border_radius placed immediately after height
    for smt in &smt_pads {
        let pad_id = smt["pcb_smtpad_id"].as_str().unwrap();
        let pad_pos = raw_circuit
            .find(&format!("\"pcb_smtpad_id\": \"{pad_id}\""))
            .unwrap();
        let obj_str = &raw_circuit[pad_pos..pad_pos + 400];
        let height_pos = obj_str.find("\"height\":").expect("height key");
        let radius_pos = obj_str
            .find("\"rect_border_radius\":")
            .expect("rect_border_radius key");
        assert!(
            radius_pos > height_pos,
            "rect_border_radius must appear after height"
        );
        let between = &obj_str[height_pos + 9..radius_pos];
        assert!(
            !between.contains("\":"),
            "No intermediate keys allowed between height and rect_border_radius: '{between}'"
        );
    }

    // Verify through-hole square pad stays sharp (pin 1 of pinheader_1x2)
    let th_pads: Vec<&Value> = circuit
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["type"] == "pcb_plated_hole")
        .collect();
    for th in &th_pads {
        assert!(
            th.get("rect_border_radius").is_none(),
            "Through-hole pads must not have rect_border_radius: {:?}",
            th
        );
    }

    // 2. Gerber checks: F_Cu, F_Mask, F_Paste
    let f_cu = std::fs::read_to_string(dir.path.join("fab/rnd/rnd-F_Cu.gbr")).unwrap();
    let f_mask = std::fs::read_to_string(dir.path.join("fab/rnd/rnd-F_Mask.gbr")).unwrap();
    let f_paste = std::fs::read_to_string(dir.path.join("fab/rnd/rnd-F_Paste.gbr")).unwrap();

    for (fname, content) in [("F_Cu", &f_cu), ("F_Mask", &f_mask), ("F_Paste", &f_paste)] {
        assert_eq!(
            content.matches("%AMRoundRect*").count(),
            1,
            "{fname} must define %AMRoundRect* exactly once"
        );
    }

    // Check F_Cu aperture: %ADDnnRoundRect,<r>X<x1>X<y1>X<x2>X<y2>X<x3>X<y3>X<x4>X<y4>*%
    let cu_roundrect_pos = f_cu.find("RoundRect,").expect("RoundRect aperture in F_Cu");
    let cu_aper_end = f_cu[cu_roundrect_pos..].find("*%").unwrap() + cu_roundrect_pos;
    let cu_aper_str = &f_cu[cu_roundrect_pos + 10..cu_aper_end];
    let cu_args: Vec<f64> = cu_aper_str.split('X').map(|s| s.parse().unwrap()).collect();
    assert_eq!(cu_args.len(), 9, "RoundRect aperture must have 9 arguments");
    let cu_r = cu_args[0];
    assert!(
        (cu_r - expected_r).abs() < 1e-4,
        "Cu aperture r {} != expected {}",
        cu_r,
        expected_r
    );

    // The four corner centres are at (±(w/2-r), ±(h/2-r))
    let exp_cx = w / 2.0 - expected_r;
    let exp_cy = h / 2.0 - expected_r;
    for i in 0..4 {
        let cx = cu_args[1 + 2 * i].abs();
        let cy = cu_args[2 + 2 * i].abs();
        assert!(
            (cx - exp_cx).abs() < 1e-4,
            "Corner x {} != expected {}",
            cx,
            exp_cx
        );
        assert!(
            (cy - exp_cy).abs() < 1e-4,
            "Corner y {} != expected {}",
            cy,
            exp_cy
        );
    }

    // Check F_Mask aperture and expansion
    let mask_roundrect_pos = f_mask
        .find("RoundRect,")
        .expect("RoundRect aperture in F_Mask");
    let mask_aper_end = f_mask[mask_roundrect_pos..].find("*%").unwrap() + mask_roundrect_pos;
    let mask_aper_str = &f_mask[mask_roundrect_pos + 10..mask_aper_end];
    let mask_args: Vec<f64> = mask_aper_str
        .split('X')
        .map(|s| s.parse().unwrap())
        .collect();
    assert_eq!(mask_args.len(), 9);
    let mask_r = mask_args[0];
    let mask_w = 2.0 * (mask_args[1].abs() + mask_r);
    let cu_w = 2.0 * (cu_args[1].abs() + cu_r);
    let expansion = (mask_w - cu_w) / 2.0;
    assert!(
        expansion > 0.0,
        "Mask expansion must be positive: {expansion}"
    );
    assert!(
        (mask_r - (cu_r + expansion)).abs() < 1e-4,
        "Mask r {} must equal copper r {} + expansion {}",
        mask_r,
        cu_r,
        expansion
    );

    // Count of D03 flashes using RoundRect apertures on F_Cu equals the number of top SMD rectangular pads (2)
    let mut roundrect_aper_ids = HashSet::new();
    let mut search_pos = 0;
    while let Some(idx) = f_cu[search_pos..].find("RoundRect,") {
        let abs_idx = search_pos + idx;
        let decl_start = f_cu[..abs_idx].rfind("%ADD").unwrap();
        let aper_id = &f_cu[decl_start + 4..abs_idx];
        roundrect_aper_ids.insert(aper_id.to_string());
        search_pos = abs_idx + 10;
    }
    assert!(
        !roundrect_aper_ids.is_empty(),
        "Must find at least one RoundRect aperture"
    );

    let mut current_aper: Option<String> = None;
    let mut roundrect_d03_count = 0;
    for line in f_cu.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('D') && trimmed.ends_with('*') {
            let num = &trimmed[1..trimmed.len() - 1];
            if let Ok(id) = num.parse::<u32>() {
                if id >= 10 {
                    current_aper = Some(num.to_string());
                }
            }
        }
        if trimmed.ends_with("D03*") {
            if let Some(ref aper) = current_aper {
                if roundrect_aper_ids.contains(aper) {
                    roundrect_d03_count += 1;
                }
            }
        }
    }
    assert_eq!(
        roundrect_d03_count, 2,
        "F_Cu must have exactly 2 RoundRect D03 flashes for the 2 pads of R1"
    );
}

// =============================================================================
// Contract D: New footprints
// =============================================================================

#[tokio::test]
async fn test_new_footprints() {
    let board_json = r#"{
      "board": {"width": 120, "height": 120, "layers": 2},
      "rules": {"trace": 0.2, "clearance": 0.2, "via": [0.6, 0.3], "edge": 0.3},
      "parts": [
        {"ref": "U1", "kind": "chip", "footprint": "to220", "at": [-35, -35]},
        {"ref": "U2", "kind": "chip", "footprint": "to92", "at": [0, -35]},
        {"ref": "U3", "kind": "chip", "footprint": "sot23_6", "at": [35, -35]},
        {"ref": "U4", "kind": "chip", "footprint": "sot89", "at": [-35, 0]},
        {"ref": "D1", "kind": "diode", "footprint": "sod323", "at": [0, 0], "rotation": 0},
        {"ref": "D2", "kind": "diode", "footprint": "sma", "at": [35, 0], "rotation": 0},
        {"ref": "D3", "kind": "diode", "footprint": "smb", "at": [-35, 30], "rotation": 0},
        {"ref": "D4", "kind": "diode", "footprint": "smc", "at": [0, 30], "rotation": 0},
        {"ref": "U5", "kind": "chip", "footprint": "msop8", "at": [35, 30]},
        {"ref": "U6", "kind": "chip", "footprint": "msop10", "at": [-25, -15]},
        {"ref": "J1", "kind": "header", "footprint": "microsd", "at": [20, -15]}
      ]
    }"#;
    let dir = project("new_fp", &[("pcb/fp.pcb.json", board_json)]);
    run("pcb_build", json!({"source": "pcb/fp.pcb.json"}), &dir.path)
        .await
        .unwrap();

    let circuit_raw = std::fs::read_to_string(dir.path.join("pcb/fp.circuit.json")).unwrap();
    let circuit: Value = serde_json::from_str(&circuit_raw).unwrap();
    let elements = circuit.as_array().unwrap();

    let get_pads_for_ref = |r: &str| -> Vec<&Value> {
        let prefix = format!("{r}.");
        elements
            .iter()
            .filter(|e| {
                e.get("pcb_smtpad_id")
                    .or_else(|| e.get("pcb_plated_hole_id"))
                    .or_else(|| e.get("pcb_hole_id"))
                    .and_then(|id| id.as_str())
                    .map_or(false, |id| id.starts_with(&prefix))
            })
            .collect()
    };

    // Pad counts:
    // to220: 3
    assert_eq!(get_pads_for_ref("U1").len(), 3, "to220 pad count");
    // to92: 3
    assert_eq!(get_pads_for_ref("U2").len(), 3, "to92 pad count");
    // sot23_6: 6
    assert_eq!(get_pads_for_ref("U3").len(), 6, "sot23_6 pad count");
    // sot89: 4 (tab pad numbered "2" like the middle pin, so two pads named 2)
    let sot89_pads = get_pads_for_ref("U4");
    assert_eq!(sot89_pads.len(), 4, "sot89 pad count");
    let sot89_pad2_count = sot89_pads
        .iter()
        .filter(|p| p["pcb_smtpad_id"].as_str() == Some("U4.2"))
        .count();
    assert_eq!(sot89_pad2_count, 2, "sot89 must have two pads named 2");
    // sod323: 2
    assert_eq!(get_pads_for_ref("D1").len(), 2, "sod323 pad count");
    // sma: 2
    assert_eq!(get_pads_for_ref("D2").len(), 2, "sma pad count");
    // smb: 2
    assert_eq!(get_pads_for_ref("D3").len(), 2, "smb pad count");
    // smc: 2
    assert_eq!(get_pads_for_ref("D4").len(), 2, "smc pad count");
    // msop8: 8
    assert_eq!(get_pads_for_ref("U5").len(), 8, "msop8 pad count");
    // msop10: 10
    assert_eq!(get_pads_for_ref("U6").len(), 10, "msop10 pad count");
    // microsd: count from KiCad file Connector_Card.pretty/microSD_HC_Hirose_DM3AT-SF-PEJM5.kicad_mod = 14
    assert_eq!(get_pads_for_ref("J1").len(), 14, "microsd pad count");

    // Pad sizes matching KiCad literals within 1e-3 mm:
    // 1. to220: Package_TO_SOT_THT.pretty/TO-220-3_Vertical.kicad_mod
    // Pad 2 size: [1.905, 2.0]
    let u1_p2 = get_pads_for_ref("U1")
        .into_iter()
        .find(|p| p["pcb_plated_hole_id"] == "U1.2")
        .expect("U1.2");
    let u1_w = u1_p2["outer_width"].as_f64().unwrap();
    let u1_h = u1_p2["outer_height"].as_f64().unwrap();
    assert!(
        (u1_w - 1.905).abs() < 1e-3 && (u1_h - 2.0).abs() < 1e-3,
        "to220 pad 2 size"
    );

    // 2. to92: Package_TO_SOT_THT.pretty/TO-92_Inline.kicad_mod
    // Pad 2 size: [1.05, 1.5]
    let u2_p2 = get_pads_for_ref("U2")
        .into_iter()
        .find(|p| p["pcb_plated_hole_id"] == "U2.2")
        .expect("U2.2");
    let u2_w = u2_p2["outer_width"].as_f64().unwrap();
    let u2_h = u2_p2["outer_height"].as_f64().unwrap();
    assert!(
        (u2_w - 1.05).abs() < 1e-3 && (u2_h - 1.5).abs() < 1e-3,
        "to92 pad 2 size"
    );

    // 3. sot23_6: Package_TO_SOT_SMD.pretty/SOT-23-6.kicad_mod
    // Pad 1 size: [1.325, 0.6]
    let u3_p1 = get_pads_for_ref("U3")
        .into_iter()
        .find(|p| p["pcb_smtpad_id"] == "U3.1")
        .expect("U3.1");
    assert!((u3_p1["width"].as_f64().unwrap() - 1.325).abs() < 1e-3);
    assert!((u3_p1["height"].as_f64().unwrap() - 0.6).abs() < 1e-3);

    // 4. sot89: Package_TO_SOT_SMD.pretty/SOT-89-3.kicad_mod
    // Pad 1 size: [1.3, 0.9]
    let u4_p1 = get_pads_for_ref("U4")
        .into_iter()
        .find(|p| p["pcb_smtpad_id"] == "U4.1")
        .expect("U4.1");
    assert!((u4_p1["width"].as_f64().unwrap() - 1.3).abs() < 1e-3);
    assert!((u4_p1["height"].as_f64().unwrap() - 0.9).abs() < 1e-3);

    // 5. sod323: Diode_SMD.pretty/D_SOD-323.kicad_mod
    // KiCad pad 1 size: [0.6, 0.45] at x = -1.05
    // Forge maps K -> pad 2, so Forge pad 2 corresponds to KiCad pad 1
    let d1_p2 = get_pads_for_ref("D1")
        .into_iter()
        .find(|p| p["pcb_smtpad_id"] == "D1.2")
        .expect("D1.2");
    assert!((d1_p2["width"].as_f64().unwrap() - 0.6).abs() < 1e-3);
    assert!((d1_p2["height"].as_f64().unwrap() - 0.45).abs() < 1e-3);
    // Diode pad 2 x has sign of KiCad pad 1 x (negative)
    assert!(
        d1_p2["x"].as_f64().unwrap() - 0.0 < 0.0,
        "D1.2 x must be negative"
    );

    // 6. sma: Diode_SMD.pretty/D_SMA.kicad_mod
    // KiCad pad 1 size: [2.5, 1.8] at x = -2.0
    let d2_p2 = get_pads_for_ref("D2")
        .into_iter()
        .find(|p| p["pcb_smtpad_id"] == "D2.2")
        .expect("D2.2");
    assert!((d2_p2["width"].as_f64().unwrap() - 2.5).abs() < 1e-3);
    assert!((d2_p2["height"].as_f64().unwrap() - 1.8).abs() < 1e-3);
    assert!(
        d2_p2["x"].as_f64().unwrap() - 35.0 < 0.0,
        "D2.2 x relative to center must be negative"
    );

    // 7. smb: Diode_SMD.pretty/D_SMB.kicad_mod
    // KiCad pad 1 size: [2.5, 2.3] at x = -2.15
    let d3_p2 = get_pads_for_ref("D3")
        .into_iter()
        .find(|p| p["pcb_smtpad_id"] == "D3.2")
        .expect("D3.2");
    assert!((d3_p2["width"].as_f64().unwrap() - 2.5).abs() < 1e-3);
    assert!((d3_p2["height"].as_f64().unwrap() - 2.3).abs() < 1e-3);
    assert!(
        d3_p2["x"].as_f64().unwrap() - (-35.0) < 0.0,
        "D3.2 x relative to center must be negative"
    );

    // 8. smc: Diode_SMD.pretty/D_SMC.kicad_mod
    // KiCad pad 1 size: [3.3, 2.5] rotated 90 deg -> unrotated width 2.5, height 3.3 at x = -3.4
    let d4_p2 = get_pads_for_ref("D4")
        .into_iter()
        .find(|p| p["pcb_smtpad_id"] == "D4.2")
        .expect("D4.2");
    assert!((d4_p2["width"].as_f64().unwrap() - 2.5).abs() < 1e-3);
    assert!((d4_p2["height"].as_f64().unwrap() - 3.3).abs() < 1e-3);
    assert!(
        d4_p2["x"].as_f64().unwrap() - 0.0 < 0.0,
        "D4.2 x relative to center must be negative"
    );

    // 9. msop8: Package_SO.pretty/MSOP-8_3x3mm_P0.65mm.kicad_mod
    // Pad 1 size: [1.625, 0.4]
    let u5_p1 = get_pads_for_ref("U5")
        .into_iter()
        .find(|p| p["pcb_smtpad_id"] == "U5.1")
        .expect("U5.1");
    assert!((u5_p1["width"].as_f64().unwrap() - 1.625).abs() < 1e-3);
    assert!((u5_p1["height"].as_f64().unwrap() - 0.4).abs() < 1e-3);

    // 10. msop10: Package_SO.pretty/MSOP-10_3x3mm_P0.5mm.kicad_mod
    // Pad 1 size: [1.5, 0.35]
    let u6_p1 = get_pads_for_ref("U6")
        .into_iter()
        .find(|p| p["pcb_smtpad_id"] == "U6.1")
        .expect("U6.1");
    assert!((u6_p1["width"].as_f64().unwrap() - 1.5).abs() < 1e-3);
    assert!((u6_p1["height"].as_f64().unwrap() - 0.35).abs() < 1e-3);

    // 11. microsd: Connector_Card.pretty/microSD_HC_Hirose_DM3AT-SF-PEJM5.kicad_mod
    // Pad 1 size: [0.7, 1.2]
    let j1_p1 = get_pads_for_ref("J1")
        .into_iter()
        .find(|p| p["pcb_smtpad_id"] == "J1.1")
        .expect("J1.1");
    assert!((j1_p1["width"].as_f64().unwrap() - 0.7).abs() < 1e-3);
    assert!((j1_p1["height"].as_f64().unwrap() - 1.2).abs() < 1e-3);

    // Check unknown footprint error message
    let bad_board = r#"{
      "board": {"width": 20, "height": 20},
      "parts": [{"ref": "X1", "kind": "chip", "footprint": "nonexistent_fp"}]
    }"#;
    let err_dir = project("bad_fp", &[("pcb/bad.pcb.json", bad_board)]);
    let err = run(
        "pcb_build",
        json!({"source": "pcb/bad.pcb.json"}),
        &err_dir.path,
    )
    .await
    .unwrap_err();
    let err_msg = err.to_string();
    assert!(
        err_msg.contains("unknown footprint nonexistent_fp; use one of:"),
        "Error message must start with expected prefix: {err_msg}"
    );
    for name in [
        "to220", "to92", "sot23_6", "sot89", "sod323", "sma", "smb", "smc", "msop8", "msop10",
        "microsd",
    ] {
        assert!(
            err_msg.contains(name),
            "Unknown footprint list must contain {name}: {err_msg}"
        );
    }
}

// =============================================================================
// Contract E: Regression
// =============================================================================

#[tokio::test]
async fn test_regression_555_blinker() {
    let dir = project("reg_555", &[("pcb/b555.pcb.json", BLINKER_555_BOARD_JSON)]);
    let build_res = run(
        "pcb_build",
        json!({"source": "pcb/b555.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert!(
        build_res
            .text
            .contains("No errors: every connection is routed."),
        "build text missing routed message: {}",
        build_res.text
    );
    let drc_res = run("pcb_drc", json!({"board": "b555"}), &dir.path)
        .await
        .unwrap();
    assert_eq!(drc_res.text.trim(), "No DRC errors.");
}
