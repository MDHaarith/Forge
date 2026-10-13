use crate::route::{Board, Routes};
use cadrum::{DVec3, SceneOption, Solid, Tessellation};
use serde_json::Value;
use std::{
    error::Error,
    f64::consts::PI,
    fs::{self, File},
    path::Path,
    time::Instant,
};

fn le32(data: &[u8], at: usize) -> usize {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap()) as usize
}

type GlbSummary = (usize, usize, [f64; 3], [f64; 3]);

fn verify_glb(bytes: &[u8], expected_triangles: usize) -> Result<GlbSummary, Box<dyn Error>> {
    assert_eq!(&bytes[..4], b"glTF");
    assert_eq!(le32(bytes, 4), 2);
    assert_eq!(le32(bytes, 8), bytes.len());
    let json_len = le32(bytes, 12);
    assert_eq!(json_len % 4, 0);
    assert_eq!(&bytes[16..20], b"JSON");
    let doc: Value = serde_json::from_slice(&bytes[20..20 + json_len])?;
    let bin_header = 20 + json_len;
    assert_eq!(&bytes[bin_header + 4..bin_header + 8], b"BIN\0");
    let bin = &bytes[bin_header + 8..];
    assert_eq!(le32(bytes, bin_header), bin.len());
    let views = doc["bufferViews"].as_array().unwrap();
    for v in views {
        assert!(
            v["byteOffset"].as_u64().unwrap_or(0) + v["byteLength"].as_u64().unwrap()
                <= bin.len() as u64
        );
    }
    let accessors = doc["accessors"].as_array().unwrap();
    let mut triangles = 0;
    let mut primitives = 0;
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for mesh in doc["meshes"].as_array().unwrap() {
        for primitive in mesh["primitives"].as_array().unwrap() {
            primitives += 1;
            if primitive["mode"].as_u64().unwrap_or(4) != 4 {
                continue;
            }
            let pos = &accessors[primitive["attributes"]["POSITION"].as_u64().unwrap() as usize];
            assert_eq!(pos["componentType"], 5126);
            assert_eq!(pos["type"], "VEC3");
            let count = pos["count"].as_u64().unwrap() as usize;
            let view = &views[pos["bufferView"].as_u64().unwrap() as usize];
            let offset = view["byteOffset"].as_u64().unwrap_or(0) as usize
                + pos["byteOffset"].as_u64().unwrap_or(0) as usize;
            let stride = view["byteStride"].as_u64().unwrap_or(12) as usize;
            for i in 0..count {
                for axis in 0..3 {
                    let at = offset + i * stride + axis * 4;
                    let value = f32::from_le_bytes(bin[at..at + 4].try_into()?) as f64;
                    assert!(value.is_finite());
                    min[axis] = min[axis].min(value);
                    max[axis] = max[axis].max(value);
                }
            }
            for axis in 0..3 {
                assert!((pos["min"][axis].as_f64().unwrap() - min[axis]).abs() < 1e-5);
                assert!((pos["max"][axis].as_f64().unwrap() - max[axis]).abs() < 1e-5);
            }
            let index = &accessors[primitive["indices"].as_u64().unwrap() as usize];
            let index_size = match index["componentType"].as_u64().unwrap() {
                5123 => 2,
                5125 => 4,
                _ => panic!("unsupported index component type"),
            };
            assert_eq!(index["type"], "SCALAR");
            let num = index["count"].as_u64().unwrap() as usize;
            assert_eq!(num % 3, 0);
            triangles += num / 3;
            let view = &views[index["bufferView"].as_u64().unwrap() as usize];
            let start = view["byteOffset"].as_u64().unwrap_or(0) as usize
                + index["byteOffset"].as_u64().unwrap_or(0) as usize;
            for i in 0..num {
                let at = start + i * index_size;
                let value = if index_size == 2 {
                    u16::from_le_bytes(bin[at..at + 2].try_into()?) as usize
                } else {
                    le32(bin, at)
                };
                assert!(value < count);
            }
        }
    }
    assert_eq!(triangles, expected_triangles);
    Ok((triangles, primitives, min, max))
}

pub fn export(b: &Board, r: &Routes, out: &Path, log: &mut String) -> Result<(), Box<dyn Error>> {
    let t = Instant::now();
    let thickness = 1.6;
    let mut board = Solid::cube(
        DVec3::new(-b.size[0] / 2.0, -b.size[1] / 2.0, -thickness / 2.0),
        DVec3::new(b.size[0] / 2.0, b.size[1] / 2.0, thickness / 2.0),
    );
    let holes: Vec<_> = b
        .holes
        .iter()
        .copied()
        .chain(
            b.pads
                .iter()
                .filter(|p| p.drill > 0.0)
                .map(|p| (p.at, p.drill)),
        )
        .chain(r.vias.iter().map(|v| (v.at, b.via[1])))
        .collect();
    for &(at, d) in &holes {
        let drill = Solid::cylinder(d / 2.0, DVec3::Z * thickness).translate(DVec3::new(
            at[0],
            at[1],
            -thickness / 2.0,
        ));
        board = (&board - &drill).build()?;
    }
    let expected_board = b.size[0] * b.size[1] * thickness
        - holes
            .iter()
            .map(|(_, d)| PI * (d / 2.0).powi(2) * thickness)
            .sum::<f64>();
    assert!((board.volume() - expected_board).abs() < 1e-6);
    crate::report(log,format!("3D slab: {} x {} x {thickness} mm drill_holes={} volume={:.6} expected={expected_board:.6} mm3",b.size[0],b.size[1],holes.len(),board.volume()));
    let mut solids = vec![board.color("#246c47")];
    for p in &b.parts {
        let z = if p.bottom {
            -thickness / 2.0 - p.body[2]
        } else {
            thickness / 2.0
        };
        let box_solid = Solid::cube(
            DVec3::new(p.at[0] - p.body[0] / 2.0, p.at[1] - p.body[1] / 2.0, z),
            DVec3::new(
                p.at[0] + p.body[0] / 2.0,
                p.at[1] + p.body[1] / 2.0,
                z + p.body[2],
            ),
        )
        .color(if p.name == "LED1" {
            "#cc3333"
        } else {
            "#303038"
        });
        let expected = p.body.iter().product::<f64>();
        assert!((box_solid.volume() - expected).abs() < 1e-7);
        crate::report(
            log,
            format!(
                "3D part {}: body={:?} mm at={:?} z=[{z:.3},{:.3}] volume={:.6} mm3",
                p.name,
                p.body,
                p.at,
                z + p.body[2],
                box_solid.volume()
            ),
        );
        solids.push(box_solid);
    }
    let build_ms = t.elapsed().as_secs_f64() * 1000.0;
    let volume = solids.iter().map(|s| s.volume()).sum::<f64>();
    let t = Instant::now();
    Solid::write_step(&solids, &mut File::create(out.join("led.step"))?)?;
    let step_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    let imported = Solid::read_step(&mut File::open(out.join("led.step"))?)?;
    let read_ms = t.elapsed().as_secs_f64() * 1000.0;
    let imported_volume = imported.iter().map(|s| s.volume()).sum::<f64>();
    assert_eq!(imported.len(), solids.len());
    assert!((imported_volume - volume).abs() < 1e-6);
    crate::report(log,format!("STEP: solids={} bytes={} total_volume={volume:.6} mm3 imported_volume={imported_volume:.6} mm3 delta={:.12} mm3 build_ms={build_ms:.3} write_ms={step_ms:.3} read_ms={read_ms:.3}",solids.len(),fs::metadata(out.join("led.step"))?.len(),(imported_volume-volume).abs()));
    let t = Instant::now();
    let mut mesh = Solid::mesh(
        &solids,
        Tessellation {
            deflection_linear: 0.05,
            deflection_angular: 0.1,
            relative_linear: false,
        },
    )?;
    let mesh_ms = t.elapsed().as_secs_f64() * 1000.0;
    mesh.scene(SceneOption {
        shading: true,
        hidden_edges: false,
        ..Default::default()
    })
    .write_svg(&mut File::create(out.join("assembly.svg"))?)?;
    // Forge's GLTFLoader applies +90 degrees around X and uses mm, so encode Y-up [x,z,-y] without metre scaling.
    let to_y_up = |p: DVec3| DVec3::new(p.x, p.z, -p.y);
    for p in mesh
        .vertices
        .iter_mut()
        .chain(&mut mesh.normals)
        .chain(&mut mesh.edges)
    {
        *p = to_y_up(*p);
    }
    let t = Instant::now();
    mesh.write_gltf_binary(&mut File::create(out.join("led.glb"))?)?;
    let glb_ms = t.elapsed().as_secs_f64() * 1000.0;
    let bytes = fs::read(out.join("led.glb"))?;
    let (triangles, primitives, min, max) = verify_glb(&bytes, mesh.indices.len() / 3)?;
    let forge_size = [max[0] - min[0], max[2] - min[2], max[1] - min[1]];
    assert!((forge_size[0] - 30.0).abs() < 1e-6);
    assert!((forge_size[1] - 20.0).abs() < 1e-6);
    assert!((forge_size[2] - 11.85).abs() < 1e-5);
    crate::report(log,format!("GLB: reused cadrum::Mesh::write_gltf_binary (no Rust GLB writer in server/src); bytes={} vertices={} triangles={triangles} primitives={primitives} chunk_lengths/indices/bounds=valid mesh_ms={mesh_ms:.3} write_ms={glb_ms:.3}",bytes.len(),mesh.vertices.len()));
    crate::report(log,format!("GLB axes: Y-up file bounds={min:?}..{max:?}; Forge Z-up size={forge_size:?} mm; keep existing mm convention, no x1000 scale"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn kernel_glb_container_roundtrip() {
        let cube = Solid::cube(DVec3::ZERO, DVec3::new(2.0, 3.0, 4.0));
        let mesh = Solid::mesh(
            [&cube],
            Tessellation {
                deflection_linear: 0.1,
                deflection_angular: 0.1,
                relative_linear: false,
            },
        )
        .unwrap();
        let mut bytes = vec![];
        mesh.write_gltf_binary(&mut bytes).unwrap();
        let (triangles, _, min, max) = verify_glb(&bytes, 12).unwrap();
        assert_eq!(triangles, 12);
        assert_eq!(min, [0.0; 3]);
        assert_eq!(max, [2.0, 3.0, 4.0]);
    }
}
