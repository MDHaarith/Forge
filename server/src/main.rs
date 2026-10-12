use forge_server::jsfmt::trim;
use forge_server::{
    env,
    http::{router, AppState},
    model::Model,
    providers::provider_label,
    Settings,
};
use std::{
    io::{self, Read},
    path::PathBuf,
};

fn app_dir() -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os("FORGE_APP_DIR") {
        return Ok(PathBuf::from(path));
    }
    if cfg!(debug_assertions) {
        return Ok(PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")));
    }
    Ok(std::env::current_exe()?
        .parent()
        .ok_or_else(|| io::Error::other("executable has no folder"))?
        .to_owned())
}

fn parse_port(raw: &str) -> io::Result<u16> {
    let raw = trim(raw);
    let numeric = if raw.is_empty() {
        Some(0.0)
    } else if let Some(digits) = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
        u64::from_str_radix(digits, 16)
            .ok()
            .filter(|_| !digits.starts_with('+'))
            .map(|n| n as f64)
    } else if let Some(digits) = raw.strip_prefix("0o").or_else(|| raw.strip_prefix("0O")) {
        u64::from_str_radix(digits, 8)
            .ok()
            .filter(|_| !digits.starts_with('+'))
            .map(|n| n as f64)
    } else if let Some(digits) = raw.strip_prefix("0b").or_else(|| raw.strip_prefix("0B")) {
        u64::from_str_radix(digits, 2)
            .ok()
            .filter(|_| !digits.starts_with('+'))
            .map(|n| n as f64)
    } else {
        raw.parse::<f64>().ok()
    };
    numeric
        .filter(|n| n.is_finite() && n.fract() == 0.0 && (0.0..=65535.0).contains(n))
        .map(|n| n as u16)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("invalid port: {raw}")))
}

#[tokio::main]
async fn main() -> io::Result<()> {
    env::load_env_file()?;
    let settings = Settings::from_env(app_dir()?);
    let port = parse_port(&std::env::var("PORT").unwrap_or_else(|_| "4317".into()))?;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    // The desktop app holds our stdin open; when it closes (the app quit or crashed), stop too.
    if std::env::var("FORGE_EXIT_WITH_STDIN").as_deref() == Ok("1") {
        std::thread::spawn(|| {
            let mut input = io::stdin().lock();
            let mut bytes = [0u8; 8192];
            loop {
                match input.read(&mut bytes) {
                    Ok(0) | Err(_) => std::process::exit(0),
                    Ok(_) => {}
                }
            }
        });
    }
    let state = AppState::from_settings(settings);
    println!(
        "Forge is running at http://127.0.0.1:{}",
        listener.local_addr()?.port()
    );
    println!(
        "Model: {} ({})",
        state.model.name(),
        provider_label(&state.model.provider())
    );
    println!("Projects: {}", state.projects.home.display());
    axum::serve(listener, router(state)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ports_accept_javascript_number_forms_and_refuse_invalid_ports() {
        for (text, expected) in [
            ("0", 0),
            ("", 0),
            (" 4317 ", 4317),
            ("4317.0", 4317),
            ("1e3", 1000),
            ("0x10", 16),
            ("0o10", 8),
            ("0b10", 2),
        ] {
            assert_eq!(parse_port(text).unwrap(), expected);
        }
        for text in [
            "-1", "65536", "1.5", "NaN", "Infinity", "abc", "0x+10", "0o+10", "0b+10",
        ] {
            assert!(parse_port(text).is_err());
        }
    }
}
