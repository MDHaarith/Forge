//! Native, deterministic SPICE subset. No external simulator or model files are loaded.
mod devices;
mod output;
mod parse;
mod solve;

use super::{input_str, paths::project_path, script3d::valid_name, Tool, ToolContext, ToolError, ToolFuture, ToolResult};
use serde_json::{json, Value};
use std::path::Path;
use tokio::io::AsyncReadExt;

pub fn tools() -> Vec<Tool> {
    vec![Tool::new("pcb", "circuit_simulate", "Simulate sim/<name>.cir natively; first line is the title, node 0/gnd is ground, SPICE suffixes work.\nR/C/L (ic=), V/I (DC, PULSE, SIN, PWL), D, NPN/PNP Q, level-1 NMOS/PMOS M (W= L=), E/G sources.\nUse .model and nested .subckt/X; comments use * or ; and continuations use +. No external includes.\nAnalyses: .op, .dc source start stop step, .tran tstep tstop [tstart [tmax]] [uic]; .ic v(node)=value.\n.options reltol abstol vntol gmin itl1 itl4; .print/.plot tran|dc v(node) selects up to 8 plotted traces.\nWrites sim/<name>/<name>-op.txt, -tran.csv/-dc.csv and standalone SVG plots; limit 2000 unknowns.",
        json!({"type":"object","additionalProperties":false,"properties":{"source":{"type":"string","description":"Project-relative SPICE netlist, such as sim/filter.cir; write it with write_file first"}},"required":["source"]}), circuit_simulate)]
}

pub struct Simulation {
    pub text: String,
    pub files: Vec<(String, String)>,
}

/// Pure entry point, also used by reference and performance tests.
pub fn simulate(netlist: &str, name: &str) -> Result<Simulation, String> {
    if !valid_name(name) { return Err("name may only use letters, digits, - and _".into()); }
    let circuit = parse::parse(netlist)?;
    output::build(&circuit, name)
}

fn circuit_simulate(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let source = input_str(&input, "source")?.to_owned();
        let full = project_path(&ctx.project_dir, &source)?;
        if !source.ends_with(".cir") { return Err(ToolError::expected("source must be a .cir file")); }
        let metadata = tokio::fs::metadata(&full).await;
        if !metadata.as_ref().is_ok_and(|m| m.is_file()) {
            return Err(ToolError::expected(format!("{source} does not exist; write it with write_file first")));
        }
        // shortcut: 1 MiB netlists, raise with streamed parsing when larger decks are needed.
        if metadata.unwrap().len() > 1_048_576 { return Err(ToolError::expected("netlist too large (limit 1 MiB)")); }
        let name = Path::new(&source).file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let mut netlist = String::new();
        tokio::fs::File::open(full).await?.take(1_048_577).read_to_string(&mut netlist).await?;
        let simulation = tokio::task::spawn_blocking(move || simulate(&netlist, &name))
            .await.map_err(|e| ToolError::unexpected(e.to_string()))?.map_err(ToolError::expected)?;
        let mut files = Vec::new();
        for (file, content) in simulation.files {
            let path = project_path(&ctx.project_dir, &file)?;
            tokio::fs::create_dir_all(path.parent().unwrap()).await?;
            tokio::fs::write(path, content).await?;
            files.push(file);
        }
        Ok(ToolResult { text: simulation.text, files })
    })
}
