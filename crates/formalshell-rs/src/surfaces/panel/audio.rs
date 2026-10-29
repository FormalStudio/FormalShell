//! A hero for the default sink (its rail the master slider,
//! its mute a `Switch`), Output listing the other sinks, an Input master row
//! over the default source with Input under it, and Apps for the live
//! playback streams. Two or three candidates render as a `ButtonGroup` pick
//! carrying the active one, four or more as rows without it (M48 D1).
//!
//! One cursor walks every row: Left and Right step the slider under it by
//! 5% or walk a pick's ring, `m` mutes, Enter switches the default on a
//! device row or pick and mutes on a slider. Masters clamp to 0..1, streams
//! to 0..1.5 with a notch at 1.0. The lists come from fs-audio's graph,
//! published only while the panel is open.

use fs_audio::Command;
use fs_system::audio::{clamp_device, clamp_stream};

use super::{Effect, Panel, View};
use crate::services::devices::audio::{self, Lists, Row};
use crate::store::{Store, Topic};
use crate::ui::el::Opt;
use crate::ui::{El, Event, Ink, What, w};

/// AudioModel's step.
const STEP: f64 = 0.05;

#[derive(Default)]
pub struct Audio {
    /// Each pick's ring, once a key or the pointer has moved it.
    output_ring: Option<usize>,
    input_ring: Option<usize>,
}

fn send(fx: &Effect, c: Command) {
    fx.service(move |_| audio::command(c));
}

fn set_volume(fx: &Effect, node: u32, v: f64) {
    send(fx, Command::SetVolume { node, volume: v as f32 });
}

fn set_muted(fx: &Effect, node: u32, muted: bool) {
    send(fx, Command::SetMuted { node, muted });
}

/// Two or three candidates read as one pick.
fn is_pick(n: usize) -> bool {
    (2..=3).contains(&n)
}

fn lists(store: &Store) -> Option<&Lists> {
    store.devices.audio.lists.as_ref()
}

fn selected(rows: &[Row], id: Option<u32>) -> usize {
    id.and_then(|id| rows.iter().position(|r| r.id == id)).unwrap_or(0)
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
    let mut parts = vec![w::icon(glyph), w::label(&r.label).elide()];
    if selected {
        parts.push(w::icon("check").ink(Ink::Primary));
    }
    w::cell(w::row(s.icon_gap, parts).fill()).ghost().interactive().selected(selected).stop(id_key.clone()).on(id_key)
}

/// A label, a percent and a mute switch over a track: the input master
/// and every stream row.
fn mixer_row(s: &fs_theme::tokens::Space, glyph: Option<&str>, label: &str, volume: f64, muted: bool, key: &str, track: El) -> El {
    let mut top = Vec::new();
    if let Some(g) = glyph {
        top.push(w::icon(g));
    }
    top.push(w::label(label).elide());
    top.push(w::value(format!("{}%", (volume * 100.0).round())).ink(if muted { Ink::Muted } else { Ink::Dim }));
    top.push(w::switch(!muted).on(format!("mute:{key}")));
    w::column(s.xs, vec![w::row(s.icon_gap, top).fill(), track])
}

impl Audio {
    fn ring(&self, input: bool, l: &Lists) -> usize {
        let (ring, rows, id) = if input {
            (self.input_ring, &l.inputs, l.source.as_ref().map(|r| r.id))
        } else {
            (self.output_ring, &l.outputs, l.sink)
        };
        ring.filter(|i| *i < rows.len()).unwrap_or_else(|| selected(rows, id))
    }

    fn pick(&self, l: &Lists, input: bool) -> El {
        let (rows, glyph, id, key) = if input {
            (&l.inputs, "mic", l.source.as_ref().map(|r| r.id), "input-pick")
        } else {
            (&l.outputs, "volume-2", l.sink, "output-pick")
        };
        let opts = rows.iter().map(|r| Opt::new(r.label.clone()).icon(glyph)).collect();
        w::group(opts, selected(rows, id), true).ring(self.ring(input, l)).stop(key).on(key)
    }

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

    fn icon(&self, _: &View) -> String {
        "volume-2".into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Devices]
    }

    fn opened(&mut self) {
        self.output_ring = None;
        self.input_ring = None;
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
            out.push(w::section_label(s, "No devices", None, true));
        }
        let output_pick = is_pick(l.outputs.len());
        let output_rows: Vec<El> = if output_pick {
            vec![self.pick(l, false)]
        } else {
            l.outputs
                .iter()
                .filter(|r| Some(r.id) != l.sink)
                .map(|r| device_row(format!("output-device:{}", r.id), "volume-2", r, false, s))
                .collect()
        };
        if !output_rows.is_empty() {
            let n = if output_pick { l.outputs.len() } else { output_rows.len() };
            out.push(w::section(s, "Output", Some(n), output_rows));
        }
        if !l.inputs.is_empty() {
            let (volume, muted) = l.source.as_ref().map_or((0.0, false), |r| (clamp_device(r.volume), r.muted));
            let track = w::slider(volume).on("volume:input-slider");
            let master = mixer_row(s, Some(if muted { "mic-off" } else { "mic" }), "Input", volume, muted, "input-slider", track);
            out.push(w::cell(master).interactive().stop("input-slider"));
            let input_pick = is_pick(l.inputs.len());
            let source = l.source.as_ref().map(|r| r.id);
            let rows: Vec<El> = if input_pick {
                vec![self.pick(l, true)]
            } else {
                l.inputs.iter().map(|r| device_row(format!("input-device:{}", r.id), "mic", r, Some(r.id) == source, s)).collect()
            };
            out.push(w::section(s, "Input", Some(l.inputs.len()), rows));
        }
        if !l.streams.is_empty() {
            let rows = l
                .streams
                .iter()
                .map(|r| {
                    let key = format!("stream:{}", r.id);
                    let track = w::slider(r.volume / 1.5).notch(1.0 / 1.5).on(format!("volume:{key}"));
                    let label = if r.label.is_empty() { "Stream" } else { &r.label };
                    w::cell(mixer_row(s, None, label, r.volume, r.muted, &key, track)).ghost().interactive().stop(key)
                })
                .collect();
            out.push(w::section(s, "Apps", Some(l.streams.len()), rows));
        }
        w::column(s.section_gap, out)
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
            ("output-pick", What::Pick(i)) => {
                self.output_ring = Some(*i);
                if let Some(r) = l.outputs.get(*i) {
                    Self::make_default(fx, false, r);
                }
            }
            ("input-pick", What::Pick(i)) => {
                self.input_ring = Some(*i);
                if let Some(r) = l.inputs.get(*i) {
                    Self::make_default(fx, true, r);
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
        match stop {
            "output-pick" => {
                if let Some(r) = l.outputs.get(self.ring(false, &l)) {
                    Self::make_default(fx, false, r);
                }
            }
            "input-pick" => {
                if let Some(r) = l.inputs.get(self.ring(true, &l)) {
                    Self::make_default(fx, true, r);
                }
            }
            _ => match by_key(&l, stop) {
                Some(("output", r)) => Self::make_default(fx, false, r),
                Some(("input", r)) => Self::make_default(fx, true, r),
                _ => Self::mute(fx, stop),
            },
        }
    }

    fn step(&mut self, stop: &str, direction: i32, fx: &mut Effect) {
        let Some(l) = lists(fx.store).cloned() else { return };
        let d = direction as f64 * STEP;
        match stop {
            "output-pick" => self.output_ring = Some((self.ring(false, &l) as i32 + direction).clamp(0, (l.outputs.len() as i32 - 1).max(0)) as usize),
            "input-pick" => self.input_ring = Some((self.ring(true, &l) as i32 + direction).clamp(0, (l.inputs.len() as i32 - 1).max(0)) as usize),
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
