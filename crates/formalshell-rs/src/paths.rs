//! Where the shell keeps and finds files. Every XDG lookup goes through
//! here so the fallbacks agree: an unset or empty variable means the
//! default under `$HOME`, and a runtime dir that is missing falls back to a
//! per-user directory under `/tmp`.
//!
//! Also holds the names another program reads or writes: `install.rs`
//! writes them into the login script and the PAM file, the shell reads them.

use std::path::PathBuf;

/// The shell's own directory under the runtime dir.
pub const SHELL_DIR: &str = "formalshell";

/// Left by the login script before the compositor starts; the shell raises
/// the boot lock when it finds it.
pub const LOGIN_LOCK_MARKER: &str = "login-lock";

/// The XDG base directory spec treats a relative value as unset.
fn var(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from).filter(|p| p.is_absolute())
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

fn base(env: &str, under_home: &str) -> PathBuf {
    var(env).unwrap_or_else(|| home().join(under_home))
}

/// `$XDG_STATE_HOME`, else `~/.local/state`.
pub fn state() -> PathBuf {
    base("XDG_STATE_HOME", ".local/state")
}

/// `$XDG_CONFIG_HOME`, else `~/.config`.
pub fn config() -> PathBuf {
    base("XDG_CONFIG_HOME", ".config")
}

/// `$XDG_CACHE_HOME`, else `~/.cache`.
pub fn cache() -> PathBuf {
    base("XDG_CACHE_HOME", ".cache")
}

/// `$XDG_DATA_HOME`, else `~/.local/share`.
pub fn data() -> PathBuf {
    base("XDG_DATA_HOME", ".local/share")
}

/// `$XDG_RUNTIME_DIR` when the session set one, for files that mean
/// nothing without it (another program's socket).
pub fn session_runtime() -> Option<PathBuf> {
    var("XDG_RUNTIME_DIR")
}

/// `$XDG_RUNTIME_DIR`, else `/tmp/formalshell-<uid>`.
pub fn runtime() -> PathBuf {
    session_runtime().unwrap_or_else(|| PathBuf::from(format!("/tmp/formalshell-{}", unsafe { libc::getuid() })))
}

/// `<runtime>/formalshell`.
pub fn shell_runtime() -> PathBuf {
    runtime().join(SHELL_DIR)
}

/// `$WAYLAND_DISPLAY` as a file name: one socket per display, so a nested
/// session's shell does not hand its calls to the outer one.
pub fn display_key() -> String {
    std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "default".into()).replace('/', "_")
}

/// `<runtime>/formalshell/<name>-<compositor instance>`: a file that
/// outlives one shell and belongs to one compositor run. Keyed on the
/// instance signature where there is one, since socket names like
/// `wayland-1` come back with the next session.
pub fn compositor_file(name: &str) -> PathBuf {
    let id = ["HYPRLAND_INSTANCE_SIGNATURE", "WAYLAND_DISPLAY"]
        .into_iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .unwrap_or_default();
    shell_runtime().join(format!("{name}-{}", id.replace('/', "_")))
}
