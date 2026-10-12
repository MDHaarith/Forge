use super::exec::{find_program, on_path};
use crate::jsfmt::trim;
use std::{
    collections::HashMap,
    io::Read,
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

/// A desktop program Forge drives headless, installed either normally (dnf, a tarball, an
/// AppImage) or as a Flatpak. Fedora Workstation ships Flatpak, and some programs (FreeCAD)
/// are only packaged there, so both have to work.
#[derive(Clone, Debug)]
pub struct AppSpec {
    /// Shown in the UI when the program is missing.
    pub label: String,
    /// Executable names or paths, tried in order.
    pub native: Vec<String>,
    pub flatpak: Option<FlatpakSpec>,
}

#[derive(Clone, Debug)]
pub struct FlatpakSpec {
    pub id: String,
    pub commands: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolved {
    Native { program: String },
    Flatpak { id: String, command: String },
}
#[derive(Debug)]
pub struct Launch {
    pub cmd: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// Finds the command inside an installed Flatpak app, e.g. freecadcmd in org.freecad.FreeCAD. Cached.
fn flatpak_command(id: &str, commands: &[String]) -> Option<String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let key = format!("{id}:{}", commands.join(","));
    if let Some(value) = cache.lock().unwrap().get(&key).cloned() {
        return value;
    }
    let mut found = None;
    if std::env::var("FORGE_NO_FLATPAK").as_deref() != Ok("1")
        && on_path("flatpak")
        && Command::new("flatpak")
            .args(["info", id])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    {
        let probe = commands
            .iter()
            .map(|c| format!("command -v {c} >/dev/null 2>&1 && {{ echo {c}; exit 0; }}"))
            .collect::<Vec<_>>()
            .join("; ")
            + "; exit 1";
        if let Ok(mut child) = Command::new("flatpak")
            .args(["run", "--command=sh", id, "-c", &probe])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            let mut stdout = child.stdout.take().unwrap();
            let reader = std::thread::spawn(move || {
                let mut out = String::new();
                let _ = stdout.read_to_string(&mut out);
                out
            });
            let deadline = Instant::now() + Duration::from_secs(60);
            let status = loop {
                match child.try_wait() {
                    Ok(Some(status)) => break Some(status),
                    Err(_) => break None,
                    Ok(None) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(20))
                    }
                    Ok(None) => break None,
                }
            };
            if status.is_none() {
                let _ = child.kill();
                let _ = child.wait();
            }
            let out = reader.join().unwrap_or_default();
            if status.is_some_and(|s| s.success()) {
                found = trim(&out)
                    .split('\n')
                    .next_back()
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned);
            }
        }
    }
    cache.lock().unwrap().insert(key, found.clone());
    found
}

/// A normal install wins over a Flatpak: it starts faster and sees the whole filesystem.
pub fn resolve_app(spec: &AppSpec) -> Option<Resolved> {
    if let Some(program) = find_program(&spec.native) {
        return Some(Resolved::Native { program });
    }
    let flatpak = spec.flatpak.as_ref()?;
    flatpak_command(&flatpak.id, &flatpak.commands).map(|command| Resolved::Flatpak {
        id: flatpak.id.clone(),
        command,
    })
}

/// Builds the command line. A Flatpak is sandboxed, so it gets explicit access to the folders it
/// must write (`rw`) and read (`ro`), and its environment is passed with --env.
pub fn launch_args(
    app: &Resolved,
    args: Vec<String>,
    rw: &[String],
    ro: &[String],
    env: Vec<(String, String)>,
) -> Launch {
    match app {
        Resolved::Native { program } => Launch {
            cmd: program.clone(),
            args,
            env,
        },
        Resolved::Flatpak { id, command } => {
            let mut launch = vec!["run".into(), format!("--command={command}")];
            launch.extend(rw.iter().map(|d| format!("--filesystem={d}")));
            launch.extend(ro.iter().map(|d| format!("--filesystem={d}:ro")));
            launch.extend(env.iter().map(|(k, v)| format!("--env={k}={v}")));
            launch.push(id.clone());
            launch.extend(args);
            Launch {
                cmd: "flatpak".into(),
                args: launch,
                env,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flatpak_access_and_environment_are_explicit() {
        let launch = launch_args(
            &Resolved::Flatpak {
                id: "org.freecad.FreeCAD".into(),
                command: "freecadcmd".into(),
            },
            vec!["runner.py".into()],
            &["/home/me/Forge/rover".into()],
            &["/opt/forge/runners".into()],
            vec![("FORGE_NAME".into(), "plate".into())],
        );
        assert_eq!(launch.cmd, "flatpak");
        assert_eq!(
            launch.args,
            [
                "run",
                "--command=freecadcmd",
                "--filesystem=/home/me/Forge/rover",
                "--filesystem=/opt/forge/runners:ro",
                "--env=FORGE_NAME=plate",
                "org.freecad.FreeCAD",
                "runner.py"
            ]
        );
        let native = launch_args(
            &Resolved::Native {
                program: "/usr/bin/blender".into(),
            },
            vec!["-b".into()],
            &[],
            &[],
            vec![],
        );
        assert_eq!(native.cmd, "/usr/bin/blender");
        assert_eq!(native.args, ["-b"]);
    }
}
