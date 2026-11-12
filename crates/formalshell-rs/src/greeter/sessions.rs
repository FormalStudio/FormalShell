//! The sessions a login can start: the `.desktop` files under
//! `greeter.sessionDirs`, as tuigreet and SDDM read them, since greetd
//! itself has no enumeration call.
//!
//! The pick is remembered per user under the greeter's own state dir,
//! `sessions/<user>`, plus `sessions/.last` for the screen before a user is
//! known, and `sessions/.user` is who logged in last.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    /// The file's stem, what `XDG_SESSION_DESKTOP` carries.
    pub id: String,
    pub name: String,
    /// `Exec` with its field codes dropped. greetd runs a session's argv
    /// through `sh -c`, so the line goes over as one word.
    pub exec: String,
    /// `DesktopNames`, `:`-joined for `XDG_CURRENT_DESKTOP`.
    pub desktops: String,
}

impl Session {
    pub fn command(&self) -> Vec<String> {
        vec![self.exec.clone()]
    }

    /// What SDDM exports for a Wayland session.
    pub fn env(&self) -> Vec<String> {
        let mut env = vec!["XDG_SESSION_TYPE=wayland".to_owned(), format!("XDG_SESSION_DESKTOP={}", self.id)];
        if !self.desktops.is_empty() {
            env.push(format!("XDG_CURRENT_DESKTOP={}", self.desktops));
        }
        env
    }
}

pub const DIRS: [&str; 1] = ["/usr/share/wayland-sessions"];

/// Every visible session in `dirs`, by name; the first dir wins a
/// repeated id.
pub fn scan(dirs: &[PathBuf]) -> Vec<Session> {
    let mut out: Vec<Session> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "desktop")).collect();
        files.sort();
        for path in files {
            let Some(id) = path.file_stem().and_then(|s| s.to_str()).map(str::to_owned) else { continue };
            if out.iter().any(|s| s.id == id) {
                continue;
            }
            if let Some(s) = std::fs::read_to_string(&path).ok().and_then(|text| parse(&id, &text)) {
                out.push(s);
            }
        }
    }
    out.sort_by_key(|s| s.name.to_lowercase());
    out
}

fn parse(id: &str, text: &str) -> Option<Session> {
    let (mut name, mut exec, mut desktops) = (None, None, String::new());
    let mut in_entry = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        let Some((key, value)) = line.split_once('=').filter(|_| in_entry) else { continue };
        match key.trim() {
            "Name" => name = Some(value.trim().to_owned()),
            "Exec" => exec = Some(fs_system::exec::strip_field_codes(value.trim())),
            "DesktopNames" => desktops = value.trim().trim_end_matches(';').replace(';', ":"),
            "Hidden" | "NoDisplay" if value.trim() == "true" => return None,
            "TryExec" if value.starts_with('/') && !Path::new(value.trim()).exists() => return None,
            _ => {}
        }
    }
    let exec = exec.filter(|e| !e.is_empty())?;
    Some(Session { id: id.to_owned(), name: name.unwrap_or_else(|| id.to_owned()), exec, desktops })
}

/// The remembered session when it still exists, else the host's default
/// (`greeter.defaultSession`) when that one does, else Hyprland, else the
/// first.
pub fn pick(sessions: &[Session], remembered: Option<&str>, default: Option<&str>) -> usize {
    let at = |id: &str| sessions.iter().position(|s| s.id == id);
    remembered
        .and_then(at)
        .or_else(|| default.and_then(at))
        .or_else(|| at("hyprland-uwsm"))
        .or_else(|| at("hyprland"))
        .or_else(|| sessions.iter().position(|s| s.id.starts_with("hyprland")))
        .unwrap_or(0)
}

/// The session whose `DesktopNames` lead with `desktop`. The boot login
/// starts its compositor through uwsm, so a uwsm entry wins over a plain
/// one for the same desktop.
pub fn running(sessions: &[Session], desktop: &str) -> Option<usize> {
    let matches = |s: &&Session| s.desktops.split(':').next() == Some(desktop);
    let uwsm = sessions.iter().position(|s| matches(&s) && s.exec.contains("uwsm"));
    uwsm.or_else(|| sessions.iter().position(|s| matches(&s)))
}

/// What the boot lock leaves for the login script
/// ([`crate::paths::LOGIN_SESSION_MARKER`]): the session's environment as
/// the greeter hands it to greetd, a line each, then its command.
pub fn handover(s: &Session) -> String {
    let mut lines = s.env();
    lines.push(s.exec.clone());
    lines.join("\n") + "\n"
}

/// The last user to log in here.
pub fn last_user(state: &Path) -> Option<String> {
    let text = std::fs::read_to_string(state.join("sessions/.user")).ok()?;
    Some(text.trim().to_owned()).filter(|t| !t.is_empty())
}

pub fn remember_user(state: &Path, user: &str) {
    let _ = std::fs::create_dir_all(state.join("sessions"));
    let _ = std::fs::write(state.join("sessions/.user"), format!("{user}\n"));
}

/// A name that is safe as one path component.
fn file_for(dir: &Path, user: Option<&str>) -> Option<PathBuf> {
    match user {
        None => Some(dir.join("sessions/.last")),
        Some(u) if !u.is_empty() && !u.starts_with('.') && !u.contains('/') => Some(dir.join("sessions").join(u)),
        Some(_) => None,
    }
}

/// `user`'s last session id, or the last one anyone started with None.
pub fn remembered(state: &Path, user: Option<&str>) -> Option<String> {
    let text = std::fs::read_to_string(file_for(state, user)?).ok()?;
    Some(text.trim().to_owned()).filter(|t| !t.is_empty())
}

pub fn remember(state: &Path, user: &str, id: &str) {
    let _ = std::fs::create_dir_all(state.join("sessions"));
    for path in [file_for(state, Some(user)), file_for(state, None)].into_iter().flatten() {
        if let Err(e) = std::fs::write(&path, format!("{id}\n")) {
            eprintln!("greeter: cannot write {}: {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str) -> Session {
        Session { id: id.into(), name: id.into(), exec: id.into(), desktops: String::new() }
    }

    #[test]
    fn parses_an_entry() {
        let s = parse(
            "hyprland-uwsm",
            "[Desktop Entry]\nName=Hyprland (uwsm-managed)\nExec=uwsm start -e -D Hyprland hyprland.desktop %U\nDesktopNames=Hyprland;KDE;\n[Desktop Action x]\nName=Other\n",
        )
        .unwrap();
        assert_eq!(s.name, "Hyprland (uwsm-managed)");
        assert_eq!(s.exec, "uwsm start -e -D Hyprland hyprland.desktop");
        assert_eq!(s.env(), ["XDG_SESSION_TYPE=wayland", "XDG_SESSION_DESKTOP=hyprland-uwsm", "XDG_CURRENT_DESKTOP=Hyprland:KDE"]);
        assert!(parse("x", "[Desktop Entry]\nName=X\nExec=x\nNoDisplay=true\n").is_none());
        assert!(parse("x", "[Desktop Entry]\nName=X\nExec=x\nTryExec=/nonexistent/bin/x\n").is_none());
        assert!(parse("x", "[Desktop Entry]\nName=X\n").is_none());
    }

    #[test]
    fn picks_remembered_then_hyprland() {
        let list = [session("plasma"), session("hyprland"), session("hyprland-uwsm")];
        assert_eq!(pick(&list, Some("plasma"), None), 0);
        assert_eq!(pick(&list, Some("gone"), None), 2);
        assert_eq!(pick(&list, None, None), 2);
        assert_eq!(pick(&list[..2], None, None), 1);
        assert_eq!(pick(&[session("sway")], None, None), 0);
    }

    #[test]
    fn the_hosts_default_comes_after_the_remembered_one() {
        let list = [session("hyprland-uwsm"), session("formalwm"), session("plasma")];
        assert_eq!(pick(&list, None, Some("formalwm")), 1);
        assert_eq!(pick(&list, Some("plasma"), Some("formalwm")), 2);
        assert_eq!(pick(&list, Some("gone"), Some("formalwm")), 1);
        assert_eq!(pick(&list, None, Some("niri")), 0);
    }

    #[test]
    fn the_running_session_is_the_current_desktops() {
        let entry = |id: &str, desktops: &str, exec: &str| Session { desktops: desktops.into(), exec: exec.into(), ..session(id) };
        let list = [
            entry("formalwm-uwsm", "ShojiWM", "uwsm start -e -D ShojiWM formalwm.desktop"),
            entry("hyprland", "Hyprland", "Hyprland"),
            entry("hyprland-uwsm", "Hyprland", "uwsm start -e -D Hyprland hyprland.desktop"),
            entry("plasma", "KDE", "startplasma-wayland"),
        ];
        assert_eq!(running(&list, "Hyprland"), Some(2));
        assert_eq!(running(&list, "ShojiWM"), Some(0));
        assert_eq!(running(&list, "KDE"), Some(3));
        assert_eq!(running(&list[..2], "Hyprland"), Some(1));
        assert_eq!(running(&list, "sway"), None);
        assert_eq!(running(&[session("formalshell")], "Hyprland"), None);
    }

    #[test]
    fn the_handover_is_what_greetd_would_start() {
        let s = Session { desktops: "ShojiWM".into(), exec: "uwsm start -e -D ShojiWM formalwm.desktop".into(), ..session("formalwm-uwsm") };
        assert_eq!(
            handover(&s),
            "XDG_SESSION_TYPE=wayland\nXDG_SESSION_DESKTOP=formalwm-uwsm\nXDG_CURRENT_DESKTOP=ShojiWM\nuwsm start -e -D ShojiWM formalwm.desktop\n"
        );
    }

    #[test]
    fn remembers_per_user() {
        let dir = std::env::temp_dir().join(format!("fs-greeter-sessions-{}", std::process::id()));
        remember(&dir, "kyan", "plasma");
        assert_eq!(remembered(&dir, Some("kyan")).as_deref(), Some("plasma"));
        assert_eq!(remembered(&dir, None).as_deref(), Some("plasma"));
        assert_eq!(remembered(&dir, Some("other")), None);
        assert_eq!(remembered(&dir, Some("../kyan")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
