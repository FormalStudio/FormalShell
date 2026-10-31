//! The popout behind the iPhone cell. Honest states first
//! (no bridge on PATH, a bridge whose daemon is not running, a daemon with
//! no phone bonded, which offers Pair with the code the phone shows), then
//! the phone itself, the Focus line worded as an inference (ANCS carries no
//! Focus state, only the `silent` flag a Focus intercept sets), and the
//! phone's own now-playing with its three transport buttons. The phone's
//! notifications live in the notification centre with the desktop's.
//!
//! The transport group and the Pair button are stops of their own; Left and
//! Right step the group's option.

use std::cell::Cell;


use super::{Effect, Panel, View};
use crate::services::ams;
use crate::services::info::iphone::{self as service, Cmd};
use crate::services::wants::{Source, Want};
use crate::store::{Store, Topic};
use crate::ui::{El, Event, Ink, Type, Variant, Weight, What, w};
use crate::ui::el::Opt;

const TRANSPORT: [(&str, &str); 3] = [("skip-back", "prev"), ("", "toggle"), ("skip-forward", "next")];

pub struct Iphone {
    _want: Want,
    /// The transport option the cursor sits on.
    np_index: usize,
    /// Whether the cursor is on the transport group, read off the last
    /// body: Left and Right step an option there and walk elsewhere.
    np_on: Cell<bool>,
}

impl Default for Iphone {
    fn default() -> Self {
        Self { _want: Want::new(Source::Iphone), np_index: 1, np_on: Cell::new(false) }
    }
}

/// The service's error, else the Apple Media Service's own (the two share
/// one line, as IphoneService.lastError does).
pub fn last_error(store: &Store) -> String {
    let s = &store.info.iphone;
    if s.last_error.is_empty() { store.media.ams.error.clone() } else { s.last_error.clone() }
}

fn press(index: usize) {
    if let Some((_, verb)) = TRANSPORT.get(index) {
        ams::command(verb);
    }
}

impl Panel for Iphone {
    fn id(&self) -> &'static str {
        "iphone"
    }

    fn title(&self, _: &View) -> String {
        "iPhone".into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Info, Topic::Media, Topic::Clock]
    }

    fn opened(&mut self) {
        service::command(Cmd::MarkRead);
    }

    fn steps(&self) -> bool {
        self.np_on.get()
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let st = &v.store.info.iphone;
        let ams = &v.store.media.ams;
        self.np_on.set(v.cursor == Some("np"));
        let not_installed = !st.installed;
        let daemon_down = st.installed && !st.observer;
        let no_phone = st.observer && !st.connected;
        let connected = st.connected;
        let pairing = st.advertising || !st.pairing_code.is_empty();
        let stale_bond = no_phone && st.bonded;
        let now_playing = connected && ams.available && !ams.title.is_empty();
        let name = if st.device_name.is_empty() { "iPhone".to_owned() } else { st.device_name.clone() };
        let error = last_error(v.store);

        let mut parts: Vec<El> = Vec::new();
        if not_installed {
            parts.push(w::panel_empty(s, "No bridge on PATH"));
        }
        if daemon_down {
            parts.push(w::column(
                s.xxs,
                vec![
                    w::panel_empty(s, "Bridge not running"),
                    w::row(0.0, vec![w::spacer(), w::caption("Start the ancs4linux observer service"), w::spacer()]).fill(),
                ],
            ));
        }
        if !error.is_empty() && !not_installed && !daemon_down {
            parts.push(w::panel_text(s, w::para(error, Type::BodySmall, Weight::Normal, Ink::Destructive, 4)));
        }
        if connected && !pairing {
            let pct = st.battery.map(|b| format!("{}%", (b * 100.0 + 0.5).floor()));
            parts.push(w::hero(
                s,
                w::Hero {
                    glyph: "smartphone".into(),
                    title: name.clone(),
                    meta: "Connected".into(),
                    readout: pct.unwrap_or_default(),
                    trailing: None,
                    rail: st.battery,
                    rail_on: None,
                },
            ));
        }
        if no_phone || pairing {
            let title = if connected && !st.device_name.is_empty() {
                format!("Pairing with {}", st.device_name)
            } else if stale_bond {
                format!("{name} not connected")
            } else {
                "No phone paired".into()
            };
            let meta = if !st.pairing_code.is_empty() {
                "Check this matches the code on your iPhone"
            } else if pairing {
                "Advertising for pairing"
            } else if stale_bond && st.linked {
                "Waiting for the phone to reconnect"
            } else if stale_bond {
                "Paired here, not on the phone? Pair again"
            } else {
                "Not paired"
            };
            parts.push(w::hero_with(
                s,
                w::Hero { glyph: "smartphone".into(), title, meta: meta.into(), readout: st.pairing_code.clone(), trailing: None, rail: None, rail_on: None },
                false,
                Type::DisplayLarge,
                None,
            ));
        }
        if connected && st.in_focus {
            parts.push(w::panel_text(s, w::row(
                s.panel_gap,
                vec![
                    w::icon("moon").ink(Ink::Muted),
                    w::para("Focus likely on, so notifications are held back", Type::BodySmall, Weight::Normal, Ink::Muted, 4),
                ],
            )));
        }
        if connected {
            let mut np = vec![w::panel_heading(s, "Now playing")];
            if !now_playing {
                np.push(w::panel_empty(s, "Not available yet"));
            } else {
                let mut lines = vec![w::panel_text(s, w::label(&ams.title).elide())];
                if !ams.artist.is_empty() {
                    lines.push(w::panel_text(s, w::value(&ams.artist).size(Type::BodySmall).ink(Ink::Muted).elide()));
                }
                let toggle = if ams.playback == "playing" { "pause" } else { "play" };
                let options = TRANSPORT
                    .iter()
                    .map(|(glyph, verb)| Opt::new("").icon(if *verb == "toggle" { toggle } else { glyph }))
                    .collect();
                let group = w::group(options, 0, false).ring(self.np_index);
                lines.push(w::panel_text(s, group.hug().stop("np").on("np")));
                np.push(w::column(s.xxs, lines));
            }
            parts.push(w::column(s.panel_gap, np));
        }
        if no_phone {
            let label = if pairing {
                "Pairing"
            } else if stale_bond {
                "Pair again"
            } else {
                "Pair"
            };
            parts.push(w::panel_text(s, w::icon_text_button("smartphone", label).variant(Variant::Outline).enabled(!pairing).stop("pair").on("pair").pad_top(s.panel_section)));
        }
        w::column(0.0, parts)
    }

    fn event(&mut self, ev: &Event, _: &mut Effect) {
        match (ev.on.as_str(), &ev.what) {
            ("pair", _) => service::command(Cmd::Pair),
            ("np", What::Pick(i)) => {
                self.np_index = *i;
                press(*i);
            }
            _ => {}
        }
    }

    fn activate(&mut self, stop: &str, _: &mut Effect) {
        if stop == "np" {
            press(self.np_index);
        } else if stop == "pair" {
            service::command(Cmd::Pair);
        }
    }

    fn step(&mut self, stop: &str, direction: i32, _: &mut Effect) {
        if stop == "np" {
            self.np_index = (self.np_index as i32 + direction).clamp(0, TRANSPORT.len() as i32 - 1) as usize;
        }
    }
}
