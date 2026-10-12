use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Loads ~/.config/forge/env (or $FORGE_ENV_FILE) when the server is started, so the API key and
/// FORGE_* settings live in one private file instead of shell profiles or the desktop launcher.
/// Variables already set in the environment win. Loaded before other modules read settings.
pub fn load_env_file() -> io::Result<()> {
    let file = std::env::var_os("FORGE_ENV_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".config/forge/env"));
    load_file(&file)
}

pub fn load_file(file: &Path) -> io::Result<()> {
    let contents = match fs::read_to_string(file) {
        Ok(contents) => contents,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for (key, value) in parse_env(&contents) {
        if std::env::var_os(&key).is_none() {
            std::env::set_var(key, value);
        }
    }
    Ok(())
}

pub fn parse_env(contents: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut lines = contents.lines();
    while let Some(line) = lines.next() {
        let line = line
            .trim()
            .strip_prefix("export ")
            .unwrap_or(line.trim())
            .trim();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty()
            || !key
                .bytes()
                .enumerate()
                .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit()))
        {
            continue;
        }
        let value = value.trim_start();
        let value = if value.starts_with(['\'', '"', '`']) {
            let quote = value.chars().next().unwrap();
            let mut quoted = value[1..].to_owned();
            while !quoted.contains(quote) {
                let Some(next) = lines.next() else { break };
                quoted.push('\n');
                quoted.push_str(next);
            }
            let value = quoted.split(quote).next().unwrap_or("").to_owned();
            if quote == '"' {
                value.replace("\\n", "\n").replace("\\r", "\r")
            } else {
                value
            }
        } else {
            value.split('#').next().unwrap_or("").trim().to_owned()
        };
        out.push((key.to_owned(), value));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_private_env_file() {
        assert_eq!(parse_env("# key\nexport KEY = \"a#b\" # comment\nEMPTY=\nPLAIN=abc # tail\nMULTI='one\ntwo'\nNEWLINE=\"x\\ny\"\nBAD-KEY=no"),
            vec![("KEY".into(), "a#b".into()), ("EMPTY".into(), "".into()), ("PLAIN".into(), "abc".into()),
                ("MULTI".into(), "one\ntwo".into()), ("NEWLINE".into(), "x\ny".into())]);
    }
}
