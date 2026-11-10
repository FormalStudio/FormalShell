//! A hero for the default sink (its rail the master slider,
//! its mute a `Switch`), then Output, Input and Apps as one group each:
//! every sink with the default checked, the source's level over every
//! source, and the live playback streams. One inset level throughout
//! (specs/2026-10-30-panel-language.md), so no picker sits boxed in a group.
//!
//! One cursor walks every row: Left and Right step the slider under it by
//! 5%, `m` mutes, Enter switches the default on a device row and mutes on a
//! slider. A stream's slider runs to `fs_audio::STREAM_MAX` with a notch
//! at 1.0. The lists come from fs-audio's graph,
//! published only while the panel is open.

use fs_audio::{STREAM_MAX, Visual};

use super::{Effect, Nav, Panel, View};
use crate::services::devices::audio::{self, Cmd, Lists, Row, Target};
use crate::services::wants::Source;
use crate::store::{Store, Topic};
use crate::ui::{El, Event, On, What, w};

/// One row of the panel, its cursor stop: the two masters, a device or a
/// stream by its node id.
#[derive(Clone, Copy, PartialEq)]
enum Line {
    Output,
    Input,
    OutputDevice(u32),
    InputDevice(u32),
    Stream(u32),
}

impl crate::ui::on::HitId for Line {}

impl std::fmt::Display for Line {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Line::Output => f.write_str("output-slider"),
            Line::Input => f.write_str("input-slider"),
            Line::OutputDevice(id) => write!(f, "output-device:{id}"),
            Line::InputDevice(id) => write!(f, "input-device:{id}"),
            Line::Stream(id) => write!(f, "stream:{id}"),
        }
    }
}

/// A press on a mute switch or a volume track, by the row it sits on.
enum Press {
    Mute(Line),
    Volume(Line),
}

impl crate::ui::on::HitId for Press {}

impl std::fmt::Display for Press {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Press::Mute(line) => write!(f, "mute:{line}"),
            Press::Volume(line) => write!(f, "volume:{line}"),
        }
    }
}

/// AudioModel's step.
const STEP: f64 = 0.05;

#[derive(Default)]
pub struct Audio;

fn set_volume(node: u32, v: f64) {
    audio::send(Cmd::Volume(Target::Node(node), Visual::from_f64(v)));
}

fn set_muted(node: u32, muted: bool) {
    audio::send(Cmd::Mute(node, muted));
}

fn lists(store: &Store) -> Option<&Lists> {
    store.audio.lists.as_ref()
}

/// A device or stream line's row in the lists.
fn row_of(l: &Lists, line: Line) -> Option<&Row> {
    let (rows, id) = match line {
        Line::OutputDevice(id) => (&l.outputs, id),
        Line::InputDevice(id) => (&l.inputs, id),
        Line::Stream(id) => (&l.streams, id),
        Line::Output | Line::Input => return None,
    };
    rows.iter().find(|r| r.id == id)
}

fn device_row(line: Line, glyph: &str, r: &Row, selected: bool, s: &fs_theme::tokens::Space) -> El {
    w::panel_row_checked(s, glyph, &r.label, None, Vec::new(), selected).stop(line).on(line)
}

/// A label, a percent and a mute switch over a track: the input master
/// and every stream row.
fn mixer_row(s: &fs_theme::tokens::Space, glyph: Option<&str>, label: &str, volume: f64, muted: bool, line: Line, track: El) -> El {
    let values = vec![w::panel_value(format!("{}%", (volume * 100.0).round())), w::switch(!muted).on(Press::Mute(line))];
    w::panel_meter(s, glyph, label, values, Some(track))
}

impl Audio {
    fn make_default(input: bool, r: &Row) {
        audio::send(Cmd::Default { source: input, name: r.name.clone() });
    }

    fn mute(fx: &Effect, line: Line) {
        let a = &fx.store.audio;
        let Some(l) = lists(fx.store) else { return };
        match line {
            Line::Output => {
                if let Some(id) = l.sink {
                    set_muted(id, !a.muted);
                }
            }
            Line::Input => {
                if let Some(r) = &l.source {
                    set_muted(r.id, !r.muted);
                }
            }
            Line::Stream(_) => {
                if let Some(r) = row_of(l, line) {
                    set_muted(r.id, !r.muted);
                }
            }
            Line::OutputDevice(_) | Line::InputDevice(_) => {}
        }
    }
}

impl Panel for Audio {
    fn title(&self, _: &View) -> String {
        "Audio".into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Audio]
    }

    fn wants(&self, _: &Store) -> Vec<Source> {
        vec![Source::AudioLists]
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let a = &v.store.audio;
        let empty = Lists::default();
        let l = a.lists.as_ref().unwrap_or(&empty);
        let mut out = Vec::new();
        if let Some(volume) = a.volume {
            let volume = volume.track().as_fraction();
            let hero = w::hero(
                s,
                w::Hero {
                    glyph: if a.muted { "volume-x" } else { "volume-2" }.into(),
                    title: if a.name.is_empty() { "Output".into() } else { a.name.clone() },
                    meta: if a.muted { "Muted" } else { "Active" }.into(),
                    readout: format!("{}%", (volume * 100.0).round()),
                    trailing: Some(w::switch(!a.muted).on(Press::Mute(Line::Output))),
                    rail: Some(volume),
                    rail_on: Some(Press::Volume(Line::Output).into()),
                },
            )
            .stop(Line::Output);
            out.push(hero);
        }
        if l.outputs.is_empty() && l.inputs.is_empty() {
            out.push(w::panel_empty(s, "No devices"));
        }
        // Device choice is the section's own rows, the current one checked:
        // one inset level, never a picker boxed inside the group.
        if l.outputs.len() > 1 {
            let rows = l.outputs.iter().map(|r| device_row(Line::OutputDevice(r.id), "volume-2", r, Some(r.id) == l.sink, s)).collect();
            out.push(w::panel_section_n(s, "Output", l.outputs.len(), rows, ""));
        }
        if !l.inputs.is_empty() {
            let (volume, muted) = l.source.as_ref().map_or((0.0, false), |r| (r.volume.track().as_fraction(), r.muted));
            let track = w::slider(volume).on(Press::Volume(Line::Input));
            let master = mixer_row(s, Some(if muted { "mic-off" } else { "mic" }), "Level", volume, muted, Line::Input, track);
            let source = l.source.as_ref().map(|r| r.id);
            let mut rows = vec![master.stop(Line::Input)];
            if l.inputs.len() > 1 {
                rows.extend(l.inputs.iter().map(|r| device_row(Line::InputDevice(r.id), "mic", r, Some(r.id) == source, s)));
            }
            out.push(w::panel_section_n(s, "Input", l.inputs.len(), rows, ""));
        }
        if !l.streams.is_empty() {
            let rows = l
                .streams
                .iter()
                .map(|r| {
                    let line = Line::Stream(r.id);
                    let volume = r.volume.as_f64();
                    let track = w::slider(volume / STREAM_MAX.as_f64()).notch(1.0 / STREAM_MAX.as_f64()).on(Press::Volume(line));
                    let label = if r.label.is_empty() { "Stream" } else { &r.label };
                    mixer_row(s, None, label, volume, r.muted, line, track).stop(line)
                })
                .collect();
            out.push(w::panel_section_n(s, "Apps", l.streams.len(), rows, ""));
        }
        w::column(0.0, out)
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        let store = fx.store;
        let Some(l) = lists(store).cloned() else { return };
        let a = &store.audio;
        match (ev.on.get::<Press>(), &ev.what) {
            (Some(&Press::Mute(line)), What::Toggle(_)) => Self::mute(fx, line),
            (Some(&Press::Volume(line)), What::Fraction(_) | What::Wheel(_)) => {
                let step = |cur: Visual| match ev.what {
                    What::Wheel(d) => cur.as_f64() + d as f64 * STEP,
                    _ => cur.as_f64(),
                };
                match line {
                    Line::Output => {
                        if let Some(id) = l.sink {
                            let to = if let What::Fraction(f) = ev.what { f } else { step(a.volume.unwrap_or_default()) };
                            set_volume(id, to);
                        }
                    }
                    Line::Input => {
                        if let Some(r) = &l.source {
                            let to = if let What::Fraction(f) = ev.what { f } else { step(r.volume) };
                            set_volume(r.id, to);
                        }
                    }
                    Line::Stream(_) => {
                        if let Some(r) = row_of(&l, line) {
                            let to = if let What::Fraction(f) = ev.what { f * STREAM_MAX.as_f64() } else { step(r.volume) };
                            set_volume(r.id, to);
                        }
                    }
                    Line::OutputDevice(_) | Line::InputDevice(_) => {}
                }
            }
            (_, What::Click) => self.activate(&ev.on.clone(), fx),
            _ => {}
        }
    }

    fn activate(&mut self, stop: &On, fx: &mut Effect) {
        let Some(&line) = stop.get::<Line>() else { return };
        let Some(l) = lists(fx.store).cloned() else {
            if line == Line::Output {
                Self::mute(fx, line);
            }
            return;
        };
        match (line, row_of(&l, line)) {
            (Line::OutputDevice(_), Some(r)) => Self::make_default(false, r),
            (Line::InputDevice(_), Some(r)) => Self::make_default(true, r),
            _ => Self::mute(fx, line),
        }
    }

    fn step(&mut self, stop: &On, direction: i32, fx: &mut Effect) {
        let Some(&line) = stop.get::<Line>() else { return };
        let Some(l) = lists(fx.store).cloned() else { return };
        let d = direction as f64 * STEP;
        match line {
            Line::Output => {
                if let (Some(id), Some(v)) = (l.sink, fx.store.audio.volume) {
                    set_volume(id, v.as_f64() + d);
                }
            }
            Line::Input => {
                if let Some(r) = &l.source {
                    set_volume(r.id, r.volume.as_f64() + d);
                }
            }
            Line::Stream(_) => {
                if let Some(r) = row_of(&l, line) {
                    set_volume(r.id, r.volume.as_f64() + d);
                }
            }
            Line::OutputDevice(_) | Line::InputDevice(_) => {}
        }
    }

    fn key(&mut self, stop: Option<&On>, text: &str, fx: &mut Effect) {
        if (text == "m" || text == "M")
            && let Some(&line) = stop.and_then(|s| s.get::<Line>())
        {
            Self::mute(fx, line);
        }
    }

    fn nav(&self, _: Option<&On>) -> Nav {
        Nav::STEPS
    }
}
