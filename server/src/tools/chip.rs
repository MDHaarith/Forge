use super::{exec::{report, run}, paths::project_path, input_str, Tool, ToolContext, ToolError, ToolFuture, ToolResult};
use serde_json::{json, Value};

pub fn tools() -> Vec<Tool> {
    vec![
        {
            let mut tool = Tool::new("chip", "chip_simulate", r#"Compile Verilog sources with Icarus Verilog and run the testbench. Put $display / $finish in the testbench; its output is returned."#, json!({"type":"object","properties":{"sources":{"type":"array","items":{"type":"string"},"minItems":1,"description":"Project-relative .v files, testbench included"}},"required":["sources"]}), chip_simulate);
            tool.requires = vec!["iverilog".into()];
            tool.install = Some(serde_json::from_value(json!({"dnf":"iverilog","apt":"iverilog","brew":"icarus-verilog"})).unwrap());
            tool
        },
        {
            let mut tool = Tool::new("chip", "chip_synthesize", r#"Synthesize Verilog to a generic gate netlist with Yosys and report cell counts. Writes build/<top>.json. The first step towards place-and-route with OpenROAD/OpenLane."#, json!({"type":"object","properties":{"sources":{"type":"array","items":{"type":"string"},"minItems":1,"description":"Project-relative .v files (no testbench)"},"top":{"type":"string","description":"Top module name"}},"required":["sources","top"]}), chip_synthesize);
            tool.requires = vec!["yosys".into()];
            tool.install = Some(serde_json::from_value(json!({"dnf":"yosys","apt":"yosys","brew":"yosys"})).unwrap());
            tool
        },
    ]
}

fn sources(input: &Value, ctx: &ToolContext) -> Result<Vec<String>, ToolError> {
    let sources = input["sources"].as_array().ok_or_else(|| ToolError::unexpected("input.sources must be an array"))?;
    sources.iter().map(|s| {
        let s = s.as_str().ok_or_else(|| ToolError::unexpected("source paths must be strings"))?;
        project_path(&ctx.project_dir, s)?;
        Ok(s.to_owned())
    }).collect()
}
fn valid_module(top: &str) -> bool {
    let mut bytes = top.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$'))
}
fn synthesis_script(top: &str, sources: &[String]) -> Result<String, ToolError> {
    let reads = sources.iter().map(|s| Ok(format!("read_verilog -sv {}",serde_json::to_string(s)?))).collect::<Result<Vec<_>, ToolError>>()?;
    Ok(format!("{}; synth -top {top}; stat; write_json build/{top}.json", reads.join("; ")))
}
fn chip_simulate(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let sources = sources(&input, &ctx)?;
        tokio::fs::create_dir_all(project_path(&ctx.project_dir, "build")?).await?;
        let mut args = vec!["-g2012".into(), "-o".into(), "build/sim.vvp".into()];
        args.extend(sources);
        let compiled = run("iverilog", &args, &ctx.project_dir, 120_000, &[]).await;
        if compiled.code != 0 {return Err(ToolError::expected(report(&compiled)));}
        Ok(ToolResult::text(report(&run("vvp", &["build/sim.vvp".into()], &ctx.project_dir, 60_000, &[]).await)))
    })
}
fn chip_synthesize(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let top = input_str(&input, "top")?;
        if !valid_module(top) {return Err(ToolError::expected("top must be a Verilog identifier"));}
        let sources = sources(&input, &ctx)?;
        let script = synthesis_script(top, &sources)?;
        tokio::fs::create_dir_all(project_path(&ctx.project_dir, "build")?).await?;
        let r = run("yosys", &["-p".into(),script], &ctx.project_dir, 120_000, &[]).await;
        if r.code != 0 {return Err(ToolError::expected(report(&r)));}
        Ok(ToolResult {text:report(&r), files:vec![format!("build/{top}.json")]})
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identifiers_and_quoted_sources() {
        for top in ["a", "_a", "a$2"] {assert!(valid_module(top));}
        for top in ["", "2a", "a;stat", "a-b", "é"] {assert!(!valid_module(top));}
        assert_eq!(synthesis_script("top", &["a b.v".into(), "q\".v".into()]).unwrap(), "read_verilog -sv \"a b.v\"; read_verilog -sv \"q\\\".v\"; synth -top top; stat; write_json build/top.json");
    }
}
