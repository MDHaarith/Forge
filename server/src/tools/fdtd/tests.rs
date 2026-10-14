use super::*;
use std::time::Instant;
fn refs() -> Value {
    serde_json::from_str(include_str!("../../../tests/fixtures/rf/accuracy.json")).unwrap()
}
fn uniform(dims: [usize; 3], size: [f64; 3]) -> mesh::Mesh {
    mesh::Mesh::new(std::array::from_fn(|q| {
        (0..dims[q])
            .map(|i| i as f64 * size[q] / (dims[q] - 1) as f64)
            .collect()
    }))
    .unwrap()
}
fn no_port() -> port::Port {
    port::Port {
        edges: vec![],
        resistance: 50.,
        length: 1.,
        sign: 1.,
        parallel: 1,
    }
}
#[test]
fn yee_mesh_is_courant_stable() {
    let mesh = mesh::Mesh::new(std::array::from_fn(|_| {
        (0..20).map(|i| i as f64 * 0.001).collect()
    }))
    .unwrap();
    assert!((mesh.dt * C0 * 3f64.sqrt() / 0.001 - 0.95).abs() < 1e-12);
}
#[test]
fn graded_mesh_preserves_feed_planes_and_bounds_spacing() {
    let lines = vec![-40., -2., -0.25, 0.25, 0.75, 1.333333, 1.833333, 8., 45.];
    let graded = mesh::smooth(lines.clone(), &vec![3.; lines.len() - 1]).unwrap();
    for x in lines {
        assert!(graded.contains(&x));
    }
    let widths: Vec<_> = graded.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(widths.iter().all(|&w| w <= 3. * (1. + 1e-10)));
    for w in widths.windows(2) {
        assert!(w[0] / w[1] <= 1.4 * (1. + 1e-10) && w[1] / w[0] <= 1.4 * (1. + 1e-10));
    }
}

#[test]
fn pec_cavity_te101_and_thread_determinism() {
    let start = Instant::now();
    let r = refs();
    let a = r["cavity"]["a_m"].as_f64().unwrap();
    let d = r["cavity"]["d_m"].as_f64().unwrap();
    let reference = C0 / 2. * (a.powi(-2) + d.powi(-2)).sqrt();
    assert!((reference - r["cavity"]["resonance_hz"].as_f64().unwrap()).abs() < 1e-3);
    let m = uniform([33, 3, 27], [a, 0.01, d]);
    let [nx, ny, nz] = m.dims();
    let mut e = vec![[0.; 3]; m.cells()];
    for k in 0..nz {
        for j in 0..ny - 1 {
            for i in 0..nx {
                e[(k * ny + j) * nx + i][1] = ((std::f64::consts::PI * m.axes[0][i] / a).sin()
                    * (std::f64::consts::PI * m.axes[2][k] / d).sin())
                    as f32;
            }
        }
    }
    let mut cfg = engine::Config::antenna(reference, reference, false);
    cfg.pml = 0;
    cfg.surface = false;
    cfg.steps = 3072;
    cfg.end = 0.;
    cfg.threads = 1;
    cfg.probes = vec![([nx / 2, 1, nz / 2], 1)];
    let c = vec![[port::material(1., 0., m.dt); 3]; m.cells()];
    let one = engine::run(&m, c.clone(), &no_port(), &cfg, Some(e.clone())).unwrap();
    cfg.threads = 3;
    let three = engine::run(&m, c, &no_port(), &cfg, Some(e.clone())).unwrap();
    assert_eq!(one.probes, three.probes);
    assert_eq!(one.decay, three.decay);
    cfg.scalar = true;
    let scalar = engine::run(
        &m,
        vec![[port::material(1., 0., m.dt); 3]; m.cells()],
        &no_port(),
        &cfg,
        Some(e),
    )
    .unwrap();
    assert_eq!(one.probes, scalar.probes);
    assert_eq!(one.decay, scalar.decay);
    let f = (0..=400)
        .map(|i| reference * (0.99 + 0.02 * i as f64 / 400.))
        .max_by(|&a, &b| {
            port::dft(&one.probes[0], m.dt, 1., a)
                .norm()
                .total_cmp(&port::dft(&one.probes[0], m.dt, 1., b).norm())
        })
        .unwrap();
    let error = (f / reference - 1.).abs();
    eprintln!("ACCURACY cavity reference {reference:.6} Hz Forge {f:.6} Hz error {:.6}% runtime {:.3}s; 1/3 threads identical",error*100.,start.elapsed().as_secs_f64());
    assert!(error < r["cavity"]["relative_tolerance"].as_f64().unwrap());
}

#[test]
fn huygens_fields_are_collocated_across_slab_halos() {
    let m = mesh::Mesh::new(std::array::from_fn(|_| {
        vec![0., 0.001, 0.003, 0.006, 0.01, 0.015]
    }))
    .unwrap();
    let [nx, ny, nz] = m.dims();
    let plane = nx * ny;
    let split = 3;
    let linear = |p: [f64; 3], q: usize| p[0] + 2. * p[1] + 3. * p[2] + q as f64;
    let mut e = vec![[0.; 3]; m.cells()];
    let mut h = e.clone();
    for k in 0..nz {
        for j in 0..ny {
            for i in 0..nx {
                let xyz = [i, j, k];
                for q in 0..3 {
                    let ep = std::array::from_fn(|a| {
                        m.axes[a][xyz[a]] + if a == q { m.step(a, xyz[a]) / 2. } else { 0. }
                    });
                    let hp = std::array::from_fn(|a| {
                        m.axes[a][xyz[a]] + if a != q { m.step(a, xyz[a]) / 2. } else { 0. }
                    });
                    e[(k * ny + j) * nx + i][q] = linear(ep, q) as f32;
                    h[(k * ny + j) * nx + i][q] = (linear(hp, q) * ETA0) as f32;
                }
            }
        }
    }
    let surfaces = nf2ff::box_surface(&m, 1).unwrap();
    let zero = vec![[0.; 3]; plane];
    let mut lo: Vec<_> = surfaces
        .iter()
        .filter(|s| s.xyz[2] < split)
        .cloned()
        .collect();
    let mut hi: Vec<_> = surfaces
        .iter()
        .filter(|s| s.xyz[2] >= split)
        .cloned()
        .collect();
    nf2ff::accumulate(
        &mut lo,
        &e[..split * plane],
        &h[..split * plane],
        &e[split * plane..(split + 1) * plane],
        &zero,
        &h[split * plane..(split + 1) * plane],
        0,
        &m,
        Complex { re: 1., im: 0. },
        Complex { re: 1., im: 0. },
    );
    nf2ff::accumulate(
        &mut hi,
        &e[split * plane..],
        &h[split * plane..],
        &zero,
        &h[(split - 1) * plane..split * plane],
        &zero,
        split,
        &m,
        Complex { re: 1., im: 0. },
        Complex { re: 1., im: 0. },
    );
    for s in lo.iter().chain(&hi) {
        for q in 0..3 {
            if q != s.axis {
                assert!((s.e[q].re - linear(s.position, q)).abs() < 1e-6);
                assert!((s.h[q].re - linear(s.position, q)).abs() < 1e-6);
            }
        }
    }
}

#[test]
fn dielectric_loss_does_not_make_air_lossy_when_er_is_one() {
    let m = mesh::Mesh::new(std::array::from_fn(|_| {
        (0..21).map(|i| (i as f64 - 10.) * 0.0005).collect()
    }))
    .unwrap();
    let board = geometry::Board {
        name: "loss".into(),
        outline: geometry::rect([0., 0.], 2., 2., 0.),
        thickness: 1.,
        layers: vec![0., 1.],
        copper: vec![],
        pours: vec![],
        barrels: vec![],
        ground_under: 0.,
        feed: geometry::Feed {
            start: [0., 0., 0.],
            stop: [0., 0., 1.],
            axis: 2,
            sign: 1.,
            note: String::new(),
        },
    };
    let c = materials(&m, &board, 1., 0.02, 2.45e9);
    assert!(c[(10 * 21 + 10) * 21 + 10][2].decay < 1.);
    assert_eq!(c[(15 * 21 + 15) * 21 + 15][2].decay, 1.);
}

#[test]
#[ignore = "3D absorber accuracy benchmark; run in release"]
fn cpml_point_pulse_against_double_box() {
    let start = Instant::now();
    let sample = |n: usize| {
        let m = uniform([n + 1; 3], [n as f64 * 0.001; 3]);
        let mid = n / 2;
        let mut cfg = engine::Config::antenna(10e9, 10e9, false);
        cfg.steps = 768;
        cfg.end = 0.;
        cfg.surface = false;
        cfg.threads = 4;
        cfg.probes = vec![([mid + 3, mid, mid], 2)];
        let feed = geometry::Feed {
            start: [mid as f64; 3],
            stop: [mid as f64, mid as f64, (mid + 1) as f64],
            axis: 2,
            sign: 1.,
            note: String::new(),
        };
        let mut p = port::Port::new(&m, &feed, 50.).unwrap();
        let mut c = vec![[port::material(1., 0., m.dt); 3]; m.cells()];
        p.stamp(&m, &mut c).unwrap();
        let fields = engine::run(&m, c, &p, &cfg, None).unwrap();
        (fields.probes[0].clone(), fields.decay)
    };
    let (small, decay) = sample(32);
    let (large, _) = sample(64);
    let peak = large.iter().map(|v| v.abs()).fold(0f64, f64::max);
    let diff = small
        .iter()
        .zip(large)
        .map(|(a, b)| (a - b).abs())
        .fold(0f64, f64::max);
    let reflection = 20. * (diff / peak).log10();
    eprintln!("ACCURACY CPML reference < -40 dB Forge {reflection:.6} dB energy {decay:.6e} runtime {:.3}s",start.elapsed().as_secs_f64());
    assert!(
        reflection < refs()["cpml"]["reflection_limit_db"].as_f64().unwrap(),
        "reflection {reflection}"
    );
    assert!(decay < 1e-3);
}

fn plate_line(load: Option<f64>, short: bool, through_pml: bool) -> (mesh::Mesh, engine::Run, f64) {
    let height = 0.004;
    let width = ETA0 * height / 50.;
    let mut m = uniform([113, 3, 25], [0.112, width * 2. / 3., 0.012]);
    m.periodic_y = true;
    let [nx, ny, _] = m.dims();
    let lower = 8;
    let upper = 16;
    let begin = 16;
    let end = if through_pml { nx - 1 } else { 96 };
    let mut c = vec![[port::material(1., 0., m.dt); 3]; m.cells()];
    let pec = engine::Coeff {
        decay: 0.,
        curl: 0.,
    };
    for i in begin..=end {
        for j in 0..ny {
            for k in [lower, upper] {
                c[(k * ny + j) * nx + i][0] = pec;
                c[(k * ny + j) * nx + i][1] = pec;
            }
        }
    }
    let make_port = |x: usize| {
        let mut p = port::Port {
            edges: vec![],
            resistance: 50.,
            length: height,
            sign: 1.,
            parallel: ny,
        };
        for j in 0..ny {
            for k in lower..upper {
                p.edges.push(port::Edge {
                    xyz: [x, j, k],
                    axis: 2,
                    length: m.step(2, k),
                    drive: 0.,
                });
            }
        }
        p
    };
    let mut source = make_port(begin);
    source.stamp(&m, &mut c).unwrap();
    if let Some(r) = load {
        let mut p = make_port(end);
        p.resistance = r;
        p.stamp(&m, &mut c).unwrap();
    }
    if short {
        for j in 0..ny {
            for k in lower..upper {
                c[(k * ny + j) * nx + end][2] = pec;
            }
        }
    }
    let mut cfg = engine::Config::antenna(1e9, 1e9, false);
    cfg.surface = false;
    cfg.steps = 12_288;
    cfg.end = 0.;
    cfg.threads = 3;
    cfg.probes = vec![([32, 1, 12], 2), ([31, 1, 12], 4), ([32, 1, 12], 4)];
    let run = engine::run(&m, c, &source, &cfg, None).unwrap();
    (m, run, width)
}
#[test]
#[ignore = "transmission and load accuracy benchmark; run in release"]
fn parallel_plate_impedance_and_lumped_loads() {
    let started = Instant::now();
    let (m, traveling, width) = plate_line(None, false, true);
    let f = 1e9;
    let e = port::dft(&traveling.probes[0], m.dt, 1., f);
    let h = (port::dft(&traveling.probes[1], m.dt, 0.5, f)
        + port::dft(&traveling.probes[2], m.dt, 0.5, f))
        * 0.5;
    let impedance = e / (h * -1.) * (0.004 / width);
    // A very wide centred stripline has two parallel-plate channels in parallel.
    let wide = 100.;
    let b = 1.;
    let rf_reference = super::super::rfmath::stripline(wide, b, 1., 0.001).z0;
    let forge_wide = impedance.re / 50. * ETA0 * b / (4. * wide);
    eprintln!("ACCURACY line rfmath wide-stripline reference {rf_reference:.9} ohm Forge {forge_wide:.9} ohm; TEM port reference 50 ohm Forge {:.9}+j{:.9} ohm runtime {:.3}s",impedance.re,impedance.im,started.elapsed().as_secs_f64());
    assert!(
        (forge_wide / rf_reference - 1.).abs()
            < refs()["line"]["relative_tolerance"].as_f64().unwrap()
    );
    let reflection = |run: &engine::Run| {
        let s = port::spectrum(&run.voltage, &run.current, m.dt, 1e9, 1e9).unwrap();
        s.s[200].norm().sqrt()
    };
    let (_, matched, _) = plate_line(Some(50.), false, false);
    let matched_s = reflection(&matched);
    let (_, open, _) = plate_line(None, false, false);
    let open_s = reflection(&open);
    let (_, short, _) = plate_line(None, true, false);
    let short_s = reflection(&short);
    eprintln!("ACCURACY loads references 0/1/1 Forge {matched_s:.9}/{open_s:.9}/{short_s:.9} runtime {:.3}s",started.elapsed().as_secs_f64());
    assert!(matched_s < 0.05, "matched S11 {matched_s}");
    assert!((open_s - 1.).abs() < 0.05, "open S11 {open_s}");
    assert!((short_s - 1.).abs() < 0.05, "short S11 {short_s}");
}

#[test]
#[ignore = "full-wave FR-4 patch benchmark; run in release"]
fn fr4_patch_transmission_line_reference() {
    let started = Instant::now();
    let r = refs();
    let f0 = r["patch"]["frequency_hz"].as_f64().unwrap();
    let er = r["patch"]["er"].as_f64().unwrap();
    let h = r["patch"]["height_mm"].as_f64().unwrap();
    let width = C0 / (2. * f0) * (2. / (er + 1.)).sqrt() * 1e3;
    let eeff = (er + 1.) / 2. + (er - 1.) / 2. * (1. + 12. * h / width).powf(-0.5);
    let extension =
        0.412 * h * (eeff + 0.3) * (width / h + 0.264) / ((eeff - 0.258) * (width / h + 0.8));
    let length = C0 / (2. * f0 * eeff.sqrt()) * 1e3 - 2. * extension;
    let circuit = serde_json::json!([
        {"type":"pcb_board","width":60,"height":60,"thickness":h,"num_layers":2},
        {"type":"source_component","source_component_id":"s","name":"PATCH"},
        {"type":"pcb_component","pcb_component_id":"c","source_component_id":"s"},
        {"type":"pcb_trace","pcb_component_id":"c","route":[{"route_type":"wire","x":0,"y":0,"width":0.8,"layer":"top"}]},
        {"type":"pcb_smtpad","pcb_component_id":"c","shape":"rect","x":-length/6.,"y":0,"width":0.8,"height":0.8,"layer":"top","port_hints":["pin1"]},
        {"type":"pcb_smtpad","pcb_component_id":"c","shape":"rect","x":0,"y":0,"width":length,"height":width,"layer":"top"},
        {"type":"pcb_copper_pour","shape":"rect","center":{"x":0,"y":0},"width":60,"height":60,"layer":"bottom"}
    ]);
    let out = simulate(&circuit, "PATCH", f0, 1.2e9, er, 0.02, false).unwrap();
    let f = out.result["resonance"].as_f64().unwrap();
    let error = (f / f0 - 1.).abs();
    eprintln!("ACCURACY patch reference {f0:.6} Hz Forge {f:.6} Hz error {:.6}% runtime {:.3}s W {width:.6} L {length:.6} h {h}",error*100.,started.elapsed().as_secs_f64());
    eprintln!("PATCH RESULT {}", out.result);
    assert!(error < r["patch"]["relative_tolerance"].as_f64().unwrap());
    let nominal = r["patch"]["tutorial_nominal_hz"].as_f64().unwrap();
    assert!((f / nominal - 1.).abs() < 0.03);
    assert!(
        out.result["peak_dir"][0].as_f64().unwrap()
            <= r["patch"]["tutorial_broadside_max_theta_deg"]
                .as_f64()
                .unwrap()
    );
}
