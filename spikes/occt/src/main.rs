use cadrum::{DVec3, Edge, Solid, SurfaceKind, Tessellation};
use std::{error::Error, f64::consts::TAU, fs::File, path::Path};

fn check_volume(name: &str, actual: f64, expected: f64) {
    println!("{name}: volume={actual:.6} mm^3, expected={expected:.3}, tolerance=0.01");
    assert!((actual - expected).abs() <= 0.01, "{name}: wrong volume");
}

fn vertical(edge: &&Edge) -> bool {
    let delta = edge.end_point() - edge.start_point();
    delta.x.abs() < 1e-8 && delta.y.abs() < 1e-8 && delta.z.abs() > 1e-8
}

fn main() -> Result<(), Box<dyn Error>> {
    let out = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cube = Solid::cube(DVec3::ZERO, DVec3::splat(20.0));
    let cylinder = Solid::cylinder(5.0, DVec3::Z * 20.0).translate(DVec3::new(10.0, 10.0, 0.0));
    let cut = (&cube - &cylinder).build()?;
    check_volume("cut", cut.volume(), 6429.204);

    let fresh = Solid::cube(DVec3::ZERO, DVec3::splat(20.0));
    let edges: Vec<_> = fresh.iter_edge().filter(vertical).collect();
    println!(
        "fillet: selected {} vertical edges, radius=2 mm",
        edges.len()
    );
    assert_eq!(edges.len(), 4);
    check_volume("fillet", fresh.fillet_edges(2.0, edges)?.volume(), 7931.327);

    let fresh = Solid::cube(DVec3::ZERO, DVec3::splat(20.0));
    let edges: Vec<_> = fresh.iter_edge().filter(vertical).collect();
    println!(
        "chamfer: selected {} vertical edges, distance=2 mm",
        edges.len()
    );
    assert_eq!(edges.len(), 4);
    check_volume("chamfer", fresh.chamfer_edges(2.0, edges)?.volume(), 7840.0);

    let profile = Edge::polygon(&[
        DVec3::new(0.0, 0.0, 0.0),
        DVec3::new(5.0, 0.0, 0.0),
        DVec3::new(5.0, 0.0, 10.0),
        DVec3::new(0.0, 0.0, 10.0),
    ])?;
    let revolved = Solid::revolve(&profile, DVec3::ZERO, DVec3::Z, TAU)?;
    check_volume("revolve", revolved.volume(), 785.398);

    Solid::write_step([&cut], &mut File::create(out.join("out.step"))?)?;
    let imported = Solid::read_step(&mut File::open(out.join("out.step"))?)?;
    assert_eq!(imported.len(), 1);
    check_volume("STEP roundtrip", imported[0].volume(), 6429.204);
    let difference = (imported[0].volume() - cut.volume()).abs();
    println!("STEP roundtrip: volume difference={difference:.12} mm^3");
    assert!(difference < 1e-7);

    let mut cylinder_count = 0;
    for (i, face) in cut.iter_face().enumerate() {
        let surface = face
            .surface()
            .expect("cut part has only elementary surfaces");
        println!("face {i}: {:?}, area={:.6} mm^2", surface.kind, face.area());
        if let SurfaceKind::Cylinder { radius } = surface.kind {
            cylinder_count += 1;
            assert!((radius - 5.0).abs() < 1e-8);
            // shortcut: full-cylinder midpoint from its seam, add UV bounds/evaluation for trimmed surfaces.
            let seam = face
                .iter_edge()
                .find(vertical)
                .expect("cylinder has a vertical seam");
            let seam_mid = (seam.start_point() + seam.end_point()) * 0.5;
            let axis_mid =
                surface.origin + surface.axis_z * (seam_mid - surface.origin).dot(surface.axis_z);
            let midpoint = axis_mid * 2.0 - seam_mid;
            let (point, normal) = face.project(midpoint);
            let inward = (axis_mid - point).normalize();
            let dot = normal.dot(inward);
            println!("cylinder radius={radius:.6} mm: parameter midpoint={point:?}, outward normal={normal:?}, dot(towards axis)={dot:.9}");
            assert!(point.distance(midpoint) < 1e-7);
            assert!((normal.length() - 1.0).abs() < 1e-8);
            assert!(dot > 1.0 - 1e-8, "hole normal points away from axis");
        }
    }
    assert_eq!(cylinder_count, 1);

    for (point, expected) in [
        (DVec3::new(2.0, 2.0, 10.0), true),
        (DVec3::new(10.0, 10.0, 10.0), false),
    ] {
        let inside = cut.contains(point);
        println!(
            "classify {point:?}: {}",
            if inside { "inside" } else { "outside" }
        );
        assert_eq!(inside, expected);
    }

    let mesh = Solid::mesh(
        [&cut],
        Tessellation {
            deflection_linear: 0.1,
            deflection_angular: 0.1,
            relative_linear: false,
        },
    )?;
    mesh.write_stl(&mut File::create(out.join("out.stl"))?)?;
    let stl = std::fs::read(out.join("out.stl"))?;
    let stl_count = u32::from_le_bytes(stl[80..84].try_into()?) as usize;
    assert_eq!(stl_count, mesh.indices.len() / 3);
    assert_eq!(stl.len(), 84 + 50 * stl_count);
    println!(
        "binary STL: {} bytes, {stl_count} triangles, binary length verified",
        stl.len()
    );

    let mut downward_area = 0.0;
    let mut downward_count = 0;
    for indices in mesh.indices.chunks_exact(3) {
        let [a, b, c] = [
            mesh.vertices[indices[0]],
            mesh.vertices[indices[1]],
            mesh.vertices[indices[2]],
        ];
        let cross = (b - a).cross(c - a);
        assert!(cross.length() > 0.0, "degenerate triangle");
        if cross.normalize().z < -0.7071 {
            assert!(
                [a.z, b.z, c.z].iter().all(|z| z.abs() < 1e-7),
                "non-bottom triangle counted"
            );
            downward_area += cross.length() * 0.5;
            downward_count += 1;
        }
    }
    println!("tessellation: deflection=0.1 mm, angular=0.1 rad, triangles={}, downward triangles={downward_count}, downward area={downward_area:.6} mm^2", mesh.indices.len() / 3);
    assert!((downward_area - 321.460).abs() <= 0.5);
    println!("all required checks passed");
    Ok(())
}
