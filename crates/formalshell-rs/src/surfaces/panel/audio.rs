//! A hero for the default sink (its rail the master slider,
//! its mute a `Switch`), then Output, Input and Apps as one group each:
//! every sink with the default checked, the source's level over every
//! source, and the live playback streams. One inset level throughout
//! (specs/2026-10-30-panel-language.md), so no picker sits boxed in a group.
//!
//! One cursor walks every row: Left and Right step the slider under it by
//! 5%, `m` mutes, Enter switches the default on a device row and mutes on a
//! slider. Masters clamp to 0..1, streams
//! to 0..1.5 with a notch at 1.0. The lists come from fs-audio's graph,
//! published only while the panel is open.

use fs_audio::Command;
use fs_system::audio::{clamp_device, clamp_stream};

use super::{Effect, Panel, View};
use crate::services::devices::audio::{self, Lists, Row};
use crate::store::{Store, Topic};
use crate::ui::{El, Event, What, w};

/// AudioModel's step.
const STEP: f64 = 0.05;

#[derive(Default)]
pub struct Audio;

fn send(fx: &Effect, c: Command) {
    fx.service(move |_| audio::command(c));
}

fn set_volume(fx: &Effect, node: u32, v: f64) {
    send(fx, Command::SetVolume { node, volume: v as f32 });
}

fn set_muted(fx: &Effect, node: u32, muted: bool) {
    send(fx, Command::SetMuted { node, muted });
}

fn lists(store: &Store) -> Option<&Lists> {
    store.devices.audio.lists.as_ref()
}

/// A stop's row: `output-device:<id>`, `input-device:<id>`, `stream:<id>`.
fn by_key<'a>(l: &'a Lists, key: &str) -> Option<(&'static str, &'a Row)> {
    let (kind, id) = key.split_once(':')?;
    let id: u32 = id.parse().ok()?;
    let rows = match kind {
        "output-device" => &l.outputs,
        "input-device" => &l.inputs,
        "stream" => &l.streams,
        _ => return None,
    };
    let kind = match kind {
        "output-device" => "output",
        "input-device" => "input",
        _ => "stream",
    };
    rows.iter().find(|r| r.id == id).map(|r| (kind, r))
}

fn device_row(id_key: String, glyph: &str, r: &Row, selected: bool, s: &fs_theme::tokens::Space) -> El {
    w::panel_row_checked(s, glyph, &r.label, None, Vec::new(), selected).stop(id_key.clone()).on(id_key)
}

/// A label, a percent and a mute switch over a track: the input master
/// and every stream row.
fn mixer_row(s: &fs_theme::tokens::Space, glyph: Option<&str>, label: &str, volume: f64, muted: bool, key: &str, track: El) -> El {
    let values = vec![w::panel_value(format!("{}%", (volume * 100.0).round())), w::switch(!muted).on(format!("mute:{key}"))];
    w::panel_meter(s, glyph, label, values, Some(track))
}

impl Audio {
    fn make_default(fx: &Effect, input: bool, r: &Row) {
        let name = r.name.clone();
        send(fx, if input { Command::SetDefaultSource(Some(name)) } else { Command::SetDefaultSink(Some(name)) });
    }

    fn mute(fx: &Effect, stop: &str) {
        let a = &fx.store.devices.audio;
        let Some(l) = lists(fx.store) else { return };
        match stop {
            "output-slider" => {
                if let Some(id) = l.sink {
                    set_muted(fx, id, !a.muted);
                }
            }
            "input-slider" => {
                if let Some(r) = &l.source {
                    set_muted(fx, r.id, !r.muted);
                }
            }
            _ => {
                if let Some(("stream", r)) = by_key(l, stop) {
                    set_muted(fx, r.id, !r.muted);
                }
            }
        }
    }
}

impl Panel for Audio {
    fn id(&self) -> &'static str {
        "audio"
    }

    fn title(&self, _: &View) -> String {
        "Audio".into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Devices]
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let a = &v.store.devices.audio;
        let empty = Lists::default();
        let l = a.lists.as_ref().unwrap_or(&empty);
        let mut out = Vec::new();
        if let Some(volume) = a.volume {
            let volume = clamp_device(volume);
            let hero = w::hero(
                s,
                w::Hero {
                    glyph: if a.muted { "volume-x" } else { "volume-2" }.into(),
                    title: if a.name.is_empty() { "Output".into() } else { a.name.clone() },
                    meta: if a.muted { "Muted" } else { "Active" }.into(),
                    readout: format!("{}%", (volume * 100.0).round()),
                    trailing: Some(w::switch(!a.muted).on("mute:output-slider")),
                    rail: Some(volume),
                    rail_on: Some("volume:output-slider".into()),
                },
            )
            .stop("output-slider");
            out.push(hero);
        }
        if l.outputs.is_empty() && l.inputs.is_empty() {
            out.push(w::panel_empty(s, "No devices"));
        }
        // Device choice is the section's own rows, the current one checked:
        // one inset level, never a picker boxed inside the group.
        if l.outputs.len() > 1 {
            let rows = l.outputs.iter().map(|r| device_row(format!("output-device:{}", r.id), "volume-2", r, Some(r.id) == l.sink, s)).collect();
            out.push(w::panel_section_n(s, "Output", l.outputs.len(), rows, ""));
        }
        if !l.inputs.is_empty() {
            let (volume, muted) = l.source.as_ref().map_or((0.0, false), |r| (clamp_device(r.volume), r.muted));
            let track = w::slider(volume).on("volume:input-slider");
            let master = mixer_row(s, Some(if muted { "mic-off" } else { "mic" }), "Level", volume, muted, "input-slider", track);
            let source = l.source.as_ref().map(|r| r.id);
            let mut rows = vec![master.stop("input-slider")];
            if l.inputs.len() > 1 {
                rows.extend(l.inputs.iter().map(|r| device_row(format!("input-device:{}", r.id), "mic", r, Some(r.id) == source, s)));
            }
            out.push(w::panel_section_n(s, "Input", l.inputs.len(), rows, ""));
        }
        if !l.streams.is_empty() {
            let rows = l
                .streams
                .iter()
                .map(|r| {
                    let key = format!("stream:{}", r.id);
                    let track = w::slider(r.volume / 1.5).notch(1.0 / 1.5).on(format!("volume:{key}"));
                    let label = if r.label.is_empty() { "Stream" } else { &r.label };
                    mixer_row(s, None, label, r.volume, r.muted, &key, track).stop(key)
                })
                .collect();
            out.push(w::panel_section_n(s, "Apps", l.streams.len(), rows, ""));
        }
        w::column(0.0, out)
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        let store = fx.store;
        let Some(l) = lists(store).cloned() else { return };
        let a = &store.devices.audio;
        let (verb, key) = ev.on.split_once(':').unwrap_or((ev.on.as_str(), ""));
        match (verb, &ev.what) {
            ("mute", What::Toggle(_)) => Self::mute(fx, key),
            ("volume", What::Fraction(_) | What::Wheel(_)) => {
                let step = |cur: f64| match ev.what {
                    What::Wheel(d) => cur + d as f64 * STEP,
                    _ => cur,
                };
                match key {
                    "output-slider" => {
                        if let Some(id) = l.sink {
                            let to = if let What::Fraction(f) = ev.what { f } else { step(a.volume.unwrap_or(0.0)) };
                            set_volume(fx, id, clamp_device(to));
                        }
                    }
                    "input-slider" => {
                        if let Some(r) = &l.source {
                            let to = if let What::Fraction(f) = ev.what { f } else { step(r.volume) };
                            set_volume(fx, r.id, clamp_device(to));
                        }
                    }
                    _ => {
                        if let Some(("stream", r)) = by_key(&l, key) {
                            let to = if let What::Fraction(f) = ev.what { f * 1.5 } else { step(r.volume) };
                            set_volume(fx, r.id, clamp_stream(to));
                        }
                    }
                }
            }
            (_, What::Click) => self.activate(&ev.on.clone(), fx),
            _ => {}
        }
    }

    fn activate(&mut self, stop: &str, fx: &mut Effect) {
        let Some(l) = lists(fx.store).cloned() else {
            if stop == "output-slider" {
                Self::mute(fx, stop);
            }
            return;
        };
        match by_key(&l, stop) {
            Some(("output", r)) => Self::make_default(fx, false, r),
            Some(("input", r)) => Self::make_default(fx, true, r),
            _ => Self::mute(fx, stop),
        }
    }

    fn step(&mut self, stop: &str, direction: i32, fx: &mut Effect) {
        let Some(l) = lists(fx.store).cloned() else { return };
        let d = direction as f64 * STEP;
        match stop {
            "output-slider" => {
                if let (Some(id), Some(v)) = (l.sink, fx.store.devices.audio.volume) {
                    set_volume(fx, id, clamp_device(v + d));
                }
            }
            "input-slider" => {
                if let Some(r) = &l.source {
                    set_volume(fx, r.id, clamp_device(r.volume + d));
                }
            }
            _ => {
                if let Some(("stream", r)) = by_key(&l, stop) {
                    set_volume(fx, r.id, clamp_stream(r.volume + d));
                }
            }
        }
    }

    fn key(&mut self, stop: Option<&str>, text: &str, fx: &mut Effect) {
        if (text == "m" || text == "M")
            && let Some(stop) = stop
        {
            Self::mute(fx, stop);
        }
    }

    fn steps(&self) -> bool {
        true
    }
}
