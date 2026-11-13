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

/// `sh -c SCOPED sh <unit> <argv>...`: the app in a scope, or bare when
/// systemd-run did not get as far as running it (not installed, no manager,
/// the scope refused). The scope's first process marks a file before it
/// execs the app, so an app that ran and failed is not started twice. The
/// shell waits as the app's parent, since systemd-run's exit status alone
/// cannot tell its own failure from the app's.
const SCOPED: &str = r#"unit=$1; shift
mark=${XDG_RUNTIME_DIR:-/tmp}/formalwm-spawn-$$
systemd-run --user --scope --quiet --collect --slice=app-graphical.slice "--unit=$unit" \
  -- /bin/sh -c ': > "$0" && exec "$@"' "$mark" "$@"
status=$?
if [ -e "$mark" ]; then rm -f "$mark"; exit "$status"; fi
exec "$@""#;

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
    let mut wrapped: Vec<String> = vec!["/bin/sh".into(), "-c".into(), SCOPED.into(), "sh".into()];
    wrapped.push(unit);
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
    use std::{fs, os::unix::fs::PermissionsExt, process};

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| word.to_string()).collect()
    }

    #[test]
    fn an_app_runs_in_a_scope_of_its_own() {
        let wrapped = scoped(
            argv(&["/opt/fspec/current/bin/t3code-desktop", "--x"]),
            true,
        );
        assert_eq!(&wrapped[..4], argv(&["/bin/sh", "-c", SCOPED, "sh"]));
        let unit = wrapped[4]
            .strip_prefix("app-formalwm-t3code-desktop-")
            .unwrap();
        assert!(
            unit.ends_with(".scope") && unit.len() == "01234567.scope".len(),
            "{unit}"
        );
        assert_eq!(
            &wrapped[5..],
            argv(&["/opt/fspec/current/bin/t3code-desktop", "--x"])
        );
    }

    #[test]
    fn a_shell_line_is_named_for_its_first_word() {
        let wrapped = scoped(argv(&["/bin/sh", "-c", "foot -e htop"]), true);
        assert!(wrapped[4].starts_with("app-formalwm-foot-"), "{wrapped:?}");
        assert_eq!(&wrapped[5..], argv(&["/bin/sh", "-c", "foot -e htop"]));
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

    /// Runs the wrapped app with `systemd_run` (a script body, or none) as
    /// the only `systemd-run` on `PATH`. Returns how often the app ran and
    /// the exit status.
    fn launch(name: &str, systemd_run: Option<&str>) -> (usize, Option<i32>) {
        let dir = std::env::temp_dir().join(format!("formalwm-spawn-{}-{name}", process::id()));
        // `rm` and the stand-in systemd-run, if any, and nothing else.
        let tools = dir.join("tools");
        fs::create_dir_all(&tools).unwrap();
        let rm = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|path| path.join("rm"))
            .find(|rm| rm.exists())
            .unwrap();
        std::os::unix::fs::symlink(rm, tools.join("rm")).unwrap();
        if let Some(body) = systemd_run {
            let path = tools.join("systemd-run");
            fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let ran = dir.join("ran");
        let app = format!("echo >> '{}'; exit 3", ran.display());
        let wrapped = scoped(argv(&["/bin/sh", "-c", &app]), true);
        let status = process::Command::new(&wrapped[0])
            .args(&wrapped[1..])
            .env("PATH", &tools)
            .env("XDG_RUNTIME_DIR", &dir)
            .status()
            .unwrap();
        let runs = fs::read_to_string(&ran).map_or(0, |ran| ran.lines().count());
        let marks = fs::read_dir(&dir)
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("formalwm-spawn-")
            })
            .count();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(marks, 0, "a mark was left behind");
        (runs, status.code())
    }

    #[test]
    fn an_app_starts_bare_when_its_scope_cannot_be_set_up() {
        assert_eq!(launch("missing", None), (1, Some(3)));
        assert_eq!(launch("refused", Some("exit 1")), (1, Some(3)));
    }

    #[test]
    fn an_app_that_ran_in_its_scope_is_not_started_again() {
        // What systemd-run does once the scope exists: exec what follows `--`.
        let systemd_run = r#"while [ "$1" != -- ]; do shift; done; shift; exec "$@""#;
        assert_eq!(launch("scoped", Some(systemd_run)), (1, Some(3)));
    }
}
