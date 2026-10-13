use super::{exec::{clip, find_program, run}, paths::project_path, rfmath::*, script3d::{files, parse_runner_output, valid_name}, Tool, ToolContext, ToolError, ToolFuture, ToolResult};
use crate::jsfmt::{locale, number, numeric, precision, round, string, truthy};
use serde_json::{json, Value};
use std::{path::PathBuf, time::Instant};

pub fn tools() -> Vec<Tool> {
    vec![
        {
            Tool::new("pcb", "rf_calc", r#"RF engineering calculations for boards with radios, antennas and fast signals:
- calc "line": impedance of a microstrip, grounded coplanar waveguide (cpwg), stripline or differential microstrip pair, or the trace width for a target impedance (target_ohms; e.g. 50 for an antenna feed, 90 differential for USB, 100 for Ethernet). Use a fab stackup (jlc-2layer-1.6, jlc-4layer-1.6-7628, jlc-4layer-1.6-3313) or height_mm and er. Gives the wavelength and quarter-wave length on the line too.
- calc "match": L matching networks that bring a load impedance (load_re + j load_im, e.g. a measured or simulated antenna) to 50 ohm, with ideal and E24 part values.
- calc "link": link budget and range from transmit power, antenna gains and receiver sensitivity.
- calc "track_width": IPC-2221 copper width from current_a, temp_rise_c, copper_mm and layer (external/internal).
- calc "via": copper via resistance and current from drill_mm, length_mm, plating_mm and temp_rise_c.
- calc "fusing": Onderdonk adiabatic copper fusing current from width_mm, copper_mm and time_s (20 C ambient).
- calc "spacing": IPC-2221B spacing from voltage_v and layer (internal/external/external_coated).
- calc "e_series": nearest E12/E24/E96 value, or best unloaded divider from vin_v and vout_v.

Use the trace width it gives in pcb/<name>.pcb.json and keep RF traces short with ground on both sides."#, json!({"type":"object", "additionalProperties": false,"properties":{"calc":{"type":"string","enum":["line","match","link","track_width","via","fusing","spacing","e_series"],"description":"Calculation: RF lines, matching, link budgets or PCB calculators"},"line":{"type":"string","enum":["microstrip","cpwg","stripline","diff_microstrip"],"description":"line: geometry (default microstrip)"},"stackup":{"type":"string","enum":["jlc-2layer-1.6","jlc-4layer-1.6-7628","jlc-4layer-1.6-3313"],"description":"line: fab stackup"},"height_mm":{"type":"number","description":"line: dielectric height to the reference plane (stripline: plane-to-plane)"},"er":{"type":"number","description":"line: relative permittivity of the dielectric"},"copper_mm":{"type":"number","description":"line/track_width/fusing: copper thickness in mm (line default 0.035)"},"width_mm":{"type":"number","description":"line/fusing: trace width in mm"},"target_ohms":{"type":"number","description":"line: impedance to find the width for (differential for diff_microstrip)"},"gap_mm":{"type":"number","description":"line: cpwg gap to side ground, or diff pair spacing"},"frequency_mhz":{"type":"number","description":"Frequency in MHz (line: default 2450)"},"load_re":{"type":"number","description":"match: load resistance in ohm"},"load_im":{"type":"number","description":"match: load reactance in ohm (negative = capacitive)"},"source_ohms":{"type":"number","description":"match: impedance to match to (default 50)"},"tx_dbm":{"type":"number","description":"link: transmit power"},"tx_gain_dbi":{"type":"number","description":"link: transmit antenna gain in dBi (default 0)"},"rx_gain_dbi":{"type":"number","description":"link: receive antenna gain in dBi (default 0)"},"sensitivity_dbm":{"type":"number","description":"link: receiver sensitivity"},"distance_m":{"type":"number","description":"link: distance to evaluate"},"loss_db":{"type":"number","description":"link: extra losses (cables, enclosure)"},"margin_db":{"type":"number","description":"link: fade margin for the range (default 10)"},"environment":{"type":"string","enum":["free_space","outdoor","indoor"],"description":"link: propagation environment (default free_space)"},"path_loss_exponent":{"type":"number","description":"link: overrides environment"},"current_a":{"type":"number","description":"track_width: required current in amperes"},"temp_rise_c":{"type":"number","description":"track_width/via: permitted temperature rise in degrees C"},"layer":{"type":"string","enum":["internal","external","external_coated"],"description":"track_width: internal/external; spacing: internal/external/external_coated"},"drill_mm":{"type":"number","description":"via: finished drill diameter in mm"},"length_mm":{"type":"number","description":"via: barrel length in mm"},"plating_mm":{"type":"number","description":"via: copper plating thickness in mm"},"time_s":{"type":"number","description":"fusing: time to melt in seconds"},"voltage_v":{"type":"number","description":"spacing: DC or AC peak voltage"},"value":{"type":"number","description":"e_series: value to round to a preferred value"},"series":{"type":"string","enum":["E12","E24","E96"],"description":"e_series: preferred-value series, default E24"},"vin_v":{"type":"number","description":"e_series: divider input voltage"},"vout_v":{"type":"number","description":"e_series: desired unloaded output voltage"}},"required":["calc"]}), rf_calc)
        },
        {
            let mut tool = Tool::new("pcb", "rf_antenna_sim", r#"Simulates a board's antenna with openEMS (a full-wave FDTD field solver), from the board pcb_build made. Reports the resonance, S11 and -10 dB bandwidth, the input impedance at the target frequency, radiation efficiency and peak gain, and writes rf/<board>/<antenna>-s11.svg, <antenna>-pattern.svg (gain in three planes), <antenna>.s1p (Touchstone, for a VNA comparison or a circuit simulator) and <antenna>.json.
The board needs ground copper (a pour) under the antenna's feed pad. Use it to check an antenna before ordering boards: fix the length if the resonance is off, then feed the impedance to rf_calc match for the matching network. "fast" takes about a minute, "accurate" several."#, json!({"type":"object", "additionalProperties": false,"properties":{"board":{"type":"string","description":"Name of the board built with pcb_build (reads pcb/<board>.circuit.json)"},"antenna":{"type":"string","description":"Antenna's component name, e.g. ANT1 (needed only when the board has several)"},"frequency_mhz":{"type":"number","description":"Target frequency (default 2450)"},"span_mhz":{"type":"number","description":"Width of the sweep around the target (default 80 % of the target)"},"quality":{"type":"string","enum":["fast","accurate"],"description":"Mesh density (default fast)"},"er":{"type":"number","description":"Substrate permittivity (default 4.5, FR-4)"},"loss_tangent":{"type":"number","description":"Substrate loss tangent (default 0.02, FR-4 at 2.4 GHz)"}},"required":["board"]}), rf_antenna_sim);
            tool.requires = openems_python();
            tool.needs = Some("openEMS");
            tool.install = Some(serde_json::from_value(json!({"url":"scripts/setup-fedora.sh --only rf (builds openEMS from source)"})).unwrap());
            tool
        },
    ]
}

pub struct Stackup { pub h: f64, pub er: f64, pub t: f64, pub note: &'static str }
/// Dielectric between an outer layer and its reference plane, from the fab's published stackups.
pub const STACKUPS: &[(&str, Stackup)] = &[
    ("jlc-2layer-1.6", Stackup { h: 1.53, er: 4.5, t: 0.035, note: "JLCPCB 2-layer 1.6 mm FR-4, top layer over a bottom ground" }),
    ("jlc-4layer-1.6-7628", Stackup { h: 0.2104, er: 4.4, t: 0.035, note: "JLCPCB JLC04161H-7628, outer layer over layer 2 (7628 prepreg)" }),
    ("jlc-4layer-1.6-3313", Stackup { h: 0.0994, er: 4.1, t: 0.035, note: "JLCPCB JLC04161H-3313, outer layer over layer 2 (3313 prepreg)" }),
];
const KINDS: &[&str] = &["microstrip", "cpwg", "stripline", "diff_microstrip"];

fn num(v: Option<&Value>, what: &str, min: f64, max: f64) -> Result<f64, ToolError> {
    let n = numeric(v);
    if v.is_none() || v.is_some_and(|v| v.is_null() || v.as_str() == Some("")) || !n.is_finite() || n < min || n > max {
        return Err(ToolError::expected(format!("{what} must be a number from {} to {}", number(min), number(max))));
    }
    Ok(n)
}
fn opt(v: Option<&Value>, what: &str, min: f64, max: f64) -> Result<Option<f64>, ToolError> {
    v.map(|v| num(Some(v), what, min, max)).transpose()
}
fn r(x: f64, n: usize) -> String { number(precision(x, n)) }
fn si(v: f64, unit: &str) -> String {
    let (scale, prefix) = if unit == "H" { if v < 1e-6 { (1e9, "n") } else { (1e6, "µ") } }
        else if v < 1e-9 { (1e12, "p") } else { (1e9, "n") };
    format!("{} {prefix}{unit}", r(v * scale, 3))
}
fn line(kind: &str, w: f64, h: f64, er: f64, t: f64, gap: Option<f64>, f: f64) -> Line {
    match kind {
        "microstrip" => microstrip(w, h, er, t, f),
        "cpwg" => cpwg(w, gap.unwrap(), h, er),
        "stripline" => stripline(w, h, er, t),
        "diff_microstrip" => coupled_microstrip(w, gap.unwrap(), h, er, f),
        _ => unreachable!(),
    }
}

fn calc_line(input: &Value) -> Result<String, ToolError> {
    let kind = string(Some(input.get("line").filter(|v| !v.is_null()).unwrap_or(&json!("microstrip"))));
    if !KINDS.contains(&kind.as_str()) { return Err(ToolError::expected(format!("line must be one of {}", KINDS.join(", ")))); }
    let preset = input.get("stackup").and_then(|v| STACKUPS.iter().find(|(key, _)| *key == string(Some(v))).map(|(_, s)| s));
    if input.get("stackup").is_some() && preset.is_none() {
        return Err(ToolError::expected(format!("unknown stackup; use one of {}, or give height_mm and er", STACKUPS.iter().map(|(key, _)| *key).collect::<Vec<_>>().join(", "))));
    }
    let h = opt(input.get("height_mm"), "height_mm", 0.01, 20.0)?.or(preset.map(|s| s.h));
    let er = opt(input.get("er"), "er", 1.0, 20.0)?.or(preset.map(|s| s.er));
    let t = opt(input.get("copper_mm"), "copper_mm", 0.0, 0.2)?.or(preset.map(|s| s.t)).unwrap_or(0.035);
    let (h, er) = h.zip(er).ok_or_else(|| ToolError::expected("give a stackup, or height_mm (dielectric to the reference plane; for stripline the plane-to-plane spacing) and er"))?;
    let f = opt(input.get("frequency_mhz"), "frequency_mhz", 1.0, 300_000.0)?.unwrap_or(2450.0) * 1e6;
    let gap = opt(input.get("gap_mm"), "gap_mm", 0.01, 50.0)?;
    if ["cpwg", "diff_microstrip"].contains(&kind.as_str()) && gap.is_none() {
        return Err(ToolError::expected(format!("{kind} needs gap_mm ({})", if kind == "cpwg" { "track to side ground" } else { "between the two traces" })));
    }
    let w = opt(input.get("width_mm"), "width_mm", 0.01, 50.0)?;
    let target = opt(input.get("target_ohms"), "target_ohms", 5.0, 300.0)?;
    if w.is_none() && target.is_none() { return Err(ToolError::expected("give width_mm to analyse a trace, or target_ohms to find its width")); }
    let mut lines = Vec::new();
    let w = if let Some(w) = w { w } else {
        // Impedance falls as the trace widens.
        let target = target.unwrap();
        let found = solve_for(|x| line(&kind, x, h, er, t, gap, f).z0, target, 0.01, 50.0).ok_or_else(|| ToolError::expected(format!("no width from 0.01 to 50 mm gives {} ohm on this stackup{}; change the gap or the stackup", number(target), gap.map_or(String::new(), |g| format!(" with a {} mm gap", number(g))))))?;
        lines.push(format!("Width for {} ohm{}: {} mm.", number(target), if kind == "diff_microstrip" { " differential" } else { "" }, r(found, 3)));
        found
    };
    let res = line(&kind, w, h, er, t, gap, f);
    let lambda = wavelength_mm(f, res.eeff);
    lines.extend([
        format!("{kind} {} mm wide{} on {} mm, er {}{}:", r(w, 3), gap.map_or(String::new(), |g| format!(", {} mm gap", number(g))), number(h), number(er), preset.map_or(String::new(), |s| format!(" ({})", s.note))),
        format!("  Z0 {} ohm{}; effective er {} at {} MHz", r(res.z0, 4), res.z_odd.filter(|z| *z != 0.0).map_or(String::new(), |odd| format!(" differential (odd mode {}, even mode {} ohm)", r(odd, 4), r(res.z_even.unwrap(), 4))), r(res.eeff, 4), number(f / 1e6)),
        format!("  wavelength on the line {} mm; a quarter wave is {} mm", r(lambda, 4), r(lambda / 4.0, 4)),
    ]);
    if w < 0.1 { lines.push(format!("  {} mm is below the 0.1 mm (4 mil) most fabs make reliably; use a thinner dielectric or a wider gap.", r(w, 3))); }
    lines.push(if kind == "cpwg" {
        "  Closed-form model (conformal mapping) that ignores copper thickness: when the gap is under about 5 times the copper thickness the real line reads a few ohms lower. Order impedance control and check the fab's own calculator for the final width.".into()
    } else {
        format!("  Closed-form model ({}), typically within 1–2 %; order impedance control and check the fab's own calculator for the final width.", if kind == "stripline" { "Wheeler/Cohn" } else { "Hammerstad-Jensen" })
    });
    Ok(lines.join("\n"))
}

fn describe(parts: &[Part]) -> String {
    parts.iter().map(|p| format!("{} {} {}", p.r#where, p.kind, si(p.value, if p.kind == "L" { "H" } else { "F" }))).collect::<Vec<_>>().join(" → ")
}
fn calc_match(input: &Value) -> Result<String, ToolError> {
    let zl = Impedance { re: num(input.get("load_re"), "load_re (load resistance, ohm)", 0.01, 10_000.0)?, im: num(Some(input.get("load_im").filter(|v| !v.is_null()).unwrap_or(&json!(0))), "load_im (load reactance, ohm)", -10_000.0, 10_000.0)? };
    let r0 = opt(input.get("source_ohms"), "source_ohms", 1.0, 1000.0)?.unwrap_or(50.0);
    let f = num(input.get("frequency_mhz"), "frequency_mhz", 0.001, 300_000.0)? * 1e6;
    let before = reflection(zl, r0);
    let nets = l_match(zl, r0, f).map_err(ToolError::unexpected)?;
    if nets.is_empty() { return Ok(format!("Load {}{}j{} ohm is already matched to {} ohm.", number(zl.re), if zl.im < 0.0 { "-" } else { "+" }, number(zl.im.abs()), number(r0))); }
    let mut lines = vec![format!("Load {}{}j{} ohm at {} MHz: return loss {} dB, VSWR {} before matching.", number(zl.re), if zl.im < 0.0 { " - " } else { " + " }, number(zl.im.abs()), number(f / 1e6), r(before.return_loss_db, 3), r(before.vswr, 3)), format!("L networks to {} ohm, listed from the source (radio) side toward the load (antenna):", number(r0))];
    for (i, parts) in nets.iter().enumerate() {
        let real = parts.iter().map(|p| Part { value: nearest_standard(p.value, E24), ..p.clone() }).collect::<Vec<_>>();
        let after = reflection(network_input(&real, zl, f), r0);
        lines.extend([format!("  {}. {}", i + 1, describe(parts)), format!("     with E24 parts {}: return loss {} dB, VSWR {}", describe(&real), r(after.return_loss_db, 3), r(after.vswr, 3))]);
    }
    lines.push("Lay the parts out as a pi footprint (shunt-series-shunt, 0402) so either network fits, and fit the final values after measuring the antenna on a VNA: the board, enclosure and hand change its impedance.".into());
    Ok(lines.join("\n"))
}
fn calc_link(input: &Value) -> Result<String, ToolError> {
    let env = string(Some(input.get("environment").filter(|v| !v.is_null()).unwrap_or(&json!("free_space"))));
    let exponent = opt(input.get("path_loss_exponent"), "path_loss_exponent", 1.5, 6.0)?.or(match env.as_str() { "free_space" => Some(2.0), "outdoor" => Some(2.7), "indoor" => Some(3.3), _ => None }).ok_or_else(|| ToolError::expected("environment must be free_space, outdoor or indoor (or give path_loss_exponent)"))?;
    let f = num(input.get("frequency_mhz"), "frequency_mhz", 1.0, 300_000.0)? * 1e6;
    let o = LinkOptions {
        tx_dbm: num(input.get("tx_dbm"), "tx_dbm", -50.0, 60.0)?,
        tx_gain_dbi: opt(input.get("tx_gain_dbi"), "tx_gain_dbi", -30.0, 40.0)?.unwrap_or(0.0),
        rx_gain_dbi: opt(input.get("rx_gain_dbi"), "rx_gain_dbi", -30.0, 40.0)?.unwrap_or(0.0),
        sensitivity_dbm: num(input.get("sensitivity_dbm"), "sensitivity_dbm", -160.0, 0.0)?, f,
        distance_m: opt(input.get("distance_m"), "distance_m", 0.01, 1e8)?,
        loss_db: opt(input.get("loss_db"), "loss_db", 0.0, 100.0)?,
        margin_db: opt(input.get("margin_db"), "margin_db", 0.0, 60.0)?, exponent: Some(exponent),
    };
    let b = link_budget(&o);
    let mut lines = vec![format!("Link at {} MHz, {env} (path-loss exponent {}): budget {} dB from {} dBm out to {} dBm sensitivity.", number(f / 1e6), number(exponent), r(b.budget_db, 4), number(o.tx_dbm), number(o.sensitivity_dbm)), format!("  Range with a {} dB fade margin: {}", number(o.margin_db.unwrap_or(10.0)), if b.range_m >= 1000.0 { format!("{} km", r(b.range_m / 1000.0, 3)) } else { format!("{} m", r(b.range_m, 3)) })];
    if let Some(rx) = b.rx_dbm { lines.push(format!("  At {} m: received {} dBm, {} dB above sensitivity", number(o.distance_m.unwrap()), r(rx, 4), r(b.margin_at_distance_db.unwrap(), 3))); }
    lines.push("  Estimates from a log-distance model; walls, bodies and the enclosure can cost 10–20 dB more. Transmit power and antenna gain must stay within the band's legal limit (EIRP) where the product is sold.".into());
    Ok(lines.join("\n"))
}

fn calc_pcb(input: &Value, calc: &str) -> Result<String, ToolError> {
    let required = |key, min, max| num(input.get(key), key, min, max);
    match calc {
        "track_width" => {
            let current = required("current_a", 1e-6, 1e6)?;
            let rise = required("temp_rise_c", 1e-6, 1000.)?;
            let copper = required("copper_mm", 1e-6, 10.)?;
            let layer = input["layer"].as_str().unwrap_or("external");
            if !["internal", "external"].contains(&layer) {
                return Err(ToolError::expected("track_width layer must be external or internal"));
            }
            Ok(format!("IPC-2221 {layer} track width: {} mm for {} A, {} C rise.\nEmpirical copper estimate; verify the fab stackup and thermal conditions.",
                r(track_width(current, rise, copper, layer == "internal"), 6), number(current), number(rise)))
        }
        "via" => {
            let drill = required("drill_mm", 1e-6, 100.)?;
            let length = required("length_mm", 1e-6, 100.)?;
            let plating = required("plating_mm", 1e-6, 10.)?;
            let rise = required("temp_rise_c", 1e-6, 1000.)?;
            let (resistance, current) = via_properties(drill, length, plating, rise);
            Ok(format!("Copper via resistance: {} mΩ.\nIPC-2221 estimated current: {} A at {} C rise.",r(resistance * 1000.,6),r(current,6),number(rise)))
        }
        "fusing" => {
            let width = required("width_mm", 1e-6, 1000.)?;
            let copper = required("copper_mm", 1e-6, 10.)?;
            let time = required("time_s", 1e-6, 1e6)?;
            Ok(format!("Onderdonk fusing current: {} A for {} s (copper, 20 C ambient, 1083 C melting point).\nAdiabatic estimate; excludes heat loss and latent heat.",r(fusing_current(width,copper,time),6),number(time)))
        }
        "spacing" => {
            let voltage = required("voltage_v",0.,1e6)?;
            let layer = input["layer"].as_str().unwrap_or("external");
            let (column, class) = match layer {
                "internal" => (0,"B1 internal"), "external" => (1,"B2 external uncoated, up to 3050 m"),
                "external_coated" => (2,"B4 permanent polymer coating"),
                _ => return Err(ToolError::expected("spacing layer must be internal, external or external_coated")),
            };
            Ok(format!("IPC-2221B {class} spacing: {} mm at {} V (DC or AC peak).",number(conductor_spacing(voltage,column)),number(voltage)))
        }
        "e_series" => {
            let series_name = input["series"].as_str().unwrap_or("E24");
            let series = match series_name { "E12" => E12, "E24" => E24, "E96" => E96,
                _ => return Err(ToolError::expected("series must be E12, E24 or E96")), };
            if input.get("value").is_some() {
                let value = required("value",1e-12,1e12)?;
                return Ok(format!("Nearest {series_name} value: {} Ω.",number(nearest_standard(value,series))));
            }
            let vin = required("vin_v",1e-6,1e6)?;
            let target = required("vout_v",1e-6,1e6)?;
            if target >= vin { return Err(ToolError::expected("vout_v must be smaller than vin_v")); }
            let (top,bottom,output) = divider(vin,target,series);
            Ok(format!("{series_name} divider: top {} Ω, bottom {} Ω.\nOutput: {} V; error {} % (unloaded).",number(precision(top,6)),number(precision(bottom,6)),r(output,8),r((output/target-1.)*100.,6)))
        }
        _ => unreachable!(),
    }
}

fn rf_calc(input: Value, _ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let calc = string(Some(input.get("calc").filter(|v| !v.is_null()).unwrap_or(&json!(""))));
        let text = match calc.as_str() { "line" => calc_line(&input)?, "match" => calc_match(&input)?, "link" => calc_link(&input)?, "track_width" | "via" | "fusing" | "spacing" | "e_series" => calc_pcb(&input, &calc)?, _ => return Err(ToolError::expected("calc must be \"line\", \"match\" or \"link\"")) };
        Ok(ToolResult::text(text))
    })
}

/// Python environments with openEMS, from the Fedora setup script's build or a manual one.
pub fn openems_python() -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(p) = std::env::var_os("FORGE_OPENEMS_PYTHON").filter(|p| !p.is_empty()) { candidates.push(PathBuf::from(p).to_string_lossy().into_owned()); }
    candidates.push(crate::env::home_dir().join("opt/openEMS/venv/bin/python3").to_string_lossy().into_owned());
    candidates.push("/opt/openEMS/venv/bin/python3".into());
    candidates
}

fn mhz(f: f64) -> String { format!("{} MHz", r(f / 1e6, 4)) }
fn pct(x: f64) -> String { format!("{} %", number(round(x * 100.0))) }
fn ohms(value: &Value) -> String {
    let (re, im) = (numeric(value.get(0)), numeric(value.get(1)));
    format!("{} {} j{} ohm", r(re, 3), if im < 0.0 { "-" } else { "+" }, r(im.abs(), 3))
}

pub fn describe_sim(board: &str, res: &Value, quality: &str) -> String {
    let n = |key| numeric(res.get(key));
    let band = res.get("band").filter(|v| truthy(v));
    let mut lines = vec![format!("Simulated {} on pcb/{board} with openEMS ({quality} mesh, {} cells, {} s), fed {}; swept {} to {}.", string(res.get("antenna")), locale(n("cells")), number(round(n("seconds"))), string(res.get("port")), mhz(numeric(res["span"].get(0))), mhz(numeric(res["span"].get(1))))];
    if let Some(band) = band {
        let (lo, hi) = (numeric(band.get(0)), numeric(band.get(1)));
        lines.push(format!("  Best match {} dB at {}; S11 below -10 dB from {} to {} ({} MHz{}).", r(n("s11_min_db"), 3), mhz(n("resonance")), mhz(lo), mhz(hi), r((hi - lo) / 1e6, 3), if truthy(&band[2]) { ", runs past the sweep" } else { "" }));
    } else { lines.push(format!("  Best match only {} dB, at {}: S11 never reaches -10 dB in the sweep.", r(n("s11_min_db"), 3), mhz(n("resonance")))); }
    lines.push(format!("  At {}: S11 {} dB, input impedance {}.", mhz(n("f0")), r(n("s11_f0_db"), 3), ohms(&res["zin_f0"])));
    lines.push(format!("  Radiation efficiency {}; with the mismatch {} of the radio's power is radiated. Peak gain {} dBi toward theta {}°, phi {}°{}.", pct(n("radiation_efficiency")), pct(n("total_efficiency")), r(n("peak_gain_dbi"), 3), string(res["peak_dir"].get(0)), string(res["peak_dir"].get(1)), if res.get("directivity_dbi") == Some(&Value::Null) { String::new() } else { format!(" (directivity {} dBi)", r(n("directivity_dbi"), 3)) }));
    if n("ground_under") > 0.1 {
        lines.push(format!("  Warning: {} of the antenna's trace has ground copper under it or within 2 mm beside it, which turns it into a transmission line rather than an antenna (low, lossy resonance, poor match). Keep every layer's pour out from under and around the radiating arm: put a <keepout> over the antenna area, or place the antenna at a board edge beyond the ground plane, then rebuild and simulate again.", pct(n("ground_under"))));
    }
    let in_band = band.is_some_and(|band| numeric(band.get(0)) <= n("f0") && n("f0") <= numeric(band.get(1)));
    if !in_band {
        let off = n("resonance") / n("f0");
        // A -3 dB dip is already a real resonance for low-profile antennas (inverted-L, IFA) whose radiation resistance is far from 50 ohm.
        if n("s11_min_db") < -3.0 && n("ground_under") <= 0.1 && (off - 1.0).abs() > 0.03 {
            lines.push(format!("  The antenna resonates {} the target: {} its radiating arm by about {} % and simulate again.", if off > 1.0 { "above" } else { "below" }, if off > 1.0 { "lengthen" } else { "shorten" }, number(round((off - 1.0).abs() * 100.0))));
        }
        lines.push(format!("  To match it at {}: rf_calc with calc \"match\", load_re {}, load_im {}, frequency_mhz {}.", mhz(n("f0")), r(numeric(res["zin_f0"].get(0)), 4), r(numeric(res["zin_f0"].get(1)), 4), number(n("f0") / 1e6)));
    }
    lines.push("  The model is the antenna's copper, the board substrate, the copper pours and vias; other parts, the enclosure and a hand nearby are not in it and usually pull the resonance down a few percent. Tune the matching network on a VNA before certification.".into());
    lines.join("\n")
}

fn rf_antenna_sim(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let board = string(Some(input.get("board").filter(|v| !v.is_null()).unwrap_or(&json!(""))));
        if !valid_name(&board) { return Err(ToolError::expected("board is the name pcb_build used: letters, digits, - and _")); }
        let circuit = project_path(&ctx.project_dir, &format!("pcb/{board}.circuit.json"))?;
        if !tokio::fs::metadata(&circuit).await.is_ok_and(|m| m.is_file()) { return Err(ToolError::expected(format!("pcb/{board}.circuit.json does not exist; build the board with pcb_build first"))); }
        let antenna = input.get("antenna").map_or(String::new(), |v| string(Some(v)));
        if !antenna.is_empty() && (antenna.len() > 32 || !antenna.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')) { return Err(ToolError::expected("antenna is a component name such as ANT1")); }
        let f0 = opt(input.get("frequency_mhz"), "frequency_mhz", 50.0, 30_000.0)?.unwrap_or(2450.0) * 1e6;
        let span = opt(input.get("span_mhz"), "span_mhz", 1.0, 30_000.0)?.unwrap_or((0.8 * f0) / 1e6) * 1e6;
        if span >= 2.0 * f0 { return Err(ToolError::expected("span_mhz must be less than twice the frequency")); }
        let quality = string(Some(input.get("quality").filter(|v| !v.is_null()).unwrap_or(&json!("fast"))));
        if quality != "fast" && quality != "accurate" { return Err(ToolError::expected("quality must be \"fast\" or \"accurate\"")); }
        let er = opt(input.get("er"), "er", 1.0, 20.0)?.unwrap_or(4.5);
        let tand = opt(input.get("loss_tangent"), "loss_tangent", 0.0, 0.2)?.unwrap_or(0.02);
        let python = find_program(&ctx.openems_python).ok_or_else(|| ToolError::expected("openEMS is not installed; run scripts/setup-fedora.sh --only rf"))?;
        let out_rel = format!("rf/{board}");
        let out = project_path(&ctx.project_dir, &out_rel)?;
        // Keep scratch paths inside the project; remove them before returning, as with the TS temp folder.
        let sim_rel = format!(".forge-openems-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        let sim = project_path(&ctx.project_dir, &sim_rel)?;
        tokio::fs::create_dir(&sim).await?;
        let args = json!({"circuit":circuit,"out":out,"antenna":antenna,"f0_hz":f0,"span_hz":span,"quality":quality,"er":er,"tand":tand,"threads":0,"sim_dir":project_path(&ctx.project_dir, &format!("{sim_rel}/run"))?});
        let started = Instant::now();
        let result = async {
            let res = run(&python, &[ctx.app_dir.join("server/runners/openems_antenna.py").to_string_lossy().into_owned(), serde_json::to_string(&args)?], &ctx.project_dir, if quality == "accurate" { 60 * 60_000 } else { 20 * 60_000 }, &[]).await;
            let parsed = parse_runner_output(&res)?;
            if let Some(error) = parsed.error { return Err(ToolError::expected(clip(&error, 4000))); }
            let mut result = parsed.result.unwrap();
            result["seconds"] = json!(started.elapsed().as_secs_f64());
            let files = files(&result["files"]).iter().map(|f| format!("{out_rel}/{f}")).collect();
            Ok(ToolResult { text: describe_sim(&board, &result, &quality), files })
        }.await;
        tokio::fs::remove_dir_all(sim).await?;
        result
    })
}
