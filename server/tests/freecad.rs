mod common;
#[path = "common/tools.rs"]
mod support;

use forge_server::{
    open::{open_in_app, opener_types},
    tools::{
        exec::find_program,
        launch::resolve_app,
        script3d::{with_override, FREECAD},
    },
    Settings,
};
use serde_json::json;
use support::{app_dir, project, run};

async fn has_freecad() -> bool {
    let settings = Settings::from_env(app_dir());
    let spec = with_override(&FREECAD, settings.freecad_program.as_deref());
    tokio::task::spawn_blocking(move || resolve_app(&spec).is_some())
        .await
        .unwrap()
}

// A 60 × 40 × 8 plate using every kind of standard hole from the forge helpers.
const PLATE: &str = r#"
p = forge.rounded_box(60, 40, 8, radius=4)
p = forge.hole(p, 6, 6, "M3", kind="counterbore")
p = forge.hole(p, 54, 6, "M4", kind="countersink")
p = forge.hole(p, 54, 34, "M3", kind="tap", depth=6)
p = forge.hole(p, 6, 34, "M3", kind="insert")
result = forge.hole(p, 30, 20, "M5")
"#;

#[tokio::test]
async fn inputs_are_checked_before_freecad_starts() {
    let dir = project(
        "fc",
        &[
            ("models/a.stl", "solid x\nendsolid x\n"),
            ("models/b.step", "x"),
        ],
    );
    for (name, input, expected) in [
        (
            "freecad_inspect",
            json!({"models":["models/a.stl"]}),
            "STEP, BREP, IGES or FCStd",
        ),
        (
            "freecad_inspect",
            json!({"models":["models/none.step"]}),
            "does not exist",
        ),
        (
            "freecad_inspect",
            json!({"models":["models/b.step"],"material":"cheese"}),
            "unknown material",
        ),
        (
            "freecad_inspect",
            json!({"models":["../b.step"]}),
            "outside the project",
        ),
        (
            "freecad_drawing",
            json!({"model":"models/b.step","name":"a b"}),
            "name may only",
        ),
    ] {
        let err = run(name, input, &dir.path).await.unwrap_err().to_string();
        assert!(err.contains(expected), "{err}");
    }
}

#[tokio::test]
async fn forge_helpers_cut_iso_holes_and_inspection_identifies_each_with_mass_and_print_data() {
    if !has_freecad().await {
        return eprintln!("skipped: freecadcmd not installed");
    }
    let dir = project("fc", &[("cad/plate.py", PLATE)]);
    let built = run(
        "freecad_build",
        json!({"script":"cad/plate.py","name":"plate","formats":["step"]}),
        &dir.path,
    )
    .await
    .unwrap();
    assert!(
        built
            .text
            .lines()
            .any(|l| l.contains("60 × 40 × 8 mm") && l.contains("valid: true")),
        "{}",
        built.text
    );

    let out = run(
        "freecad_inspect",
        json!({"models":["models/plate.step"],"material":"aluminium 6061"}),
        &dir.path,
    )
    .await
    .unwrap();
    for expected in [
        "5 hole(s)",
        "Ø3.4 through along Z at (6, 6, 8), counterbore Ø6.5 × 3.3 [M3 clearance]",
        "Ø2.5 6 deep along Z at (54, 34, 8) [M3 tap]",
        "0 mm² overhangs steeper than 45°",
    ] {
        assert!(out.text.contains(expected), "{}", out.text);
    }
    for (start, end) in [
        ("Ø4.5 through ", "countersink Ø8.96 [M4 clearance]"),
        ("Ø4 6.2 deep ", "[M3 insert]"),
        ("Ø5.5 through ", "[M5 clearance]"),
    ] {
        assert!(
            out.text.lines().any(|l| l
                .split_once(start)
                .is_some_and(|(_, tail)| tail.contains(end))),
            "{}",
            out.text
        );
    }
    let volume: f64 = out
        .text
        .split("volume ")
        .nth(1)
        .unwrap()
        .split(" mm³")
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let mass: f64 = out
        .text
        .split("mass ")
        .nth(1)
        .unwrap()
        .split(" g in Aluminium 6061")
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!((mass - volume / 1000.0 * 2.7).abs() < 0.01);
}

#[tokio::test]
async fn rounded_pocket_corners_and_edge_chamfers_are_not_holes_and_blind_holes_are_m2_5_inserts() {
    if !has_freecad().await {
        return eprintln!("skipped: freecadcmd not installed");
    }
    let dir = project(
        "fc",
        &[(
            "cad/box.py",
            r#"
box = forge.enclosure(90, 60, 32, wall=2.4, floor=2.4, radius=5)
box = forge.standoffs(box, forge.grid(12.5, 15, 65, 30), "M2.5", height=5, base=2.4)
box = forge.hole(box, 45, 30, "M3", top=2.4)
result = forge.chamfer(box, 0.4, forge.edges_at(box, 0))
"#,
        )],
    );
    run(
        "freecad_build",
        json!({"script":"cad/box.py","name":"box","formats":["step"]}),
        &dir.path,
    )
    .await
    .unwrap();
    let out = run(
        "freecad_inspect",
        json!({"models":["models/box.step"]}),
        &dir.path,
    )
    .await
    .unwrap();
    assert!(out.text.contains("5 hole(s)"), "{}", out.text);
    assert_eq!(
        out.text
            .lines()
            .filter(|l| l
                .split_once("Ø3.6 5 deep along Z ")
                .is_some_and(|(_, tail)| tail.contains("[M2.5 insert]")))
            .count(),
        4,
        "{}",
        out.text
    );
    assert!(
        out.text
            .contains("Ø3.4 through along Z at (45, 30, 2.4) [M3 clearance]"),
        "{}",
        out.text
    );
    assert!(!out.text.contains("countersink"), "{}", out.text);
}

#[tokio::test]
async fn inspection_finds_interference_and_clearance_between_parts_of_an_assembly() {
    if !has_freecad().await {
        return eprintln!("skipped: freecadcmd not installed");
    }
    let dir = project(
        "fc",
        &[(
            "cad/asm.py",
            r#"
a = doc.addObject("Part::Feature", "A"); a.Shape = Part.makeBox(10, 10, 10)
b = doc.addObject("Part::Feature", "B"); b.Shape = Part.makeBox(10, 10, 10, Vector(8, 0, 0))
c = doc.addObject("Part::Feature", "C"); c.Shape = Part.makeBox(10, 10, 10, Vector(0, 11.5, 0))
"#,
        )],
    );
    run(
        "freecad_build",
        json!({"script":"cad/asm.py","name":"asm","formats":["step"]}),
        &dir.path,
    )
    .await
    .unwrap();
    let out = run(
        "freecad_inspect",
        json!({"models":["models/asm.step"]}),
        &dir.path,
    )
    .await
    .unwrap();
    assert!(
        out.text
            .contains("asm.step#1 and asm.step#2: INTERFERE by 200 mm³"),
        "{}",
        out.text
    );
    assert!(
        out.text
            .contains("asm.step#1 and asm.step#3: clearance 1.5 mm"),
        "{}",
        out.text
    );
}

#[tokio::test]
async fn drawing_has_views_dimensions_hole_table_title_block_dxf_and_pdf() {
    if !has_freecad().await {
        return eprintln!("skipped: freecadcmd not installed");
    }
    let dir = project("fc", &[("cad/plate.py", PLATE)]);
    run(
        "freecad_build",
        json!({"script":"cad/plate.py","name":"plate","formats":["step"]}),
        &dir.path,
    )
    .await
    .unwrap();
    let out = run("freecad_drawing", json!({"model":"models/plate.step","name":"plate","title":"Mounting plate","material":"PLA","dxf":true}), &dir.path).await.unwrap();
    for expected in [
        "Drawing on A3 at 2:1, first angle projection, part 60 × 40 × 8 mm",
        "H1: Ø3.4 THRU  CBORE Ø6.5 ↧3.3, axis Z, X 6  Y 6 (M3 clearance)",
        "H5: Ø2.5 ↧6, axis Z, X 54  Y 34 (M3 tap)",
    ] {
        assert!(out.text.contains(expected), "{}", out.text);
    }
    let svg = std::fs::read_to_string(dir.path.join("drawings/plate.svg")).unwrap();
    assert!(svg
        .starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"420mm\" height=\"297mm\""));
    for text in [
        "FRONT",
        "TOP",
        "LEFT",
        "ISOMETRIC",
        "Mounting plate",
        "PLA",
        "ISO 2768-m",
        "2:1",
        ">60<",
        ">40<",
        ">8<",
        "H5",
        "M3 tap",
    ] {
        assert!(svg.contains(text), "drawing lacks {text}");
    }
    assert!(
        svg.matches("class=\"hid\"").count() > 4,
        "hidden lines are drawn"
    );
    let dxf = std::fs::read_to_string(dir.path.join("drawings/plate-top.dxf")).unwrap();
    assert!(dxf.starts_with("0\nSECTION\n2\nENTITIES\n"));
    assert!(dxf.ends_with("0\nEOF\n"));
    assert!(dxf.matches("\nCIRCLE\n").count() + dxf.matches("\nARC\n").count() >= 9);
    if find_program(&["rsvg-convert".into()]).is_some() {
        assert!(out.files.iter().any(|f| f == "drawings/plate.pdf"));
        assert_eq!(
            &std::fs::read(dir.path.join("drawings/plate.pdf")).unwrap()[..5],
            b"%PDF-"
        );
    } else {
        assert!(out.text.contains("librsvg2-tools"));
    }

    let third = run(
        "freecad_drawing",
        json!({"model":"models/plate.step","name":"plate-us","projection":"third","sheet":"A3"}),
        &dir.path,
    )
    .await
    .unwrap();
    let scale = third
        .text
        .strip_prefix("Drawing on A3 at ")
        .unwrap()
        .split_once(":1, third angle projection")
        .unwrap()
        .0;
    assert!(
        !scale.is_empty() && scale.bytes().all(|b| b.is_ascii_digit()),
        "{}",
        third.text
    );
    assert!(
        std::fs::read_to_string(dir.path.join("drawings/plate-us.svg"))
            .unwrap()
            .contains("RIGHT")
    );
}

#[tokio::test]
async fn partdesign_bodies_built_from_constrained_sketches_export_as_the_part() {
    if !has_freecad().await {
        return eprintln!("skipped: freecadcmd not installed");
    }
    let dir = project(
        "fc",
        &[(
            "cad/pd.py",
            r#"
import Sketcher
body = doc.addObject("PartDesign::Body", "Body")
sk = body.newObject("Sketcher::SketchObject", "Profile")
sk.AttachmentSupport = (doc.getObject("XY_Plane"), [""])
sk.MapMode = "FlatFace"
pts = [Vector(0, 0, 0), Vector(30, 0, 0), Vector(30, 20, 0), Vector(0, 20, 0)]
for i in range(4):
    sk.addGeometry(Part.LineSegment(pts[i], pts[(i + 1) % 4]))
for i in range(4):
    sk.addConstraint(Sketcher.Constraint("Coincident", i, 2, (i + 1) % 4, 1))
sk.addConstraint(Sketcher.Constraint("DistanceX", 0, 1, 0, 2, 30))
sk.addConstraint(Sketcher.Constraint("DistanceY", 1, 1, 1, 2, 20))
pad = body.newObject("PartDesign::Pad", "Pad")
pad.Profile = sk
pad.Length = 5
"#,
        )],
    );
    let out = run(
        "freecad_build",
        json!({"script":"cad/pd.py","name":"pd","formats":["step","fcstd"]}),
        &dir.path,
    )
    .await
    .unwrap();
    assert!(
        out.text
            .contains("1 solid(s): 30 × 20 × 5 mm, volume 3000 mm³, valid: true"),
        "{}",
        out.text
    );
    // Rebuilding replaces the FCStd without leaving FreeCAD's backup files in the project.
    run(
        "freecad_build",
        json!({"script":"cad/pd.py","name":"pd","formats":["fcstd"]}),
        &dir.path,
    )
    .await
    .unwrap();
    let mut files = std::fs::read_dir(dir.path.join("models"))
        .unwrap()
        .map(|f| f.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    files.sort();
    assert_eq!(files, ["pd.FCStd", "pd.step"]);
}

#[tokio::test]
async fn project_files_open_in_freecad_sandboxed_to_the_project_when_it_is_a_flatpak() {
    use std::os::unix::fs::PermissionsExt;
    let dir = project("open", &[("proj/models/part.FCStd", "x")]);
    let log = dir.path.join("opened.log");
    let stub = dir.path.join("freecad-stub");
    std::fs::write(
        &stub,
        format!("#!/bin/sh\necho \"$@\" >> '{}'\n", log.display()),
    )
    .unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let proj = dir.path.join("proj");
    let mut settings = Settings::new(dir.path.clone(), app_dir());
    settings.freecad_gui_program = Some(stub);
    let copy = settings.clone();
    let types = tokio::task::spawn_blocking(move || opener_types(&copy))
        .await
        .unwrap();
    let app = open_in_app(&proj, "models/part.FCStd", &settings)
        .await
        .unwrap();
    let mut errors = Vec::new();
    for rel in ["models/x.txt", "../etc/passwd.step", "models/none.step"] {
        errors.push(open_in_app(&proj, rel, &settings).await.unwrap_err().status);
    }
    assert_eq!(
        json!({"app":app,"fcstd":types[".fcstd"],"step":types[".step"],"errors":errors}),
        json!({"app":"FreeCAD","fcstd":"FreeCAD","step":"FreeCAD","errors":[400,400,404]})
    );
    for _ in 0..50 {
        if log.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(
        std::fs::read_to_string(log).unwrap().trim(),
        proj.join("models/part.FCStd").to_string_lossy()
    );
}
