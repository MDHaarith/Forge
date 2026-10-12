use crate::{
    agent::{system_prompt, AgentEvent, Emit},
    model::{Model, ModelFuture, ModelRequest},
    projects::{Projects, Session},
    tools::{available_tools, exec::on_path, paths::lexical_path},
};
use serde_json::{json, Value};
use std::{collections::VecDeque, path::PathBuf, process::Stdio};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::mpsc,
};

/// Claude Code owns its conversation and tool loop; this model only records the picker selection.
pub struct ClaudeCodeModel(pub String);
impl Model for ClaudeCodeModel {
    fn name(&self) -> String {
        self.0.clone()
    }
    fn provider(&self) -> Option<String> {
        Some("claude".into())
    }
    fn respond<'a>(&'a self, _: &'a ModelRequest) -> ModelFuture<'a> {
        Box::pin(async {
            Err("Claude Code turns must run through the CLI and Forge MCP endpoint".into())
        })
    }
}

/// Resolve PATH once, keeping relative executables independent of the Claude project's cwd.
pub(crate) fn resolve_program(program: &str) -> Option<PathBuf> {
    if !on_path(program) {
        return None;
    }
    if program.contains(['/', '\\']) {
        return Some(lexical_path(&PathBuf::from(program)));
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(program))
        .find(|path| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                path.metadata()
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            }
            #[cfg(not(unix))]
            {
                path.is_file()
            }
        })
        .map(|path| lexical_path(&path))
}

struct LiveGuard<'a>(&'a Session);
impl Drop for LiveGuard<'_> {
    fn drop(&mut self) {
        self.0.live.lock().unwrap_or_else(|e| e.into_inner()).take();
    }
}

fn stale(message: &str, resume: Option<&str>) -> bool {
    resume.is_some_and(|id| message.contains("No conversation found") || message.contains(id))
}

/// Drain stderr independently, retaining only its tail so a noisy CLI cannot block stdout.
async fn stderr_tail(
    stderr: tokio::process::ChildStderr,
    resume: Option<String>,
) -> Result<(String, bool), String> {
    let mut reader = BufReader::new(stderr);
    let mut tail = VecDeque::new();
    let mut missing = false;
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        if reader
            .read_until(b'\n', &mut bytes)
            .await
            .map_err(|e| e.to_string())?
            == 0
        {
            break;
        }
        let line = String::from_utf8_lossy(&bytes).into_owned();
        missing |= stale(&line, resume.as_deref());
        tail.push_back(line);
        if tail.len() > 20 {
            tail.pop_front();
        }
    }
    Ok((tail.into_iter().collect::<String>().trim().into(), missing))
}

/// Run on the detached chat task; MCP requests only enqueue events through Session.live.
pub(crate) async fn send(
    projects: &Projects,
    name: &str,
    session: &Session,
    message: &str,
    emit: &Emit<'_>,
) -> Result<(), String> {
    let (sender, mut receiver) = mpsc::unbounded_channel();
    *session.live.lock().unwrap() = Some(sender);
    let live = LiveGuard(session);
    let result = async {
        let settings = projects.settings.clone();
        let tools = tokio::task::spawn_blocking(move || available_tools(&settings)).await.map_err(|e| e.to_string())?;
        let system = system_prompt(name, &tools);
        let program = projects.settings.claude_program.as_ref().ok_or("Claude Code could not start: no Claude program configured")?;
        let base = projects.server_url.get().ok_or("Claude Code MCP server URL is not configured")?;
        let config = json!({"mcpServers":{"forge":{"type":"http","url":format!("{base}/mcp/{name}")}}});
        let mut resume = session.claude_session.lock().unwrap().clone();
        for attempt in 0..2 {
            let mut command = Command::new(program);
            command.current_dir(projects.dir(name)?)
                .args(["-p", "--output-format", "stream-json", "--verbose", "--model"])
                .arg(projects.model.name())
                .arg("--system-prompt").arg(&system)
                .args(["--tools", "", "--strict-mcp-config", "--setting-sources", "", "--mcp-config"])
                .arg(config.to_string())
                .args(["--allowedTools", "mcp__forge"])
                .env_remove("CLAUDECODE")
                .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
                .kill_on_drop(true);
            if let Some(effort) = projects.effort.lock().unwrap().clone() {
                command.arg("--effort").arg(effort);
            }
            if let Some(id) = &resume {
                command.arg("--resume").arg(id);
            }
            let mut child = command.spawn().map_err(|e| format!("Claude Code could not start: {e}"))?;
            let stderr = tokio::spawn(stderr_tail(child.stderr.take().unwrap(), resume.clone()));
            let mut stdin = child.stdin.take().unwrap();
            let input = message.as_bytes().to_vec();
            // Writing concurrently also handles a CLI that prints before reading a large prompt.
            let input = tokio::spawn(async move {
                let result = stdin.write_all(&input).await;
                drop(stdin);
                result
            });
            let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
            let mut errors = Vec::new();
            let mut missing = false;
            loop {
                tokio::select! {
                    event = receiver.recv() => {
                        if let Some(event) = event { emit(event); }
                    }
                    line = lines.next_line() => {
                        let line = match line {
                            Ok(Some(line)) => line,
                            Ok(None) => break,
                            Err(error) => {
                                errors.push(error.to_string());
                                let _ = child.kill().await;
                                break;
                            }
                        };
                        let Ok(event) = serde_json::from_str::<Value>(&line) else { continue; };
                        match event["type"].as_str() {
                            Some("system") if event["subtype"] == "init" => {
                                if let Some(id) = event["session_id"].as_str() {
                                    *session.claude_session.lock().unwrap() = Some(id.into());
                                    projects.save(name, session).await?;
                                }
                            }
                            Some("assistant") => {
                                if let Some(content) = event["message"]["content"].as_array() {
                                    for block in content {
                                        if block["type"] == "text" {
                                            if let Some(text) = block["text"].as_str().filter(|s| !crate::jsfmt::trim(s).is_empty()) {
                                                emit(AgentEvent::Text { text: text.into() });
                                            }
                                        }
                                    }
                                }
                            }
                            Some("result") if event["is_error"] == true => {
                                let message = event["result"].as_str().or(event["subtype"].as_str()).unwrap_or("Claude Code failed");
                                missing |= stale(message, resume.as_deref());
                                errors.push(message.into());
                            }
                            _ => {}
                        }
                    }
                }
            }
            // stdout is drained before waiting; stderr and stdin have their own tasks.
            let status = child.wait().await.map_err(|e| e.to_string())?;
            let (tail, stderr_missing) = stderr.await.map_err(|e| e.to_string())??;
            let input_result = input.await.map_err(|e| e.to_string())?;
            if attempt == 0 && resume.is_some() && (missing || stderr_missing) && (!status.success() || !errors.is_empty()) {
                *session.claude_session.lock().unwrap() = None;
                projects.save(name, session).await?;
                resume = None;
                continue;
            }
            if !status.success() && errors.is_empty() {
                errors.push(if tail.is_empty() { format!("Claude Code exited with {status}") } else { tail });
            }
            if errors.is_empty() {
                input_result.map_err(|e| format!("Claude Code stdin failed: {e}"))?;
            }
            for message in errors { emit(AgentEvent::Error { message }); }
            break;
        }
        Ok(())
    }.await;
    drop(live);
    // In-flight MCP calls retain their sender until tool_result, which must precede done.
    while let Some(event) = receiver.recv().await {
        emit(event);
    }
    result
}
