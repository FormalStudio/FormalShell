use fs_dbus::{Props, prop};
use zbus::zvariant::{OwnedObjectPath, Structure};

/// One `a(iiay)` entry, already converted from the wire's ARGB32 to RGBA8.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pixmap {
    pub width: i32,
    pub height: i32,
    pub rgba: Vec<u8>,
}

impl Pixmap {
    fn from_wire(width: i32, height: i32, argb: &[u8]) -> Option<Self> {
        if width <= 0 || height <= 0 {
            return None;
        }
        let want = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        if argb.len() < want {
            return None;
        }
        let mut rgba = Vec::with_capacity(want);
        for px in argb[..want].as_chunks::<4>().0 {
            rgba.extend_from_slice(&[px[1], px[2], px[3], px[0]]);
        }
        Some(Self {
            width,
            height,
            rgba,
        })
    }
}

/// The first adequate pixmap that is smaller than the
/// current pick, else the largest.
pub fn closest_pixmap(pixmaps: &[Pixmap], width: i32, height: i32) -> Option<&Pixmap> {
    let mut best: Option<&Pixmap> = None;
    for pm in pixmaps {
        let Some(cur) = best else {
            best = Some(pm);
            continue;
        };
        let existing_adequate = cur.width >= width && cur.height >= height;
        let new_adequate = pm.width >= width && pm.height >= height;
        let new_smaller = pm.width < cur.width || pm.height < cur.height;
        if (existing_adequate && new_adequate && new_smaller)
            || (!existing_adequate && !new_smaller)
        {
            best = Some(pm);
        }
    }
    best
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    Passive,
    #[default]
    Active,
    NeedsAttention,
}

impl Status {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Passive" => Some(Self::Passive),
            "Active" => Some(Self::Active),
            "NeedsAttention" => Some(Self::NeedsAttention),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Tooltip {
    pub icon_name: String,
    pub icon_pixmap: Vec<Pixmap>,
    pub title: String,
    pub description: String,
}

/// What the bar draws, resolved from the item's icon properties.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IconSource<'a> {
    Named {
        name: &'a str,
        theme_path: &'a str,
    },
    /// No usable name: draw `icon_pixmap` (or `attention_icon_pixmap`), overlay on top.
    Pixmaps,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Item {
    /// The watcher's address for the item, `service/path`. Stable for the item's lifetime.
    pub key: String,
    pub service: String,
    pub path: String,
    pub id: String,
    pub title: String,
    pub status: Status,
    pub icon_name: String,
    pub icon_pixmap: Vec<Pixmap>,
    pub icon_theme_path: String,
    pub overlay_icon_name: String,
    pub overlay_icon_pixmap: Vec<Pixmap>,
    pub attention_icon_name: String,
    pub attention_icon_pixmap: Vec<Pixmap>,
    pub tooltip: Tooltip,
    pub item_is_menu: bool,
    /// The `Menu` object path, `None` when the item has no dbusmenu.
    pub menu_path: Option<String>,
}

impl Item {
    pub fn has_menu(&self) -> bool {
        self.menu_path.is_some()
    }

    pub fn icon_source(&self) -> IconSource<'_> {
        if self.status == Status::NeedsAttention {
            if !self.attention_icon_name.is_empty() {
                return IconSource::Named {
                    name: &self.attention_icon_name,
                    theme_path: &self.icon_theme_path,
                };
            }
        } else if !self.icon_name.is_empty() && self.overlay_icon_name.is_empty() {
            return IconSource::Named {
                name: &self.icon_name,
                theme_path: &self.icon_theme_path,
            };
        }
        IconSource::Pixmaps
    }
}

fn pixmaps(value: Option<Vec<(i32, i32, Vec<u8>)>>) -> Vec<Pixmap> {
    value
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(w, h, data)| Pixmap::from_wire(w, h, &data))
        .collect()
}

/// A property that some items send as `o` and others as `s`.
fn take_path(props: &Props, key: &str) -> Option<String> {
    prop::<OwnedObjectPath>(props, key).map(|p| p.to_string()).or_else(|| prop::<String>(props, key))
}

/// Fold a `GetAll` result into `item`. Missing properties keep their defaults, as every item
/// implements a different subset of the interface.
pub(crate) fn apply_properties(item: &mut Item, props: &Props) {
    item.id = prop::<String>(props, "Id").unwrap_or_default();
    item.title = prop::<String>(props, "Title").unwrap_or_default();
    if let Some(s) = prop::<String>(props, "Status").and_then(|s| Status::parse(&s)) {
        item.status = s;
    }
    item.icon_name = prop::<String>(props, "IconName").unwrap_or_default();
    item.icon_pixmap = pixmaps(prop(props, "IconPixmap"));
    item.icon_theme_path = prop::<String>(props, "IconThemePath").unwrap_or_default();
    item.overlay_icon_name = prop::<String>(props, "OverlayIconName").unwrap_or_default();
    item.overlay_icon_pixmap = pixmaps(prop(props, "OverlayIconPixmap"));
    item.attention_icon_name = prop::<String>(props, "AttentionIconName").unwrap_or_default();
    item.attention_icon_pixmap = pixmaps(prop(props, "AttentionIconPixmap"));
    item.tooltip = prop::<Structure<'static>>(props, "ToolTip")
        .and_then(|s| {
            let (icon, pix, title, description) =
                <(String, Vec<(i32, i32, Vec<u8>)>, String, String)>::try_from(s).ok()?;
            Some(Tooltip {
                icon_name: icon,
                icon_pixmap: pixmaps(Some(pix)),
                title,
                description,
            })
        })
        .unwrap_or_default();
    item.item_is_menu = prop::<bool>(props, "ItemIsMenu").unwrap_or(false);
    // Qt apps advertise "/NO_DBUSMENU" for an item with no menu.
    item.menu_path = take_path(props, "Menu").filter(|p| !p.is_empty() && p != "/NO_DBUSMENU");
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Toggle {
    #[default]
    None,
    Checkmark,
    Radio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CheckState {
    #[default]
    Unchecked,
    Checked,
    Partial,
}

/// One dbusmenu node. `children` holds every child; use [`MenuItem::visible_children`] for what
/// a menu draws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub id: i32,
    /// The label with mnemonic underscores removed.
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub separator: bool,
    pub icon_name: String,
    /// PNG bytes from `icon-data`.
    pub icon_png: Vec<u8>,
    pub toggle: Toggle,
    pub check: CheckState,
    /// `children-display == "submenu"`, or the root.
    pub has_children: bool,
    /// The first key equivalent of `shortcut`: its modifiers, then the key.
    pub shortcut: Vec<String>,
    /// `x-atoll-section`: a heading over the rows after it, not a choice.
    pub section: bool,
    pub children: Vec<MenuItem>,
}

impl MenuItem {
    pub(crate) fn new(id: i32) -> Self {
        Self {
            id,
            label: String::new(),
            enabled: true,
            visible: true,
            separator: false,
            icon_name: String::new(),
            icon_png: Vec::new(),
            toggle: Toggle::None,
            check: CheckState::Unchecked,
            has_children: false,
            shortcut: Vec::new(),
            section: false,
            children: Vec::new(),
        }
    }

    pub fn visible_children(&self) -> impl Iterator<Item = &MenuItem> {
        self.children.iter().filter(|c| c.visible)
    }

    pub fn find(&self, id: i32) -> Option<&MenuItem> {
        if self.id == id {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(id))
    }

    pub(crate) fn find_mut(&mut self, id: i32) -> Option<&mut MenuItem> {
        if self.id == id {
            return Some(self);
        }
        self.children.iter_mut().find_map(|c| c.find_mut(id))
    }

    /// A `GetLayout` node replaces the whole property set: absent keys are defaults.
    pub(crate) fn set_properties(&mut self, props: &Props) {
        let mut fresh = MenuItem::new(self.id);
        fresh.apply(props);
        fresh.children = std::mem::take(&mut self.children);
        *self = fresh;
    }

    /// An `ItemsPropertiesUpdated` entry: only the named keys move.
    pub(crate) fn update_properties(
        &mut self,
        updated: &Props,
        removed: &[String],
    ) {
        if updated.is_empty() && removed.is_empty() {
            return;
        }
        let defaults = MenuItem::new(self.id);
        for key in removed {
            self.reset(key, &defaults);
        }
        self.apply(updated);
    }

    fn reset(&mut self, key: &str, d: &MenuItem) {
        match key {
            "label" => self.label = d.label.clone(),
            "enabled" => self.enabled = d.enabled,
            "visible" => self.visible = d.visible,
            "icon-name" => self.icon_name = d.icon_name.clone(),
            "icon-data" => self.icon_png = Vec::new(),
            "type" => self.separator = false,
            "toggle-type" => self.toggle = Toggle::None,
            "toggle-state" => self.check = CheckState::Unchecked,
            "children-display" => self.has_children = false,
            "shortcut" => self.shortcut = Vec::new(),
            "x-atoll-section" => self.section = false,
            _ => {}
        }
    }

    fn apply(&mut self, props: &Props) {
        if let Some(label) = prop::<String>(props, "label") {
            self.label = clean_label(&label);
        }
        if let Some(v) = prop::<bool>(props, "enabled") {
            self.enabled = v;
        }
        if let Some(v) = prop::<bool>(props, "visible") {
            self.visible = v;
        }
        if let Some(v) = prop::<String>(props, "icon-name") {
            self.icon_name = v;
        }
        if let Some(v) = prop::<Vec<u8>>(props, "icon-data") {
            self.icon_png = v;
        }
        if let Some(v) = prop::<String>(props, "type") {
            self.separator = v == "separator";
        }
        if let Some(v) = prop::<String>(props, "toggle-type") {
            self.toggle = match v.as_str() {
                "checkmark" => Toggle::Checkmark,
                "radio" => Toggle::Radio,
                _ => Toggle::None,
            };
        }
        let state = prop::<i32>(props, "toggle-state")
            .or_else(|| prop::<bool>(props, "toggle-state").map(i32::from));
        if let Some(v) = state {
            self.check = match v {
                0 => CheckState::Unchecked,
                1 => CheckState::Checked,
                _ => CheckState::Partial,
            };
        }
        if let Some(v) = prop::<String>(props, "children-display") {
            self.has_children = v == "submenu";
        }
        if let Some(v) = prop::<Vec<Vec<String>>>(props, "shortcut") {
            self.shortcut = v.into_iter().next().unwrap_or_default();
        }
        if let Some(v) = prop::<bool>(props, "x-atoll-section") {
            self.section = v;
        }
    }
}

/// The mnemonic clean-up pass, quirks included: every `_` but a trailing one is dropped,
/// and the character after a dropped `_` is skipped, so `__` survives as one literal `_`.
pub(crate) fn clean_label(label: &str) -> String {
    let mut chars: Vec<char> = label.chars().collect();
    let mut i = 0;
    while i + 1 < chars.len() {
        if chars[i] == '_' {
            chars.remove(i);
        }
        i += 1;
    }
    chars.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mnemonic_underscores_follow_dbusmenu() {
        assert_eq!(clean_label("_Open"), "Open");
        assert_eq!(clean_label("Save _As"), "Save As");
        assert_eq!(clean_label("a__b"), "a_b");
        assert_eq!(clean_label("trailing_"), "trailing_");
    }

    #[test]
    fn pixmap_is_argb_to_rgba_and_rejects_short_data() {
        let pm = Pixmap::from_wire(1, 1, &[10, 20, 30, 40]).unwrap();
        assert_eq!(pm.rgba, vec![20, 30, 40, 10]);
        assert!(Pixmap::from_wire(2, 2, &[0; 8]).is_none());
        assert!(Pixmap::from_wire(0, 1, &[]).is_none());
    }

    #[test]
    fn closest_pixmap_prefers_the_smallest_adequate() {
        let mk = |n: i32| Pixmap {
            width: n,
            height: n,
            rgba: Vec::new(),
        };
        let all = [mk(16), mk(64), mk(32), mk(128)];
        assert_eq!(closest_pixmap(&all, 30, 30).unwrap().width, 32);
        assert_eq!(closest_pixmap(&all, 256, 256).unwrap().width, 128);
        assert!(closest_pixmap(&[], 16, 16).is_none());
    }
}
