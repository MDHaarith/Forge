mod common;
use common::TempDir;
use forge_server::tools::{
    launch::{launch_args, resolve_app, Resolved},
    script3d::{parse_runner_output, run_app, FREECAD},
    ToolContext,
};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[test]
fn freecad_installed_only_as_flatpak_is_found_and_runs() {
    const CHILD_DIR: &str = "FORGE_PHASE1_FLATPAK_CHILD";
    let app_dir = forge_server::tools::paths::lexical_path(Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/.."
    )));
    if let Some(dir) = std::env::var_os(CHILD_DIR) {
        let dir = PathBuf::from(dir);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            assert!(matches!(
                resolve_app(&FREECAD),
                Some(Resolved::Flatpak { .. })
            ));
            let ctx = ToolContext::new(dir.join("project"), app_dir.clone());
            let result = run_app(
                &FREECAD,
                vec![app_dir
                    .join("server/runners/freecad_runner.py")
                    .to_string_lossy()
                    .into_owned()],
                &ctx,
                300_000,
                vec![
                    ("PYTHONDONTWRITEBYTECODE".into(), "1".into()),
                    (
                        "FORGE_PROJECT".into(),
                        ctx.project_dir.to_string_lossy().into_owned(),
                    ),
                    (
                        "FORGE_SCRIPT".into(),
                        ctx.project_dir
                            .join("cad/cube.py")
                            .to_string_lossy()
                            .into_owned(),
                    ),
                    ("FORGE_NAME".into(), "cube".into()),
                    ("FORGE_FORMATS".into(), "step".into()),
                ],
            )
            .await
            .unwrap();
            let parsed = parse_runner_output(&result).unwrap();
            assert!(parsed.error.is_none(), "{:?}", parsed.error);
            let mut result = parsed.result.unwrap();
            forge_server::jsfmt::normalize_json(&mut result);
            assert_eq!(result["size_mm"], serde_json::json!([10, 20, 30]));
            assert_eq!(result["volume_mm3"], 6000);
            assert_eq!(result["valid"], true);
            println!("10 × 20 × 30 mm, volume 6000 mm³, valid: true");
        });
        return;
    }
    let Some(real) = resolve_app(&FREECAD) else {
        eprintln!("skipped: freecadcmd not installed (native or Flatpak)");
        return;
    };
    let dir = TempDir::new("flatpak");
    let bin = dir.path.join("bin");
    std::fs::create_dir(&bin).unwrap();
    let runner = match real {
        Resolved::Native { program } => format!("exec {} \"$@\"", shell_quote(&program)),
        Resolved::Flatpak {
            ref id,
            ref command,
        } => {
            let launch = launch_args(&real, vec![], &[], &[], vec![]);
            assert_eq!(launch.cmd, "flatpak");
            assert_eq!(id, "org.freecad.FreeCAD");
            assert_eq!(command, "freecadcmd");
            let flatpak = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|p| p.join("flatpak"))
                .find(|p| p.is_file())
                .unwrap();
            format!(
                "exec {} run --command=freecadcmd \"${{access[@]}}\" org.freecad.FreeCAD \"$@\"",
                shell_quote(&flatpak.to_string_lossy())
            )
        }
    };
    // A stand-in flatpak logs access grants and forwards execution to the real installation.
    let script = format!(
        r#"#!/bin/bash
[ "$1" = info ] && {{ [ "$2" = org.freecad.FreeCAD ]; exit; }}
[ "$1" = run ] || exit 1
shift
cmd=
access=()
while [ $# -gt 0 ]; do
  case "$1" in
    --command=*) cmd="${{1#--command=}}" ;;
    --filesystem=*) echo "$1" >> {log}; access+=("$1") ;;
    --env=*) export "${{1#--env=}}" ;;
    org.freecad.FreeCAD) shift; break ;;
  esac
  shift
done
[ "$cmd" = sh ] && {{ echo freecadcmd; exit 0; }}
[ "$cmd" = freecadcmd ] || exit 1
{runner}
"#,
        log = shell_quote(&dir.path.join("access.log").to_string_lossy())
    );
    let fake = bin.join("flatpak");
    std::fs::write(&fake, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let project = dir.path.join("project");
    std::fs::create_dir_all(project.join("cad")).unwrap();
    std::fs::write(
        project.join("cad/cube.py"),
        "result = Part.makeBox(10, 20, 30)\n",
    )
    .unwrap();
    // A child process, so its test configuration cannot race the parent tests' environment.
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "freecad_installed_only_as_flatpak_is_found_and_runs",
            "--nocapture",
        ])
        .env(CHILD_DIR, &dir.path)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("FORGE_NO_FLATPAK", "0")
        .env("FORGE_FREECAD", "")
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    assert!(String::from_utf8_lossy(&child.stdout)
        .contains("10 × 20 × 30 mm, volume 6000 mm³, valid: true"));
    let access = std::fs::read_to_string(dir.path.join("access.log")).unwrap();
    assert_eq!(
        access.trim().lines().collect::<Vec<_>>(),
        [
            format!("--filesystem={}", project.display()),
            format!(
                "--filesystem={}:ro",
                app_dir.join("server/runners").display()
            )
        ]
    );
    assert!(Path::new(&project.join("models/cube.step")).is_file());
}
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}
