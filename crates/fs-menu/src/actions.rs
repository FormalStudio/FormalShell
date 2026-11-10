//! The launcher's footer and the row accessory: what
//! Enter does to the row under the cursor, the keys that always apply beside
//! it, and the muted value a row carries on its right.
//!
//! Verbs come from a node's `kind`, never from its id; a provider that knows
//! better says so in the row's own `verb`. A `dim` row gets no primary action
//! rather than a verb that would do nothing. Keys are names, one per cap.

use crate::abbrev::Source;
use crate::intent::Intent;
use crate::model::Mode;
use crate::node::{ActionEntry, Kind, Node};
use crate::providers::{clipboard_entry_id, is_wifi_network};

/// What running an action does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Run {
    /// One of the shell's own IPC calls, answered in process.
    Ipc(IpcCall),
    /// One of the launcher's own verbs.
    Intent(Intent),
    /// A program and its arguments, spawned through the compositor.
    Spawn(Vec<String>),
    /// A line of the user's own, spawned through the compositor as `sh -c`.
    Shell(String),
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
    /// An action as menu.jsonc, settings.json and the default menu spell
    /// it: `@ipc:<target>.<function>` with `:`-separated arguments, the
    /// same call written out as `formalshell-ipc call <target> <function>
    /// <args>` in plain words, or else a shell line.
    pub fn parse(text: &str) -> Run {
        let call = match text.strip_prefix("@ipc:") {
            Some(call) => IpcCall::parse(call),
            None => match self_call(text) {
                Some(call) => call,
                None => return Run::Shell(text.to_owned()),
            },
        };
        Run::from(call)
    }
}

/// The action as it would be written: an `@ipc:` call, a shell line, a
/// program's words.
impl std::fmt::Display for Run {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Run::Ipc(call) => {
                write!(f, "@ipc:{call}")?;
                call.args.iter().try_for_each(|a| write!(f, ":{a}"))
            }
            Run::Intent(intent) => std::fmt::Display::fmt(intent, f),
            Run::Spawn(argv) => f.write_str(&argv.join(" ")),
            Run::Shell(line) => f.write_str(line),
            Run::Into(id) => write!(f, "into {id}"),
            Run::Target(kind) => write!(f, "target {}", kind.label()),
            Run::Input(prompt) => write!(f, "input {prompt}"),
            Run::Note => f.write_str("note"),
        }
    }
}

/// A call naming one of the launcher's own verbs is that verb; one naming
/// it with the wrong arguments stays a call, refused where it is checked.
impl From<IpcCall> for Run {
    fn from(call: IpcCall) -> Run {
        match Intent::from_call(&call) {
            Some(Ok(intent)) => Run::Intent(intent),
            _ => Run::Ipc(call),
        }
    }
}

impl From<Intent> for Run {
    fn from(intent: Intent) -> Run {
        Run::Intent(intent)
    }
}

impl<'de> serde::Deserialize<'de> for Run {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Run, D::Error> {
        Ok(Run::parse(&String::deserialize(d)?))
    }
}

/// `formalshell-ipc call` and its words, when none of them is something
/// a shell would read differently from the word itself.
fn self_call(text: &str) -> Option<IpcCall> {
    let mut words = text.split_whitespace();
    if words.next()? != "formalshell-ipc" || words.next()? != "call" {
        return None;
    }
    let plain = |w: &str| !w.contains(|c: char| "'\"\\$`;&|<>(){}[]*?~#!".contains(c));
    let words: Vec<&str> = words.collect();
    if words.len() < 2 || !words.iter().all(|w| plain(w)) {
        return None;
    }
    Some(IpcCall { target: words[0].to_owned(), function: words[1].to_owned(), args: words[2..].iter().map(|w| (*w).to_owned()).collect() })
}

/// One call to a target the shell serves, as `formalshell-ipc call
/// <target> <function> <args>` sends it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IpcCall {
    pub target: String,
    pub function: String,
    pub args: Vec<String>,
}

impl IpcCall {
    pub fn new(target: &str, function: &str) -> IpcCall {
        IpcCall { target: target.to_owned(), function: function.to_owned(), args: Vec::new() }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> IpcCall {
        self.args.push(arg.into());
        self
    }

    /// Argument `i`, "" past the last.
    pub fn at(&self, i: usize) -> &str {
        self.args.get(i).map_or("", String::as_str)
    }

    /// The part of an `@ipc:` action after the prefix: `<target>.<function>`
    /// and its `:`-separated arguments.
    pub fn parse(text: &str) -> IpcCall {
        let mut parts = text.split(':');
        let name = parts.next().unwrap_or_default();
        let (target, function) = name.split_once('.').unwrap_or((name, ""));
        IpcCall { target: target.to_owned(), function: function.to_owned(), args: parts.map(str::to_owned).collect() }
    }

    /// The arguments for a function taking `n`: a colon past the `n`th
    /// argument belonged to the last one (an SSID, a path, a plugin's line).
    pub fn fitted(mut self, n: usize) -> IpcCall {
        if n > 0 && self.args.len() > n {
            let tail = self.args.split_off(n - 1).join(":");
            self.args.push(tail);
        }
        self
    }
}

impl std::fmt::Display for IpcCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.target, self.function)
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
            self.run = a.clone();
        }
        self.confirm = e.confirm.unwrap_or(self.confirm);
        self.keep_open = e.keep_open.unwrap_or(self.keep_open);
        self.destructive = e.destructive.unwrap_or(self.destructive);
        self.paste_after = e.paste_after.unwrap_or(self.paste_after);
        self
    }

    fn from_entry(e: &ActionEntry) -> Option<Action> {
        let mut a = Action::new(&e.id, e.label.clone().unwrap_or_else(|| e.id.clone()), e.action.clone()?);
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
    /// The fspec package id an Uninstall of this app takes out, once
    /// `fspec app owner` said this host's spec holds it.
    pub removable: Option<&'a str>,
    /// A wallpaper tile holding one picture, which can fill one mode's
    /// slot alone.
    pub single: bool,
}

impl<'a> Subject<'a> {
    pub fn of(node: &'a Node) -> Subject<'a> {
        Subject { node, windows: 0, discrete_gpu: false, picker_select: false, abbrev: None, pinned: false, removable: None, single: false }
    }

    /// A dim note, a keybind, an unavailable row: Enter, Shift+Enter and a
    /// click do nothing on it, and its column still lists what it has.
    pub fn inert(&self) -> bool {
        self.node.dim == Some(true) || self.node.kind == Kind::Note
    }
}

/// What a pick, an abbreviation and `Recent` know a subject by. The last
/// two name subjects outside the tree, so `Recent` rebuilds them off the
/// id; state.json keeps it as text.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SubjectId {
    /// An app, by desktop id: `app:<id>`.
    App(String),
    /// A file or folder, by absolute path: `file:<path>`.
    File(String),
    /// An emoji, by itself: `emoji:<char>`.
    Emoji(String),
    /// Anything else, by node id.
    Node(String),
}

impl SubjectId {
    pub fn parse(id: &str) -> SubjectId {
        if let Some(app) = id.strip_prefix("app:") {
            SubjectId::App(app.to_owned())
        } else if let Some(path) = id.strip_prefix("file:") {
            SubjectId::File(path.to_owned())
        } else if let Some(ch) = id.strip_prefix("emoji:") {
            SubjectId::Emoji(ch.to_owned())
        } else {
            SubjectId::Node(id.to_owned())
        }
    }
}

impl std::fmt::Display for SubjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubjectId::App(id) => write!(f, "app:{id}"),
            SubjectId::File(path) => write!(f, "file:{path}"),
            SubjectId::Emoji(ch) => write!(f, "emoji:{ch}"),
            SubjectId::Node(id) => f.write_str(id),
        }
    }
}

pub fn subject_id(node: &Node) -> SubjectId {
    match (&node.kind, &node.entry) {
        (Kind::App, Some(e)) => SubjectId::App(e.id.clone()),
        (Kind::File | Kind::Folder, _) => SubjectId::File(node.path.clone()),
        (Kind::Emoji, _) => match crate::providers::emoji_of(node) {
            Some(ch) => SubjectId::Emoji(ch.to_owned()),
            None => SubjectId::Node(node.id.clone()),
        },
        _ => SubjectId::Node(node.id.clone()),
    }
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// `text` onto the clipboard.
pub fn copy_run(text: &str) -> Run {
    Run::Spawn(vec!["wl-copy".into(), "--".into(), text.into()])
}

fn copy(id: &str, label: &str, text: &str) -> Action {
    Action::new(id, label, copy_run(text))
}

fn xdg_open(path: &str) -> Run {
    Run::Spawn(vec!["xdg-open".into(), path.into()])
}

/// What a `file://` URI escapes in a path: everything but the unreserved
/// characters and `/`. A comma too, which `dbus-send` would split on.
const URI_PATH: &percent_encoding::AsciiSet =
    &percent_encoding::NON_ALPHANUMERIC.remove(b'/').remove(b'-').remove(b'_').remove(b'.').remove(b'~');

/// The file manager's own window on `path` with it selected, over
/// org.freedesktop.FileManager1; with no file manager on the bus, its
/// folder through xdg-open.
fn show_in_folder(path: &str) -> Run {
    let uri = format!("file://{}", percent_encoding::utf8_percent_encode(path, URI_PATH));
    let parent = match path.trim_end_matches('/').rsplit_once('/') {
        Some(("", _)) | None => "/",
        Some((p, _)) => p,
    };
    Run::Shell(format!(
        "dbus-send --session --print-reply --dest=org.freedesktop.FileManager1 /org/freedesktop/FileManager1 org.freedesktop.FileManager1.ShowItems {} string: >/dev/null 2>&1 || xdg-open {}",
        shell_quote(&format!("array:string:{uri}")),
        shell_quote(parent)
    ))
}

/// The kind's own actions, before the node's.
fn builtins(s: &Subject) -> Vec<Action> {
    let n = s.node;
    let mut out = Vec::new();
    match &n.kind {
        Kind::App => {
            let Some(e) = &n.entry else { return out };
            out.push(Action::new("open", "Open", Intent::AppOpen(e.id.clone()).into()));
            out.push(Action::new("new-window", "New window", Intent::AppNew(e.id.clone()).into()));
            for (key, name) in &e.actions {
                out.push(Action::new(&format!("desktop:{key}"), name.clone(), Intent::AppAction(e.id.clone(), key.clone()).into()));
            }
            if s.windows > 0 {
                let label = if s.windows == 1 { "Show its window".to_owned() } else { format!("Show its {} windows", s.windows) };
                out.push(Action::new("windows", label, Run::Into(subject_id(n).to_string())));
            }
            if s.discrete_gpu {
                out.push(Action::new("discrete-gpu", "Open on discrete GPU", Intent::AppGpu(e.id.clone()).into()));
            }
            if s.windows > 0 {
                out.push(Action::new("move-to-workspace", "Move to workspace\u{2026}", Run::Target(TargetKind::Workspace)));
            }
            if !e.path.is_empty() {
                out.push(copy("copy-path", "Copy desktop file path", &e.path));
            }
        }
        Kind::Window => {
            let id = &n.path;
            out.push(Action::new("focus", "Focus", Intent::WindowFocus(id.clone()).into()));
            let mut close = Action::new("close", "Close", Intent::WindowClose(id.clone()).into());
            close.destructive = true;
            out.push(close);
            out.push(Action::new("move-to-workspace", "Move to workspace\u{2026}", Run::Target(TargetKind::Workspace)));
            out.push(Action::new("float", "Toggle floating", Intent::WindowFloat(id.clone()).into()));
            out.push(Action::new("fullscreen", "Fullscreen", Intent::WindowFullscreen(id.clone()).into()));
        }
        Kind::Submenu | Kind::Provider | Kind::Link => {
            let to = n.target.clone().filter(|_| n.kind == Kind::Link).unwrap_or_else(|| n.id.clone());
            out.push(Action::new("open", "Open", Run::Into(to)));
            let mut pin = if s.pinned {
                Action::new("unpin-recent", "Unpin from Recent", Intent::RecentUnpin(subject_id(n).to_string()).into())
            } else {
                Action::new("pin-recent", "Pin to Recent", Intent::RecentPin(subject_id(n).to_string()).into())
            };
            pin.keep_open = true;
            out.push(pin);
        }
        Kind::File => {
            out.push(Action::new("open", "Open", xdg_open(&n.path)));
            out.push(Action::new("open-with", "Open with\u{2026}", Run::Target(TargetKind::App)));
            out.push(Action::new("show-in-folder", "Show in folder", show_in_folder(&n.path)));
            out.push(copy("copy-path", "Copy path", &n.path));
            out.push(Action::new("send-to", "Send to\u{2026}", Run::Target(TargetKind::Peer)));
        }
        Kind::Folder => {
            out.push(Action::new("open", "Open in the file manager", xdg_open(&n.path)));
            out.push(copy("copy-path", "Copy path", &n.path));
            out.push(Action::new("contents", "Open folder contents", Run::Into(subject_id(n).to_string())));
        }
        Kind::Package => {
            if let Some(action) = &n.action {
                let mut install = Action::new("install", "Install", action.clone());
                install.keep_open = true;
                out.push(install);
            }
            if let Some(run) = &n.alternate {
                out.push(Action::new("run-once", "Run once", run.clone()));
            }
        }
        Kind::Image => {
            let label = if s.picker_select { "Choose" } else { "Set wallpaper" };
            out.push(Action::new("set", label, Intent::WallpaperSet(n.path.clone()).into()));
            // The sky cell is drawn by the shell: no file to open or copy.
            if !n.path.starts_with('/') {
                return out;
            }
            if s.single && !s.picker_select {
                out.push(Action::new("light-only", "Use in light mode only", Intent::WallpaperSetFor("light".into(), n.path.clone()).into()));
                out.push(Action::new("dark-only", "Use in dark mode only", Intent::WallpaperSetFor("dark".into(), n.path.clone()).into()));
            }
            out.push(Action::new("open-file", "Open file", xdg_open(&n.path)));
            out.push(copy("copy-path", "Copy path", &n.path));
        }
        Kind::Action | Kind::Emoji => {
            let Some(action) = &n.action else { return out };
            let label = match n.verb.as_deref().filter(|v| !v.is_empty()) {
                Some(v) => v,
                None if n.checked.is_some() => "Toggle",
                None => "Run",
            };
            let mut run = Action::new("run", label, action.clone());
            run.confirm = n.confirm == Some(true);
            run.keep_open = n.keep_open == Some(true);
            run.paste_after = n.paste_after;
            let line = match &run.run {
                Run::Shell(line) if n.verb.is_none() && n.checked.is_none() => Some(line.clone()),
                _ => None,
            };
            let copy_only = n.paste_after.then(|| Action { id: "copy-only".into(), label: "Copy only".into(), paste_after: false, ..run.clone() });
            out.push(run);
            if let Some(alt) = &n.alternate {
                let mut a = Action::new("alternate", n.alternate_label.clone().unwrap_or_else(|| "Alternate".into()), alt.clone());
                a.keep_open = n.keep_open == Some(true);
                out.push(a);
            }
            out.extend(copy_only);
            if clipboard_entry_id(n).is_some() {
                out.push(Action::new("send-to", "Send to\u{2026}", Run::Target(TargetKind::Peer)));
            }
            if let Some(line) = line {
                out.push(copy("copy-command", "Copy command", &line));
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
    let forget = Run::Intent(Intent::AbbrevForget(q.to_owned()));
    let pin = Run::Intent(Intent::AbbrevPin(q.to_owned()));
    let keep = |mut a: Action| {
        a.keep_open = true;
        a
    };
    match source {
        Source::Configured => vec![Action::new("abbrev-configured", "Set in settings.json", Run::Note)],
        Source::Pinned | Source::Learned => {
            let mut out = vec![
                keep(Action::new("abbrev-forget", format!("Forget \u{201C}{q}\u{201D}"), forget)),
                Action::new("abbrev-edit", "Edit abbreviation", Run::Input(format!("Abbreviation for {}", s.node.label))),
            ];
            if source == Source::Learned {
                out.push(keep(Action::new("abbrev-pin", format!("Pin \u{201C}{q}\u{201D}"), pin)));
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
        out.push(Action::new("details", "Details", Run::Into(subject_id(s.node).to_string())));
    }
    out.extend(abbreviation_actions(s));
    out.extend(uninstall_action(s));
    out
}

/// An app's last action, once fspec said this host's spec holds it.
pub fn uninstall_action(s: &Subject) -> Option<Action> {
    let id = s.removable.filter(|_| s.node.kind == Kind::App)?;
    let mut uninstall = Action::new("uninstall", format!("Uninstall {}", s.node.label), Intent::PackageUninstall(id.to_owned()).into());
    uninstall.confirm = true;
    uninstall.destructive = true;
    Some(uninstall)
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

/// What the menu hands the footer.
#[derive(Clone, Debug)]
pub struct ActionCtx<'a> {
    pub mode: Mode,
    pub node: Option<&'a Node>,
    pub at_root: bool,
    pub picker_select: bool,
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
            confirming: false,
            alternate_label: "",
        }
    }
}

const KEY_ENTER: &[&str] = &["Enter"];
const KEY_ESC: &[&str] = &["Esc"];
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
        Kind::Option => "Select",
        Kind::Image if c.picker_select => "Choose",
        Kind::Image => "Set wallpaper",
        Kind::App | Kind::Submenu | Kind::Provider | Kind::Link => "Open",
        Kind::Action | Kind::Emoji => "Run",
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
    if n.chord.is_empty() { count_for(node) } else { n.chord.clone() }
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
        let opt = Node::new("select.0", "o", Kind::Option);
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
        let c = ActionCtx { node: Some(&app), alternate_label: "New window", ..Default::default() };
        assert_eq!(labels(&hints(&c)), ["New window", "Back"]);
        assert_eq!(hints(&c)[0].keys, ["Shift", "Enter"]);
        assert_eq!(labels(&hints(&ActionCtx { confirming: true, ..c })), ["Back"]);
    }

    #[test]
    fn an_action_is_parsed_once_into_a_call_or_a_line() {
        let call = |t: &str, f: &str, args: &[&str]| Run::Ipc(IpcCall { target: t.into(), function: f.into(), args: args.iter().map(|a| (*a).into()).collect() });
        assert_eq!(Run::parse("@ipc:theme.toggleMode"), Run::Intent(Intent::ThemeToggleMode));
        assert_eq!(Run::parse("@ipc:wifi.activate:Home:5G"), Run::Intent(Intent::WifiActivate("Home:5G".into())));
        assert_eq!(Run::parse("@ipc:app.open"), call("app", "open", &[]));
        assert_eq!(Run::parse("@ipc:panel.open:audio"), call("panel", "open", &["audio"]));
        assert_eq!(Run::parse("formalshell-ipc call screenshot full default"), call("screenshot", "full", &["default"]));
        assert_eq!(Run::parse("formalshell-ipc call panel open $X"), Run::Shell("formalshell-ipc call panel open $X".into()));
        assert_eq!(Run::parse("formalshell-ipc call panel"), Run::Shell("formalshell-ipc call panel".into()));
        assert_eq!(Run::parse("foot -e htop"), Run::Shell("foot -e htop".into()));
        assert_eq!(Run::parse("@ipc:wifi.activate:Home:5G").to_string(), "@ipc:wifi.activate:Home:5G");
    }

    #[test]
    fn a_fitted_call_keeps_its_colons_in_the_last_argument() {
        let wifi = IpcCall::parse("wifi.activate:Home:5G");
        assert_eq!(wifi.clone().fitted(1).args, ["Home:5G"]);
        assert_eq!(wifi.clone().fitted(2).args, ["Home", "5G"]);
        assert_eq!(wifi.clone().fitted(0).args, ["Home", "5G"]);
        assert_eq!(wifi.at(1), "5G");
        assert_eq!(wifi.at(2), "");
        assert_eq!(IpcCall::parse("nodot"), IpcCall { target: "nodot".into(), ..IpcCall::default() });
    }

    #[test]
    fn accessories() {
        let app = Node::new("apps.x", "X", Kind::App);
        assert_eq!(accessory_for(Some(&app)), "");
        assert_eq!(accessory_for(Some(&Node::new("a", "a", Kind::Action))), "");
        let emoji = Node { prefix: Some(":e".into()), ..Node::new("emoji", "Emoji", Kind::Provider) };
        assert_eq!(accessory_for(Some(&emoji)), ":e");
        let clip = Node { chord: "Super+Ctrl+V".into(), ..Node::new("clipboard", "Clipboard", Kind::Provider) };
        assert_eq!(accessory_for(Some(&clip)), "Super Ctrl V");
        let tray = Node { child_ids: vec!["a".into(), "b".into()], ..Node::new("tray", "Tray", Kind::Provider) };
        assert_eq!(accessory_for(Some(&tray)), "2");
        let meta = Node { meta: Some("42%".into()), ..app };
        assert_eq!(accessory_for(Some(&meta)), "42%");
        assert_eq!(accessory_for(None), "");
    }
}
