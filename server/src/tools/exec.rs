use crate::jsfmt::trim;
use std::{
    collections::HashMap,
    path::Path,
    process::Stdio,
    sync::{Mutex, OnceLock},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};

#[derive(Clone, Debug)]
pub struct ExecResult {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

const MAX_BUFFER: usize = 32 * 1024 * 1024;

async fn read_output(mut stream: impl AsyncRead + Unpin, out: &mut Vec<u8>) -> std::io::Result<()> {
    let mut buf = [0u8; 8192];
    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }
        let left = MAX_BUFFER - out.len();
        out.extend_from_slice(&buf[..n.min(left)]);
        if n > left {
            return Err(std::io::Error::other("maxBuffer length exceeded"));
        }
    }
}

/// Runs a program without a shell. Never rejects: a failing exit code is part of the result.
pub async fn run(
    cmd: &str,
    args: &[String],
    cwd: &Path,
    timeout_ms: u64,
    env: &[(String, String)],
) -> ExecResult {
    execute(cmd, args, cwd, timeout_ms, env, None).await
}

/// The PCB engine inherits no credentials, and has its own timeout message.
pub async fn run_isolated(
    cmd: &str,
    args: &[String],
    cwd: &Path,
    timeout_ms: u64,
    env: &[(String, String)],
    timeout_message: &str,
) -> ExecResult {
    execute(cmd, args, cwd, timeout_ms, env, Some(timeout_message)).await
}

async fn execute(
    cmd: &str,
    args: &[String],
    cwd: &Path,
    timeout_ms: u64,
    env: &[(String, String)],
    isolated_timeout: Option<&str>,
) -> ExecResult {
    let mut command = Command::new(cmd);
    if isolated_timeout.is_some() {
        command.env_clear();
    }
    command
        .args(args)
        .current_dir(cwd)
        .envs(env.iter().cloned())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return ExecResult {
                code: 1,
                stdout: String::new(),
                stderr: error.to_string(),
            }
        }
    };
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let result = tokio::time::timeout(Duration::from_millis(timeout_ms), async {
        tokio::try_join!(
            child.wait(),
            read_output(stdout, &mut out),
            read_output(stderr, &mut err)
        )
    })
    .await;
    let timed_out = result.is_err();
    let (code, failure) = match result {
        Ok(Ok((status, _, _))) => (status.code().unwrap_or(1), None),
        Ok(Err(error)) => (1, Some(error.to_string())),
        Err(_) => (
            1,
            Some(format!(
                "Command failed: {cmd} {} (timed out)",
                args.join(" ")
            )),
        ),
    };
    if failure.is_some() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    let stdout = String::from_utf8_lossy(&out).into_owned();
    let mut stderr = String::from_utf8_lossy(&err).into_owned();
    if timed_out {
        if let Some(message) = isolated_timeout {
            stderr = message.into();
        }
    }
    if code != 0 && stderr.is_empty() {
        stderr = failure.unwrap_or_else(|| format!("Command failed: {cmd} {}\n", args.join(" ")));
    }
    ExecResult {
        code,
        stdout,
        stderr,
    }
}

/// True when `program` is on PATH, or is a path to an existing file. Cached for the life of the process.
pub fn on_path(program: &str) -> bool {
    static FOUND: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    let found = FOUND.get_or_init(Default::default);
    if let Some(value) = found.lock().unwrap().get(program).copied() {
        return value;
    }
    let value = if program.contains(['/', '\\']) {
        Path::new(program).exists()
    } else {
        std::env::var_os("PATH").is_some_and(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                let path = dir.join(program);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    path.metadata()
                        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                }
                #[cfg(not(unix))]
                {
                    path.is_file()
                        || std::env::var("PATHEXT")
                            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into())
                            .split(';')
                            .any(|ext| dir.join(format!("{program}{ext}")).is_file())
                }
            })
        })
    };
    found.lock().unwrap().insert(program.into(), value);
    value
}

/// The first of `candidates` that can be run, or None. Lets one tool accept several program names.
pub fn find_program(candidates: &[String]) -> Option<String> {
    candidates.iter().find(|p| on_path(p)).cloned()
}

/// Trims long tool output so one noisy command can't flood the agent's context.
pub fn clip(text: &str, max: usize) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    if units.len() <= max {
        return text.to_owned();
    }
    let half = max / 2;
    format!(
        "{}\n... ({} characters cut) ...\n{}",
        String::from_utf16_lossy(&units[..half]),
        units.len() - max,
        String::from_utf16_lossy(&units[units.len() - half..])
    )
}

pub fn report(r: &ExecResult) -> String {
    let mut parts = vec![format!("exit code {}", r.code)];
    if !trim(&r.stdout).is_empty() {
        parts.push(format!("stdout:\n{}", trim(&r.stdout)));
    }
    if !trim(&r.stderr).is_empty() {
        parts.push(format!("stderr:\n{}", trim(&r.stderr)));
    }
    clip(&parts.join("\n"), 12_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn process_failures_output_env_and_timeout() {
        let missing = run(
            "forge-missing-program-12345",
            &[],
            Path::new("/"),
            1000,
            &[],
        )
        .await;
        assert_eq!(missing.code, 1);
        assert!(!missing.stderr.is_empty());
        let args = vec![
            "-c".into(),
            "printf '%s' \"$FORGE_EXEC_TEST\"; printf 'err' >&2; exit 7".into(),
        ];
        let r = run(
            "sh",
            &args,
            Path::new("/"),
            1000,
            &[("FORGE_EXEC_TEST".into(), "merged".into())],
        )
        .await;
        assert_eq!(r.code, 7);
        assert_eq!(r.stdout, "merged");
        assert_eq!(r.stderr, "err");
        let r = run(
            "sh",
            &["-c".into(), "exec sleep 10".into()],
            Path::new("/"),
            30,
            &[],
        )
        .await;
        assert_eq!(r.code, 1);
        assert!(r.stderr.contains("timed out"));
    }
    #[tokio::test]
    async fn pipes_drain_concurrently_and_cap_output() {
        let r = run(
            "sh",
            &[
                "-c".into(),
                "head -c 100000 /dev/zero; head -c 100000 /dev/zero >&2".into(),
            ],
            Path::new("/"),
            2000,
            &[],
        )
        .await;
        assert_eq!(r.code, 0);
        assert_eq!(r.stdout.len(), 100000);
        assert_eq!(r.stderr.len(), 100000);
        let r = run(
            "head",
            &[
                "-c".into(),
                (MAX_BUFFER + 1).to_string(),
                "/dev/zero".into(),
            ],
            Path::new("/"),
            5000,
            &[],
        )
        .await;
        assert_eq!(r.code, 1);
        assert_eq!(r.stdout.len(), MAX_BUFFER);
        assert!(r.stderr.contains("maxBuffer"));
    }
    #[test]
    fn clips_and_reports() {
        assert_eq!(
            clip("abcdefghij", 6),
            "abc\n... (4 characters cut) ...\nhij"
        );
        assert_eq!(
            report(&ExecResult {
                code: 0,
                stdout: " hi \n".into(),
                stderr: "".into()
            }),
            "exit code 0\nstdout:\nhi"
        );
    }
}
