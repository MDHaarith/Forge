use crate::{
    platform::{current_distro, install_text, InstallHint},
    tools::{
        launch::{launch_args, resolve_app, AppSpec, FlatpakSpec},
        paths::project_path,
        script3d::{with_override, BLENDER},
    },
    Settings,
};
use serde_json::{json, Value};
use std::{
    path::Path,
    process::{Command, Stdio},
};

pub struct Opener {
    pub name: &'static str,
    pub types: Vec<&'static str>,
    pub app: AppSpec,
    pub install: InstallHint,
}
fn hint(value: Value) -> InstallHint {
    serde_json::from_value(value).unwrap()
}

/// Desktop programs that project files open in, so a part built by Forge can be refined by hand.
pub fn openers(settings: &Settings) -> Vec<Opener> {
    let kicad = |command: &str| AppSpec {
        label: command.into(),
        native: vec![command.into()],
        flatpak: Some(FlatpakSpec {
            id: "org.kicad.KiCad".into(),
            commands: vec![command.into()],
        }),
    };
    let kicad_hint = hint(
        json!({"dnf":"kicad","apt":"kicad","flatpak":"org.kicad.KiCad","brew":"--cask kicad","url":"https://www.kicad.org/download/"}),
    );
    vec![
        Opener {
            name: "Blender",
            types: vec![".blend"],
            app: with_override(&BLENDER, settings.blender_program.as_deref()),
            install: hint(
                json!({"dnf":"blender","apt":"blender","flatpak":"org.blender.Blender","brew":"--cask blender","url":"https://www.blender.org/download/"}),
            ),
        },
        Opener {
            name: "KiCad",
            types: vec![".kicad_pcb"],
            app: kicad("pcbnew"),
            install: kicad_hint.clone(),
        },
        Opener {
            name: "KiCad",
            types: vec![".kicad_sch"],
            app: kicad("eeschema"),
            install: kicad_hint,
        },
    ]
}

#[derive(Debug)]
pub struct OpenError {
    pub status: u16,
    pub message: String,
}
impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for OpenError {}
fn error(status: u16, message: impl Into<String>) -> OpenError {
    OpenError {
        status,
        message: message.into(),
    }
}

/// Which installed program opens each file type, for the UI's "Open in …" button.
pub fn opener_types(settings: &Settings) -> Value {
    let mut types = json!({});
    for opener in openers(settings) {
        if resolve_app(&opener.app).is_some() {
            for extension in opener.types {
                types[extension] = json!(opener.name);
            }
        }
    }
    types
}

/// Starts the program for a project file in the background and returns the program's name.
pub async fn open_in_app(
    project_dir: &Path,
    rel: &str,
    settings: &Settings,
) -> Result<String, OpenError> {
    let full = project_path(project_dir, rel).map_err(|e| error(400, e.to_string()))?;
    let extension = full
        .extension()
        .map(|v| format!(".{}", v.to_string_lossy().to_lowercase()))
        .unwrap_or_default();
    let opener = openers(settings)
        .into_iter()
        .find(|o| o.types.iter().any(|t| *t == extension))
        .ok_or_else(|| {
            error(
                400,
                format!(
                    "Forge does not know a program for {}",
                    full.file_name().unwrap_or_default().to_string_lossy()
                ),
            )
        })?;
    if !tokio::fs::metadata(&full).await.is_ok_and(|m| m.is_file()) {
        return Err(error(404, "no such file"));
    }
    let app = opener.app.clone();
    let resolved = tokio::task::spawn_blocking(move || resolve_app(&app))
        .await
        .map_err(|e| error(500, e.to_string()))?;
    let app = resolved.ok_or_else(|| {
        let hint = install_text(Some(&opener.install), current_distro());
        error(
            501,
            format!(
                "{} is not installed{}",
                opener.name,
                hint.map(|h| format!(": {h}")).unwrap_or_default()
            ),
        )
    })?;
    // A Flatpak gets the project folder only, read-write so the user can save their changes.
    let launch = launch_args(
        &app,
        vec![full.to_string_lossy().into_owned()],
        &[project_dir.to_string_lossy().into_owned()],
        &[],
        vec![],
    );
    let mut child = Command::new(&launch.cmd)
        .args(&launch.args)
        .current_dir(project_dir)
        .envs(launch.env)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| error(500, format!("could not start {}: {e}", opener.name)))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(opener.name.into())
}
