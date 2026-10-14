use super::devices::Model;
use std::collections::{BTreeMap, BTreeSet};

pub(super) const NONE: usize = usize::MAX;

#[derive(Clone)]
pub(super) struct Options {
    pub reltol: f64, pub abstol: f64, pub vntol: f64, pub gmin: f64,
    pub itl1: usize, pub itl4: usize, pub temp: f64, pub tnom: f64,
}
impl Default for Options {
    fn default() -> Self { Self { reltol: 1e-3, abstol: 1e-12, vntol: 1e-6, gmin: 1e-12, itl1: 100, itl4: 10, temp: 27., tnom: 27. } }
}

#[derive(Clone)]
pub(super) enum Waveform { Pulse(Vec<f64>), Sin(Vec<f64>), Pwl(Vec<(f64, f64)>) }
#[derive(Clone)]
pub(super) struct Source { pub dc: Option<f64>, pub wave: Option<Waveform>, pub ac: Option<(f64, f64)> }
impl Source {
    pub fn value(&self, time: Option<f64>, step: f64, stop: f64) -> f64 {
        if time.is_none() { return self.dc.unwrap_or_else(|| self.value(Some(0.), step, stop)); }
        let t = time.unwrap();
        match &self.wave {
            None => self.dc.unwrap_or(0.),
            Some(Waveform::Pulse(p)) => {
                let [v1, v2, td, tr, tf, pw, per] = pulse(p, step, stop);
                if t < td { return v1; }
                let t = t - td;
                let t = if t > per { t.rem_euclid(per) } else { t };
                if t < tr { v1 + (v2 - v1) * t / tr }
                else if t < tr + pw { v2 }
                else if t < tr + pw + tf { v2 + (v1 - v2) * (t - tr - pw) / tf }
                else { v1 }
            }
            Some(Waveform::Sin(p)) => {
                let td = p.get(3).copied().unwrap_or(0.);
                let t = (t - td).max(0.);
                p[0] + p[1] * (std::f64::consts::TAU * p.get(2).copied().filter(|v| *v > 0.).unwrap_or(1. / stop) * t).sin()
                    * (-p.get(4).copied().unwrap_or(0.) * t).exp()
            }
            Some(Waveform::Pwl(p)) => {
                let exact = p.partition_point(|(x, _)| *x < t);
                if exact < p.len() && p[exact].0 == t { return p[exact].1; }
                let i = p.partition_point(|(x, _)| *x <= t);
                if i == 0 { p[0].1 } else if i == p.len() { p[i - 1].1 }
                else { let (a, va) = p[i - 1]; let (b, vb) = p[i]; va + (vb - va) * (t - a) / (b - a) }
            }
        }
    }
    pub fn breakpoint(&self, t: f64, step: f64, stop: f64) -> f64 {
        let mut next = stop;
        let mut consider = |x: f64| { if x > t + stop * 1e-14 && x < next { next = x; } };
        match &self.wave {
            Some(Waveform::Pulse(p)) => {
                let [_, _, td, tr, tf, pw, per] = pulse(p, step, stop);
                let cycle = ((t - td) / per).floor().max(0.);
                for k in [cycle, cycle + 1.] {
                    for offset in [0., tr, tr + pw, tr + pw + tf, per] { consider(td + k * per + offset); }
                }
            }
            Some(Waveform::Pwl(p)) => for (x, _) in p { consider(*x); },
            Some(Waveform::Sin(p)) => consider(p.get(3).copied().unwrap_or(0.)),
            None => {}
        }
        next
    }
}
fn pulse(p: &[f64], step: f64, stop: f64) -> [f64; 7] {
    // SPICE treats omitted or zero rise/fall times as TSTEP, not ideal discontinuities.
    let positive = |i: usize, default: f64| p.get(i).copied().filter(|v| *v > 0.).unwrap_or(default);
    [p[0], p[1], p.get(2).copied().unwrap_or(0.), positive(3, step), positive(4, step), positive(5, stop), positive(6, stop)]
}

#[derive(Clone)]
pub(super) enum Kind {
    R(f64), C(f64, Option<f64>), L(f64, Option<f64>), V(Source), I(Source), E(f64), G(f64),
    Device(Model),
}
#[derive(Clone)]
pub(super) struct Element { pub name: String, pub nodes: [usize; 4], pub kind: Kind, pub branch: usize, pub tc: [f64; 2] }
impl Element {
    pub fn voltages(&self, x: &[f64]) -> [f64; 4] { self.nodes.map(|n| if n == 0 { 0. } else { x[n - 1] }) }
}
#[derive(Clone)]
pub(super) struct Tran { pub step: f64, pub stop: f64, pub start: f64, pub max: f64, pub uic: bool }
#[derive(Clone)]
pub(super) struct Sweep { pub frequencies: Vec<f64> }
#[derive(Clone)]
pub(super) struct Noise { pub output: [usize; 2], pub source: usize, pub sweep: Sweep }
#[derive(Clone)]
pub(super) enum Analysis { Op, Dc { source: usize, start: f64, stop: f64, step: f64 }, Tran(Tran), Ac(Sweep), Noise(Noise) }
#[derive(Clone)]
pub(super) struct Circuit {
    pub title: String, pub nodes: Vec<String>, pub visible: Vec<bool>, pub elements: Vec<Element>,
    pub analyses: Vec<Analysis>, pub options: Options, pub ic: BTreeMap<usize, f64>,
    pub traces: BTreeMap<String, Vec<String>>, pub size: usize, pub temperatures: Vec<f64>,
}
impl Circuit {
    pub fn at_temperature(&self, temp: f64) -> Result<Self, String> {
        let mut c = self.clone();
        c.options.temp = temp;
        for e in &mut c.elements {
            match &mut e.kind {
                Kind::R(r) => {
                    let delta = temp - c.options.tnom;
                    *r *= 1. + e.tc[0] * delta + e.tc[1] * delta * delta;
                    if !r.is_finite() || *r <= 0. { return Err(format!("{} must be positive", e.name)); }
                }
                Kind::Device(model) => model.temperature(temp, c.options.tnom)?,
                _ => {}
            }
        }
        Ok(c)
    }
}

#[derive(Clone)]
struct Line { n: usize, words: Vec<String> }
struct Scope { lines: Vec<Line>, models: BTreeMap<String, Model>, subs: BTreeMap<String, Subcircuit> }
struct Subcircuit { ports: Vec<String>, scope: Scope }
fn error(line: usize, text: impl AsRef<str>) -> String { format!("line {line}: {}", text.as_ref()) }

pub(super) fn number(text: &str, line: usize) -> Result<f64, String> {
    let s = text.to_ascii_lowercase();
    let bytes = s.as_bytes();
    let mut i = usize::from(bytes.first().is_some_and(|b| matches!(b, b'+' | b'-')));
    let mut digits = 0;
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
        digits += usize::from(bytes[i].is_ascii_digit()); i += 1;
    }
    if digits == 0 { return Err(error(line, format!("bad number {text}"))); }
    if i < bytes.len() && bytes[i] == b'e' {
        let exponent = i;
        i += 1;
        if i < bytes.len() && matches!(bytes[i], b'+' | b'-') { i += 1; }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
        if start == i { i = exponent; }
    }
    let value = s[..i].parse::<f64>().map_err(|_| error(line, format!("bad number {text}")))?;
    let suffix = &s[i..];
    let scale = if suffix.starts_with("meg") { 1e6 } else if suffix.starts_with("mil") { 25.4e-6 }
        else { match suffix.as_bytes().first() { Some(b'f') => 1e-15, Some(b'p') => 1e-12, Some(b'n') => 1e-9,
            Some(b'u') => 1e-6, Some(b'm') => 1e-3, Some(b'k') => 1e3, Some(b'g') => 1e9, Some(b't') => 1e12, _ => 1. } };
    let result = value * scale;
    if !result.is_finite() { return Err(error(line, format!("bad number {text}"))); }
    Ok(result)
}
fn parameters(words: &[String], line: usize) -> Result<BTreeMap<String, f64>, String> {
    let mut params = BTreeMap::new();
    let mut i = 0;
    while i < words.len() {
        let key = words[i].to_ascii_lowercase();
        i += 1;
        if words.get(i).is_some_and(|s| s == "=") { i += 1; }
        let text = words.get(i).ok_or_else(|| error(line, format!("bad number {key}")))?;
        let value = if ["vaf", "var", "ikf"].contains(&key.as_str()) && text.eq_ignore_ascii_case("inf") { f64::INFINITY }
            else { number(text, line)? };
        params.insert(key, value); i += 1;
    }
    Ok(params)
}
fn scope(lines: &[Line], index: &mut usize, nested: bool, depth: usize) -> Result<Scope, String> {
    // shortcut: 32 definition/instance levels, use an explicit stack for deeper hierarchies.
    if depth > 32 { return Err("subcircuit nesting too deep (limit 32)".into()); }
    let mut result = Scope { lines: vec![], models: BTreeMap::new(), subs: BTreeMap::new() };
    while let Some(line) = lines.get(*index) {
        *index += 1;
        let w = &line.words;
        match w[0].to_ascii_lowercase().as_str() {
            ".end" => break,
            ".ends" if nested => return Ok(result),
            ".ends" => return Err(error(line.n, "unexpected .ends")),
            ".subckt" => {
                if w.len() < 2 { return Err(error(line.n, "missing subcircuit name")); }
                let sub = Subcircuit { ports: w[2..].iter().map(|s| s.to_ascii_lowercase()).collect(), scope: scope(lines, index, true, depth + 1)? };
                if result.subs.insert(w[1].to_ascii_lowercase(), sub).is_some() { return Err(error(line.n, "duplicate subcircuit")); }
            }
            ".model" => {
                if w.len() < 3 { return Err(error(line.n, "missing model name or type")); }
                let params = parameters(&w[3..], line.n)?;
                let model = Model::parse(&w[2], params).map_err(|e| error(line.n, e))?;
                if result.models.insert(w[1].to_ascii_lowercase(), model).is_some() { return Err(error(line.n, "duplicate model")); }
            }
            _ => result.lines.push(line.clone()),
        }
    }
    if nested { return Err("missing .ends".into()); }
    Ok(result)
}

struct Builder {
    circuit: Circuit, node_ids: BTreeMap<String, usize>, names: BTreeSet<String>,
    directives: Vec<Line>, initials: Vec<(String, f64)>,
}
impl Builder {
    fn node(&mut self, name: &str, visible: bool) -> usize {
        let name = name.to_ascii_lowercase();
        if name == "0" || name == "gnd" { return 0; }
        if let Some(&id) = self.node_ids.get(&name) { return id; }
        let id = self.circuit.nodes.len();
        self.circuit.nodes.push(name.clone()); self.circuit.visible.push(visible);
        self.node_ids.insert(name, id); id
    }
    fn add(&mut self, name: String, nodes: [usize; 4], kind: Kind, line: usize) -> Result<(), String> {
        if !self.names.insert(name.to_ascii_lowercase()) { return Err(error(line, format!("duplicate element {name}"))); }
        // shortcut: 20000 flattened elements, raise with resource budgeting for larger decks.
        if self.circuit.elements.len() >= 20000 { return Err("too many elements (limit 20000)".into()); }
        self.circuit.elements.push(Element { name, nodes, kind, branch: NONE, tc: [0.; 2] }); Ok(())
    }
    fn expand(&mut self, current: &Scope, parents: &[&Scope], prefix: &str, ports: &BTreeMap<String, String>, depth: usize) -> Result<(), String> {
        // shortcut: 32 definition/instance levels, use an explicit stack for deeper hierarchies.
        if depth > 32 { return Err("subcircuit nesting too deep (limit 32)".into()); }
        let mut scopes = parents.to_vec(); scopes.push(current);
        let mapped = |s: &str| {
            let lower = s.to_ascii_lowercase();
            if lower == "0" || lower == "gnd" { "0".to_owned() }
            else { ports.get(&lower).cloned().unwrap_or_else(|| format!("{prefix}{lower}")) }
        };
        for line in &current.lines {
            let w = &line.words;
            let first = w[0].to_ascii_lowercase();
            if first.starts_with('.') {
                if first == ".ic" {
                    let mut i = 1;
                    while i < w.len() {
                        if !w[i].eq_ignore_ascii_case("v") || w.get(i + 2).map(String::as_str) != Some("=") || i + 3 >= w.len() {
                            return Err(error(line.n, "expected .ic v(node)=value"));
                        }
                        self.initials.push((mapped(&w[i + 1]), number(&w[i + 3], line.n)?)); i += 4;
                    }
                } else if prefix.is_empty() { self.directives.push(line.clone()); }
                else { return Err(error(line.n, format!("unsupported subcircuit directive {}", w[0]))); }
                continue;
            }
            if first.starts_with('x') {
                let subname = w.last().unwrap().to_ascii_lowercase();
                let sub = scopes.iter().rev().find_map(|s| s.subs.get(&subname))
                    .ok_or_else(|| error(line.n, format!("unknown subcircuit {}", w.last().unwrap())))?;
                if w.len() != sub.ports.len() + 2 { return Err(error(line.n, format!("{} needs {} nodes", w[0], sub.ports.len()))); }
                let port_map = sub.ports.iter().cloned().zip(w[1..w.len()-1].iter().map(|s| mapped(s))).collect();
                self.expand(&sub.scope, &scopes, &format!("{prefix}{}.", w[0]), &port_map, depth + 1)?;
                continue;
            }
            let count = match first.as_bytes()[0] { b'r' | b'c' | b'l' | b'v' | b'i' | b'd' => 2, b'q' => 3, b'm' | b'e' | b'g' => 4,
                _ => return Err(error(line.n, format!("unknown element {}", w[0]))) };
            if w.len() < count + 2 { return Err(error(line.n, format!("{} needs {count} nodes", w[0]))); }
            let name = format!("{prefix}{}", w[0]);
            let mut nodes = [0; 4];
            for i in 0..count { nodes[i] = self.node(&mapped(&w[i + 1]), true); }
            let tail = &w[count + 1..];
            let mut tc = [0.; 2];
            let kind = match first.as_bytes()[0] {
                b'r' | b'c' | b'l' => {
                    let value = number(&tail[0], line.n)?;
                    if value <= 0. { return Err(error(line.n, format!("{} must be positive", w[0]))); }
                    let params = parameters(&tail[1..], line.n)?;
                    if params.keys().any(|k| if first.starts_with('r') { !["tc1", "tc2"].contains(&k.as_str()) } else { k != "ic" }) {
                        return Err(error(line.n, format!("unsupported parameter on {}", w[0])));
                    }
                    tc = [params.get("tc1").copied().unwrap_or(0.), params.get("tc2").copied().unwrap_or(0.)];
                    match first.as_bytes()[0] { b'r' => Kind::R(value), b'c' => Kind::C(value, params.get("ic").copied()), _ => Kind::L(value, params.get("ic").copied()) }
                }
                b'v' => Kind::V(source(tail, line.n)?), b'i' => Kind::I(source(tail, line.n)?),
                b'e' => Kind::E(number(&tail[0], line.n)?), b'g' => Kind::G(number(&tail[0], line.n)?),
                _ => {
                    let model = scopes.iter().rev().find_map(|s| s.models.get(&tail[0].to_ascii_lowercase()))
                        .ok_or_else(|| error(line.n, format!("unknown model {}", tail[0])))?;
                    let mut model = model.clone();
                    if !model.matches(first.as_bytes()[0]) { return Err(error(line.n, format!("wrong model type {}", tail[0]))); }
                    let params = parameters(&tail[1..], line.n)?;
                    model.instance(&params).map_err(|e| error(line.n, e))?;
                    for (terminal, resistance) in model.resistances() {
                        if resistance > 0. {
                            let internal = self.node(&format!("@{name}:{terminal}"), false);
                            self.add(format!("@{name}:r{terminal}"), [nodes[terminal], internal, 0, 0], Kind::R(resistance), line.n)?;
                            nodes[terminal] = internal;
                        }
                    }
                    Kind::Device(model)
                }
            };
            self.add(name, nodes, kind, line.n)?;
            self.circuit.elements.last_mut().unwrap().tc = tc;
        }
        Ok(())
    }
}
fn source(w: &[String], line: usize) -> Result<Source, String> {
    let mut result = Source { dc: None, wave: None, ac: None };
    let mut i = 0;
    while i < w.len() {
        let kind = w[i].to_ascii_lowercase();
        if kind == "ac" {
            i += 1;
            let magnitude = number(w.get(i).ok_or_else(|| error(line, "missing source value"))?, line)?;
            i += 1;
            let phase = if w.get(i).is_some_and(|s| !["dc", "pulse", "sin", "pwl", "ac"].contains(&s.to_ascii_lowercase().as_str())) {
                let phase = number(&w[i], line)?; i += 1; phase
            } else { 0. };
            if result.ac.replace((magnitude, phase)).is_some() { return Err(error(line, "duplicate AC source value")); }
            continue;
        }
        if ["pulse", "sin", "pwl"].contains(&kind.as_str()) {
            let end = w[i + 1..].iter().position(|s| s.eq_ignore_ascii_case("ac") || s.eq_ignore_ascii_case("dc")).map(|k| i + 1 + k).unwrap_or(w.len());
            let values = w[i + 1..end].iter().map(|s| number(s, line)).collect::<Result<Vec<_>, _>>()?;
            result.wave = Some(match kind.as_str() {
                "pulse" if (2..=7).contains(&values.len()) && values[2..].iter().all(|v| *v >= 0.) => Waveform::Pulse(values),
                "sin" if (2..=5).contains(&values.len()) && values[2..].iter().all(|v| *v >= 0.) => Waveform::Sin(values),
                "pwl" if values.len() >= 2 && values.len() % 2 == 0 => {
                    let points: Vec<_> = values.chunks_exact(2).map(|p| (p[0], p[1])).collect();
                    if points[0].0 < 0. || points.windows(2).any(|p| p[0].0 > p[1].0) { return Err(error(line, "PWL times must be nonnegative and ordered")); }
                    Waveform::Pwl(points)
                }
                _ => return Err(error(line, format!("invalid {} source", w[i]))),
            });
            i = end; continue;
        }
        if kind == "dc" { i += 1; }
        let value = w.get(i).ok_or_else(|| error(line, "missing source value"))?;
        if result.dc.is_some() { return Err(error(line, "duplicate DC source value")); }
        result.dc = Some(number(value, line)?); i += 1;
    }
    Ok(result)
}

fn sweep(words: &[String], line: usize, analysis: &str) -> Result<Sweep, String> {
    let mode = words.first().map(|s| s.to_ascii_lowercase()).unwrap_or_default();
    if !["dec", "oct", "lin"].contains(&mode.as_str()) { return Err(error(line, format!(".{analysis} needs dec, oct or lin"))); }
    if words.len() != 4 { return Err(error(line, format!("expected .{analysis} dec|oct|lin n fstart fstop"))); }
    let (n, start, stop) = (number(&words[1], line)?, number(&words[2], line)?, number(&words[3], line)?);
    if start <= 0. || stop < start { return Err(error(line, format!(".{analysis} needs fstart > 0 and fstop >= fstart"))); }
    if n < 1. || n.fract() != 0. || n > 100001. { return Err(error(line, "invalid frequency sweep (limit 100001 points)")); }
    let count = if start == stop { 1. } else if mode == "lin" { n } else { ((stop.ln() - start.ln()) / if mode == "dec" { 10f64.ln() } else { 2f64.ln() } * n + 1e-10).floor() + 1. };
    if !(1. ..=100001.).contains(&count) { return Err(error(line, "invalid frequency sweep (limit 100001 points)")); }
    let frequencies = (0..count as usize).map(|i| if mode == "lin" {
        if n == 1. { start } else { start + (stop - start) * i as f64 / (n - 1.) }
    } else { (start.ln() + i as f64 / n * if mode == "dec" { 10f64.ln() } else { 2f64.ln() }).exp() }).collect();
    Ok(Sweep { frequencies })
}

pub(super) fn parse(text: &str) -> Result<Circuit, String> {
    if text.len() > 1_048_576 { return Err("netlist too large (limit 1 MiB)".into()); }
    let mut physical = text.lines();
    let title = physical.next().unwrap_or("").to_owned();
    let mut logical: Vec<(usize, String)> = vec![];
    for (index, raw) in physical.enumerate() {
        let raw = raw.split(';').next().unwrap().trim();
        if raw.is_empty() || raw.starts_with('*') { continue; }
        if let Some(tail) = raw.strip_prefix('+') {
            let previous = logical.last_mut().ok_or_else(|| error(index + 2, "continuation without a previous line"))?;
            previous.1.push(' '); previous.1.push_str(tail);
        } else { logical.push((index + 2, raw.into())); }
    }
    let lines: Vec<_> = logical.into_iter().map(|(n, s)| Line { n, words: s.replace(['(', ')', ','], " ").replace('=', " = ")
        .split_whitespace().map(str::to_owned).collect() }).collect();
    if let Some(line) = lines.iter().find(|line| line.words.is_empty()) { return Err(error(line.n, "empty statement")); }
    let root = scope(&lines, &mut 0, false, 0)?;
    let mut builder = Builder { circuit: Circuit { title, nodes: vec!["0".into()], visible: vec![true], elements: vec![], analyses: vec![],
        options: Options::default(), ic: BTreeMap::new(), traces: BTreeMap::new(), size: 0, temperatures: vec![] }, node_ids: BTreeMap::new(), names: BTreeSet::new(), directives: vec![], initials: vec![] };
    builder.expand(&root, &[], "", &BTreeMap::new(), 0)?;
    let mut circuit = builder.circuit;
    for (node, value) in builder.initials {
        let id = if node == "0" { 0 } else { *builder.node_ids.get(&node).ok_or_else(|| format!("unknown node {node}"))? };
        circuit.ic.insert(id, value);
    }
    let mut seen = BTreeSet::new();
    for line in &builder.directives {
        let w = &line.words;
        let first = w[0].to_ascii_lowercase();
        let num = |i: usize| -> Result<f64, String> { number(w.get(i).ok_or_else(|| error(line.n, format!("missing argument for {}", w[0])))?, line.n) };
        match first.as_str() {
            ".op" => { if seen.insert("op") { circuit.analyses.push(Analysis::Op); } }
            ".ac" => {
                let sweep = sweep(&w[1..], line.n, "ac")?;
                if !seen.insert("ac") { return Err(error(line.n, "only one .ac analysis is supported")); }
                circuit.analyses.push(Analysis::Ac(sweep));
            }
            ".noise" => {
                let invalid = || error(line.n, ".noise needs an output node and an independent source");
                if w.len() < 8 || !w[1].eq_ignore_ascii_case("v") { return Err(invalid()); }
                let source_index = w.len() - 5;
                if !(3..=4).contains(&source_index) { return Err(invalid()); }
                let node = |s: &str| if s.eq_ignore_ascii_case("gnd") { Some(0) } else { circuit.nodes.iter().position(|n| n.eq_ignore_ascii_case(s)) };
                let output = [node(&w[2]).ok_or_else(invalid)?, if source_index == 4 { node(&w[3]).ok_or_else(invalid)? } else { 0 }];
                let source = circuit.elements.iter().position(|e| e.name.eq_ignore_ascii_case(&w[source_index]) && matches!(e.kind, Kind::V(_) | Kind::I(_))).ok_or_else(invalid)?;
                let sweep = sweep(&w[source_index + 1..], line.n, "noise")?;
                if !seen.insert("noise") { return Err(error(line.n, "only one .noise analysis is supported")); }
                circuit.analyses.push(Analysis::Noise(Noise { output, source, sweep }));
            }
            ".temp" => {
                if w.len() < 2 { return Err(error(line.n, ".temp needs at least one temperature")); }
                for i in 1..w.len() {
                    let temp = num(i)?;
                    if temp <= -273.15 { return Err(error(line.n, "temperature must exceed absolute zero")); }
                    if !circuit.temperatures.contains(&temp) { circuit.temperatures.push(temp); }
                }
                // shortcut: 100 temperature runs, add total-work budgeting before raising this limit.
                if circuit.temperatures.len() > 100 { return Err(error(line.n, "too many temperatures (limit 100)")); }
            }
            ".dc" => {
                if w.len() != 5 { return Err(error(line.n, "expected .dc source start stop step")); }
                let source = circuit.elements.iter().position(|e| e.name.eq_ignore_ascii_case(&w[1]) && matches!(e.kind, Kind::V(_) | Kind::I(_)))
                    .ok_or_else(|| error(line.n, format!("unknown source {}", w[1])))?;
                let (start, stop, step) = (num(2)?, num(3)?, num(4)?);
                if step == 0. || (stop - start) * step < 0. || !(0. ..=100000.).contains(&((stop - start) / step)) { return Err(error(line.n, "invalid DC sweep (limit 100001 points)")); }
                if !seen.insert("dc") { return Err(error(line.n, "only one .dc analysis is supported")); }
                circuit.analyses.push(Analysis::Dc { source, start, stop, step });
            }
            ".tran" => {
                let uic = w.last().is_some_and(|s| s.eq_ignore_ascii_case("uic"));
                let len = w.len() - usize::from(uic);
                if !(3..=5).contains(&len) { return Err(error(line.n, "expected .tran tstep tstop [tstart [tmax]] [uic]")); }
                let (step, stop) = (num(1)?, num(2)?);
                let start = if len > 3 { num(3)? } else { 0. };
                let max = if len > 4 { num(4)? } else { step.min((stop - start) / 50.) };
                if step <= 0. || stop <= 0. || start < 0. || start >= stop || max <= 0. { return Err(error(line.n, "invalid transient times")); }
                if !seen.insert("tran") { return Err(error(line.n, "only one .tran analysis is supported")); }
                circuit.analyses.push(Analysis::Tran(Tran { step, stop, start, max, uic }));
            }
            ".options" | ".option" => for (key, value) in parameters(&w[1..], line.n)? {
                match key.as_str() {
                    "temp" | "tnom" if value > -273.15 => if key == "temp" { circuit.options.temp = value; } else { circuit.options.tnom = value; },
                    "reltol" | "abstol" | "vntol" if value > 0. => match key.as_str() { "reltol" => circuit.options.reltol = value, "abstol" => circuit.options.abstol = value, _ => circuit.options.vntol = value },
                    "gmin" if value >= 0. => circuit.options.gmin = value,
                    "itl1" | "itl4" if (1. ..=10000.).contains(&value) && value.fract() == 0. => if key == "itl1" { circuit.options.itl1 = value as usize; } else { circuit.options.itl4 = value as usize; },
                    _ => return Err(error(line.n, format!("invalid option {key}"))),
                }
            },
            ".print" | ".plot" => {
                let mut i = 1;
                let analysis = if w.get(i).is_some_and(|s| ["tran", "dc", "ac", "noise"].contains(&s.to_ascii_lowercase().as_str())) { i += 1; w[1].to_ascii_lowercase() } else { "all".into() };
                let traces = circuit.traces.entry(analysis).or_default();
                while i < w.len() {
                    let label = if (w[i].eq_ignore_ascii_case("v") || w[i].eq_ignore_ascii_case("i")) && i + 1 < w.len() {
                        let label = format!("{}({})", w[i].to_ascii_lowercase(), w[i + 1].to_ascii_lowercase()); i += 2; label
                    } else { let label = format!("v({})", w[i].to_ascii_lowercase()); i += 1; label };
                    if !traces.contains(&label) { traces.push(label); }
                }
            }
            _ => return Err(error(line.n, format!("unknown directive {}", w[0]))),
        }
    }
    if circuit.analyses.is_empty() { return Err("no analysis: add .op, .dc or .tran".into()); }
    circuit.size = circuit.nodes.len() - 1;
    for element in &mut circuit.elements {
        if matches!(element.kind, Kind::V(_) | Kind::L(_, _) | Kind::E(_)) { element.branch = circuit.size; circuit.size += 1; }
    }
    if circuit.size > 2000 { return Err(format!("circuit too large: {} unknowns (limit 2000)", circuit.size)); }
    Ok(circuit)
}
