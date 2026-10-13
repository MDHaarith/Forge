use std::{fs, io};

#[derive(Clone, Debug)]
pub struct Land {
    pub pin: usize,
    pub at: [f64; 2],
    pub size: [f64; 2],
}

#[derive(Clone, Copy)]
struct Range(f64, f64);

impl Range {
    fn tol(self) -> f64 {
        self.1 - self.0
    }
}

fn round(x: f64, step: f64) -> f64 {
    (x / step).round() * step
}

// The IPC root-sum-square model, including independent terminal-length tolerances.
fn ipc(l: Range, s: Range, cs: f64, w: Range, goals: [f64; 3], step: f64) -> [f64; 3] {
    let rms = |c: f64| (c * c + 0.1_f64.powi(2) + 0.05_f64.powi(2)).sqrt();
    let z = round(l.0 + 2.0 * goals[0] + rms(l.tol()), step);
    let g = round(s.1 - 2.0 * goals[1] - rms(cs), step);
    let x = round(w.0 + 2.0 * goals[2] + rms(w.tol()), step);
    [(z - g) / 2.0, x, (z + g) / 2.0]
}

pub fn pattern(name: &str) -> Vec<Land> {
    // Inputs come from KiCad's published generator size definitions, not fitted pad values.
    let (length, width, spacing, n, pitch) = match name {
        "0402" => {
            let [a, b, c] = ipc(
                Range(1.0, 1.1),
                Range(0.4, 0.7),
                0.3,
                Range(0.48, 0.6),
                [0.2, 0.0, 0.0],
                0.02,
            );
            (a, b, c, 2, 0.0)
        }
        "0603" => {
            let [a, b, c] = ipc(
                Range(1.5, 1.7),
                Range(0.7, 1.11),
                0.41,
                Range(0.7, 0.95),
                [0.35, 0.0, 0.0],
                0.05,
            );
            (a, b, c, 2, 0.0)
        }
        "soic8" | "qfn32" => {
            let (l, t, w, goals, n, pitch) = if name == "soic8" {
                (
                    Range(5.8, 6.2),
                    Range(0.4, 1.27),
                    Range(0.31, 0.51),
                    [0.35, 0.35, 0.03],
                    8,
                    1.27,
                )
            } else {
                (
                    Range(4.9, 5.1),
                    Range(0.3, 0.5),
                    Range(0.18, 0.3),
                    [0.3, 0.0, -0.04],
                    32,
                    0.5,
                )
            };
            let cs = (l.tol().powi(2) + 2.0 * t.tol().powi(2)).sqrt();
            let s_min = l.0 - 2.0 * t.1;
            let s_max = l.1 - 2.0 * t.0;
            let s = Range(s_min, s_max - ((s_max - s_min) - cs) / 2.0);
            let [a, b, c] = ipc(l, s, cs, w, goals, 0.05);
            (a, b, c, n, pitch)
        }
        _ => panic!("unsupported spike footprint {name}"),
    };
    if n == 2 {
        return (0..2)
            .map(|i| Land {
                pin: i + 1,
                at: [(i as f64 - 0.5) * spacing, 0.0],
                size: [length, width],
            })
            .collect();
    }
    let rows = if n == 8 { 4 } else { 8 };
    let mut pads = Vec::new();
    for i in 0..rows {
        let y = (i as f64 - (rows - 1) as f64 / 2.0) * pitch;
        pads.push(Land {
            pin: i + 1,
            at: [-spacing / 2.0, y],
            size: [length, width],
        });
        pads.push(Land {
            pin: if n == 8 { 8 - i } else { 24 - i },
            at: [spacing / 2.0, y],
            size: [length, width],
        });
        if n == 32 {
            pads.push(Land {
                pin: 9 + i,
                at: [y, spacing / 2.0],
                size: [width, length],
            });
            pads.push(Land {
                pin: 32 - i,
                at: [y, -spacing / 2.0],
                size: [width, length],
            });
        }
    }
    if n == 32 {
        pads.push(Land {
            pin: 33,
            at: [0.0, 0.0],
            size: [3.1, 3.1],
        });
    }
    pads.sort_by_key(|p| p.pin);
    pads
}

// shortcut: read only numbered copper pads and their at/size fields, use an S-expression parser for foreign files.
pub fn parse_kicad(text: &str) -> Vec<Land> {
    text.split("(pad ")
        .skip(1)
        .filter_map(|block| {
            let id = block.split('"').nth(1)?.parse::<usize>().ok()?;
            let field = |name: &str| -> Option<[f64; 2]> {
                let s = block
                    .split(&format!("({name} "))
                    .nth(1)?
                    .split(')')
                    .next()?;
                let mut nums = s.split_whitespace().map(str::parse::<f64>);
                Some([nums.next()?.ok()?, nums.next()?.ok()?])
            };
            if !block.contains("\"F.Cu\"") {
                return None;
            }
            Some(Land {
                pin: id,
                at: field("at")?,
                size: field("size")?,
            })
        })
        .collect()
}

pub fn compare(log: &mut String) -> io::Result<()> {
    for (name, path) in [
        ("0402", "Resistor_SMD.pretty/R_0402_1005Metric"),
        ("0603", "Resistor_SMD.pretty/R_0603_1608Metric"),
        ("soic8", "Package_SO.pretty/SOIC-8_3.9x4.9mm_P1.27mm"),
        (
            "qfn32",
            "Package_DFN_QFN.pretty/QFN-32-1EP_5x5mm_P0.5mm_EP3.1x3.1mm",
        ),
    ] {
        let source = format!("/usr/share/kicad/footprints/{path}.kicad_mod");
        let reference = match fs::read_to_string(&source) {
            Ok(s) => parse_kicad(&s),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                crate::report(log, format!("skipped: KiCad footprint missing: {source}"));
                continue;
            }
            Err(e) => return Err(e),
        };
        let ours = pattern(name);
        assert_eq!(ours.len(), reference.len());
        let pin = |pads: &[Land], id| pads.iter().find(|p| p.pin == id).unwrap().clone();
        let a = pin(&ours, 1);
        let b = pin(&reference, 1);
        let opposite = if name == "0402" || name == "0603" {
            2
        } else if name == "soic8" {
            8
        } else {
            24
        };
        let span_a = (pin(&ours, opposite).at[0] - a.at[0]).abs();
        let span_b = (pin(&reference, opposite).at[0] - b.at[0]).abs();
        let delta = (a.size[0] - b.size[0])
            .abs()
            .max((a.size[1] - b.size[1]).abs())
            .max((span_a - span_b).abs());
        crate::report(log,format!("footprint {name}: IPC nominal pad={:.4} x {:.4} mm centres={span_a:.4} mm; KiCad pad={:.4} x {:.4} mm centres={span_b:.4} mm; max_delta={delta:.4} mm within_0.05={} copper_pads={}",a.size[0],a.size[1],b.size[0],b.size[1],delta<=0.05+1e-9,ours.len()));
        if ours.len() > 2 {
            let pitch_a = (pin(&ours, 2).at[1] - a.at[1]).abs();
            let pitch_b = (pin(&reference, 2).at[1] - b.at[1]).abs();
            crate::report(
                log,
                format!("footprint {name}: row pitch IPC={pitch_a:.4} KiCad={pitch_b:.4} mm"),
            );
            assert!((pitch_a - pitch_b).abs() < 1e-9);
        }
        if name == "qfn32" {
            crate::report(
                log,
                format!(
                    "qfn32 exposed pad: IPC={:?} KiCad={:?} mm",
                    pin(&ours, 33).size,
                    pin(&reference, 33).size
                ),
            );
        }
        crate::report(log, format!("  reference: {source}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nominal_calculations_and_multiline_reader() {
        let p = pattern("soic8");
        assert!((p[0].size[0] - 1.95).abs() < 1e-9);
        assert!((p[0].size[1] - 0.6).abs() < 1e-9);
        let q = pattern("qfn32");
        assert_eq!(q.len(), 33);
        assert!((q[0].size[0] - 0.875).abs() < 1e-9);
        let text="(pad \"\" smd rect (at 0 0) (size 1 1) (layers \"F.Paste\"))\n(pad \"1\" smd roundrect\n (at -0.51 0)\n (size 0.54 0.64) (layers \"F.Cu\"))";
        let parsed = parse_kicad(text);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].at, [-0.51, 0.0]);
        assert_eq!(parsed[0].size, [0.54, 0.64]);
    }
}
