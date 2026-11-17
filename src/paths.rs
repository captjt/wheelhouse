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
