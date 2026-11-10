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

pub fn snapshot(store: &Store) -> Snapshot {
    let radio = &store.media.radio;
    let l = &store.lights;
    let ipad = store.ipad.bridge.as_ref();
    Snapshot {
        nightlight: store.nightlight.active,
        overnight: store.state.data.overnight.is_some(),
        caffeinate: store.caffeinate.active,
        dnd: store.notifications.model.dnd,
        dark: store.state.data.mode == "dark",
        lights_on: l.brightness > 0,
        hdr: crate::services::display::hdr_active(store),
        ipad_active: ipad.is_some_and(|i| i.active),
        lights_effect: l.effect.clone(),
        lights_source: l.source.clone(),
        lights_colour: if l.source == "custom" { l.custom_colour.clone() } else { String::new() },
        lights_speed: l.speed.clone(),
        lights_brightness: l.brightness.to_string(),
        wifi_ssid: store.network.ssid.as_ref().map_or_else(String::new, |(s, _)| s.clone()),
        audio_sink: store.audio.default_sink.clone(),
        audio_source: store.audio.default_source.clone(),
        radio_station: radio.station.as_ref().filter(|_| radio.running()).map_or_else(String::new, |s| s.uuid.clone()),
        ipad_edge: ipad.map_or_else(String::new, |i| i.edge.as_str().to_owned()),
        ipad_stretch: ipad
            .and_then(|i| fs_menu::providers::IPAD_STRETCHES.iter().find(|(_, _, a, b)| i.stretch.near(*a, *b)))
            .map_or_else(String::new, |(id, ..)| (*id).to_owned()),
        bluetooth_connected: store.bluetooth.devices.iter().filter(|x| x.connected).map(|x| x.address.to_string()).collect(),
    }
}

pub(super) fn radio_station(s: &fs_media::radio::stations::Station) -> fs_menu::providers::RadioStation {
    fs_menu::providers::RadioStation { uuid: s.uuid.clone(), name: s.name.clone(), country: s.country.clone(), codec: s.codec.clone() }
}

/// RadioSearchProvider.rowsFor: the answer for `q` once it is in, a
/// searching row until then. Asking is idempotent; the service debounces.
pub fn radio_search_rows(store: &Store, q: &str) -> Vec<Node> {
    use fs_menu::providers as p;
    let q = q.trim();
    if q.chars().count() < 2 {
        return Vec::new();
    }
    crate::services::radio::send(crate::services::radio::Cmd::Search(q.to_owned()));
    let radio = &store.media.radio;
    match &radio.search {
        Some((answered, found)) if answered == q => match found {
            None => vec![p::radio_failed_row()],
            Some(v) if v.is_empty() => vec![p::radio_no_results_row()],
            Some(v) => {
                let favs: std::collections::HashSet<String> = radio.favorites.iter().map(|s| s.uuid.clone()).collect();
                let list: Vec<_> = v.iter().take(50).map(radio_station).collect();
                p::radio_result_rows(&list, &favs)
            }
        },
        _ => vec![p::radio_searching_row()],
    }
}

pub(super) fn wifi_network(r: &crate::services::devices::network::Row) -> fs_menu::providers::WifiNetwork {
    fs_menu::providers::WifiNetwork {
        name: r.ssid.clone(),
        known: r.known,
        connected: r.connected,
        secured: r.secured,
        enterprise: r.enterprise,
        in_range: r.in_range,
        signal: r.signal.as_fraction(),
        security: r.security.clone(),
        addresses: r.addresses.clone(),
    }
}

/// A network stepped into: its details off the live scan, a note once it
/// is no longer listed.
pub(super) fn network_rows(store: &Store, ssid: &str, q: &str) -> Vec<Node> {
    let Some(row) = store.network.row(ssid) else { return vec![Node::note("wifi.details.gone", "No longer listed")] };
    let q = q.trim().to_lowercase();
    fs_menu::providers::wifi_details(&wifi_network(row)).into_iter().filter(|n| q.is_empty() || n.label.to_lowercase().contains(&q)).collect()
}

/// The four device routes' rows, off the store.
pub fn device_sources(store: &Store) -> Vec<(Provider, Vec<Node>)> {
    use fs_menu::providers as p;
    let n = &store.network;
    let wifi = p::WifiState {
        has_device: n.wifi_device.is_some(),
        enabled: n.wifi_enabled,
        networks: n.rows.iter().map(wifi_network).collect(),
        action_ssid: n.action.as_ref().map(|(_, s)| s.clone()).unwrap_or_default(),
        action_kind: n.action.as_ref().map(|(k, _)| k.as_str().to_owned()).unwrap_or_default(),
        failure_ssid: n.failure.as_ref().map(|f| f.ssid.clone()).unwrap_or_default(),
        failure_text: n.failure.as_ref().map(|f| f.text.clone()).unwrap_or_default(),
    };
    let b = &store.bluetooth;
    let bluetooth = p::BluetoothState { available: b.powered.is_some(), enabled: b.powered == Some(true), devices: b.devices.clone() };
    let audio: Vec<p::AudioDevice> =
        store.audio.devices.iter().map(|(name, label, sink)| p::AudioDevice { name: name.clone(), label: label.clone(), is_sink: *sink }).collect();
    let r = &store.media.radio;
    let radio = p::RadioState { running: r.running(), favorites: r.favorites.iter().map(radio_station).collect() };
    vec![
        (Provider::Wifi, p::wifi_rows(&wifi)),
        (Provider::Bluetooth, p::bluetooth_rows(&bluetooth)),
        (Provider::Audio, p::audio_rows(&audio)),
        (Provider::Radio, p::radio_rows(&radio)),
    ]
}

/// Every launcher plugin with a mode, as `(route id, name)`.
pub fn plugin_modes(store: &Store) -> Vec<(String, String)> {
    fs_chrome::plugins::launcher_plugins(&store.plugins.resolved.plugins)
        .filter_map(|p| Some((fs_chrome::plugins::surface_key(p), p.mode.clone()?)))
        .collect()
}

/// Each plugin mode's rows off what its plugin last printed, or the one dim
/// row that says why there are none.
pub fn plugin_sources(store: &Store) -> Vec<(Provider, Vec<Node>)> {
    use crate::services::plugins::Run as Printed;
    let state = &store.plugins;
    fs_chrome::plugins::launcher_plugins(&state.resolved.plugins)
        .filter(|p| p.mode.is_some())
        .map(|p| {
            let route = fs_chrome::plugins::surface_key(p);
            // Only the mode's own view shows why it has no rows.
            let note = |text: String| vec![Node { local_only: true, ..Node::note(format!("{route}.note"), text) }];
            let rows = match state.runs.get(&p.id) {
                None => note(format!("Loading {}", p.name)),
                Some(Printed::Failed(why)) => note(format!("{} failed: {why}", p.name)),
                Some(Printed::Live(d)) => {
                    let rows = plugin_rows(&p.id, &route, &d.rows);
                    if rows.is_empty() { note("No results".into()) } else { rows }
                }
            };
            (Provider::Plugin(route), rows)
        })
        .collect()
}

/// The verb that writes `line` to plugin `id`'s stdin.
pub(super) fn plugin_send(id: &str, line: &str) -> Intent {
    Intent::PluginSend(id.to_owned(), line.to_owned())
}

/// A plugin mode's rows: each activates its row, then its own actions; a
/// `label` row heads the rows under it.
pub(super) fn plugin_rows(id: &str, route: &str, rows: &[plugins::Row]) -> Vec<Node> {
    use plugins::RowKind;
    let mut section = None;
    let mut out = Vec::new();
    for r in rows {
        if r.kind == RowKind::Label {
            section = Some(r.text.clone()).filter(|t| !t.is_empty());
            continue;
        }
        let toggle = r.kind == RowKind::Toggle;
        let activate = plugins::activate_event(&r.id, toggle.then_some(!r.checked));
        let mut n = Node::new(format!("{route}.{}", r.id), r.text.clone(), NodeKind::Action).with_action(plugin_send(id, &activate));
        n.icon = r.icon.clone();
        n.desc = if toggle { Some(if r.checked { "On" } else { "Off" }.to_owned()) } else { Some(r.detail.clone()).filter(|d| !d.is_empty()) };
        n.verb = toggle.then(|| "Toggle".to_owned());
        n.keep_open = toggle.then_some(true);
        n.section = section.clone();
        n.actions = r
            .actions
            .iter()
            .map(|a| {
                let label = if a.label.is_empty() { &a.id } else { &a.label };
                let run = plugin_send(id, &plugins::action_event(&r.id, &a.id));
                ActionEntry { icon: Some(a.icon.clone()).filter(|i| !i.is_empty()), ..ActionEntry::new(&a.id, label, run) }
            })
            .collect();
        out.push(n);
    }
    out
}

/// An app subject's windows as rows, each a window subject: its title
/// over the app's icon, its workspace trailing.
pub(super) fn window_rows(store: &Store, subject: &str, q: &str) -> Vec<Node> {
    let Some(app) = model::subject_node(store.menu.nodes(), subject) else { return Vec::new() };
    let Some(entry) = &app.entry else { return Vec::new() };
    let snap = &store.compositor.snapshot;
    let q = q.trim().to_lowercase();
    app_windows(store, entry)
        .iter()
        .filter_map(|id| store.compositor.window(id))
        .filter(|w| q.is_empty() || w.title.to_lowercase().contains(&q))
        .map(|w| Node {
            path: w.id.to_string(),
            icon: app.icon.clone(),
            icon_source: app.icon_source.clone(),
            meta: snap.workspaces.iter().find(|ws| ws.id == w.workspace_id).map(|ws| ws.name.clone()),
            ..Node::new(
                format!("window:{}", w.id),
                if w.title.is_empty() { app.label.clone() } else { w.title.clone() },
                NodeKind::Window,
            )
        })
        .collect()
}

/// A folder stepped into: its entries as subjects, a note while the
/// service reads it or when it cannot, or when it holds nothing.
pub(super) fn folder_rows(store: &Store, path: &str, q: &str) -> Vec<Node> {
    match store.menu.folder.as_ref().filter(|(p, _)| p == path).map(|(_, l)| l) {
        None => vec![Node::note("files.reading", "Reading folder")],
        Some(Err(why)) => vec![Node::note("files.unreadable", why.clone())],
        Some(Ok(entries)) => {
            let rows = fs_menu::providers::listing_rows(path, entries, q);
            if rows.is_empty() && q.is_empty() { vec![Node::note("files.empty", "Empty folder")] } else { rows }
        }
    }
}

/// LocalSend's peers off its last scan, each trailing its address; with
/// none, one note saying why.
pub(super) fn peer_targets(store: &Store) -> Vec<Node> {
    let ls = &store.localsend;
    if !ls.installed {
        return vec![Node::note("peer:none", "LocalSend is not installed")];
    }
    let peers: Vec<Node> = ls
        .peers
        .iter()
        .enumerate()
        .map(|(i, p)| Node { meta: Some(p.ip.clone()), ..Node::new(format!("peer:{i}"), p.name.clone(), NodeKind::Target) })
        .collect();
    match (peers.is_empty(), ls.scanning || ls.last_scan_at == 0.0) {
        (false, _) => peers,
        (true, true) => vec![Node::note("peer:scanning", "Looking for devices\u{2026}")],
        (true, false) => vec![Node::note("peer:none", "No devices found")],
    }
}

/// What a target action picks from. Workspaces: the focused output's, by
/// slot, then the next free one as a new one. Apps: every installed one,
/// by name. Peers: LocalSend's last scan, which a listing asks again.
pub(super) fn targets_for(store: &Store, kind: TargetKind) -> Vec<Node> {
    match kind {
        TargetKind::App => {
            let mut apps: Vec<Node> = model::visible_children(store.menu.nodes(), Some("apps"), &store.menu.cond)
                .into_iter()
                .filter_map(|n| n.entry.as_ref().map(|e| Node::new(format!("target-app:{}", e.id), n.label.clone(), NodeKind::Target)))
                .collect();
            apps.sort_by_key(|n| n.label.to_lowercase());
            apps
        }
        TargetKind::Peer => {
            use crate::services::localsend;
            localsend::command(localsend::Cmd::Scan { force: false });
            peer_targets(store)
        }
        TargetKind::Workspace => {
            let snap = &store.compositor.snapshot;
            let output = &snap.focused_output_name;
            let mut slots: Vec<(i64, String)> = snap
                .workspaces
                .iter()
                .filter(|w| w.idx >= 1 && (output.is_empty() || w.output == *output))
                .map(|w| (w.idx, w.name.clone()))
                .collect();
            slots.sort_by_key(|(idx, _)| *idx);
            slots.dedup_by_key(|(idx, _)| *idx);
            // One past every output's: Hyprland's idx is global, so one past
            // this output's alone can name another output's workspace.
            let next = snap.workspaces.iter().map(|w| w.idx).max().unwrap_or(0).max(0) + 1;
            let row = |idx: i64, meta: Option<String>| Node {
                meta,
                ..Node::new(format!("workspace:{idx}"), format!("Workspace {idx}"), NodeKind::Workspace)
            };
            let mut out: Vec<Node> = slots.into_iter().map(|(idx, name)| row(idx, (!name.is_empty() && name != idx.to_string()).then_some(name))).collect();
            out.push(row(next, Some("New".into())));
            out
        }
    }
}

pub(super) fn row_height(node: &Node, theme: &Theme, kit: &mut Kit) -> f64 {
    let s = &theme.space;
    let row = launcher_px(fs_theme::tokens::LAUNCHER.row_height);
    if node.emoji_only {
        let h = ui::measure(&w::text("Ag").size(Type::Display), 100.0, theme, kit).1;
        return row.max(h + s.control_padding_y * 2.0);
    }
    row
}

/// A clipboard entry's glyph by what it holds, as Pond marks them: the
/// picture itself shows in the pane, never in the row.
pub(super) fn clip_glyph(row: &Node) -> Option<&'static str> {
    let clip = fs_menu::providers::clipboard_entry_id(row).is_some() || row.id.starts_with("share.history.");
    if !clip || row.emoji_only {
        return None;
    }
    let text = row.full_text.trim();
    Some(if !row.thumb_source.is_empty() {
        "image"
    } else if !text.contains(char::is_whitespace) && (text.starts_with("http://") || text.starts_with("https://") || text.starts_with("www.")) {
        "link"
    } else {
        "text"
    })
}

/// A section heading over the rows: the panels' `caption` 600 in `muted`
/// (panel-language spec §5), on the rows' text column.
pub(super) fn heading_el(s: &fs_theme::tokens::Space, text: &str) -> El {
    w::text(text.to_owned()).size(Type::Caption).weight(Weight::Semibold).ink(Ink::Muted).pad_start(s.control_padding_x)
}

/// A list row's words, the label, description and value alike, at Pond's
/// one row size.
pub(super) fn row_words(text: String) -> El {
    w::text(text).size(Type::Row).elide()
}

/// A picker tile: its picture 16:9 with the current mark and a filed
/// single's mode badge, the caption under it, the cursor's ring round both.
#[allow(clippy::too_many_arguments)]
pub(super) fn picker_tile(m: &Model, store: &Store, theme: &Theme, tiles: &[(&str, fs_menu::providers::Item)], row: &Node, i: usize, selected: bool, slot: &Slot) -> El {
    use fs_menu::providers::{ItemKind, Mode as Filed};
    let s = &theme.space;
    let item = tiles.iter().map(|(_, t)| t).find(|t| row.id.strip_prefix("wallpaper.") == Some(t.id.as_str()));
    let (w, h) = m.picker_tile_px(theme);
    let key = item.map(|t| crate::services::picker::tile_key(t, &row.path)).unwrap_or_default();
    let image = store.picker.thumbs.get(&key).cloned().flatten();
    let current = !m.picker.select && item.is_some_and(|t| Model::picker_current(store, t));
    let ringed = item.is_some_and(|t| Model::picker_mode_on(store, t));
    let badge = match item.map(|t| &t.kind) {
        Some(ItemKind::Single { filed: Some(Filed::Dark), .. }) if !m.picker.select => Some("moon"),
        Some(ItemKind::Single { filed: Some(Filed::Light), .. }) if !m.picker.select => Some("sun"),
        _ => None,
    };
    let by = match item.map(|t| &t.kind) {
        Some(ItemKind::Sky) => "Follows the sun".to_owned(),
        _ => item.and_then(|t| t.credit.as_ref()).map(|c| c.author.clone()).unwrap_or_default(),
    };
    let mut words = vec![w::text(row.label.clone()).size(Type::Caption).weight(Weight::Medium).elide().hug()];
    if !by.is_empty() {
        words.push(w::text(format!("\u{B7} {by}")).size(Type::Caption).weight(Weight::Medium).ink(Ink::Muted).elide());
    }
    let caption = w::row(s.xs, words).width(Size::Px(w));
    let content = w::column(s.row_gap, vec![w::tile_picture(image, w, h, current, ringed, badge), caption]);
    w::cell(content)
        .ghost()
        .interactive()
        .cell_state(|st| {
            st.cursor = selected && m.from_keys;
            st.inset = Some((s.xs, s.xs));
        })
        .on(format!("row:{i}"))
        .stop(format!("p:{}", row.id))
        .width(Size::Px(slot.w))
        .height(slot.h - slot.band)
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

/// Where an item sits across the body, off its key.
pub(super) fn slot_x_of(m: &Model, el: &El) -> f64 {
    let Some(key) = &el.key else { return 0.0 };
    let id = key.split_once(':').map_or("", |(_, id)| id);
    m.rows.iter().position(|r| r.id == id).and_then(|i| m.layout.slots.get(i)).map_or(0.0, |s| s.x)
}

/// One row: a `row_icon` mark, the label at 400 (600 under the selection),
/// a dim trailing value, `lg` either side.
#[allow(clippy::too_many_arguments)]
pub(super) fn menu_row(m: &Model, store: &Store, row: &Node, i: usize, selected: bool, checked: bool, theme: &Theme) -> El {
    let s = &theme.space;
    let confirming = m.confirming(&row.id);
    let icon_px = launcher_px(fs_theme::tokens::LAUNCHER.row_icon);
    let live = fs_menu::providers::is_package(row).then(|| crate::surfaces::packages::live(store, &row.path)).flatten();
    let checked = crate::surfaces::packages::row_checked(store, row, checked);
    let mut lead = Vec::new();
    let ringed = live.as_ref().is_some_and(|l| l.ring.is_some());
    if let Some(ring) = live.as_ref().and_then(|l| l.ring) {
        lead.push(w::ring(ring).width(Size::Px(icon_px)).height(icon_px));
    }
    // A plugin's rows and its mode carry icon names from the active set,
    // where every other node's own icon is a raw glyph.
    let glyph = match clip_glyph(row).unwrap_or(fs_menu::icons::icon_for(Some(row))) {
        "" if row.id.starts_with(fs_chrome::plugins::PLUGIN_PREFIX) => row.icon.as_str(),
        named => named,
    };
    let picture = (!row.icon_source.is_empty() && glyph.is_empty())
        .then(|| store.menu.row_icons.get(&row.icon_source).cloned().flatten())
        .flatten();
    // An emoji in Recent is its own picture, drawn as text.
    let emoji = matches!(fs_menu::actions::subject_id(row), SubjectId::Emoji(_));
    let drawn = if !glyph.is_empty() {
        glyph.to_owned()
    } else if picture.is_none() && !row.emoji_only && !emoji {
        fs_menu::icons::fallback_for(Some(row)).to_owned()
    } else {
        String::new()
    };
    if ringed {
        // The ring stands in the icon slot.
    } else if let Some(image) = picture {
        let density = image.pixmap.width() as f64 / icon_px.max(1.0);
        lead.push(w::picture(Some(image.at(density)), icon_px));
    } else if !drawn.is_empty() {
        lead.push(w::icon(drawn).size(Type::Heading).width(Size::Px(icon_px)).height(icon_px));
    } else if emoji {
        lead.push(w::text(row.icon.clone()).size(Type::Heading).centred().width(Size::Px(icon_px)));
    }
    let label = if confirming { crate::ui::confirm::ask(&row.label) } else { row.label.clone() };
    let mut name = row_words(label);
    if row.dim == Some(true) {
        name = name.ink(Ink::Dim);
    }
    if row.emoji_only {
        name = name.size(Type::Display);
    }
    lead.push(name);
    if let Some(desc) = row.desc.clone().filter(|d| !d.is_empty()) {
        lead.push(row_words(desc).ink(Ink::Dim));
    }
    let windows = match &row.entry {
        Some(entry) if row.kind == NodeKind::App && row.meta.as_deref().unwrap_or("").is_empty() => app_windows(store, entry).len(),
        _ => 0,
    };
    let value = match windows {
        _ if live.is_some() => live.map(|l| l.value).unwrap_or_default(),
        0 => fs_menu::actions::accessory_for(Some(row)),
        1 => "1 window".into(),
        n => format!("{n} windows"),
    };
    let mut trail = Vec::new();
    if !value.is_empty() {
        trail.push(row_words(value).ink(Ink::Dim).end());
    }
    if checked && !confirming {
        trail.push(w::icon("check"));
    }
    let mut parts = vec![w::row(s.lg, lead).fill()];
    if !trail.is_empty() {
        parts.push(w::row(s.icon_gap, trail).width(Size::AtMost(0.4)));
    }
    w::cell(w::row(s.lg, parts).fill())
        .ghost()
        .selected(selected)
        .interactive()
        .cell_state(|st| {
            st.row = true;
            st.destructive = confirming;
        })
        .on(format!("row:{i}"))
        .stop(format!("r:{}", row.id))
}
