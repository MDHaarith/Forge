use super::spec::Board;

// shortcut: 1 oz copper only, expose copper thickness when other foil weights are supported.
const COPPER: f64 = 0.035;

pub(super) fn height(board: &Board) -> f64 {
    if board.layers == 4 { board.prepreg } else { board.thickness - 2. * COPPER }
}

fn z0(w: f64, h: f64, er: f64) -> f64 {
    87. / (er + 1.41).sqrt() * (5.98 * h / (0.8 * w + COPPER)).ln()
}

pub(super) fn report(spec: &super::Spec) -> Result<Option<String>, String> {
    let n = crate::jsfmt::number;
    let mut targets = vec![];
    for (name, class) in &spec.netclasses {
        if let Some(z) = class.impedance {
            targets.push((name, format!("{name} {} ohm -> {} mm", n(z), n(width(&spec.board, z, None)?))));
        }
    }
    for (name, pair) in &spec.diffpairs {
        if let Some(z) = pair.impedance {
            targets.push((name, format!("{name} {} ohm differential -> {} mm, gap {} mm", n(z), n(spec.pair_width(pair)?), n(spec.pair_gap(pair)))));
        }
    }
    targets.sort_by(|a, b| a.0.cmp(b.0));
    Ok((!targets.is_empty()).then(|| format!("Impedance: {}", targets.into_iter().map(|(_, text)| text).collect::<Vec<_>>().join("; "))))
}

pub(super) fn width(board: &Board, target: f64, gap: Option<f64>) -> Result<f64, String> {
    let h = height(board);
    let z = |w| z0(w, h, board.er) * gap.map_or(1., |s| 2. * (1. - 0.48 * (-0.96 * s / h).exp()));
    let (mut lo, mut hi) = (0.05, 5.);
    if h <= 0. || !z(lo).is_finite() || target > z(lo) || target < z(hi) {
        return Err(format!("impedance {} ohm needs a trace width outside 0.05-5 mm", crate::jsfmt::number(target)));
    }
    for _ in 0..60 {
        let mid = (lo + hi) / 2.;
        if z(mid) > target { lo = mid; } else { hi = mid; }
    }
    // shortcut: stripline not modelled; inner layers reuse the microstrip width, add stripline with a full stackup.
    Ok(((lo + hi) / 2. * 1000.).round() / 1000.)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc2141_reference_widths_and_range() {
        for (layers, expected) in [(4, 0.344), (2, 2.785)] {
            let spec = super::super::parse(&format!(r#"{{"board":{{"width":30,"height":20,"layers":{layers}}}}}"#)).unwrap();
            assert!((width(&spec.board, 50., None).unwrap() - expected).abs() < 0.002);
            assert_eq!(width(&spec.board, 1000., None).unwrap_err(), "impedance 1000 ohm needs a trace width outside 0.05-5 mm");
            let w = width(&spec.board, 90., Some(0.15)).unwrap();
            let z = 2. * z0(w, height(&spec.board), spec.board.er) * (1. - 0.48 * (-0.96 * 0.15 / height(&spec.board)).exp());
            assert!((z - 90.).abs() < 0.1);
        }
    }
}
