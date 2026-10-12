use crate::common::TempDir;
use forge_server::{serve_with_settings, Settings};
use serde_json::{json, Value};
use std::{os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};

pub const SUCCESS: &str = r#"cat <<'JSON'
{"type":"system","subtype":"init","session_id":"sess-1"}
{"type":"assistant","message":{"content":[{"type":"text","text":"hello from claude"}]}}
{"type":"result","is_error":false,"result":"hello from claude"}
JSON
"#;

pub const WAIT: &str = r#"printf '%s\n' '{"type":"system","subtype":"init","session_id":"sess-1"}'
touch started
i=0
while [ ! -f go ] && [ "$i" -lt 200 ]; do
  sleep 0.05
  i=$((i + 1))
done
if [ ! -f go ]; then echo 'fake CLI timed out' >&2; exit 1; fi
printf '%s\n' '{"type":"result","is_error":false}'
"#;

pub struct FakeServer {
    pub temp: TempDir,
    pub base: String,
    pub settings: Settings,
    task: tokio::task::JoinHandle<()>,
}
impl FakeServer {
    pub async fn start(script: &str) -> Self {
        let temp = TempDir::new("claude");
        let program = temp.path.join("fake-claude");
        std::fs::write(
            &program,
            format!("#!/bin/sh\nprintf '%s\\n' \"$@\" >> args\ncat > stdin\npwd > cwd\n{script}"),
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut settings = Settings::new(temp.path.join("home"), temp.path.join("app"));
        settings.claude_program = Some(program);
        std::fs::create_dir_all(settings.home.join("test")).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let configured = settings.clone();
        let task =
            tokio::spawn(async move { serve_with_settings(listener, configured).await.unwrap() });
        Self {
            temp,
            base,
            settings,
            task,
        }
    }
    pub fn dir(&self) -> PathBuf {
        self.settings.home.join("test")
    }
    pub async fn post(&self, path: &str, body: Value) -> reqwest::Response {
        reqwest::Client::new()
            .post(format!("{}{path}", self.base))
            .json(&body)
            .send()
            .await
            .unwrap()
    }
    pub async fn chat(&self, message: &str) -> reqwest::Response {
        self.post("/api/projects/test/chat", json!({"message":message}))
            .await
    }
    pub async fn history(&self) -> Value {
        reqwest::Client::new()
            .get(format!("{}/api/projects/test/history", self.base))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    pub async fn wait_started(&self) {
        tokio::time::timeout(Duration::from_secs(10), async {
            while !self.dir().join("started").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }
}
impl Drop for FakeServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub async fn events(response: reqwest::Response) -> Vec<Value> {
    let body = tokio::time::timeout(Duration::from_secs(15), response.text())
        .await
        .unwrap()
        .unwrap();
    body.lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
