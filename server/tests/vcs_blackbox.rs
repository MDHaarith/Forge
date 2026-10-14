mod common;

use common::TempDir;
use forge_server::{
    jsfmt::trim,
    model::OfflineModel,
    projects::Projects,
    tools::{self, vcs, ToolContext},
    Settings,
};
use serde_json::{json, Value};
use std::{fs, path::Path, sync::Arc};

async fn tool_result(name: &str, input: Value, dir: &Path) -> Result<String, String> {
    let tool = vcs::tools()
        .into_iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("tool {name} not found"));
    assert!(
        tool.requires.is_empty() && tool.install.is_none(),
        "tool {name} must have empty requires and no install hint"
    );
    (tool.run)(input, ToolContext::new(dir.to_owned(), dir.to_owned()))
        .await
        .map(|r| r.text)
        .map_err(|e| e.to_string())
}

async fn tool(name: &str, input: Value, dir: &Path) -> String {
    match tool_result(name, input, dir).await {
        Ok(text) => text,
        Err(err) => err,
    }
}

/// 1. Tool list: git_status, git_commit, git_log exist and have no requires/install hint for git.
/// openscad_render, chip_simulate, chip_synthesize are absent.
#[tokio::test]
async fn test_1_tool_list_git_tools_present_no_hints_deprecated_tools_absent() {
    let all = tools::all_tools();
    for name in ["git_status", "git_commit", "git_log"] {
        let t = all
            .iter()
            .find(|tool| tool.name == name)
            .unwrap_or_else(|| panic!("expected {name} in all_tools"));
        assert!(
            t.requires.is_empty(),
            "{name} requires must be empty, got {:?}",
            t.requires
        );
        assert!(t.install.is_none(), "{name} install hint must be none");
        assert!(t.needs.is_none(), "{name} needs must be none");
        assert!(t.app.is_none(), "{name} app must be none");
    }

    for absent in ["openscad_render", "chip_simulate", "chip_synthesize"] {
        assert!(
            !all.iter().any(|tool| tool.name == absent),
            "{absent} must not be present in all_tools"
        );
    }

    let temp = TempDir::new("bb-settings");
    let settings = Settings::new(temp.path.clone(), temp.path.clone());
    let described = tools::describe_tools(&settings);

    for name in ["git_status", "git_commit", "git_log"] {
        let entry = described
            .iter()
            .find(|d| d["name"] == name)
            .unwrap_or_else(|| panic!("expected {name} in describe_tools"));
        assert!(
            entry.get("requires").is_none(),
            "{name} must not have requires in describe_tools"
        );
        assert!(
            entry.get("install").is_none(),
            "{name} must not have install in describe_tools"
        );
        assert_eq!(entry["available"], true);
    }

    for absent in ["openscad_render", "chip_simulate", "chip_synthesize"] {
        assert!(
            !described.iter().any(|d| d["name"] == absent),
            "{absent} must not be present in describe_tools"
        );
    }
}

/// 2. Fresh project dir with no .git: each git tool returns the "not a git repository" error text from the brief.
#[tokio::test]
async fn test_2_fresh_project_dir_no_git_reports_error() {
    let dir = TempDir::new("bb-no-git");
    let expected_error =
        "exit code 128\nstderr:\nfatal: not a git repository (or any of the parent directories): .git";

    let status_out = tool("git_status", json!({}), &dir.path).await;
    assert_eq!(status_out, expected_error, "git_status error mismatch");

    let log_out = tool("git_log", json!({}), &dir.path).await;
    assert_eq!(log_out, expected_error, "git_log error mismatch");

    let commit_err = tool_result("git_commit", json!({"message": "test commit"}), &dir.path)
        .await
        .unwrap_err();
    assert_eq!(commit_err, expected_error, "git_commit error mismatch");
}

/// 3. Init through project creation, then: add files, status shows ??; commit; status says working tree clean;
/// modify, delete, add files -> M/D/?? in short-format order; commit again; git_log lists both commits newest first
/// with the messages, limit honoured.
#[tokio::test]
async fn test_3_git_lifecycle_init_commit_modify_delete_status_order_log_limit() {
    let home = TempDir::new("bb-lifecycle-home");
    let projects = Projects::new(
        Settings::new(home.path.clone(), home.path.clone()),
        Arc::new(OfflineModel),
    );
    projects.create("demo_proj").await.unwrap();
    let dir = projects.dir("demo_proj").unwrap();

    assert_eq!(
        tool("git_status", json!({}), &dir).await,
        "working tree clean"
    );

    let initial_log = tool("git_log", json!({}), &dir).await;
    assert_eq!(initial_log.lines().count(), 1);
    assert!(initial_log.ends_with(" Start project"));

    fs::write(dir.join("a.txt"), "hello a").unwrap();
    fs::write(dir.join("b.txt"), "hello b").unwrap();
    assert_eq!(
        tool("git_status", json!({}), &dir).await,
        "?? a.txt\n?? b.txt"
    );

    let commit1 = tool("git_commit", json!({"message": "Add a and b"}), &dir).await;
    assert!(
        commit1.starts_with("committed ") && commit1.ends_with(" Add a and b"),
        "unexpected commit response: {commit1}"
    );
    assert_eq!(
        tool("git_status", json!({}), &dir).await,
        "working tree clean"
    );

    fs::write(dir.join("a.txt"), "modified a").unwrap();
    fs::remove_file(dir.join("b.txt")).unwrap();
    fs::write(dir.join("c.txt"), "hello c").unwrap();

    let status = tool("git_status", json!({}), &dir).await;
    assert_eq!(
        status,
        "M a.txt\n D b.txt\n?? c.txt",
        "status short-format ordering mismatch"
    );

    let commit2 = tool("git_commit", json!({"message": "Update files"}), &dir).await;
    assert!(
        commit2.starts_with("committed ") && commit2.ends_with(" Update files"),
        "unexpected commit response: {commit2}"
    );
    assert_eq!(
        tool("git_status", json!({}), &dir).await,
        "working tree clean"
    );

    let log_all = tool("git_log", json!({}), &dir).await;
    let lines: Vec<&str> = log_all.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].ends_with(" Update files"), "line 0: {}", lines[0]);
    assert!(lines[1].ends_with(" Add a and b"), "line 1: {}", lines[1]);
    assert!(lines[2].ends_with(" Start project"), "line 2: {}", lines[2]);

    let log_limit_1 = tool("git_log", json!({"limit": 1}), &dir).await;
    assert_eq!(log_limit_1.lines().count(), 1);
    assert!(log_limit_1.ends_with(" Update files"));

    let log_limit_2 = tool("git_log", json!({"limit": 2}), &dir).await;
    let lines_2: Vec<&str> = log_limit_2.lines().collect();
    assert_eq!(lines_2.len(), 2);
    assert!(lines_2[0].ends_with(" Update files"));
    assert!(lines_2[1].ends_with(" Add a and b"));
}

/// 4. Nested dirs, file names with spaces and unicode: status quoting per brief; commit captures them; empty dirs ignored.
#[tokio::test]
async fn test_4_nested_dirs_spaces_unicode_quoting_and_empty_dirs() {
    let dir = TempDir::new("bb-paths");
    vcs::init_repo(&dir.path).await.unwrap();

    fs::create_dir_all(dir.path.join("empty_dir")).unwrap();
    fs::create_dir_all(dir.path.join("nested/deep_empty")).unwrap();

    fs::create_dir_all(dir.path.join("nested/sub")).unwrap();
    fs::write(dir.path.join("nested/sub/data.txt"), "nested content").unwrap();

    fs::write(dir.path.join("space file.txt"), "space content").unwrap();

    fs::write(dir.path.join("café.txt"), "unicode content").unwrap();

    let status = tool("git_status", json!({}), &dir.path).await;
    assert!(!status.contains("empty_dir"));
    assert!(!status.contains("deep_empty"));

    assert!(status.contains("?? \"caf\\303\\251.txt\""), "expected octal quoted café.txt, got:\n{status}");
    assert!(status.contains("?? \"space file.txt\""), "expected quoted space file, got:\n{status}");
    assert!(status.contains("?? nested/"), "expected nested/ directory summary, got:\n{status}");

    let commit = tool("git_commit", json!({"message": "Add special paths"}), &dir.path).await;
    assert!(commit.starts_with("committed "));
    assert_eq!(
        tool("git_status", json!({}), &dir.path).await,
        "working tree clean"
    );

    let proj = dir.path.clone();
    tokio::task::spawn_blocking(move || {
        let repo = gix::open(proj).unwrap();
        let index = repo.index().unwrap();
        let paths: Vec<String> = index
            .entries()
            .iter()
            .map(|e| e.path(&index).to_string())
            .collect();
        assert!(paths.contains(&"nested/sub/data.txt".to_string()));
        assert!(paths.contains(&"space file.txt".to_string()));
        assert!(paths.contains(&"café.txt".to_string()));
        assert!(!paths.iter().any(|p| p.contains("empty_dir") || p.contains("deep_empty")));
    })
    .await
    .unwrap();

    let config = dir.path.join(".git/config");
    let original_config = fs::read_to_string(&config).unwrap();
    fs::write(
        &config,
        format!("{original_config}\n[core]\nquotePath = false\n"),
    )
    .unwrap();

    fs::write(dir.path.join("café.txt"), "updated unicode").unwrap();
    fs::write(dir.path.join("space file.txt"), "updated space").unwrap();

    let unquoted_status = tool("git_status", json!({}), &dir.path).await;
    assert!(unquoted_status.contains("café.txt"), "expected raw café.txt, got:\n{unquoted_status}");
    assert!(unquoted_status.contains(" M \"space file.txt\""), "expected quoted space file, got:\n{unquoted_status}");
}

/// 5. .gitignore honoured in status and commit.
#[tokio::test]
async fn test_5_gitignore_honoured_in_status_and_commit() {
    let dir = TempDir::new("bb-gitignore");
    vcs::init_repo(&dir.path).await.unwrap();

    fs::write(
        dir.path.join(".gitignore"),
        "*.tmp\nsecret.dat\nbuild/\n",
    )
    .unwrap();
    fs::write(dir.path.join("notes.txt"), "track me").unwrap();
    fs::write(dir.path.join("test.tmp"), "ignored tmp").unwrap();
    fs::write(dir.path.join("secret.dat"), "ignored secret").unwrap();
    fs::create_dir_all(dir.path.join("build")).unwrap();
    fs::write(dir.path.join("build/app.bin"), "ignored build").unwrap();

    let status = tool("git_status", json!({}), &dir.path).await;
    assert_eq!(status, "?? .gitignore\n?? notes.txt");

    let commit = tool("git_commit", json!({"message": "Add tracked files"}), &dir.path).await;
    assert!(commit.starts_with("committed "));
    assert_eq!(
        tool("git_status", json!({}), &dir.path).await,
        "working tree clean"
    );

    let no_change = tool("git_commit", json!({"message": "No change"}), &dir.path).await;
    assert_eq!(no_change, "nothing to commit");

    let proj = dir.path.clone();
    tokio::task::spawn_blocking(move || {
        let repo = gix::open(proj).unwrap();
        let index = repo.index().unwrap();
        let paths: Vec<String> = index
            .entries()
            .iter()
            .map(|e| e.path(&index).to_string())
            .collect();
        assert_eq!(paths, vec![".gitignore", "notes.txt"]);
    })
    .await
    .unwrap();
}

/// 6. No git identity configured: commit succeeds as Forge <forge@localhost> (check with real git when installed, else via log).
#[tokio::test]
async fn test_6_no_git_identity_commits_as_forge_default() {
    let dir = TempDir::new("bb-no-identity");
    vcs::init_repo(&dir.path).await.unwrap();

    let config = dir.path.join(".git/config");
    let original_config = fs::read_to_string(&config).unwrap();
    fs::write(
        &config,
        format!("{original_config}\n[user]\nname = \"\"\nemail = \"\"\n"),
    )
    .unwrap();

    fs::write(dir.path.join("first.txt"), "forge content").unwrap();
    let res = tool("git_commit", json!({"message": "Commit with default identity"}), &dir.path).await;
    assert!(res.starts_with("committed "));

    use forge_server::tools::exec::{on_path, run};
    if tokio::task::spawn_blocking(|| on_path("git")).await.unwrap() {
        let log = run(
            "git",
            &["log".into(), "-1".into(), "--format=%an <%ae>".into()],
            &dir.path,
            5000,
            &[],
        )
        .await;
        assert_eq!(log.code, 0, "git log failed: {}", log.stderr);
        assert_eq!(trim(&log.stdout), "Forge <forge@localhost>");
    }

    let proj = dir.path.clone();
    tokio::task::spawn_blocking(move || {
        let repo = gix::open(proj).unwrap();
        let commit = repo.head_commit().unwrap();
        let author = commit.author().unwrap();
        assert_eq!(author.name, "Forge");
        assert_eq!(author.email, "forge@localhost");
    })
    .await
    .unwrap();
}

/// 7. If git installed: native commits are readable by git fsck --strict and git log; git status --short matches git_status.
#[tokio::test]
async fn test_7_native_git_matches_real_git_when_installed() {
    use forge_server::tools::exec::{on_path, run};
    if !tokio::task::spawn_blocking(|| on_path("git"))
        .await
        .unwrap()
    {
        eprintln!("skipped: git not installed");
        return;
    }

    let dir = TempDir::new("bb-git-compare");
    vcs::init_repo(&dir.path).await.unwrap();

    fs::write(dir.path.join("tracked.txt"), "v1").unwrap();
    fs::write(dir.path.join("to_delete.txt"), "delete me").unwrap();
    fs::create_dir_all(dir.path.join("src")).unwrap();
    fs::write(dir.path.join("src/main.rs"), "fn main() {}").unwrap();
    fs::write(dir.path.join(".gitignore"), "target/\n").unwrap();

    let commit1 = tool("git_commit", json!({"message": "Initial structure"}), &dir.path).await;
    assert!(commit1.starts_with("committed "));

    fs::write(dir.path.join("tracked.txt"), "v2").unwrap();
    fs::remove_file(dir.path.join("to_delete.txt")).unwrap();
    fs::write(dir.path.join("new_file.txt"), "brand new").unwrap();
    fs::create_dir_all(dir.path.join("target")).unwrap();
    fs::write(dir.path.join("target/ignored.bin"), "bin").unwrap();

    let git_status = run("git", &["status".into(), "--short".into()], &dir.path, 5000, &[]).await;
    assert_eq!(git_status.code, 0, "git status stderr: {}", git_status.stderr);
    let tool_status = tool("git_status", json!({}), &dir.path).await;
    assert_eq!(tool_status, trim(&git_status.stdout), "git_status vs git status --short");

    let commit2 = tool("git_commit", json!({"message": "Second revision\n\nDetailed body"}), &dir.path).await;
    assert!(commit2.starts_with("committed "));

    let git_log = run(
        "git",
        &[
            "log".into(),
            "-20".into(),
            "--format=%h %ad %s".into(),
            "--date=short".into(),
        ],
        &dir.path,
        5000,
        &[],
    )
    .await;
    assert_eq!(git_log.code, 0, "git log stderr: {}", git_log.stderr);
    let tool_log = tool("git_log", json!({}), &dir.path).await;
    assert_eq!(tool_log, trim(&git_log.stdout), "git_log vs real git log");

    let fsck = run("git", &["fsck".into(), "--strict".into()], &dir.path, 5000, &[]).await;
    assert_eq!(fsck.code, 0, "git fsck --strict failed: {}\n{}", fsck.stdout, fsck.stderr);
}

/// 8. Symlink pointing outside the project is committed as a link, never followed.
#[cfg(unix)]
#[tokio::test]
async fn test_8_symlink_pointing_outside_project_committed_as_link_never_followed() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new("bb-symlink-proj");
    let outside = TempDir::new("bb-symlink-out");
    let secret_path = outside.path.join("secret_data.txt");
    fs::write(&secret_path, "top secret outside content").unwrap();

    vcs::init_repo(&dir.path).await.unwrap();

    let link_path = dir.path.join("link_to_secret");
    symlink(&secret_path, &link_path).unwrap();

    let status = tool("git_status", json!({}), &dir.path).await;
    assert_eq!(status, "?? link_to_secret");

    let commit = tool("git_commit", json!({"message": "Add symlink to outside"}), &dir.path).await;
    assert!(commit.starts_with("committed "));

    assert_eq!(
        tool("git_status", json!({}), &dir.path).await,
        "working tree clean"
    );

    let proj = dir.path.clone();
    let expected_target = secret_path.to_string_lossy().into_owned();
    tokio::task::spawn_blocking(move || {
        let repo = gix::open(proj).unwrap();
        let index = repo.index().unwrap();
        let entry = index.entry_by_path("link_to_secret".into()).unwrap();
        assert_eq!(entry.mode, gix::index::entry::Mode::SYMLINK);

        let blob = repo.find_blob(entry.id).unwrap();
        assert_eq!(blob.data, expected_target.as_bytes());
        assert_ne!(blob.data, b"top secret outside content");
    })
    .await
    .unwrap();
}

/// 9. Every new error message in the brief, one assertion each.
#[tokio::test]
async fn test_9_every_vcs_contract_error_message() {
    // 1. Not a git repository
    let no_git_dir = TempDir::new("bb-err-no-git");
    let err_no_git = tool_result("git_commit", json!({"message": "test"}), &no_git_dir.path)
        .await
        .unwrap_err();
    assert_eq!(
        err_no_git,
        "exit code 128\nstderr:\nfatal: not a git repository (or any of the parent directories): .git"
    );

    // 2. Empty commit message
    let git_dir = TempDir::new("bb-err-msg");
    vcs::init_repo(&git_dir.path).await.unwrap();
    fs::write(git_dir.path.join("file.txt"), "hello").unwrap();
    let err_empty_msg = tool_result("git_commit", json!({"message": "   \n\t  "}), &git_dir.path)
        .await
        .unwrap_err();
    assert_eq!(
        err_empty_msg,
        "exit code 1\nstderr:\nAborting commit due to empty commit message."
    );

    // 3. Current branch does not have any commits yet
    let unborn_dir = TempDir::new("bb-err-unborn");
    let path = unborn_dir.path.clone();
    tokio::task::spawn_blocking(move || {
        gix::init(path).unwrap();
    })
    .await
    .unwrap();
    let err_unborn = tool("git_log", json!({}), &unborn_dir.path).await;
    assert_eq!(
        err_unborn,
        "exit code 128\nstderr:\nfatal: your current branch 'main' does not have any commits yet"
    );

    // 4. Missing message string property in input
    let err_bad_input = tool_result("git_commit", json!({}), &git_dir.path)
        .await
        .unwrap_err();
    assert_eq!(err_bad_input, "input.message must be a string");

    // 5. Outside project path (tracked directory replaced by symlink pointing outside)
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let sym_proj = TempDir::new("bb-err-sym-proj");
        let sym_out = TempDir::new("bb-err-sym-out");
        fs::write(sym_out.path.join("secret.txt"), "outside data").unwrap();
        vcs::init_repo(&sym_proj.path).await.unwrap();

        fs::create_dir(sym_proj.path.join("folder")).unwrap();
        fs::write(sym_proj.path.join("folder/secret.txt"), "inside data").unwrap();
        tool("git_commit", json!({"message": "Store folder"}), &sym_proj.path).await;

        fs::remove_dir_all(sym_proj.path.join("folder")).unwrap();
        symlink(&sym_out.path, sym_proj.path.join("folder")).unwrap();

        let err_outside = tool_result("git_commit", json!({"message": "Follow outside"}), &sym_proj.path)
            .await
            .unwrap_err();
        assert_eq!(
            err_outside,
            "path \"folder/secret.txt\" is outside the project"
        );
    }

    // 6. Git repository is outside the project
    let foreign_proj = TempDir::new("bb-err-foreign-proj");
    let foreign_git = TempDir::new("bb-err-foreign-git");
    let fpath = foreign_git.path.clone();
    tokio::task::spawn_blocking(move || {
        gix::init(fpath).unwrap();
    })
    .await
    .unwrap();
    fs::write(
        foreign_proj.path.join(".git"),
        format!("gitdir: {}\n", foreign_git.path.join(".git").display()),
    )
    .unwrap();
    let err_foreign = tool_result("git_commit", json!({"message": "Commit foreign"}), &foreign_proj.path)
        .await
        .unwrap_err();
    assert_eq!(err_foreign, "git repository is outside the project");
}
