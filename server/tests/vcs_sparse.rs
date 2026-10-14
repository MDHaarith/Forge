mod common;

use common::TempDir;
use forge_server::tools::{vcs, ToolContext};
use serde_json::json;
use std::fs;

#[tokio::test]
async fn commits_preserve_hidden_sparse_files_and_uninitialized_submodules() {
    let dir = TempDir::new("git-sparse");
    fs::write(dir.path.join("README.md"), "Start").unwrap();
    fs::create_dir(dir.path.join("hidden")).unwrap();
    fs::write(dir.path.join("hidden/keep.txt"), "keep this data").unwrap();
    vcs::init_repo(&dir.path).await.unwrap();
    let tools = vcs::tools();
    let commit = tools.iter().find(|tool| tool.name == "git_commit").unwrap();
    let status = tools.iter().find(|tool| tool.name == "git_status").unwrap();
    let ctx = ToolContext::new(dir.path.clone(), dir.path.clone());
    fs::remove_dir_all(dir.path.join("hidden")).unwrap();

    for compact in [false, true] {
        let project = dir.path.clone();
        tokio::task::spawn_blocking(move || {
            let repo = gix::open(&project).unwrap();
            let tree = repo.head_tree().unwrap();
            let mut index = repo.index_from_tree(&tree.id).unwrap();
            if compact {
                let hidden = tree.find_entry("hidden").unwrap().object_id();
                index.remove_entries(|_, path, _| path.starts_with(b"hidden/"));
                index.dangerously_push_entry(
                    Default::default(),
                    hidden,
                    gix::index::entry::Flags::SKIP_WORKTREE | gix::index::entry::Flags::EXTENDED,
                    gix::index::entry::Mode::DIR,
                    "hidden/".into(),
                );
                index.sort_entries();
            } else {
                for (entry, path) in index.entries_mut_with_paths() {
                    if path.starts_with(b"hidden/") {
                        entry.flags.insert(
                            gix::index::entry::Flags::SKIP_WORKTREE
                                | gix::index::entry::Flags::EXTENDED,
                        );
                    }
                }
            }
            index.write(Default::default()).unwrap();
            if compact {
                // The low-level fixture constructor does not set State's sparse marker; write its mandatory extension.
                let path = project.join(".git/index");
                let mut bytes = fs::read(&path).unwrap();
                bytes.truncate(bytes.len() - 20);
                bytes.extend_from_slice(b"sdir\0\0\0\0");
                let mut hash = gix::hash::hasher(gix::hash::Kind::Sha1);
                hash.update(&bytes);
                bytes.extend_from_slice(hash.try_finalize().unwrap().as_slice());
                fs::write(path, bytes).unwrap();
                assert!(gix::open(&project).unwrap().index().unwrap().is_sparse());
            }
        })
        .await
        .unwrap();
        assert_eq!(
            (status.run)(json!({}), ctx.clone()).await.unwrap().text,
            "working tree clean"
        );
        fs::write(dir.path.join("README.md"), format!("changed {compact}")).unwrap();
        (commit.run)(json!({"message":"Change visible file"}), ctx.clone())
            .await
            .unwrap();
        assert_eq!(
            (status.run)(json!({}), ctx.clone()).await.unwrap().text,
            "working tree clean"
        );
        let project = dir.path.clone();
        tokio::task::spawn_blocking(move || {
            let repo = gix::open(&project).unwrap();
            let index = repo.index().unwrap();
            let entry = index.entry_by_path("hidden/keep.txt".into()).unwrap();
            assert!(entry
                .flags
                .contains(gix::index::entry::Flags::SKIP_WORKTREE));
            assert_eq!(repo.find_blob(entry.id).unwrap().data, b"keep this data");
            assert!(repo
                .head_tree()
                .unwrap()
                .lookup_entry_by_path("hidden/keep.txt")
                .unwrap()
                .is_some());
            assert!(!project.join("hidden").exists());
        })
        .await
        .unwrap();
    }

    let project = dir.path.clone();
    tokio::task::spawn_blocking(move || {
        let repo = gix::open(&project).unwrap();
        let mut index = (**repo.index().unwrap()).clone();
        index.dangerously_push_entry(
            Default::default(),
            repo.head_id().unwrap().detach(),
            gix::index::entry::Flags::empty(),
            gix::index::entry::Mode::COMMIT,
            "submodule".into(),
        );
        index.sort_entries();
        index.write(Default::default()).unwrap();
        fs::create_dir(project.join("submodule")).unwrap();
    })
    .await
    .unwrap();
    (commit.run)(json!({"message":"Keep uninitialized submodule"}), ctx)
        .await
        .unwrap();
    let project = dir.path.clone();
    tokio::task::spawn_blocking(move || {
        let repo = gix::open(project).unwrap();
        assert!(repo
            .head_tree()
            .unwrap()
            .find_entry("submodule")
            .unwrap()
            .mode()
            .is_commit());
    })
    .await
    .unwrap();
}
