use super::ToolError;
use std::path::{Component, Path, PathBuf};

pub fn lexical_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir().unwrap().join(path)
    };
    let mut out = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            component => out.push(component.as_os_str()),
        }
    }
    out
}

/// Resolves a project-relative path, refusing anything that escapes the project or enters .git.
pub fn project_path(project_dir: &Path, relative: impl AsRef<Path>) -> Result<PathBuf, ToolError> {
    let relative = relative.as_ref();
    let relative_text = relative.display();
    let root = lexical_path(project_dir);
    let full = lexical_path(&root.join(relative));
    let rel = full
        .strip_prefix(&root)
        .map_err(|_| ToolError::expected(format!("path \"{relative_text}\" is outside the project")))?;
    // path.relative().startsWith("..") also refuses names such as "..hidden" in the TS server.
    if rel.to_string_lossy().starts_with("..") {
        return Err(ToolError::expected(format!(
            "path \"{relative_text}\" is outside the project"
        )));
    }
    if rel
        .components()
        .next()
        .is_some_and(|c| c.as_os_str() == ".git")
    {
        return Err(ToolError::expected(
            "the .git folder is managed by the git tools",
        ));
    }
    Ok(full)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_paths_stay_inside_and_avoid_git() {
        assert_eq!(
            project_path(Path::new("/p"), "a/b.txt").unwrap(),
            Path::new("/p/a/b.txt")
        );
        for path in ["../etc/passwd", "/etc/passwd", "a/../../x", "..hidden"] {
            assert_eq!(
                project_path(Path::new("/p"), path).unwrap_err().to_string(),
                format!("path \"{path}\" is outside the project")
            );
        }
        for path in [".git", ".git/config", "a/../.git/config"] {
            assert_eq!(
                project_path(Path::new("/p"), path).unwrap_err().to_string(),
                "the .git folder is managed by the git tools"
            );
        }
        assert_eq!(
            project_path(Path::new("/p"), ".github/x").unwrap(),
            Path::new("/p/.github/x")
        );
        assert_eq!(
            project_path(Path::new("/p"), "a/../new.txt").unwrap(),
            Path::new("/p/new.txt")
        );
        assert_eq!(project_path(Path::new("/p"), "").unwrap(), Path::new("/p"));
    }
}
