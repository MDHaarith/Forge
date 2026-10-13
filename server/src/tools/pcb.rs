use super::{exec::{report, run}, input_str, paths::project_path, Tool, ToolContext, ToolError, ToolFuture, ToolResult};
use serde_json::{json, Value};

mod footprint;
mod place;
mod spec;

pub use footprint::{footprint, Footprint, Pad};
pub use place::{place, PlacedPart};
pub use spec::{parse, prepare, Board, Hole, Keepout, Kind, Layer, Net, NetClass, Part, Pin, Pour, PreparedBoard, Rules, Spec, Trace};

pub fn tools() -> Vec<Tool> {
    vec![
        {
            let mut tool = Tool::new("pcb", "pcb_drc", r#"Run KiCad's design rule check on a board and write the report to fab/drc.rpt."#, json!({"type":"object", "additionalProperties": false,"properties":{"board":{"type":"string","description":"Project-relative .kicad_pcb file"}},"required":["board"]}), pcb_drc);
            tool.requires = vec!["kicad-cli".into()];
            tool.install = Some(serde_json::from_value(json!({"dnf":"kicad","apt":"kicad","brew":"--cask kicad","url":"https://www.kicad.org/download/"})).unwrap());
            tool
        },
        {
            let mut tool = Tool::new("pcb", "pcb_export_fab", r#"Export manufacturing files for a board: Gerbers and drill files into fab/, ready to zip and upload to a PCB fab such as JLCPCB or PCBWay."#, json!({"type":"object", "additionalProperties": false,"properties":{"board":{"type":"string","description":"Project-relative .kicad_pcb file"}},"required":["board"]}), pcb_export_fab);
            tool.requires = vec!["kicad-cli".into()];
            tool.install = Some(serde_json::from_value(json!({"dnf":"kicad","apt":"kicad","brew":"--cask kicad","url":"https://www.kicad.org/download/"})).unwrap());
            tool
        },
    ]
}

fn pcb_drc(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let board = input_str(&input, "board")?;
        project_path(&ctx.project_dir, board)?;
        project_path(&ctx.project_dir, "fab/drc.rpt")?;
        let r = run("kicad-cli", &["pcb".into(), "drc".into(), "--output".into(), "fab/drc.rpt".into(), board.into()], &ctx.project_dir, 120_000, &[]).await;
        Ok(ToolResult { text: report(&r), files: vec!["fab/drc.rpt".into()] })
    })
}

fn pcb_export_fab(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let board = input_str(&input, "board")?;
        project_path(&ctx.project_dir, board)?;
        project_path(&ctx.project_dir, "fab/")?;
        let gerbers = run("kicad-cli", &["pcb".into(), "export".into(), "gerbers".into(), "--output".into(), "fab/".into(), board.into()], &ctx.project_dir, 120_000, &[]).await;
        if gerbers.code != 0 { return Err(ToolError::expected(report(&gerbers))); }
        let drill = run("kicad-cli", &["pcb".into(), "export".into(), "drill".into(), "--output".into(), "fab/".into(), board.into()], &ctx.project_dir, 120_000, &[]).await;
        Ok(ToolResult { text: format!("gerbers:\n{}\n\ndrill:\n{}", report(&gerbers), report(&drill)), files: vec!["fab/".into()] })
    })
}
