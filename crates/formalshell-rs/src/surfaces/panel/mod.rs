//! Popout panels.
//!
//! A panel is one module implementing [`Panel`]: its id, its title and
//! icon, the store topics it reads, a body built from `ui` widgets, the
//! header actions it adds beside the close button, and what its keyboard
//! stops and actions do. [`build`] is where a module is registered against
//! its `panel` name. [`host::Host`] owns everything a panel does not: the
//! card and its motion, the header, the scroll, the size morph, the
//! handoff, the keyboard cursor and its halo, focus, Escape and the click
//! outside.
//!
//! The cursor model: every element marked `.stop(key)` in
//! the body is one stop, in reading order; the first navigation key only
//! reveals the cursor, arrows walk the stops (or step the value on one
//! when [`Panel::steps`] says so), Enter or Space calls
//! [`Panel::activate`], `x` calls [`Panel::delete`], and any other
//! printable key reaches [`Panel::key`].

pub mod activity;
pub mod appmenu;
pub mod audio;
pub mod bluetooth;
pub mod calendar;
pub mod center;
pub mod display;
pub mod dualsense;
pub mod earbuds;
pub mod gallery;
pub mod github;
pub mod host;
pub mod info;
pub mod iphone;
pub mod media;
pub mod monitor;
pub mod network;
pub mod plugin;
pub mod power;
pub mod settings;
pub mod standin;
pub mod systemupdate;
pub mod tailscale;
pub mod usage;
pub mod weather;
pub mod workspace_preview;

use fs_theme::theme::Theme;

use crate::runtime::Runtime;
use crate::store::{Store, Topic};
use crate::ui::{El, Event};

/// What a panel reads while it describes itself.
pub struct View<'a> {
    pub store: &'a Store,
    pub theme: &'a Theme,
    /// The output's size, for a panel sized off it (the gallery).
    pub output: (f64, f64),
    /// The stop holding the keyboard cursor.
    pub cursor: Option<&'a str>,
    /// The stop of the row the pointer is over, whichever of its parts it is
    /// on; the keyboard cursor does not count.
    pub hovered: Option<String>,
}

/// A key reaching an inline text field.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// For the field's own [`fs_menu::field::Field::key`].
    Key(fs_menu::field::Key, fs_menu::nav::Modifiers, Option<String>),
    /// The clipboard's text, read for an earlier Ctrl+V.
    Paste(String),
    Tab,
    Submit,
    Cancel,
}

/// What a panel may do from an input.
/// The `summon` that opens Radio Atlas rather than a launcher route.
pub const ATLAS: &str = "@radio-atlas";

pub struct Effect<'a> {
    pub store: &'a Store,
    pub runtime: Option<&'a Runtime>,
    /// Set to close the panel.
    pub close: bool,
    /// A launcher route to open as the panel closes.
    pub summon: Option<&'static str>,
    /// Another panel to open in this one's place.
    pub panel: Option<&'static str>,
    /// An IPC target and function the shell calls on itself as the panel
    /// closes (`lock`, `lock`).
    pub call: Option<(&'static str, &'static str)>,
}

impl Effect<'_> {
    /// Runs `f` on the service thread.
    pub fn service(&self, f: impl FnOnce(&crate::runtime::Ctx) + Send + 'static) {
        if let Some(rt) = self.runtime {
            rt.service(f);
        }
    }
}

pub trait Panel {
    fn id(&self) -> &'static str;
    fn title(&self, v: &View) -> String;
    fn icon(&self, v: &View) -> String;
    /// The store slices whose change redraws this panel.
    fn reads(&self) -> &'static [Topic];
    /// The card's width (`panelWidth`): `popupWidthDefault` unless said.
    fn width(&self, v: &View) -> f64 {
        v.theme.space.popup_width_default
    }
    /// Whether the card takes the keyboard. One that does not (a preview a
    /// pointer opened) takes input over its own rect alone, so whatever it
    /// hangs off keeps the pointer around it.
    fn takes_keyboard(&self) -> bool {
        true
    }
    /// The header row; off for a strip rather than a sheet.
    fn header(&self) -> bool {
        true
    }
    /// The title in the display role with no glyph, over a hairline inset
    /// to the content (Pond's Wi-Fi and Information cards).
    fn display_title(&self) -> bool {
        false
    }
    /// The header's own controls, ahead of the close button.
    fn actions(&self, _v: &View) -> Vec<El> {
        Vec::new()
    }
    fn body(&self, v: &View) -> El;
    /// A fresh open: where the cursor starts, what resets.
    fn opened(&mut self) {}
    /// The card starts closing, or is handed over to another panel: what
    /// the panel holds open while it shows (a process it wants running)
    /// lets go now, not when the exit lands.
    fn closed(&mut self) {}
    /// Right after the card is built, with the store and the services in
    /// reach: what a panel asks for once as it opens (a probe, a refresh).
    fn start(&mut self, _fx: &mut Effect) {}
    /// The stop the cursor starts on, before any key moves it.
    fn cursor_start(&self) -> Option<String> {
        None
    }
    /// The stop the panel moved the cursor to on its own (a selection
    /// carrying it to another cell), once.
    fn take_cursor(&mut self) -> Option<String> {
        None
    }
    /// When the body next reads differently with nothing published (a
    /// playing track's elapsed second).
    /// Whether the header carries the host's close button.
    fn closable(&self) -> bool {
        true
    }

    fn wake(&self, _v: &View) -> Option<std::time::Instant> {
        None
    }
    /// Escape, before it closes the panel: true when the panel took it (an
    /// open menu shutting).
    fn escape(&mut self) -> bool {
        false
    }
    /// An IPC verb aimed at this panel (`calendar select`); `None` when the
    /// panel answers none.
    fn call(&mut self, _verb: &str, _arg: &str) -> Option<String> {
        None
    }
    /// A text field holds the keyboard: every key goes to [`Panel::edit`].
    fn editing(&self) -> bool {
        false
    }
    fn edit(&mut self, _e: Edit, _fx: &mut Effect) {}
    /// An element's `on` action fired.
    fn event(&mut self, _ev: &Event, _fx: &mut Effect) {}
    /// Enter or Space on a stop.
    fn activate(&mut self, _stop: &str, _fx: &mut Effect) {}
    /// Left or Right on a stop that carries a value, when `steps` is on.
    fn step(&mut self, _stop: &str, _direction: i32, _fx: &mut Effect) {}
    /// Tab or Shift+Tab (`direction` -1) from the stop under the cursor:
    /// the stop to land on when the panel walks sections,
    /// or none to only reveal the cursor.
    fn tab(&mut self, _stop: Option<&str>, _direction: i32) -> Option<String> {
        None
    }
    /// After every key: the stop now holding the cursor.
    fn reached(&mut self, _stop: &str, _fx: &mut Effect) {}
    /// The stop under the pointer changed, `None` once it is over none.
    fn pointed(&mut self, _stop: Option<&str>) {}
    /// `x` on a stop.
    fn delete(&mut self, _stop: &str, _fx: &mut Effect) {}
    /// A printable key no binding took.
    fn key(&mut self, _stop: Option<&str>, _text: &str, _fx: &mut Effect) {}
    /// Left and Right step the value under the cursor rather than walking.
    fn steps(&self) -> bool {
        false
    }
    /// Above 1, the stops are a grid this many wide (Calendar's month).
    fn columns(&self) -> usize {
        1
    }
}

/// The panels `panel` can open, each one's
/// module.
pub const PANELS: [&str; 22] = [
    "activity",
    "appmenu",
    "audio",
    "calendar",
    "network",
    "bluetooth",
    "earbuds",
    "iphone",
    "dualsense",
    "power",
    "weather",
    "media",
    "github",
    "usage",
    "tailscale",
    "systemupdate",
    "display",
    "monitor",
    "trayoverflow",
    "radio",
    "info",
    "settings",
];

pub fn known(name: &str) -> Option<&'static str> {
    PANELS.iter().find(|n| **n == name).copied()
}

pub fn build(name: &str) -> Option<Box<dyn Panel>> {
    if name.starts_with(plugin::PREFIX) {
        return Some(Box::new(plugin::PluginPanel::new(plugin::intern(name))));
    }
    let id = known(name)?;
    Some(match id {
        "activity" => Box::new(activity::Activity::default()),
        "appmenu" => Box::new(appmenu::AppMenu),
        "audio" => Box::new(audio::Audio::default()),
        "calendar" => Box::new(calendar::Calendar::new(true)),
        "iphone" => Box::new(iphone::Iphone::default()),
        "media" => Box::new(media::Media::default()),
        "weather" => Box::new(weather::Weather::default()),
        "network" => Box::new(network::Network::default()),
        "bluetooth" => Box::new(bluetooth::Bluetooth),
        "dualsense" => Box::new(dualsense::Dualsense),
        "earbuds" => Box::new(earbuds::Earbuds::default()),
        "display" => Box::new(display::Display::new()),
        "github" => Box::new(github::Github::new()),
        "monitor" => Box::new(monitor::Monitor::new()),
        "power" => Box::new(power::Power::new()),
        "info" => Box::new(info::Info::new()),
        "settings" => Box::<settings::Settings>::default(),
        "systemupdate" => Box::new(systemupdate::SystemUpdate::new()),
        "tailscale" => Box::new(tailscale::Tailscale::new()),
        "usage" => Box::new(usage::Usage::new()),
        _ => Box::new(standin::StandIn::new(id)),
    })
}

