mod common;
use common::TempDir;
use forge_server::{
    tools::{all_tools, describe_tools, is_available},
    Settings,
};

#[test]
fn availability_keeps_registry_requires_semantics_and_pcb_is_native() {
    let dir = TempDir::new("availability");
    let settings = Settings::new(dir.path.clone(), dir.path.clone());
    let tools = all_tools();
    let pcb = tools.iter().find(|t| t.name == "pcb_build").unwrap();
    let rf = tools.iter().find(|t| t.name == "rf_antenna_sim").unwrap();
    assert!(pcb.requires.is_empty());
    assert!(is_available(pcb, &settings));
    let stub = dir.path.join("program-stub");
    std::fs::write(&stub, "").unwrap();

    assert!(rf.requires.is_empty());
    assert!(rf.needs.is_none());
    assert!(rf.install.is_none());
    assert!(is_available(rf, &settings));

    let mut tool = tools
        .iter()
        .find(|t| t.name == "git_status")
        .unwrap()
        .clone();
    tool.requires = vec![
        "forge-missing-program-registry".into(),
        stub.to_string_lossy().into_owned(),
    ];
    assert!(is_available(&tool, &settings));
    // A relative path is checked in the process cwd, as findProgram does in TS.
    tool.requires = vec!["./Cargo.toml".into()];
    assert!(is_available(&tool, &settings));
    tool.requires.clear();
    assert!(is_available(&tool, &settings));
}

#[test]
fn describe_tools_keeps_registry_order_and_requires_labels() {
    let dir = TempDir::new("describe");
    let mut settings = Settings::new(dir.path.clone(), dir.path.clone());
    let tools = all_tools();
    let descriptions = describe_tools(&settings);
    assert_eq!(
        descriptions
            .iter()
            .map(|d| d["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        tools.iter().map(|t| t.name).collect::<Vec<_>>()
    );
    for (tool, description) in tools.iter().zip(&descriptions) {
        let requires = match tool.name {
            "run_command" => Some("FORGE_ALLOW_COMMANDS=1"),
            _ => tool
                .needs
                .or_else(|| tool.app.as_ref().map(|a| a.label.as_str()))
                .or_else(|| tool.requires.last().map(String::as_str)),
        };
        assert_eq!(
            description.get("requires").and_then(|v| v.as_str()),
            requires,
            "{}",
            tool.name
        );
        assert_eq!(
            description["available"].as_bool().unwrap(),
            is_available(tool, &settings)
        );
        assert_eq!(description["domain"], tool.domain);
        assert_eq!(description["description"], tool.description);
    }
    let commands = tools.iter().find(|t| t.name == "run_command").unwrap();
    assert!(!is_available(commands, &settings));
    settings.allow_commands = true;
    assert!(is_available(commands, &settings));
}
