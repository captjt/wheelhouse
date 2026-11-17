//! Where Wheelhouse keeps its state.

use std::path::PathBuf;

/// `$WH_STATE`, or `~/.local/state/wheelhouse`.
pub fn state_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("WH_STATE") {
        return PathBuf::from(dir);
    }
    home().join(".local/state/wheelhouse")
}

/// The hub's socket, for the CLI and the agents' hooks.
pub fn socket() -> PathBuf {
    if let Some(p) = std::env::var_os("WH_SOCKET") {
        return PathBuf::from(p);
    }
    state_dir().join("wh.sock")
}

pub fn db() -> PathBuf {
    state_dir().join("wheelhouse.db")
}

pub fn keys() -> PathBuf {
    state_dir().join("keys")
}

pub fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/".into()))
}

/// The home folder as `~`, for display.
pub fn tilde(path: &str) -> String {
    let home = home();
    let home = home.to_string_lossy();
    match path.strip_prefix(home.as_ref()) {
        Some("") => "~".to_string(),
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_string(),
    }
}

/// `~/x` as a path in the home folder; anything else as it is.
pub fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None if path == "~" => home(),
        None => PathBuf::from(path),
    }
}

/// A program on PATH, or in the usual places a LaunchAgent's PATH lacks.
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var("PATH").unwrap_or_default();
    path.split(':')
        .map(PathBuf::from)
        .chain(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from))
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}
