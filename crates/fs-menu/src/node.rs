//! The launcher's tree vocabulary: the JSONC `Entry` a menu file or a
//! provider declares, and the `Node` the tree and every row are made of.

use indexmap::IndexMap;
use serde::{Deserialize, Deserializer};

/// What a node is. `Other` carries an explicit `kind` the five inferred ones
/// do not cover (a user entry may say anything; the shell itself uses `note`
/// and `image`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Submenu,
    Link,
    Provider,
    Action,
    App,
    Note,
    Image,
    Other(String),
}

impl Kind {
    pub fn as_str(&self) -> &str {
        match self {
            Kind::Submenu => "submenu",
            Kind::Link => "link",
            Kind::Provider => "provider",
            Kind::Action => "action",
            Kind::App => "app",
            Kind::Note => "note",
            Kind::Image => "image",
            Kind::Other(s) => s,
        }
    }

    pub fn from_name(name: &str) -> Kind {
        match name {
            "submenu" => Kind::Submenu,
            "link" => Kind::Link,
            "provider" => Kind::Provider,
            "action" => Kind::Action,
            "app" => Kind::App,
            "note" => Kind::Note,
            "image" => Kind::Image,
            other => Kind::Other(other.to_string()),
        }
    }
}

impl<'de> Deserialize<'de> for Kind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Kind, D::Error> {
        Ok(Kind::from_name(&String::deserialize(d)?))
    }
}

/// One JSONC entry, as `default-menu.jsonc`, a user `menu.jsonc` and the
/// providers' dynamic fragments spell it. Every field is optional because
/// the per-key merge of user over default needs to tell "absent" from "set".
/// `desc` and `thumb_source` are carried by provider fragments only;
/// `build_tree` does not copy them onto the node.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Entry {
    pub label: Option<String>,
    pub icon: Option<String>,
    pub title: Option<String>,
    pub aliases: Option<Vec<String>>,
    pub kind: Option<Kind>,
    pub action: Option<String>,
    pub target: Option<String>,
    pub provider: Option<String>,
    pub when: Option<String>,
    pub checked: Option<String>,
    pub confirm: Option<bool>,
    pub dim: Option<bool>,
    pub keep_open: Option<bool>,
    pub route_only: Option<bool>,
    pub section: Option<String>,
    pub prompt: Option<String>,
    pub prefix: Option<String>,
    pub hidden: Option<bool>,
    pub desc: Option<String>,
    pub thumb_source: Option<String>,
    pub actions: Option<Vec<ActionEntry>>,
    pub alternate: Option<String>,
    pub alternate_label: Option<String>,
}

/// One entry of a node's `actions` array: a built-in's id overrides that
/// action key by key, `hidden: true` removes it, any other id adds one.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ActionEntry {
    pub id: String,
    pub label: Option<String>,
    pub icon: Option<String>,
    pub action: Option<String>,
    pub confirm: Option<bool>,
    pub keep_open: Option<bool>,
    pub hidden: Option<bool>,
    pub destructive: Option<bool>,
    pub paste_after: Option<bool>,
}

impl ActionEntry {
    pub fn new(id: &str, label: &str, action: impl Into<String>) -> ActionEntry {
        ActionEntry { id: id.into(), label: Some(label.into()), action: Some(action.into()), ..ActionEntry::default() }
    }

    pub fn destructive(mut self) -> ActionEntry {
        self.destructive = Some(true);
        self
    }

    fn merged(&self, over: &ActionEntry) -> ActionEntry {
        ActionEntry {
            id: self.id.clone(),
            label: over.label.clone().or_else(|| self.label.clone()),
            icon: over.icon.clone().or_else(|| self.icon.clone()),
            action: over.action.clone().or_else(|| self.action.clone()),
            confirm: over.confirm.or(self.confirm),
            keep_open: over.keep_open.or(self.keep_open),
            hidden: over.hidden.or(self.hidden),
            destructive: over.destructive.or(self.destructive),
            paste_after: over.paste_after.or(self.paste_after),
        }
    }
}

/// `over` laid onto `base` per action id, base order first.
pub fn merge_actions(base: &[ActionEntry], over: &[ActionEntry]) -> Vec<ActionEntry> {
    let mut out: Vec<ActionEntry> = base.to_vec();
    for o in over {
        match out.iter_mut().find(|a| a.id == o.id) {
            Some(a) => *a = a.merged(o),
            None => out.push(o.clone()),
        }
    }
    out
}

impl Entry {
    pub fn labelled(label: &str) -> Entry {
        Entry { label: Some(label.to_string()), ..Entry::default() }
    }

    pub fn with_icon(mut self, icon: &str) -> Entry {
        self.icon = Some(icon.to_string());
        self
    }

    pub fn with_action(mut self, action: impl Into<String>) -> Entry {
        self.action = Some(action.into());
        self
    }

    pub fn with_aliases(mut self, aliases: &[&str]) -> Entry {
        self.aliases = Some(aliases.iter().map(|a| a.to_string()).collect());
        self
    }

    pub fn with_checked(mut self, checked: impl Into<String>) -> Entry {
        self.checked = Some(checked.into());
        self
    }

    pub fn keeping_open(mut self) -> Entry {
        self.keep_open = Some(true);
        self
    }

    /// A key-by-key overlay: every field `over` sets wins.
    pub fn merged(&self, over: Option<&Entry>) -> Entry {
        let Some(u) = over else { return self.clone() };
        Entry {
            label: u.label.clone().or_else(|| self.label.clone()),
            icon: u.icon.clone().or_else(|| self.icon.clone()),
            title: u.title.clone().or_else(|| self.title.clone()),
            aliases: u.aliases.clone().or_else(|| self.aliases.clone()),
            kind: u.kind.clone().or_else(|| self.kind.clone()),
            action: u.action.clone().or_else(|| self.action.clone()),
            target: u.target.clone().or_else(|| self.target.clone()),
            provider: u.provider.clone().or_else(|| self.provider.clone()),
            when: u.when.clone().or_else(|| self.when.clone()),
            checked: u.checked.clone().or_else(|| self.checked.clone()),
            confirm: u.confirm.or(self.confirm),
            dim: u.dim.or(self.dim),
            keep_open: u.keep_open.or(self.keep_open),
            route_only: u.route_only.or(self.route_only),
            section: u.section.clone().or_else(|| self.section.clone()),
            prompt: u.prompt.clone().or_else(|| self.prompt.clone()),
            prefix: u.prefix.clone().or_else(|| self.prefix.clone()),
            hidden: u.hidden.or(self.hidden),
            desc: u.desc.clone().or_else(|| self.desc.clone()),
            thumb_source: u.thumb_source.clone().or_else(|| self.thumb_source.clone()),
            alternate: u.alternate.clone().or_else(|| self.alternate.clone()),
            alternate_label: u.alternate_label.clone().or_else(|| self.alternate_label.clone()),
            actions: match (&self.actions, &u.actions) {
                (Some(base), Some(over)) => Some(merge_actions(base, over)),
                (base, over) => over.clone().or_else(|| base.clone()),
            },
        }
    }
}

/// Dotted-id keyed entries in declaration order (a JS object's key order).
pub type Entries = IndexMap<String, Entry>;

/// A desktop entry, the fields the launcher reads off one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DesktopEntry {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub generic_name: String,
    pub startup_class: String,
    /// The `.desktop` file itself.
    pub path: String,
    /// Its `[Desktop Action]` groups: the group key and its `Name`.
    pub actions: Vec<(String, String)>,
}

/// A tree node and, equally, a display row.
///
/// Optional fields are the ones a consumer distinguishes "never set" from
/// "set to the empty value" on; plain strings and flags are the ones every
/// provider sets uniformly.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Node {
    pub id: String,
    pub parent_id: Option<String>,
    pub label: String,
    pub icon: String,
    pub title: String,
    pub aliases: Vec<String>,
    pub kind: Kind,
    pub child_ids: Vec<String>,
    pub action: Option<String>,
    pub target: Option<String>,
    pub provider: Option<String>,
    pub when: Option<String>,
    pub checked: Option<String>,
    pub confirm: Option<bool>,
    pub dim: Option<bool>,
    pub keep_open: Option<bool>,
    pub route_only: Option<bool>,
    pub section: Option<String>,
    pub prompt: Option<String>,
    /// What typed before a space in the field enters this route as a mode.
    pub prefix: Option<String>,
    /// The dim trailing slot of a row.
    pub desc: Option<String>,
    pub icon_source: String,
    pub thumb_source: String,
    pub clipssh_path: String,
    pub full_text: String,
    pub emoji_only: bool,
    pub time: String,
    pub verb: Option<String>,
    pub paste_after: bool,
    pub recent: bool,
    pub notify_summary: Option<String>,
    pub notify_body: Option<String>,
    pub meta: Option<String>,
    pub path: String,
    pub alternate: Option<String>,
    pub alternate_label: Option<String>,
    pub local_only: bool,
    /// The desktop entry an app row launches or focuses.
    pub entry: Option<DesktopEntry>,
    /// Actions past the kind's own: a provider's, then `menu.jsonc`'s.
    pub actions: Vec<ActionEntry>,
}

impl Node {
    pub fn new(id: impl Into<String>, label: impl Into<String>, kind: Kind) -> Node {
        Node { id: id.into(), label: label.into(), kind, ..Node::default() }
    }

    /// A dim, non-activatable row.
    pub fn note(id: impl Into<String>, label: impl Into<String>) -> Node {
        Node { dim: Some(true), ..Node::new(id, label, Kind::Note) }
    }

    pub fn with_action(mut self, action: impl Into<String>) -> Node {
        self.action = Some(action.into());
        self
    }
}

/// `build_tree`'s result: root ids in declaration order and every node by id.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tree {
    pub root_ids: Vec<String>,
    pub nodes: IndexMap<String, Node>,
}
