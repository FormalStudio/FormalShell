//! The sessions a login can start: the `.desktop` files under
//! `greeter.sessionDirs`, as tuigreet and SDDM read them, since greetd
//! itself has no enumeration call.
//!
//! The pick is remembered per user under the greeter's own state dir,
//! `sessions/<user>`, plus `sessions/.last` for the screen before anyone
//! has typed a name.

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

pub const DIRS: [&str; 2] = ["/run/current-system/sw/share/wayland-sessions", "/usr/share/wayland-sessions"];

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
            "Exec" => exec = Some(strip_codes(value.trim())),
            "DesktopNames" => desktops = value.trim().trim_end_matches(';').replace(';', ":"),
            "Hidden" | "NoDisplay" if value.trim() == "true" => return None,
            "TryExec" if value.starts_with('/') && !Path::new(value.trim()).exists() => return None,
            _ => {}
        }
    }
    let exec = exec.filter(|e| !e.is_empty())?;
    Some(Session { id: id.to_owned(), name: name.unwrap_or_else(|| id.to_owned()), exec, desktops })
}

/// The desktop entry spec's `%f`-style codes, `%%` kept as one `%`.
fn strip_codes(exec: &str) -> String {
    let mut out = String::with_capacity(exec.len());
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        if let Some('%') = chars.next() {
            out.push('%');
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The remembered session when it still exists, else Hyprland, else the
/// first.
pub fn pick(sessions: &[Session], remembered: Option<&str>) -> usize {
    let at = |id: &str| sessions.iter().position(|s| s.id == id);
    remembered
        .and_then(at)
        .or_else(|| at("hyprland-uwsm"))
        .or_else(|| at("hyprland"))
        .or_else(|| sessions.iter().position(|s| s.id.starts_with("hyprland")))
        .unwrap_or(0)
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
        assert_eq!(strip_codes("a %f 100%% b"), "a 100% b");
    }

    #[test]
    fn picks_remembered_then_hyprland() {
        let list = [session("plasma"), session("hyprland"), session("hyprland-uwsm")];
        assert_eq!(pick(&list, Some("plasma")), 0);
        assert_eq!(pick(&list, Some("gone")), 2);
        assert_eq!(pick(&list, None), 2);
        assert_eq!(pick(&list[..2], None), 1);
        assert_eq!(pick(&[session("sway")], None), 0);
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
