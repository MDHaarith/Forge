//! Stage 2 spike: loft, sweep, shell, mirror, helix thread, intersection volume and hidden-line scenes.
use cadrum::{DVec3, Edge, ProfileOrient, SceneOption, Solid, Tessellation};
use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::fmt::Write;
use std::time::Instant;

fn square(half: f64, z: f64) -> Vec<Edge> {
    let p = [[-half, -half], [half, -half], [half, half], [-half, half]]
        .map(|[x, y]| DVec3::new(x, y, z));
    Edge::polygon(&p).unwrap()
}
fn report(name: &str, t: Instant, s: &Result<Solid, cadrum::Error>) {
    match s {
        Ok(s) => {
            let b = s.bounding_box();
            println!(
                "{name}: volume {:.4} area {:.4} bbox {:.3?}..{:.3?} faces {} ({} ms)",
                s.volume(),
                s.area(),
                b[0].to_array(),
                b[1].to_array(),
                s.iter_face().count(),
                t.elapsed().as_millis()
            );
        }
        Err(e) => println!("{name}: FAILED {e} ({} ms)", t.elapsed().as_millis()),
    }
}
fn mesh(s: &[&Solid]) -> cadrum::Mesh {
    Solid::mesh(
        s.iter().copied(),
        Tessellation {
            deflection_linear: 0.05,
            deflection_angular: 0.1,
            relative_linear: false,
        },
    )
    .unwrap()
}
fn polylines(points: &[cadrum::DVec2]) -> usize {
    points
        .split(|p| p.x.is_nan())
        .filter(|s| s.len() > 1)
        .count()
}

fn close(name: &str, actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{name}: {actual} != {expected} (tolerance {tolerance})"
    );
}

fn check_bbox(name: &str, actual: [DVec3; 2], expected: [DVec3; 2], tolerance: f64) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        for (actual, expected) in actual.to_array().into_iter().zip(expected.to_array()) {
            close(name, actual, expected, tolerance);
        }
    }
}

fn fine_mesh(solids: &[&Solid]) -> cadrum::Mesh {
    Solid::mesh(
        solids.iter().copied(),
        Tessellation {
            deflection_linear: 0.005,
            deflection_angular: 0.1,
            relative_linear: false,
        },
    )
    .unwrap()
}

fn mesh_bbox(mesh: &cadrum::Mesh) -> [DVec3; 2] {
    mesh.vertices.iter().fold(
        [DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)],
        |[min, max], p| [min.min(*p), max.max(*p)],
    )
}

fn thread_checks() {
    let total = Instant::now();
    let (d, pitch, length) = (6.0, 1.0, 10.0);
    let h = 3.0_f64.sqrt() / 2.0 * pitch;
    let minor = d / 2.0 - 5.0 * h / 8.0;
    let overlap = 0.01;
    let base = minor - overlap;
    let half_base = 3.0 * pitch / 8.0 + overlap / 3.0_f64.sqrt();
    // Extend by one pitch at each end so clipping leaves complete end threads.
    let helix = Edge::helix(base, pitch, length + 2.0 * pitch, DVec3::Z, DVec3::X)
        .unwrap()
        .translate(-DVec3::Z * pitch);
    let profile = Edge::polygon(&[
        DVec3::new(base, 0.0, -pitch - half_base),
        DVec3::new(d / 2.0, 0.0, -pitch - pitch / 16.0),
        DVec3::new(d / 2.0, 0.0, -pitch + pitch / 16.0),
        DVec3::new(base, 0.0, -pitch + half_base),
    ])
    .unwrap();
    println!("thread profile: H {h:.9}, minor radius {minor:.9}, crest width {:.6}, root width {:.6}, flank angle {:.6} degrees, helix start {:?}", pitch / 8.0, pitch / 4.0, (5.0 * h / 8.0 / (5.0 * pitch / 16.0)).atan().to_degrees(), helix.start_point().to_array());
    let diagnostic_helix = Edge::helix(base, pitch, length, DVec3::Z, DVec3::X).unwrap();
    let diagnostic_profile: Vec<_> = profile
        .iter()
        .map(|edge| edge.clone().translate(DVec3::Z * pitch))
        .collect();
    let diagnostic = Solid::sweep(
        diagnostic_profile.iter(),
        [&diagnostic_helix],
        ProfileOrient::Up(DVec3::Z),
    )
    .unwrap();
    let raw_diagnostic = diagnostic.bounding_box();
    let diagnostic_mesh = mesh(&[&diagnostic]);
    println!("original 10-turn bbox reproduction: unmeshed BRep {:.6?}, mesh {:.6?}, max mesh radius {:.6}", raw_diagnostic.map(|p| p.to_array()), mesh_bbox(&diagnostic_mesh).map(|p| p.to_array()), diagnostic_mesh.vertices.iter().map(|p| p.x.hypot(p.y)).fold(0.0, f64::max));
    let t = Instant::now();
    let ridge = Solid::sweep(profile.iter(), [&helix], ProfileOrient::Up(DVec3::Z)).unwrap();
    println!(
        "extended thread ridge sweep: {} ms",
        t.elapsed().as_millis()
    );
    let raw = ridge.bounding_box();
    let ridge_mesh = fine_mesh(&[&ridge]);
    let radius = ridge_mesh
        .vertices
        .iter()
        .map(|p| p.x.hypot(p.y))
        .fold(0.0, f64::max);
    println!("thread placement diagnosis: unmeshed BRep bbox {:.6?}, mesh bbox {:.6?}, mesh max radius {radius:.6}, meshed BRep bbox {:.6?} ({} ms)", raw.map(|p| p.to_array()), mesh_bbox(&ridge_mesh).map(|p| p.to_array()), ridge.bounding_box().map(|p| p.to_array()), t.elapsed().as_millis());
    close("thread ridge radius", radius, 3.0, 0.001);
    let t = Instant::now();
    let core =
        Solid::cylinder(minor, DVec3::Z * (length + 2.0 * pitch)).translate(-DVec3::Z * pitch);
    let clip = Solid::cube(DVec3::new(-4.0, -4.0, 0.0), DVec3::new(4.0, 4.0, length));
    let fused = (cadrum::Boolean::<Solid>::default() + &core + &ridge)
        .build()
        .unwrap();
    println!("thread untrimmed fused volume {:.6}", fused.volume());
    let mut rods = (cadrum::Boolean::<Solid>::from(&fused) * &clip)
        .build_vec()
        .unwrap();
    println!(
        "thread fuse and clip: {} solids ({} ms)",
        rods.len(),
        t.elapsed().as_millis()
    );
    assert_eq!(rods.len(), 1);
    let rod = rods.remove(0);
    let t = Instant::now();
    let rod_mesh = fine_mesh(&[&rod]);
    let bbox = rod.bounding_box();
    let volume = rod.volume();
    println!("external M6x1 length10: volume {volume:.6}, error vs 224.84 {:.4}%, 1 solid, BRep bbox {:.6?}, mesh bbox {:.6?}, bbox meshing {} ms, total {} ms", 100.0 * (volume / 224.84 - 1.0), bbox.map(|p| p.to_array()), mesh_bbox(&rod_mesh).map(|p| p.to_array()), t.elapsed().as_millis(), total.elapsed().as_millis());
    close("external thread volume", volume, 224.84, 224.84 * 0.03);
    check_bbox(
        "external thread bbox",
        bbox,
        [DVec3::new(-3.0, -3.0, 0.0), DVec3::new(3.0, 3.0, length)],
        0.01,
    );
    // Confirm the flat root/crest ISO basic profile by integrating r(z)^2 over one pitch.
    let analytic = PI
        * length
        * (minor.powi(2) / 4.0 + 5.0 / 8.0 * (minor.powi(2) + minor * 3.0 + 9.0) / 3.0 + 9.0 / 8.0);
    println!(
        "ISO basic profile analytic volume {analytic:.6}; pitch-diameter approximation {:.6}",
        PI / 4.0 * (d - 0.6495 * pitch).powi(2) * length
    );
    close("thread analytic volume", volume, analytic, 0.05);

    let t = Instant::now();
    let box20 = Solid::cube(DVec3::ZERO, DVec3::new(20.0, 20.0, length));
    let at = DVec3::new(10.0, 10.0, 0.0);
    let tap = Solid::cylinder(2.5, DVec3::Z * length).translate(at);
    let cutter = rod.clone().translate(at);
    let tapped = (cadrum::Boolean::<Solid>::default() + &box20 - &tap - &cutter)
        .build_vec()
        .unwrap();
    assert_eq!(tapped.len(), 1);
    let removed = 4000.0 - tapped[0].volume();
    println!("internal M6 through 20x20x10: tap drill diameter 5, body volume {:.6}, removed volume {removed:.6}, error vs 224.84 {:.4}%, {} solid ({} ms)", tapped[0].volume(), 100.0 * (removed / 224.84 - 1.0), tapped.len(), t.elapsed().as_millis());
    close(
        "internal thread removed volume",
        removed,
        224.84,
        224.84 * 0.03,
    );
    let flank_fraction = 2.0 * (3.0 - 2.5) / 3.0_f64.sqrt();
    let internal_analytic = PI
        * length
        * (2.5_f64.powi(2) * (1.0 - 0.125 - flank_fraction)
            + flank_fraction * (2.5_f64.powi(2) + 2.5 * 3.0 + 9.0) / 3.0
            + 9.0 * 0.125);
    println!("tap drill plus ISO flank analytic removed volume {internal_analytic:.6}");
    close(
        "internal thread analytic volume",
        removed,
        internal_analytic,
        0.05,
    );
}

fn advanced_checks() {
    let t = Instant::now();
    let v = |x, z| DVec3::new(x, 0.0, z);
    // One closed XZ section, including exact inner and outer quarter circles.
    let section = [
        Edge::line(v(0.0, 0.0), v(30.0, 0.0)).unwrap(),
        Edge::arc_3pts(
            v(30.0, 0.0),
            v(30.0 + 4.0 / 2.0_f64.sqrt(), 4.0 - 4.0 / 2.0_f64.sqrt()),
            v(34.0, 4.0),
        )
        .unwrap(),
        Edge::line(v(34.0, 4.0), v(34.0, 24.0)).unwrap(),
        Edge::line(v(34.0, 24.0), v(32.0, 24.0)).unwrap(),
        Edge::line(v(32.0, 24.0), v(32.0, 4.0)).unwrap(),
        Edge::arc_3pts(
            v(32.0, 4.0),
            v(30.0 + 2.0 / 2.0_f64.sqrt(), 4.0 - 2.0 / 2.0_f64.sqrt()),
            v(30.0, 2.0),
        )
        .unwrap(),
        Edge::line(v(30.0, 2.0), v(0.0, 2.0)).unwrap(),
        Edge::line(v(0.0, 2.0), v(0.0, 0.0)).unwrap(),
    ];
    let sheet = Solid::extrude(section.iter(), DVec3::Y * 10.0).unwrap();
    let bbox = sheet.bounding_box();
    let flat = 50.0 + FRAC_PI_2 * (2.0 + 0.44 * 2.0);
    println!(
        "sheet L t2 w10 flanges30/20 r2: volume {:.6}, bbox {:.6?}, flat [{flat:.6},10] ({} ms)",
        sheet.volume(),
        bbox.map(|p| p.to_array()),
        t.elapsed().as_millis()
    );
    close("sheet volume", sheet.volume(), 1094.248, 0.001);
    check_bbox(
        "sheet bbox",
        bbox,
        [DVec3::ZERO, DVec3::new(34.0, 10.0, 24.0)],
        0.001,
    );
    close("sheet flat length", flat, 54.524, 0.001);

    let t = Instant::now();
    let start = DVec3::ZERO;
    let a = DVec3::new(10.0, 0.0, 0.0);
    let b = DVec3::new(20.0, 10.0, 0.0);
    let end = DVec3::new(20.0, 20.0, 0.0);
    let path = [
        Edge::line(start, a).unwrap(),
        Edge::arc_3pts(a, DVec3::new(17.071068, 2.928932, 0.0), b).unwrap(),
        Edge::line(b, end).unwrap(),
    ];
    for join in path.windows(2) {
        let tangent0 = join[0].project(join[0].end_point()).1.normalize();
        let tangent1 = join[1].project(join[1].start_point()).1.normalize();
        let angle = tangent0.dot(tangent1).clamp(-1.0, 1.0).acos().to_degrees();
        println!(
            "sweep join gap {:.9}, tangent angle {angle:.9} degrees",
            join[0].end_point().distance(join[1].start_point())
        );
        assert!(angle < 1.0);
    }
    let profile = Edge::circle(2.0, DVec3::Z)
        .unwrap()
        .align_z(DVec3::X, DVec3::Y)
        .translate(start);
    let swept = Solid::sweep([&profile], path.iter(), ProfileOrient::Up(DVec3::Z)).unwrap();
    println!(
        "sweep line/arc/line: volume {:.9}, rounded {:.3}, length {:.9} ({} ms)",
        swept.volume(),
        swept.volume(),
        20.0 + 10.0 * FRAC_PI_2,
        t.elapsed().as_millis()
    );
    close("sweep reference tolerance", swept.volume(), 448.719, 0.001);
    close(
        "sweep analytic volume",
        swept.volume(),
        PI * 4.0 * (20.0 + 10.0 * FRAC_PI_2),
        0.00001,
    );
    println!(
        "plan correction: sweep 448.719 -> 448.720; pi*2^2*(10+pi*10/2+10) = {:.9}",
        PI * 4.0 * (20.0 + 10.0 * FRAC_PI_2)
    );

    let t = Instant::now();
    let cylinder = Solid::cylinder(2.0, DVec3::Z * 10.0).translate(DVec3::X * 10.0);
    let copies: Vec<_> = (0..6)
        .map(|i| cylinder.clone().rotate_z(TAU * i as f64 / 6.0))
        .collect();
    let mut union = cadrum::Boolean::<Solid>::default();
    for copy in &copies {
        union = union + copy;
    }
    let pattern = union.build_vec().unwrap();
    let volume = pattern.iter().map(Solid::volume).sum::<f64>();
    let bbox = pattern.iter().map(Solid::bounding_box).fold(
        [DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)],
        |[min, max], b| [min.min(b[0]), max.max(b[1])],
    );
    println!(
        "polar 6 cylinders r2 h10 radius10: volume {volume:.6}, {} solids, bbox {:.6?} ({} ms)",
        pattern.len(),
        bbox.map(|p| p.to_array()),
        t.elapsed().as_millis()
    );
    assert_eq!(pattern.len(), 6);
    close("polar volume", volume, 753.982, 0.001);
    check_bbox(
        "polar bbox",
        bbox,
        [
            DVec3::new(-12.0, -10.660254, 0.0),
            DVec3::new(12.0, 10.660254, 10.0),
        ],
        0.001,
    );
    clearance_checks();
}

fn point_segment_distance_squared(p: DVec3, a: DVec3, b: DVec3) -> f64 {
    let ab = b - a;
    let t = if ab.length_squared() == 0.0 {
        0.0
    } else {
        ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
    };
    p.distance_squared(a + t * ab)
}

fn point_triangle_distance_squared(p: DVec3, [a, b, c]: [DVec3; 3]) -> f64 {
    let normal = (b - a).cross(c - a);
    if normal.length_squared() > 0.0 {
        let projected = p - normal * (p - a).dot(normal) / normal.length_squared();
        if [(a, b), (b, c), (c, a)]
            .iter()
            .all(|(a, b)| (b - a).cross(projected - a).dot(normal) >= 0.0)
        {
            return p.distance_squared(projected);
        }
    }
    [(a, b), (b, c), (c, a)]
        .into_iter()
        .map(|(a, b)| point_segment_distance_squared(p, a, b))
        .fold(f64::INFINITY, f64::min)
}

fn mesh_clearance(a: &cadrum::Mesh, b: &cadrum::Mesh) -> f64 {
    // shortcut: vertex-to-triangle misses edge-to-edge minima, add segment-pair distances before claiming general exact clearance.
    [(a, b), (b, a)]
        .into_iter()
        .flat_map(|(from, to)| {
            from.vertices.iter().flat_map(move |p| {
                to.indices.chunks_exact(3).map(move |tri| {
                    point_triangle_distance_squared(
                        *p,
                        [
                            to.vertices[tri[0]],
                            to.vertices[tri[1]],
                            to.vertices[tri[2]],
                        ],
                    )
                })
            })
        })
        .fold(f64::INFINITY, f64::min)
        .sqrt()
}

fn clearance_checks() {
    close(
        "triangle face projection",
        point_triangle_distance_squared(
            DVec3::new(0.25, 0.25, 2.0),
            [DVec3::ZERO, DVec3::X, DVec3::Y],
        ),
        4.0,
        1e-12,
    );
    close(
        "triangle edge/vertex projection",
        point_triangle_distance_squared(
            DVec3::new(-1.0, -1.0, 0.0),
            [DVec3::ZERO, DVec3::X, DVec3::Y],
        ),
        2.0,
        1e-12,
    );
    close(
        "degenerate triangle",
        point_triangle_distance_squared(DVec3::Y, [DVec3::ZERO, DVec3::X, DVec3::X * 2.0]),
        1.0,
        1e-12,
    );
    let t = Instant::now();
    let a = Solid::cube(DVec3::ZERO, DVec3::splat(10.0));
    let b = Solid::cube(DVec3::new(13.0, 14.0, 0.0), DVec3::new(23.0, 24.0, 10.0));
    let tessellation = Tessellation {
        deflection_linear: 0.01,
        deflection_angular: 0.1,
        relative_linear: false,
    };
    let ma = Solid::mesh([&a], tessellation).unwrap();
    let mb = Solid::mesh([&b], tessellation).unwrap();
    let clearance = mesh_clearance(&ma, &mb);
    println!("mesh clearance cubes [0,0,0]/[13,14,0]: {clearance:.6}, analytic sqrt(3^2+4^2)=5, triangles {}/{} ({} ms)", ma.indices.len() / 3, mb.indices.len() / 3, t.elapsed().as_millis());
    close("mesh clearance", clearance, 5.0, 1e-9);
}

fn joined_polylines(points: &[cadrum::DVec2]) -> Vec<Vec<cadrum::DVec2>> {
    // shortcut: endpoint joining scans fragments quadratically, index endpoints for large drawings.
    let near = |a: cadrum::DVec2, b: cadrum::DVec2| a.distance_squared(b) < 1e-12;
    let mut pending: Vec<Vec<_>> = Vec::new();
    for line in points.split(|p| p.x.is_nan()).filter(|line| line.len() > 1) {
        if !pending.iter().any(|other| {
            other.len() == line.len()
                && (other.iter().zip(line).all(|(a, b)| near(*a, *b))
                    || other.iter().rev().zip(line).all(|(a, b)| near(*a, *b)))
        }) {
            pending.push(line.to_vec());
        }
    }
    let mut joined = Vec::new();
    while let Some(mut line) = pending.pop() {
        loop {
            let end = *line.last().unwrap();
            let start = line[0];
            let Some(index) = pending.iter().position(|next| {
                near(end, next[0])
                    || near(end, *next.last().unwrap())
                    || near(start, next[0])
                    || near(start, *next.last().unwrap())
            }) else {
                break;
            };
            let mut next = pending.swap_remove(index);
            if !near(end, next[0]) && !near(end, *next.last().unwrap()) {
                line.reverse();
            }
            let end = *line.last().unwrap();
            if !near(end, next[0]) {
                next.reverse();
            }
            line.extend(next.into_iter().skip(1));
        }
        joined.push(line);
    }
    joined
}

fn svg_paths(svg: &mut String, class: &str, points: &[cadrum::DVec2], offset: cadrum::DVec2) {
    // Join scene fragments so each tiny edge does not restart the SVG dash pattern.
    for line in joined_polylines(points) {
        write!(svg, "<path class=\"{class}\" d=\"").unwrap();
        for (i, p) in line.iter().enumerate() {
            write!(
                svg,
                "{} {:.4} {:.4} ",
                if i == 0 { "M" } else { "L" },
                p.x + offset.x,
                -p.y + offset.y
            )
            .unwrap();
        }
        svg.push_str("\"/>\n");
    }
}

fn main() {
    let joined = joined_polylines(&[
        cadrum::DVec2::ZERO,
        cadrum::DVec2::X,
        cadrum::DVec2::NAN,
        cadrum::DVec2::X * 2.0,
        cadrum::DVec2::X,
        cadrum::DVec2::NAN,
        cadrum::DVec2::X,
        cadrum::DVec2::ZERO,
    ]);
    assert_eq!(joined.len(), 1);
    assert_eq!(joined[0].len(), 3);
    // Loft: 20x20 square at z=0 to 10x10 square at z=10, ruled -> frustum 7000/3 = 2333.333
    let t = Instant::now();
    let loft = Solid::loft(
        [square(10.0, 0.0), square(5.0, 10.0)]
            .iter()
            .map(|s| s.iter()),
        true,
    );
    report("loft ruled square 20->10 h10 (expect 2333.333)", t, &loft);
    let t = Instant::now();
    let c0 = Edge::circle(10.0, DVec3::Z).unwrap();
    let c1 = Edge::circle(5.0, DVec3::Z)
        .unwrap()
        .translate(DVec3::Z * 10.0);
    let loft_c = Solid::loft([vec![c0], vec![c1]].iter().map(|s| s.iter()), true);
    report(
        "loft ruled circle r10->r5 h10 (expect 1832.596)",
        t,
        &loft_c,
    );
    let t = Instant::now();
    let loft_s = Solid::loft(
        [square(10.0, 0.0), square(5.0, 10.0), square(10.0, 20.0)]
            .iter()
            .map(|s| s.iter()),
        false,
    );
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
    let arc = Edge::arc_3pts(
        DVec3::new(r, 0.0, 0.0),
        DVec3::new(r * 0.5f64.sqrt(), r * 0.5f64.sqrt(), 0.0),
        DVec3::new(0.0, r, 0.0),
    )
    .unwrap();
    let prof = Edge::circle(2.0, DVec3::Y)
        .unwrap()
        .translate(DVec3::new(r, 0.0, 0.0));
    let bend = Solid::sweep([&prof], [&arc], ProfileOrient::Up(DVec3::Z));
    report(
        "sweep circle r2 along quarter arc R20 (expect 394.784)",
        t,
        &bend,
    );

    // Shell: 40x30x20 box, top open, thickness -2 (inward) -> 24000 - 36*26*18 = 7152
    let t = Instant::now();
    let boxed = Solid::cube(DVec3::ZERO, DVec3::new(40.0, 30.0, 20.0));
    let top: Vec<_> = boxed
        .iter_face()
        .filter(|f| (f.center().z - 20.0).abs() < 1e-6)
        .collect();
    println!("top faces found: {}", top.len());
    let shell_in = boxed.shell(-2.0, top.iter().copied());
    report("shell inward -2 open top (expect 7152)", t, &shell_in);
    let t = Instant::now();
    let shell_out = boxed.shell(2.0, top.iter().copied());
    report(
        "shell outward +2 open top (rounded outer shell; no sharp-corner reference)",
        t,
        &shell_out,
    );
    let t = Instant::now();
    let cyl = Solid::cylinder(10.0, DVec3::Z * 20.0);
    let ctop: Vec<_> = cyl
        .iter_face()
        .filter(|f| (f.center().z - 20.0).abs() < 1e-6)
        .collect();
    let shell_c = cyl.shell(-1.0, ctop.iter().copied());
    report(
        "shell cylinder r10 h20 inward -1 open top (expect pi*(2000-81*19)=1448.274)",
        t,
        &shell_c,
    );

    // Mirror: box at x 5..15 mirrored about x=0 -> -15..-5
    let m = Solid::cube(DVec3::new(5.0, 0.0, 0.0), DVec3::new(15.0, 10.0, 10.0))
        .mirror(DVec3::ZERO, DVec3::X);
    let b = m.bounding_box();
    println!(
        "mirror bbox {:?}..{:?} volume {:.3}",
        b[0].to_array(),
        b[1].to_array(),
        m.volume()
    );

    thread_checks();
    advanced_checks();

    // Interference: two 10 mm cubes overlapping by 2 mm in x -> 200
    let a = Solid::cube(DVec3::ZERO, DVec3::splat(10.0));
    let b2 = Solid::cube(DVec3::new(8.0, 0.0, 0.0), DVec3::new(18.0, 10.0, 10.0));
    let i = (cadrum::Boolean::<Solid>::default() + &a) * &b2;
    println!(
        "overlap volume {:.3}",
        i.build_vec()
            .map(|v| v.iter().map(Solid::volume).sum::<f64>())
            .unwrap_or(-1.0)
    );
    let far = Solid::cube(DVec3::new(13.0, 0.0, 0.0), DVec3::new(23.0, 10.0, 10.0));
    let i = (cadrum::Boolean::<Solid>::default() + &a) * &far;
    println!("disjoint intersect: {:?}", i.build_vec().map(|v| v.len()));

    // Hidden-line scenes: 40x30x20 box with a blind r4 hole from the top, depth 10
    let part = (cadrum::Boolean::<Solid>::default() + &boxed)
        - &Solid::cylinder(4.0, DVec3::Z * 10.0).translate(DVec3::new(20.0, 15.0, 10.0));
    let part = part.build_vec().unwrap();
    assert_eq!(part.len(), 1);
    close(
        "blind hole volume",
        part[0].volume(),
        24000.0 - PI * 4.0_f64.powi(2) * 10.0,
        1e-6,
    );
    let m = mesh(&part.iter().collect::<Vec<_>>());
    let mut svg = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200mm\" height=\"155mm\" viewBox=\"0 0 200 155\">\n<style>.visible{fill:none;stroke:#111;stroke-width:0.5}.hidden{fill:none;stroke:#666;stroke-width:0.25;stroke-dasharray:2 1}text{font:4px sans-serif;fill:#111}</style>\n<rect width=\"200\" height=\"155\" fill=\"white\"/>\n<text x=\"10\" y=\"10\">Stage 2: 40 x 30 x 20 mm box; blind hole r4, depth 10</text>\n");
    for (name, view, up, cx, cy) in [
        ("front", -DVec3::Y, DVec3::Z, 50.0, 45.0),
        ("top", DVec3::Z, DVec3::Y, 50.0, 110.0),
        ("right", DVec3::X, DVec3::Z, 145.0, 45.0),
        ("iso", DVec3::new(1.0, -1.0, 1.0), DVec3::Z, 145.0, 110.0),
    ] {
        let t = Instant::now();
        let s = m.scene(SceneOption {
            view,
            up,
            hidden_edges: true,
            shading: false,
        });
        let xs: Vec<_> = s.edges_visible.iter().filter(|p| !p.x.is_nan()).collect();
        let (minx, maxx) = xs
            .iter()
            .fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.x), b.max(p.x)));
        let (miny, maxy) = xs
            .iter()
            .fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.y), b.max(p.y)));
        println!("scene {name}: visible polylines {} hidden polylines {} extent x {:.2}..{:.2} y {:.2}..{:.2} ({} ms)", polylines(&s.edges_visible), polylines(&s.edges_hidden), minx, maxx, miny, maxy, t.elapsed().as_millis());
        assert!(polylines(&s.edges_visible) > 0);
        assert!(polylines(&s.edges_hidden) > 0);
        let offset = cadrum::DVec2::new(cx - (minx + maxx) / 2.0, cy + (miny + maxy) / 2.0);
        writeln!(
            svg,
            "<g id=\"view-{name}\">\n<text x=\"{}\" y=\"{}\">{name}</text>",
            cx - 10.0,
            cy - 28.0
        )
        .unwrap();
        svg_paths(&mut svg, "hidden", &s.edges_hidden, offset);
        svg_paths(&mut svg, "visible", &s.edges_visible, offset);
        svg.push_str("</g>\n");
    }
    svg.push_str("</svg>\n");
    let svg_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("stage2.svg");
    std::fs::write(&svg_path, &svg).unwrap();
    println!("SVG written: {} ({} bytes), four Mesh::scene views; hidden stroke-dasharray 2 1; blind-hole volume {:.6}", svg_path.display(), svg.len(), part[0].volume());
    let plain = mesh(&[&boxed]);
    let s = plain.scene(SceneOption {
        view: -DVec3::Y,
        up: DVec3::Z,
        hidden_edges: true,
        shading: false,
    });
    println!(
        "plain box front: visible {} hidden {}",
        polylines(&s.edges_visible),
        polylines(&s.edges_hidden)
    );
    assert_eq!(polylines(&s.edges_visible), 4);
    assert_eq!(polylines(&s.edges_hidden), 0);
    println!("Stage 2: all numerical assertions passed");
}
