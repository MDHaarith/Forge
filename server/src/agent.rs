use crate::jsfmt::trim;
use crate::{
    model::{Model, ModelRequest},
    tools::{validate::validate, Tool, ToolContext, ToolError, ToolResult},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

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
    format!(
        r#"You are Forge, an engineering assistant that runs a personal workshop: software, 3D/mechanical parts, printed circuit boards and chips. You act through tools on the user's project "{project_name}", a git repository on their machine.

Available domains right now: {domains}. Domains with no tools here need software the user has not installed; when a request needs one, say which program to install instead of pretending.

How to work:
- Keep every design as text in the project (code, OpenSCAD, Verilog, KiCad files, notes) so it can be diffed, reviewed and rebuilt.
- Use the check tools (simulation, DRC, synthesis, builds) on your own output before calling something finished, and report what passed or failed.
- Commit with git_commit after each meaningful step, with a one-line message.
- Use millimetres for mechanical work. State key dimensions and assumptions in your reply.
- Pick the 3D tool by purpose: freecad_build for engineering parts that need exact dimensions, holes, fits or a STEP file for manufacturing; blender_build for visual and organic models, scenes, materials and renders; cad_model only for quick block mock-ups when neither is available. Keep the .py script in the project (cad/ for FreeCAD, art/ for Blender) and re-run it after edits.
- For FreeCAD parts: use the forge helpers for standard holes, standoffs and enclosures instead of guessing sizes; check the result with freecad_inspect (holes, mass, interference between mating parts, print overhangs) and fix what does not meet the request; make a freecad_drawing when the part will be manufactured or the user wants a drawing.
- For circuit boards: write the board as tscircuit code in pcb/<name>.tsx and build it with pcb_build (Forge's own PCB engine; no KiCad needed). Fix every error it reports and rebuild until it says every connection is routed. Use KiCad tools only when the user works in KiCad.
- For radios and antennas: get trace widths for 50 ohm feeds and impedance-controlled pairs from rf_calc and use them in the board code; pour ground under RF traces but keep it out from under the antenna; put a pi matching footprint between the radio and the antenna. When rf_antenna_sim is available, simulate the antenna after pcb_build, fix its length if the resonance is off, and size the matching network from the simulated impedance with rf_calc match. Check range with rf_calc link. Prefer a pre-certified radio module unless the user wants a custom RF design.
- For electronics in an enclosure, design both together like in Fusion 360: build the board, then size the enclosure in FreeCAD from the board's size, hole positions and tallest part, then call design_assemble with the board's .glb and the enclosure parts. Fix every collision (move parts, the board or the enclosure feature) and re-run it; the Design view shows the PCB beside the 3D assembly.
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
        if let Err(message) = self.run_loop(emit).await {
            emit(AgentEvent::Error { message });
        }
        drop(guard);
        emit(AgentEvent::Done);
    }
    async fn run_loop(&self, emit: &Emit<'_>) -> Result<(), String> {
        let source = self.tools.clone();
        let tools = tokio::task::spawn_blocking(move || source())
            .await
            .map_err(|e| e.to_string())?;
        let by_name: HashMap<_, _> = tools.iter().map(|t| (t.name, t)).collect();
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
                            by_name.get(name).copied(),
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
    tool: Option<&Tool>,
    name: &str,
    input: &Value,
    context: ToolContext,
) -> (bool, ToolResult) {
    let Some(tool) = tool else {
        return (
            false,
            ToolResult::text(format!("Unknown or unavailable tool \"{name}\".")),
        );
    };
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

pub(crate) struct BusyGuard<'a>(&'a AtomicBool);
impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
