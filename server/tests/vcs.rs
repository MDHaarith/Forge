mod common;

use common::TempDir;
use forge_server::{
    model::OfflineModel,
    projects::Projects,
    tools::{vcs, ToolContext},
    Settings,
};
use serde_json::{json, Value};
use std::{fs, path::Path, sync::Arc};

async fn tool(name: &str, input: Value, dir: &Path) -> String {
    let tool = vcs::tools().into_iter().find(|t| t.name == name).unwrap();
    assert!(tool.requires.is_empty() && tool.install.is_none());
    (tool.run)(input, ToolContext::new(dir.to_owned(), dir.to_owned()))
        .await
        .unwrap()
        .text
}

#[tokio::test]
async fn native_git_lifecycle() {
    // Run the same test with an empty PATH and isolated config, without changing the parent environment.
    let child_home = std::env::var_os("FORGE_TEST_GIT_CHILD");
    if child_home.is_none() || child_home != std::env::var_os("HOME") {
        let home = TempDir::new("git-home");
        let bin = home.path.join("empty-bin");
        fs::create_dir(&bin).unwrap();
        let output = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "native_git_lifecycle", "--nocapture"])
            .env_clear()
            .env("PATH", bin)
            .env("HOME", &home.path)
            .env("XDG_CONFIG_HOME", home.path.join("config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("FORGE_TEST_GIT_CHILD", &home.path)
            .kill_on_drop(true)
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let home = TempDir::new("native-git");
    let projects = Projects::new(
        Settings::new(home.path.clone(), home.path.clone()),
        Arc::new(OfflineModel),
    );
    projects.create("part").await.unwrap();
    assert_eq!(projects.list().await.unwrap(), ["part"]);
    let dir = projects.dir("part").unwrap();
    assert_eq!(
        tool("git_status", json!({}), &dir).await,
        "working tree clean"
    );
    let initial = tool("git_log", json!({}), &dir).await;
    assert_eq!(initial.lines().count(), 1);
    assert!(initial.ends_with(" Start project"), "{initial}");

    fs::write(dir.join(".gitignore"), "ignored.txt\ncache/\n").unwrap();
    fs::write(dir.join("ignored.txt"), "private").unwrap();
    fs::create_dir(dir.join("cache")).unwrap();
    fs::write(dir.join("cache/result"), "ignored").unwrap();
    fs::create_dir(dir.join("models")).unwrap();
    fs::write(dir.join("models/part.txt"), "first").unwrap();
    fs::write(dir.join("space name.txt"), "quoted").unwrap();
    assert_eq!(
        tool("git_status", json!({}), &dir).await,
        "?? .gitignore\n?? models/\n?? \"space name.txt\""
    );
    let committed = tool("git_commit", json!({"message":"Add part"}), &dir).await;
    assert!(
        committed.starts_with("committed ") && committed.ends_with(" Add part"),
        "{committed}"
    );
    let project = dir.clone();
    tokio::task::spawn_blocking(move || {
        let repo = gix::open(project).unwrap();
        let commit = repo.head_commit().unwrap();
        let author = commit.author().unwrap();
        assert_eq!(author.name, "Forge");
        assert_eq!(author.email, "forge@localhost");
        let index = repo.index().unwrap();
        let paths: Vec<_> = index
            .entries()
            .iter()
            .map(|e| e.path(&index).to_string())
            .collect();
        assert_eq!(
            paths,
            [
                ".gitignore",
                "README.md",
                "models/part.txt",
                "space name.txt"
            ]
        );
    })
    .await
    .unwrap();
    assert_eq!(
        tool("git_status", json!({}), &dir).await,
        "working tree clean"
    );
    assert_eq!(
        tool("git_commit", json!({"message":"No change"}), &dir).await,
        "nothing to commit"
    );
    let log = tool("git_log", json!({"limit":1}), &dir).await;
    assert_eq!(log.lines().count(), 1);
    let fields: Vec<_> = log.splitn(3, ' ').collect();
    assert_eq!(committed, format!("committed {} {}", fields[0], fields[2]));
    assert_eq!(fields[1].len(), 10);
    assert_eq!(&fields[1][4..5], "-");
    assert_eq!(&fields[1][7..8], "-");
    assert_eq!(tool("git_log", json!({"limit":0}), &dir).await, log);
    assert_eq!(
        tool("git_log", json!({"limit":999}), &dir)
            .await
            .lines()
            .count(),
        2
    );

    fs::write(
        Path::new(&std::env::var_os("HOME").unwrap()).join(".gitconfig"),
        "[user]\nname = Global User\nemail = global@example.com\n",
    )
    .unwrap();
    fs::write(dir.join("README.md"), "Global identity").unwrap();
    tool("git_commit", json!({"message":"Use global identity"}), &dir).await;
    let project = dir.clone();
    tokio::task::spawn_blocking(move || {
        let repo = gix::open(project).unwrap();
        let commit = repo.head_commit().unwrap();
        assert_eq!(commit.author().unwrap().name, "Global User");
        assert_eq!(commit.author().unwrap().email, "global@example.com");
    })
    .await
    .unwrap();

    let config = dir.join(".git/config");
    let original = fs::read_to_string(&config).unwrap();
    fs::write(
        &config,
        format!("{original}\n[user]\nname = Test User\nemail = test@example.com\n"),
    )
    .unwrap();
    fs::write(dir.join("models/part.txt"), "second").unwrap();
    fs::remove_file(dir.join("space name.txt")).unwrap();
    assert_eq!(
        tool("git_status", json!({}), &dir).await,
        "M models/part.txt\n D \"space name.txt\""
    );
    tool(
        "git_commit",
        json!({"message":"Revise part\ncontinued subject\n\nDetails"}),
        &dir,
    )
    .await;
    let project = dir.clone();
    tokio::task::spawn_blocking(move || {
        let repo = gix::open(project).unwrap();
        let commit = repo.head_commit().unwrap();
        let author = commit.author().unwrap();
        assert_eq!(author.name, "Test User");
        assert_eq!(author.email, "test@example.com");
        assert_eq!(
            commit.message().unwrap().summary().to_string(),
            "Revise part continued subject"
        );
    })
    .await
    .unwrap();
    assert_eq!(
        tool("git_status", json!({}), &dir).await,
        "working tree clean"
    );
    fs::write(dir.join("models/part.txt"), "third").unwrap();
    let commit = vcs::tools()
        .into_iter()
        .find(|t| t.name == "git_commit")
        .unwrap();
    let result = (commit.run)(
        json!({"message":" \n "}),
        ToolContext::new(dir.clone(), dir.clone()),
    )
    .await;
    assert_eq!(
        result.unwrap_err().to_string(),
        "exit code 1\nstderr:\nAborting commit due to empty commit message."
    );
    assert_eq!(
        tool("git_status", json!({}), &dir).await,
        "M  models/part.txt"
    );
    tool("git_commit", json!({"message":"Third part"}), &dir).await;
    assert_eq!(
        tool("git_status", json!({}), &dir).await,
        "working tree clean"
    );
    fs::write(
        &config,
        format!(
            "{}\n[status]\nshowUntrackedFiles = no\n",
            fs::read_to_string(&config).unwrap()
        ),
    )
    .unwrap();
    fs::write(dir.join("hidden-status.txt"), "still stage this").unwrap();
    assert_eq!(
        tool("git_status", json!({}), &dir).await,
        "working tree clean"
    );
    assert!(tool(
        "git_commit",
        json!({"message":"Stage hidden untracked file"}),
        &dir
    )
    .await
    .ends_with(" Stage hidden untracked file"));
    assert_eq!(
        tool("git_commit", json!({"message":"No change"}), &dir).await,
        "nothing to commit"
    );
}

#[tokio::test]
async fn git_tools_report_missing_and_unborn_repositories() {
    let dir = TempDir::new("no-git");
    let error = "exit code 128\nstderr:\nfatal: not a git repository (or any of the parent directories): .git";
    assert_eq!(tool("git_status", json!({}), &dir.path).await, error);
    assert_eq!(tool("git_log", json!({}), &dir.path).await, error);
    let path = dir.path.clone();
    tokio::task::spawn_blocking(move || {
        gix::init(path).unwrap();
    })
    .await
    .unwrap();
    assert_eq!(
        tool("git_status", json!({}), &dir.path).await,
        "working tree clean"
    );
    assert_eq!(
        tool("git_commit", json!({"message":"Empty"}), &dir.path).await,
        "nothing to commit"
    );
    assert!(tool("git_log", json!({}), &dir.path)
        .await
        .ends_with("does not have any commits yet"));
}

#[tokio::test]
async fn native_git_matches_git_when_installed() {
    use forge_server::{
        jsfmt::trim,
        tools::exec::{on_path, run},
    };
    if !tokio::task::spawn_blocking(|| on_path("git"))
        .await
        .unwrap()
    {
        eprintln!("skipped: git not installed");
        return;
    }
    let dir = TempDir::new("git-compare");
    fs::write(dir.path.join("README.md"), "Start").unwrap();
    vcs::init_repo(&dir.path).await.unwrap();
    fs::write(dir.path.join(".gitignore"), "ignored*\ncache/\n").unwrap();
    fs::write(dir.path.join("a.txt"), "original").unwrap();
    fs::write(dir.path.join("move.txt"), "move").unwrap();
    tool(
        "git_commit",
        json!({"message":"Add files\nsecond subject line\n\nBody"}),
        &dir.path,
    )
    .await;
    let project = dir.path.clone();
    tokio::task::spawn_blocking(move || {
        let repo = gix::open(project).unwrap();
        let mut tree = repo.head_tree().unwrap().edit().unwrap();
        let staged = repo.write_blob(b"staged").unwrap();
        let moved = repo.write_blob(b"move").unwrap();
        tree.upsert("a.txt", gix::objs::tree::EntryKind::Blob, staged)
            .unwrap();
        tree.remove("move.txt").unwrap();
        tree.upsert("renamed file.txt", gix::objs::tree::EntryKind::Blob, moved)
            .unwrap();
        let tree = tree.write().unwrap();
        repo.index_from_tree(&tree)
            .unwrap()
            .write(Default::default())
            .unwrap();
    })
    .await
    .unwrap();
    fs::write(dir.path.join("a.txt"), "unstaged").unwrap();
    fs::rename(dir.path.join("move.txt"), dir.path.join("renamed file.txt")).unwrap();
    fs::write(dir.path.join("ignored.txt"), "ignored").unwrap();
    fs::create_dir(dir.path.join("cache")).unwrap();
    fs::write(dir.path.join("cache/a"), "ignored").unwrap();
    fs::create_dir(dir.path.join("models")).unwrap();
    fs::write(dir.path.join("models/new.txt"), "new").unwrap();
    for path in [
        "space name.txt",
        "tab\tname.txt",
        "newline\nname.txt",
        "é.txt",
    ] {
        fs::write(dir.path.join(path), "new").unwrap();
    }
    for (name, args) in [
        ("git_status", vec!["status".into(), "--short".into()]),
        (
            "git_log",
            vec![
                "log".into(),
                "-20".into(),
                "--format=%h %ad %s".into(),
                "--date=short".into(),
            ],
        ),
    ] {
        let expected = run("git", &args, &dir.path, 5000, &[]).await;
        assert_eq!(expected.code, 0, "{}", expected.stderr);
        assert_eq!(
            tool(name, json!({}), &dir.path).await,
            trim(&expected.stdout),
            "{name}"
        );
    }
    let committed = tool(
        "git_commit",
        json!({"message":"Commit every change"}),
        &dir.path,
    )
    .await;
    let head = run(
        "git",
        &["log".into(), "-1".into(), "--format=%h %s".into()],
        &dir.path,
        5000,
        &[],
    )
    .await;
    assert_eq!(committed, format!("committed {}", trim(&head.stdout)));
    assert_eq!(
        tool("git_status", json!({}), &dir.path).await,
        "working tree clean"
    );
    let config = dir.path.join(".git/config");
    fs::write(
        &config,
        format!(
            "{}\n[core]\nquotePath = false\nabbrev = 9\n",
            fs::read_to_string(&config).unwrap()
        ),
    )
    .unwrap();
    fs::write(dir.path.join("é.txt"), "modified").unwrap();
    let expected = run(
        "git",
        &["status".into(), "--short".into()],
        &dir.path,
        5000,
        &[],
    )
    .await;
    assert_eq!(
        tool("git_status", json!({}), &dir.path).await,
        trim(&expected.stdout)
    );
    let expected = run(
        "git",
        &[
            "log".into(),
            "--format=%h %ad %s".into(),
            "--date=short".into(),
        ],
        &dir.path,
        5000,
        &[],
    )
    .await;
    assert_eq!(
        tool("git_log", json!({}), &dir.path).await,
        trim(&expected.stdout)
    );
}

#[cfg(unix)]
#[tokio::test]
async fn native_git_preserves_symlinks_and_refuses_external_directory_contents() {
    use std::{
        ffi::OsString,
        os::unix::{ffi::OsStringExt, fs::symlink},
    };
    let dir = TempDir::new("git-links");
    let outside = TempDir::new("git-outside");
    fs::write(outside.path.join("private.txt"), "outside data").unwrap();
    fs::write(dir.path.join("README.md"), "Start").unwrap();
    vcs::init_repo(&dir.path).await.unwrap();
    let name = OsString::from_vec(b"non-utf8-\xff.txt".to_vec());
    fs::write(dir.path.join(&name), "native path").unwrap();
    assert_eq!(
        tool("git_status", json!({}), &dir.path).await,
        "?? \"non-utf8-\\377.txt\""
    );
    symlink(outside.path.join("private.txt"), dir.path.join("link")).unwrap();
    tool("git_commit", json!({"message":"Store link"}), &dir.path).await;
    let project = dir.path.clone();
    let expected = outside
        .path
        .join("private.txt")
        .to_string_lossy()
        .into_owned();
    tokio::task::spawn_blocking(move || {
        let repo = gix::open(project).unwrap();
        let index = repo.index().unwrap();
        let link = index.entry_by_path("link".into()).unwrap();
        assert_eq!(link.mode, gix::index::entry::Mode::SYMLINK);
        assert_eq!(repo.find_blob(link.id).unwrap().data, expected.as_bytes());
    })
    .await
    .unwrap();
    fs::create_dir(dir.path.join("folder")).unwrap();
    fs::write(dir.path.join("folder/private.txt"), "project data").unwrap();
    tool("git_commit", json!({"message":"Store folder"}), &dir.path).await;
    let before = tool("git_log", json!({}), &dir.path).await;
    fs::remove_dir_all(dir.path.join("folder")).unwrap();
    symlink(&outside.path, dir.path.join("folder")).unwrap();
    let commit = vcs::tools()
        .into_iter()
        .find(|t| t.name == "git_commit")
        .unwrap();
    let error = (commit.run)(
        json!({"message":"Do not read outside"}),
        ToolContext::new(dir.path.clone(), dir.path.clone()),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "path \"folder/private.txt\" is outside the project"
    );
    assert_eq!(tool("git_log", json!({}), &dir.path).await, before);
    assert_eq!(
        fs::read_to_string(outside.path.join("private.txt")).unwrap(),
        "outside data"
    );
}
