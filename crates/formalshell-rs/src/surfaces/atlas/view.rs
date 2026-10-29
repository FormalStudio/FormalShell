// Portions from omarchy-radio-atlas (MIT, Copyright 2026 Akshar Patel)

//! The overlay's card and the atlas's layout: the card buds off
//! the top line the way the launcher's does and rests centred on the
//! output; the header, the globe pane and the sidebar are drawn into it.

use std::time::Instant;

use fs_media::radio::model as rm;
use fs_theme::theme::Theme;
use vello_cpu::kurbo::{Affine, Rect};

use super::globe::Ink as GlobeInk;
use super::{Atlas, PREFERRED};
use crate::scene::{IRect, NodeId, Paint};
use crate::services::radio;
use crate::surfaces::bar::cell::{Kit, Painter};
use crate::surfaces::modal::Modal;
use crate::ui::el::Opt;
use crate::ui::{self, El, Ink, Size, Type, Ui, Variant, Weight, w};

const KEYBOARD: [(&[&str], &str); 10] = [
    (&["/"], "Search"),
    (&["Up", "Down"], "Select station"),
    (&["Enter"], "Play selected station"),
    (&["Space"], "Play or pause"),
    (&["R"], "Tune randomly"),
    (&["F"], "Favorite selected station"),
    (&["M"], "Mute or unmute"),
    (&["+", "-"], "Change volume"),
    (&["Esc"], "Back, clear, or close"),
    (&["?"], "Show or hide controls"),
];

const MOUSE: [(&str, &str); 4] = [
    ("Drag or flick", "Spin globe"),
    ("Wheel on globe", "Zoom"),
    ("Click a signal", "Play station"),
    ("Click a country", "Browse stations"),
];

/// The atlas's own size off the output's.
pub fn content_size(output: (f64, f64)) -> (f64, f64) {
    ((PREFERRED.0.min(output.0 * 0.85)).round(), (PREFERRED.1.min(output.1 * 0.85)).round())
}

pub struct Shown {
    pub modal: Modal,
    head: Ui,
    foot: Ui,
    side: Ui,
    list: Ui,
    player: Ui,
    help: Ui,
    tip: Ui,
    globe_node: NodeId,
    tip_box: NodeId,
    rules: Vec<NodeId>,
    output: (f64, f64),
    scale: f64,
    pub hover: Option<String>,
    pub animating_content: bool,
}

impl Shown {
    pub const DEFORM_AMOUNT: f64 = 0.1;

    pub fn new(mut modal: Modal, output: (f64, f64), scale: f64) -> Self {
        let card = &mut modal.card;
        let top = card.top_node();
        let globe_node = card.scene.add_after(Some(top), IRect::default(), Paint::Rect { fill: fs_theme::color::Rgba::TRANSPARENT, radius: 0.0 });
        let tip_box = card.scene.add_after(Some(globe_node), IRect::default(), Paint::Rect { fill: fs_theme::color::Rgba::TRANSPARENT, radius: 0.0 });
        card.scene.set_visible(globe_node, false);
        card.scene.set_visible(tip_box, false);
        Self {
            modal,
            head: Ui::new(Some(top)),
            foot: Ui::new(Some(top)),
            side: Ui::new(Some(top)),
            list: Ui::new(Some(top)),
            player: Ui::new(Some(top)),
            help: Ui::new(Some(top)),
            tip: Ui::new(Some(tip_box)),
            globe_node,
            tip_box,
            rules: Vec::new(),
            output,
            scale,
            hover: None,
            animating_content: false,
        }
    }

    pub fn animating(&self, now: Instant) -> bool {
        self.modal.animating(now) || self.animating_content
    }

    pub fn hit(&self, x: f64, y: f64) -> Option<ui::Hit> {
        [&self.head, &self.side, &self.list, &self.player, &self.help, &self.foot].iter().find_map(|u| u.hit(x, y)).cloned()
    }

    pub fn set_hover(&mut self, path: Option<String>) -> bool {
        let changed = self.hover != path;
        for u in [&mut self.head, &mut self.side, &mut self.list, &mut self.player, &mut self.help, &mut self.foot] {
            u.hover = path.clone();
        }
        self.hover = path;
        changed
    }

    pub fn layout(&mut self, a: &mut Atlas, r: &radio::State, theme: &Theme, kit: &mut Kit, now: Instant) {
        let s = &theme.space;
        let (cw, ch) = content_size(self.output);
        let (w_, h_) = (cw + s.panel_padding * 2.0, ch + s.panel_padding * 2.0);
        let x = ((self.output.0 - w_) / 2.0).round();
        let y = ((self.output.1 - h_) / 2.0).round();
        self.modal.place(Rect::new(x, y, x + w_, y + h_), now);
        self.draw(a, r, theme, kit, now, (cw, ch));
    }

    #[allow(clippy::too_many_lines)]
    fn draw(&mut self, a: &mut Atlas, r: &radio::State, theme: &Theme, kit: &mut Kit, now: Instant, (cw, chh): (f64, f64)) {
        let (frame, alpha) = self.modal.card.content;
        let clip = self.modal.card.clip;
        let s = theme.space.clone();
        let p = s.panel_padding;
        let bw = theme.border_width;
        let (ox, oy) = (frame.x as f64 + p, frame.y as f64 + p);
        let ctl = s.control_height;
        self.animating_content = false;
        for u in [&mut self.head, &mut self.foot, &mut self.side, &mut self.list, &mut self.player, &mut self.help, &mut self.tip] {
            u.motion_scale = self.scale;
        }
        let scene = &mut self.modal.card.scene;
        let mut wake = false;

        // The header.
        let field_w = 330.0_f64.min(cw * 0.32);
        let head = w::row(
            s.sm,
            vec![
                w::text("Radio Atlas").size(Type::Title),
                w::spacer(),
                w::row(0.0, vec![w::field(&a.search, "Search station, country, or genre", a.editing, None).fill().on("search")]).width(Size::Px(field_w)),
                w::icon_button("shuffle").tip("Tune randomly").on("shuffle"),
                w::icon_button("circle-help")
                    .variant(if a.help { Variant::Selected } else { Variant::Ghost })
                    .tip(if a.help { "Hide controls" } else { "Show controls" })
                    .on("help"),
                w::icon_button("x").tip("Close").on("close"),
            ],
        )
        .fill();
        wake |= self.head.draw(&head, Rect::new(ox, oy, ox + cw, oy + ctl), Some(clip), alpha, theme, kit, scene, now).animating;

        let rule_y = (oy + ctl + p).round();
        let body_top = rule_y + bw;
        let sidebar_w = 390.0_f64.min(cw * 0.39);
        let side_x = ox + cw - sidebar_w;
        let vrule_x = (side_x - p - bw).round();
        let map_right = vrule_x;
        let pl = player_el(a, r, &s);
        let player_h = ui::measure(&pl, sidebar_w, theme, kit).1;
        let player_top = oy + chh - player_h;
        let player_rule = (player_top - p - bw).round();

        let sep = theme.box_style("separator", None).fill;
        let sep = sep.with_alpha(sep.a * alpha);
        let mut rules = Painter::new(scene, &mut self.rules, Some(clip));
        let bwi = bw.round().max(1.0) as i32;
        rules.rect(IRect::new(frame.x, rule_y as i32, frame.w, bwi), sep, 0.0);
        if !a.help {
            rules.rect(IRect::new(vrule_x as i32, body_top as i32, bwi, (frame.y + frame.h) - body_top as i32), sep, 0.0);
            let px = (side_x - p).round() as i32;
            rules.rect(IRect::new(px, player_rule as i32, frame.x + frame.w - px, bwi), sep, 0.0);
        }
        rules.finish();

        if a.help {
            for u in [&mut self.foot, &mut self.side, &mut self.list, &mut self.player, &mut self.tip] {
                u.hide(scene);
            }
            scene.set_visible(self.globe_node, false);
            scene.set_visible(self.tip_box, false);
            a.globe.leave();
            let width = cw.min(920.0);
            let gap = s.section_gap * 3.0;
            let col_w = (width - gap) / 2.0;
            let keyboard: Vec<El> = KEYBOARD
                .iter()
                .map(|(keys, action)| {
                    w::row(0.0, vec![w::row(0.0, vec![w::chord_keys(&s, keys)]).width(Size::Px(col_w * 0.4)), w::text(*action).ink(Ink::Muted).elide().fill()])
                        .pad_start(s.control_padding_x)
                })
                .map(|e| w::row(0.0, vec![w::column(0.0, vec![w::space(ctl)]).width(Size::Px(0.0)), e.fill()]))
                .collect();
            let mouse: Vec<El> = MOUSE
                .iter()
                .map(|(input, action)| {
                    w::row(
                        0.0,
                        vec![
                            w::column(0.0, vec![w::space(ctl)]).width(Size::Px(0.0)),
                            w::label(*input).elide().width(Size::Px(col_w * 0.4)).pad_start(s.control_padding_x),
                            w::text(*action).ink(Ink::Muted).elide().fill(),
                        ],
                    )
                })
                .collect();
            let section = |title: &str, rows: Vec<El>| {
                let mut col = vec![w::section_label(&s, title, None, true)];
                col.extend(rows);
                w::column(s.row_gap, col).width(Size::Px(col_w)).top()
            };
            let help = w::row(gap, vec![section("Keyboard", keyboard), section("Mouse", mouse)]).top();
            let hx = ox + (cw - width) / 2.0;
            let hy = body_top + s.section_gap;
            wake |= self.help.draw(&help, Rect::new(hx, hy, hx + width, hy + chh), Some(clip), alpha, theme, kit, scene, now).animating;
            return self.finish_wake(wake, now);
        }
        self.help.hide(scene);

        // The map pane's foot: the hint and the signal count.
        let error = if a.fetch_error.is_empty() { r.local_error.clone() } else { a.fetch_error.clone() };
        let hint = if !error.is_empty() {
            w::caption(error).ink(Ink::Destructive)
        } else if !a.active_name.is_empty() {
            w::caption(format!("{} · click another country to browse", a.active_name))
        } else {
            w::caption("Drag or flick to spin · wheel to zoom · click a signal or country")
        };
        let count = w::caption(format!("{} signals", a.geo.len())).mono();
        let foot = w::row(s.section_gap, vec![hint.elide().fill(), count]).fill();
        let foot_w = map_right - p - ox;
        let (_, hint_h) = ui::measure(&foot, foot_w, theme, kit);
        let foot_y = oy + chh - hint_h;
        wake |= self.foot.draw(&foot, Rect::new(ox, foot_y, ox + foot_w, foot_y + hint_h), Some(clip), alpha, theme, kit, scene, now).animating;

        // The globe.
        let g_top = body_top + p;
        let g_bottom = foot_y - s.md;
        a.globe.rect = (ox, g_top, (map_right - p - ox).max(0.0), (g_bottom - g_top).max(0.0));
        a.globe.set_selected(r.station.clone().or_else(|| a.selected.clone()));
        a.globe.active_country = a.active_code.clone();
        let c = |n: &str| theme.colors.get(n);
        let ink = GlobeInk { sphere: c("background"), land: c("muted"), grid: c("border"), outline: c("mutedForeground"), signal: c("primary"), accent: c("primary"), radius: theme.radii.lg };
        let ops = a.globe.paint(&ink, alpha);
        let (gx, gy, gw, gh) = a.globe.rect;
        let bounds = IRect::new(gx.floor() as i32, gy.floor() as i32, gw.ceil() as i32 + 1, gh.ceil() as i32 + 1);
        scene.update_with(self.globe_node, bounds, Paint::Vector(ops), true, Affine::IDENTITY, Some(bounds.intersect(&clip)));

        // The tooltip over the globe: the station under the pointer, or the
        // one a coast landed near.
        let station = a.globe.hovered.clone().or_else(|| a.globe.highlighted.clone());
        let landing = a.globe.hovered.is_none() && a.globe.highlighted.is_some();
        let shown = station.as_ref().filter(|_| !a.globe.grabbing() && !a.globe.kinetic);
        match shown {
            Some(st) => {
                let anchor = if a.globe.hovered.is_some() { a.globe.hover } else { a.globe.highlight };
                let popover = theme.colors.get("popoverForeground");
                let label = format!("{}{}", if landing { "Landed near " } else { "" }, st.name);
                let column = |elide: bool| {
                    let text = w::text(label.clone()).size(Type::BodySmall).ink(Ink::Color(popover));
                    let mut lines = vec![if elide { text.elide() } else { text.hug() }];
                    if st.estimated_location {
                        let caption = w::caption("Approximate location");
                        lines.push(if elide { caption } else { caption.hug() });
                    }
                    let col = w::column(s.xxs, lines);
                    if elide { col } else { col.hug() }
                };
                let content = column(true);
                let max_w = s.popup_width_narrow.min((gw - s.md * 2.0).max(0.0));
                let (tw, _) = ui::measure(&column(false), max_w, theme, kit);
                let bw_ = max_w.min(tw + s.control_padding_x * 2.0);
                let (_, th) = ui::measure(&content, (bw_ - s.control_padding_x * 2.0).max(0.0), theme, kit);
                let bh = th + s.control_padding_y * 2.0;
                let tx = (gw - bw_ - s.md).min(s.md.max(anchor.0 + s.xl));
                let ty = (gh - bh - s.md).min(s.md.max(anchor.1 + s.xl));
                let rect = IRect::new((gx + tx).round() as i32, (gy + ty).round() as i32, bw_.round() as i32, bh.round() as i32);
                let b = theme.box_style("popover", None);
                let radius = theme.box_radius(&b, rect.h as f64) as f32;
                let (border, width) = b.border.as_ref().map_or((fs_theme::color::Rgba::TRANSPARENT, 0.0), |l| (l.color, l.width as f32));
                let fade = |c: fs_theme::color::Rgba| c.with_alpha(c.a * alpha);
                scene.update_with(
                    self.tip_box,
                    rect,
                    Paint::Framed { fill: fade(b.fill), radius, border: fade(border), width },
                    true,
                    Affine::IDENTITY,
                    Some(clip),
                );
                let inner = Rect::new(
                    rect.x as f64 + s.control_padding_x,
                    rect.y as f64 + s.control_padding_y,
                    rect.right() as f64 - s.control_padding_x,
                    rect.bottom() as f64 - s.control_padding_y,
                );
                self.tip.draw(&content, inner, Some(clip), alpha, theme, kit, scene, now);
            }
            None => {
                scene.set_visible(self.tip_box, false);
                self.tip.hide(scene);
            }
        }

        // The sidebar's tabs.
        let tabs_y = body_top + p;
        let names = ["World", "cliamp", "Favorites", "Recent"];
        let tabs = w::group(names.iter().map(|n| Opt::new(*n)).collect(), a.mode.tab().unwrap_or(usize::MAX), true).fill().on("tab");
        let tabs_h = ctl;
        wake |= self.side.draw(&tabs, Rect::new(side_x, tabs_y, side_x + sidebar_w, tabs_y + tabs_h), Some(clip), alpha, theme, kit, scene, now).animating;

        // The station list, or the output picker in its place.
        let list_top = tabs_y + ctl + s.section_gap;
        let list_bottom = player_rule - p;
        let ring = theme.ring_width();
        let view = IRect::new(
            (side_x - ring).floor() as i32,
            (list_top - ring).floor() as i32,
            (sidebar_w + ring * 2.0).ceil() as i32,
            (list_bottom - list_top + ring * 2.0).max(0.0).ceil() as i32,
        )
        .intersect(&clip);
        a.list_h = (list_bottom - list_top).max(0.0);
        a.row_h = ctl * 2.0 + bw;
        let list = if a.outputs { outputs_el(a, r, &s) } else { list_el(a, r, &s, theme, kit, sidebar_w) };
        let (origin, el) = list;
        let top = if origin { list_top } else { list_top - a.scroll };
        wake |= self.list.draw(&el, Rect::new(side_x, top, side_x + sidebar_w, top + a.list_h.max(1.0)), Some(view), alpha, theme, kit, scene, now).animating;

        // The player.
        wake |= self.player.draw(&pl, Rect::new(side_x, player_top, side_x + sidebar_w, player_top + player_h), Some(clip), alpha, theme, kit, scene, now).animating;
        self.finish_wake(wake, now);
    }

    fn finish_wake(&mut self, wake: bool, _now: Instant) {
        self.animating_content = wake;
    }
}

/// The list's El, and whether it is drawn from the list's top rather than
/// scrolled.
fn list_el(a: &Atlas, r: &radio::State, s: &fs_theme::tokens::Space, theme: &Theme, kit: &mut Kit, width: f64) -> (bool, El) {
    let stations = a.display(r);
    if stations.is_empty() {
        let loading = a.fetching && a.remote();
        let el = if loading {
            w::section_label(s, "Loading stations", None, false)
        } else {
            let failed = !a.fetch_error.is_empty() || !r.local_error.is_empty();
            w::para(a.empty_text(r), Type::Body, Weight::Normal, if failed { Ink::Destructive } else { Ink::Muted }, 4).mid()
        };
        let inner = (width - s.panel_padding * 2.0).max(0.0);
        let (_, h) = ui::measure(&el, inner, theme, kit);
        let pad = ((a.list_h - h) / 2.0).max(0.0);
        return (true, w::column(0.0, vec![w::space(pad), el.pad(s.panel_padding, 0.0, s.panel_padding, 0.0)]).fill());
    }
    let row_h = a.row_h.max(1.0);
    let first = (a.scroll / row_h).floor().max(0.0) as usize;
    let last = (((a.scroll + a.list_h) / row_h).ceil() as usize).min(stations.len());
    let playing = r.station.as_ref().map(|s| s.uuid.as_str()).unwrap_or("");
    let mut rows = vec![w::space(first as f64 * row_h)];
    for (i, st) in stations.iter().enumerate().take(last).skip(first) {
        let is_playing = playing == st.uuid;
        let favorite = r.is_favorite(&st.uuid);
        let name = w::text(st.name.clone()).weight(if is_playing { Weight::Semibold } else { Weight::Medium }).ink(Ink::Fg).elide();
        let meta = w::caption(rm::station_meta(Some(st))).mono().ink(Ink::Dim).elide();
        let star = w::icon_button("star")
            .variant(if favorite { Variant::Default } else { Variant::Ghost })
            .tip(if favorite { "Remove favorite" } else { "Add favorite" })
            .on(format!("fav:{i}"));
        let content = w::row(
            0.0,
            vec![strut(s.control_height * 2.0 - s.control_padding_y * 2.0), w::column(s.xxs, vec![name, meta]).fill(), w::space(s.icon_gap), star],
        )
        .fill();
        let cursor = a.keyboard_selection && a.selected_index == Some(i);
        let cell = w::cell(content).ghost().interactive().selected(is_playing).cell_state(|c| c.cursor = cursor).on(format!("row:{i}")).key(st.uuid.clone());
        let tail = if i + 1 < stations.len() { w::separator() } else { w::space(theme.border_width) };
        rows.push(w::column(0.0, vec![cell, tail]).fill());
    }
    (false, w::column(0.0, rows).fill())
}

fn outputs_el(a: &Atlas, r: &radio::State, s: &fs_theme::tokens::Space) -> (bool, El) {
    let mut col = vec![w::section_label(s, "Audio output", None, true)];
    let choices = std::iter::once((String::new(), "System default".to_owned())).chain(r.outputs.iter().map(|o| (o.id.clone(), o.label.clone())));
    for (i, (id, label)) in choices.enumerate() {
        let cursor = a.output_cursor == i;
        col.push(
            w::cell(w::text(label).ink(Ink::Fg).elide().fill())
                .ghost()
                .interactive()
                .selected(id == r.output)
                .cell_state(|c| c.cursor = cursor)
                .on(format!("output:{i}"))
                .key(format!("output:{id}")),
        );
    }
    if r.outputs.is_empty() && !r.outputs_loading {
        let failed = !r.outputs_error.is_empty();
        let text = if failed { r.outputs_error.clone() } else { "No other audio outputs found".to_owned() };
        col.push(w::caption(text).ink(if failed { Ink::Destructive } else { Ink::Muted }).pad_start(s.control_padding_x));
    }
    (true, w::column(s.row_gap, col).fill())
}

fn player_el(a: &Atlas, r: &radio::State, s: &fs_theme::tokens::Space) -> El {
    let running = r.running();
    let name = match &r.station {
        Some(st) if running => {
            let n = st.name.trim();
            if !n.is_empty() {
                n.to_owned()
            } else if !r.title.is_empty() {
                r.title.clone()
            } else {
                "Unknown station".to_owned()
            }
        }
        _ => "Nothing playing".to_owned(),
    };
    let failed = !r.player_error.is_empty() || !r.error.is_empty();
    let status = if !r.player_error.is_empty() {
        r.player_error.clone()
    } else if !r.error.is_empty() {
        format!("{}. Play to retry, or Next.", r.error)
    } else if !running {
        "Choose a signal to begin".to_owned()
    } else if !r.loaded {
        "Connecting".to_owned()
    } else {
        let track = r.track_title();
        let mut t = if track.is_empty() { String::new() } else { format!("{track} · ") };
        t.push_str(if r.paused() { "Paused" } else { "Live" });
        if r.queue.len() > 1 {
            t.push_str(&format!(" · {} stations queued", r.queue.len()));
        }
        t
    };
    let mut top = vec![
        strut(s.control_height),
        w::column(
            s.xxs,
            vec![
                w::text(name).weight(if running { Weight::Semibold } else { Weight::Medium }).elide(),
                w::caption(status).ink(if failed { Ink::Destructive } else { Ink::Muted }).elide(),
            ],
        )
        .fill(),
    ];
    top.push(w::space(s.icon_gap));
    if running {
        let uuid = r.station.as_ref().map(|s| s.uuid.as_str()).unwrap_or("");
        let fav = r.is_favorite(uuid);
        top.push(
            w::icon_button("star")
                .variant(if fav { Variant::Default } else { Variant::Ghost })
                .tip(if fav { "Remove from favorites" } else { "Add to favorites" })
                .on("playing-fav"),
        );
    } else {
        top.push(w::space(s.control_height));
    }
    let opt = |icon: &str, enabled: bool| {
        let mut o = Opt::new("").icon(icon);
        o.enabled = enabled;
        o
    };
    let transport = w::group(
        vec![
            opt("skip-back", running),
            opt(if running && !r.paused() { "pause" } else { "play" }, running || a.selected.is_some()),
            opt("skip-forward", running),
            opt("square", running),
        ],
        usize::MAX,
        false,
    )
    .height(s.control_height)
    .on("transport");
    let tip = if r.output.is_empty() { "Choose audio output".to_owned() } else { format!("Audio output: {}", r.output_label(&r.output)) };
    let controls = w::row(
        s.sm,
        vec![
            transport,
            w::spacer(),
            w::icon_button("headphones").variant(if a.outputs { Variant::Selected } else { Variant::Ghost }).tip(tip).on("outputs"),
            w::icon_button("volume-x")
                .variant(if r.muted { Variant::Default } else { Variant::Ghost })
                .enabled(running)
                .tip(if r.muted { "Unmute" } else { "Mute" })
                .on("mute"),
        ],
    )
    .fill();
    let glyph = if r.volume > 0 { if r.volume < 50 { "volume-1" } else { "volume-2" } } else { "volume-x" };
    let volume = w::row(
        0.0,
        vec![
            strut(s.control_height),
            w::icon(glyph).ink(Ink::Muted).pad_start(s.control_padding_x),
            w::space(s.icon_gap),
            w::slider(r.volume as f64 / 100.0).fill().on("volume"),
            w::space(s.icon_gap),
            w::value(format!("{}%", r.volume)).ink(Ink::Muted),
        ],
    )
    .fill();
    w::column(s.row_gap, vec![w::row(0.0, top).fill(), controls, volume]).fill()
}

/// Holds a row at least `h` tall, the height these rows are given outright.
fn strut(h: f64) -> El {
    w::column(0.0, vec![w::space(h)]).width(Size::Px(0.0))
}
