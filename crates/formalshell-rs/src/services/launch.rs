//! Launch feedback: a desktop entry the shell spawns is a
//! running Activity item from the spawn until a window of it maps, read off
//! the compositor's window list as it changes. One that maps none in
//! [`fs_activity::LAUNCH_MS`] reads as failed (the model's own fold), and a
//! window mapping later still takes that back, however much later: a launch
//! is watched until its window maps or its entry is launched again, so the
//! list holds one launch per entry at most.

use std::sync::OnceLock;

use fs_menu::node::DesktopEntry;
use fs_system::monitor::gpu::{self, OffloadTools};
use fs_types::{Window, WindowId};

use super::activity::{self, Input};
use super::menu::State as MenuState;
use super::{compositor, proc};
use crate::runtime::{Ctx, Publisher};
use crate::store::{self, Store};

/// One tier's comparison across every window, in window order. An empty
/// `app_id` never matches, so a window the compositor has not classified
/// cannot be claimed by an entry with an empty `startup_class`.
fn collect(needle: &str, windows: &[Window], fold: bool) -> Vec<WindowId> {
    let want = if fold { needle.to_lowercase() } else { needle.to_string() };
    windows
        .iter()
        .filter(|w| !w.app_id.is_empty())
        .filter(|w| if fold { w.app_id.to_lowercase() == want } else { w.app_id == want })
        .map(|w| w.id.clone())
        .collect()
}

/// Which running windows belong to a desktop entry, so activating an app row
/// focuses the app you already have open instead of spawning a second copy.
///
/// Only two entry fields are read: `startup_class` and `id` (the .desktop
/// basename, which equals the app_id for the large majority of Linux apps).
/// Each is tried exactly, then case-folded. There is deliberately no fuzzy
/// tier: Electron and wrapper-launched apps often report an app_id unrelated
/// to their .desktop basename, and a miss falls through to the caller's
/// spawn path while a fuzzy hit focuses the wrong app. Window ids are
/// opaque: `app_id`s are compared, ids are copied into the result verbatim.
///
/// Window ids in window order, or empty when nothing matched.
///
/// The first non-empty tier wins outright, never a union: a union would mix a
/// precise `startup_class` hit with a coincidental id hit and then cycle
/// between two different apps.
pub fn match_windows(entry: Option<&DesktopEntry>, windows: &[Window]) -> Vec<WindowId> {
    let Some(entry) = entry else { return Vec::new() };
    let mut tiers: Vec<&str> = Vec::new();
    if !entry.startup_class.is_empty() {
        tiers.push(&entry.startup_class);
    }
    if !entry.id.is_empty() {
        tiers.push(&entry.id);
    }
    for tier in tiers {
        let exact = collect(tier, windows, false);
        if !exact.is_empty() {
            return exact;
        }
        let folded = collect(tier, windows, true);
        if !folded.is_empty() {
            return folded;
        }
    }
    Vec::new()
}

/// The window to focus next: the one AFTER `focused_window_id` in `matches`,
/// wrapping, so repeat-activating an app row cycles its open instances. Focus
/// outside `matches` (another app, or nothing focused) starts the cycle at
/// the first instance. None when nothing matched.
pub fn next_window(matches: &[WindowId], focused_window_id: &WindowId) -> Option<WindowId> {
    let at = matches.iter().position(|m| m == focused_window_id).map_or(0, |at| at + 1);
    matches.get(at % matches.len().max(1)).cloned()
}

pub struct Pending {
    entry: DesktopEntry,
    /// The windows the entry already had, which do not end the launch.
    before: Vec<WindowId>,
}

/// The launches being watched, one per entry.
#[derive(Default)]
pub struct State {
    pending: Vec<Pending>,
}

pub enum Diff {
    Spawned(Pending),
    /// These entries' launches mapped a window.
    Landed(Vec<String>),
}

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        match diff {
            Diff::Spawned(p) => {
                self.pending.retain(|q| q.entry.id != p.entry.id);
                self.pending.push(p);
                true
            }
            Diff::Landed(ids) => {
                self.pending.retain(|p| !ids.contains(&p.entry.id));
                !ids.is_empty()
            }
        }
    }
}

/// Spawns run on the UI thread from surfaces holding the store read-only,
/// so a launch reaches its slice through the channel every service uses.
static PUBLISHER: OnceLock<Publisher> = OnceLock::new();

pub fn start(ctx: &Ctx) {
    let _ = PUBLISHER.set(ctx.publisher().clone());
}

/// The desktop entry `id` names in the launcher's tree.
fn entry(store: &Store, id: &str) -> Option<DesktopEntry> {
    store.menu.tree.nodes.values().find_map(|n| n.entry.as_ref().filter(|e| e.id == id)).cloned()
}

fn uwsm() -> bool {
    std::env::var_os("UWSM_FINALIZE_VARNAMES").is_some_and(|v| !v.is_empty())
}

/// Proc.appLaunch: under uwsm a plain entry goes by its id, so uwsm reads
/// its Exec line; everything else runs its own argv.
pub fn app_argv(menu: &MenuState, id: &str) -> Option<Vec<String>> {
    let command = menu.exec.apps.get(id).filter(|c| !c.is_empty())?;
    let plain = id.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_') && id.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    if uwsm() && plain {
        return Some(proc::argv(&["uwsm", "app", "--", &format!("{id}.desktop")]));
    }
    Some(scoped(command))
}

/// Desktop action `key` of entry `id`, run as its own argv.
pub fn action_argv(menu: &MenuState, id: &str, key: &str) -> Option<Vec<String>> {
    let command = menu.exec.actions.get(&(id.to_owned(), key.to_owned())).filter(|c| !c.is_empty())?;
    Some(scoped(command))
}

/// `command` in a scope of its own under uwsm, as it is outside one.
pub fn scoped(command: &[String]) -> Vec<String> {
    if !uwsm() {
        return command.to_vec();
    }
    let mut argv = proc::argv(&["uwsm", "app", "--"]);
    argv.extend(command.iter().cloned());
    argv
}

/// Desktop entry `id`'s next window focused, else the entry launched.
pub fn open(store: &Store, id: &str) {
    let Some(entry) = entry(store, id) else { return };
    let snapshot = &store.compositor.snapshot;
    match next_window(&match_windows(Some(&entry), &snapshot.windows), &snapshot.focused_window_id) {
        Some(window) => compositor::focus_window(&window),
        None => {
            if let Some(argv) = app_argv(&store.menu, id) {
                spawn(store, id, &argv);
            }
        }
    }
}

/// Spawns `argv`, which starts desktop entry `id`, its launch shown until a
/// new window of it maps. An id the launcher's tree does not hold has no
/// window to wait on, so its spawn shows nothing.
pub fn spawn(store: &Store, id: &str, argv: &[String]) {
    compositor::spawn(argv);
    let Some(entry) = entry(store, id) else { return };
    let before = match_windows(Some(&entry), &store.compositor.snapshot.windows);
    let title = if entry.name.is_empty() { entry.id.clone() } else { entry.name.clone() };
    let app = entry.id.clone();
    if let Some(p) = PUBLISHER.get() {
        p.publish(store::Diff::Launch(Diff::Spawned(Pending { entry, before })));
    }
    activity::send(Input::Launch { app, title });
}

/// The compositor's windows changed: a launch whose entry has a window it
/// did not have at the spawn is over.
pub fn windows_changed(store: &Store) -> Option<store::Diff> {
    let windows = &store.compositor.snapshot.windows;
    let landed: Vec<String> = store
        .launches
        .pending
        .iter()
        .filter(|p| match_windows(Some(&p.entry), windows).iter().any(|id| !p.before.contains(id)))
        .map(|p| p.entry.id.clone())
        .collect();
    for id in &landed {
        activity::send(Input::Launched(id.clone()));
    }
    (!landed.is_empty()).then_some(store::Diff::Launch(Diff::Landed(landed)))
}

/// What [`on_gpu`] started.
pub struct OnGpu {
    pub card: String,
    pub name: String,
    pub argv: Vec<String>,
    /// The entry asked for a terminal and `console.command` names none.
    pub bare_terminal: bool,
}

struct DesktopFile {
    exec: String,
    path: String,
    terminal: bool,
}

/// `DesktopEntries.byId`: the first `<id>.desktop` on the data dirs. The
/// raw `Exec` is kept, since the offload wrapper runs it through `sh -c`.
fn desktop_file(id: &str) -> Option<DesktopFile> {
    let text = crate::paths::data_dirs().iter().find_map(|d| std::fs::read_to_string(d.join("applications").join(format!("{id}.desktop"))).ok())?;
    let mut in_entry = false;
    let (mut exec, mut path, mut terminal) = (None, None, false);
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry && let Some((k, v)) = line.split_once('=') {
            match k.trim() {
                "Exec" if exec.is_none() => exec = Some(v.trim().to_owned()),
                "Path" if path.is_none() => path = Some(v.trim().to_owned()),
                "Terminal" => terminal = v.trim() == "true",
                _ => {}
            }
        }
    }
    Some(DesktopFile { exec: exec.unwrap_or_default(), path: path.unwrap_or_default(), terminal })
}

fn shq(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Spawns entry `id` on GPU `card`, or the first discrete one when `card` is
/// empty, with the offload argv built by hand since a desktop entry's own
/// launch carries no environment.
pub fn on_gpu(store: &Store, id: &str, card: &str) -> Result<OnGpu, String> {
    let Some(file) = desktop_file(id) else { return Err(format!("no desktop entry '{id}'")) };
    let m = &store.monitor;
    let target = if card.is_empty() {
        m.cards.iter().find(|c| c.discrete).ok_or("no discrete GPU on this machine")?
    } else {
        m.cards.iter().find(|c| c.record.card.card == card).ok_or_else(|| format!("no GPU card '{card}'"))?
    };
    let mut exec = file.exec;
    if !file.path.is_empty() {
        exec = format!("cd {} && {exec}", shq(&file.path));
    }
    let tools = OffloadTools { nvidia_offload: m.tools.0, prime_run: m.tools.1 };
    let mut argv = gpu::offload_argv(&exec, Some(&target.record.card), tools);
    let mut bare = true;
    if file.terminal {
        let term = crate::settings::console_command_set(&store.config).unwrap_or_default();
        if !term.is_empty() {
            argv = term.into_iter().chain(argv).collect();
            bare = false;
        }
    }
    spawn(store, id, &argv);
    Ok(OnGpu { card: target.record.card.card.clone(), name: target.name.clone(), argv, bare_terminal: file.terminal && bare })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(id: &str, app_id: &str) -> Window {
        Window { id: WindowId::new(id), app_id: app_id.into(), ..Window::default() }
    }

    fn next(matches: &[WindowId], focused: &str) -> Option<String> {
        next_window(matches, &WindowId::new(focused)).map(|id| id.to_string())
    }

    fn entry(id: &str, startup_class: &str) -> DesktopEntry {
        DesktopEntry { id: id.into(), startup_class: startup_class.into(), name: id.into(), ..DesktopEntry::default() }
    }

    fn ids(v: &[&str]) -> Vec<WindowId> {
        v.iter().map(|s| WindowId::new(*s)).collect()
    }

    #[test]
    fn startup_class_beats_id_when_both_present() {
        let windows = [win("1", "firefox"), win("2", "org.mozilla.firefox")];
        let matches = match_windows(Some(&entry("org.mozilla.firefox", "firefox")), &windows);
        assert_eq!(matches, ["1"]);
    }

    #[test]
    fn startup_class_matches_case_insensitively() {
        let windows = [win("1", "Alacritty")];
        let matches = match_windows(Some(&entry("alacritty-terminal", "alacritty")), &windows);
        assert_eq!(matches, ["1"]);
    }

    #[test]
    fn exact_startup_class_wins_over_a_case_folded_one() {
        let windows = [win("1", "Alacritty"), win("2", "alacritty")];
        let matches = match_windows(Some(&entry("alacritty", "alacritty")), &windows);
        assert_eq!(matches, ["2"]);
    }

    #[test]
    fn falls_back_to_entry_id_when_startup_class_is_empty() {
        let windows = [win("1", "mpv")];
        assert_eq!(match_windows(Some(&entry("mpv", "")), &windows), ["1"]);
    }

    #[test]
    fn no_match_returns_empty_list_so_the_caller_spawns() {
        let windows = [win("1", "firefox"), win("2", "mpv")];
        assert!(match_windows(Some(&entry("code", "code")), &windows).is_empty());
    }

    #[test]
    fn empty_app_id_on_a_window_never_matches_an_empty_startup_class() {
        let windows = [win("1", "")];
        assert!(match_windows(Some(&entry("", "")), &windows).is_empty());
        assert!(match_windows(Some(&entry("mpv", "")), &windows).is_empty());
    }

    #[test]
    fn every_instance_of_the_same_app_is_matched_in_window_order() {
        let windows = [win("1", "kitty"), win("2", "mpv"), win("3", "kitty")];
        let matches = match_windows(Some(&entry("kitty", "kitty")), &windows);
        assert_eq!(matches, ["1", "3"]);
    }

    #[test]
    fn next_window_cycles_past_the_focused_instance_and_wraps() {
        let matches = ids(&["a", "b", "c"]);
        assert_eq!(next(&matches, "a").as_deref(), Some("b"));
        assert_eq!(next(&matches, "b").as_deref(), Some("c"));
        assert_eq!(next(&matches, "c").as_deref(), Some("a"));
    }

    #[test]
    fn next_window_returns_first_when_focus_is_elsewhere_or_empty() {
        let matches = ids(&["a", "b"]);
        assert_eq!(next(&matches, "").as_deref(), Some("a"));
        assert_eq!(next(&matches, "zzz").as_deref(), Some("a"));
    }

    #[test]
    fn next_window_of_empty_matches_is_none() {
        assert_eq!(next(&[], "a"), None);
    }

    #[test]
    fn window_ids_are_returned_verbatim_never_parsed() {
        let windows = [win("abc123def", "kitty"), win("0x55f1", "kitty")];
        let matches = match_windows(Some(&entry("kitty", "kitty")), &windows);
        assert_eq!(matches, ["abc123def", "0x55f1"]);
        assert_eq!(next(&matches, "abc123def").as_deref(), Some("0x55f1"));
    }
}
