#[derive(Clone, Debug, PartialEq)]
pub struct Pad {
    pub number: String,
    pub at: [f64; 2],
    pub size: [f64; 2],
    pub drill: Option<[f64; 2]>,
    pub round: bool,
    pub plated: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Footprint {
    pub pads: Vec<Pad>,
    pub courtyard: [f64; 4],
    pub height: f64,
    pub body: [f64; 4],
    pub silkscreen: Vec<[f64; 2]>,
    pub pin1: [f64; 2],
}

const NAMES: &str = "0402, 0603, 0805, 1206, sot23, sot23_5, sot223, soicN (even 8..16), tssopN (even 8..28), qfnN (multiples of 4, 16..48), dipN (even 4..40), bgaN (square), pinheader_1xN, pinheader_2xN, crystal_hc49, crystal_3225, button_6x6, sod123, usb_c_16, usb_micro_b, jst_ph_N (2..16), barrel_jack";

#[derive(Clone, Copy)]
struct Range(f64, f64);

impl Range {
    fn tol(self) -> f64 {
        self.1 - self.0
    }
}

// IPC-7351B nominal density, F=0.1 mm and P=0.05 mm; inputs documented in spikes/pcb/README.md.
fn ipc(l: Range, s: Range, cs: f64, w: Range, goals: [f64; 3], step: f64) -> [f64; 3] {
    let round = |x: f64| (x / step).round() * step;
    let rss = |c: f64| (c * c + 0.1_f64.powi(2) + 0.05_f64.powi(2)).sqrt();
    let z = round(l.0 + 2.0 * goals[0] + rss(l.tol()));
    let g = round(s.1 - 2.0 * goals[1] - rss(cs));
    let x = round(w.0 + 2.0 * goals[2] + rss(w.tol()));
    [(z - g) / 2.0, x, (z + g) / 2.0]
}

fn leads(l: Range, t: Range, w: Range, goals: [f64; 3]) -> [f64; 3] {
    let cs = (l.tol().powi(2) + 2.0 * t.tol().powi(2)).sqrt();
    let s_min = l.0 - 2.0 * t.1;
    let s_max = l.1 - 2.0 * t.0;
    let s = Range(s_min, s_max - ((s_max - s_min) - cs) / 2.0);
    ipc(l, s, cs, w, goals, 0.05)
}

fn pad(number: usize, at: [f64; 2], size: [f64; 2], drill: Option<f64>) -> Pad {
    // shortcut: rectangular SMD pads, upgrade to rounded rectangles if a fab or paste stencil needs them.
    Pad {
        number: number.to_string(),
        at,
        size,
        drill: drill.map(|d| [d, d]),
        round: drill.is_some(),
        plated: true,
    }
}

fn pad_ext(
    number: impl Into<String>,
    at: [f64; 2],
    size: [f64; 2],
    drill: Option<[f64; 2]>,
    round: bool,
    plated: bool,
) -> Pad {
    Pad {
        number: number.into(),
        at,
        size,
        drill,
        round,
        plated,
    }
}

fn pair(dim: [f64; 3], drill: Option<f64>) -> Vec<Pad> {
    (0..2)
        .map(|i| {
            pad(
                i + 1,
                [(i as f64 - 0.5) * dim[2], 0.0],
                [dim[0], dim[1]],
                drill,
            )
        })
        .collect()
}

fn rows(n: usize, pitch: f64, dim: [f64; 3], drill: Option<f64>) -> Vec<Pad> {
    let mut pads = Vec::new();
    for i in 0..n / 2 {
        let y = (i as f64 - (n / 2 - 1) as f64 / 2.0) * pitch;
        pads.push(pad(i + 1, [-dim[2] / 2.0, y], [dim[0], dim[1]], drill));
        pads.push(pad(n - i, [dim[2] / 2.0, y], [dim[0], dim[1]], drill));
    }
    pads.sort_by_key(|p| p.number.parse::<usize>().unwrap());
    pads
}

fn count(name: &str, prefix: &str, min: usize, max: usize, multiple: usize) -> Option<usize> {
    let suffix = name.strip_prefix(prefix)?;
    if suffix.starts_with('0') || !suffix.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let n = suffix.parse::<usize>().ok()?;
    (n >= min && n <= max && n % multiple == 0).then_some(n)
}

fn box_size(size: [f64; 2]) -> [f64; 4] {
    [-size[0] / 2.0, -size[1] / 2.0, size[0] / 2.0, size[1] / 2.0]
}

#[allow(dead_code)]
pub(super) fn package(pads: Vec<Pad>, size: [f64; 2], height: f64) -> Footprint {
    package_body(pads, box_size(size), height)
}

pub(super) fn package_body(pads: Vec<Pad>, body: [f64; 4], height: f64) -> Footprint {
    let mut courtyard = body;
    for p in &pads {
        for axis in 0..2 {
            courtyard[axis] = courtyard[axis].min(p.at[axis] - p.size[axis] / 2.0);
            courtyard[axis + 2] = courtyard[axis + 2].max(p.at[axis] + p.size[axis] / 2.0);
        }
    }
    let margin = if pads.iter().any(|p| p.drill.is_some()) {
        0.5
    } else {
        0.25
    };
    courtyard = [
        courtyard[0] - margin,
        courtyard[1] - margin,
        courtyard[2] + margin,
        courtyard[3] + margin,
    ];
    // Keep the outline beyond the copper instead of drawing through perimeter pads.
    let silk = [
        courtyard[0] + 0.1,
        courtyard[1] + 0.1,
        courtyard[2] - 0.1,
        courtyard[3] - 0.1,
    ];
    let silkscreen = vec![
        [silk[0], silk[1]],
        [silk[2], silk[1]],
        [silk[2], silk[3]],
        [silk[0], silk[3]],
        [silk[0], silk[1]],
    ];
    let pin1 = [silk[0], silk[1]];
    Footprint {
        pads,
        courtyard,
        height,
        body,
        silkscreen,
        pin1,
    }
}

/// Generate one canonical nominal package, without any installed footprint library.
pub fn footprint(name: &str) -> Result<Footprint, String> {
    let (pads, body, height) = match name {
        "0402" => (
            pair(
                ipc(
                    Range(1.0, 1.1),
                    Range(0.4, 0.7),
                    0.3,
                    Range(0.48, 0.6),
                    [0.2, 0.0, 0.0],
                    0.02,
                ),
                None,
            ),
            box_size([1.0, 0.5]),
            0.5,
        ),
        "0603" => (
            pair(
                ipc(
                    Range(1.5, 1.7),
                    Range(0.7, 1.11),
                    0.41,
                    Range(0.7, 0.95),
                    [0.35, 0.0, 0.0],
                    0.05,
                ),
                None,
            ),
            box_size([1.6, 0.8]),
            0.6,
        ),
        // IPC-SM-782 nominal land dimensions, matching KiCad's metric resistor packages.
        "0805" => (pair([1.025, 1.4, 1.825], None), box_size([2.0, 1.25]), 0.6),
        "1206" => (pair([1.125, 1.75, 2.925], None), box_size([3.2, 1.6]), 0.6),
        "sot23" => (
            vec![
                pad(1, [-0.9375, -0.95], [1.475, 0.6], None),
                pad(2, [-0.9375, 0.95], [1.475, 0.6], None),
                pad(3, [0.9375, 0.0], [1.475, 0.6], None),
            ],
            box_size([1.3, 2.9]),
            1.1,
        ),
        "sot23_5" => (
            vec![
                pad(1, [-1.1375, -0.95], [1.325, 0.6], None),
                pad(2, [-1.1375, 0.0], [1.325, 0.6], None),
                pad(3, [-1.1375, 0.95], [1.325, 0.6], None),
                pad(4, [1.1375, 0.95], [1.325, 0.6], None),
                pad(5, [1.1375, -0.95], [1.325, 0.6], None),
            ],
            box_size([1.6, 2.9]),
            1.1,
        ),
        "sot223" => (
            vec![
                pad(1, [-3.15, -2.3], [2.0, 1.5], None),
                pad(2, [-3.15, 0.0], [2.0, 1.5], None),
                pad(3, [-3.15, 2.3], [2.0, 1.5], None),
                pad(2, [3.15, 0.0], [2.0, 3.8], None),
            ],
            box_size([3.5, 6.5]),
            1.8,
        ),
        "sod123" => (pair([0.9, 1.2, 3.3], None), box_size([2.7, 1.6]), 1.35),
        "crystal_hc49" => (
            pair([1.5, 1.5, 4.88], Some(0.8)),
            box_size([11.05, 4.65]),
            13.46,
        ),
        "crystal_3225" => (
            vec![
                pad(1, [-1.1, -0.85], [1.4, 1.2], None),
                pad(2, [1.1, -0.85], [1.4, 1.2], None),
                pad(3, [1.1, 0.85], [1.4, 1.2], None),
                pad(4, [-1.1, 0.85], [1.4, 1.2], None),
            ],
            box_size([3.2, 2.5]),
            0.8,
        ),
        "button_6x6" => (
            vec![
                pad(1, [-3.25, -2.25], [2.0, 2.0], Some(1.1)),
                pad(1, [3.25, -2.25], [2.0, 2.0], Some(1.1)),
                pad(2, [-3.25, 2.25], [2.0, 2.0], Some(1.1)),
                pad(2, [3.25, 2.25], [2.0, 2.0], Some(1.1)),
            ],
            box_size([6.0, 6.0]),
            4.3,
        ),
        "usb_c_16" => {
            // HRO TYPE-C-31-M-12 USB 2.0 Type-C receptacle, SMD
            let mut pads = Vec::new();
            let signals = [
                ("A1", -3.25, 0.6),
                ("A4", -2.45, 0.6),
                ("A5", -1.25, 0.3),
                ("A6", -0.25, 0.3),
                ("A7", 0.25, 0.3),
                ("A8", 1.25, 0.3),
                ("A9", 2.45, 0.6),
                ("A12", 3.25, 0.6),
                ("B1", 3.25, 0.6),
                ("B4", 2.45, 0.6),
                ("B5", 1.75, 0.3),
                ("B6", 0.75, 0.3),
                ("B7", -0.75, 0.3),
                ("B8", -1.75, 0.3),
                ("B9", -2.45, 0.6),
                ("B12", -3.25, 0.6),
            ];
            for (num, x, w) in signals {
                pads.push(pad_ext(num, [x, -4.045], [w, 1.45], None, false, true));
            }
            // 4 shell tabs with oval slotted plated drills
            for (x, y, h, dh) in [
                (-4.32, -3.13, 2.1, 1.7),
                (-4.32, 1.05, 1.6, 1.2),
                (4.32, -3.13, 2.1, 1.7),
                (4.32, 1.05, 1.6, 1.2),
            ] {
                pads.push(pad_ext("S1", [x, y], [1.0, h], Some([0.6, dh]), true, true));
            }
            // 2 non-plated locating holes
            for x in [-2.89, 2.89] {
                pads.push(pad_ext(
                    "",
                    [x, -2.6],
                    [0.65, 0.65],
                    Some([0.65, 0.65]),
                    true,
                    false,
                ));
            }
            (pads, [-4.47, -3.65, 4.47, 3.65], 3.16)
        }
        "usb_micro_b" => {
            // Molex 105017-0001 USB Micro-B, SMD
            let mut pads = Vec::new();
            for (i, x) in [(-1.3, 1), (-0.65, 2), (0.0, 3), (0.65, 4), (1.3, 5)] {
                pads.push(pad_ext(
                    x.to_string(),
                    [i, -1.4625],
                    [0.4, 1.35],
                    None,
                    false,
                    true,
                ));
            }
            pads.push(pad_ext(
                "6",
                [-3.5, 1.2375],
                [1.2, 1.9],
                Some([0.6, 1.3]),
                true,
                true,
            ));
            pads.push(pad_ext("6", [-2.9, 1.2375], [1.2, 1.9], None, false, true));
            pads.push(pad_ext(
                "6",
                [-2.5, -1.4625],
                [1.45, 1.45],
                Some([0.85, 0.85]),
                true,
                true,
            ));
            pads.push(pad_ext("6", [-1.0, 1.2375], [1.5, 1.9], None, false, true));
            pads.push(pad_ext("6", [1.0, 1.2375], [1.5, 1.9], None, false, true));
            pads.push(pad_ext(
                "6",
                [2.5, -1.4625],
                [1.45, 1.45],
                Some([0.85, 0.85]),
                true,
                true,
            ));
            pads.push(pad_ext("6", [2.9, 1.2375], [1.2, 1.9], None, false, true));
            pads.push(pad_ext(
                "6",
                [3.5, 1.2375],
                [1.2, 1.9],
                Some([0.6, 1.3]),
                true,
                true,
            ));
            (pads, [-3.75, -1.6125, 3.75, 3.3875], 2.8)
        }
        "barrel_jack" => {
            // CUI PJ-102AH horizontal 2.1 mm DC barrel jack
            let pads = vec![
                pad_ext("1", [0.0, 0.0], [2.6, 2.6], Some([1.6, 1.6]), false, true),
                pad_ext("2", [0.0, 6.0], [2.6, 2.6], Some([1.6, 1.6]), true, true),
                pad_ext("3", [4.7, 3.0], [2.6, 2.6], Some([1.6, 1.6]), true, true),
            ];
            (pads, [-4.5, -0.7, 4.5, 13.7], 11.0)
        }
        _ => {
            if let Some(n) = count(name, "soic", 8, 16, 2) {
                let dim = leads(
                    Range(5.8, 6.2),
                    Range(0.4, 1.27),
                    Range(0.31, 0.51),
                    [0.35, 0.35, 0.03],
                );
                (
                    rows(n, 1.27, dim, None),
                    box_size([3.9, n as f64 / 2.0 * 1.27 - 0.18]),
                    1.75,
                )
            } else if let Some(n) = count(name, "tssop", 8, 28, 2) {
                let (dim, width) = if n == 8 {
                    ([1.1, 0.4, 4.3], 3.0)
                } else {
                    ([1.475, 0.4, 5.725], 4.4)
                };
                (
                    rows(n, 0.65, dim, None),
                    box_size([width, (n as f64 / 2.0 * 0.65 + 0.5).ceil()]),
                    1.2,
                )
            } else if let Some(n) = count(name, "qfn", 16, 48, 4) {
                // Canonical square packages; exposed pads vary by package even at the same pin count.
                let (size, ep) = match n {
                    16 => (3.0, 1.7),
                    20 => (3.5, 2.0),
                    24 => (4.0, 2.6),
                    28 | 32 => (5.0, 3.1),
                    36 => (6.0, 4.1),
                    40 => (6.0, 4.6),
                    44 => (7.0, 5.15),
                    _ => (7.0, 5.1),
                };
                let dim = leads(
                    Range(size - 0.1, size + 0.1),
                    Range(0.3, 0.5),
                    Range(0.18, 0.3),
                    [0.3, 0.0, -0.04],
                );
                let mut pads = Vec::new();
                for i in 0..n / 4 {
                    let y = (i as f64 - (n / 4 - 1) as f64 / 2.0) * 0.5;
                    pads.push(pad(i + 1, [-dim[2] / 2.0, y], [dim[0], dim[1]], None));
                    pads.push(pad(
                        n / 2 + n / 4 - i,
                        [dim[2] / 2.0, y],
                        [dim[0], dim[1]],
                        None,
                    ));
                    pads.push(pad(
                        n / 4 + 1 + i,
                        [y, dim[2] / 2.0],
                        [dim[1], dim[0]],
                        None,
                    ));
                    pads.push(pad(n - i, [y, -dim[2] / 2.0], [dim[1], dim[0]], None));
                }
                pads.sort_by_key(|p| p.number.parse::<usize>().unwrap());
                pads.push(pad(n + 1, [0.0, 0.0], [ep; 2], None));
                (pads, box_size([size; 2]), 1.0)
            } else if let Some(n) = count(name, "dip", 4, 40, 2) {
                let spacing = if n >= 24 { 15.24 } else { 7.62 };
                (
                    rows(n, 2.54, [1.6, 1.6, spacing], Some(0.8)),
                    box_size([spacing - 1.02, n as f64 / 2.0 * 2.54]),
                    4.8,
                )
            // shortcut: cap BGA at 4096 balls and headers at 1024 rows, raise limits when a real package needs them.
            } else if let Some(n) = count(name, "bga", 1, 4096, 1)
                .filter(|n| ((*n as f64).sqrt() as usize).pow(2) == *n)
            {
                let side = (n as f64).sqrt() as usize;
                let pads = (0..n)
                    .map(|i| {
                        pad(
                            i + 1,
                            [
                                (i % side) as f64 * 0.8 - (side - 1) as f64 * 0.4,
                                (i / side) as f64 * 0.8 - (side - 1) as f64 * 0.4,
                            ],
                            [0.4; 2],
                            None,
                        )
                    })
                    .collect();
                (pads, box_size([side as f64 * 0.8 + 0.8; 2]), 1.2)
            } else if let Some((columns, n)) = count(name, "pinheader_1x", 1, 1024, 1)
                .map(|n| (1, n))
                .or_else(|| count(name, "pinheader_2x", 1, 1024, 1).map(|n| (2, n)))
            {
                let pads = (0..n * columns)
                    .map(|i| {
                        pad(
                            i + 1,
                            [
                                (i % columns) as f64 * 2.54 - (columns - 1) as f64 * 1.27,
                                (i / columns) as f64 * 2.54 - (n - 1) as f64 * 1.27,
                            ],
                            [1.7; 2],
                            Some(1.0),
                        )
                    })
                    .collect();
                (
                    pads,
                    box_size([columns as f64 * 2.54, n as f64 * 2.54]),
                    8.5,
                )
            } else if let Some(n) = count(name, "jst_ph_", 2, 16, 1) {
                let mut pads = Vec::new();
                pads.push(pad_ext(
                    "1",
                    [0.0, 0.0],
                    [1.2, 1.75],
                    Some([0.75, 0.75]),
                    false,
                    true,
                ));
                for i in 2..=n {
                    pads.push(pad_ext(
                        i.to_string(),
                        [(i - 1) as f64 * 2.0, 0.0],
                        [1.2, 1.75],
                        Some([0.75, 0.75]),
                        true,
                        true,
                    ));
                }
                (pads, [-1.95, -1.7, 2.0 * (n - 1) as f64 + 1.95, 2.8], 6.0)
            } else {
                return Err(format!("unknown footprint {name}; use one of: {NAMES}"));
            }
        }
    };
    Ok(package_body(pads, body, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_have_valid_pads_and_enclosing_courtyards() {
        for name in [
            "0402",
            "0603",
            "0805",
            "1206",
            "sot23",
            "sot23_5",
            "sot223",
            "soic16",
            "tssop28",
            "qfn16",
            "qfn48",
            "dip40",
            "bga64",
            "pinheader_1x2",
            "pinheader_2x4",
            "crystal_hc49",
            "crystal_3225",
            "button_6x6",
            "sod123",
            "usb_c_16",
            "usb_micro_b",
            "jst_ph_2",
            "jst_ph_16",
            "barrel_jack",
        ] {
            let fp = footprint(name).unwrap();
            for p in fp.pads {
                for axis in 0..2 {
                    assert!(p.size[axis] > 0.0);
                    assert!(p.at[axis] - p.size[axis] / 2.0 > fp.courtyard[axis]);
                    assert!(p.at[axis] + p.size[axis] / 2.0 < fp.courtyard[axis + 2]);
                }
                assert!(p.drill.is_none_or(|d| d[0] > 0.0
                    && d[1] > 0.0
                    && d[0] <= p.size[0]
                    && d[1] <= p.size[1]));
            }
        }
        for name in [
            "soic7",
            "soic18",
            "qfn18",
            "dip41",
            "bga63",
            "pinheader_1x0",
            "soic08",
            "jst_ph_1",
            "jst_ph_17",
            "jst_ph_02",
        ] {
            assert!(footprint(name).is_err());
        }
    }
}
