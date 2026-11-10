//! Popout panels.
//!
//! A panel is one row of fs-chrome's panel table (its name, launcher row,
//! width and home) and one module implementing [`Panel`]: its title, the
//! store topics it reads, a body built from `ui` widgets, the header
//! actions it adds beside the close button, and what its keyboard stops
//! and actions do. [`build`] is where a module is registered against its
//! [`PanelId`]. [`host::Host`] owns everything a panel does not: the
//! card and its motion, the header, the scroll, the size morph, the
//! handoff, the keyboard cursor and its halo, focus, Escape and the click
//! outside.
//!
//! The cursor model: every element marked `.stop(key)` in
//! the body is one stop, in reading order; the first navigation key only
//! reveals the cursor, arrows walk the stops (or step the value on one
//! when [`Panel::nav`] says so), Enter or Space calls
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
pub mod quick;
pub mod settings;
pub mod settings_page;
pub mod systemupdate;
pub mod tailscale;
pub mod tray;
pub mod usage;
pub mod weather;
pub mod workspace_preview;

use fs_chrome::bar::layout::Entry;
use fs_chrome::bar::panels::{PanelId, Width};
use fs_theme::theme::Theme;
use fs_theme::tokens::Space;
use fs_types::Edge;

use crate::runtime::Runtime;
use quick::Origin;
use crate::services::wants::Source;
use crate::store::{Store, Topic};
use crate::ui::{El, Event, On};

/// What a panel reads while it describes itself.
pub struct View<'a> {
    pub store: &'a Store,
    pub theme: &'a Theme,
    /// The output's size, for a panel sized off it (the gallery).
    pub output: (f64, f64),
    /// The stop holding the keyboard cursor.
    pub cursor: Option<&'a On>,
    /// The stop of the row the pointer is over, whichever of its parts it is
    /// on; the keyboard cursor does not count.
    pub hovered: Option<On>,
    /// The frame being drawn, for a body that moves on its own clock.
    pub now: std::time::Instant,
    /// The card's `debug motionScale`, 1 at full speed.
    pub motion_scale: f64,
    /// The scale the card's surface draws at, what its pictures are made at.
    pub raster: f64,
}

/// A key reaching an inline text field.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// For the field's own [`crate::ui::field::Field::key`].
    Key(crate::ui::field::Key, crate::ui::field::Modifiers, Option<String>),
    /// The clipboard's text, read for an earlier Ctrl+V.
    Paste(String),
    Tab,
    Submit,
    Cancel,
}

/// The `summon` that opens Radio Atlas rather than a launcher route.
pub const ATLAS: &str = "@radio-atlas";

/// What a panel asks of the shell outside it. A panel pushes as many as it
/// needs; whatever holds the card runs them in order. A service call that
/// needs nothing of the shell is made by the panel itself, through the
/// service's own `send`.
#[derive(Debug)]
pub enum Out {
    /// Close the card.
    Close,
    /// Close, then open the launcher on this route ([`ATLAS`] for Radio
    /// Atlas).
    Summon(&'static str),
    /// Open this panel in the open one's place, then press Enter on this
    /// stop of its (a network's row, opening its passphrase step).
    Panel(PanelId, Option<On>),
    /// Back to the card that handed over to this one, opened again from
    /// where it was.
    Back(PanelId, Origin),
    /// A tray item's menu off this card, by id, centred on `anchor` along
    /// the line (the item's own button) or at the line's end.
    Menu(String, Option<f64>),
    /// Close, then run a bar cell's own click (an info tile whose cell
    /// opens no panel).
    Act(crate::surfaces::bar::cell::Action),
    /// A call only the shell can make, the card left open.
    Do(Verb),
    /// A change to the store, applied on the UI thread.
    Apply(Box<crate::store::Diff>),
    /// Desktop entry `.0` launched off a command line of its own (one of
    /// its actions'), the card left open.
    Launch(String, Vec<String>),
}

impl Out {
    pub fn apply(diff: impl Into<crate::store::Diff>) -> Self {
        Out::Apply(Box::new(diff.into()))
    }
}

/// The calls behind [`Out::Do`]. Whatever holds the card runs them
/// through the functions the matching IPC verb calls, so a refusal reads
/// the same from either.
#[derive(Clone, Debug, PartialEq)]
pub enum Verb {
    Screenshot,
    Lock,
    Logout,
    Overview,
    Caffeinate,
    /// HDR on the output quick settings shows it for.
    Hdr,
    /// One output's HDR.
    HdrOutput(String, bool),
    RecordStop,
}

/// What a panel may do from an input.
pub struct Effect<'a> {
    pub store: &'a Store,
    pub runtime: Option<&'a Runtime>,
    outs: Vec<Out>,
}

impl<'a> Effect<'a> {
    pub fn new(store: &'a Store, runtime: Option<&'a Runtime>) -> Self {
        Self { store, runtime, outs: Vec::new() }
    }

    pub fn push(&mut self, out: Out) {
        self.outs.push(out);
    }

    /// Runs `f` on the service thread.
    pub fn service(&self, f: impl FnOnce(&crate::runtime::Ctx) + Send + 'static) {
        if let Some(rt) = self.runtime {
            rt.service(f);
        }
    }

    /// What the panel asked for, in the order it asked.
    pub fn outs(self) -> Vec<Out> {
        self.outs
    }

    /// The end of an event whose host has nowhere to run an out: none may
    /// have been queued.
    pub fn none(self, event: &str) {
        debug_assert!(self.outs.is_empty(), "a panel queued outs on {event}, which drops them: {:?}", self.outs);
    }
}

/// How the arrows move between a panel's stops.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Walk {
    /// Reading order.
    #[default]
    List,
    /// A grid this many wide (Calendar's month): Up and Down move a row.
    Grid(usize),
    /// The nearest stop on screen in the arrow's direction: a grid whose
    /// rows hold different counts.
    Spatial,
}

/// What the arrows do on a stop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Nav {
    pub walk: Walk,
    /// Left and Right step the stop's value through [`Panel::step`]
    /// rather than walking.
    pub steps: bool,
}

impl Nav {
    /// Reading order, Left and Right stepping every value.
    pub const STEPS: Nav = Nav { walk: Walk::List, steps: true };
}

pub trait Panel: std::any::Any {
    fn title(&self, v: &View) -> String;
    /// The store slices whose change redraws this panel.
    fn reads(&self) -> &'static [Topic];
    /// What runs only while the card shows, asked as it opens and again
    /// whenever it lays out; whatever holds the card takes them.
    fn wants(&self, _store: &Store) -> Vec<Source> {
        Vec::new()
    }
    /// A width of its own, past the table's class: one that moves (media's
    /// split, the gallery's output) or a card the table has no row for.
    fn width(&self, _v: &View) -> Option<f64> {
        None
    }
    /// Whether the card is its body's own width (a strip of chips) rather
    /// than `width`; see [`card_width`]. Only the body's root is made to
    /// hug, so everything under it has to hug or carry a fixed width.
    fn fits(&self, _v: &View) -> bool {
        false
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
    fn cursor_start(&self) -> Option<On> {
        None
    }
    /// The stop the panel moved the cursor to on its own (a selection
    /// carrying it to another cell), once.
    fn take_cursor(&mut self) -> Option<On> {
        None
    }
    /// Whether the header carries the host's close button.
    fn closable(&self) -> bool {
        true
    }

    /// When the body next reads differently with nothing published (a
    /// playing track's elapsed second).
    fn wake(&self, _v: &View) -> Option<std::time::Instant> {
        None
    }
    /// Escape, before it closes the panel: true when the panel took it (an
    /// open menu shutting).
    fn escape(&mut self) -> bool {
        false
    }
    /// What the panel holds that its target's `status` reports
    /// (`quicksettings status`, `network status`); null for none.
    fn status(&self) -> serde_json::Value {
        serde_json::Value::Null
    }
    /// A text field holds the keyboard: every key goes to [`Panel::edit`].
    fn editing(&self) -> bool {
        false
    }
    fn edit(&mut self, _e: Edit, _fx: &mut Effect) {}
    /// An element's `on` action fired.
    fn event(&mut self, _ev: &Event, _fx: &mut Effect) {}
    /// Enter or Space on a stop.
    fn activate(&mut self, _stop: &On, _fx: &mut Effect) {}
    /// Left or Right on a stop whose [`Nav`] steps.
    fn step(&mut self, _stop: &On, _direction: i32, _fx: &mut Effect) {}
    /// Tab or Shift+Tab (`direction` -1) from the stop under the cursor:
    /// the stop to land on when the panel walks sections,
    /// or none to only reveal the cursor.
    fn tab(&mut self, _stop: Option<&On>, _direction: i32) -> Option<On> {
        None
    }
    /// After every key: the stop now holding the cursor.
    fn reached(&mut self, _stop: &On, _fx: &mut Effect) {}
    /// The stop under the pointer changed, `None` once it is over none.
    fn pointed(&mut self, _stop: Option<&On>) {}
    /// Backspace outside a text field: a page going back up a level.
    fn back(&mut self, _fx: &mut Effect) {}
    /// `x` on a stop.
    fn delete(&mut self, _stop: &On, _fx: &mut Effect) {}
    /// A printable key no binding took.
    fn key(&mut self, _stop: Option<&On>, _text: &str, _fx: &mut Effect) {}
    /// What the arrows do on `stop`, the stop under the cursor if any.
    fn nav(&self, _stop: Option<&On>) -> Nav {
        Nav::default()
    }
    /// The body is moving its own height (an in-place menu opening): the
    /// card takes each frame's height outright rather than morphing after
    /// it.
    fn follows(&self, _v: &View) -> bool {
        false
    }
}

pub fn width_of(class: Width, s: &Space) -> f64 {
    match class {
        Width::Narrow => s.popup_width_narrow,
        Width::Default => s.popup_width_default,
        Width::Wide => s.popup_width_wide,
        Width::Menu => s.popup_width_menu,
        Width::Panel => s.panel_width,
        Width::PanelWide => s.panel_width_wide,
    }
}

/// The card's width wherever it is drawn, `class` its row's in the table.
/// A card that fits its content is the body's natural width plus the rows'
/// inset on either side, the same inset its body sits at, so the room
/// after the last chip is the room before the first.
pub fn card_width(module: &dyn Panel, class: Width, v: &View, kit: &mut crate::paint::Kit) -> f64 {
    if !module.fits(v) {
        return module.width(v).unwrap_or_else(|| width_of(class, &v.theme.space));
    }
    let inset = v.theme.space.panel_row_inset;
    let natural = crate::ui::measure(&module.body(v).hug(), v.output.0.max(1.0), v.theme, kit).0;
    natural + inset * 2.0
}

/// A panel card's frame round its body, one set of numbers wherever the
/// card is drawn (the panel host, a page the launcher holds;
/// specs/2026-10-30-panel-language.md §2, §3): the header row `panelGap`
/// under the card's edge, the hairline `panelGap` under the row, the body
/// `panelGap` past the hairline, and the room under the body.
#[derive(Clone, Copy)]
pub struct Frame {
    pub top: f64,
    pub head: f64,
    pub gap: f64,
    pub foot: f64,
}

impl Frame {
    /// The body ends its rows' own inset above the card's edge, the room
    /// they keep from the sides; [`Frame::closing`] adds what a body
    /// closing on the text column needs.
    pub fn new(theme: &Theme, header: bool) -> Self {
        let s = &theme.space;
        let (head, gap) = if header { (s.panel_row, s.panel_gap * 2.0 + theme.border_width) } else { (0.0, 0.0) };
        Frame { top: s.panel_gap, head, gap, foot: s.panel_row_inset }
    }

    /// The room under `body` its last thing keeps from the sides: a row's
    /// box ends `panelRowInset` up, a group, caption or slider on the text
    /// column `panelInset`, and a body scrolled under the edge is cut there.
    pub fn closing(mut self, theme: &Theme, body: &crate::ui::el::El) -> Self {
        self.foot += crate::ui::w::closing_room(&theme.space, body);
        self
    }

    /// The hairline's offset under the card's top edge.
    pub fn rule(&self, theme: &Theme) -> f64 {
        self.top + self.head + theme.space.panel_gap
    }

    /// The body's offset under the card's top edge.
    pub fn body(&self) -> f64 {
        self.top + self.head + self.gap
    }
}

/// A panel's title in its header row.
pub fn title(text: String) -> El {
    crate::ui::w::label(text).size(crate::ui::Type::Display).weight(crate::ui::Weight::Semibold).elide()
}

/// A card the `panel` verbs open: the table's, or a plugin's by its
/// interned key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Name {
    Panel(PanelId),
    Plugin(&'static str),
}

impl Name {
    pub fn as_str(self) -> &'static str {
        match self {
            Name::Panel(id) => id.as_str(),
            Name::Plugin(key) => key,
        }
    }

    pub fn panel(self) -> Option<PanelId> {
        match self {
            Name::Panel(id) => Some(id),
            Name::Plugin(_) => None,
        }
    }
}

impl From<PanelId> for Name {
    fn from(id: PanelId) -> Self {
        Name::Panel(id)
    }
}

/// What a panel is built with past its id: quick settings' info tiles,
/// the cells its opener governs on the bar that opened it, and that bar's
/// edge, which their cells read.
pub struct Args {
    pub tiles: Vec<Entry>,
    pub edge: Edge,
}

impl Default for Args {
    fn default() -> Self {
        Args { tiles: Vec::new(), edge: Edge::Top }
    }
}

/// A panel's module. Radio Atlas is a modal of its own, not a module.
pub fn build(id: PanelId, args: Args) -> Option<Box<dyn Panel>> {
    Some(match id {
        PanelId::Activity => Box::new(activity::Activity::default()),
        PanelId::AppMenu => Box::new(appmenu::AppMenu),
        PanelId::Audio => Box::new(audio::Audio),
        PanelId::Calendar => Box::new(calendar::Calendar::new(true)),
        PanelId::Iphone => Box::new(iphone::Iphone::default()),
        PanelId::Media => Box::new(media::Media::default()),
        PanelId::Weather => Box::new(weather::Weather),
        PanelId::Network => Box::new(network::Network::default()),
        PanelId::Bluetooth => Box::new(bluetooth::Bluetooth),
        PanelId::Dualsense => Box::new(dualsense::Dualsense),
        PanelId::Earbuds => Box::new(earbuds::Earbuds::default()),
        PanelId::Display => Box::new(display::Display),
        PanelId::Github => Box::new(github::Github),
        PanelId::Monitor => Box::new(monitor::Monitor),
        PanelId::Power => Box::new(power::Power::default()),
        PanelId::QuickSettings => Box::new(quick::QuickSettings::new(args.tiles, args.edge)),
        PanelId::Info => Box::new(info::Info),
        PanelId::Settings => Box::<settings::Settings>::default(),
        PanelId::SettingsAppearance | PanelId::SettingsBar | PanelId::SettingsDesktop | PanelId::SettingsSystem => {
            Box::new(settings_page::SettingsPage::new(id)?)
        }
        PanelId::SystemUpdate => Box::new(systemupdate::SystemUpdate),
        PanelId::Tailscale => Box::new(tailscale::Tailscale),
        PanelId::Usage => Box::new(usage::Usage),
        PanelId::TrayOverflow => Box::new(tray::Tray::default()),
        PanelId::Radio => return None,
    })
}
