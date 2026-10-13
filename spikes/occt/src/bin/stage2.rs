//! Stage 2 spike: loft, sweep, shell, mirror, helix thread, intersection volume and hidden-line scenes.
use cadrum::{DVec3, Edge, ProfileOrient, SceneOption, Solid, Tessellation};
use std::time::Instant;

fn square(half: f64, z: f64) -> Vec<Edge> {
    let p = [[-half, -half], [half, -half], [half, half], [-half, half]].map(|[x, y]| DVec3::new(x, y, z));
    Edge::polygon(&p).unwrap()
}
fn report(name: &str, t: Instant, s: &Result<Solid, cadrum::Error>) {
    match s {
        Ok(s) => {
            let b = s.bounding_box();
            println!("{name}: volume {:.4} area {:.4} bbox {:.3?}..{:.3?} faces {} ({} ms)", s.volume(), s.area(), b[0].to_array(), b[1].to_array(), s.iter_face().count(), t.elapsed().as_millis());
        }
        Err(e) => println!("{name}: FAILED {e} ({} ms)", t.elapsed().as_millis()),
    }
}
fn mesh(s: &[&Solid]) -> cadrum::Mesh {
    Solid::mesh(s.iter().copied(), Tessellation { deflection_linear: 0.05, deflection_angular: 0.1, relative_linear: false }).unwrap()
}
fn polylines(points: &[cadrum::DVec2]) -> usize {
    points.split(|p| p.x.is_nan()).filter(|s| s.len() > 1).count()
}

fn main() {
    // Loft: 20x20 square at z=0 to 10x10 square at z=10, ruled -> frustum 7000/3 = 2333.333
    let t = Instant::now();
    let loft = Solid::loft([square(10.0, 0.0), square(5.0, 10.0)].iter().map(|s| s.iter()), true);
    report("loft ruled square 20->10 h10 (expect 2333.333)", t, &loft);
    let t = Instant::now();
    let c0 = Edge::circle(10.0, DVec3::Z).unwrap();
    let c1 = Edge::circle(5.0, DVec3::Z).unwrap().translate(DVec3::Z * 10.0);
    let loft_c = Solid::loft([vec![c0], vec![c1]].iter().map(|s| s.iter()), true);
    report("loft ruled circle r10->r5 h10 (expect 7330.383)", t, &loft_c);
    let t = Instant::now();
    let loft_s = Solid::loft([square(10.0, 0.0), square(5.0, 10.0), square(10.0, 20.0)].iter().map(|s| s.iter()), false);
    report("loft smooth 3 squares", t, &loft_s);

    // Sweep: r=2 circle along a 30 mm line in +X -> 120*pi = 376.991
    let t = Instant::now();
    let profile = Edge::circle(2.0, DVec3::X).unwrap();
    let spine = Edge::line(DVec3::ZERO, DVec3::X * 30.0).unwrap();
    let sweep = Solid::sweep([&profile], [&spine], ProfileOrient::Fixed);
    report("sweep circle r2 along 30 line (expect 376.991)", t, &sweep);
    // Sweep along a quarter arc of radius 20 (torus quarter): 2*pi^2*R*r^2/4 = pi^2*20*4/2 = 394.784
    let t = Instant::now();
    let r = 20.0;
    let arc = Edge::arc_3pts(DVec3::new(r, 0.0, 0.0), DVec3::new(r * 0.5f64.sqrt(), r * 0.5f64.sqrt(), 0.0), DVec3::new(0.0, r, 0.0)).unwrap();
    let prof = Edge::circle(2.0, DVec3::Y).unwrap().translate(DVec3::new(r, 0.0, 0.0));
    let bend = Solid::sweep([&prof], [&arc], ProfileOrient::Up(DVec3::Z));
    report("sweep circle r2 along quarter arc R20 (expect 394.784)", t, &bend);

    // Shell: 40x30x20 box, top open, thickness -2 (inward) -> 24000 - 36*26*18 = 7152
    let t = Instant::now();
    let boxed = Solid::cube(DVec3::ZERO, DVec3::new(40.0, 30.0, 20.0));
    let top: Vec<_> = boxed.iter_face().filter(|f| (f.center().z - 20.0).abs() < 1e-6).collect();
    println!("top faces found: {}", top.len());
    let shell_in = boxed.shell(-2.0, top.iter().copied());
    report("shell inward -2 open top (expect 7152)", t, &shell_in);
    let t = Instant::now();
    let shell_out = boxed.shell(2.0, top.iter().copied());
    report("shell outward +2 open top (expect 44*34*22-24000 = 8912)", t, &shell_out);
    let t = Instant::now();
    let cyl = Solid::cylinder(10.0, DVec3::Z * 20.0);
    let ctop: Vec<_> = cyl.iter_face().filter(|f| (f.center().z - 20.0).abs() < 1e-6).collect();
    let shell_c = cyl.shell(-1.0, ctop.iter().copied());
    report("shell cylinder r10 h20 inward -1 open top (expect 2000pi-1539pi... = pi*(2000-81*19)=1448.0)", t, &shell_c);

    // Mirror: box at x 5..15 mirrored about x=0 -> -15..-5
    let m = Solid::cube(DVec3::new(5.0, 0.0, 0.0), DVec3::new(15.0, 10.0, 10.0)).mirror(DVec3::ZERO, DVec3::X);
    let b = m.bounding_box();
    println!("mirror bbox {:?}..{:?} volume {:.3}", b[0].to_array(), b[1].to_array(), m.volume());

    // Thread: M6 x1, 10 mm, 60 deg triangle swept on helix, union with minor-diameter core
    let t = Instant::now();
    let (d, p, len) = (6.0, 1.0, 10.0);
    let h = 0.866025 * p; // fundamental triangle height
    let r_minor = d / 2.0 - 5.0 / 8.0 * h;
    let helix = Edge::helix(r_minor - 0.01, p, len, DVec3::Z, DVec3::X).unwrap();
    let tri = [DVec3::new(r_minor - 0.01, 0.0, -p * 0.45), DVec3::new(d / 2.0, 0.0, -0.06 * p), DVec3::new(d / 2.0, 0.0, 0.06 * p), DVec3::new(r_minor - 0.01, 0.0, p * 0.45)];
    let prof = Edge::polygon(&tri).unwrap();
    let ridge = Solid::sweep(prof.iter(), [&helix], ProfileOrient::Up(DVec3::Z));
    report("thread ridge sweep", t, &ridge);
    if let Ok(ridge) = &ridge {
        let t = Instant::now();
        let core = Solid::cylinder(r_minor, DVec3::Z * len);
        let rod = (cadrum::Boolean::<Solid>::default() + &core + ridge).build_vec();
        match rod {
            Ok(v) => {
                let vol: f64 = v.iter().map(Solid::volume).sum();
                println!("thread rod: {} solids volume {:.3} (core {:.3}, plain M6 {:.3}) ({} ms)", v.len(), vol, core.volume(), std::f64::consts::PI * 9.0 * len, t.elapsed().as_millis());
                // clip to z 0..len
                let t = Instant::now();
                let clip = Solid::cube(DVec3::new(-10.0, -10.0, 0.0), DVec3::new(10.0, 10.0, len));
                let mut e = cadrum::Boolean::<Solid>::default();
                for s in &v { e = e + s; }
                match (e * &clip).build_vec() {
                    Ok(c) => { let bb = c[0].bounding_box(); println!("clipped rod volume {:.3} bbox {:?}..{:?} ({} ms)", c.iter().map(Solid::volume).sum::<f64>(), bb[0].to_array(), bb[1].to_array(), t.elapsed().as_millis()); }
                    Err(e) => println!("clip failed {e}"),
                }
            }
            Err(e) => println!("thread union FAILED {e}"),
        }
    }

    // Interference: two 10 mm cubes overlapping by 2 mm in x -> 200
    let a = Solid::cube(DVec3::ZERO, DVec3::splat(10.0));
    let b2 = Solid::cube(DVec3::new(8.0, 0.0, 0.0), DVec3::new(18.0, 10.0, 10.0));
    let i = (cadrum::Boolean::<Solid>::default() + &a) * &b2;
    println!("overlap volume {:.3}", i.build_vec().map(|v| v.iter().map(Solid::volume).sum::<f64>()).unwrap_or(-1.0));
    let far = Solid::cube(DVec3::new(13.0, 0.0, 0.0), DVec3::new(23.0, 10.0, 10.0));
    let i = (cadrum::Boolean::<Solid>::default() + &a) * &far;
    println!("disjoint intersect: {:?}", i.build_vec().map(|v| v.len()));

    // Hidden-line scenes: 40x30x20 box with a blind r4 hole from the top, depth 10
    let part = (cadrum::Boolean::<Solid>::default() + &boxed) - &Solid::cylinder(4.0, DVec3::Z * 10.0).translate(DVec3::new(20.0, 15.0, 10.0));
    let part = part.build_vec().unwrap();
    let m = mesh(&part.iter().collect::<Vec<_>>());
    for (name, view, up) in [("front", -DVec3::Y, DVec3::Z), ("top", DVec3::Z, DVec3::Y), ("right", DVec3::X, DVec3::Z), ("iso", DVec3::new(1.0, -1.0, 1.0), DVec3::Z)] {
        let t = Instant::now();
        let s = m.scene(SceneOption { view, up, hidden_edges: true, shading: false });
        let xs: Vec<_> = s.edges_visible.iter().filter(|p| !p.x.is_nan()).collect();
        let (minx, maxx) = xs.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.x), b.max(p.x)));
        let (miny, maxy) = xs.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.y), b.max(p.y)));
        println!("scene {name}: visible polylines {} hidden polylines {} extent x {:.2}..{:.2} y {:.2}..{:.2} ({} ms)", polylines(&s.edges_visible), polylines(&s.edges_hidden), minx, maxx, miny, maxy, t.elapsed().as_millis());
    }
    let plain = mesh(&[&boxed]);
    let s = plain.scene(SceneOption { view: -DVec3::Y, up: DVec3::Z, hidden_edges: true, shading: false });
    println!("plain box front: visible {} hidden {}", polylines(&s.edges_visible), polylines(&s.edges_hidden));
}
