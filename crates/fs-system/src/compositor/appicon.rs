//! Which desktop entry a window belongs to, and the picture that entry names.
//! The live entry list, the /proc answers and the themed lookup come from the
//! service; the chain is pure.
//!
//! The window is the backend's row (`app_id`, and on Hyprland `initial_class`,
//! `initial_title`); `entries` is the desktop entry list; `procs` is
//! `parse_procs`' answer for the window's pid, or nothing. First hit wins, in
//! this order:
//!
//!   1. The window's class, then its initial class, against each entry by the
//!      tiers: the id, the id case-folded, StartupWMClass, StartupWMClass
//!      case-folded. Then a reverse-DNS tier: the last dot segment of either side
//!      against the other, case-folded, which is what joins a Flatpak's
//!      `com.discordapp.Discord` to a window classed `discord` and
//!      `org.wezfurlong.wezterm` to an entry called `wezterm`. Last, a Steam
//!      game's `steam_app_<id>` against the shortcut Steam writes for it
//!      (`Exec=steam steam://rungameid/<id>`, no StartupWMClass).
//!   2. The process. The window's own pid against each entry's Exec program, by
//!      absolute path or by basename; then up to three ancestors, by absolute
//!      path only. An app launched through a script (`Exec=~/bin/x` running
//!      `sh ~/bin/x`, which starts the real binary) leaves the script's path in
//!      an ancestor's argv, while a basename match that far up would hand a
//!      class-less window launched from a terminal the terminal's own icon.
//!      A Steam game's shortcut claims no process: its program is Steam
//!      itself, so it would hand Steam's own windows (the class-less
//!      "Updating Steam..." one) the game's picture.
//!   3. The initial title against an entry's Name, case-folded. A native Wayland
//!      app that never sets an app id and runs from a wrapper the process walk
//!      cannot see through still usually titles its first window with its own
//!      name.
//!
//! The app matcher deliberately stops at tier 1's first four steps, because a
//! wrong hit there focuses the wrong app. A wrong hit here costs a wrong
//! picture on a tile that would otherwise be the generic one.

use fs_js as js;
use regex::Regex;
use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

pub const ANCESTOR_DEPTH: usize = 3;

/// Interpreters and launchers whose basename says nothing about the app running
/// under them: an entry whose Exec starts with one of these would otherwise
/// claim every script that one runs.
const LAUNCHERS: [&str; 15] = [
    "sh", "bash", "dash", "zsh", "fish", "env", "python", "python3", "node", "perl", "ruby", "systemd-run",
    "flatpak", "uwsm", "app2unit",
];

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DesktopEntry {
    pub id: String,
    pub name: String,
    pub startup_class: String,
    pub icon: String,
    pub command: Vec<String>,
    pub generic_name: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Window {
    pub app_id: String,
    pub initial_class: String,
    pub initial_title: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProcInfo {
    pub exe: String,
    pub argv: Vec<String>,
}

fn tail(value: &str) -> &str {
    value.rfind('.').map_or(value, |dot| &value[dot + 1..])
}

static WRAPPED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\.(.+)-wrapped$").unwrap());

/// A program's name as a user would call it: the path's last segment with a
/// nixpkgs wrapper's `.name-wrapped` undone, so `/nix/store/.../bin/.foot-wrapped`
/// reads as `foot`.
fn basename(path: &str) -> &str {
    let name = path.rfind('/').map_or(path, |i| &path[i + 1..]);
    WRAPPED.captures(name).map_or(name, |m| m.get(1).map_or(name, |g| g.as_str()))
}

/// The game a Steam game's window class names (`steam_app_2386720`).
fn steam_game(class: &str) -> Option<&str> {
    class.strip_prefix("steam_app_").filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
}

/// The game a Steam shortcut launches (`steam steam://rungameid/2386720`).
fn launches_game(entry: &DesktopEntry) -> Option<&str> {
    entry.command.iter().skip(1).find_map(|a| a.strip_prefix("steam://rungameid/"))
}

fn class_tiers<'a>(needle: &str, entries: &'a [DesktopEntry]) -> Option<&'a DesktopEntry> {
    let lower = needle.to_lowercase();
    let needle_tail = tail(needle).to_lowercase();
    let hits = |tier: usize, e: &DesktopEntry| match tier {
        0 => e.id == needle,
        1 => e.id.to_lowercase() == lower,
        2 => e.startup_class == needle,
        3 => !e.startup_class.is_empty() && e.startup_class.to_lowercase() == lower,
        4 => tail(&e.id).to_lowercase() == lower,
        5 => e.id.to_lowercase() == needle_tail,
        _ => steam_game(needle).is_some_and(|id| launches_game(e) == Some(id)),
    };
    (0..7).find_map(|tier| entries.iter().find(|e| hits(tier, e)))
}

/// The entry the first class tier finds. Split out so the service can tell
/// which windows need their process read at all.
pub fn by_class<'a>(win: Option<&Window>, entries: &'a [DesktopEntry]) -> Option<&'a DesktopEntry> {
    let w = win?;
    let needles = [w.app_id.as_str(), w.initial_class.as_str()];
    for (n, needle) in needles.iter().enumerate() {
        if needle.is_empty() || (n > 0 && *needle == needles[0]) {
            continue;
        }
        if let Some(hit) = class_tiers(needle, entries) {
            return Some(hit);
        }
    }
    None
}

fn program(entry: &DesktopEntry) -> &str {
    entry.command.first().map_or("", String::as_str)
}

fn proc_matches(proc: &ProcInfo, program: &str, depth: usize) -> bool {
    if program.starts_with('/') && (proc.exe == program || proc.argv.iter().any(|a| a == program)) {
        return true;
    }
    if depth > 0 {
        return false;
    }
    let name = basename(program);
    if name.is_empty() || LAUNCHERS.contains(&name) {
        return false;
    }
    basename(&proc.exe) == name || proc.argv.first().is_some_and(|a| basename(a) == name)
}

/// `procs` is the window's own process first, then its ancestors, nearest
/// first.
pub fn by_process<'a>(procs: Option<&[ProcInfo]>, entries: &'a [DesktopEntry]) -> Option<&'a DesktopEntry> {
    let chain = procs.unwrap_or(&[]);
    for (depth, proc) in chain.iter().enumerate().take(ANCESTOR_DEPTH + 1) {
        for entry in entries.iter().filter(|e| launches_game(e).is_none()) {
            let prog = program(entry);
            if !prog.is_empty() && proc_matches(proc, prog, depth) {
                return Some(entry);
            }
        }
    }
    None
}

pub fn by_title<'a>(win: Option<&Window>, entries: &'a [DesktopEntry]) -> Option<&'a DesktopEntry> {
    let title = js::trim(&win?.initial_title).to_lowercase();
    if title.is_empty() {
        return None;
    }
    entries.iter().find(|e| js::trim(&e.name).to_lowercase() == title)
}

pub fn entry_for<'a>(
    win: Option<&Window>,
    entries: &'a [DesktopEntry],
    procs: Option<&[ProcInfo]>,
) -> Option<&'a DesktopEntry> {
    by_class(win, entries).or_else(|| by_process(procs, entries)).or_else(|| by_title(win, entries))
}

/// An entry's `Icon=` as something an image can load: an absolute path as a file
/// url, an existing url as given, a theme name through `themed` (the icon theme
/// lookup, "" for a name the theme lacks). A path goes straight to the file
/// rather than through the icon provider, whose theme lookup would not find it.
pub fn source(name: &str, themed: Option<&dyn Fn(&str) -> String>) -> String {
    if name.is_empty() {
        return String::new();
    }
    if name.starts_with("file:") || name.starts_with("image:") {
        return name.to_string();
    }
    if name.starts_with('/') {
        return format!("file://{name}");
    }
    themed.map_or_else(String::new, |f| f(name))
}

static DIGITS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9]+$").unwrap());

/// The one shell pass the icon service runs over /proc, one line per process:
/// `<window pid>\t<depth>\t<exe>\t<argv joined by \x1f>`. Returns
/// `{ "<pid>": [ProcInfo, ...] }`, depth order; a line that does not parse is
/// dropped rather than guessed at.
pub fn parse_procs(text: &str) -> HashMap<String, Vec<ProcInfo>> {
    let mut sparse: HashMap<String, BTreeMap<usize, ProcInfo>> = HashMap::new();
    for line in text.split('\n') {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 4 || !DIGITS.is_match(fields[0]) || !DIGITS.is_match(fields[1]) {
            continue;
        }
        let mut argv: Vec<String> = fields[3..].join("\t").split('\u{1f}').map(String::from).collect();
        while argv.last().is_some_and(String::is_empty) {
            argv.pop();
        }
        let chain = sparse.entry(fields[0].to_string()).or_default();
        // An index past usize can never be reached by the dense walk below.
        if let Ok(depth) = fields[1].parse::<usize>() {
            chain.insert(depth, ProcInfo { exe: fields[2].to_string(), argv });
        }
    }
    sparse
        .into_iter()
        .map(|(pid, chain)| {
            let mut dense = Vec::new();
            let mut by_depth = chain;
            while let Some(info) = by_depth.remove(&dense.len()) {
                dense.push(info);
            }
            (pid, dense)
        })
        .collect()
}

/// The command that produces `parse_procs`' input for `pids`: each one and up to
/// `ANCESTOR_DEPTH` parents, stopping at init. `/proc/<pid>/status`'s PPid line
/// rather than `stat`'s fourth field, which sits after a comm that may itself
/// carry spaces and parentheses.
pub fn proc_command(pids: &[String]) -> Vec<String> {
    let script = [
        r#"for p in "$@"; do q=$p; d=0; "#.to_string(),
        format!(r#"while [ "$d" -le {ANCESTOR_DEPTH} ] && [ "$q" -gt 1 ] && [ -r "/proc/$q/cmdline" ]; do "#),
        r#"printf "%s\t%s\t%s\t" "$p" "$d" "$(readlink "/proc/$q/exe" 2>/dev/null)"; "#.to_string(),
        r#"tr "\000" "\037" < "/proc/$q/cmdline"; echo; "#.to_string(),
        r#"q=$(sed -n "s/^PPid:[[:space:]]*//p" "/proc/$q/status" 2>/dev/null); q=${q:-0}; d=$((d + 1)); "#.to_string(),
        "done; done".to_string(),
    ]
    .concat();
    let mut argv = vec!["sh".to_string(), "-c".to_string(), script, "appicon".to_string()];
    argv.extend(pids.iter().map(|p| js::num_str(js::parse_number(p))));
    argv
}

#[cfg(test)]
mod tests {
    use super::*;

    const THEME_ICONS: [&str; 4] = ["foot", "firefox", "discord", "org.wezfurlong.wezterm"];

    fn themed(name: &str) -> String {
        if THEME_ICONS.contains(&name) { format!("image://icon/{name}") } else { String::new() }
    }

    fn entry(id: &str, f: impl FnOnce(&mut DesktopEntry)) -> DesktopEntry {
        let mut e = DesktopEntry {
            id: id.into(),
            name: id.into(),
            startup_class: String::new(),
            icon: id.into(),
            command: vec![id.into()],
            generic_name: String::new(),
        };
        f(&mut e);
        e
    }

    fn cmd(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// Messages as it is on the owner's host: a native Wayland app with no app
    /// id at all, launched from a script in ~/.local/bin, whose entry names an
    /// absolute .svg and carries no StartupWMClass.
    fn messages() -> DesktopEntry {
        entry("messages", |e| {
            e.name = "Messages".into();
            e.icon = "/home/kyandesutter/.local/share/icons/messages.svg".into();
            e.command = cmd(&["/home/kyandesutter/.local/bin/messages"]);
        })
    }

    fn entries() -> Vec<DesktopEntry> {
        vec![
            entry("foot", |e| e.name = "Foot".into()),
            entry("firefox", |e| {
                e.name = "Firefox".into();
                e.startup_class = "firefox".into();
                e.command = cmd(&["firefox", "--name", "firefox"]);
            }),
            entry("com.discordapp.Discord", |e| {
                e.name = "Discord".into();
                e.icon = "discord".into();
                e.command = cmd(&["/usr/bin/flatpak", "run", "com.discordapp.Discord"]);
            }),
            entry("org.wezfurlong.wezterm", |e| {
                e.name = "WezTerm".into();
                e.command = cmd(&["wezterm", "start"]);
            }),
            entry("Alacritty", |e| {
                e.name = "Alacritty".into();
                e.startup_class = "Alacritty".into();
            }),
            entry("shell-script", |e| {
                e.name = "Some Script".into();
                e.command = cmd(&["sh", "-c", "echo"]);
            }),
            messages(),
        ]
    }

    fn win(f: impl FnOnce(&mut Window)) -> Window {
        let mut w = Window::default();
        f(&mut w);
        w
    }

    fn id_of(found: Option<&DesktopEntry>) -> Option<&str> {
        found.map(|e| e.id.as_str())
    }

    fn proc(exe: &str, argv: &[&str]) -> ProcInfo {
        ProcInfo { exe: exe.into(), argv: cmd(argv) }
    }

    // --- tier 1: the class ---

    #[test]
    fn the_class_matches_an_entry_id() {
        let es = entries();
        assert_eq!(id_of(entry_for(Some(&win(|w| w.app_id = "foot".into())), &es, None)), Some("foot"));
    }

    #[test]
    fn the_class_matches_an_id_case_folded() {
        let es = entries();
        assert_eq!(id_of(entry_for(Some(&win(|w| w.app_id = "FOOT".into())), &es, None)), Some("foot"));
    }

    #[test]
    fn the_class_matches_startup_wm_class() {
        let es = entries();
        assert_eq!(id_of(entry_for(Some(&win(|w| w.app_id = "alacritty".into())), &es, None)), Some("Alacritty"));
    }

    #[test]
    fn the_initial_class_answers_when_the_class_is_empty() {
        let es = entries();
        assert_eq!(id_of(entry_for(Some(&win(|w| w.initial_class = "firefox".into())), &es, None)), Some("firefox"));
    }

    #[test]
    fn a_reverse_dns_entry_takes_its_last_segment() {
        let es = entries();
        assert_eq!(
            id_of(entry_for(Some(&win(|w| w.app_id = "discord".into())), &es, None)),
            Some("com.discordapp.Discord")
        );
    }

    #[test]
    fn a_reverse_dns_class_takes_its_last_segment() {
        let plain = vec![entry("wezterm", |e| e.name = "WezTerm".into())];
        assert_eq!(
            id_of(entry_for(Some(&win(|w| w.app_id = "org.wezfurlong.wezterm".into())), &plain, None)),
            Some("wezterm")
        );
    }

    #[test]
    fn an_exact_id_wins_over_a_tail_match() {
        let es = entries();
        assert_eq!(
            id_of(entry_for(Some(&win(|w| w.app_id = "org.wezfurlong.wezterm".into())), &es, None)),
            Some("org.wezfurlong.wezterm")
        );
    }

    // --- tier 2: the process ---

    #[test]
    fn an_empty_class_resolves_through_the_launching_script() {
        let es = entries();
        let procs = [
            proc("/nix/store/abc-webkitgtk/bin/MiniBrowser", &["MiniBrowser", "https://messages"]),
            proc("/nix/store/xyz-bash/bin/bash", &["bash", "/home/kyandesutter/.local/bin/messages"]),
        ];
        assert_eq!(id_of(entry_for(Some(&win(|_| {})), &es, Some(&procs))), Some("messages"));
    }

    #[test]
    fn an_empty_class_resolves_through_the_exe_itself() {
        let es = entries();
        let procs = [proc("/home/kyandesutter/.local/bin/messages", &["messages"])];
        assert_eq!(id_of(entry_for(Some(&win(|_| {})), &es, Some(&procs))), Some("messages"));
    }

    #[test]
    fn a_nix_wrapper_reads_as_its_program() {
        let es = entries();
        let procs = [proc("/nix/store/q-foot/bin/.foot-wrapped", &["/run/current-system/sw/bin/foot"])];
        assert_eq!(id_of(entry_for(Some(&win(|_| {})), &es, Some(&procs))), Some("foot"));
    }

    #[test]
    fn an_ancestor_never_matches_by_basename() {
        let es = entries();
        let procs = [
            proc("/opt/thing/thing", &["thing"]),
            proc("/usr/bin/fish", &["fish"]),
            proc("/usr/bin/foot", &["foot"]),
        ];
        assert_eq!(entry_for(Some(&win(|_| {})), &es, Some(&procs)), None);
    }

    #[test]
    fn an_interpreter_basename_claims_nothing() {
        let es = entries();
        let procs = [proc("/usr/bin/sh", &["sh", "-c", "run"])];
        assert_eq!(entry_for(Some(&win(|_| {})), &es, Some(&procs)), None);
    }

    // --- Steam, as on the owner's g815 ---

    /// The shortcut Steam writes for an installed game, read ahead of Steam's
    /// own entry since ~/.local comes first.
    fn steam_entries() -> Vec<DesktopEntry> {
        vec![
            entry("STRAFTAT", |e| {
                e.icon = "steam_icon_2386720".into();
                e.command = cmd(&["steam", "steam://rungameid/2386720"]);
            }),
            entry("steam", |e| {
                e.name = "Steam".into();
                e.command = cmd(&["/usr/bin/steam", "%U"]);
            }),
        ]
    }

    #[test]
    fn steams_classless_updater_is_steam() {
        let es = steam_entries();
        let exe = "/home/kyandesutter/.local/share/Steam/ubuntu12_32/steam";
        let procs = [proc(exe, &[exe, "-srt-logger-opened"]), proc("/usr/bin/bash", &["/usr/bin/bash", "/usr/bin/steam"])];
        let w = win(|w| w.initial_title = "Updating Steam...".into());
        assert_eq!(id_of(entry_for(Some(&w), &es, Some(&procs))), Some("steam"));
    }

    #[test]
    fn steams_own_window_is_steam() {
        let es = steam_entries();
        assert_eq!(id_of(entry_for(Some(&win(|w| w.app_id = "steam".into())), &es, None)), Some("steam"));
    }

    #[test]
    fn a_steam_game_is_its_shortcut_by_its_app_id() {
        let es = steam_entries();
        let w = win(|w| w.app_id = "steam_app_2386720".into());
        assert_eq!(id_of(entry_for(Some(&w), &es, None)), Some("STRAFTAT"));
        let other = win(|w| w.app_id = "steam_app_2386721".into());
        assert_eq!(entry_for(Some(&other), &es, None), None);
    }

    // --- tier 3: the initial title ---

    #[test]
    fn an_empty_class_and_no_process_falls_to_the_initial_title() {
        let es = entries();
        assert_eq!(id_of(entry_for(Some(&win(|w| w.initial_title = "Messages".into())), &es, None)), Some("messages"));
    }

    #[test]
    fn the_current_title_alone_matches_nothing() {
        let es = entries();
        assert_eq!(entry_for(Some(&win(|_| {})), &es, None), None);
    }

    #[test]
    fn nothing_at_all_is_none() {
        let es = entries();
        assert_eq!(entry_for(Some(&win(|_| {})), &es, None), None);
        assert_eq!(entry_for(None, &es, None), None);
        assert_eq!(entry_for(Some(&win(|w| w.app_id = "foot".into())), &[], None), None);
    }

    // --- the picture ---

    #[test]
    fn an_absolute_icon_path_is_a_file_url() {
        assert_eq!(source(&messages().icon, Some(&themed)), "file:///home/kyandesutter/.local/share/icons/messages.svg");
    }

    #[test]
    fn a_theme_name_goes_through_the_theme() {
        assert_eq!(source("foot", Some(&themed)), "image://icon/foot");
        assert_eq!(source("no-such-icon", Some(&themed)), "");
        assert_eq!(source("", Some(&themed)), "");
        assert_eq!(source("file:///x.png", Some(&themed)), "file:///x.png");
    }

    // --- the /proc read ---

    #[test]
    fn the_proc_read_parses_into_depth_order() {
        let text = "4242\t1\t/usr/bin/bash\tbash\x1f/home/k/.local/bin/messages\x1f\n\
                    4242\t0\t/opt/app/app\tapp\x1f--flag\x1f\n\
                    garbage line\n\
                    17\t0\t\t\n";
        let read = parse_procs(text);
        assert_eq!(read["4242"].len(), 2);
        assert_eq!(read["4242"][0].exe, "/opt/app/app");
        assert_eq!(read["4242"][0].argv.len(), 2);
        assert_eq!(read["4242"][1].argv[1], "/home/k/.local/bin/messages");
        assert_eq!(read["17"].len(), 1);
        assert_eq!(read["17"][0].argv.len(), 0);
    }

    #[test]
    fn the_proc_command_passes_pids_as_arguments() {
        let argv = proc_command(&cmd(&["4242", "17"]));
        assert_eq!(argv[0], "sh");
        assert_eq!(argv[1], "-c");
        assert_eq!(argv[argv.len() - 2..], cmd(&["4242", "17"])[..]);
    }
}
