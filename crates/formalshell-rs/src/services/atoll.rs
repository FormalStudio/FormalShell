//! A Mac app taken out by `atoll uninstall`, told as an Activity item the
//! way fspec tells its own: running, then done, or failed with atoll's
//! last line.

use std::path::Path;
use std::time::Duration;

use fs_activity::progress::Progress;
use fs_activity::{Item, Origin, State};

use super::activity::{self, Input};
use super::proc;
use crate::runtime::{Ctx, now_ms};
use crate::store::Store;

/// The atoll that installed an entry: the one its `Exec` runs, else the
/// one on the path (an Electron app's entry runs the app's own binary).
fn program(exec: &[String]) -> String {
    exec.first().filter(|p| Path::new(p).file_name().is_some_and(|n| n == "atoll")).cloned().unwrap_or_else(|| "atoll".to_owned())
}

/// What `uninstall` runs for desktop entry `id`, and the app's name:
/// nothing for an entry that is not a Mac app's.
pub fn uninstall_of(store: &Store, id: &str) -> Option<(Vec<String>, String)> {
    let entry = store.menu.nodes().get(&format!("apps.{id}"))?.entry.as_ref()?;
    if entry.bundle_id.is_empty() {
        return None;
    }
    let exec = store.menu.exec.apps.get(id).map_or(&[][..], Vec::as_slice);
    let name = if entry.name.is_empty() { entry.id.clone() } else { entry.name.clone() };
    Some((vec![program(exec), "uninstall".to_owned(), entry.bundle_id.clone()], name))
}

fn failure(stderr: &str, code: i32) -> String {
    let line = stderr.lines().rev().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let line = line.strip_prefix("atoll: ").unwrap_or(line);
    match (line.is_empty(), code) {
        (false, _) => line.to_owned(),
        (true, proc::MISSING) => "atoll is not installed".to_owned(),
        (true, code) => format!("atoll exited {code}"),
    }
}

/// Runs `argv` for the app `id`, named `name`. Its desktop entry going
/// takes the app's row with it (`services/menu.rs` watches the directory).
pub fn uninstall(ctx: &Ctx, id: String, name: String, argv: Vec<String>) {
    ctx.spawn(async move {
        let started = now_ms();
        let item = |state: State, summary: Option<String>| {
            Box::new(Item {
                id: format!("atoll:uninstall:{id}"),
                origin: Origin::Atoll,
                source: format!("atoll:{id}"),
                kind: "uninstall".into(),
                app: Some(id.clone()),
                title: format!("Uninstalling {name}"),
                detail: String::new(),
                phase: None,
                progress: Progress::Indeterminate,
                state,
                urgent: state == State::Failed,
                keys: Vec::new(),
                summary,
                started_ms: started,
                updated_ms: now_ms(),
                ended_ms: None,
            })
        };
        activity::send(Input::Op(item(State::Running, None)));
        let done = proc::capture_err(&argv, Duration::from_secs(120)).await;
        let ended = match done.code {
            0 => item(State::Succeeded, Some(format!("Uninstalled {name}"))),
            code => {
                let why = failure(&done.stderr, code);
                eprintln!("atoll: uninstall {id}: {why}");
                item(State::Failed, Some(why))
            }
        };
        activity::send(Input::Op(ended));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entrys_own_atoll_runs_the_uninstall() {
        let exec = |words: &[&str]| words.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
        assert_eq!(program(&exec(&["/opt/atoll/bin/atoll", "run", "/apps/Rex.app"])), "/opt/atoll/bin/atoll");
        assert_eq!(program(&exec(&["/apps/md.obsidian/Obsidian/electron", "--ozone-platform-hint=auto"])), "atoll");
        assert_eq!(program(&[]), "atoll");
    }

    #[test]
    fn a_failure_reads_as_atolls_last_line() {
        assert_eq!(failure("warning\natoll: com.x is not installed\n\n", 1), "com.x is not installed");
        assert_eq!(failure("", proc::MISSING), "atoll is not installed");
        assert_eq!(failure("", 2), "atoll exited 2");
    }
}
