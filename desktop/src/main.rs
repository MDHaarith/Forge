// Forge desktop app: runs Forge's Rust server in-process on a free local port and shows
// its web interface in a native window. The server stops when the app exits.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let window =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .title("Forge")
                    .inner_size(1360.0, 860.0)
                    .min_inner_size(820.0, 520.0)
                    .build()?;
            let start = (|| -> Result<_, Box<dyn std::error::Error>> {
                let dir = app_dir(app.handle())?;
                let listener = TcpListener::bind("127.0.0.1:0")?;
                listener.set_nonblocking(true)?;
                let port = listener.local_addr()?.port();
                Ok((listener, dir, port))
            })();
            let (listener, dir, port) = match start {
                Ok(start) => start,
                Err(e) => {
                    show_error(&window, "Forge could not start", &e.to_string());
                    return Ok(());
                }
            };
            let handle = app.handle().clone();
            let server_window = window.clone();
            std::thread::spawn(move || {
                let result = (|| {
                    let runtime = tokio::runtime::Builder::new_multi_thread()
                        .enable_all()
                        .build()?;
                    runtime.block_on(async {
                        let listener = tokio::net::TcpListener::from_std(listener)?;
                        forge_server::serve(listener, dir).await
                    })
                })();
                if let Err(e) = result {
                    show_error(&server_window, "Forge could not start", &e.to_string());
                    handle.exit(1);
                }
            });
            let url = Url::parse(&format!("http://127.0.0.1:{port}/"))?;
            if let Err(e) = window.navigate(url) {
                show_error(&window, "Forge could not open its window", &e.to_string());
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the Forge app");
}

fn show_error(window: &WebviewWindow, title: &str, detail: &str) {
    eprintln!("{title}: {detail}");
    let js = format!(
        "showError({}, {})",
        serde_json::json!(title),
        serde_json::json!(detail)
    );
    let _ = window.eval(js);
}

/// Where the built interface, Python runners and bundled PCB engine live.
fn app_dir(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("FORGE_APP_DIR") {
        return Ok(PathBuf::from(dir));
    }
    if cfg!(debug_assertions) {
        // A development build runs straight from the repository.
        return Ok(Path::new(env!("CARGO_MANIFEST_DIR")).join(".."));
    }
    app.path()
        .resource_dir()
        .map(|d| d.join("app"))
        .map_err(|e| e.to_string())
}
