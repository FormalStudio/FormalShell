//! The settings file: what the owner changes without a new engine. It is
//! `settings.json` under `~/.config/formalwm/`, in the dialect the shell's
//! own hand-written files use (JSON with `//` comments and trailing commas,
//! camelCase keys), with an optional `host.json` beside it merged over it
//! for what differs per machine (its `outputs` are listed first, the
//! shared ones after). A file with an unknown key, a bad pattern,
//! chord or action is refused whole.

use std::{collections::BTreeMap, path::Path};

use serde::Deserialize;
use serde_json::Value;

use crate::{
    action::Action,
    chord::{Chord, Key},
    cursor::Dynamic,
    model::{Options, WorkspaceSpec},
    rules::{Anchor, Effect, Length, Matcher, Pattern, Position, Rule},
    smear::Smear,
};

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Layout {
    /// A new column's share of the output.
    pub column_width: f64,
    /// What `column-width cycle` walks.
    pub presets: Vec<f64>,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            column_width: 0.5,
            presets: vec![0.333, 0.5, 0.667, 1.0],
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Keyboard {
    pub layout: Option<String>,
    pub variant: Option<String>,
    pub options: Option<String>,
    /// Repeats per second.
    pub repeat_rate: Option<i32>,
    /// Milliseconds before the first repeat.
    pub repeat_delay: Option<i32>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Accel {
    Adaptive,
    Flat,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Pointer {
    pub accel: Option<Accel>,
    /// libinput's pointer speed, -1 to 1.
    pub sensitivity: Option<f64>,
    pub natural_scroll: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Touchpad {
    pub accel: Option<Accel>,
    pub sensitivity: Option<f64>,
    pub natural_scroll: Option<bool>,
    /// A two-finger press is the right button, a three-finger one the
    /// middle.
    pub clickfinger: Option<bool>,
    pub tap_to_click: Option<bool>,
    pub disable_while_typing: Option<bool>,
    pub scroll_factor: Option<f64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Input {
    pub keyboard: Keyboard,
    pub pointer: Pointer,
    pub touchpad: Touchpad,
    /// By libinput device name, lower case with dashes for spaces as
    /// Hyprland spells them.
    pub devices: BTreeMap<String, Pointer>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Cursor {
    pub theme: String,
    pub size: u32,
    /// The stretch or the lean that follows the pointer's speed, and the
    /// magnification of a shaken one.
    pub dynamic: Dynamic,
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            theme: "default".to_owned(),
            size: 24,
            dynamic: Dynamic::default(),
        }
    }
}

/// One output of a host. An entry with neither `name` nor `description`
/// is the rule for every output no other entry matches.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct OutputSettings {
    /// The connector, `eDP-1`.
    pub name: Option<String>,
    /// Make, model and serial as the monitor reports them, or their start.
    pub description: Option<String>,
    /// `2560x1440@144`, `2560x1440`, `highrr` for the highest refresh rate
    /// the monitor has, `preferred` or absent for the mode it asks for.
    pub mode: Option<String>,
    /// Absent places it beside the others.
    pub position: Option<[i32; 2]>,
    pub scale: Option<f64>,
    /// 0 off, 1 on, 2 fullscreen only, 3 fullscreen games and video.
    pub vrr: u8,
    pub enabled: Option<bool>,
    /// The numbers of the workspaces that live on it.
    pub workspaces: Vec<usize>,
    /// The workspace it shows first.
    pub default_workspace: Option<usize>,
}

/// What a connected output is, for matching.
#[derive(Clone, Copy, Debug, Default)]
pub struct OutputFacts<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub make: &'a str,
    pub model: &'a str,
    pub serial: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mode {
    pub width: u16,
    pub height: u16,
    pub refresh: Option<f64>,
}

/// The mode an output's entry asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ModeChoice {
    /// The monitor's own.
    Preferred,
    /// Hyprland's `highrr`: the highest refresh rate, and of the modes that
    /// have it the one with the most pixels.
    HighRefresh,
    Exact(Mode),
}

impl ModeChoice {
    /// Which of a monitor's modes (width, height, Hz) `highrr` takes.
    pub fn highest_refresh(modes: &[(i32, i32, f64)]) -> Option<(i32, i32, f64)> {
        let rank = |&(w, h, hz): &(i32, i32, f64)| {
            ((hz * 1000.0).round() as i64, i64::from(w) * i64::from(h))
        };
        modes.iter().copied().max_by_key(rank)
    }
}

impl OutputFacts<'_> {
    /// Whether `description` is this output's make, model and serial, or
    /// their start.
    fn described_by(&self, description: &str) -> bool {
        let reported = format!("{} {} {}", self.make, self.model, self.serial);
        reported.starts_with(description) || self.description.starts_with(description)
    }
}

impl OutputSettings {
    fn matches(&self, facts: &OutputFacts<'_>) -> bool {
        if let Some(name) = &self.name {
            return name == facts.name;
        }
        self.description
            .as_ref()
            .is_none_or(|description| facts.described_by(description))
    }

    pub fn mode(&self) -> ModeChoice {
        self.mode
            .as_deref()
            .and_then(|mode| parse_mode(mode).ok())
            .unwrap_or(ModeChoice::Preferred)
    }

    /// Whether `vrr` asks for variable refresh now. `fullscreen` is the
    /// content type of the window fullscreen on the output, empty when its
    /// rule names none.
    pub fn wants_vrr(&self, fullscreen: Option<&str>) -> bool {
        match (self.vrr, fullscreen) {
            (1, _) | (2, Some(_)) => true,
            (3, Some(content)) => matches!(content, "game" | "video"),
            _ => false,
        }
    }
}

fn parse_mode(text: &str) -> Result<ModeChoice, String> {
    match text {
        "preferred" => return Ok(ModeChoice::Preferred),
        "highrr" => return Ok(ModeChoice::HighRefresh),
        _ => {}
    }
    let bad =
        || format!("bad mode `{text}`, expected 2560x1440, 2560x1440@144, highrr or preferred");
    let (size, refresh) = match text.split_once('@') {
        Some((size, refresh)) => (size, Some(refresh.parse::<f64>().map_err(|_| bad())?)),
        None => (text, None),
    };
    let (width, height) = size.split_once('x').ok_or_else(bad)?;
    Ok(ModeChoice::Exact(Mode {
        width: width.parse().map_err(|_| bad())?,
        height: height.parse().map_err(|_| bad())?,
        refresh,
    }))
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct MatchFile {
    app_id: Option<String>,
    title: Option<String>,
    xwayland: Option<bool>,
    tag: Option<String>,
    floating: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
enum LengthFile {
    Px(f64),
    Text(String),
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct PositionFile {
    anchor: String,
    inset: f64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct RuleFile {
    #[serde(rename = "match")]
    matcher: MatchFile,
    workspace: Option<usize>,
    overlay: Option<String>,
    float: Option<bool>,
    pin: Option<bool>,
    pip: Option<bool>,
    size: Option<[LengthFile; 2]>,
    position: Option<PositionFile>,
    opacity: Option<f64>,
    no_blur: Option<bool>,
    content: Option<String>,
    confine_pointer: Option<bool>,
    idle_pointer: Option<f64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct LayerFile {
    namespace: String,
    blur: bool,
    alpha_threshold: f64,
    animate: Option<bool>,
}

/// How a layer surface is treated, by namespace.
#[derive(Clone, Debug)]
pub struct LayerRule {
    pub namespace: Pattern,
    pub blur: bool,
    /// Pixels more transparent than this are left unblurred.
    pub alpha_threshold: f64,
    /// False: the compositor plays nothing on it, the client animates
    /// itself.
    pub animate: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct BindFile {
    chord: String,
    action: String,
    args: Vec<String>,
    title: String,
    locked: bool,
    repeat: bool,
    release: bool,
    passthrough: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bind {
    pub chord: Chord,
    pub action: Action,
    pub action_name: String,
    pub args: Vec<String>,
    pub title: String,
    /// Fires while the session is locked.
    pub locked: bool,
    /// Fires again while held.
    pub repeat: bool,
    /// Fires when the key is let go.
    pub release: bool,
    /// Hyprland's `transparent`: on a modifier's release, fires after a
    /// chord with that modifier too, not only after a tap.
    pub passthrough: bool,
}

impl Bind {
    /// Whether a modifier let go runs the bind after a chord that used the
    /// modifier, where every other release bind wants the modifier pressed
    /// and let go alone. As in Hyprland the bind has to say so
    /// (`passthrough`) and to hold the modifier it is on: a bare `Alt_L`
    /// matches no modifier state a chord leaves behind.
    pub fn after_chord(&self) -> bool {
        self.release && self.passthrough && self.chord.names_its_modifier()
    }
}

/// The key and wheel binds of one shortcut. Hyprland runs every bind a
/// chord has; ShojiWM fires one shortcut once, so they share a registration.
#[derive(Clone, Debug, PartialEq)]
pub struct BindGroup {
    /// ShojiWM's shortcut: the chord, or the modifier alone for one let go.
    pub shortcut: String,
    /// The first of its binds, by its place in the settings.
    pub index: usize,
    pub release: bool,
    /// On a modifier let go: it fires whatever was pressed while the
    /// modifier was down ([`Bind::after_chord`]). One that does not is
    /// taken by the chord that used the modifier, so the two are registered
    /// apart.
    pub transparent: bool,
    /// Some bind of the group fires while the session is locked.
    pub locked: bool,
    /// Some bind of the group repeats while the key is held.
    pub repeat: bool,
    actions: Vec<Run>,
}

/// One action of a shortcut, with what its own bind says of it.
#[derive(Clone, Debug, PartialEq)]
struct Run {
    action: Action,
    locked: bool,
    repeat: bool,
}

impl BindGroup {
    /// What the shortcut runs: on the lock screen only the actions of the
    /// binds marked `locked`, on a held key's repeat only those marked
    /// `repeat`.
    pub fn actions(&self, locked: bool, repeat: bool) -> impl Iterator<Item = &Action> {
        self.actions
            .iter()
            .filter(move |run| (run.locked || !locked) && (run.repeat || !repeat))
            .map(|run| &run.action)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
struct SettingsFile {
    workspaces: Vec<String>,
    terminal: String,
    layout: Layout,
    input: Input,
    cursor: Cursor,
    env: BTreeMap<String, String>,
    outputs: Vec<OutputSettings>,
    lid_output: Option<String>,
    xwayland_primary: Option<String>,
    opacity: f64,
    motion: Smear,
    direct_scanout: bool,
    rules: Vec<RuleFile>,
    layers: Vec<LayerFile>,
    binds: Vec<BindFile>,
}

impl Default for SettingsFile {
    fn default() -> Self {
        Self {
            workspaces: (1..=9).map(|n| n.to_string()).collect(),
            terminal: "ghostty".to_owned(),
            layout: Layout::default(),
            input: Input::default(),
            cursor: Cursor::default(),
            env: BTreeMap::new(),
            outputs: Vec::new(),
            lid_output: None,
            xwayland_primary: None,
            opacity: 1.0,
            motion: Smear::default(),
            direct_scanout: true,
            rules: Vec::new(),
            layers: Vec::new(),
            binds: Vec::new(),
        }
    }
}

/// The settings, read and checked.
#[derive(Clone, Debug)]
pub struct Settings {
    pub workspaces: Vec<String>,
    pub terminal: String,
    pub layout: Layout,
    pub input: Input,
    pub cursor: Cursor,
    pub env: BTreeMap<String, String>,
    pub outputs: Vec<OutputSettings>,
    /// The output that goes dark with the lid.
    pub lid_output: Option<String>,
    /// The output XWayland calls primary.
    pub xwayland_primary: Option<String>,
    /// Every window's opacity, so the blur behind has something to show.
    pub opacity: f64,
    /// What speed does to a window's picture.
    pub motion: Smear,
    pub direct_scanout: bool,
    pub rules: Vec<Rule>,
    pub layers: Vec<LayerRule>,
    pub binds: Vec<Bind>,
}

impl Default for Settings {
    fn default() -> Self {
        Self::parse("{}", None).expect("the defaults are valid")
    }
}

impl Settings {
    /// Reads `settings.json` and, when it is there, `host.json` from `dir`.
    /// No `settings.json` at all gives the defaults.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let read = |name: &str| match std::fs::read_to_string(dir.join(name)) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("{}: {error}", dir.join(name).display())),
        };
        let main = read("settings.json")?;
        let host = read("host.json")?;
        Self::parse(main.as_deref().unwrap_or("{}"), host.as_deref())
    }

    pub fn parse(main: &str, host: Option<&str>) -> Result<Self, String> {
        let mut value: Value = serde_json::from_str(&strip_jsonc(main))
            .map_err(|error| format!("settings.json: {error}"))?;
        if let Some(host) = host {
            let host: Value = serde_json::from_str(&strip_jsonc(host))
                .map_err(|error| format!("host.json: {error}"))?;
            // A host's monitors go ahead of the shared ones instead of in
            // their place, so the shared rule for a display nobody listed
            // still stands behind them.
            let shared = match (value.get("outputs"), host.get("outputs")) {
                (Some(Value::Array(shared)), Some(Value::Array(_))) => shared.clone(),
                _ => Vec::new(),
            };
            merge(&mut value, host);
            if let Some(Value::Array(outputs)) = value.get_mut("outputs") {
                outputs.extend(shared);
            }
        }
        let file: SettingsFile =
            serde_json::from_value(value).map_err(|error| format!("settings: {error}"))?;
        Self::check(file)
    }

    fn check(file: SettingsFile) -> Result<Self, String> {
        if file.workspaces.is_empty() {
            return Err("workspaces: at least one is needed".to_owned());
        }
        if !(0.1..=1.0).contains(&file.layout.column_width)
            || file.layout.presets.iter().any(|p| !(0.1..=1.0).contains(p))
        {
            return Err("layout: widths are between 0.1 and 1".to_owned());
        }
        let count = file.workspaces.len();
        let workspace = |number: usize, at: &str| {
            if (1..=count).contains(&number) {
                Ok(number)
            } else {
                Err(format!("{at}: there is no workspace {number}"))
            }
        };
        for (index, output) in file.outputs.iter().enumerate() {
            let at = format!("outputs[{index}]");
            if let Some(mode) = &output.mode {
                parse_mode(mode).map_err(|error| format!("{at}: {error}"))?;
            }
            for number in output.workspaces.iter().chain(&output.default_workspace) {
                workspace(*number, &at)?;
            }
        }
        // `std::env::set_var` panics on each of these.
        for (name, value) in &file.env {
            if name.is_empty() || name.contains(['=', '\0']) || value.contains('\0') {
                return Err(format!("env: `{}` cannot be set", name.escape_debug()));
            }
        }
        let mut rules = Vec::new();
        for (index, rule) in file.rules.into_iter().enumerate() {
            let at = format!("rules[{index}]");
            let pattern = |text: Option<String>| {
                text.map(|text| Pattern::new(&text))
                    .transpose()
                    .map_err(|error| format!("{at}: {error}"))
            };
            let length = |length: &LengthFile| match length {
                LengthFile::Px(px) => Ok(Length::Px(*px)),
                LengthFile::Text(text) => {
                    Length::parse(text).map_err(|error| format!("{at}: {error}"))
                }
            };
            if let Some(number) = rule.workspace {
                workspace(number, &at)?;
            }
            rules.push(Rule {
                matcher: Matcher {
                    app_id: pattern(rule.matcher.app_id)?,
                    title: pattern(rule.matcher.title)?,
                    tag: pattern(rule.matcher.tag)?,
                    xwayland: rule.matcher.xwayland,
                    floating: rule.matcher.floating,
                },
                effect: Effect {
                    workspace: rule.workspace,
                    overlay: rule.overlay,
                    float: rule.float,
                    pin: rule.pin,
                    pip: rule.pip,
                    size: match &rule.size {
                        Some([w, h]) => Some((length(w)?, length(h)?)),
                        None => None,
                    },
                    position: match rule.position {
                        Some(position) => Some(Position {
                            anchor: Anchor::parse(&position.anchor)
                                .map_err(|error| format!("{at}: {error}"))?,
                            inset: position.inset,
                        }),
                        None => None,
                    },
                    opacity: rule.opacity,
                    no_blur: rule.no_blur,
                    content: rule.content,
                    confine_pointer: rule.confine_pointer,
                    idle_pointer: rule.idle_pointer,
                },
            });
        }
        let mut layers = Vec::new();
        for (index, layer) in file.layers.into_iter().enumerate() {
            layers.push(LayerRule {
                namespace: Pattern::new(&layer.namespace)
                    .map_err(|error| format!("layers[{index}]: {error}"))?,
                blur: layer.blur,
                alpha_threshold: layer.alpha_threshold,
                animate: layer.animate.unwrap_or(true),
            });
        }
        let mut binds = Vec::new();
        for (index, bind) in file.binds.into_iter().enumerate() {
            let at = format!("binds[{index}]");
            let chord = Chord::parse(&bind.chord).map_err(|error| format!("{at}: {error}"))?;
            let action = Action::parse(&bind.action, &bind.args)
                .map_err(|error| format!("{at}: {error}"))?;
            let pointer = matches!(action, Action::MoveWindow | Action::ResizeWindow);
            if pointer != matches!(chord.key, Key::Button(_)) {
                return Err(format!(
                    "{at}: a pointer button binds move-window or resize-window, and nothing else does"
                ));
            }
            match &action {
                Action::Workspace(crate::action::WorkspaceRef::Number(number))
                | Action::MoveToWorkspace(number) => {
                    workspace(*number, &at)?;
                }
                _ => {}
            }
            binds.push(Bind {
                chord,
                action,
                action_name: bind.action,
                args: bind.args,
                title: bind.title,
                locked: bind.locked,
                repeat: bind.repeat,
                release: bind.release,
                passthrough: bind.passthrough,
            });
        }
        file.cursor.dynamic.check()?;
        file.motion.check()?;
        Ok(Self {
            workspaces: file.workspaces,
            terminal: file.terminal,
            layout: file.layout,
            input: file.input,
            cursor: file.cursor,
            env: file.env,
            outputs: file.outputs,
            lid_output: file.lid_output,
            xwayland_primary: file.xwayland_primary,
            opacity: file.opacity.clamp(0.0, 1.0),
            motion: file.motion,
            direct_scanout: file.direct_scanout,
            rules,
            layers,
            binds,
        })
    }

    /// The binds by shortcut, in the shortcuts' order. A pointer button has
    /// no shortcut and is in none.
    pub fn bind_groups(&self) -> Vec<BindGroup> {
        let mut groups: BTreeMap<(String, bool, bool), BindGroup> = BTreeMap::new();
        for (index, bind) in self.binds.iter().enumerate() {
            let shortcut = match (bind.release, bind.chord.modifier_key()) {
                (true, Some(modifier)) => Some(modifier.to_owned()),
                _ => bind.chord.shortcut(),
            };
            let Some(shortcut) = shortcut else {
                continue;
            };
            let transparent = bind.after_chord();
            let repeat = bind.repeat && !bind.release;
            // Both Alt keys let go are one shortcut here.
            let group = groups
                .entry((shortcut.clone(), bind.release, transparent))
                .or_insert_with(|| BindGroup {
                    shortcut,
                    index,
                    release: bind.release,
                    transparent,
                    locked: false,
                    repeat: false,
                    actions: Vec::new(),
                });
            group.locked |= bind.locked;
            group.repeat |= repeat;
            match group
                .actions
                .iter_mut()
                .find(|run| run.action == bind.action)
            {
                Some(run) => {
                    run.locked |= bind.locked;
                    run.repeat |= repeat;
                }
                None => group.actions.push(Run {
                    action: bind.action.clone(),
                    locked: bind.locked,
                    repeat,
                }),
            }
        }
        groups.into_values().collect()
    }

    /// The entry for a connected output: the first that names it, else the
    /// one that names nothing.
    pub fn output_for(&self, facts: &OutputFacts<'_>) -> Option<&OutputSettings> {
        let named =
            |output: &&OutputSettings| output.name.is_some() || output.description.is_some();
        self.outputs
            .iter()
            .filter(named)
            .find(|output| output.matches(facts))
            .or_else(|| self.outputs.iter().find(|output| !named(output)))
    }

    /// The workspaces with the output each is pinned to, given which
    /// connected output every entry matched.
    pub fn workspace_specs(&self, connected: &[OutputFacts<'_>]) -> Vec<WorkspaceSpec> {
        self.workspaces
            .iter()
            .enumerate()
            .map(|(index, name)| WorkspaceSpec {
                name: name.clone(),
                pin: connected
                    .iter()
                    .find(|facts| {
                        self.output_for(facts)
                            .is_some_and(|output| output.workspaces.contains(&(index + 1)))
                    })
                    .map(|facts| facts.name.to_owned()),
            })
            .collect()
    }

    /// `lidOutput` names an output by its connector or its description.
    pub fn is_lid_output(&self, facts: &OutputFacts<'_>) -> bool {
        self.lid_output
            .as_ref()
            .is_some_and(|lid| lid == facts.name || facts.described_by(lid))
    }

    /// The layout's options under the shell's chrome.
    pub fn options(&self, chrome: &fs_wm_proto::Chrome) -> Options {
        Options {
            default_width: self.layout.column_width,
            presets: self.layout.presets.clone(),
            gaps_in: chrome.gaps_in as f64,
            gaps_out: chrome.gaps_out as f64,
            border: chrome.edge.thickness(),
        }
    }

    pub fn layer_rule(&self, namespace: &str) -> Option<&LayerRule> {
        self.layers
            .iter()
            .find(|rule| rule.namespace.matches(namespace))
    }
}

/// Drops `//` and `/* */` comments and trailing commas, outside strings.
pub fn strip_jsonc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            match c {
                '\\' => {
                    if let Some(next) = chars.next() {
                        out.push(next);
                    }
                }
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut last = ' ';
                for next in chars.by_ref() {
                    if last == '*' && next == '/' {
                        break;
                    }
                    if next == '\n' {
                        out.push('\n');
                    }
                    last = next;
                }
            }
            ',' => {
                let mut ahead = chars.clone();
                let mut closes = false;
                while let Some(next) = ahead.next() {
                    match next {
                        c if c.is_whitespace() => {}
                        '/' if ahead.peek() == Some(&'/') => {
                            for skipped in ahead.by_ref() {
                                if skipped == '\n' {
                                    break;
                                }
                            }
                        }
                        '/' if ahead.peek() == Some(&'*') => {
                            ahead.next();
                            let mut last = ' ';
                            for skipped in ahead.by_ref() {
                                if last == '*' && skipped == '/' {
                                    break;
                                }
                                last = skipped;
                            }
                        }
                        '}' | ']' => {
                            closes = true;
                            break;
                        }
                        _ => break,
                    }
                }
                if !closes {
                    out.push(',');
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// Lays `over` on `base`: objects merge key by key, anything else replaces.
fn merge(base: &mut Value, over: Value) {
    match (base, over) {
        (Value::Object(base), Value::Object(over)) => {
            for (key, value) in over {
                merge(base.entry(key).or_insert(Value::Null), value);
            }
        }
        (base, over) => *base = over,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::WorkspaceRef;

    /// Gaps are measured to the frame formalwm draws, the panel edge, and
    /// not to Hyprland's `borderSize`.
    #[test]
    fn the_layout_border_is_the_edge_formalwm_draws() {
        let chrome = fs_wm_proto::Chrome {
            border_size: 5,
            edge: fs_wm_proto::Edge {
                width: 1.0,
                highlight: 1.0,
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(Settings::default().options(&chrome).border, 2.0);
        let bare = fs_wm_proto::Edge {
            width: 1.0,
            highlight: 0.0,
            ..Default::default()
        };
        assert_eq!(
            Settings::default()
                .options(&fs_wm_proto::Chrome {
                    edge: bare,
                    ..chrome
                })
                .border,
            1.0
        );
    }

    #[test]
    fn an_empty_file_is_the_defaults() {
        let settings = Settings::parse("{}", None).unwrap();
        assert_eq!(settings.workspaces.len(), 9);
        assert_eq!(settings.layout, Layout::default());
        assert_eq!(settings.opacity, 1.0);
        assert!(settings.binds.is_empty());
    }

    #[test]
    fn comments_and_trailing_commas_are_allowed() {
        let text = r#"{
            // the names the bar shows
            "workspaces": ["web", "term", /* three */ "dev",],
            "terminal": "ghostty", // not "kitty//x"
            "env": { "A": "b // not a comment", },
        }"#;
        let settings = Settings::parse(text, None).unwrap();
        assert_eq!(settings.workspaces, ["web", "term", "dev"]);
        assert_eq!(settings.env["A"], "b // not a comment");
    }

    #[test]
    fn the_host_file_lays_over_the_shared_one() {
        let main = r#"{ "opacity": 1.0, "env": { "A": "1", "B": "2" }, "input": { "keyboard": { "layout": "es" } } }"#;
        let host = r#"{ "opacity": 0.96, "env": { "B": "3" }, "input": { "keyboard": { "options": "caps:escape" } } }"#;
        let settings = Settings::parse(main, Some(host)).unwrap();
        assert_eq!(settings.opacity, 0.96);
        assert_eq!(settings.env["A"], "1");
        assert_eq!(settings.env["B"], "3");
        assert_eq!(settings.input.keyboard.layout.as_deref(), Some("es"));
        assert_eq!(
            settings.input.keyboard.options.as_deref(),
            Some("caps:escape")
        );
    }

    #[test]
    fn a_hosts_outputs_come_before_the_shared_ones() {
        let main = r#"{ "outputs": [{ "name": "eDP-1", "scale": 1 }, { "mode": "highrr" }] }"#;
        let host = r#"{ "outputs": [{ "name": "eDP-1", "scale": 1.25 }, { "name": "DP-1", "mode": "2560x1440@144" }] }"#;
        let settings = Settings::parse(main, Some(host)).unwrap();
        let named = |name| OutputFacts {
            name,
            ..OutputFacts::default()
        };
        assert_eq!(
            settings.output_for(&named("eDP-1")).unwrap().scale,
            Some(1.25)
        );
        assert!(matches!(
            settings.output_for(&named("DP-1")).unwrap().mode(),
            ModeChoice::Exact(_)
        ));
        assert_eq!(
            settings.output_for(&named("HDMI-A-2")).unwrap().mode(),
            ModeChoice::HighRefresh,
            "a display the host does not list keeps the shared rule"
        );
    }

    #[test]
    fn a_bind_is_a_chord_to_an_action() {
        let text = r#"{ "binds": [
            { "chord": "SUPER+code:12", "action": "workspace", "args": ["3"] },
            { "chord": "XF86AudioRaiseVolume", "action": "spawn", "args": ["wpctl", "set-volume"], "locked": true, "repeat": true, "title": "Volume up" },
            { "chord": "SUPER+mouse:272", "action": "move-window" }
        ] }"#;
        let settings = Settings::parse(text, None).unwrap();
        assert_eq!(
            settings.binds[0].action,
            Action::Workspace(WorkspaceRef::Number(3))
        );
        assert!(settings.binds[1].locked && settings.binds[1].repeat);
        assert_eq!(settings.binds[1].title, "Volume up");
        assert_eq!(settings.binds[2].action, Action::MoveWindow);
    }

    #[test]
    fn what_is_refused_says_where() {
        let cases = [
            (r#"{ "workspace": [] }"#, "unknown field"),
            (
                r#"{ "binds": [ { "chord": "SUPER+", "action": "close" } ] }"#,
                "binds[0]",
            ),
            (
                r#"{ "binds": [ { "chord": "SUPER+q", "action": "quit" } ] }"#,
                "unknown action",
            ),
            (
                r#"{ "binds": [ { "chord": "SUPER+q", "action": "move-window" } ] }"#,
                "pointer button",
            ),
            (
                r#"{ "binds": [ { "chord": "SUPER+code:19", "action": "workspace", "args": ["10"] } ] }"#,
                "no workspace 10",
            ),
            (
                r#"{ "rules": [ { "match": { "appId": "(" }, "float": true } ] }"#,
                "rules[0]",
            ),
            (
                r#"{ "rules": [ { "match": {}, "workspace": 12 } ] }"#,
                "no workspace 12",
            ),
            (
                r#"{ "rules": [ { "match": {}, "size": ["a%", 10] } ] }"#,
                "bad length",
            ),
            (r#"{ "outputs": [ { "mode": "big" } ] }"#, "bad mode"),
            (r#"{ "layout": { "columnWidth": 3 } }"#, "between 0.1 and 1"),
            (r#"{ "workspaces": [] }"#, "at least one"),
            (r#"{ "binds": [ "#, "settings.json"),
        ];
        for (text, needle) in cases {
            let error = Settings::parse(text, None).unwrap_err();
            assert!(
                error.contains(needle),
                "{text} gave {error:?}, wanted {needle:?}"
            );
        }
    }

    #[test]
    fn on_the_lock_screen_a_chord_runs_only_its_binds_marked_locked() {
        let text = r#"{ "binds": [
            { "chord": "XF86AudioRaiseVolume", "action": "spawn", "args": ["louder"], "locked": true, "repeat": true },
            { "chord": "XF86AudioRaiseVolume", "action": "spawn", "args": ["osd"] },
            { "chord": "ALT+Alt_L", "action": "spawn", "args": ["commit"], "release": true, "passthrough": true },
            { "chord": "ALT+Alt_R", "action": "spawn", "args": ["commit"], "release": true, "passthrough": true },
            { "chord": "SUPER+mouse:272", "action": "move-window" }
        ] }"#;
        let groups = Settings::parse(text, None).unwrap().bind_groups();
        let spawn = |command: &str| Action::Spawn(vec![command.to_owned()]);
        let run =
            |group: &BindGroup, locked| group.actions(locked, false).cloned().collect::<Vec<_>>();
        assert_eq!(groups.len(), 2, "a pointer button has no shortcut");

        let alt = &groups[0];
        assert_eq!((alt.shortcut.as_str(), alt.index), ("Alt", 2));
        assert!(alt.release && alt.transparent && !alt.locked);
        assert_eq!(run(alt, false), [spawn("commit")]);
        assert_eq!(run(alt, true), []);

        let volume = &groups[1];
        assert_eq!(volume.shortcut, "XF86AudioRaiseVolume");
        assert!(volume.locked && volume.repeat && !volume.release);
        assert_eq!(run(volume, false), [spawn("louder"), spawn("osd")]);
        assert_eq!(run(volume, true), [spawn("louder")]);
    }

    #[test]
    fn each_bind_of_a_shortcut_keeps_what_it_says_of_itself() {
        let text = r#"{ "binds": [
            { "chord": "ALT+Alt_L", "action": "spawn", "args": ["commit"], "release": true, "passthrough": true },
            { "chord": "ALT+Alt_L", "action": "spawn", "args": ["plain"], "release": true },
            { "chord": "SUPER+Up", "action": "spawn", "args": ["louder"], "repeat": true },
            { "chord": "SUPER+Up", "action": "spawn", "args": ["osd"] }
        ] }"#;
        let groups = Settings::parse(text, None).unwrap().bind_groups();
        let spawn = |command: &str| Action::Spawn(vec![command.to_owned()]);
        let run =
            |group: &BindGroup, repeat| group.actions(false, repeat).cloned().collect::<Vec<_>>();
        // A modifier let go: the bind a chord takes and the one it cannot
        // take are two registrations.
        let [plain, commit, up] = &groups[..] else {
            panic!("{groups:?}");
        };
        assert_eq!((plain.shortcut.as_str(), plain.transparent), ("Alt", false));
        assert_eq!(run(plain, false), [spawn("plain")]);
        assert_eq!(
            (commit.shortcut.as_str(), commit.transparent),
            ("Alt", true)
        );
        assert_eq!(run(commit, false), [spawn("commit")]);
        // A held key repeats the binds that say so and no other.
        assert!(up.repeat);
        assert_eq!(run(up, false), [spawn("louder"), spawn("osd")]);
        assert_eq!(run(up, true), [spawn("louder")]);
    }

    #[test]
    fn a_bare_modifier_let_go_fires_after_the_modifier_alone() {
        let text = r#"{ "binds": [
            { "chord": "ALT+Alt_L", "action": "spawn", "args": ["commit"], "release": true, "passthrough": true },
            { "chord": "Alt_L", "action": "spawn", "args": ["bare"], "release": true, "passthrough": true },
            { "chord": "Alt_R", "action": "spawn", "args": ["plain"], "release": true },
            { "chord": "SHIFT+Alt_L", "action": "spawn", "args": ["other"], "release": true, "passthrough": true }
        ] }"#;
        let settings = Settings::parse(text, None).unwrap();
        let after_chord: Vec<bool> = settings.binds.iter().map(Bind::after_chord).collect();
        assert_eq!(after_chord, [true, false, false, false]);
        let groups = settings.bind_groups();
        let spawn = |command: &str| Action::Spawn(vec![command.to_owned()]);
        let run = |group: &BindGroup| group.actions(false, false).cloned().collect::<Vec<_>>();
        // The binds a chord with Alt takes are one registration, the one
        // it cannot take another.
        let [alone, commit] = &groups[..] else {
            panic!("{groups:?}");
        };
        assert_eq!((alone.shortcut.as_str(), alone.transparent), ("Alt", false));
        assert_eq!(run(alone), [spawn("bare"), spawn("plain"), spawn("other")]);
        assert_eq!(
            (commit.shortcut.as_str(), commit.transparent),
            ("Alt", true)
        );
        assert_eq!(run(commit), [spawn("commit")]);
    }

    #[test]
    fn motion_is_a_blur_and_a_stretch_with_the_stretch_off_until_asked_for() {
        use crate::smear::Smear;
        let defaults = Settings::default().motion;
        assert_eq!(
            defaults,
            Smear {
                blur: 1.0,
                stretch: 0.0
            }
        );
        let set = Settings::parse(r#"{ "motion": { "blur": 0, "stretch": 1.5 } }"#, None).unwrap();
        assert_eq!(
            set.motion,
            Smear {
                blur: 0.0,
                stretch: 1.5
            }
        );
        // One key leaves the other as it was, and a host can turn one off.
        let host = Settings::parse(
            r#"{ "motion": { "stretch": 1 } }"#,
            Some(r#"{ "motion": { "blur": 0 } }"#),
        )
        .unwrap();
        assert_eq!(
            host.motion,
            Smear {
                blur: 0.0,
                stretch: 1.0
            }
        );
        for bad in [
            r#"{ "blur": 2.5 }"#,
            r#"{ "stretch": -0.1 }"#,
            r#"{ "shutter": 1 }"#,
        ] {
            let error = Settings::parse(&format!(r#"{{ "motion": {bad} }}"#), None).unwrap_err();
            assert!(
                error.contains("motion") || error.contains("shutter"),
                "{bad} gave {error:?}"
            );
        }
    }

    #[test]
    fn an_env_name_or_value_that_cannot_be_set_is_refused() {
        for env in [
            r#"{ "FOO": "a\u0000b" }"#,
            r#"{ "": "x" }"#,
            r#"{ "A=B": "x" }"#,
            r#"{ "A\u0000": "x" }"#,
        ] {
            let error = Settings::parse(&format!(r#"{{ "env": {env} }}"#), None).unwrap_err();
            assert!(error.starts_with("env:"), "{env} gave {error:?}");
            let host = format!(r#"{{ "env": {env} }}"#);
            assert!(
                Settings::parse("{}", Some(&host)).is_err(),
                "{env} in host.json"
            );
        }
        let fine = Settings::parse(r#"{ "env": { "FOO": "", "BAR": "a=b" } }"#, None).unwrap();
        assert_eq!(fine.env["BAR"], "a=b");
    }

    #[test]
    fn outputs_match_by_connector_then_description_then_the_catch_all() {
        let text = r#"{ "outputs": [
            { "description": "ASUSTek COMPUTER INC PA278CGV T1LMQS063802", "mode": "2560x1440@144", "position": [2560, 0], "scale": 1, "vrr": 2, "defaultWorkspace": 1 },
            { "description": "BOE NE180QDM-NZC", "scale": 1.25, "workspaces": [4, 8] },
            { "name": "HDMI-A-1", "scale": 2 },
            { "scale": 1 }
        ] }"#;
        let settings = Settings::parse(text, None).unwrap();
        let desk = OutputFacts {
            name: "DP-1",
            make: "ASUSTek COMPUTER INC",
            model: "PA278CGV",
            serial: "T1LMQS063802",
            ..OutputFacts::default()
        };
        let panel = OutputFacts {
            name: "eDP-1",
            make: "BOE",
            model: "NE180QDM-NZC",
            ..OutputFacts::default()
        };
        let hdmi = OutputFacts {
            name: "HDMI-A-1",
            make: "Some",
            model: "TV",
            ..OutputFacts::default()
        };
        let other = OutputFacts {
            name: "DP-3",
            make: "Dell",
            model: "U2720Q",
            ..OutputFacts::default()
        };
        let matched = settings.output_for(&desk).unwrap();
        assert_eq!(
            matched.mode(),
            ModeChoice::Exact(Mode {
                width: 2560,
                height: 1440,
                refresh: Some(144.0)
            })
        );
        let modes = [
            (3840, 2160, 60.0),
            (1920, 1080, 119.88),
            (2560, 1440, 119.88),
            (1280, 720, 100.0),
        ];
        assert_eq!(
            ModeChoice::highest_refresh(&modes),
            Some((2560, 1440, 119.88))
        );
        let unlisted = OutputSettings {
            mode: Some("highrr".to_owned()),
            ..Default::default()
        };
        assert_eq!(unlisted.mode(), ModeChoice::HighRefresh);
        assert_eq!(OutputSettings::default().mode(), ModeChoice::Preferred);
        assert_eq!(matched.vrr, 2);
        let vrr = |vrr: u8, fullscreen| {
            OutputSettings {
                vrr,
                ..Default::default()
            }
            .wants_vrr(fullscreen)
        };
        assert!(!vrr(0, Some("game")));
        assert!(vrr(1, None));
        assert!(!vrr(2, None));
        assert!(vrr(2, Some("")));
        assert!(!vrr(3, Some("")));
        assert!(vrr(3, Some("game")));
        assert!(vrr(3, Some("video")));
        assert_eq!(settings.output_for(&panel).unwrap().scale, Some(1.25));
        assert_eq!(settings.output_for(&hdmi).unwrap().scale, Some(2.0));
        assert_eq!(settings.output_for(&other).unwrap().scale, Some(1.0));

        let specs = settings.workspace_specs(&[desk, panel]);
        assert_eq!(specs[3].pin.as_deref(), Some("eDP-1"));
        assert_eq!(specs[7].pin.as_deref(), Some("eDP-1"));
        assert_eq!(specs[0].pin, None);
    }

    #[test]
    fn a_layer_rule_by_namespace() {
        let text = r#"{ "layers": [
            { "namespace": "formalshell:bar", "blur": true, "alphaThreshold": 0.2, "animate": false },
            { "namespace": "formalshell:veil", "animate": false }
        ] }"#;
        let settings = Settings::parse(text, None).unwrap();
        let bar = settings.layer_rule("formalshell:bar").unwrap();
        assert!(bar.blur && !bar.animate);
        assert_eq!(bar.alpha_threshold, 0.2);
        assert!(!settings.layer_rule("formalshell:veil").unwrap().blur);
        assert!(settings.layer_rule("formalshell:bar2").is_none());
    }
}
