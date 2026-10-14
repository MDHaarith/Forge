use super::{input_str, paths::project_path, Tool, ToolContext, ToolError, ToolFuture, ToolResult};
use crate::jsfmt::trim;
use gix::{
    bstr::{BStr, BString, ByteSlice},
    prelude::ObjectIdExt,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

fn git_error(error: impl std::fmt::Display) -> ToolError {
    ToolError::expected(format!("exit code 128\nstderr:\n{error}"))
}

fn open_repo(cwd: &Path) -> Result<gix::Repository, ToolError> {
    if !cwd.join(".git").exists() {
        return Err(git_error(
            "fatal: not a git repository (or any of the parent directories): .git",
        ));
    }
    let mut repo = gix::open(cwd).map_err(git_error)?;
    if repo.workdir() != Some(cwd) || !repo.path().starts_with(cwd) {
        return Err(ToolError::expected("git repository is outside the project"));
    }
    // shortcut: external filters and hooks are not run, add native equivalents if projects require them.
    let mut config = repo.config_snapshot_mut();
    let filters: Vec<_> = config
        .sections_and_ids_by_name("filter")
        .into_iter()
        .flatten()
        .map(|(_, id)| id)
        .collect();
    for id in filters {
        config.remove_section_by_id(id);
    }
    config.commit().map_err(git_error)?;
    Ok(repo)
}

pub fn tools() -> Vec<Tool> {
    vec![
        Tool::new(
            "vcs",
            "git_status",
            "Show which project files changed since the last commit.",
            json!({"type": "object", "additionalProperties": false, "properties": {}}),
            git_status,
        ),
        Tool::new("vcs", "git_commit", "Record every current change in the project's history with a message. Commit after each meaningful step so the user can review and roll back.", json!({"type": "object", "additionalProperties": false, "properties": {"message": {"type": "string", "description": "What changed and why, one line"}}, "required": ["message"]}), git_commit),
        Tool::new(
            "vcs",
            "git_log",
            "Show the most recent commits in the project.",
            json!({"type": "object", "additionalProperties": false, "properties": {"limit": {"type": "integer", "description": "How many commits (default 20)"}}}),
            git_log,
        ),
    ]
}

fn git_status(_input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let cwd = project_path(&ctx.project_dir, "")?;
        tokio::task::spawn_blocking(move || {
            Ok(ToolResult::text(
                status(&cwd).unwrap_or_else(|error| error.to_string()),
            ))
        })
        .await
        .map_err(|error| ToolError::unexpected(error.to_string()))?
    })
}

fn quoted_path(path: &BStr, quote_unicode: bool) -> String {
    let quote_unicode = quote_unicode || path.to_str().is_err();
    if !path
        .iter()
        .any(|&b| b <= b' ' || b == 127 || b == b'"' || b == b'\\' || (quote_unicode && b >= 128))
    {
        return path.to_str_lossy().into_owned();
    }
    let mut out = vec![b'"'];
    for &byte in path.iter() {
        match byte {
            b'"' | b'\\' => out.extend([b'\\', byte]),
            b'\n' => out.extend(b"\\n"),
            b'\r' => out.extend(b"\\r"),
            b'\t' => out.extend(b"\\t"),
            7 => out.extend(b"\\a"),
            8 => out.extend(b"\\b"),
            11 => out.extend(b"\\v"),
            12 => out.extend(b"\\f"),
            b if b < 32 || b == 127 || (quote_unicode && b >= 128) => {
                out.extend(format!("\\{b:03o}").bytes())
            }
            b => out.push(b),
        }
    }
    out.push(b'"');
    String::from_utf8_lossy(&out).into_owned()
}

fn status(cwd: &Path) -> Result<String, ToolError> {
    use gix::{
        diff::index::Change,
        status::{index_worktree::iter::Summary, Item},
    };
    let repo = open_repo(cwd)?;
    let quote_unicode = repo
        .config_snapshot()
        .boolean("core.quotePath")
        .unwrap_or(true);
    let mut tracked = BTreeMap::<BString, (char, char, Option<BString>)>::new();
    let mut untracked = BTreeMap::new();
    for item in repo
        .status(gix::progress::Discard)
        .map_err(git_error)?
        .index(worktree_index(&repo, cwd)?.into())
        .into_iter(Vec::<BString>::new())
        .map_err(git_error)?
    {
        let item = item.map_err(git_error)?;
        let location = item.location().to_owned();
        match item {
            Item::TreeIndex(change) => {
                let row = tracked.entry(location).or_insert((' ', ' ', None));
                row.0 = match change {
                    Change::Addition { .. } => 'A',
                    Change::Deletion { .. } => 'D',
                    Change::Modification {
                        previous_entry_mode,
                        entry_mode,
                        ..
                    } => {
                        if previous_entry_mode.bits() & 0o170000 != entry_mode.bits() & 0o170000 {
                            'T'
                        } else {
                            'M'
                        }
                    }
                    Change::Rewrite {
                        source_location,
                        copy,
                        ..
                    } => {
                        row.2 = Some(source_location.into_owned());
                        if copy {
                            'C'
                        } else {
                            'R'
                        }
                    }
                };
            }
            Item::IndexWorktree(change) => {
                let Some(summary) = change.summary() else {
                    continue;
                };
                if let gix::status::index_worktree::Item::DirectoryContents { entry, .. } = &change
                {
                    let mut location = location;
                    if matches!(
                        entry.disk_kind,
                        Some(gix::dir::entry::Kind::Directory | gix::dir::entry::Kind::Repository)
                    ) {
                        location.push(b'/');
                    }
                    untracked.insert(location, ());
                    continue;
                }
                let row = tracked.entry(location).or_insert((' ', ' ', None));
                if let gix::status::index_worktree::Item::Modification {
                    status:
                        gix::status::plumbing::index_as_worktree::EntryStatus::Conflict {
                            summary, ..
                        },
                    ..
                } = &change
                {
                    use gix::status::plumbing::index_as_worktree::Conflict;
                    (row.0, row.1) = match summary {
                        Conflict::BothDeleted => ('D', 'D'),
                        Conflict::AddedByUs => ('A', 'U'),
                        Conflict::DeletedByThem => ('U', 'D'),
                        Conflict::AddedByThem => ('U', 'A'),
                        Conflict::DeletedByUs => ('D', 'U'),
                        Conflict::BothAdded => ('A', 'A'),
                        Conflict::BothModified => ('U', 'U'),
                    };
                    continue;
                }
                row.1 = match summary {
                    Summary::Removed => 'D',
                    Summary::TypeChange => 'T',
                    Summary::Added | Summary::IntentToAdd => 'A',
                    Summary::Modified => 'M',
                    Summary::Renamed => 'R',
                    Summary::Copied => 'C',
                    Summary::Conflict => {
                        row.0 = 'U';
                        'U'
                    }
                };
            }
        }
    }
    let mut lines = Vec::new();
    for (path, (x, y, source)) in tracked {
        let path = quoted_path(path.as_ref(), quote_unicode);
        let path = match source {
            Some(source) => format!("{} -> {path}", quoted_path(source.as_ref(), quote_unicode)),
            None => path,
        };
        lines.push(format!("{x}{y} {path}"));
    }
    lines.extend(
        untracked
            .keys()
            .map(|p| format!("?? {}", quoted_path(p.as_ref(), quote_unicode))),
    );
    Ok(if lines.is_empty() {
        "working tree clean".into()
    } else {
        trim(&lines.join("\n")).into()
    })
}

fn git_commit(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let cwd = project_path(&ctx.project_dir, "")?;
        let message = input_str(&input, "message")?.to_owned();
        tokio::task::spawn_blocking(move || commit(&open_repo(&cwd)?, &cwd, &message))
            .await
            .map_err(|error| ToolError::unexpected(error.to_string()))?
    })
}

fn stage(repo: &gix::Repository, cwd: &Path) -> Result<gix::ObjectId, ToolError> {
    use gix::objs::tree::EntryKind;
    // shortcut: staging re-reads tracked files, reuse unchanged index blobs if large project commits become slow.
    let index = worktree_index(repo, cwd)?;
    let mut paths = BTreeMap::new();
    for entry in index.entries() {
        paths.insert(entry.path(&index).to_owned(), entry.mode);
    }
    let options = repo
        .dirwalk_options()
        .map_err(git_error)?
        .emit_untracked(gix::dir::walk::EmissionMode::Matching);
    for item in repo
        .dirwalk_iter(
            index.clone(),
            Vec::<BString>::new(),
            Default::default(),
            options,
        )
        .map_err(git_error)?
    {
        let entry = item.map_err(git_error)?.entry;
        if entry.status == gix::dir::entry::Status::Untracked {
            paths
                .entry(entry.rela_path)
                .or_insert(gix::index::entry::Mode::FILE);
        }
    }
    let mut tree = repo.empty_tree().edit().map_err(git_error)?;
    let (mut filters, _) = repo.filter_pipeline(None).map_err(git_error)?;
    for (relative, old_mode) in paths {
        let relative_path = gix::path::from_bstr(&relative);
        let path = project_path(cwd, &relative_path)?;
        if let Some(entry) = index.entry_by_path(relative.as_ref()).filter(|entry| {
            entry
                .flags
                .contains(gix::index::entry::Flags::SKIP_WORKTREE)
        }) {
            tree.upsert(
                &relative,
                entry
                    .mode
                    .to_tree_entry_mode()
                    .ok_or_else(|| git_error("invalid git index mode"))?
                    .kind(),
                entry.id,
            )
            .map_err(git_error)?;
            continue;
        }
        // Do not follow a tracked directory replaced by a symlink outside the project.
        let mut parent = path.parent();
        while let Some(dir) = parent.filter(|dir| *dir != cwd) {
            if std::fs::symlink_metadata(dir).is_ok_and(|m| m.is_symlink()) {
                return Err(ToolError::expected(format!(
                    "path \"{relative}\" is outside the project"
                )));
            }
            parent = dir.parent();
        }
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                continue
            }
            Err(error) => return Err(error.into()),
        };
        let (kind, id) = if metadata.is_symlink() {
            let target = std::fs::read_link(&path)?;
            (
                EntryKind::Link,
                repo.write_blob(gix::path::into_bstr(target).as_ref())
                    .map_err(git_error)?
                    .detach(),
            )
        } else if metadata.is_file() {
            let mut kind = EntryKind::Blob;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let executable = if repo.config_snapshot().boolean("core.filemode") == Some(false) {
                    old_mode == gix::index::entry::Mode::FILE_EXECUTABLE
                } else {
                    metadata.permissions().mode() & 0o111 != 0
                };
                if executable {
                    kind = EntryKind::BlobExecutable;
                }
            }
            let mut content = filters
                .convert_to_git(std::fs::File::open(&path)?, &relative_path, &index)
                .map_err(git_error)?;
            (
                kind,
                repo.write_blob_stream(&mut content)
                    .map_err(git_error)?
                    .detach(),
            )
        } else if metadata.is_dir() {
            if path.join(".git").exists() {
                let nested = gix::open(&path).map_err(git_error)?;
                (
                    EntryKind::Commit,
                    nested.head_id().map_err(git_error)?.detach(),
                )
            } else if old_mode == gix::index::entry::Mode::COMMIT {
                (
                    EntryKind::Commit,
                    index.entry_by_path(relative.as_ref()).unwrap().id,
                )
            } else {
                continue;
            }
        } else {
            continue;
        };
        tree.upsert(&relative, kind, id).map_err(git_error)?;
    }
    let id = tree.write().map_err(git_error)?.detach();
    let mut staged = repo.index_from_tree(&id).map_err(git_error)?;
    keep_skip_flags(&index, &mut staged);
    staged.write(Default::default()).map_err(git_error)?;
    Ok(id)
}

fn keep_skip_flags(old: &gix::index::State, new: &mut gix::index::File) {
    let mut files = BTreeSet::new();
    let mut directories = BTreeSet::new();
    for entry in old.entries().iter().filter(|entry| {
        entry
            .flags
            .contains(gix::index::entry::Flags::SKIP_WORKTREE)
    }) {
        if entry.mode == gix::index::entry::Mode::DIR {
            directories.insert(
                entry
                    .path(old)
                    .trim_end_with(|b| b == '/')
                    .as_bstr()
                    .to_owned(),
            );
        } else {
            files.insert(entry.path(old).to_owned());
        }
    }
    for (entry, path) in new.entries_mut_with_paths() {
        let skipped = files.contains(path)
            || path
                .iter()
                .enumerate()
                .any(|(i, b)| *b == b'/' && directories.contains(path[..i].as_bstr()));
        entry.flags.set(
            gix::index::entry::Flags::SKIP_WORKTREE | gix::index::entry::Flags::EXTENDED,
            skipped,
        );
    }
}

fn worktree_index(repo: &gix::Repository, cwd: &Path) -> Result<gix::index::File, ToolError> {
    let index = repo.index_or_empty().map_err(git_error)?;
    if !index.is_sparse() {
        return Ok((**index).clone());
    }
    // Expand compact sparse directories without checking their files out or losing their skip flags.
    let mut tree = repo.empty_tree().edit().map_err(git_error)?;
    for entry in index.entries() {
        let relative = entry.path(&index).trim_end_with(|b| b == '/').as_bstr();
        project_path(cwd, gix::path::from_bstr(relative))?;
        tree.upsert(
            relative,
            entry
                .mode
                .to_tree_entry_mode()
                .ok_or_else(|| git_error("invalid git index mode"))?
                .kind(),
            entry.id,
        )
        .map_err(git_error)?;
    }
    let tree = tree.write().map_err(git_error)?;
    let mut expanded = repo.index_from_tree(&tree).map_err(git_error)?;
    keep_skip_flags(&index, &mut expanded);
    Ok(expanded)
}

fn commit(repo: &gix::Repository, cwd: &Path, message: &str) -> Result<ToolResult, ToolError> {
    let tree = stage(repo, cwd)?;
    let parent = repo.head().map_err(git_error)?.id().map(|id| id.detach());
    let previous_tree = match parent {
        Some(parent) => repo
            .find_commit(parent)
            .map_err(git_error)?
            .tree_id()
            .map_err(git_error)?
            .detach(),
        None => repo.empty_tree().id,
    };
    if previous_tree == tree {
        return Ok(ToolResult::text("nothing to commit"));
    }
    let message = message
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    let message = trim(&message);
    if message.is_empty() {
        return Err(ToolError::expected(
            "exit code 1\nstderr:\nAborting commit due to empty commit message.",
        ));
    }
    let fallback = gix::actor::Signature {
        name: "Forge".into(),
        email: "forge@localhost".into(),
        time: gix::date::Time::now_local_or_utc(),
    };
    let author = repo
        .author()
        .transpose()
        .map_err(git_error)?
        .map(|s| s.to_owned())
        .transpose()
        .map_err(git_error)?
        .filter(|s| !s.name.trim().is_empty() && !s.email.trim().is_empty())
        .unwrap_or_else(|| fallback.clone());
    let committer = repo
        .committer()
        .transpose()
        .map_err(git_error)?
        .map(|s| s.to_owned())
        .transpose()
        .map_err(git_error)?
        .filter(|s| !s.name.trim().is_empty() && !s.email.trim().is_empty())
        .unwrap_or(fallback);
    let id = repo
        .commit_as(
            committer.to_ref(&mut Default::default()),
            author.to_ref(&mut Default::default()),
            "HEAD",
            format!("{message}\n"),
            tree,
            parent,
        )
        .map_err(git_error)?;
    let commit = repo.find_commit(id).map_err(git_error)?;
    Ok(ToolResult::text(format!(
        "committed {} {}",
        id.shorten_or_id(),
        commit.message().map_err(git_error)?.summary()
    )))
}

fn git_log(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let limit = input["limit"].as_f64().unwrap_or(20.).clamp(1., 200.) as usize;
        let cwd = project_path(&ctx.project_dir, "")?;
        tokio::task::spawn_blocking(move || {
            Ok(ToolResult::text(
                log(&cwd, limit).unwrap_or_else(|error| error.to_string()),
            ))
        })
        .await
        .map_err(|error| ToolError::unexpected(error.to_string()))?
    })
}

fn log(cwd: &Path, limit: usize) -> Result<String, ToolError> {
    let repo = open_repo(cwd)?;
    let head = repo.head().map_err(git_error)?;
    let Some(id) = head.id() else {
        let branch = head
            .referent_name()
            .map(|name| name.shorten().to_string())
            .unwrap_or_default();
        return Err(git_error(format!(
            "fatal: your current branch '{branch}' does not have any commits yet"
        )));
    };
    let mut lines = Vec::new();
    for info in repo
        .rev_walk([id])
        .sorting(gix::revision::walk::Sorting::ByCommitTime(
            Default::default(),
        ))
        .all()
        .map_err(git_error)?
        .take(limit)
    {
        let id = info.map_err(git_error)?.id.attach(&repo);
        let commit = repo.find_commit(id).map_err(git_error)?;
        let date = commit
            .author()
            .map_err(git_error)?
            .time()
            .map_err(git_error)?
            .format(gix::date::time::format::SHORT)
            .map_err(git_error)?;
        lines.push(format!(
            "{} {date} {}",
            id.shorten_or_id(),
            commit.message().map_err(git_error)?.summary()
        ));
    }
    Ok(if lines.is_empty() {
        "no commits yet".into()
    } else {
        trim(&lines.join("\n")).into()
    })
}

/// Creates the repository for a new project with an initial commit.
pub async fn init_repo(project_dir: &Path) -> Result<(), ToolError> {
    let cwd = project_path(project_dir, "")?;
    tokio::task::spawn_blocking(move || {
        gix::init(&cwd).map_err(git_error)?;
        commit(&open_repo(&cwd)?, &cwd, "Start project")?;
        Ok(())
    })
    .await
    .map_err(|error| ToolError::unexpected(error.to_string()))?
}
