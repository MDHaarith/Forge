use super::{geometry::Board, mesh::Mesh, nf2ff::FarField, port::Spectrum, Simulation};
use crate::jsfmt::number;
use serde_json::json;
use std::fmt::Write;

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn s11_svg(s: &Spectrum, f0: f64, name: &str) -> String {
    let lo = (-30f64).min((s.db.iter().copied().fold(f64::INFINITY, f64::min) / 5.).floor() * 5.);
    let fx = |f: f64| 56. + (f - s.f[0]) / (s.f[400] - s.f[0]) * 568.;
    let fy = |v: f64| 30. + v / lo * 290.;
    let mut out=String::from("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 640 360\" font-family=\"sans-serif\" font-size=\"12\"><rect width=\"640\" height=\"360\" fill=\"#fff\"/>\n");
    for d in (lo as i32..=0).rev().step_by(5) {
        writeln!(out,"<line x1=\"56\" x2=\"624\" y1=\"{:.1}\" y2=\"{:.1}\" stroke=\"#ddd\"/><text x=\"50\" y=\"{:.1}\" text-anchor=\"end\">{d}</text>",fy(d as f64),fy(d as f64),fy(d as f64)+4.).unwrap();
    }
    let step = 10f64.powf(((s.f[400] - s.f[0]) / 1e6 / 4.).log10().floor()) * 1e6;
    let mut f = (s.f[0] / step).ceil() * step;
    while f <= s.f[400] + 1. {
        writeln!(out,"<line x1=\"{:.1}\" x2=\"{:.1}\" y1=\"30\" y2=\"320\" stroke=\"#eee\"/><text x=\"{:.1}\" y=\"336\" text-anchor=\"middle\">{}</text>",fx(f),fx(f),fx(f),number(f/1e6)).unwrap();
        f += step;
    }
    writeln!(out,"<line x1=\"56\" x2=\"624\" y1=\"{:.1}\" y2=\"{:.1}\" stroke=\"#c33\" stroke-dasharray=\"5 4\"/><line x1=\"{:.1}\" x2=\"{:.1}\" y1=\"30\" y2=\"320\" stroke=\"#36c\" stroke-dasharray=\"3 3\"/>",fy(-10.),fy(-10.),fx(f0),fx(f0)).unwrap();
    out.push_str("<polyline points=\"");
    for (&f, &v) in s.f.iter().zip(&s.db) {
        write!(out, "{:.1},{:.1} ", fx(f), fy(v.max(lo))).unwrap();
    }
    write!(out,"\" fill=\"none\" stroke=\"#111\" stroke-width=\"2\"/><text x=\"56\" y=\"18\" font-weight=\"bold\">S11 of {}</text><text x=\"340\" y=\"352\" text-anchor=\"middle\">Frequency (MHz)</text><text x=\"14\" y=\"175\" transform=\"rotate(-90 14 175)\" text-anchor=\"middle\">S11 (dB)</text></svg>\n",escape(name)).unwrap();
    out
}
fn pattern_svg(gain: &[f64], peak: f64, name: &str, f0: f64) -> String {
    let top = (peak / 5.).ceil() * 5.;
    let floor = top - 30.;
    let rad = |g: f64| 160. * (g.max(floor) - floor) / 30.;
    let mut out=String::from("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 560 440\" font-family=\"sans-serif\" font-size=\"12\"><rect width=\"560\" height=\"440\" fill=\"#fff\"/>\n");
    for d in (0..=30).step_by(10) {
        let g = floor + d as f64;
        writeln!(out,"<circle cx=\"210\" cy=\"220\" r=\"{:.1}\" fill=\"none\" stroke=\"#ddd\"/><text x=\"213\" y=\"{:.1}\" fill=\"#888\">{} dBi</text>",rad(g),232.-rad(g),number(g)).unwrap();
    }
    for a in (0..360).step_by(30) {
        let (s, c) = (a as f64).to_radians().sin_cos();
        writeln!(out,"<line x1=\"210\" y1=\"220\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"#eee\"/><text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" fill=\"#888\">{a}°</text>",210.+160.*s,220.-160.*c,210.+174.*s,224.-174.*c).unwrap();
    }
    let xy: Vec<_> = (0..72)
        .map(|p| (90. - p as f64 * 5., gain[18 * 72 + p]))
        .collect();
    let cut = |p: usize| {
        let mut v: Vec<_> = (0..=36)
            .map(|t| (t as f64 * 5., gain[t * 72 + p]))
            .collect();
        v.extend(
            (0..=36)
                .rev()
                .map(|t| (360. - t as f64 * 5., gain[t * 72 + (p + 36) % 72])),
        );
        v
    };
    for (i, (label, color, points)) in [
        ("XY plane (board)", "#111", xy),
        ("XZ plane", "#c33", cut(0)),
        ("YZ plane", "#36c", cut(18)),
    ]
    .iter()
    .enumerate()
    {
        out.push_str("<polygon points=\"");
        for &(a, g) in points {
            let (s, c) = a.to_radians().sin_cos();
            write!(out, "{:.1},{:.1} ", 210. + rad(g) * s, 220. - rad(g) * c).unwrap();
        }
        writeln!(out,"\" fill=\"none\" stroke=\"{color}\" stroke-width=\"2\"/><line x1=\"420\" x2=\"440\" y1=\"{}\" y2=\"{}\" stroke=\"{color}\" stroke-width=\"2\"/><text x=\"446\" y=\"{}\">{label}</text>",60+20*i,60+20*i,64+20*i).unwrap();
    }
    writeln!(
        out,
        "<text x=\"10\" y=\"18\" font-weight=\"bold\">{}: gain at {} MHz</text></svg>",
        escape(name),
        number(f0 / 1e6)
    )
    .unwrap();
    out
}
pub fn artifacts(
    board: &Board,
    mesh: &Mesh,
    s: &Spectrum,
    far: &FarField,
    f0: f64,
    span: f64,
) -> Result<Simulation, String> {
    let min =
        s.db.iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| a.total_cmp(b))
            .unwrap()
            .0;
    let band = if s.db[min] < -10. {
        let mut lo = min;
        let mut hi = min;
        while lo > 0 && s.db[lo - 1] < -10. {
            lo -= 1;
        }
        while hi < 400 && s.db[hi + 1] < -10. {
            hi += 1;
        }
        json!([s.f[lo], s.f[hi], lo == 0 || hi == 400])
    } else {
        json!(null)
    };
    let accepted = (s.v0 * s.i0.conj()).re * 0.5;
    let inc = (s.v0 + s.i0 * 50.) * 0.5;
    let incident = inc.norm() / 100.;
    let rad_eff = if accepted > 0. {
        far.power / accepted
    } else {
        0.
    };
    let tot_eff = if incident > 0. {
        far.power / incident
    } else {
        0.
    };
    let gain: Vec<_> = far
        .intensity
        .iter()
        .map(|u| {
            10. * (u / far.intensity[far.peak] * far.directivity * rad_eff.max(1e-9))
                .max(1e-12)
                .log10()
        })
        .collect();
    let peak = gain[far.peak];
    let stem: String = board
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let stem = if stem.is_empty() { "antenna" } else { &stem };
    let files = vec![
        format!("{stem}-s11.svg"),
        format!("{stem}-pattern.svg"),
        format!("{stem}.s1p"),
        format!("{stem}.json"),
    ];
    let result = json!({"antenna":board.name,"port":board.feed.note,"cells":mesh.cells(),"f0":f0,"span":[f0-span/2.,f0+span/2.],
        "resonance":s.f[min],"s11_min_db":s.db[min],"band":band,"s11_f0_db":s.db[200],"zin_f0":[s.zin[200].re,s.zin[200].im],
        "zin_res":[s.zin[min].re,s.zin[min].im],"directivity_dbi":10.*far.directivity.log10(),"peak_gain_dbi":peak,
        "peak_dir":[(far.peak/72) as f64*5.,(far.peak%72) as f64*5.],"radiation_efficiency":rad_eff,"total_efficiency":tot_eff,
        "ground_under":board.ground_under,"files":files});
    let mut touchstone = format!(
        "! Forge FDTD simulation of {}\n# Hz S RI R 50\n",
        board.name.replace(['\r', '\n'], " ")
    );
    for (&f, s) in s.f.iter().zip(&s.s) {
        writeln!(touchstone, "{f:.0} {:.6} {:.6}", s.re, s.im).unwrap();
    }
    let json = crate::jsfmt::stringify_pretty(&result).map_err(|e| e.to_string())?;
    let artifacts = files
        .into_iter()
        .zip([
            s11_svg(s, f0, &board.name),
            pattern_svg(&gain, peak, &board.name, f0),
            touchstone,
            json,
        ])
        .collect();
    Ok(Simulation { result, artifacts })
}
