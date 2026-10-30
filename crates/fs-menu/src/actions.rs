//! The launcher's footer (actions.js) and the row accessory (hints.js): what
//! Enter does to the row under the cursor, the keys that always apply beside
//! it, and the muted value a row carries on its right.
//!
//! Verbs come from a node's `kind`, never from its id; a provider that knows
//! better says so in the row's own `verb`. A `dim` row gets no primary action
//! rather than a verb that would do nothing. Keys are names, one per cap.

use crate::abbrev::Source;
use crate::model::Mode;
use crate::node::{ActionEntry, Kind, Node};
use crate::providers::{FILE_KIND, FOLDER_KIND, is_wifi_network};

/// What running an action does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Run {
    /// A shell command, spawned through the compositor.
    Command(String),
    /// An in-process name, the part of an `@ipc:` action after the prefix.
    Ipc(String),
    /// What → reaches: a level by node id, or a subject's own children by
    /// its subject id (an app's windows).
    Into(String),
    /// An argument to pick from a list first, replacing the column's
    /// actions.
    Target(TargetKind),
    /// Words to type first, in the launcher's Input step; the string is
    /// its prompt.
    Input(String),
    /// A line the column shows that runs nothing.
    Note,
}

/// What a `Run::Target` action asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetKind {
    Workspace,
    /// An app to open a file with.
    App,
    /// A LocalSend peer.
    Peer,
}

impl TargetKind {
    /// The field's third token while its list is up.
    pub fn label(self) -> &'static str {
        match self {
            TargetKind::Workspace => "Workspace",
            TargetKind::App => "Open with",
            TargetKind::Peer => "Send to",
        }
    }
}

impl Run {
    pub fn parse(action: &str) -> Run {
        match action.strip_prefix("@ipc:") {
            Some(name) => Run::Ipc(name.to_owned()),
            None => Run::Command(action.to_owned()),
        }
    }
}

/// Something done to a subject. The first of a subject's list runs on
/// Enter, the second on Shift+Enter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    pub id: String,
    pub label: String,
    pub icon: Option<String>,
    pub run: Run,
    /// Two runs: the first arms, the second runs.
    pub confirm: bool,
    pub keep_open: bool,
    /// Drawn with a `destructive` icon, never a filled row.
    pub destructive: bool,
    /// The paste chord goes out once the launcher is gone.
    pub paste_after: bool,
}

impl Action {
    pub fn new(id: &str, label: impl Into<String>, run: Run) -> Action {
        Action {
            id: id.to_owned(),
            label: label.into(),
            icon: None,
            run,
            confirm: false,
            keep_open: false,
            destructive: false,
            paste_after: false,
        }
    }

    /// An action entry laid over this built-in, key by key.
    fn overlaid(mut self, e: &ActionEntry) -> Action {
        if let Some(l) = &e.label {
            self.label = l.clone();
        }
        if e.icon.is_some() {
            self.icon = e.icon.clone();
        }
        if let Some(a) = &e.action {
            self.run = Run::parse(a);
        }
        self.confirm = e.confirm.unwrap_or(self.confirm);
        self.keep_open = e.keep_open.unwrap_or(self.keep_open);
        self.destructive = e.destructive.unwrap_or(self.destructive);
        self.paste_after = e.paste_after.unwrap_or(self.paste_after);
        self
    }

    fn from_entry(e: &ActionEntry) -> Option<Action> {
        let mut a = Action::new(&e.id, e.label.clone().unwrap_or_else(|| e.id.clone()), Run::parse(e.action.as_deref()?));
        a.icon = e.icon.clone();
        a.confirm = e.confirm.unwrap_or(false);
        a.keep_open = e.keep_open.unwrap_or(false);
        a.destructive = e.destructive.unwrap_or(false);
        a.paste_after = e.paste_after.unwrap_or(false);
        Some(a)
    }
}

/// A row as something with actions: the node and what the shell knows
/// about it that the node does not carry.
#[derive(Clone, Copy, Debug)]
pub struct Subject<'a> {
    pub node: &'a Node,
    /// Open windows of an app subject.
    pub windows: usize,
    /// The machine has a discrete GPU to offer an app.
    pub discrete_gpu: bool,
    pub picker_select: bool,
    /// The abbreviation the field's query is for this subject, and its
    /// source.
    pub abbrev: Option<(&'a str, Source)>,
    /// Pinned to the root's `Recent`.
    pub pinned: bool,
}

/// A row's kind for one compositor window, its id in the node's `path`.
pub const WINDOW_KIND: &str = "window";

impl<'a> Subject<'a> {
    pub fn of(node: &'a Node) -> Subject<'a> {
        Subject { node, windows: 0, discrete_gpu: false, picker_select: false, abbrev: None, pinned: false }
    }

    /// A dim note, a keybind, an unavailable row: Enter, Shift+Enter and a
    /// click do nothing on it, and its column still lists what it has.
    pub fn inert(&self) -> bool {
        self.node.dim == Some(true) || self.node.kind == Kind::Note
    }
}

/// The id a pick, an abbreviation and `Recent` know a subject by:
/// `app:<desktop id>` for an app, the node id for everything else.
pub fn subject_id(node: &Node) -> String {
    match &node.entry {
        Some(e) if node.kind == Kind::App => format!("app:{}", e.id),
        _ => node.id.clone(),
    }
}

/// A single-quoted shell word.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn copy(id: &str, label: &str, text: &str) -> Action {
    Action::new(id, label, Run::Command(format!("wl-copy -- {}", shell_quote(text))))
}

/// What a `file://` URI escapes in a path: everything but the unreserved
/// characters and `/`. A comma too, which `dbus-send` would split on.
const URI_PATH: &percent_encoding::AsciiSet =
    &percent_encoding::NON_ALPHANUMERIC.remove(b'/').remove(b'-').remove(b'_').remove(b'.').remove(b'~');

/// The file manager's own window on `path` with it selected, over
/// org.freedesktop.FileManager1; with no file manager on the bus, its
/// folder through xdg-open.
fn show_in_folder(path: &str) -> String {
    let uri = format!("file://{}", percent_encoding::utf8_percent_encode(path, URI_PATH));
    let parent = match path.trim_end_matches('/').rsplit_once('/') {
        Some(("", _)) | None => "/",
        Some((p, _)) => p,
    };
    format!(
        "dbus-send --session --print-reply --dest=org.freedesktop.FileManager1 /org/freedesktop/FileManager1 org.freedesktop.FileManager1.ShowItems {} string: >/dev/null 2>&1 || xdg-open {}",
        shell_quote(&format!("array:string:{uri}")),
        shell_quote(parent)
    )
}

/// The kind's own actions, before the node's.
fn builtins(s: &Subject) -> Vec<Action> {
    let n = s.node;
    let mut out = Vec::new();
    match &n.kind {
        Kind::App => {
            let Some(e) = &n.entry else { return out };
            out.push(Action::new("open", "Open", Run::Ipc(format!("app.open:{}", e.id))));
            out.push(Action::new("new-window", "New window", Run::Ipc(format!("app.new:{}", e.id))));
            for (key, name) in &e.actions {
                out.push(Action::new(&format!("desktop:{key}"), name.clone(), Run::Ipc(format!("app.action:{}:{key}", e.id))));
            }
            if s.windows > 0 {
                let label = if s.windows == 1 { "Show its window".to_owned() } else { format!("Show its {} windows", s.windows) };
                out.push(Action::new("windows", label, Run::Into(subject_id(n))));
            }
            if s.discrete_gpu {
                out.push(Action::new("discrete-gpu", "Open on discrete GPU", Run::Ipc(format!("app.gpu:{}", e.id))));
            }
            if s.windows > 0 {
                out.push(Action::new("move-to-workspace", "Move to workspace\u{2026}", Run::Target(TargetKind::Workspace)));
            }
            if !e.path.is_empty() {
                out.push(copy("copy-path", "Copy desktop file path", &e.path));
            }
        }
        Kind::Other(k) if k == WINDOW_KIND => {
            let id = &n.path;
            out.push(Action::new("focus", "Focus", Run::Ipc(format!("window.focus:{id}"))));
            let mut close = Action::new("close", "Close", Run::Ipc(format!("window.close:{id}")));
            close.destructive = true;
            out.push(close);
            out.push(Action::new("move-to-workspace", "Move to workspace\u{2026}", Run::Target(TargetKind::Workspace)));
            out.push(Action::new("float", "Toggle floating", Run::Ipc(format!("window.float:{id}"))));
            out.push(Action::new("fullscreen", "Fullscreen", Run::Ipc(format!("window.fullscreen:{id}"))));
        }
        Kind::Submenu | Kind::Provider | Kind::Link => {
            let to = n.target.clone().filter(|_| n.kind == Kind::Link).unwrap_or_else(|| n.id.clone());
            out.push(Action::new("open", "Open", Run::Into(to)));
            let mut pin = if s.pinned {
                Action::new("unpin-recent", "Unpin from Recent", Run::Ipc(format!("recent.unpin:{}", subject_id(n))))
            } else {
                Action::new("pin-recent", "Pin to Recent", Run::Ipc(format!("recent.pin:{}", subject_id(n))))
            };
            pin.keep_open = true;
            out.push(pin);
        }
        Kind::Other(k) if k == FILE_KIND => {
            out.push(Action::new("open", "Open", Run::Command(format!("xdg-open {}", shell_quote(&n.path)))));
            out.push(Action::new("open-with", "Open with\u{2026}", Run::Target(TargetKind::App)));
            out.push(Action::new("show-in-folder", "Show in folder", Run::Command(show_in_folder(&n.path))));
            out.push(copy("copy-path", "Copy path", &n.path));
            out.push(Action::new("send-to", "Send to\u{2026}", Run::Target(TargetKind::Peer)));
        }
        Kind::Other(k) if k == FOLDER_KIND => {
            out.push(Action::new("open", "Open in the file manager", Run::Command(format!("xdg-open {}", shell_quote(&n.path)))));
            out.push(copy("copy-path", "Copy path", &n.path));
            out.push(Action::new("contents", "Open folder contents", Run::Into(subject_id(n))));
        }
        Kind::Image => {
            let label = if s.picker_select { "Choose" } else { "Set wallpaper" };
            out.push(Action::new("set", label, Run::Ipc(format!("wallpaper.set:{}", n.path))));
            out.push(Action::new("open-file", "Open file", Run::Command(format!("xdg-open {}", shell_quote(&n.path)))));
            out.push(copy("copy-path", "Copy path", &n.path));
        }
        Kind::Action => {
            let Some(action) = n.action.as_deref() else { return out };
            let label = match n.verb.as_deref().filter(|v| !v.is_empty()) {
                Some(v) => v,
                None if n.checked.is_some() => "Toggle",
                None => "Run",
            };
            let mut run = Action::new("run", label, Run::parse(action));
            run.confirm = n.confirm == Some(true);
            run.keep_open = n.keep_open == Some(true);
            run.paste_after = n.paste_after;
            let plain_command = matches!(run.run, Run::Command(_)) && n.verb.is_none() && n.checked.is_none();
            let copy_only = n.paste_after.then(|| Action { id: "copy-only".into(), label: "Copy only".into(), paste_after: false, ..run.clone() });
            out.push(run);
            if let Some(alt) = n.alternate.as_deref() {
                let mut a = Action::new("alternate", n.alternate_label.clone().unwrap_or_else(|| "Alternate".into()), Run::parse(alt));
                a.keep_open = n.keep_open == Some(true);
                out.push(a);
            }
            out.extend(copy_only);
            if plain_command {
                out.push(copy("copy-command", "Copy command", action));
            }
        }
        _ => {}
    }
    out
}

/// What the column offers for the abbreviation that put the subject where
/// it is: a configured one is settings.json's, so only a line saying so.
fn abbreviation_actions(s: &Subject) -> Vec<Action> {
    let Some((q, source)) = s.abbrev else { return Vec::new() };
    let ipc = |verb: &str| Run::Ipc(format!("abbrev.{verb}:{q}"));
    let keep = |mut a: Action| {
        a.keep_open = true;
        a
    };
    match source {
        Source::Configured => vec![Action::new("abbrev-configured", "Set in settings.json", Run::Note)],
        Source::Pinned | Source::Learned => {
            let mut out = vec![
                keep(Action::new("abbrev-forget", format!("Forget \u{201C}{q}\u{201D}"), ipc("forget"))),
                Action::new("abbrev-edit", "Edit abbreviation", Run::Input(format!("Abbreviation for {}", s.node.label))),
            ];
            if source == Source::Learned {
                out.push(keep(Action::new("abbrev-pin", format!("Pin \u{201C}{q}\u{201D}"), ipc("pin"))));
            }
            out
        }
    }
}

/// A subject's actions in order: the kind's, then the node's own (a
/// provider's and `menu.jsonc`'s, where an entry with a built-in's id
/// overrides it and `hidden: true` removes it), then its abbreviation's.
/// Built on Tab, never per keystroke.
pub fn actions_for(s: &Subject) -> Vec<Action> {
    let mut out = builtins(s);
    for e in &s.node.actions {
        match out.iter().position(|a| a.id == e.id) {
            Some(i) if e.hidden == Some(true) => {
                out.remove(i);
            }
            Some(i) => out[i] = out[i].clone().overlaid(e),
            None if e.hidden == Some(true) => {}
            None => out.extend(Action::from_entry(e)),
        }
    }
    // After the node's own, so a network with no Forget keeps the panel on
    // Shift+Enter.
    if is_wifi_network(s.node) {
        out.push(Action::new("details", "Details", Run::Into(subject_id(s.node))));
    }
    out.extend(abbreviation_actions(s));
    out
}

fn runnable(s: &Subject) -> impl Iterator<Item = Action> {
    let all = if s.inert() { Vec::new() } else { actions_for(s) };
    all.into_iter().filter(|a| a.run != Run::Note)
}

/// The action Enter runs, `None` on an inert subject.
pub fn first_action(s: &Subject) -> Option<Action> {
    runnable(s).next()
}

/// The action Shift+Enter runs: the second, else the first.
pub fn second_action(s: &Subject) -> Option<Action> {
    let mut all = runnable(s);
    let first = all.next();
    all.next().or(first)
}

/// What → runs on the subject: its first action stepping into something.
pub fn into_action(s: &Subject) -> Option<Action> {
    runnable(s).find(|a| matches!(a.run, Run::Into(_)))
}

/// The positions of the labels holding every word of `query`,
/// in order.
pub fn filter(labels: &[&str], query: &str) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    labels
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            let l = l.to_lowercase();
            words.iter().all(|w| l.contains(w.as_str()))
        })
        .map(|(i, _)| i)
        .collect()
}

/// One legend entry: the keys of a chord (or keys offered side by side) and
/// the outcome they name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyHint {
    pub keys: Vec<&'static str>,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionBar {
    pub primary: Option<KeyHint>,
    pub hints: Vec<KeyHint>,
}

/// Which variant the wallpaper route's Tab would show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    Dark,
    Light,
}

/// What the menu hands the footer.
#[derive(Clone, Debug)]
pub struct ActionCtx<'a> {
    pub mode: Mode,
    pub node: Option<&'a Node>,
    pub at_root: bool,
    pub picker_select: bool,
    /// The variant Tab would switch to, `None` wherever Tab does nothing.
    pub variant_switch: Option<Variant>,
    pub confirming: bool,
    /// The row's second action, "" for none.
    pub alternate_label: &'a str,
}

impl Default for ActionCtx<'_> {
    fn default() -> Self {
        Self {
            mode: Mode::Menu,
            node: None,
            at_root: false,
            picker_select: false,
            variant_switch: None,
            confirming: false,
            alternate_label: "",
        }
    }
}

const KEY_ENTER: &[&str] = &["Enter"];
const KEY_ESC: &[&str] = &["Esc"];
const KEY_TAB: &[&str] = &["Tab"];
const KEY_SHIFT_ENTER: &[&str] = &["Shift", "Enter"];

fn hint(keys: &[&'static str], label: impl Into<String>) -> KeyHint {
    KeyHint { keys: keys.to_vec(), label: label.into() }
}

pub fn primary_action(c: &ActionCtx) -> Option<KeyHint> {
    if c.mode == Mode::Input {
        return Some(hint(KEY_ENTER, "Submit"));
    }
    let node = c.node?;
    if node.dim == Some(true) {
        return None;
    }
    if c.confirming {
        return Some(hint(KEY_ENTER, format!("Confirm {}", node.label)));
    }
    if let Some(verb) = node.verb.as_deref().filter(|v| !v.is_empty()) {
        return Some(hint(KEY_ENTER, verb));
    }
    let label = match node.kind {
        Kind::Other(ref k) if k == "option" => "Select",
        Kind::Image if c.picker_select => "Choose",
        Kind::Image => "Set wallpaper",
        Kind::App | Kind::Submenu | Kind::Provider | Kind::Link => "Open",
        Kind::Action => "Run",
        _ => return None,
    };
    Some(hint(KEY_ENTER, label))
}

/// The keys beside Enter. Escape reads Back wherever there is a level to pop
/// and Close at the root, since those are two different outcomes.
pub fn hints(c: &ActionCtx) -> Vec<KeyHint> {
    if c.mode == Mode::Input {
        return vec![hint(KEY_ESC, "Cancel")];
    }
    let mut out = Vec::new();
    match c.variant_switch {
        Some(Variant::Light) => out.push(hint(KEY_TAB, "Show light")),
        Some(Variant::Dark) => out.push(hint(KEY_TAB, "Show dark")),
        None => {}
    }
    if !c.alternate_label.is_empty() && !c.confirming {
        out.push(hint(KEY_SHIFT_ENTER, c.alternate_label));
    }
    if c.mode == Mode::Select {
        out.push(hint(KEY_ESC, "Cancel"));
    } else {
        out.push(hint(KEY_ESC, if c.at_root { "Close" } else { "Back" }));
    }
    out
}

pub fn action_bar(c: &ActionCtx) -> ActionBar {
    ActionBar { primary: primary_action(c), hints: hints(c) }
}

/// The chords that summon a route directly, as a reader would type them.
pub const ROUTE_CHORDS: &[(&str, &str)] = &[
    ("apps", "Super+Alt+Space"),
    ("calc", "Super+Ctrl+Q"),
    ("capture", "Super+Ctrl+C"),
    ("clipboard", "Super+Ctrl+V"),
    ("emoji", "Super+Ctrl+E"),
    ("keybinds", "Super+K"),
    ("reminder", "Super+Ctrl+R"),
    ("share", "Super+Ctrl+S"),
    ("system", "Super+Escape"),
    ("theme", "Super+Shift+Ctrl+Space"),
    ("toggles", "Super+Ctrl+O"),
    ("wallpaper", "Super+Ctrl+Space"),
];

fn lookup(table: &[(&str, &'static str)], id: &str) -> &'static str {
    table.iter().find(|(k, _)| *k == id).map_or("", |(_, v)| v)
}

pub fn chord_for(id: &str) -> &'static str {
    lookup(ROUTE_CHORDS, id)
}

/// A count only for a provider node, whose children are a listing; zero reads
/// as no hint at all.
pub fn count_for(node: Option<&Node>) -> String {
    match node {
        Some(n) if n.kind == Kind::Provider && !n.child_ids.is_empty() => n.child_ids.len().to_string(),
        _ => String::new(),
    }
}

/// The chord wins over the count.
pub fn hint_for(node: Option<&Node>) -> String {
    let Some(n) = node else { return String::new() };
    let chord = chord_for(&n.id);
    if chord.is_empty() { count_for(node) } else { chord.to_string() }
}

/// The row's trailing value: its own (a level, a source), else a route's
/// `prefix`, chord or count. An app or an action carries none of its own.
pub fn accessory_for(node: Option<&Node>) -> String {
    let Some(n) = node else { return String::new() };
    if let Some(meta) = n.meta.as_deref().filter(|m| !m.is_empty()) {
        return meta.to_string();
    }
    match n.kind {
        Kind::Submenu | Kind::Provider | Kind::Link => match n.prefix.as_deref().filter(|p| !p.is_empty()) {
            Some(prefix) => prefix.to_owned(),
            None => hint_for(node).replace('+', " "),
        },
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(h: &[KeyHint]) -> Vec<&str> {
        h.iter().map(|k| k.label.as_str()).collect()
    }

    #[test]
    fn primary_by_kind() {
        let app = Node::new("apps.x", "X", Kind::App);
        let act = Node::new("a", "Lock", Kind::Action);
        let img = Node::new("i", "i", Kind::Image);
        let opt = Node::new("select.0", "o", Kind::Other("option".into()));
        let ctx = |n| ActionCtx { node: Some(n), ..Default::default() };
        assert_eq!(primary_action(&ctx(&app)).unwrap().label, "Open");
        assert_eq!(primary_action(&ctx(&act)).unwrap().label, "Run");
        assert_eq!(primary_action(&ctx(&img)).unwrap().label, "Set wallpaper");
        assert_eq!(primary_action(&ActionCtx { picker_select: true, ..ctx(&img) }).unwrap().label, "Choose");
        assert_eq!(primary_action(&ctx(&opt)).unwrap().label, "Select");
        assert_eq!(primary_action(&ActionCtx { confirming: true, ..ctx(&act) }).unwrap().label, "Confirm Lock");
        let verb = Node { verb: Some("Paste".into()), ..act.clone() };
        assert_eq!(primary_action(&ctx(&verb)).unwrap().label, "Paste");
        assert_eq!(primary_action(&ctx(&Node::note("n", "none"))), None);
        assert_eq!(primary_action(&ActionCtx::default()), None);
        let input = ActionCtx { mode: Mode::Input, ..Default::default() };
        assert_eq!(primary_action(&input).unwrap().label, "Submit");
        assert_eq!(labels(&hints(&input)), ["Cancel"]);
    }

    #[test]
    fn hints_by_context() {
        let app = Node::new("apps.x", "X", Kind::App);
        assert_eq!(labels(&hints(&ActionCtx { at_root: true, ..Default::default() })), ["Close"]);
        assert_eq!(labels(&hints(&ActionCtx::default())), ["Back"]);
        assert_eq!(labels(&hints(&ActionCtx { mode: Mode::Select, ..Default::default() })), ["Cancel"]);
        let c = ActionCtx { node: Some(&app), variant_switch: Some(Variant::Light), alternate_label: "New window", ..Default::default() };
        assert_eq!(labels(&hints(&c)), ["Show light", "New window", "Back"]);
        assert_eq!(hints(&c)[1].keys, ["Shift", "Enter"]);
        assert_eq!(labels(&hints(&ActionCtx { confirming: true, ..c })), ["Show light", "Back"]);
    }

    #[test]
    fn accessories() {
        let app = Node::new("apps.x", "X", Kind::App);
        assert_eq!(accessory_for(Some(&app)), "");
        assert_eq!(accessory_for(Some(&Node::new("a", "a", Kind::Action))), "");
        let emoji = Node { prefix: Some(":e".into()), ..Node::new("emoji", "Emoji", Kind::Provider) };
        assert_eq!(accessory_for(Some(&emoji)), ":e");
        let clip = Node::new("clipboard", "Clipboard", Kind::Provider);
        assert_eq!(accessory_for(Some(&clip)), "Super Ctrl V");
        let tray = Node { child_ids: vec!["a".into(), "b".into()], ..Node::new("tray", "Tray", Kind::Provider) };
        assert_eq!(accessory_for(Some(&tray)), "2");
        let meta = Node { meta: Some("42%".into()), ..app };
        assert_eq!(accessory_for(Some(&meta)), "42%");
        assert_eq!(accessory_for(None), "");
    }
}
