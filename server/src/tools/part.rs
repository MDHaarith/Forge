use super::{input_str, paths::project_path, Tool, ToolContext, ToolError, ToolFuture, ToolResult};
use crate::jsfmt::{fixed, json_number, stringify_pretty};
use cadrum::{Boolean, DVec3, Solid, Tessellation};
use serde_json::{json, Value};
use std::fs;

mod holes;
mod inspect;
mod kit;
mod ops;
mod standards;

pub fn tools() -> Vec<Tool> {
    let outline = json!({"type":"object", "properties":{
        "circle":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},
        "start":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2},
        "segments":{"type":"array","items":{"type":"object","properties":{
            "line":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2},
            "arc":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2},
            "through":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2}
        }}}
    }});
    let edges = json!({"anyOf":[
        {"type":"string","enum":["all","vertical"]},
        {"type":"object","properties":{"at_z":{"type":"number"}},"required":["at_z"]}
    ]});
    let points = json!({"anyOf":[
        {"type":"array","items":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2}},
        {"type":"object","properties":{
            "grid":{"type":"array","items":{"type":"number"}},
            "corners":{"type":"array","items":{"type":"number"}}
        }}
    ]});
    vec![Tool::new(
        "3d",
        "part_build",
        r#"Build exact solids (STEP kernel, not meshes) from an ordered list of steps; writes models/<name>.step, models/<name>.stl (binary) and models/<name>.json (steps, per-body volume, bbox). Lengths are mm, angles degrees, Z up. Choose edges by selector, never by number: "all", "vertical" (straight edges parallel to Z) or {"at_z":z} (edges lying entirely at height z). Steps run in order. A shape step makes a body; id is optional and defaults to s<index> (index counts from 0 over all steps). An operation step names its target body. union/cut/intersect also name a tool body, which is consumed. Every body left at the end is exported. material is optional (used for mass): PLA, PETG, ABS, ASA, Nylon PA12, Polycarbonate, Acrylic, POM (Delrin), Aluminium 6061, Aluminium 7075, Steel, Stainless 304, Brass, Copper, Titanium Ti-6Al-4V or Plywood.
Ops (at defaults to [0,0,0]):
- box: size [x,y,z], at; minimum corner at at.
- cylinder: radius, height, at; axis +Z, base centre at at.
- sphere: radius, at; centre at at.
- cone: radius1 (bottom), radius2 (top, may be 0), height, at; axis +Z.
- extrude: outline, height, at; outline in XY, extruded +Z, then moved by at.
- revolve: outline, angle (360); outline in XZ (x = radius >= 0), turned about Z from +X towards +Y.
- union / cut / intersect: target, tool; an empty intersect is allowed (volume 0).
- fillet: target, radius, edges. chamfer: target, size, edges (equal distance).
- place: target, rotate [rx,ry,rz] (0,0,0), move [x,y,z] (0,0,0); rotates about X, then Y, then Z through the origin, then moves.
- hole: target, points, size ("M3"), kind, fit ("normal"), depth (omit = through).
- enclosure: size [x,y,z], wall (2), floor (2), radius (0); open-top box.
- lid: size [x,y], thickness (2), wall (2), lip (2), gap (0.2), radius (0), at; plate from z=0 to thickness, lip from z=-lip to 0, then moved by at.
- standoffs: target, points, size ("M3"), height (5), base (0), kind ("insert"); posts on the target, base = z where they start.
outline: {"circle":[cx,cy,r]} or {"start":[x,y],"segments":[{"line":[x,y]} | {"arc":[x,y],"through":[x,y]}, ...]}; the last point must equal start.
points: [[x,y],...] or {"grid":[x0,y0,dx,dy,nx,ny]} or {"corners":[length,width,inset]}.
kind: "clearance", "tap", "counterbore", "countersink" or "insert".
Limits: no sketcher constraints, loft, sweep, shell, threads or drawings (use freecad_drawing for drawings while it exists).
Example, 80x50x25 enclosure with 2 mm walls, M3 insert standoffs, and a matching lid beside it at x=90 (at z=2 so its lip rests on the bed):
{"name":"case","steps":[{"op":"enclosure","id":"box","size":[80,50,25],"wall":2,"floor":2},{"op":"standoffs","target":"box","points":{"corners":[80,50,6]},"size":"M3","base":2,"kind":"insert"},{"op":"lid","id":"cover","size":[80,50],"at":[90,0,2]}]}
Errors: step <index>: unknown op "<op>"; step <index>: no body "<id>"; step <index>: fillet failed (or chamfer failed)."#,
        json!({"type":"object", "additionalProperties": false, "properties":{
            "name":{"type":"string", "description":"Lowercase letters, digits, - and _ only"},
            "material":{"type":"string", "description":"Optional MATERIALS key, used for mass"},
            "steps":{"type":"array", "description":"Ordered shape and operation steps; each op may use any of the declared step fields", "minItems":1, "items":{"type":"object", "additionalProperties":false, "properties":{
                "op":{"type":"string", "enum":["box","cylinder","sphere","cone","extrude","revolve","union","cut","intersect","fillet","chamfer","place","hole","enclosure","lid","standoffs"]}, "id":{"type":"string"},
                "size":{"anyOf":[{"type":"number"},{"type":"string"},{"type":"array","items":{"type":"number"}}], "description":"Array for box and enclosure dimensions (lid: [x,y]); string such as M3 for hole and standoffs; number for chamfer"},
                "at":{"type":"array", "items":{"type":"number"}, "minItems":3, "maxItems":3},
                "radius":{"type":"number"}, "radius1":{"type":"number"},
                "radius2":{"type":"number"}, "height":{"type":"number"},
                "angle":{"type":"number"}, "outline":outline, "edges":edges,
                "rotate":{"type":"array", "items":{"type":"number"}, "minItems":3, "maxItems":3},
                "move":{"type":"array", "items":{"type":"number"}, "minItems":3, "maxItems":3},
                "target":{"type":"string"}, "tool":{"type":"string"},
                "points":points, "kind":{"type":"string"}, "fit":{"type":"string"},
                "depth":{"type":"number"}, "wall":{"type":"number"}, "floor":{"type":"number"},
                "thickness":{"type":"number"}, "lip":{"type":"number"}, "gap":{"type":"number"},
                "base":{"type":"number"}
            }, "required":["op"]}}
        }, "required":["name","steps"]}),
        part_build,
    ), inspect::tool()]
}

fn part_build(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        tokio::task::spawn_blocking(move || build(input, ctx))
            .await
            .map_err(|err| ToolError::unexpected(format!("part build failed: {err}")))?
    })
}

fn number(step: &Value, key: &str, index: usize, allow_zero: bool) -> Result<f64, ToolError> {
    step[key]
        .as_f64()
        .filter(|n| n.is_finite() && if allow_zero { *n >= 0.0 } else { *n > 0.0 })
        .ok_or_else(|| {
            ToolError::expected(format!(
                "step {index}: {key} must be {}",
                if allow_zero {
                    "nonnegative"
                } else {
                    "positive"
                }
            ))
        })
}

fn vector(step: &Value, key: &str, index: usize) -> Result<DVec3, ToolError> {
    let values = step[key]
        .as_array()
        .filter(|v| v.len() == 3)
        .ok_or_else(|| {
            ToolError::expected(format!("step {index}: {key} must be three finite numbers"))
        })?;
    let mut xyz = [0.0; 3];
    for (i, value) in values.iter().enumerate() {
        xyz[i] = value.as_f64().filter(|n| n.is_finite()).ok_or_else(|| {
            ToolError::expected(format!("step {index}: {key} must be three finite numbers"))
        })?;
    }
    Ok(DVec3::from_array(xyz))
}

fn body_index(bodies: &[(String, Vec<Solid>)], id: &str, index: usize) -> Result<usize, ToolError> {
    bodies
        .iter()
        .position(|(name, _)| name == id)
        .ok_or_else(|| ToolError::expected(format!("step {index}: no body \"{id}\"")))
}

// shortcut: OpenCASCADE crashes (SIGSEGV) when two threads use it at once, so every part builds or
// inspects one at a time; move kernel work to a worker process when parallel builds are needed.
static KERNEL: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Held for the whole time a thread uses OpenCASCADE, including dropping its shapes.
fn kernel() -> std::sync::MutexGuard<'static, ()> {
    KERNEL.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn build(input: Value, ctx: ToolContext) -> Result<ToolResult, ToolError> {
    let _kernel = kernel();
    let name = input_str(&input, "name")?;
    if name.is_empty()
        || !name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_')
    {
        return Err(ToolError::expected(
            "name may only use lowercase letters, digits, - and _",
        ));
    }
    let material = match input.get("material") {
        None => Value::Null,
        Some(value) => {
            let material = input_str(&input, "material")?;
            standards::material_density(material)?;
            value.clone()
        }
    };
    let steps = input["steps"]
        .as_array()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ToolError::expected("input.steps must be a nonempty array"))?;
    let files: Vec<_> = ["step", "stl", "json"]
        .iter()
        .map(|ext| format!("models/{name}.{ext}"))
        .collect();
    let paths = files
        .iter()
        .map(|file| project_path(&ctx.project_dir, file))
        .collect::<Result<Vec<_>, _>>()?;
    let mut bodies: Vec<(String, Vec<Solid>)> = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        let op = input_str(step, "op")?;
        match op {
            "box" | "cylinder" | "sphere" | "cone" | "extrude" | "revolve" | "enclosure"
            | "lid" => {
                let id = match step.get("id") {
                    Some(_) => input_str(step, "id")?.to_owned(),
                    None => format!("s{index}"),
                };
                if bodies.iter().any(|(name, _)| name == &id) {
                    return Err(ToolError::expected(format!(
                        "step {index}: duplicate body \"{id}\""
                    )));
                }
                let at = if step.get("at").is_some() {
                    vector(step, "at", index)?
                } else {
                    DVec3::ZERO
                };
                let solid = match op {
                    "box" => {
                        let size = vector(step, "size", index)?;
                        if size.min_element() <= 0.0 || !(at + size).is_finite() {
                            return Err(ToolError::expected(format!(
                                "step {index}: size must be positive with finite corners"
                            )));
                        }
                        Solid::cube(at, at + size)
                    }
                    "cylinder" => Solid::cylinder(
                        number(step, "radius", index, false)?,
                        DVec3::Z * number(step, "height", index, false)?,
                    )
                    .translate(at),
                    "sphere" => Solid::sphere(number(step, "radius", index, false)?).translate(at),
                    "cone" => {
                        let r1 = number(step, "radius1", index, true)?;
                        let r2 = number(step, "radius2", index, true)?;
                        let height = number(step, "height", index, false)?;
                        if r1 == 0.0 && r2 == 0.0 {
                            return Err(ToolError::expected(format!(
                                "step {index}: cone needs a positive radius"
                            )));
                        }
                        if r1 == r2 {
                            Solid::cylinder(r1, DVec3::Z * height).translate(at)
                        } else {
                            Solid::cone(r1, r2, DVec3::Z * height).translate(at)
                        }
                    }
                    "extrude" | "revolve" => ops::profile_solid(step, op, index, at)?,
                    "enclosure" | "lid" => kit::shape(step, op, index, at)?,
                    _ => unreachable!(),
                };
                bodies.push((id, vec![solid]));
            }
            "union" | "cut" | "intersect" => {
                let target = body_index(&bodies, input_str(step, "target")?, index)?;
                let tool = body_index(&bodies, input_str(step, "tool")?, index)?;
                if target == tool {
                    return Err(ToolError::expected(format!(
                        "step {index}: target and tool must be different bodies"
                    )));
                }
                let a = bodies[target]
                    .1
                    .iter()
                    .fold(Boolean::<Solid>::default(), |expr, solid| expr + solid);
                let b = bodies[tool]
                    .1
                    .iter()
                    .fold(Boolean::<Solid>::default(), |expr, solid| expr + solid);
                let expression = match op {
                    "union" => a + b,
                    "cut" => a - b,
                    "intersect" => a * b,
                    _ => unreachable!(),
                };
                bodies[target].1 = if expression.clauses().is_empty() {
                    vec![]
                } else {
                    expression.build_vec().map_err(|err| {
                        ToolError::expected(format!("step {index}: {op} failed: {err}"))
                    })?
                };
                bodies.remove(tool);
            }
            "hole" | "standoffs" => {
                let target = body_index(&bodies, input_str(step, "target")?, index)?;
                bodies[target].1 = kit::modify(&bodies[target].1, step, op, index)?;
            }
            "fillet" | "chamfer" | "place" => {
                let target = body_index(&bodies, input_str(step, "target")?, index)?;
                bodies[target].1 = ops::modify(&bodies[target].1, step, op, index)?;
            }
            _ => {
                return Err(ToolError::expected(format!(
                    "step {index}: unknown op \"{op}\""
                )))
            }
        }
    }
    let metadata: Vec<_> = bodies
        .iter()
        .map(|(id, solids)| {
            let bbox = solids
                .iter()
                .map(Solid::bounding_box)
                .reduce(|[min, max], [a, b]| [min.min(a), max.max(b)])
                .unwrap_or([DVec3::ZERO; 2]);
            let bbox = bbox.map(|v| v.to_array().map(|n| json_number(fixed(n, 3))));
            let volume: f64 = solids.iter().map(Solid::volume).sum();
            json!({"id":id,"volume":json_number(fixed(volume,3)),"bbox":bbox,"solids":solids.len()})
        })
        .collect();
    let document = json!({"name":name,"material":material,"steps":steps,"bodies":metadata});
    let json = stringify_pretty(&document)? + "\n";
    let solids: Vec<_> = bodies
        .iter()
        .flat_map(|(_, solids)| solids.iter())
        .collect();
    let mut step = Vec::new();
    Solid::write_step(solids.iter().copied(), &mut step)
        .map_err(|err| ToolError::expected(format!("STEP export failed: {err}")))?;
    // shortcut: fixed 0.1 mm / 0.1 rad tessellation, expose tolerances when finer exports are needed.
    let mesh = Solid::mesh(
        solids.iter().copied(),
        Tessellation {
            deflection_linear: 0.1,
            deflection_angular: 0.1,
            relative_linear: false,
        },
    )
    .map_err(|err| ToolError::expected(format!("STL export failed: {err}")))?;
    let mut stl = Vec::new();
    mesh.write_stl(&mut stl)
        .map_err(|err| ToolError::expected(format!("STL export failed: {err}")))?;
    fs::create_dir_all(project_path(&ctx.project_dir, "models")?)?;
    for (path, data) in paths
        .iter()
        .zip([step.as_slice(), stl.as_slice(), json.as_bytes()])
    {
        fs::write(path, data)?;
    }
    Ok(ToolResult {
        text: format!(
            "built {name}: {} bodies, {} solids; wrote STEP, binary STL and JSON",
            bodies.len(),
            solids.len()
        ),
        files,
    })
}

#[cfg(test)]
#[path = "../../tests/common/mod.rs"]
mod test_common;

#[cfg(test)]
mod tests {
    use super::*;
    use test_common::TempDir;

    fn context(dir: &TempDir) -> ToolContext {
        ToolContext::new(dir.path.clone(), dir.path.clone())
    }

    #[tokio::test]
    async fn validates_input_before_writing() {
        let dir = TempDir::new("part-validation");
        for (input, expected) in [
            (
                json!({"name":"../escape","steps":[{"op":"box","size":[1,1,1]}]}),
                "name may only use lowercase letters, digits, - and _",
            ),
            (
                json!({"name":"Upper","steps":[{"op":"box","size":[1,1,1]}]}),
                "name may only use lowercase letters, digits, - and _",
            ),
            (
                json!({"name":"t","steps":[{"op":"box","size":[1,0,1]}]}),
                "step 0: size must be positive with finite corners",
            ),
            (
                json!({"name":"t","steps":[{"op":"sphere","radius":-1}]}),
                "step 0: radius must be positive",
            ),
            (
                json!({"name":"t","steps":[{"op":"cylinder","radius":1,"height":0}]}),
                "step 0: height must be positive",
            ),
            (
                json!({"name":"t","steps":[{"op":"cone","radius1":0,"radius2":0,"height":1}]}),
                "step 0: cone needs a positive radius",
            ),
            (
                json!({"name":"t","steps":[{"op":"sphere","radius":1,"at":[0,0]}]}),
                "step 0: at must be three finite numbers",
            ),
            (
                json!({"name":"t","steps":[{"op":"teapot"}]}),
                "step 0: unknown op \"teapot\"",
            ),
            (
                json!({"name":"t","steps":[{"op":"cut","target":"x","tool":"y"}]}),
                "step 0: no body \"x\"",
            ),
            (
                json!({"name":"t","steps":[{"id":"a","op":"box","size":[1,1,1]},{"op":"cut","target":"a","tool":"y"}]}),
                "step 1: no body \"y\"",
            ),
            (
                json!({"name":"t","steps":[{"id":"a","op":"box","size":[1,1,1]},{"id":"a","op":"sphere","radius":1}]}),
                "step 1: duplicate body \"a\"",
            ),
        ] {
            assert_eq!(
                part_build(input, context(&dir))
                    .await
                    .unwrap_err()
                    .to_string(),
                expected
            );
        }
        let err = part_build(
            json!({"name":"t","material":"unknown","steps":[{"op":"sphere","radius":1}]}),
            context(&dir),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("valid names: PLA, PETG"));
        assert!(!dir.path.join("models").exists());
    }

    #[tokio::test]
    async fn exports_all_bodies_and_keeps_json_order_and_numbers() {
        let dir = TempDir::new("part-export");
        let steps = json!([
            {"op":"sphere","radius":2,"at":[10,20,30]},
            {"id":"base","op":"box","size":[1.23456,2,3],"at":[-0.0,0,0]}
        ]);
        let result = part_build(
            json!({"name":"t","material":"PLA","steps":steps}),
            context(&dir),
        )
        .await
        .unwrap();
        assert_eq!(
            result.files,
            ["models/t.step", "models/t.stl", "models/t.json"]
        );
        let text = fs::read_to_string(dir.path.join(&result.files[2])).unwrap();
        let out: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            out.as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["name", "material", "steps", "bodies"]
        );
        assert_eq!(
            stringify_pretty(&out["steps"]).unwrap(),
            stringify_pretty(&steps).unwrap()
        );
        assert_eq!(out["bodies"][0]["id"], "s0");
        assert_eq!(out["bodies"][0]["bbox"], json!([[8, 18, 28], [12, 22, 32]]));
        assert_eq!(out["bodies"][1]["bbox"], json!([[0, 0, 0], [1.235, 2, 3]]));
        assert!(!text.contains("-0"));
        let stl = fs::read(dir.path.join(&result.files[1])).unwrap();
        let triangles = u32::from_le_bytes(stl[80..84].try_into().unwrap()) as usize;
        assert!(triangles > 0);
        assert_eq!(stl.len(), 84 + 50 * triangles);
        let path = project_path(&dir.path, &result.files[0]).unwrap();
        tokio::task::spawn_blocking(move || {
            let _kernel = kernel();
            let solids = Solid::read_step(&mut fs::File::open(path).unwrap()).unwrap();
            assert_eq!(solids.len(), 2);
            assert!(
                (solids.iter().map(Solid::volume).sum::<f64>()
                    - (32.0 * std::f64::consts::PI / 3.0 + 7.40736))
                    .abs()
                    < 1e-6
            );
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn booleans_keep_disconnected_solids_and_empty_bodies() {
        let dir = TempDir::new("part-boolean");
        for (steps, volume, count) in [
            (
                json!([
                    {"id":"b","op":"box","size":[2,2,2],"at":[10,0,0]},
                    {"id":"a","op":"box","size":[2,2,2]},
                    {"op":"union","target":"a","tool":"b"},
                    {"id":"c","op":"box","size":[2,2,2]},
                    {"op":"cut","target":"a","tool":"c"}
                ]),
                8,
                1,
            ),
            (
                json!([
                    {"id":"a","op":"box","size":[2,2,2]},
                    {"id":"b","op":"box","size":[2,2,2],"at":[10,0,0]},
                    {"op":"union","target":"a","tool":"b"}
                ]),
                16,
                2,
            ),
            (
                json!([
                    {"id":"a","op":"box","size":[2,2,2]},
                    {"id":"b","op":"box","size":[2,2,2],"at":[10,0,0]},
                    {"op":"intersect","target":"a","tool":"b"}
                ]),
                0,
                0,
            ),
            (
                json!([
                    {"id":"a","op":"box","size":[2,2,2]},
                    {"id":"b","op":"box","size":[2,2,2],"at":[10,0,0]},
                    {"op":"intersect","target":"a","tool":"b"},
                    {"id":"c","op":"cone","radius1":1,"radius2":1,"height":1},
                    {"op":"cut","target":"a","tool":"c"}
                ]),
                0,
                0,
            ),
        ] {
            part_build(json!({"name":"t","steps":steps}), context(&dir))
                .await
                .unwrap();
            let out: Value =
                serde_json::from_str(&fs::read_to_string(dir.path.join("models/t.json")).unwrap())
                    .unwrap();
            assert_eq!(out["bodies"].as_array().unwrap().len(), 1);
            assert_eq!(out["bodies"][0]["id"], "a");
            assert_eq!(out["bodies"][0]["volume"], volume);
            assert_eq!(out["bodies"][0]["solids"], count);
            if count == 0 {
                assert_eq!(out["bodies"][0]["bbox"], json!([[0, 0, 0], [0, 0, 0]]));
                assert_eq!(fs::read(dir.path.join("models/t.stl")).unwrap().len(), 84);
            }
        }
    }
}
