mod common;
use common::TempDir;
use forge_server::{
    agent::{Agent, AgentEvent},
    model::{Model, ModelFuture, ModelReply, ModelRequest},
    tools::{all_tools, Tool, ToolContext, ToolError, ToolFuture},
};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

/// Replies from a fixed script and records every request it receives.
struct ScriptedModel {
    requests: Mutex<Vec<ModelRequest>>,
    replies: Mutex<VecDeque<ModelReply>>,
}
impl Model for ScriptedModel {
    fn name(&self) -> String {
        "scripted".into()
    }
    fn respond<'a>(&'a self, request: &'a ModelRequest) -> ModelFuture<'a> {
        self.requests.lock().unwrap().push(request.clone());
        let reply = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| "script ran out".into());
        Box::pin(async move { reply })
    }
}
fn text(t: &str) -> Value {
    json!({"type":"text","text":t,"citations":null})
}
fn call(id: &str, name: &str, input: Value) -> Value {
    json!({"type":"tool_use","id":id,"name":name,"input":input})
}
fn reply(reason: &str, content: Vec<Value>) -> ModelReply {
    ModelReply {
        stop_reason: Some(reason.into()),
        content,
    }
}
fn setup(replies: Vec<ModelReply>) -> (TempDir, Arc<ScriptedModel>, Agent, Mutex<Vec<AgentEvent>>) {
    let dir = TempDir::new("agent");
    let model = Arc::new(ScriptedModel {
        requests: Mutex::new(vec![]),
        replies: Mutex::new(replies.into()),
    });
    let tools = Arc::new(|| {
        all_tools()
            .into_iter()
            .filter(|t| t.domain == "workspace" || t.domain == "3d")
            .collect()
    });
    let agent = Agent::new(
        model.clone(),
        "test",
        ToolContext::new(dir.path.clone(), env!("CARGO_MANIFEST_DIR").into()),
        tools,
    );
    (dir, model, agent, Mutex::new(vec![]))
}
fn kinds(events: &[AgentEvent]) -> Vec<String> {
    events
        .iter()
        .map(|e| {
            serde_json::to_value(e).unwrap()["type"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect()
}

#[tokio::test]
async fn runs_tools_feeds_results_and_keeps_history_append_only() {
    let (dir, model, agent, events) = setup(vec![
        reply(
            "tool_use",
            vec![
                text("Writing it."),
                call(
                    "t1",
                    "write_file",
                    json!({"path":"src/hello.txt","contents":"hi"}),
                ),
            ],
        ),
        reply("end_turn", vec![text("Done.")]),
    ]);
    agent
        .send("make a file", &|e| events.lock().unwrap().push(e))
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path.join("src/hello.txt")).unwrap(),
        "hi"
    );
    assert_eq!(
        kinds(&events.lock().unwrap()),
        ["user", "text", "tool_call", "tool_result", "text", "done"]
    );
    assert!(events
        .lock()
        .unwrap()
        .iter()
        .any(|e| matches!(e,AgentEvent::ToolResult{ok:true,files,..}if files[0]=="src/hello.txt")));
    // The second request starts with the first request's messages unchanged.
    let requests = model.requests.lock().unwrap();
    let (first, second) = (&requests[0], &requests[1]);
    assert_eq!(second.messages[..first.messages.len()], first.messages);
    assert_eq!(
        agent
            .messages
            .lock()
            .unwrap()
            .iter()
            .map(|m| m["role"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["user", "assistant", "user", "assistant"]
    );
    assert!(first.tools.iter().any(|t| t["name"] == "cad_model"));
    assert!(!first.tools.iter().any(|t| t["name"] == "git_commit"));
    assert!(!agent.running());
}

#[tokio::test]
async fn invalid_unknown_and_tool_errors_return_together() {
    let (_dir, model, agent, events) = setup(vec![
        reply(
            "tool_use",
            vec![
                call("a", "write_file", json!({"path":"x.txt"})),
                call("b", "launch_rocket", json!({})),
                call("c", "read_file", json!({"path":"../../etc/passwd"})),
            ],
        ),
        reply("end_turn", vec![text("ok")]),
    ]);
    agent
        .send("go", &|e| events.lock().unwrap().push(e))
        .await
        .unwrap();
    let events = events.lock().unwrap();
    let results = events
        .iter()
        .filter_map(|e| {
            if let AgentEvent::ToolResult { ok, text, .. } = e {
                Some((*ok, text))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|(ok, _)| !*ok));
    assert_eq!(
        serde_json::from_str::<Value>(results[0].1).unwrap(),
        json!({"INVALID_INPUT":["input.contents is required"],"received":{"path":"x.txt"}})
    );
    assert_eq!(
        results[1].1,
        "Unknown or unavailable tool \"launch_rocket\"."
    );
    assert_eq!(
        results[2].1,
        "path \"../../etc/passwd\" is outside the project"
    );
    // All three results return together in one user message.
    let requests = model.requests.lock().unwrap();
    let last = requests[1].messages.last().unwrap();
    let blocks = last["content"].as_array().unwrap();
    assert_eq!(blocks.len(), 3);
    assert!(blocks
        .iter()
        .all(|b| b["type"] == "tool_result" && b["is_error"] == true));
}

#[tokio::test]
async fn cut_off_reply_does_not_run_its_call() {
    let (dir, _model, agent, events) = setup(vec![reply(
        "max_tokens",
        vec![call(
            "t",
            "write_file",
            json!({"path":"half.txt","contents":"trunc"}),
        )],
    )]);
    agent
        .send("go", &|e| events.lock().unwrap().push(e))
        .await
        .unwrap();
    assert!(!dir.path.join("half.txt").exists());
    assert!(events
        .lock()
        .unwrap()
        .iter()
        .any(|e| matches!(e, AgentEvent::Error { .. })));
    // The dangling call is still answered, so the next request is valid.
    assert_eq!(
        agent.messages.lock().unwrap().last().unwrap()["role"],
        "user"
    );
    assert!(events.lock().unwrap().iter().any(|e|matches!(e,AgentEvent::ToolResult{ok:false,text,..}if text=="Not run: the reply hit its length limit before this call was complete.")));
}

#[tokio::test]
async fn cad_model_writes_stl_scad_and_integer_json() {
    let (dir, _model, agent, events) = setup(vec![
        reply(
            "tool_use",
            vec![call(
                "t",
                "cad_model",
                json!({"name":"bracket","parts":[{"shape":"box","size":[10.0,10.0,2.0]},{"shape":"cylinder","radius":2.0,"height":5.0,"at":[5.0,5.0,2.0]}]}),
            )],
        ),
        reply("end_turn", vec![]),
    ]);
    agent
        .send("bracket", &|e| events.lock().unwrap().push(e))
        .await
        .unwrap();
    let mut files = std::fs::read_dir(dir.path.join("models"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    files.sort();
    assert_eq!(files, ["bracket.json", "bracket.scad", "bracket.stl"]);
    let json = std::fs::read_to_string(dir.path.join("models/bracket.json")).unwrap();
    assert!(!json.contains(".0"));
    assert!(json.ends_with('\n'));
}

#[tokio::test]
async fn pause_refusal_length_without_calls_and_model_errors() {
    for reason in ["refusal", "max_tokens"] {
        let (_dir, _model, agent, events) =
            setup(vec![reply("pause_turn", vec![]), reply(reason, vec![])]);
        agent
            .send("go", &|e| events.lock().unwrap().push(e))
            .await
            .unwrap();
        assert_eq!(kinds(&events.lock().unwrap()), ["user", "error", "done"]);
    }
    let (dir, model, agent, events) = setup(vec![reply(
        "refusal",
        vec![call("t", "write_file", json!({"path":"x","contents":"hi"}))],
    )]);
    agent
        .send("go", &|e| events.lock().unwrap().push(e))
        .await
        .unwrap();
    assert!(!dir.path.join("x").exists());
    assert!(events.lock().unwrap().iter().any(
        |e| matches!(e,AgentEvent::Error{message}if message=="Stopped: the request was declined.")
    ));
    agent
        .send("again", &|e| events.lock().unwrap().push(e))
        .await
        .unwrap();
    assert!(events
        .lock()
        .unwrap()
        .iter()
        .any(|e| matches!(e,AgentEvent::Error{message}if message=="script ran out")));
    assert_eq!(model.requests.lock().unwrap().len(), 2);
    assert!(!agent.running());
}

#[tokio::test]
async fn step_limit_and_unexpected_tool_errors() {
    let (_dir, model, agent, events) = setup(vec![reply("pause_turn", vec![]); 40]);
    agent
        .send("go", &|e| events.lock().unwrap().push(e))
        .await
        .unwrap();
    assert_eq!(model.requests.lock().unwrap().len(), 40);
    assert!(events.lock().unwrap().iter().any(|e|matches!(e,AgentEvent::Error{message}if message=="Stopped after 40 steps; send a message to continue.")));
    fn fail(_: Value, _: ToolContext) -> ToolFuture {
        Box::pin(async { Err(ToolError::unexpected("disk error")) })
    }
    let (dir, model, _agent, events) = setup(vec![
        reply("tool_use", vec![call("a", "fail", json!({}))]),
        reply("end_turn", vec![]),
    ]);
    let tool = Tool::new("workspace", "fail", "Fails", json!({"type":"object"}), fail);
    let agent = Agent::new(
        model,
        "test",
        ToolContext::new(dir.path.clone(), dir.path.clone()),
        Arc::new(move || vec![tool.clone()]),
    );
    agent
        .send("go", &|e| events.lock().unwrap().push(e))
        .await
        .unwrap();
    assert!(events.lock().unwrap().iter().any(
        |e| matches!(e,AgentEvent::ToolResult{ok:false,text,..}if text=="fail failed: disk error")
    ));
}

#[tokio::test]
async fn stop_ends_a_turn_and_answers_its_open_tool_calls() {
    fn hang(_: Value, _: ToolContext) -> ToolFuture {
        Box::pin(std::future::pending())
    }
    let (dir, model, _agent, events) = setup(vec![reply("tool_use", vec![call("a", "hang", json!({}))])]);
    let tool = Tool::new("workspace", "hang", "Never ends", json!({"type":"object"}), hang);
    let agent = Agent::new(
        model,
        "test",
        ToolContext::new(dir.path.clone(), dir.path.clone()),
        Arc::new(move || vec![tool.clone()]),
    );
    let emit = |e| events.lock().unwrap().push(e);
    let turn = agent.send("go", &emit);
    let stop = async {
        while !kinds(&events.lock().unwrap()).contains(&"tool_call".to_owned()) {
            tokio::task::yield_now().await;
        }
        assert!(agent.stop());
    };
    let (result, ()) = tokio::join!(turn, stop);
    result.unwrap();
    assert_eq!(kinds(&events.lock().unwrap()), ["user", "tool_call", "error", "done"]);
    let messages = agent.messages.lock().unwrap();
    assert_eq!(messages.last().unwrap()["content"][0]["tool_use_id"], "a");
    assert!(!agent.running());
}
