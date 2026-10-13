mod common;
use common::TempDir;
use forge_server::tools::{
    launch::{resolve_app, Resolved},
    script3d::{parse_runner_output, run_app, BLENDER},
    ToolContext,
};
use std::{path::PathBuf, process::Command};

#[test]
fn blender_flatpak_runs_with_scoped_access_and_can_be_disabled() {
    const CHILD_DIR: &str = "FORGE_FLATPAK_CHILD";
    let app_dir = forge_server::tools::paths::lexical_path(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/.."
    )));
    if let Some(dir) = std::env::var_os(CHILD_DIR) {
        let dir = PathBuf::from(dir);
        let mut spec = BLENDER.clone();
        // Force the Flatpak path even when the test machine has native Blender.
        spec.native.clear();
        if std::env::var("FORGE_NO_FLATPAK").as_deref() == Ok("1") {
            assert_eq!(resolve_app(&spec), None);
            return;
        }
        assert_eq!(
            resolve_app(&spec),
            Some(Resolved::Flatpak {
                id: "org.blender.Blender".into(),
                command: "blender".into(),
            })
        );
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let ctx = ToolContext::new(dir.join("project"), app_dir.clone());
            let result = run_app(
                &spec,
                vec![
                    "--background".into(),
                    "--python".into(),
                    app_dir
                        .join("server/runners/blender_runner.py")
                        .to_string_lossy()
                        .into_owned(),
                ],
                &ctx,
                5_000,
                vec![("FORGE_NAME".into(), "cube".into())],
            )
            .await
            .unwrap();
            let parsed = parse_runner_output(&result).unwrap();
            assert!(parsed.error.is_none(), "{:?}", parsed.error);
            assert_eq!(
                parsed.result.unwrap()["size"],
                serde_json::json!([10, 20, 30])
            );
        });
        return;
    }
    let dir = TempDir::new("flatpak");
    let bin = dir.path.join("bin");
    std::fs::create_dir(&bin).unwrap();
    let fake = bin.join("flatpak");
    std::fs::write(
        &fake,
        r#"#!/bin/sh
[ "$1" = info ] && { [ "$2" = org.blender.Blender ]; exit; }
[ "$1" = run ] || exit 1
[ "$2" = --command=sh ] && { echo blender; exit 0; }
[ "$2" = --command=blender ] || exit 1
printf '%s\n' "$@" > "$FORGE_FLATPAK_CHILD/access.log"
printf '%s\n' 'FORGE_RESULT {"size":[10,20,30]}'
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let project = dir.path.join("project");
    std::fs::create_dir(&project).unwrap();
    // Child processes isolate the environment and the Flatpak probe cache.
    for disabled in ["0", "1"] {
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "blender_flatpak_runs_with_scoped_access_and_can_be_disabled",
                "--nocapture",
            ])
            .env(CHILD_DIR, &dir.path)
            .env("PATH", &bin)
            .env("FORGE_NO_FLATPAK", disabled)
            .output()
            .unwrap();
        assert!(
            child.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
    }
    let access = std::fs::read_to_string(dir.path.join("access.log")).unwrap();
    assert_eq!(
        access.lines().collect::<Vec<_>>(),
        [
            "run".to_owned(),
            "--command=blender".into(),
            format!("--filesystem={}", project.display()),
            format!(
                "--filesystem={}:ro",
                app_dir.join("server/runners").display()
            ),
            "--env=FORGE_NAME=cube".into(),
            "org.blender.Blender".into(),
            "--background".into(),
            "--python".into(),
            app_dir
                .join("server/runners/blender_runner.py")
                .to_string_lossy()
                .into_owned(),
        ]
    );
}
