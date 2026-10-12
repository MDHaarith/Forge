use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Fedora is the primary platform; the others get best-effort install hints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Distro {
    Fedora,
    Debian,
    Macos,
    Other,
}

/// How to install a program on each platform. `flatpak` is a Flathub app ID.
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct InstallHint {
    pub dnf: Option<String>,
    pub flatpak: Option<String>,
    pub apt: Option<String>,
    pub brew: Option<String>,
    pub url: Option<String>,
}

/// Reads /etc/os-release style text. Fedora relatives (RHEL, CentOS Stream, Nobara, Ultramarine) count as Fedora.
pub fn detect_distro(os_release: Option<&str>, platform: &str) -> Distro {
    if platform == "darwin" || platform == "macos" {
        return Distro::Macos;
    }
    let file;
    let text = match os_release {
        Some(text) => text,
        None => {
            file = match std::fs::read_to_string("/etc/os-release") {
                Ok(v) => v,
                Err(_) => return Distro::Other,
            };
            &file
        }
    };
    let field = |key: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(&format!("{key}=")))
            .unwrap_or("")
            .trim_start_matches('"')
            .split('"')
            .next()
            .unwrap_or("")
            .to_lowercase()
    };
    let ids = format!("{} {}", field("ID"), field("ID_LIKE"));
    if ids
        .split_whitespace()
        .any(|v| matches!(v, "fedora" | "rhel" | "centos"))
    {
        Distro::Fedora
    } else if ids
        .split_whitespace()
        .any(|v| matches!(v, "debian" | "ubuntu"))
    {
        Distro::Debian
    } else {
        Distro::Other
    }
}

pub fn current_distro() -> Distro {
    static CURRENT: OnceLock<Distro> = OnceLock::new();
    *CURRENT.get_or_init(|| detect_distro(None, std::env::consts::OS))
}

/// The one command a user on `distro` should run, falling back to the download page.
pub fn install_text(hint: Option<&InstallHint>, distro: Distro) -> Option<String> {
    let hint = hint?;
    let flatpak = || {
        hint.flatpak
            .as_ref()
            .filter(|s| !s.is_empty())
            .map(|s| format!("flatpak install flathub {s}"))
    };
    let choice = match distro {
        Distro::Fedora => hint
            .dnf
            .as_ref()
            .filter(|s| !s.is_empty())
            .map(|s| format!("sudo dnf install {s}"))
            .or_else(flatpak),
        Distro::Debian => hint
            .apt
            .as_ref()
            .filter(|s| !s.is_empty())
            .map(|s| format!("sudo apt install {s}"))
            .or_else(flatpak),
        Distro::Macos => hint
            .brew
            .as_ref()
            .filter(|s| !s.is_empty())
            .map(|s| format!("brew install {s}")),
        Distro::Other => flatpak(),
    };
    choice.or_else(|| hint.url.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_families_and_macos() {
        assert_eq!(
            detect_distro(
                Some("NAME=\"Fedora Linux\"\nID=fedora\nVERSION_ID=43\n"),
                "linux"
            ),
            Distro::Fedora
        );
        assert_eq!(
            detect_distro(Some("ID=nobara\nID_LIKE=\"rhel centos fedora\"\n"), "linux"),
            Distro::Fedora
        );
        assert_eq!(
            detect_distro(Some("ID=ubuntu\nID_LIKE=debian\n"), "linux"),
            Distro::Debian
        );
        assert_eq!(detect_distro(Some("ID=arch\n"), "linux"), Distro::Other);
        assert_eq!(detect_distro(Some(""), "darwin"), Distro::Macos);
    }
    #[test]
    fn install_hints_prefer_native_then_flathub() {
        let blender: InstallHint = serde_json::from_value(serde_json::json!({"dnf":"blender python3-numpy", "apt":"blender", "flatpak":"org.blender.Blender", "url":"https://blender.org"})).unwrap();
        let freecad: InstallHint = serde_json::from_value(serde_json::json!({"flatpak":"org.freecad.FreeCAD", "apt":"freecad", "url":"https://freecad.org"})).unwrap();
        assert_eq!(
            install_text(Some(&blender), Distro::Fedora).as_deref(),
            Some("sudo dnf install blender python3-numpy")
        );
        assert_eq!(
            install_text(Some(&freecad), Distro::Fedora).as_deref(),
            Some("flatpak install flathub org.freecad.FreeCAD")
        );
        assert_eq!(
            install_text(Some(&freecad), Distro::Debian).as_deref(),
            Some("sudo apt install freecad")
        );
        let url = InstallHint {
            url: Some("https://x".into()),
            ..Default::default()
        };
        assert_eq!(
            install_text(Some(&url), Distro::Macos).as_deref(),
            Some("https://x")
        );
        assert_eq!(install_text(None, Distro::Fedora), None);
    }
}
