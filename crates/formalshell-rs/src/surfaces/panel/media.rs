//! The active source's now-playing column. The source menu
//! trigger (Auto or a pinned pick) with its menu inline under it, the cover
//! beside the title, artist and album with the spectrum at the row's
//! trailing end, the position row (elapsed, the track, total) and the
//! transport beside the player's own volume.
//!
//! Every control is gated on what the source can do: no seek makes the
//! position row a readout, no shuffle or loop leaves those buttons out, no
//! volume leaves the volume out, AirPlay gets no transport at all (UxPlay
//! takes no remote command), and no source at all is the dim `No player`.
//!
//! The panel redraws on its own once per elapsed second while playing,
//! which keeps the readout and the fill on the position `media status`
//! reports; nothing ticks while paused.

use std::cell::Cell;
use std::time::{Duration, Instant};

use fs_media::visualizer::styles;

use super::{Effect, Panel, View};
use crate::services::devices::audio;
use crate::services::{lyrics, motion_art};
use crate::services::media::Active;
use crate::services::visualizer::{self, Avail};
use crate::store::{self, Topic};
use crate::ui::el::Opt;
use crate::ui::{El, Event, Ink, Type, What, w};

const SEEK_STEP: f64 = 5.0;
const VOLUME_STEP: f64 = 0.05;
const SPECTRUM_COLUMNS: usize = 12;

pub struct Media {
    /// The inline menu open under its trigger: "source", "output" or none.
    menu: &'static str,
    /// The transport button the keyboard sits on.
    transport: usize,
    /// Whether the cursor sits on a stop Left and Right step rather than walk,
    /// read off the last body.
    stepping: Cell<bool>,
    spectrum: Cell<bool>,
    open: bool,
    /// The cursor sat on a lyric row after the last key.
    in_lyrics: bool,
    /// The stops of each keyboard section present in the last body, in Tab
    /// order: the transport, the tracks, the menu.
    sections: std::cell::RefCell<Vec<Vec<String>>>,
}

impl Default for Media {
    fn default() -> Self {
        Self { menu: "", transport: 1, stepping: Cell::new(false), spectrum: Cell::new(false), open: true, in_lyrics: false, sections: Default::default() }
    }
}

impl Drop for Media {
    fn drop(&mut self) {
        motion_art::want(motion_art::PANEL, None);
        visualizer::set_panel(false);
        audio::routing_wanted(0, false);
    }
}

fn format_time(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

fn source_icon(kind: &str) -> &'static str {
    match kind {
        "radio" => "radio",
        "iphone" => "smartphone",
        "airplay" => "airplay",
        "stream" => "audio-lines",
        _ => "music",
    }
}

fn has_timeline(a: &Active) -> bool {
    a.kind == "mpris" || (a.kind == "iphone" && a.length > 0.0)
}

/// The transport, in the order the buttons draw.
fn transport(a: &Active) -> Vec<&'static str> {
    if a.kind == "stream" || a.kind == "airplay" {
        return Vec::new();
    }
    let mut out = vec!["previous", "playpause", "next"];
    if a.shuffle.is_some() {
        out.push("shuffle");
    }
    if a.loop_name.is_some() {
        out.push("loop");
    }
    out
}

fn transport_opt(a: &Active, id: &str) -> Opt {
    let icon = match id {
        "previous" => "skip-back",
        "playpause" if a.playing => "pause",
        "playpause" => "play",
        "next" => "skip-forward",
        "shuffle" => "shuffle",
        _ if a.loop_name == Some("track") => "repeat-1",
        _ => "repeat",
    };
    let mut o = Opt::new("").icon(icon);
    o.enabled = match id {
        "previous" => a.can_previous,
        "next" => a.can_next,
        _ => true,
    };
    o.active = match id {
        "shuffle" => a.shuffle == Some(true),
        "loop" => a.loop_name.is_some_and(|l| l != "none"),
        _ => false,
    };
    o
}

fn press(store: &store::Store, id: &str) {
    let m = &store.media;
    match id {
        "previous" => m.previous(),
        "playpause" => m.play_pause(),
        "next" => m.next(),
        "shuffle" => {
            if let Some(on) = m.active().and_then(|a| a.shuffle) {
                m.set_shuffle(!on);
            }
        }
        "loop" => {
            if let Some(l) = m.active().and_then(|a| a.loop_name) {
                m.set_loop(fs_media::media::next_loop(l));
            }
        }
        _ => {}
    }
}

fn set_follow(fx: &Effect, on: bool) {
    if fx.store.lyrics.follow != on {
        fx.service(move |ctx| ctx.publish(store::Diff::Lyrics(lyrics::Diff::Follow(on))));
    }
}

fn spectrum_enabled(v: &View) -> bool {
    v.store.config.bool("media.visualizer").unwrap_or(true)
}

impl Media {
    /// The rows of the open menu: source (Auto, then every player) or
    /// output (every sink, the one the source plays on checked).
    fn menu_rows(&self, v: &View) -> Vec<El> {
        let s = &v.theme.space;
        let m = &v.store.media;
        let mut rows = Vec::new();
        if self.menu == "output" {
            let on = m.output_id();
            rows.extend(m.outputs().iter().map(|(id, label)| (id.clone(), label.clone(), "speaker", false, *id == on)));
        } else {
            rows.push((String::new(), "Auto".to_owned(), "", false, m.selected.is_empty()));
            rows.extend(m.players().into_iter().map(|p| (p.id.clone(), p.label.clone(), source_icon(&p.kind), p.is_playing, m.selected == p.id)));
        }
        rows.into_iter()
            .map(|(id, label, icon, playing, current)| {
                let glyph = if current { "check" } else { icon };
                let mut parts = vec![
                    w::icon(if glyph.is_empty() { "check" } else { glyph })
                        .size(Type::BodySmall)
                        .ink(if glyph.is_empty() { Ink::Color(fs_theme::color::Rgba::TRANSPARENT) } else { Ink::Fg }),
                    w::text(label).size(Type::BodySmall).elide(),
                ];
                if playing {
                    parts.push(w::icon("play").size(Type::Caption).ink(Ink::Muted));
                }
                w::cell(w::row(s.icon_gap, parts)).selected(current).interactive().stop(format!("pick:{id}")).on(format!("pick:{id}"))
            })
            .collect()
    }

    fn pick(&mut self, id: &str, fx: &Effect) {
        if self.menu == "output" {
            fx.store.media.set_output(id);
            self.menu = "";
            return;
        }
        let id = id.to_owned();
        fx.service(move |ctx| ctx.publish(store::Diff::Media(crate::services::media::Diff::Select(id))));
        self.menu = "";
    }

    fn toggle_menu(&mut self, name: &'static str) {
        self.menu = if self.menu == name { "" } else { name };
    }

    fn step_volume(&self, fx: &Effect, by: f64) {
        if let Some(v) = fx.store.media.active().and_then(|a| a.volume) {
            fx.store.media.set_volume(v + by);
        }
    }

    fn seek_by(&self, fx: &Effect, by: f64) {
        if let Some(a) = fx.store.media.active() {
            fx.store.media.seek_to(a.position + by);
        }
    }
}

impl Panel for Media {
    fn id(&self) -> &'static str {
        "media"
    }

    fn title(&self, _: &View) -> String {
        "Media".into()
    }


    fn reads(&self) -> &'static [Topic] {
        &[Topic::Media, Topic::Visualizer, Topic::Config, Topic::Lyrics]
    }

    fn width(&self, v: &View) -> f64 {
        let s = &v.theme.space;
        if v.store.lyrics.synced() { s.popup_width_menu_split } else { s.popup_width_wide }
    }

    fn actions(&self, v: &View) -> Vec<El> {
        let mut out = vec![w::icon_button("radio").tip("Radio").on("radio").key("radio")];
        if v.store.media.active().is_some_and(|a| a.can_raise) {
            out.push(w::icon_button("external-link").on("raise").key("raise"));
        }
        out
    }

    fn opened(&mut self) {
        self.menu = "";
        self.in_lyrics = false;
        self.transport = 1;
    }

    fn closed(&mut self) {
        self.open = false;
        motion_art::want(motion_art::PANEL, None);
        visualizer::set_panel(false);
        audio::routing_wanted(0, false);
    }

    fn start(&mut self, fx: &mut Effect) {
        set_follow(fx, true);
        audio::routing_wanted(0, true);
        let on = fx.store.config.bool("media.visualizer").unwrap_or(true);
        self.spectrum.set(on);
        visualizer::set_panel(on);
    }

    fn cursor_start(&self) -> Option<String> {
        Some("transport".into())
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let m = &v.store.media;
        self.stepping.set(matches!(v.cursor, Some("transport" | "progress" | "volume")));
        let enabled = spectrum_enabled(v);
        if self.open && enabled != self.spectrum.get() {
            self.spectrum.set(enabled);
            visualizer::set_panel(enabled);
        }
        let Some(a) = m.active() else {
            motion_art::want(motion_art::PANEL, None);
            self.sections.borrow_mut().clear();
            let radio = w::icon_text_button("radio", "Radio").variant(crate::ui::Variant::Outline).stop("transport").on("radio");
            return w::column(s.section_gap, vec![w::row(0.0, vec![w::panel_empty(s, "No player").fill(), radio]).fill()]);
        };
        let mut col = Vec::new();

        let label = m.players().into_iter().find(|r| r.id == a.id).map(|r| r.label).unwrap_or_default();
        let label = if m.selected.is_empty() { format!("Auto · {label}") } else { label };
        let mut source = w::trigger(s, source_icon(a.kind), &label, self.menu == "source").stop("source").on("source");
        if self.menu == "source" {
            source = source.float(w::column(0.0, self.menu_rows(v)));
        }
        let mut triggers = vec![w::row(0.0, vec![source]).fill()];
        if m.can_route() {
            let on = m.output_id();
            let named = m.outputs().iter().find(|(id, _)| *id == on).map_or("Output", |(_, l)| l.as_str());
            let mut output = w::trigger(s, "speaker", named, self.menu == "output").stop("output").on("output");
            if self.menu == "output" {
                output = output.float(w::column(0.0, self.menu_rows(v)));
            }
            triggers.push(output);
        }
        col.push(w::row(s.sm, triggers).fill());

        let mut info = Vec::new();
        let mut motion = None;
        if !a.art_url.is_empty() {
            let slot = s.control_height * 3.0;
            let (px, radius) = w::cover_inner(v.theme, slot);
            // Animated album art over the static art: only while the panel
            // shows the slot, motion is on and the album has motion art.
            let animated = self.open
                && v.theme.motion_enabled
                && v.store.config.bool("media.appleMusicArt").unwrap_or(false)
                && !a.artist.is_empty()
                && !a.album.is_empty();
            let scale = crate::scene::raster_scale();
            motion = animated.then(|| motion_art::Want { artist: a.artist.clone(), album: a.album.clone(), size: px, radius, playing: a.playing }.at(scale));
            let frame = motion.as_ref().and_then(|w| m.motion(&w.key(), px, scale));
            info.push(w::cover(frame.or_else(|| m.cover(&a.art_url, px, radius, scale)), slot));
        }
        motion_art::want(motion_art::PANEL, motion);
        let mut words = vec![w::label(if a.title.is_empty() { "Unknown title".to_owned() } else { a.title.clone() }).size(Type::Title).elide()];
        if !a.artist.is_empty() {
            words.push(w::text(a.artist.clone()).elide());
        }
        if !a.album.is_empty() {
            words.push(w::text(a.album.clone()).size(Type::BodySmall).ink(Ink::Muted).elide());
        }
        info.push(w::column(s.xxs, words).fill());
        let vis = &v.store.visualizer;
        if enabled && (vis.avail == Avail::Available || a.kind == "iphone") {
            let style = vis.style(v.store.config.str("media.visualizerStyle").unwrap_or("bars"));
            let tip = styles::label(&style);
            info.push(w::spectrum(style, SPECTRUM_COLUMNS, vis.running).on("spectrum").tip(tip).key("spectrum"));
        }
        col.push(w::row(s.lg, info).fill());

        if has_timeline(&a) {
            let fraction = if a.length > 0.0 { a.position / a.length } else { 0.0 };
            let track = if a.can_seek { w::slider(fraction) } else { w::track(fraction) };
            col.push(
                w::row(
                    s.icon_gap,
                    vec![
                        w::value(format_time(a.position)).ink(Ink::Fg),
                        track.fill().stop("progress").on("progress"),
                        w::value(format_time(a.length)).ink(Ink::Muted),
                    ],
                )
                .fill()
                .pad(0.0, s.xs, 0.0, s.xs),
            );
        }

        let ids = transport(&a);
        let mut controls = Vec::new();
        if !ids.is_empty() {
            let options: Vec<Opt> = ids.iter().map(|id| transport_opt(&a, id)).collect();
            let index = self.transport.min(ids.len() - 1);
            let group = w::group(options, index, false).ring(index);
            controls.push(group.hug().stop("transport").on("transport"));
        }
        if let Some(volume) = a.volume {
            if !controls.is_empty() {
                controls.push(w::space(s.section_gap - s.icon_gap));
            }
            let glyph = if volume <= 0.0 {
                "volume-x"
            } else if volume < 0.5 {
                "volume-1"
            } else {
                "volume-2"
            };
            controls.push(w::icon(glyph).ink(Ink::Muted));
            controls.push(w::slider(volume).fill().stop("volume").on("volume"));
            controls.push(w::value(format!("{}%", (volume * 100.0).round())).ink(Ink::Muted).gauge("100%"));
        }
        if !controls.is_empty() {
            col.push(w::row(s.icon_gap, controls).fill());
        }
        let column = w::column(s.section_gap, col);
        let body = if v.store.lyrics.synced() {
            let settings = lyrics::Settings::read(&v.store.config);
            let ly = &v.store.lyrics;
            let cursor = v.cursor.and_then(|k| match k {
                "lyric-resync" => Some(ly.lines.len()),
                _ => k.strip_prefix("lyric:").and_then(|i| i.parse::<usize>().ok()),
            });
            let on_line = cursor.is_some_and(|c| c < ly.lines.len());
            let pane = El::new(crate::ui::el::Kind::Lyrics(crate::ui::lyrics::View {
                lines: ly.lines.clone(),
                t: fs_media::lyrics::led_position(a.position, settings.hold(ly.latency)),
                follow: ly.follow || on_line,
                cursor,
                blur: settings.blur,
                strength: settings.strength,
                height: s.control_height * 6.0,
            }))
            .key("lyrics");
            w::row(s.sm, vec![column.width(crate::ui::Size::Fill), pane]).top()
        } else {
            column
        };
        let mut sections = Vec::new();
        if !ids.is_empty() {
            sections.push(vec!["transport".to_owned()]);
        }
        let tracks: Vec<String> = [has_timeline(&a).then_some("progress"), a.volume.map(|_| "volume")].into_iter().flatten().map(str::to_owned).collect();
        if !tracks.is_empty() {
            sections.push(tracks);
        }
        if v.store.lyrics.synced() {
            let n = v.store.lyrics.lines.len();
            sections.push((0..n).map(|i| format!("lyric:{i}")).chain(std::iter::once("lyric-resync".to_owned())).collect());
        }
        let mut menu = vec!["source".to_owned()];
        if m.can_route() {
            menu.push("output".to_owned());
        }
        if self.menu == "source" {
            menu.extend(std::iter::once("pick:".to_owned()).chain(m.players().into_iter().map(|p| format!("pick:{}", p.id))));
        } else if self.menu == "output" {
            menu.extend(m.outputs().iter().map(|(id, _)| format!("pick:{id}")));
        }
        sections.push(menu);
        *self.sections.borrow_mut() = sections;
        body
    }

    fn wake(&self, v: &View) -> Option<Instant> {
        let a = v.store.media.active()?;
        if !a.playing || !has_timeline(&a) {
            return None;
        }
        // The lit set moves on the song's own clock, so a synced track redraws
        // every frame it plays; between line changes only the lit row's
        // nodes differ.
        if v.store.lyrics.synced() {
            return Some(Instant::now());
        }
        let into = a.position.fract();
        Some(Instant::now() + Duration::from_secs_f64((1.0 - into).max(0.01)))
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        if let Some(id) = ev.on.strip_prefix("pick:") {
            self.pick(id, fx);
            return;
        }
        if let Some(i) = ev.on.strip_prefix("lyric:").and_then(|i| i.parse::<usize>().ok()) {
            if let Some(line) = fx.store.lyrics.lines.get(i) {
                fx.store.media.seek_to(line.time);
            }
            return;
        }
        let a = fx.store.media.active();
        match (ev.on.as_str(), &ev.what) {
            ("radio", _) => fx.summon = Some(super::ATLAS),
            ("raise", _) => fx.store.media.raise(),
            ("lyrics-follow", _) => set_follow(fx, true),
            ("lyrics-pane", What::Scroll(..)) => set_follow(fx, false),
            ("source", _) => self.toggle_menu("source"),
            ("output", _) => self.toggle_menu("output"),
            ("transport", What::Pick(i)) => {
                if let Some(a) = &a
                    && let Some(id) = transport(a).get(*i)
                {
                    self.transport = *i;
                    press(fx.store, id);
                }
            }
            ("progress", What::Fraction(f)) => {
                if let Some(a) = &a {
                    fx.store.media.seek_to(f * a.length);
                }
            }
            ("progress", What::Wheel(d)) => self.seek_by(fx, *d as f64 * SEEK_STEP),
            ("volume", What::Fraction(f)) => fx.store.media.set_volume(*f),
            ("volume", What::Wheel(d)) => self.step_volume(fx, *d as f64 * VOLUME_STEP),
            ("spectrum", What::Click) => {
                let configured = fx.store.config.str("media.visualizerStyle").unwrap_or("bars").to_owned();
                let next = styles::step(&fx.store.visualizer.style(&configured), 1).to_owned();
                fx.service(move |ctx| ctx.publish(store::Diff::Visualizer(visualizer::Diff::Style(next))));
            }
            _ => {}
        }
    }

    fn activate(&mut self, stop: &str, fx: &mut Effect) {
        if let Some(id) = stop.strip_prefix("pick:") {
            self.pick(id, fx);
            return;
        }
        if let Some(i) = stop.strip_prefix("lyric:").and_then(|i| i.parse::<usize>().ok()) {
            if let Some(line) = fx.store.lyrics.lines.get(i) {
                fx.store.media.seek_to(line.time);
            }
            return;
        }
        match stop {
            "lyric-resync" => set_follow(fx, true),
            "transport" if fx.store.media.active().is_none() => fx.summon = Some(super::ATLAS),
            "source" => self.toggle_menu("source"),
            "output" => self.toggle_menu("output"),
            "transport" => {
                if let Some(a) = fx.store.media.active() {
                    let ids = transport(&a);
                    if let Some(id) = ids.get(self.transport.min(ids.len().saturating_sub(1))) {
                        let enabled = transport_opt(&a, id).enabled;
                        if enabled {
                            press(fx.store, id);
                        }
                    }
                }
            }
            "progress" => fx.store.media.play_pause(),
            _ => {}
        }
    }

    fn step(&mut self, stop: &str, direction: i32, fx: &mut Effect) {
        match stop {
            "transport" => {
                let n = fx.store.media.active().map_or(0, |a| transport(&a).len());
                if n > 0 {
                    self.transport = (self.transport as i32 + direction).clamp(0, n as i32 - 1) as usize;
                }
            }
            "progress" => self.seek_by(fx, direction as f64 * SEEK_STEP),
            "volume" => self.step_volume(fx, direction as f64 * VOLUME_STEP),
            _ => {}
        }
    }

    fn tab(&mut self, stop: Option<&str>, direction: i32) -> Option<String> {
        let sections = self.sections.borrow();
        if sections.is_empty() {
            return None;
        }
        let n = sections.len() as i32;
        let at = stop.and_then(|k| sections.iter().position(|s| s.iter().any(|x| x == k))).map_or(-1, |i| i as i32);
        let next = if at < 0 { 0 } else { (at + direction).rem_euclid(n) };
        sections[next as usize].first().cloned()
    }

    fn steps(&self) -> bool {
        self.stepping.get()
    }

    /// Reaching the lyrics parks the column back on the song: the cursor
    /// is about to drive it, and a column left where a wheel put it would
    /// answer the first Up with a jump.
    fn reached(&mut self, stop: &str, fx: &mut Effect) {
        let now = stop.starts_with("lyric");
        if now && !self.in_lyrics {
            set_follow(fx, true);
        }
        self.in_lyrics = now;
    }

    fn escape(&mut self) -> bool {
        !std::mem::take(&mut self.menu).is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_read_the_way_the_panel_writes_them() {
        assert_eq!(format_time(0.0), "00:00");
        assert_eq!(format_time(65.9), "01:05");
        assert_eq!(format_time(3725.0), "1:02:05");
        assert_eq!(format_time(-3.0), "00:00");
    }

    #[test]
    fn airplay_has_no_transport_and_a_player_its_own_toggles() {
        let airplay = Active { kind: "airplay", ..Active::default() };
        assert!(transport(&airplay).is_empty());
        let mpv = Active { kind: "mpris", shuffle: Some(true), loop_name: Some("track"), ..Active::default() };
        assert_eq!(transport(&mpv), ["previous", "playpause", "next", "shuffle", "loop"]);
        assert_eq!(transport_opt(&mpv, "loop").icon, "repeat-1");
        assert!(transport_opt(&mpv, "shuffle").active);
        assert!(!transport_opt(&mpv, "next").enabled);
    }

    #[test]
    fn the_phone_has_a_timeline_only_with_a_length() {
        assert!(!has_timeline(&Active { kind: "iphone", ..Active::default() }));
        assert!(has_timeline(&Active { kind: "iphone", length: 200.0, ..Active::default() }));
        assert!(!has_timeline(&Active { kind: "radio", ..Active::default() }));
    }
}
