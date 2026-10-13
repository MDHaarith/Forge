use crate::jsfmt::trim;
use crate::{
    model::{Model, ModelRequest},
    tools::{validate::validate, Tool, ToolContext, ToolError, ToolResult},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tokio::sync::Notify;

pub(crate) const STOPPED: &str = "Stopped by the user.";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    User {
        text: String,
    },
    Text {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        id: String,
        name: String,
        ok: bool,
        text: String,
        files: Vec<String>,
    },
    Error {
        message: String,
    },
    Done,
}

const MAX_STEPS: usize = 40;
pub type ToolSource = Arc<dyn Fn() -> Vec<Tool> + Send + Sync>;
pub type Emit<'a> = dyn Fn(AgentEvent) + Send + Sync + 'a;

pub fn system_prompt(project_name: &str, tools: &[Tool]) -> String {
    let mut domains = Vec::new();
    for tool in tools {
        if !domains.contains(&tool.domain) {
            domains.push(tool.domain);
        }
    }
    let domains = domains.join(", ");
    let guidance = [
        (&["git_commit"][..], "Commit with git_commit after each meaningful step, with a one-line message."),
        (&["part_build"][..], "Use part_build for every exact engineering part (Forge's own CAD kernel): loft, sweep, shell, mirror, patterns, threads, sheet metal, standard holes, enclosures and STEP files for manufacturing. Use the standard hole, standoff, enclosure and lid ops instead of guessing sizes."),
        (&["part_inspect"][..], "Check every exact part with part_inspect (size, mass, holes and the screws they fit, print overhangs). Use against to check interference or clearance between mating parts and fix what does not meet the request."),
        (&["part_drawing"][..], "Use part_drawing when the part will be manufactured or the user wants a drawing."),
        (&["blender_build"][..], "Use blender_build for visual and organic models, scenes, materials and renders. Keep Blender .py scripts in art/ and re-run them after edits."),
        (&["pcb_build"][..], "For circuit boards: write the board as JSON in pcb/<name>.pcb.json and build it with pcb_build (Forge's own PCB engine; no KiCad needed). Fix every error it reports and rebuild until it says every connection is routed. pcb_build writes all fabrication files."),
        (&["pcb_drc"][..], "Run pcb_drc with the built board name and fix every DRC fault before ordering."),
        (&["rf_calc"][..], "For radios and antennas: get trace widths for 50 ohm feeds and impedance-controlled pairs from rf_calc and use them in the board JSON; pour ground under RF traces but keep it out from under the antenna; put a pi matching footprint between the radio and the antenna. Check range with rf_calc link. Use rf_calc track_width and via for current capacity, spacing for voltage clearance, fusing for copper fuse estimates, and e_series for standard values or resistor dividers. Prefer a pre-certified radio module unless the user wants a custom RF design."),
        (&["rf_antenna_sim", "pcb_build", "rf_calc"][..], "Simulate the antenna with rf_antenna_sim after pcb_build, fix its length if the resonance is off, and size the matching network from the simulated impedance with rf_calc match."),
        (&["design_assemble", "part_build"][..], "For electronics in an enclosure, design both together like in Fusion 360: build the board, then size the enclosure with part_build from the board's size, hole positions and tallest part, then call design_assemble with the board's .glb and the enclosure parts. Fix every collision (move parts, the board or the enclosure feature) and re-run it; the Design view shows the PCB beside the 3D assembly."),
    ].into_iter()
        .filter(|(names, _)| names.iter().all(|name| tools.iter().any(|tool| tool.name == *name)))
        .map(|(_, sentence)| format!("- {sentence}"))
        .collect::<Vec<_>>().join("\n");

    format!(
        r#"You are Forge, an engineering assistant that runs a personal workshop: software, 3D/mechanical parts, printed circuit boards and chips. You act through tools on the user's project "{project_name}", a git repository on their machine.

Available domains right now: {domains}. Domains with no tools here need software the user has not installed; when a request needs one, say which program to install instead of pretending.

How to work:
- Keep every design as text in the project (code, OpenSCAD, Verilog, KiCad files, notes) so it can be diffed, reviewed and rebuilt.
- Use the check tools (simulation, DRC, synthesis, builds) on your own output before calling something finished, and report what passed or failed.
- Use millimetres for mechanical work. State key dimensions and assumptions in your reply.
{guidance}
- Manufacturing means producing files ready to order (STL for printing, Gerbers for a PCB fab, GDS for a shuttle). You cannot place orders; tell the user what to upload where.
- Be brief in chat: what you made, where it is, what to check."#
    )
}

/// One project's conversation with the model. The message history is append-only:
/// assistant turns are stored exactly as returned, which keeps thinking blocks valid
/// and the prompt cache warm.
pub struct Agent {
    pub messages: Mutex<Vec<Value>>,
    busy: AtomicBool,
    /// Wakes the running turn so it can end early (the Stop button).
    pub(crate) stop: Notify,
    model: Arc<dyn Model>,
    project_name: String,
    context: ToolContext,
    tools: ToolSource,
}

impl Agent {
    pub fn new(
        model: Arc<dyn Model>,
        project_name: impl Into<String>,
        context: ToolContext,
        tools: ToolSource,
    ) -> Self {
        Self {
            messages: Mutex::new(vec![]),
            busy: AtomicBool::new(false),
            stop: Notify::new(),
            model,
            project_name: project_name.into(),
            context,
            tools,
        }
    }
    pub fn running(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }
    pub(crate) fn turn_guard(&self) -> BusyGuard<'_> {
        BusyGuard(&self.busy)
    }
    /// Ends the running turn; returns false when nothing is running.
    pub fn stop(&self) -> bool {
        self.stop.notify_waiters();
        self.running()
    }
    pub fn try_start(&self) -> Result<(), String> {
        self.busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| "Forge is still working on the previous request".into())
    }
    pub async fn send(&self, user_text: &str, emit: &Emit<'_>) -> Result<(), String> {
        self.try_start()?;
        self.send_started(user_text, emit).await;
        Ok(())
    }
    /// Completes a turn reserved by try_start(), even if its HTTP client has disconnected.
    pub async fn send_started(&self, user_text: &str, emit: &Emit<'_>) {
        let guard = BusyGuard(&self.busy);
        emit(AgentEvent::User {
            text: user_text.into(),
        });
        self.messages
            .lock()
            .unwrap()
            .push(json!({"role":"user","content":user_text}));
        tokio::select! {
            result = self.run_loop(emit) => {
                if let Err(message) = result {
                    emit(AgentEvent::Error { message });
                }
            }
            _ = self.stop.notified() => {
                self.answer_open_calls();
                emit(AgentEvent::Error { message: STOPPED.into() });
            }
        }
        drop(guard);
        emit(AgentEvent::Done);
    }
    /// A stopped turn can end after a tool_use; the API needs a result for every call.
    fn answer_open_calls(&self) {
        let mut messages = self.messages.lock().unwrap();
        let Some(last) = messages.last().filter(|m| m["role"] == "assistant") else { return };
        let results = last["content"].as_array().into_iter().flatten()
            .filter(|b| b["type"] == "tool_use")
            .map(|b| json!({"type":"tool_result","tool_use_id":b["id"],"content":"Not run: stopped by the user.","is_error":true}))
            .collect::<Vec<_>>();
        if !results.is_empty() {
            messages.push(json!({"role":"user","content":results}));
        }
    }
    async fn run_loop(&self, emit: &Emit<'_>) -> Result<(), String> {
        let source = self.tools.clone();
        let tools = tokio::task::spawn_blocking(move || source())
            .await
            .map_err(|e| e.to_string())?;
        let system = system_prompt(&self.project_name, &tools);
        let definitions=tools.iter().map(|t|json!({"name":t.name,"description":t.description,"input_schema":t.input_schema})).collect::<Vec<_>>();
        for _ in 0..MAX_STEPS {
            let request = ModelRequest {
                system: system.clone(),
                tools: definitions.clone(),
                messages: self.messages.lock().unwrap().clone(),
            };
            let reply = self.model.respond(&request).await?;
            self.messages
                .lock()
                .unwrap()
                .push(json!({"role":"assistant","content":reply.content}));
            for block in &reply.content {
                if block["type"] == "text" {
                    if let Some(text) = block["text"].as_str().filter(|s| !trim(s).is_empty()) {
                        emit(AgentEvent::Text { text: text.into() });
                    }
                }
            }
            let calls = reply
                .content
                .iter()
                .filter(|b| b["type"] == "tool_use")
                .collect::<Vec<_>>();
            // A cut-off or declined turn can end inside a tool input: answer those calls without running them.
            let halted = match reply.stop_reason.as_deref() {
                Some("max_tokens") => {
                    Some("the reply hit its length limit before this call was complete")
                }
                Some("refusal") => Some("the request was declined"),
                _ => None,
            };
            if calls.is_empty() {
                if reply.stop_reason.as_deref() == Some("pause_turn") {
                    continue;
                }
                if let Some(reason) = halted {
                    emit(AgentEvent::Error {
                        message: format!("Stopped: {reason}."),
                    });
                }
                return Ok(());
            }
            let mut results = Vec::new();
            for call in calls {
                let id = call["id"].as_str().unwrap_or("");
                let name = call["name"].as_str().unwrap_or("");
                let input = &call["input"];
                emit(AgentEvent::ToolCall {
                    id: id.into(),
                    name: name.into(),
                    input: input.clone(),
                });
                let (ok, outcome) = match halted {
                    Some(reason) => (false, ToolResult::text(format!("Not run: {reason}."))),
                    None => {
                        execute_tool(
                            &tools,
                            name,
                            input,
                            self.context.clone(),
                        )
                        .await
                    }
                };
                emit(AgentEvent::ToolResult {
                    id: id.into(),
                    name: name.into(),
                    ok,
                    text: outcome.text.clone(),
                    files: outcome.files,
                });
                let mut result =
                    json!({"type":"tool_result","tool_use_id":id,"content":outcome.text});
                if !ok {
                    result["is_error"] = json!(true);
                }
                results.push(result);
            }
            // All results go back in one message so parallel calls stay parallel.
            self.messages
                .lock()
                .unwrap()
                .push(json!({"role":"user","content":results}));
            if let Some(reason) = halted {
                emit(AgentEvent::Error {
                    message: format!("Stopped: {reason}."),
                });
                return Ok(());
            }
        }
        emit(AgentEvent::Error {
            message: format!("Stopped after {MAX_STEPS} steps; send a message to continue."),
        });
        Ok(())
    }
}

/// Both model loops share validation and error wording at the tool boundary.
pub(crate) async fn execute_tool(
    tools: &[Tool],
    name: &str,
    input: &Value,
    context: ToolContext,
) -> (bool, ToolResult) {
    let Some(tool) = find_tool(tools, name) else {
        let available = tools.iter().map(|t| t.name).collect::<Vec<_>>().join(", ");
        return (
            false,
            ToolResult::text(format!(
                "Unknown or unavailable tool \"{name}\". Available tools: {available}."
            )),
        );
    };
    if let Some(raw) = input
        .as_object()
        .filter(|o| o.len() == 1)
        .and_then(|o| o.get("_unparsed"))
        .and_then(Value::as_str)
    {
        if let Err(error) = serde_json::from_str::<Value>(raw) {
            return (
                false,
                ToolResult::text(format!(
                    "Arguments were not valid JSON: {error}. Send the input as one JSON object."
                )),
            );
        }
    }
    let problems = validate(input, &tool.input_schema);
    if !problems.is_empty() {
        return (
            false,
            ToolResult::text(json!({"INVALID_INPUT":problems,"received":input}).to_string()),
        );
    }
    match (tool.run)(input.clone(), context).await {
        Ok(result) => (true, result),
        Err(error) => {
            let message = match error {
                ToolError::Expected(message) => message,
                ToolError::Unexpected(message) => format!("{name} failed: {message}"),
            };
            (false, ToolResult::text(message))
        }
    }
}

pub(crate) fn find_tool<'a>(tools: &'a [Tool], name: &str) -> Option<&'a Tool> {
    if let Some(tool) = tools.iter().find(|t| t.name == name) {
        return Some(tool);
    }
    let lower = name.to_lowercase();
    let suffix = lower.rsplit('.').next().unwrap_or(&lower);
    let suffix = suffix.rsplit("__").next().unwrap_or(suffix);
    let normal = suffix.replace('-', "_");
    tools.iter().find(|t| t.name == normal)
}

pub(crate) struct BusyGuard<'a>(&'a AtomicBool);
impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_lookup_normalises_prefixes_case_and_hyphens_after_exact_match() {
        let mut tools = crate::tools::workspace::tools();
        for name in [
            "write_file",
            "functions.write-file",
            "mcp__forge__write_file",
            "FUNCTIONS.MCP__FORGE__WRITE-FILE",
        ] {
            assert_eq!(find_tool(&tools, name).unwrap().name, "write_file");
        }
        assert!(find_tool(&tools, "write-file.extra").is_none());
        let mut exact = tools[2].clone();
        exact.name = "functions.write-file";
        tools.push(exact);
        assert_eq!(
            find_tool(&tools, "functions.write-file").unwrap().name,
            "functions.write-file"
        );
    }

    #[tokio::test]
    async fn malformed_arguments_use_the_parser_error_only_for_the_single_key_marker() {
        let tools = crate::tools::workspace::tools();
        let context = ToolContext::new(std::env::temp_dir(), std::env::temp_dir());
        let (ok, result) = execute_tool(
            &tools,
            "write_file",
            &json!({"_unparsed":"{oops"}),
            context.clone(),
        )
        .await;
        let error = serde_json::from_str::<Value>("{oops").unwrap_err();
        assert!(!ok);
        assert_eq!(
            result.text,
            format!("Arguments were not valid JSON: {error}. Send the input as one JSON object.")
        );
        let (ok, result) = execute_tool(
            &tools,
            "write_file",
            &json!({"_unparsed":"{oops", "path":"x"}),
            context,
        )
        .await;
        assert!(!ok);
        assert_eq!(
            serde_json::from_str::<Value>(&result.text).unwrap()["INVALID_INPUT"],
            json!([
                "input.contents is required",
                "input._unparsed is not a known field"
            ])
        );
    }

    #[test]
    fn prompt_omits_every_unavailable_tool_even_in_single_tool_subsets() {
        let tools = crate::tools::all_tools();
        for available in &tools {
            let prompt = system_prompt("p", std::slice::from_ref(available));
            for missing in tools.iter().filter(|tool| tool.name != available.name) {
                assert!(
                    !prompt.contains(missing.name),
                    "{} mentions {}",
                    available.name,
                    missing.name
                );
            }
        }
    }
}
