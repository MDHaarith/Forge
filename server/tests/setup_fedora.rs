#![cfg(unix)]

mod common;

use common::TempDir;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::Duration,
};

const FEDORA: &str = "NAME=\"Fedora Linux\"\nID=fedora\nVERSION_ID=43\n";
const UBUNTU: &str = "NAME=Ubuntu\nID=ubuntu\nID_LIKE=debian\nVERSION_ID=24.04\n";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned()
}

fn has_program(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

/// A fake machine: an os-release file, a home folder, and stub programs that log their arguments.
struct Sandbox {
    root: TempDir,
    home: PathBuf,
    bin: PathBuf,
}

impl Sandbox {
    fn new(os_release: &str) -> Option<Self> {
        if !has_program("bash") {
            eprintln!("skipped: bash not installed");
            return None;
        }
        let root = TempDir::new("setup");
        let bin = root.path.join("bin");
        let home = root.path.join("home");
        fs::create_dir(&bin).unwrap();
        fs::create_dir(&home).unwrap();
        fs::write(root.path.join("os-release"), os_release).unwrap();
        for name in [
            "dnf",
            "sudo",
            "flatpak",
            "systemctl",
            "npm",
            "cargo",
            "update-desktop-database",
            "xdg-open",
        ] {
            // sudo runs its command so `sudo dnf` is logged as dnf too.
            let body = if name == "sudo" {
                "echo \"sudo $*\" >> \"$HOME/../calls.log\"; exec \"$@\"".into()
            } else {
                format!("echo \"{name} $*\" >> \"$HOME/../calls.log\"")
            };
            let path = bin.join(name);
            fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        Some(Self { root, home, bin })
    }

    fn command(&self, program: &str) -> Command {
        let mut paths = vec![self.bin.clone()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        let mut command = Command::new(program);
        command
            .env("HOME", &self.home)
            .env("PATH", std::env::join_paths(paths).unwrap())
            .env("OS_RELEASE", self.root.path.join("os-release"))
            .env("NPM", self.bin.join("npm"));
        command
    }

    fn setup(&self, args: &[&str]) -> Output {
        self.command("bash")
            .arg(repo().join("scripts/setup-fedora.sh"))
            .args(args)
            .output()
            .unwrap()
    }

    fn calls(&self) -> String {
        fs::read_to_string(self.root.path.join("calls.log")).unwrap_or_default()
    }
}

fn succeeded(output: &Output) -> String {
    let stdout = String::from_utf8(output.stdout.clone()).unwrap();
    assert!(
        output.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    stdout
}

fn dnf_packages(stdout: &str) -> Vec<&str> {
    stdout
        .lines()
        .find(|line| line.contains("dnf install"))
        .unwrap()
        .split_whitespace()
        .collect()
}

#[test]
fn refuses_to_run_on_a_system_that_is_not_fedora() {
    let Some(sandbox) = Sandbox::new(UBUNTU) else {
        return;
    };
    let output = sandbox.setup(&[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("This script is for Fedora"));
    assert!(sandbox.calls().is_empty());
}

#[test]
fn a_dry_run_lists_the_dnf_packages_per_domain_and_changes_nothing() {
    let Some(sandbox) = Sandbox::new(FEDORA) else {
        return;
    };
    let stdout = succeeded(&sandbox.setup(&["--dry-run"]));
    let packages = dnf_packages(&stdout);
    for package in [
        "rust",
        "cargo",
        "nodejs",
        "nodejs-npm",
        "blender",
        "python3-numpy",
        "librsvg2-tools",
        "flatpak",
        "kicad",
    ] {
        assert!(
            packages.contains(&package),
            "{package} missing from: {stdout}"
        );
    }
    assert!(!stdout.contains("flatpak install"));
    assert!(stdout.contains("cargo build --release -p forge-server --manifest-path"));

    let pcb = succeeded(&sandbox.setup(&["--dry-run", "--only", "pcb"]));
    let packages = dnf_packages(&pcb);
    assert!(
        packages.contains(&"kicad")
            && !packages.contains(&"blender")
            && !packages.contains(&"git")
    );
    assert!(!pcb.contains("flatpak install"));
    assert_eq!(fs::read_dir(&sandbox.home).unwrap().count(), 0);
    assert!(sandbox.calls().is_empty());
    assert_eq!(sandbox.setup(&["--only", "cnc"]).status.code(), Some(2));
}

#[test]
fn rf_uses_the_built_in_solver_without_extra_packages_or_downloads() {
    let Some(sandbox) = Sandbox::new(FEDORA) else { return; };
    let stdout = succeeded(&sandbox.setup(&["--dry-run", "--only", "rf"]));
    let packages = dnf_packages(&stdout);
    assert!(!packages.contains(&"git") && !packages.contains(&"python3-numpy"));
    assert!(!stdout.contains("git clone") && !stdout.contains("vtk-devel"));
    assert!(stdout.contains("cargo build --release -p forge-server"));
    assert_eq!(sandbox.setup(&["--with-rf"]).status.code(), Some(2));
    assert!(sandbox.calls().is_empty());
}

#[test]
fn desktop_adds_tauri_packages_builds_and_installs_the_rpm_and_skips_the_browser_launcher() {
    let Some(sandbox) = Sandbox::new(FEDORA) else {
        return;
    };
    let stdout = succeeded(&sandbox.setup(&["--dry-run", "--desktop", "--only", "pcb"]));
    let packages = dnf_packages(&stdout);
    for package in [
        "gcc",
        "rust",
        "cargo",
        "webkit2gtk4.1-devel",
        "openssl-devel",
        "librsvg2-devel",
        "libxdo-devel",
    ] {
        assert!(packages.contains(&package), "{package}");
    }
    assert!(stdout.contains(&format!(
        "npm --prefix {} run desktop:build",
        repo().display()
    )));
    let package: serde_json::Value =
        serde_json::from_slice(&fs::read(repo().join("package.json")).unwrap()).unwrap();
    let arch = String::from_utf8(Command::new("uname").arg("-m").output().unwrap().stdout).unwrap();
    assert!(stdout.contains(&format!(
        "dnf install -y {}/target/release/bundle/rpm/Forge-{}-1.{}.rpm",
        repo().display(),
        package["version"].as_str().unwrap(),
        arch.trim()
    )));
    assert!(!stdout.contains("applications/forge.desktop"));
    let normal = succeeded(&sandbox.setup(&["--dry-run"]));
    assert!(!normal.contains("webkit2gtk") && !normal.contains("desktop:build"));
}

#[tokio::test]
async fn a_real_run_installs_packages_the_service_and_a_working_forge_command() {
    let Some(sandbox) = Sandbox::new(FEDORA) else {
        return;
    };
    if !has_program("curl") {
        eprintln!("skipped: curl not installed");
        return;
    }
    succeeded(&sandbox.setup(&[]));
    let calls = sandbox.calls();
    let packages = dnf_packages(&calls);
    for package in [
        "rust",
        "cargo",
        "nodejs",
        "nodejs-npm",
        "blender",
        "kicad",
    ] {
        assert!(packages.contains(&package), "{package}: {calls}");
    }
    assert!(!calls.lines().any(|line| line.starts_with("flatpak ")));
    for args in ["ci --no-audit --no-fund", "run build"] {
        assert!(calls
            .lines()
            .any(|line| line == format!("npm --prefix {} {args}", repo().display())));
    }
    assert!(calls.lines().any(|line| line
        == format!(
            "cargo build --release -p forge-server --manifest-path {}/Cargo.toml",
            repo().display()
        )));
    assert!(calls
        .lines()
        .any(|line| line == "systemctl --user daemon-reload"));

    let env_file = sandbox.home.join(".config/forge/env");
    assert_eq!(
        fs::metadata(&env_file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let settings = fs::read_to_string(&env_file).unwrap();
    assert!(settings.contains("# FORGE_CLAUDE=") && !settings.contains("ANTHROPIC_API_KEY"));
    let service =
        fs::read_to_string(sandbox.home.join(".config/systemd/user/forge.service")).unwrap();
    for expected in [
        format!("ExecStart={}/target/release/forge-server", repo().display()),
        format!("Environment=FORGE_APP_DIR={}", repo().display()),
        "Environment=PATH=%h/.local/bin:/usr/local/bin:/usr/bin".into(),
    ] {
        assert!(service.lines().any(|line| line == expected), "{service}");
    }
    let desktop =
        fs::read_to_string(sandbox.home.join(".local/share/applications/forge.desktop")).unwrap();
    assert!(desktop
        .lines()
        .any(|line| line == format!("Exec={}/.local/bin/forge", sandbox.home.display())));

    // Running setup again keeps the user's settings.
    fs::write(&env_file, "OPENAI_API_KEY=kept\n").unwrap();
    succeeded(&sandbox.setup(&["--only", "pcb"]));
    assert_eq!(
        fs::read_to_string(&env_file).unwrap(),
        "OPENAI_API_KEY=kept\n"
    );

    // The generated forge command starts the service, waits for Forge to answer, and opens the browser.
    let mut settings = forge_server::Settings::new(sandbox.root.path.join("projects"), repo());
    settings.offline = true;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(forge_server::serve_with_settings(listener, settings));
    fs::write(&env_file, format!("PORT={port}\n")).unwrap();
    // Async spawn: a blocking Command::output would starve this process's server.
    let mut command = sandbox.command("sh");
    command.arg(sandbox.home.join(".local/bin/forge"));
    let output = tokio::time::timeout(
        Duration::from_secs(60),
        tokio::process::Command::from(command)
            .kill_on_drop(true)
            .output(),
    )
    .await;
    server.abort();
    succeeded(&output.unwrap().unwrap());
    let calls = sandbox.calls();
    assert!(calls
        .lines()
        .any(|line| line == "systemctl --user start forge.service"));
    assert!(calls
        .lines()
        .any(|line| line == format!("xdg-open http://127.0.0.1:{port}/")));
}
