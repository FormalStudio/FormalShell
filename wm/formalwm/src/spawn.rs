//! An app started from formalwm (a bind, the shell's launcher and dock
//! through `spawn`) runs in a systemd scope of its own, as `uwsm app` puts
//! it under Hyprland. Started bare it would stay in the compositor's unit:
//! its crash or OOM kill counted against the session, and the session's
//! stop taking it down with it.

use std::{
    path::Path,
    sync::atomic::{AtomicU32, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use shojiwm_rs::prelude::Command;

/// `argv` as an app.
pub fn app(argv: Vec<String>) -> Command {
    Command::exec(scoped(argv, manager()))
}

/// A shell line as an app, run as a bind runs one (`sh -c`).
pub fn app_shell(line: String) -> Command {
    match manager() {
        true => app(vec!["/bin/sh".into(), "-c".into(), line]),
        false => crate::binds::sh(line),
    }
}

/// A user manager answers on its private socket.
fn manager() -> bool {
    std::env::var_os("XDG_RUNTIME_DIR")
        .is_some_and(|dir| Path::new(&dir).join("systemd/private").exists())
}

/// Commands that make a unit of their own are left alone.
const LAUNCHERS: [&str; 3] = ["uwsm", "systemd-run", "app2unit"];

fn scoped(argv: Vec<String>, manager: bool) -> Vec<String> {
    let program = argv.first().map(|first| name(first)).unwrap_or_default();
    // `sh -c <line>` is named for the line's first word.
    let named = match argv.as_slice() {
        [sh, flag, line] if flag == "-c" && name(sh) == "sh" => {
            name(line.split_whitespace().next().unwrap_or("sh"))
        }
        _ => program.clone(),
    };
    if !manager || program.is_empty() || LAUNCHERS.contains(&program.as_str()) {
        return argv;
    }
    let unit = format!("app-formalwm-{}-{}.scope", escape(&named), suffix());
    let mut wrapped: Vec<String> = [
        "systemd-run",
        "--user",
        "--scope",
        "--quiet",
        "--collect",
        "--slice=app-graphical.slice",
    ]
    .map(String::from)
    .to_vec();
    wrapped.push(format!("--unit={unit}"));
    wrapped.push("--".into());
    wrapped.extend(argv);
    wrapped
}

fn name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// What a unit name may hold, as uwsm escapes an app's name.
fn escape(name: &str) -> String {
    name.chars()
        .take(48)
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Eight hex digits that differ from one spawn to the next.
fn suffix() -> String {
    static COUNT: AtomicU32 = AtomicU32::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |now| now.subsec_nanos());
    let count = COUNT.fetch_add(1, Ordering::Relaxed);
    format!("{:08x}", nanos ^ count.rotate_left(24) ^ std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| word.to_string()).collect()
    }

    #[test]
    fn an_app_runs_in_a_scope_of_its_own() {
        let wrapped = scoped(
            argv(&["/opt/fspec/current/bin/t3code-desktop", "--x"]),
            true,
        );
        assert_eq!(
            &wrapped[..6],
            argv(&[
                "systemd-run",
                "--user",
                "--scope",
                "--quiet",
                "--collect",
                "--slice=app-graphical.slice"
            ])
        );
        let unit = wrapped[6]
            .strip_prefix("--unit=app-formalwm-t3code-desktop-")
            .unwrap();
        assert!(
            unit.ends_with(".scope") && unit.len() == "01234567.scope".len(),
            "{unit}"
        );
        assert_eq!(
            &wrapped[7..],
            argv(&["--", "/opt/fspec/current/bin/t3code-desktop", "--x"])
        );
    }

    #[test]
    fn a_shell_line_is_named_for_its_first_word() {
        let wrapped = scoped(argv(&["/bin/sh", "-c", "foot -e htop"]), true);
        assert!(
            wrapped[6].starts_with("--unit=app-formalwm-foot-"),
            "{wrapped:?}"
        );
        assert_eq!(&wrapped[8..], argv(&["/bin/sh", "-c", "foot -e htop"]));
    }

    #[test]
    fn a_launcher_and_a_session_without_a_manager_run_bare() {
        let uwsm = argv(&["uwsm", "app", "--", "foot.desktop"]);
        assert_eq!(scoped(uwsm.clone(), true), uwsm);
        let foot = argv(&["foot"]);
        assert_eq!(scoped(foot.clone(), false), foot);
    }

    #[test]
    fn a_unit_name_holds_only_what_systemd_takes() {
        assert_eq!(escape("Visual Studio Code@1"), "Visual_Studio_Code_1");
        assert_ne!(suffix(), suffix());
    }
}
