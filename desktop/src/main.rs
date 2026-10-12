// Forge desktop app: starts Forge's Node server on a free local port and shows its web
// interface in a native window. The server stops when the window closes.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, RunEvent, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// Forge needs Node.js 22.18 or newer, for built-in TypeScript support.
const MIN_NODE: (u32, u32) = (22, 18);
const START_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Default)]
struct Server(Mutex<Option<Child>>);

/// The last lines the server printed, to explain a failed start.
type Log = Arc<Mutex<VecDeque<String>>>;

fn main() {
    tauri::Builder::default()
        .manage(Server::default())
        .setup(|app| {
            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Forge")
                .inner_size(1360.0, 860.0)
                .min_inner_size(820.0, 520.0)
                .build()?;
            let handle = app.handle().clone();
            std::thread::spawn(move || match start_server(&handle) {
                Ok(url) => {
                    if let Err(e) = window.navigate(url) {
                        show_error(&window, "Forge could not open its window", &e.to_string());
                    }
                }
                Err(e) => show_error(&window, "Forge could not start", &e),
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building the Forge app")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                stop_server(app);
            }
        });
}

fn show_error(window: &WebviewWindow, title: &str, detail: &str) {
    eprintln!("{title}: {detail}");
    let js = format!("showError({}, {})", serde_json::json!(title), serde_json::json!(detail));
    let _ = window.eval(js);
}

/// Where the server, the built interface and their packages live.
fn app_dir(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("FORGE_APP_DIR") {
        return Ok(PathBuf::from(dir));
    }
    if cfg!(debug_assertions) {
        // A development build runs straight from the repository.
        return Ok(Path::new(env!("CARGO_MANIFEST_DIR")).join(".."));
    }
    app.path().resource_dir().map(|d| d.join("app")).map_err(|e| e.to_string())
}

fn start_server(app: &AppHandle) -> Result<Url, String> {
    let dir = app_dir(app)?;
    let entry = dir.join("server").join("index.ts");
    if !entry.is_file() {
        return Err(format!("{} is missing; the app is not installed correctly.", entry.display()));
    }
    let node = find_node()?;
    let port = free_port()?;

    let log: Log = Arc::default();
    let mut child = Command::new(&node)
        .arg(&entry)
        .current_dir(&dir)
        // An explicit port beats any PORT in ~/.config/forge/env, so the app never collides
        // with a Forge already running as a service.
        .env("PORT", port.to_string())
        .env("FORGE_EXIT_WITH_STDIN", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not run {}: {e}", node.display()))?;
    keep_log(child.stdout.take(), &log);
    keep_log(child.stderr.take(), &log);
    // The child keeps its stdin pipe, which closes if this app dies without cleaning up.
    let state = app.state::<Server>();
    *state.0.lock().unwrap() = Some(child);

    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        if responds(port) {
            return Url::parse(&format!("http://127.0.0.1:{port}/")).map_err(|e| e.to_string());
        }
        let exited = state.0.lock().unwrap().as_mut().and_then(|c| c.try_wait().ok().flatten());
        if let Some(status) = exited {
            return Err(format!("The Forge server stopped ({status}).\n\n{}", tail(&log)));
        }
        if Instant::now() > deadline {
            return Err(format!("The Forge server did not answer within {} seconds.\n\n{}", START_TIMEOUT.as_secs(), tail(&log)));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn stop_server(app: &AppHandle) {
    if let Some(mut child) = app.state::<Server>().0.lock().unwrap().take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Echoes a server stream to our own stderr and keeps its last lines.
fn keep_log(stream: Option<impl Read + Send + 'static>, log: &Log) {
    let Some(stream) = stream else { return };
    let log = log.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            eprintln!("[server] {line}");
            let mut log = log.lock().unwrap();
            if log.len() == 40 {
                log.pop_front();
            }
            log.push_back(line);
        }
    });
}

fn tail(log: &Log) -> String {
    let lines: Vec<String> = log.lock().unwrap().iter().cloned().collect();
    if lines.is_empty() { "It printed nothing.".into() } else { lines.join("\n") }
}

fn free_port() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("No free local port: {e}"))?;
    listener.local_addr().map(|a| a.port()).map_err(|e| e.to_string())
}

/// True once Forge answers its status endpoint on the port.
fn responds(port: u16) -> bool {
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else { return false };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let request = format!("GET /api/status HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut head = [0u8; 12];
    stream.read_exact(&mut head).is_ok() && head.starts_with(b"HTTP/1.1 200")
}

/// The first Node.js that is new enough: $FORGE_NODE, then node, node-24, node-22 on PATH.
fn find_node() -> Result<PathBuf, String> {
    let mut candidates: Vec<PathBuf> = std::env::var_os("FORGE_NODE").map(PathBuf::from).into_iter().collect();
    candidates.extend(["node", "node-24", "node-22"].map(PathBuf::from));
    let mut found = Vec::new();
    for node in candidates {
        let Ok(out) = Command::new(&node).args(["-p", "process.versions.node"]).output() else { continue };
        let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if node_ok(&version) {
            return Ok(node);
        }
        found.push(format!("{} is {version}", node.display()));
    }
    let (major, minor) = MIN_NODE;
    let seen = if found.is_empty() { "No Node.js was found.".into() } else { format!("Found: {}.", found.join(", ")) };
    Err(format!("Forge needs Node.js {major}.{minor} or newer. {seen}\n\nOn Fedora: sudo dnf install nodejs\nOr set FORGE_NODE to the node program to use."))
}

fn node_ok(version: &str) -> bool {
    let mut parts = version.trim_start_matches('v').split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let (major, minor) = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    (major, minor) >= MIN_NODE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_versions() {
        assert!(node_ok("22.18.0"));
        assert!(node_ok("v22.22.2"));
        assert!(node_ok("24.1.0"));
        assert!(!node_ok("22.17.9"));
        assert!(!node_ok("20.19.0"));
        assert!(!node_ok(""));
    }

    #[test]
    fn free_ports_are_free() {
        let port = free_port().unwrap();
        assert!(TcpListener::bind(("127.0.0.1", port)).is_ok());
        assert!(!responds(port));
    }
}
