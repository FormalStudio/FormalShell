use super::*;

/// The wallpaper route lists `dir`, and in
/// select mode answers `token` in picker-selection.txt instead of setting
/// the wallpaper.
#[derive(Clone, Debug, Default)]
pub struct Picker {
    pub select: bool,
    pub dir: String,
    pub token: String,
    /// `picker select` set this up for the open that follows.
    pub(super) pending: bool,
    /// The settings page it was summoned from, which leaving it returns to.
    pub from: Option<PanelId>,
}

/// One of the picker's sections: its id, heading, and the words at the
/// heading's far end.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PickerSection {
    pub id: &'static str,
    pub title: String,
    pub far: String,
}

pub const STYLE: &str = "style";

pub const YOURS: &str = "yours";

pub const INCLUDED: &str = "included";

/// A select request's one flat section.
pub(super) const FILES: &str = "files";

pub const PICKER_ROUTE: &str = "wallpaper";

pub fn picker_selection_path() -> std::path::PathBuf {
    state::state_path().with_file_name("picker-selection.txt")
}

/// The Style heading's far end: where settings.json pins the mode, so;
/// under `auto`, the mode and when the schedule next turns it.
pub(super) fn picker_style_note(store: &Store) -> String {
    use crate::services::theme;
    let key = theme::mode_key(store.config.settings());
    match key {
        "dark" | "light" => "Set in settings.json".into(),
        "auto" => {
            let data = &store.state.data;
            let times = store.theme.schedule.as_ref();
            let over = theme::parse_override(&data.mode_override);
            let mode = theme::effective_mode(key, times, over.as_ref(), &data.mode);
            let until = fs_theme::sun::next_change(&chrono::Local::now(), times);
            format!("Auto, {mode} until {}", until.format("%H:%M"))
        }
        _ => String::new(),
    }
}

/// An SPDX licence id as a heading names it: `CC0-1.0` reads `CC0`.
pub(super) fn licence_name(spdx: &str) -> String {
    match spdx.rsplit_once('-') {
        Some((name, version)) if !name.is_empty() && version.chars().all(|c| c.is_ascii_digit() || c == '.') => name.to_owned(),
        _ => spdx.to_owned(),
    }
}

pub(super) fn write_picker_selection(payload: &Value) {
    let path = picker_selection_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, payload.to_string());
}

impl Model {
    pub fn on_picker(&self) -> bool {
        self.mode == Mode::Menu && self.level.as_deref() == Some(PICKER_ROUTE)
    }

    /// The picker's tiles by section: the directory's pairs and singles
    /// under Your wallpapers, then the built-ins and the Sky under Included.
    /// A select request lists the directory's files alone, one flat section.
    pub fn picker_tiles(&self, store: &Store) -> Vec<(&'static str, fs_menu::providers::Item)> {
        use fs_menu::providers::{Item, ItemKind, Mode as Filed, image_basename};
        let p = &store.picker;
        let scanned = p.dir == self.picker.dir;
        if self.picker.select {
            let files = if scanned { p.scanned.as_slice() } else { &[] };
            return files
                .iter()
                .map(|f| (FILES, Item { id: f.clone(), kind: ItemKind::Single { path: f.clone(), filed: None }, title: image_basename(f).to_owned(), credit: None }))
                .collect();
        }
        let yours = if scanned { p.items.as_slice() } else { &[] };
        let sky = Item { id: "sky".into(), kind: ItemKind::Sky, title: "Sky".into(), credit: None };
        let style = [(Filed::Light, "light", "Light"), (Filed::Dark, "dark", "Dark")].map(|(mode, name, title)| {
            let path = store.desktop_for(name);
            (STYLE, Item { id: format!("mode-{name}"), kind: ItemKind::Mode { mode, path }, title: title.into(), credit: None })
        });
        style
            .into_iter()
            .chain(yours.iter().map(|i| (YOURS, i.clone())))
            .chain(p.included.iter().chain([&sky]).map(|i| (INCLUDED, i.clone())))
            .collect()
    }

    /// The sections the picker shows, in order.
    pub(super) fn picker_sections(&self, store: &Store) -> Vec<PickerSection> {
        if self.picker.select {
            return vec![PickerSection { id: FILES, ..Default::default() }];
        }
        let home = crate::services::menu::home();
        let dir = match self.picker.dir.strip_prefix(home.trim_end_matches('/')) {
            Some(rest) if !home.is_empty() && rest.starts_with('/') => format!("~{rest}"),
            _ => self.picker.dir.clone(),
        };
        let mut licences: Vec<&str> = store.picker.included.iter().filter_map(|i| i.credit.as_ref()).map(|c| c.license.as_str()).collect();
        licences.dedup();
        let licence = match licences.as_slice() {
            [one] => licence_name(one),
            _ => String::new(),
        };
        vec![
            PickerSection { id: STYLE, title: "Style".into(), far: picker_style_note(store) },
            PickerSection { id: YOURS, title: "Your wallpapers".into(), far: dir },
            PickerSection { id: INCLUDED, title: "Included".into(), far: licence },
        ]
    }

    pub(super) fn picker_rows(&self, store: &Store, query: &str) -> Vec<Node> {
        let dark = store.state.data.mode == "dark";
        let sky = crate::services::wallpaper::sky_source(store.sky_step());
        let tiles = self.picker_tiles(store);
        let mut rows = Vec::new();
        for section in [FILES, STYLE, YOURS, INCLUDED] {
            let items: Vec<fs_menu::providers::Item> = tiles.iter().filter(|(s, _)| *s == section).map(|(_, i)| i.clone()).collect();
            rows.extend(fs_menu::providers::tile_rows(&items, section, dark, &sky, query));
        }
        rows
    }

    /// The tile a row stands for.
    pub(super) fn picker_tile_of(&self, store: &Store, id: &str) -> Option<fs_menu::providers::Item> {
        let id = id.strip_prefix("wallpaper.")?;
        self.picker_tiles(store).into_iter().map(|(_, i)| i).find(|i| i.id == id)
    }

    /// The tile holds the slot the mode shows: a pair both slots, a single
    /// the picture shown, the Sky while it is up.
    pub fn picker_current(store: &Store, item: &fs_menu::providers::Item) -> bool {
        use fs_menu::providers::ItemKind;
        match &item.kind {
            ItemKind::Pair { light, dark } => store.desktop_for("light") == *light && store.desktop_for("dark") == *dark,
            ItemKind::Single { path, .. } => store.desktop() == *path,
            ItemKind::Sky => crate::services::wallpaper::sky(&store.desktop()).is_some(),
            ItemKind::Mode { .. } => false,
        }
    }

    /// A Style preview stands for the mode in effect.
    pub fn picker_mode_on(store: &Store, item: &fs_menu::providers::Item) -> bool {
        use fs_menu::providers::{ItemKind, Mode as Filed};
        match item.kind {
            ItemKind::Mode { mode, .. } => (mode == Filed::Dark) == (store.state.data.mode == "dark"),
            _ => false,
        }
    }

    /// Enter or a click on a Style preview: `theme mode` under its own rules
    /// (a snooze under `auto`, nothing where settings.json pins the mode,
    /// which the section's far end says), the card left open on the
    /// previews.
    pub(super) fn picker_style(&mut self, store: &Store, mode: fs_menu::providers::Mode) -> Out {
        let name = if mode == fs_menu::providers::Mode::Dark { "dark" } else { "light" };
        let key = crate::services::theme::mode_key(store.config.settings());
        if let Ok(mode) = crate::services::theme::request_mode(name, &store.state.data.mode, key)
            && mode != store.state.data.mode
        {
            state::set(vec![state::Field::Mode(mode)]);
        }
        self.placed = true;
        self.dirty = true;
        Out::None
    }

    /// A tile's picture size: `pickerTile` wide at the least, as many
    /// columns as fit the card's content an `lg` apart, 16:9. The cursor's
    /// ring sits `xs` outside the picture, inside the content.
    pub fn picker_tile_px(&self, theme: &Theme) -> (f64, f64) {
        let s = &theme.space;
        let width = self.content_width(theme) - s.xs * 2.0;
        let columns = fs_menu::appgrid::columns_for(width + s.lg, s.picker_tile + s.lg).max(1);
        let w = ((width - s.lg * (columns - 1) as f64) / columns as f64).floor().max(1.0);
        (w, (w * 9.0 / 16.0).round())
    }

    /// `openImageSelect`.
    pub fn open_image_select(&mut self, store: &Store, dir: &str, token: &str) {
        self.picker_abandon();
        let dir = crate::services::picker::directory(if dir.is_empty() { crate::settings::PICKER_DIRECTORY.get(&store.config) } else { dir });
        self.picker = Picker { select: true, dir, token: token.into(), pending: true, from: None };
        self.open(store, Some(PICKER_ROUTE));
    }

    /// `chooseImage`: false off the route or for a path no tile holds. A
    /// pair sets both slots, a picture for both modes both, one filed under
    /// a mode its slot and that mode (unless settings.json pins the mode),
    /// the Sky the sky over both.
    pub fn choose_image(&mut self, store: &Store, path: &str) -> bool {
        use fs_menu::providers::{ItemKind, Mode as Filed};
        if !self.open || !self.on_picker() {
            return false;
        }
        let sky = path == fs_theme::theme::SKY || crate::services::wallpaper::sky(path).is_some();
        let tiles = self.picker_tiles(store);
        let Some((_, item)) = tiles.iter().find(|(_, i)| if sky { i.kind == ItemKind::Sky } else { i.files().contains(&path) }) else {
            return false;
        };
        if self.picker.select {
            write_picker_selection(&json!({"token": self.picker.token, "value": path}));
            self.picker.token.clear();
        } else {
            match &item.kind {
                ItemKind::Mode { .. } => return false,
                ItemKind::Sky => state::set_wallpaper(state::Choice::Both(fs_theme::theme::SKY.into()), None),
                ItemKind::Pair { light, dark } => state::set_wallpaper(state::Choice::Pair(light.clone(), dark.clone()), None),
                ItemKind::Single { path, filed: None } => state::set_wallpaper(state::Choice::Both(path.clone()), None),
                ItemKind::Single { path, filed: Some(filed) } => {
                    let (half, name) = if *filed == Filed::Light { (state::Half::Light, "light") } else { (state::Half::Dark, "dark") };
                    let key = crate::services::theme::mode_key(store.config.settings());
                    let mode = crate::services::theme::request_mode(name, &store.state.data.mode, key).ok();
                    state::set_wallpaper(state::Choice::Slot(half, path.clone()), mode.as_deref());
                }
            }
        }
        self.close();
        true
    }

    /// `picker chooseFor`: one single for one mode's slot, the mode left as
    /// it is. False off the route, in select mode, or for anything but a
    /// listed single.
    pub fn choose_for(&mut self, store: &Store, path: &str, half: state::Half) -> bool {
        use fs_menu::providers::ItemKind;
        if !self.open || !self.on_picker() || self.picker.select {
            return false;
        }
        let single = self.picker_tiles(store).iter().any(|(_, i)| matches!(&i.kind, ItemKind::Single { path: p, .. } if p == path));
        if !single {
            return false;
        }
        state::set_wallpaper(state::Choice::Slot(half, path.into()), None);
        self.close();
        true
    }

    /// `pickerStatus`.
    pub fn picker_status(&self, store: &Store) -> Value {
        use fs_menu::providers::{ItemKind, Mode as Filed};
        let tiles = self.picker_tiles(store);
        let sections = self.picker_sections(store);
        let filed = |m: &Option<Filed>| m.map(|m| if m == Filed::Light { "light" } else { "dark" });
        let items: Vec<Value> = tiles
            .iter()
            .map(|(section, i)| {
                let credit = i.credit.clone().unwrap_or_default();
                let (kind, light, dark, filing) = match &i.kind {
                    ItemKind::Pair { light, dark } => ("pair", Some(light.as_str()), Some(dark.as_str()), None),
                    ItemKind::Single { path, filed: f } => {
                        let f = filed(f);
                        ("single", (f != Some("dark")).then_some(path.as_str()), (f != Some("light")).then_some(path.as_str()), f)
                    }
                    ItemKind::Sky => ("sky", None, None, None),
                    ItemKind::Mode { mode: Filed::Light, path } => ("mode", Some(path.as_str()), None, None),
                    ItemKind::Mode { mode: Filed::Dark, path } => ("mode", None, Some(path.as_str()), None),
                };
                json!({
                    "id": i.id,
                    "section": section,
                    "kind": kind,
                    "title": i.title,
                    "author": (!credit.author.is_empty()).then_some(credit.author),
                    "license": (!credit.license.is_empty()).then_some(credit.license),
                    "source": (!credit.source.is_empty()).then_some(credit.source),
                    "light": light,
                    "dark": dark,
                    "filed": filing,
                    "current": !self.picker.select && (Self::picker_current(store, i) || Self::picker_mode_on(store, i)),
                })
            })
            .collect();
        let count = |id: &str| tiles.iter().filter(|(s, _)| *s == id).count();
        json!({
            "open": self.open && self.on_picker(),
            "mode": if self.picker.select { "select" } else { "wallpaper" },
            "directory": self.picker.dir,
            "included": (!store.picker.included_dir.is_empty()).then_some(&store.picker.included_dir),
            "count": tiles.len(),
            "cursor": self.cursor,
            "cursorId": self.cursor_id,
            "cachedThumbnails": tiles.iter().filter(|(_, i)| store.picker.warm(i)).count(),
            "current": {"light": store.desktop_for("light"), "dark": store.desktop_for("dark"), "shown": store.desktop()},
            "sections": sections.iter().map(|s| json!({"id": s.id, "title": s.title, "far": s.far, "count": count(s.id)})).collect::<Vec<_>>(),
            "items": items,
        })
    }

    pub(super) fn picker_enter(&mut self, store: &Store) {
        if !self.picker.pending {
            self.picker_abandon();
            self.picker.select = false;
            self.picker.dir = crate::services::picker::directory(crate::settings::PICKER_DIRECTORY.get(&store.config));
            self.picker.token.clear();
            self.picker.from = None;
        }
        self.picker.pending = false;
        crate::services::picker::command(crate::services::picker::Cmd::Scan(self.picker.dir.clone()));
    }

    pub(super) fn picker_abandon(&mut self) {
        if self.picker.select && !self.picker.token.is_empty() {
            write_picker_selection(&json!({"token": self.picker.token, "cancelled": true}));
            self.picker.token.clear();
        }
    }

    /// The picker's sections down the body: each heading, then its tiles
    /// `lg` apart in as many columns as `width` holds, a section's first row
    /// carrying its heading as their band. Your wallpapers with nothing in
    /// it says so under its heading. Returns the content's foot.
    pub(super) fn picker_layout(&self, out: &mut Layout, theme: &Theme, kit: &mut Kit, width: f64, heading_h: f64, top: f64) -> f64 {
        let s = &theme.space;
        let (gap, ring) = (s.lg, s.xs);
        let (tile_w, pic_h) = self.picker_tile_px(theme);
        let caption_h = ui::measure(&w::caption("Ag"), width, theme, kit).1;
        // A slot is the picture and its caption with the ring's margin
        // round them; the pictures stand `lg` apart either way.
        let (slot_w, slot_h) = (tile_w + ring * 2.0, pic_h + s.row_gap + caption_h + ring * 2.0);
        let pitch = slot_h - ring * 2.0 + gap;
        let columns = (((width - ring * 2.0 + gap) / (tile_w + gap)).floor() as usize).max(1);
        let mut y = top;
        let mut first = true;
        for sec in &self.picker_sections {
            let tiles: Vec<usize> = (0..self.rows.len()).filter(|&i| self.sections.get(i).map(String::as_str) == Some(sec.id)).collect();
            let note = tiles.is_empty() && sec.id == YOURS && self.query.is_empty();
            if tiles.is_empty() && !note {
                continue;
            }
            if !first {
                y += s.section_gap;
            }
            first = false;
            let band = if sec.title.is_empty() { 0.0 } else { heading_h + s.row_gap };
            if band > 0.0 {
                out.headings.push((sec.title.clone(), y, ring - s.control_padding_x, sec.far.clone()));
            }
            if note {
                let line_h = ui::measure(&w::text("Ag"), width, theme, kit).1;
                out.notes.push((y + band, format!("No pictures in {}", sec.far), "Set picker.directory in settings.json".into()));
                y += band + line_h + s.row_gap + caption_h;
                continue;
            }
            // GNOME centres its two Style previews over the page
            // (`cc-background-panel.blp`).
            let indent = if sec.id == STYLE && tiles.len() < columns { (columns - tiles.len()) as f64 * (tile_w + gap) / 2.0 } else { 0.0 };
            for (k, _) in tiles.iter().enumerate() {
                let (r, c) = (k / columns, k % columns);
                let x = indent + c as f64 * (tile_w + gap);
                out.slots.push(match r {
                    0 => Slot { x, y, w: slot_w, h: band + slot_h, band },
                    _ => Slot { x, y: y + band + r as f64 * pitch, w: slot_w, h: slot_h, band: 0.0 },
                });
            }
            y += band + tiles.len().div_ceil(columns) as f64 * pitch - gap + ring * 2.0;
        }
        out.columns = columns;
        out.cell_w = tile_w;
        out.cell_h = pitch;
        y
    }

    /// One key's move over the picker's tiles, by where they sit: Up and
    /// Down to the nearest tile of the row above or under, across
    /// sections; a page a viewport of rows; Left and Right in reading order.
    pub(super) fn picker_step(&self, dir: Dir) -> nav::Step {
        let slots = &self.layout.slots;
        let n = slots.len().min(self.rows.len());
        let i = self.cursor.min(n.saturating_sub(1));
        let snap = |index| nav::Step { index, travels: false };
        let Some(at) = slots.get(i) else { return snap(0) };
        // A row's tiles share the top of their picture.
        let top = |k: usize| slots[k].y + slots[k].band;
        let mut rows: Vec<f64> = (0..n).map(top).collect();
        rows.sort_by(f64::total_cmp);
        rows.dedup_by(|a, b| (*a - *b).abs() < 0.5);
        let row = rows.iter().position(|y| (*y - top(i)).abs() < 0.5).unwrap_or(0);
        let nearest = |r: usize| (0..n).filter(|&k| (top(k) - rows[r]).abs() < 0.5).min_by(|&a, &b| (slots[a].x - at.x).abs().total_cmp(&(slots[b].x - at.x).abs()));
        let page = ((self.body_h / self.layout.cell_h.max(1.0)).floor() as usize).max(1);
        match dir {
            Dir::Home => snap(0),
            Dir::End => snap(n - 1),
            Dir::Left if i > 0 => nav::Step { index: i - 1, travels: true },
            Dir::Right if i + 1 < n => nav::Step { index: i + 1, travels: true },
            Dir::Up if row > 0 => nearest(row - 1).map_or(snap(i), |k| nav::Step { index: k, travels: true }),
            Dir::Down if row + 1 < rows.len() => nearest(row + 1).map_or(snap(i), |k| nav::Step { index: k, travels: true }),
            Dir::PageUp => nearest(row.saturating_sub(page)).map_or(snap(i), snap),
            Dir::PageDown => nearest((row + page).min(rows.len() - 1)).map_or(snap(i), snap),
            _ => snap(i),
        }
    }
}
