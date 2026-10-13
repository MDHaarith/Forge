mod common;
use common::TempDir;
use forge_server::{
    tools::{all_tools, available_tools, describe_tools, ToolContext},
    Settings,
};
use serde_json::json;

#[tokio::test]
async fn workspace_checks_paths_read_limits_and_byte_counts() {
    let dir = TempDir::new("workspace");
    let ctx = ToolContext::new(dir.path.clone(), dir.path.clone());
    let tools = all_tools();
    let tool = |name| tools.iter().find(|t| t.name == name).unwrap();
    assert_eq!(
        (tool("list_files").run)(json!({}), ctx.clone())
            .await
            .unwrap()
            .text,
        "(the project is empty)"
    );
    let write =
        (tool("write_file").run)(json!({"path":"nested/a.txt","contents":"×"}), ctx.clone())
            .await
            .unwrap();
    assert_eq!(write.text, "wrote nested/a.txt (2 bytes)");
    assert_eq!(write.files, ["nested/a.txt"]);
    assert_eq!(
        (tool("read_file").run)(json!({"path":"nested/a.txt"}), ctx.clone())
            .await
            .unwrap()
            .text,
        "×"
    );
    for path in ["../outside", ".git/config"] {
        assert!(
            (tool("write_file").run)(json!({"path":path,"contents":"x"}), ctx.clone())
                .await
                .is_err()
        );
    }
    assert_eq!(
        (tool("read_file").run)(json!({"path":"missing"}), ctx.clone())
            .await
            .unwrap_err()
            .to_string(),
        "missing does not exist"
    );
    std::fs::write(dir.path.join("big"), vec![0u8; 200_001]).unwrap();
    assert_eq!(
        (tool("read_file").run)(json!({"path":"big"}), ctx.clone())
            .await
            .unwrap_err()
            .to_string(),
        "big is 200001 bytes; only files up to 200000 bytes can be read"
    );
    std::fs::create_dir(dir.path.join("node_modules")).unwrap();
    std::fs::write(dir.path.join("node_modules/skip"), "x").unwrap();
    std::fs::write(dir.path.join("A"), "x").unwrap();
    std::fs::write(dir.path.join("a"), "x").unwrap();
    assert_eq!(
        (tool("list_files").run)(json!({}), ctx).await.unwrap().text,
        "A\na\nbig\nnested/a.txt"
    );
}

#[tokio::test]
async fn commands_require_explicit_opt_in_in_context_and_registry() {
    let dir = TempDir::new("commands");
    let mut settings = Settings::new(dir.path.clone(), dir.path.clone());
    let tool = all_tools()
        .into_iter()
        .find(|t| t.name == "run_command")
        .unwrap();
    assert!(!available_tools(&settings)
        .iter()
        .any(|t| t.name == "run_command"));
    let input = json!({"program":"sh","args":["-c","printf result"]});
    assert_eq!(
        (tool.run)(
            input.clone(),
            ToolContext::from_settings(dir.path.clone(), &settings)
        )
        .await
        .unwrap_err()
        .to_string(),
        "running commands is disabled; start Forge with FORGE_ALLOW_COMMANDS=1 to allow it"
    );
    settings.allow_commands = true;
    assert!(available_tools(&settings)
        .iter()
        .any(|t| t.name == "run_command"));
    assert_eq!(
        describe_tools(&settings)
            .iter()
            .find(|t| t["name"] == "run_command")
            .unwrap()["available"],
        true
    );
    assert_eq!(
        (tool.run)(
            input,
            ToolContext::from_settings(dir.path.clone(), &settings)
        )
        .await
        .unwrap()
        .text,
        "exit code 0\nstdout:\nresult"
    );
}

