use super::{footprint, place, Footprint, PlacedPart};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub board: Board,
    #[serde(default)]
    pub rules: Rules,
    #[serde(default)]
    pub netclasses: BTreeMap<String, NetClass>,
    #[serde(default)]
    pub parts: Vec<Part>,
    #[serde(default)]
    pub nets: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub holes: Vec<Hole>,
    #[serde(default)]
    pub pours: Vec<Pour>,
    #[serde(default)]
    pub keepouts: Vec<Keepout>,
    #[serde(default)]
    pub traces: Vec<Trace>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Board {
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub outline: Option<Vec<[f64; 2]>>,
    #[serde(default = "two")]
    pub layers: usize,
    #[serde(default = "thickness")]
    pub thickness: f64,
}
fn two() -> usize {
    2
}
fn thickness() -> f64 {
    1.6
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Rules {
    pub trace: f64,
    pub clearance: f64,
    pub via: [f64; 2],
    pub edge: f64,
}
impl Default for Rules {
    fn default() -> Self {
        Self {
            trace: 0.2,
            clearance: 0.2,
            via: [0.6, 0.3],
            edge: 0.3,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetClass {
    pub nets: Vec<String>,
    pub trace: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Resistor,
    Capacitor,
    Inductor,
    Led,
    Diode,
    Crystal,
    Button,
    Switch,
    Chip,
    Header,
    Antenna,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Layer {
    #[default]
    Top,
    Bottom,
    Inner1,
    Inner2,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    #[serde(rename = "ref")]
    pub reference: String,
    pub kind: Kind,
    pub value: Option<String>,
    pub footprint: Option<String>,
    pub at: Option<[f64; 2]>,
    #[serde(default)]
    pub rotation: f64,
    #[serde(default)]
    pub side: Layer,
    #[serde(default)]
    pub pins: BTreeMap<String, String>,
    pub lcsc: Option<String>,
    pub shape: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hole {
    pub at: [f64; 2],
    pub d: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pour {
    pub net: String,
    pub layer: Layer,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keepout {
    pub rect: [f64; 4],
    pub layers: Vec<Layer>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trace {
    pub net: String,
    pub width: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pin {
    pub part: usize,
    pub number: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Net {
    pub name: String,
    pub pins: Vec<Pin>,
    pub trace: f64,
}

#[derive(Clone, Debug)]
pub struct PreparedBoard {
    pub spec: Spec,
    pub parts: Vec<PlacedPart>,
    pub nets: Vec<Net>,
}

fn positive(value: f64, field: &str) -> Result<(), String> {
    if !value.is_finite() || value <= 0.0 {
        return Err(format!("{field} must be positive"));
    }
    Ok(())
}
fn nonnegative(value: f64, field: &str) -> Result<(), String> {
    if !value.is_finite() || value < 0.0 {
        return Err(format!("{field} must be nonnegative"));
    }
    Ok(())
}
fn point(at: [f64; 2]) -> Result<(), String> {
    if at.iter().any(|x| !x.is_finite()) {
        return Err("coordinates must be finite".into());
    }
    Ok(())
}
fn net_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.chars().any(char::is_control) {
        return Err("net names must be nonempty and contain no control characters".into());
    }
    Ok(())
}

impl Board {
    pub fn polygon(&self) -> Vec<[f64; 2]> {
        let mut polygon = self.outline.clone().unwrap_or_else(|| {
            let w = self.width.unwrap_or_default() / 2.0;
            let h = self.height.unwrap_or_default() / 2.0;
            vec![[-w, -h], [w, -h], [w, h], [-w, h]]
        });
        if polygon.len() > 1 && polygon.first() == polygon.last() {
            polygon.pop();
        }
        polygon
    }

    pub fn bounds(&self) -> [f64; 4] {
        self.polygon().iter().fold(
            [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ],
            |mut b, p| {
                for i in 0..2 {
                    b[i] = b[i].min(p[i]);
                    b[i + 2] = b[i + 2].max(p[i]);
                }
                b
            },
        )
    }
}

impl Spec {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.board.layers, 2 | 4) {
            return Err("board.layers must be 2 or 4".into());
        }
        positive(self.board.thickness, "board.thickness")?;
        if let Some(w) = self.board.width {
            positive(w, "board.width")?;
        }
        if let Some(h) = self.board.height {
            positive(h, "board.height")?;
        }
        if let Some(outline) = &self.board.outline {
            super::place::validate_outline(outline)?;
        } else if self.board.width.is_none() || self.board.height.is_none() {
            return Err("board needs width and height or outline".into());
        }
        positive(self.rules.trace, "rules.trace")?;
        nonnegative(self.rules.clearance, "rules.clearance")?;
        nonnegative(self.rules.edge, "rules.edge")?;
        positive(self.rules.via[0], "rules.via diameter")?;
        positive(self.rules.via[1], "rules.via drill")?;
        if self.rules.via[1] >= self.rules.via[0] {
            return Err("rules.via drill must be smaller than diameter".into());
        }
        let mut refs = BTreeSet::new();
        for p in &self.parts {
            if !super::super::script3d::valid_name(&p.reference) {
                return Err("name may only use letters, digits, - and _".into());
            }
            if !refs.insert(&p.reference) {
                return Err(format!("duplicate part {}", p.reference));
            }
            if let Some(at) = p.at {
                point(at)?;
            }
            if !p.rotation.is_finite() {
                return Err(format!("{} rotation must be finite", p.reference));
            }
            if !matches!(p.side, Layer::Top | Layer::Bottom) {
                return Err(format!("{} side must be top or bottom", p.reference));
            }
            if p.kind == Kind::Antenna {
                if p.footprint.is_some() {
                    return Err(format!(
                        "{}: antenna takes shape, not footprint",
                        p.reference
                    ));
                }
                if !matches!(
                    p.shape.as_deref(),
                    Some(
                        "2.4ghz_quarter_wave_monopole"
                            | "2.4ghz_meandered_monopole"
                            | "2.4ghz_inverted_f"
                            | "2.4ghz_meandered_inverted_f"
                            | "2.4ghz_folded_dipole"
                    )
                ) {
                    return Err(format!("{}: unknown antenna shape", p.reference));
                }
            } else {
                if p.shape.is_some() {
                    return Err(format!("{}: shape is only for antennas", p.reference));
                }
                footprint(
                    p.footprint
                        .as_deref()
                        .ok_or_else(|| format!("{} needs a footprint", p.reference))?,
                )?;
            }
        }
        for h in &self.holes {
            point(h.at)?;
            positive(h.d, "hole diameter")?;
        }
        for k in &self.keepouts {
            point([k.rect[0], k.rect[1]])?;
            positive(k.rect[2], "keepout width")?;
            positive(k.rect[3], "keepout height")?;
            if k.layers.is_empty() {
                return Err("keepout.layers must not be empty".into());
            }
            for &layer in &k.layers {
                self.validate_layer(layer)?;
            }
        }
        for p in &self.pours {
            net_name(&p.net)?;
            self.validate_layer(p.layer)?;
        }
        let mut classes = BTreeSet::new();
        for (name, class) in &self.netclasses {
            net_name(name)?;
            positive(class.trace, "netclass.trace")?;
            for net in &class.nets {
                net_name(net)?;
                if !classes.insert(net) {
                    return Err(format!("net {net} belongs to more than one netclass"));
                }
            }
        }
        let mut traces = BTreeSet::new();
        for t in &self.traces {
            net_name(&t.net)?;
            positive(t.width, "trace width")?;
            if !traces.insert(&t.net) {
                return Err(format!("duplicate trace width for net {}", t.net));
            }
        }
        Ok(())
    }

    fn validate_layer(&self, layer: Layer) -> Result<(), String> {
        if self.board.layers == 2 && matches!(layer, Layer::Inner1 | Layer::Inner2) {
            return Err("inner layers need a 4-layer board".into());
        }
        Ok(())
    }

    pub fn footprints(&self) -> Result<Vec<Footprint>, String> {
        self.parts
            .iter()
            .map(|p| {
                if p.kind != Kind::Antenna {
                    return footprint(p.footprint.as_deref().unwrap_or(""));
                }
                Ok(super::antenna::geometry(p.shape.as_deref().unwrap_or(""))?.footprint())
            })
            .collect()
    }

    /// Explicit nets win over chip labels; unconnected labels join their named net.
    pub fn netlist(&self, footprints: &[Footprint]) -> Result<Vec<Net>, String> {
        if footprints.len() != self.parts.len() {
            return Err("one footprint is required per part".into());
        }
        let mut pins: BTreeMap<String, BTreeSet<Pin>> = BTreeMap::new();
        let mut owners: BTreeMap<Pin, String> = BTreeMap::new();
        for (name, endpoints) in &self.nets {
            net_name(name)?;
            pins.entry(name.clone()).or_default();
            for endpoint in endpoints {
                let (reference, number) = endpoint
                    .rsplit_once('.')
                    .ok_or_else(|| format!("invalid pin {endpoint}; use <ref>.<pin>"))?;
                let index = self
                    .parts
                    .iter()
                    .position(|p| p.reference == reference)
                    .ok_or_else(|| format!("unknown part {reference}"))?;
                for number in resolve_pins(&self.parts[index], &footprints[index], number)? {
                    let pin = Pin {
                        part: index,
                        number,
                    };
                    if let Some(previous) = owners.insert(pin.clone(), name.clone()) {
                        if previous != *name {
                            return Err(format!(
                                "{endpoint} belongs to nets {previous} and {name}"
                            ));
                        }
                    }
                    pins.get_mut(name).unwrap().insert(pin);
                }
            }
        }
        for (index, part) in self.parts.iter().enumerate() {
            for (number, label) in &part.pins {
                net_name(label)?;
                if !footprints[index].pads.iter().any(|p| p.number == *number) {
                    return Err(pin_error(part, &footprints[index], number));
                }
                let pin = Pin {
                    part: index,
                    number: number.clone(),
                };
                if !owners.contains_key(&pin) {
                    pins.entry(label.clone()).or_default().insert(pin);
                }
            }
        }
        Ok(pins
            .into_iter()
            .map(|(name, pins)| {
                let trace = self
                    .traces
                    .iter()
                    .find(|t| t.net == name)
                    .map(|t| t.width)
                    .or_else(|| {
                        self.netclasses
                            .values()
                            .find(|c| c.nets.contains(&name))
                            .map(|c| c.trace)
                    })
                    .unwrap_or(self.rules.trace);
                Net {
                    name,
                    pins: pins.into_iter().collect(),
                    trace,
                }
            })
            .collect())
    }
}

fn pin_error(part: &Part, fp: &Footprint, pin: &str) -> String {
    let mut numbers: Vec<_> = fp.pads.iter().map(|p| p.number.clone()).collect();
    numbers.sort_by_key(|s| s.parse::<usize>().unwrap_or(usize::MAX));
    numbers.dedup();
    format!(
        "{}.{pin}: {} ({}) has pins {}",
        part.reference,
        part.reference,
        part.footprint
            .as_deref()
            .or(part.shape.as_deref())
            .unwrap_or(""),
        numbers.join(", ")
    )
}

fn resolve_pins(part: &Part, fp: &Footprint, name: &str) -> Result<Vec<String>, String> {
    let alias = match (part.kind, name) {
        (Kind::Led | Kind::Diode, "A") => Some("1"),
        (Kind::Led | Kind::Diode, "K") => Some("2"),
        _ => None,
    };
    let numbers = if fp.pads.iter().any(|p| p.number == name) {
        vec![name.to_owned()]
    } else if let Some(number) = alias {
        vec![number.to_owned()]
    } else {
        part.pins
            .iter()
            .filter(|(_, label)| *label == name)
            .map(|(number, _)| number.clone())
            .collect()
    };
    if numbers.is_empty()
        || numbers
            .iter()
            .any(|n| !fp.pads.iter().any(|p| p.number == *n))
    {
        return Err(pin_error(part, fp, name));
    }
    Ok(numbers)
}

pub fn parse(json: &str) -> Result<Spec, String> {
    let spec: Spec = serde_json::from_str(json).map_err(|e| e.to_string())?;
    spec.validate()?;
    Ok(spec)
}

pub fn prepare(json: &str) -> Result<PreparedBoard, String> {
    let spec = parse(json)?;
    let footprints = spec.footprints()?;
    let nets = spec.netlist(&footprints)?;
    let parts = place(&spec, &footprints, &nets)?;
    Ok(PreparedBoard { spec, parts, nets })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn example() -> serde_json::Value {
        json!({"board":{"width":30,"height":20},"parts":[
            {"ref":"U1","kind":"chip","footprint":"soic8","pins":{"1":"VCC","8":"GND"}},
            {"ref":"LED1","kind":"led","footprint":"0603"}],"nets":{"GND":["LED1.K"],"A":["LED1.A","U1.VCC"]}})
    }

    #[test]
    fn strict_fields_dimensions_and_net_ownership() {
        let mut input = example();
        input["board"]["typo"] = json!(3);
        assert!(parse(&input.to_string())
            .unwrap_err()
            .contains("unknown field"));
        input = example();
        input["rules"] = json!({"via":[0.3,0.6]});
        assert!(parse(&input.to_string()).is_err());
        input = example();
        input["board"]["width"] = json!(0);
        assert!(parse(&input.to_string()).is_err());
        input = example();
        input["nets"]["SHORT"] = json!(["LED1.1"]);
        assert!(prepare(&input.to_string())
            .unwrap_err()
            .contains("belongs to nets"));
        input = example();
        input["parts"][0]["pins"]["9"] = json!("BAD");
        assert!(prepare(&input.to_string())
            .unwrap_err()
            .contains("U1.9: U1 (soic8) has pins"));
    }

    #[test]
    fn aliases_labels_dedup_and_trace_overrides() {
        let mut input = example();
        input["nets"]["GND"] = json!(["LED1.K", "LED1.2"]);
        input["netclasses"] = json!({"power":{"nets":["GND"],"trace":0.4}});
        input["traces"] = json!([{"net":"A","width":0.5}]);
        let board = prepare(&input.to_string()).unwrap();
        assert_eq!(
            board
                .nets
                .iter()
                .map(|n| n.name.as_str())
                .collect::<Vec<_>>(),
            ["A", "GND"]
        );
        assert_eq!(board.nets[0].trace, 0.5);
        assert_eq!(board.nets[1].trace, 0.4);
        assert_eq!(board.nets[1].pins.len(), 2);
        assert_eq!(
            board.nets[1].pins[0],
            Pin {
                part: 0,
                number: "8".into()
            }
        );
        input["parts"][0]["pins"]["7"] = json!("GND");
        input["nets"]["GND"] = json!(["LED1.K", "U1.GND"]);
        let board = prepare(&input.to_string()).unwrap();
        assert_eq!(board.nets[1].pins.len(), 3);
    }
}
