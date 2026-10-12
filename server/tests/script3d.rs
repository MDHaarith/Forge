mod common;
use common::TempDir;
use forge_server::tools::{
    all_tools,
    launch::resolve_app,
    script3d::{BLENDER, FREECAD},
    ToolContext, ToolResult,
};
use serde_json::{json, Value};
use std::path::Path;

fn app_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn project(prefix: &str, files: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new(prefix);
    for (rel, text) in files {
        let full = dir.path.join(rel);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, text).unwrap();
    }
    dir
}

async fn run(name: &str, input: Value, dir: &Path) -> Result<ToolResult, String> {
    let tool = all_tools().into_iter().find(|t| t.name == name).unwrap();
    (tool.run)(input, ToolContext::new(dir.to_path_buf(), app_dir()))
        .await
        .map_err(|e| e.to_string())
}

#[tokio::test]
async fn script_tools_refuse_missing_scripts_and_bad_names_before_launching() {
    let dir = project("3d", &[]);
    let err = run("freecad_build", json!({"script":"cad/none.py","name":"x"}), &dir.path).await.unwrap_err();
    assert!(err.contains("does not exist"), "{err}");
    let err = run("blender_build", json!({"script":"a.py","name":"../x"}), &dir.path).await.unwrap_err();
    assert!(err.contains("name may only"), "{err}");
}

#[tokio::test]
async fn freecad_builds_a_drilled_plate_and_exports_step_stl_and_fcstd() {
    if resolve_app(&FREECAD).is_none() {
        return eprintln!("skipped: freecadcmd not installed");
    }
    let dir = project("3d", &[("cad/plate.py", "
plate = Part.makeBox(60, 40, 5)
for x, y in [(5, 5), (55, 5), (5, 35), (55, 35)]:
    plate = plate.cut(Part.makeCylinder(1.6, 5, App.Vector(x, y, 0)))
result = plate
")]);
    let out = run("freecad_build", json!({"script":"cad/plate.py","name":"plate"}), &dir.path).await.unwrap();
    assert!(out.text.contains("60 × 40 × 5 mm"), "{}", out.text);
    assert!(out.text.contains("valid: true"), "{}", out.text);
    let volume = 60.0 * 40.0 * 5.0 - 4.0 * std::f64::consts::PI * 1.6f64.powi(2) * 5.0;
    let reported: f64 = out.text.split("volume ").nth(1).unwrap().split(' ').next().unwrap().parse().unwrap();
    assert!((reported - volume).abs() < 0.01, "{}", out.text);
    assert_eq!(out.files, ["models/plate.step", "models/plate.stl", "models/plate.FCStd"]);
    assert!(std::fs::read_to_string(dir.path.join("models/plate.step")).unwrap().starts_with("ISO-10303-21;"));
    assert!(std::fs::metadata(dir.path.join("models/plate.stl")).unwrap().len() > 1000);
}

#[tokio::test]
async fn freecad_reports_script_errors_to_the_agent() {
    if resolve_app(&FREECAD).is_none() {
        return eprintln!("skipped: freecadcmd not installed");
    }
    let dir = project("3d", &[("cad/bad.py", "result = Part.makeBox(1, 1)\n")]);
    let err = run("freecad_build", json!({"script":"cad/bad.py","name":"bad"}), &dir.path).await.unwrap_err();
    assert!(err.contains("FreeCAD: the script raised an error"), "{err}");
}

#[tokio::test]
async fn blender_builds_exports_and_renders_a_scene() {
    if resolve_app(&BLENDER).is_none() {
        return eprintln!("skipped: blender not installed");
    }
    let dir = project("3d", &[("art/scene.py", r#"
import bpy
bpy.ops.mesh.primitive_uv_sphere_add(radius=1, location=(0, 0, 1))
ball = bpy.context.object
ball.modifiers.new("smooth", "SUBSURF").levels = 2
mat = bpy.data.materials.new("glossy")
mat.use_nodes = True
mat.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.8, 0.2, 0.1, 1)
ball.data.materials.append(mat)
bpy.ops.mesh.primitive_plane_add(size=6)
"#)]);
    let out = run("blender_build", json!({"script":"art/scene.py","name":"ball","formats":["glb","stl"],"render":true}), &dir.path).await.unwrap();
    assert!(out.text.contains("Blender built 2 objects"), "{}", out.text);
    assert_eq!(out.files, ["models/ball.glb", "models/ball.stl", "renders/ball.png"]);
    assert_eq!(&std::fs::read(dir.path.join("models/ball.glb")).unwrap()[..4], b"glTF");
    assert_eq!(&std::fs::read(dir.path.join("renders/ball.png")).unwrap()[1..4], b"PNG");
}

#[tokio::test]
async fn blender_reports_script_errors_to_the_agent() {
    if resolve_app(&BLENDER).is_none() {
        return eprintln!("skipped: blender not installed");
    }
    let dir = project("3d", &[("art/bad.py", "import bpy\nbpy.ops.mesh.primitive_no_such_thing()\n")]);
    let err = run("blender_build", json!({"script":"art/bad.py","name":"bad"}), &dir.path).await.unwrap_err();
    assert!(err.contains("Blender: the script raised an error"), "{err}");
}
