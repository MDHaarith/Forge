use super::{input_str, project_path, Tool, ToolContext, ToolError, ToolFuture, ToolResult};
use crate::jsfmt::{fixed_string, stringify_pretty};
use cadrum::{DVec2, DVec3, Mesh, SceneOption, Solid, Tessellation};
use serde_json::{json, Value};
use std::{fmt::Write, fs};

pub(super) fn tool() -> Tool {
    Tool::new(
        "3d",
        "part_drawing",
        r#"Draw a project-relative STEP or STP file; writes drawings/<name>.svg and, with dxf:true, drawings/<name>-top.dxf (ASCII R12, top visible outline at 1:1, minimum corner at [0,0]). Returns one JSON object: svg, dxf (path or null), sheet, scale, views ["front","top","right","iso"] and holes (Z-axis hole count). Lengths are mm, Z up. name defaults to the file stem and accepts lowercase letters, digits, - and _; title defaults to name. material is an optional MATERIALS key for mass in g. projection is first (default) or third angle; sheet is auto (default), A4, A3 or A2, landscape. Auto selects the first sheet fitting 1:1, then the largest standard scale that fits; otherwise reduces on A2. Orthographic views have dashed hidden lines, overall X/Y/Z dimensions, Z-axis hole tags and a hole table with size, depth and coordinates from the minimum corner. The title block shows title, material, mass, scale, sheet, projection and ISO 2768-m. The iso view has no hidden lines and is not to scale. Use it after part_build to make a technical drawing. Limits: no PDF (print the SVG from a browser); DXF curves are short lines.
Example: {"path":"models/case.step","title":"Enclosure","material":"PLA","projection":"third","sheet":"auto","dxf":true}"#,
        json!({"type":"object","additionalProperties":false,"properties":{
            "path":{"type":"string","description":"Project-relative .step or .stp file"},
            "name":{"type":"string","description":"Lowercase letters, digits, - and _; defaults to file stem"},
            "title":{"type":"string","description":"Title block text; defaults to name"},
            "material":{"type":"string","description":"Optional MATERIALS key, used for mass"},
            "projection":{"type":"string","enum":["first","third"],"description":"First angle (default) or third angle view placement"},
            "sheet":{"type":"string","enum":["auto","A4","A3","A2"],"description":"Landscape sheet size; auto chooses the first sheet fitting 1:1, otherwise reduces on A2"},
            "dxf":{"type":"boolean","description":"Write the top visible outline as ASCII R12 DXF at 1:1 (default false)"}
        },"required":["path"]}),
        part_drawing,
    )
}

fn part_drawing(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        tokio::task::spawn_blocking(move || drawing(input, ctx))
            .await
            .map_err(|err| ToolError::unexpected(format!("part drawing failed: {err}")))?
    })
}

const SHEETS: [(&str, DVec2); 3] = [
    ("A4", DVec2::new(297.0, 210.0)),
    ("A3", DVec2::new(420.0, 297.0)),
    ("A2", DVec2::new(594.0, 420.0)),
];
const SCALES: [(f64, &str); 8] = [
    (5.0, "5:1"),
    (2.0, "2:1"),
    (1.0, "1:1"),
    (0.5, "1:2"),
    (0.2, "1:5"),
    (0.1, "1:10"),
    (0.05, "1:20"),
    (0.02, "1:50"),
];

fn sheet_scale(
    size: DVec3,
    requested: &str,
) -> Result<(&'static str, DVec2, f64, &'static str), ToolError> {
    let fits = |sheet: DVec2, scale: f64| {
        (size.x + size.y) * scale + 60.0 <= sheet.x - 20.0
            && (size.y + size.z) * scale + 60.0 <= sheet.y - 60.0
    };
    let (name, sheet) = if requested == "auto" {
        SHEETS
            .iter()
            .copied()
            .find(|(_, sheet)| fits(*sheet, 1.0))
            .unwrap_or(SHEETS[2])
    } else {
        SHEETS
            .iter()
            .copied()
            .find(|(name, _)| *name == requested)
            .ok_or_else(|| ToolError::expected("sheet must be auto, A4, A3 or A2"))?
    };
    let (scale, label) = SCALES
        .into_iter()
        .find(|(scale, _)| fits(sheet, *scale))
        .ok_or_else(|| ToolError::expected("part does not fit on A2 at 1:50"))?;
    Ok((name, sheet, scale, label))
}

struct View {
    visible: Vec<Vec<DVec2>>,
    hidden: Vec<Vec<DVec2>>,
    min: DVec2,
    max: DVec2,
}

impl View {
    fn new(mesh: &Mesh, view: DVec3, up: DVec3) -> Self {
        let scene = mesh.scene(SceneOption {
            view,
            up,
            hidden_edges: true,
            shading: false,
        });
        let visible = joined_polylines(&scene.edges_visible);
        let hidden = joined_polylines(&scene.edges_hidden);
        let [min, max] = visible.iter().chain(&hidden).flatten().fold(
            [DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)],
            |[min, max], p| [min.min(*p), max.max(*p)],
        );
        Self {
            visible,
            hidden,
            min,
            max,
        }
    }

    fn offset(&self, center: DVec2, scale: f64) -> DVec2 {
        let middle = (self.min + self.max) / 2.0;
        center - DVec2::new(middle.x, -middle.y) * scale
    }
}

fn drawing(input: Value, ctx: ToolContext) -> Result<ToolResult, ToolError> {
    let _kernel = super::kernel();
    let file = input_str(&input, "path")?;
    let path = project_path(&ctx.project_dir, file)?;
    if !matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("step" | "stp")
    ) {
        return Err(ToolError::expected("path must be a .step or .stp file"));
    }
    let name = input
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_else(|| path.file_stem().and_then(|s| s.to_str()).unwrap_or(""));
    if name.is_empty()
        || !name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_')
    {
        return Err(ToolError::expected(
            "name may only use lowercase letters, digits, - and _",
        ));
    }
    let title = input.get("title").and_then(Value::as_str).unwrap_or(name);
    let material = input.get("material").and_then(Value::as_str);
    let density = material
        .map(super::standards::material_density)
        .transpose()?;
    let projection = input
        .get("projection")
        .and_then(Value::as_str)
        .unwrap_or("first");
    if !matches!(projection, "first" | "third") {
        return Err(ToolError::expected("projection must be first or third"));
    }
    let requested = input.get("sheet").and_then(Value::as_str).unwrap_or("auto");
    let svg_file = format!("drawings/{name}.svg");
    let svg_path = project_path(&ctx.project_dir, &svg_file)?;
    let dxf_file = input
        .get("dxf")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        .then(|| format!("drawings/{name}-top.dxf"));
    let dxf_path = dxf_file
        .as_ref()
        .map(|file| project_path(&ctx.project_dir, file))
        .transpose()?;
    let solids = Solid::read_step(&mut fs::File::open(&path)?)
        .map_err(|err| ToolError::expected(format!("STEP import failed: {err}")))?;
    let mesh = Solid::mesh(
        solids.iter(),
        Tessellation {
            deflection_linear: 0.05,
            deflection_angular: 0.1,
            relative_linear: false,
        },
    )
    .map_err(|err| ToolError::expected(format!("drawing mesh failed: {err}")))?;
    let [min, max] = mesh.vertices.iter().fold(
        [DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)],
        |[min, max], p| [min.min(*p), max.max(*p)],
    );
    if !min.is_finite() || !max.is_finite() {
        return Err(ToolError::expected("STEP file has no solids"));
    }
    let size = max - min;
    let (sheet_name, sheet, scale, scale_label) = sheet_scale(size, requested)?;
    let holes: Vec<_> = super::holes::holes_in(&solids, &mesh)
        .into_iter()
        .filter(|hole| hole["axis"] == "Z")
        .collect();
    // Mesh::scene uses the camera's direction from the part, opposite the viewing ray.
    let views = [
        View::new(&mesh, -DVec3::Y, DVec3::Z),
        View::new(&mesh, DVec3::Z, DVec3::Y),
        View::new(&mesh, DVec3::X, DVec3::Z),
        View::new(&mesh, DVec3::new(1.0, -1.0, 1.0), DVec3::Z),
    ];
    let third = projection == "third";
    let columns = if third {
        [size.x, size.y]
    } else {
        [size.y, size.x]
    }
    .map(|n| n * scale);
    let rows = if third {
        [size.y, size.z]
    } else {
        [size.z, size.y]
    }
    .map(|n| n * scale);
    let origin = DVec2::new(
        (sheet.x - columns[0] - columns[1] - 20.0) / 2.0,
        (sheet.y - 40.0 - rows[0] - rows[1] - 20.0) / 2.0,
    );
    let centers_x = [
        origin.x + columns[0] / 2.0,
        origin.x + columns[0] + 20.0 + columns[1] / 2.0,
    ];
    let centers_y = [
        origin.y + rows[0] / 2.0,
        origin.y + rows[0] + 20.0 + rows[1] / 2.0,
    ];
    let (corner, corner_size) = if third {
        let x = origin.x + columns[0] + 20.0;
        (
            DVec2::new(x, 30.0),
            DVec2::new(sheet.x - 30.0 - x, origin.y + rows[0] - 30.0),
        )
    } else {
        let y = origin.y + rows[0] + 20.0;
        (
            DVec2::new(30.0, y),
            DVec2::new(origin.x + columns[0] - 30.0, sheet.y - 70.0 - y),
        )
    };
    let cells = if third {
        [(0, 1), (0, 0), (1, 1), (1, 0)]
    } else {
        [(1, 0), (1, 1), (0, 0), (0, 1)]
    };
    let mut svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}mm\" height=\"{}mm\" viewBox=\"0 0 {} {}\" font-size=\"3\">\n<style>.visible{{fill:none;stroke:#111;stroke-width:0.5}}.hidden{{fill:none;stroke:#666;stroke-width:0.25;stroke-dasharray:2 1}}.dimension{{fill:none;stroke:#666;stroke-width:0.25}}text{{font-family:sans-serif;fill:#111}}</style>\n<rect width=\"100%\" height=\"100%\" fill=\"white\"/>\n", n(sheet.x), n(sheet.y), n(sheet.x), n(sheet.y));
    for (i, (view, name)) in views
        .iter()
        .zip(["front", "top", "right", "iso"])
        .enumerate()
    {
        let (col, row) = cells[i];
        let mut center = DVec2::new(centers_x[col], centers_y[row]);
        let view_scale = if i == 3 {
            let height = corner_size.y * if holes.is_empty() { 1.0 } else { 0.5 };
            center = corner + DVec2::new(corner_size.x, height) / 2.0;
            let extent = view.max - view.min;
            (corner_size.x * 0.8 / extent.x)
                .min(height * 0.8 / extent.y)
                .min(1.0)
        } else {
            scale
        };
        let offset = view.offset(center, view_scale);
        writeln!(svg, "<g id=\"view-{name}\">").unwrap();
        if i != 3 {
            svg_paths(&mut svg, "hidden", &view.hidden, offset, view_scale);
        }
        svg_paths(&mut svg, "visible", &view.visible, offset, view_scale);
        let extent = (view.max - view.min) * view_scale;
        if i == 1 {
            dimension(&mut svg, center, extent, size.x, true);
            dimension(&mut svg, center, extent, size.y, false);
            for (j, hole) in holes.iter().enumerate() {
                let p = DVec2::new(
                    hole["center"][0].as_f64().unwrap(),
                    -hole["center"][1].as_f64().unwrap(),
                ) * scale
                    + offset;
                text(&mut svg, p.x + 2.0, p.y - 2.0, &format!("H{}", j + 1));
            }
        } else if i == 0 {
            dimension(&mut svg, center, extent, size.z, false);
        }
        text(
            &mut svg,
            center.x - 8.0,
            center.y - extent.y / 2.0 - 5.0,
            if i == 3 { "iso (not to scale)" } else { name },
        );
        svg.push_str("</g>\n");
    }
    if !holes.is_empty() {
        let x = corner.x;
        let y = corner.y + corner_size.y / 2.0;
        let row_height = (corner_size.y / 2.0 / (holes.len() + 2) as f64).min(5.0);
        writeln!(
            svg,
            "<g id=\"hole-table\" style=\"font-size:{}px\">",
            n((row_height * 0.6).min(3.0))
        )
        .unwrap();
        for (j, label) in ["Tag", "Size", "Depth", "X", "Y"].iter().enumerate() {
            text(
                &mut svg,
                x + corner_size.x * [0.0, 0.1, 0.5, 0.7, 0.85][j],
                y + row_height,
                label,
            );
        }
        for (i, hole) in holes.iter().enumerate() {
            let values = [
                format!("H{}", i + 1),
                hole["fits"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("⌀{}", n(hole["diameter"].as_f64().unwrap()))),
                if hole["through"] == true {
                    "THRU".into()
                } else {
                    n(hole["depth"].as_f64().unwrap())
                },
                n(hole["center"][0].as_f64().unwrap() - min.x),
                n(hole["center"][1].as_f64().unwrap() - min.y),
            ];
            for (j, value) in values.iter().enumerate() {
                text(
                    &mut svg,
                    x + corner_size.x * [0.0, 0.1, 0.5, 0.7, 0.85][j],
                    y + row_height * (i + 2) as f64,
                    value,
                );
            }
        }
        svg.push_str("</g>\n");
    }
    let mass = density.map(|rho| n(solids.iter().map(Solid::volume).sum::<f64>() * rho / 1000.0));
    let y = sheet.y - 50.0;
    writeln!(svg, "<g id=\"title-block\"><rect class=\"dimension\" x=\"10\" y=\"{}\" width=\"{}\" height=\"40\"/>", n(y), n(sheet.x - 20.0)).unwrap();
    for (i, value) in [
        title.to_owned(),
        format!("Material: {}", material.unwrap_or("—")),
        format!("Mass: {} g", mass.as_deref().unwrap_or("—")),
        format!("Scale: {scale_label}    Sheet: {sheet_name}"),
        format!("{projection} angle    ISO 2768-m"),
    ]
    .iter()
    .enumerate()
    {
        text(&mut svg, 15.0, y + 7.0 + i as f64 * 6.0, value);
    }
    svg.push_str("</g>\n</svg>\n");
    // shortcut: no PDF output, print the SVG from a browser; add PDF when a renderer is bundled.
    fs::create_dir_all(project_path(&ctx.project_dir, "drawings")?)?;
    fs::write(svg_path, svg)?;
    let mut files = vec![svg_file.clone()];
    if let Some(path) = dxf_path {
        fs::write(path, dxf(&views[1].visible, min.truncate()))?;
        files.push(dxf_file.clone().unwrap());
    }
    Ok(ToolResult {
        text: stringify_pretty(&json!({"svg":svg_file,"dxf":dxf_file,
        "sheet":sheet_name,"scale":scale_label,"views":["front","top","right","iso"],"holes":holes.len()}))?,
        files,
    })
}

fn n(value: f64) -> String {
    fixed_string(value, 3)
}

fn text(svg: &mut String, x: f64, y: f64, value: &str) {
    let escaped = value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    writeln!(svg, "<text x=\"{}\" y=\"{}\">{escaped}</text>", n(x), n(y)).unwrap();
}

fn dimension(svg: &mut String, center: DVec2, extent: DVec2, value: f64, horizontal: bool) {
    let start = center - extent / 2.0;
    let end = center + extent / 2.0;
    let (a, b, label) = if horizontal {
        (
            DVec2::new(start.x, end.y + 5.0),
            DVec2::new(end.x, end.y + 5.0),
            DVec2::new(center.x, end.y + 10.0),
        )
    } else {
        (
            DVec2::new(start.x - 5.0, start.y),
            DVec2::new(start.x - 5.0, end.y),
            DVec2::new(start.x - 15.0, center.y),
        )
    };
    writeln!(
        svg,
        "<path class=\"dimension\" d=\"M {} {} L {} {}\"/>",
        n(a.x),
        n(a.y),
        n(b.x),
        n(b.y)
    )
    .unwrap();
    text(svg, label.x, label.y, &n(value));
}

fn joined_polylines(points: &[DVec2]) -> Vec<Vec<DVec2>> {
    // shortcut: endpoint joining scans fragments quadratically, index endpoints for large drawings.
    let near = |a: DVec2, b: DVec2| a.distance_squared(b) < 1e-12;
    let mut pending: Vec<Vec<DVec2>> = Vec::new();
    for line in points
        .split(|p| !p.is_finite())
        .filter(|line| line.len() > 1)
    {
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
            if !near(*line.last().unwrap(), next[0]) {
                next.reverse();
            }
            line.extend(next.into_iter().skip(1));
        }
        joined.push(line);
    }
    joined
}

fn svg_paths(svg: &mut String, class: &str, lines: &[Vec<DVec2>], offset: DVec2, scale: f64) {
    for line in lines {
        write!(svg, "<path class=\"{class}\" d=\"").unwrap();
        for (i, p) in line.iter().enumerate() {
            write!(
                svg,
                "{} {} {} ",
                if i == 0 { "M" } else { "L" },
                n(p.x * scale + offset.x),
                n(-p.y * scale + offset.y)
            )
            .unwrap();
        }
        svg.push_str("\"/>\n");
    }
}

fn dxf(lines: &[Vec<DVec2>], min: DVec2) -> String {
    // shortcut: arcs are written as short lines, emit ARC entities when cadrum exposes curve types.
    let mut dxf = String::from(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n",
    );
    for line in lines {
        for pair in line.windows(2) {
            let [a, b] = [pair[0] - min, pair[1] - min];
            writeln!(
                dxf,
                "0\nLINE\n8\nOUTLINE\n10\n{}\n20\n{}\n30\n0\n11\n{}\n21\n{}\n31\n0",
                n(a.x),
                n(a.y),
                n(b.x),
                n(b.y)
            )
            .unwrap();
        }
    }
    dxf.push_str("0\nENDSEC\n0\nEOF\n");
    dxf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_fragments_join_in_both_directions_and_deduplicate() {
        let points = [
            DVec2::X,
            DVec2::new(2.0, 0.0),
            DVec2::NAN,
            DVec2::ZERO,
            DVec2::X,
            DVec2::NAN,
            DVec2::X,
            DVec2::ZERO,
            DVec2::NAN,
            DVec2::new(3.0, 0.0),
            DVec2::new(2.0, 0.0),
        ];
        let lines = joined_polylines(&points);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].len(), 4);
        assert_eq!(
            lines[0]
                .windows(2)
                .map(|p| p[0].distance(p[1]))
                .sum::<f64>(),
            3.0
        );
        let mut svg = String::new();
        svg_paths(&mut svg, "hidden", &lines, DVec2::ZERO, 1.0);
        assert_eq!(svg.matches("<path").count(), 1);
    }

    #[test]
    fn auto_sheet_and_explicit_sheet_choose_largest_fitting_scale() {
        let (_, _, _, scale) = sheet_scale(DVec3::new(40.0, 30.0, 20.0), "A3").unwrap();
        assert_eq!(scale, "2:1");
        let (sheet, _, _, scale) = sheet_scale(DVec3::new(100.0, 80.0, 60.0), "auto").unwrap();
        assert_eq!((sheet, scale), ("A3", "1:1"));
        let (sheet, _, _, scale) = sheet_scale(DVec3::new(200.0, 150.0, 100.0), "auto").unwrap();
        assert_eq!((sheet, scale), ("A2", "1:1"));
    }
}
