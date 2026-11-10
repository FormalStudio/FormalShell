//! The launcher. One keyboard-exclusive overlay window over the
//! output, a card budding off the top line (a `Card`) to
//! rest at 30% of the output's height, three bands split by full-bleed
//! rules: the header (the mode token and the search field), the body (the app
//! grid, the row list or the emoji grid, sections and the empty state) and
//! the footer (where you are, what Enter does). The scrim behind it is two
//! single-pixel surfaces the App owns.
//!
//! The model ([`Model`]) lives as long as the shell: the level and the
//! select request outlive a close. The window
//! ([`Shown`]) exists from an open until its exit has run.
//!
//! Rows come off the store's warm index (`services::menu`); a keystroke
//! re-ranks a few hundred nodes and builds only the rows inside the
//! viewport.

use std::time::Instant;

use fs_chrome::bar::panels::PanelId;
use fs_menu::actions::{Action, IpcCall, Run, SubjectId, TargetKind};
use fs_menu::model::{self, Mode, SectionCtx};
use fs_menu::nav::{self, Dir};
use fs_menu::intent::Intent;
use fs_menu::node::{ActionEntry, Kind as NodeKind, Node};
use fs_menu::toggles::{self, Snapshot};
use crate::ui::confirm::Armed;
use fs_types::{Edge, IRect, WindowId};
use fs_theme::style::Role;
use fs_theme::theme::Theme;
use serde_json::{Value, json};
use vello_cpu::kurbo::Rect;

use crate::motion::{Animated, Kind as Clock};
use crate::scene::NodeId;
use crate::scroll::{Touchpad, Travel};
use crate::services::menu::{self as index, Ask};
use crate::services::{clipboard, plugins, state};
use crate::store::Store;
use crate::paint::{Kit, Painter};
use crate::surfaces::modal::Modal;
use crate::ui::field;
use crate::ui::{self, El, Ink, Size, Type, Ui, Weight, w};

pub mod keys;
mod mirror;
mod monitor;
mod picker;
mod preview;
mod rows;
mod settings;
mod view;
pub use mirror::*;
pub use monitor::*;
pub use picker::*;
pub use preview::*;
pub use rows::*;
pub use settings::*;
pub use view::*;
use keys::{KeyAction, KeyCtx, KeyMode};

/// Which view draws the level (`menu status`'s `view`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Rows,
    Emoji,
    AppGrid,
    Picker,
    /// The metric tiles over the process table.
    Monitor,
    /// One camera's feed.
    Mirror,
    /// A panel the launcher holds, centred: Settings and its pages, and
    /// Information, Pond's way.
    Panel,
}

impl View {
    fn name(self) -> &'static str {
        match self {
            View::Rows => "rows",
            View::Emoji => "emoji",
            View::AppGrid => "appGrid",
            View::Picker => "picker",
            View::Monitor => "monitor",
            View::Mirror => "mirror",
            View::Panel => "panel",
        }
    }
}

/// One drawn row or cell's place in the body, in body coordinates.
#[derive(Clone, Copy, Debug, Default)]
struct Slot {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    /// The heading band above a row, part of its height.
    band: f64,
}

/// What the body lays out, recomputed on every resolve.
#[derive(Default)]
struct Layout {
    slots: Vec<Slot>,
    /// The headings: text, the band's top and its x, in body coordinates,
    /// and the words at the band's far end.
    headings: Vec<(String, f64, f64, String)>,
    /// A section's empty state under its heading: its top, the line and
    /// the caption under it.
    notes: Vec<(f64, String, String)>,
    content_h: f64,
    columns: usize,
    cell_w: f64,
    cell_h: f64,
    /// Laid out at half the card: the selection moving onto or off a
    /// subject with a preview lays it out again.
    halved: bool,
    /// The body width it was laid out in, which an output narrower than
    /// the card changes.
    body_w: f64,
    /// The list's inset under its last row, kept clear of rows while it
    /// scrolls so the body always ends on a whole one.
    foot: f64,
    /// A held page's row ends (`ui::row_ends`), so a page taller than the
    /// card rests on a whole row.
    row_ends: Vec<f64>,
    /// A held page's room under its body past the frame's own
    /// (`panel::Frame::closing`).
    close: f64,
}

/// What input moves: the rows key, the field, the cursor, an armed row, and
/// the actions column's cursor and target step.
#[derive(PartialEq)]
struct ViewId {
    key: String,
    query: String,
    cursor: String,
    confirm: Option<Confirm>,
    column: Option<(usize, bool)>,
}

/// A screen-reading call parked behind a `wl_display.sync`. Each hold gets
/// an id, and only the callback or deadline carrying the current id gets
/// the call back: a late answer, or one from a session since reopened,
/// finds nothing.
#[derive(Default)]
pub struct Grab {
    next: u64,
    held: Option<(u64, IpcCall)>,
}

impl Grab {
    pub fn hold(&mut self, call: IpcCall) -> u64 {
        self.next += 1;
        self.held = Some((self.next, call));
        self.next
    }

    /// The call, when `id` is still the one held.
    pub fn take(&mut self, id: u64) -> Option<IpcCall> {
        match self.held {
            Some((held, _)) if held == id => self.held.take().map(|(_, call)| call),
            _ => None,
        }
    }

    pub fn cancel(&mut self) {
        self.held = None;
    }
}

pub struct Model {
    /// Behind a cell: rows and subjects are read through `&self` and ask
    /// as they are read.
    requests: std::cell::RefCell<Vec<Request>>,
    pub open: bool,
    pub mode: Mode,
    pub level: Option<String>,
    pub query: field::Field,
    select_prompt: String,
    select_options: Vec<String>,
    select_token: String,
    // What `resolve` committed.
    pub rows: Vec<Node>,
    sections: Vec<String>,
    pub view: View,
    pub app_count: usize,
    empty: Option<Node>,
    searching: bool,
    key: String,
    pub cursor: usize,
    pub cursor_id: String,
    placed: bool,
    want: String,
    pub from_keys: bool,
    travels: bool,
    /// The last scroll came off a wheel notch: it glides there without
    /// overshooting either end.
    wheeled: bool,
    confirm: Armed<Confirm>,
    pub scroll: f64,
    /// A touchpad's pull past either end, drawn over `scroll`, and the
    /// coast after a flick, which moves `scroll` frame by frame.
    pub touch: Touchpad,
    layout: Layout,
    body_h: f64,
    /// The rows changed since the window last drew.
    pub dirty: bool,
    /// A `pasteAfter` row ran: the chord goes out once the window is gone.
    pub paste: bool,
    /// A call that reads the screen, held until the window is gone so the
    /// closing card is not in the picture.
    pub after_unmap: Option<IpcCall>,
    /// The screen-reading call whose compositor sync is in flight.
    pub grab: Grab,
    /// Counts closes, so an answer asked for in one session can tell it
    /// arrived in another.
    pub sessions: u64,
    pub picker: Picker,
    /// The picker's sections while it is the level.
    picker_sections: Vec<PickerSection>,
    /// The output's width, which the card is cut to.
    pub output_w: f64,
    /// The monitor route's tile strip height, 0 off the route.
    monitor_strip_h: f64,
    /// The monitor route keeps both polls running while it is the level.
    monitor_wants: Option<(crate::services::wants::Want, crate::services::wants::Want)>,
    /// The pid an Enter armed TERM on; the next Enter on it sends it.
    monitor_armed: Armed<u64>,
    /// The camera the mirror shows, by node path.
    pub mirror_current: String,
    /// The monitor tiles' history lines, started when the route opens.
    monitor_history: MonitorHistory,
    /// What the query's abbreviations say, for the ranking and `abbrev`.
    lookup: fs_menu::abbrev::Lookup,
    /// The query as an abbreviation, when it is one.
    abbrev_key: Option<String>,
    /// The actions column beside the list, while it is open.
    column: Option<Column>,
    /// The subjects whose own children the list holds (an app's windows, a
    /// folder's entries), stepped into over `level`, outermost first.
    children: Vec<Children>,
    /// The captured frame of the window the preview pane shows.
    pub pane_frame: Option<crate::scene::Bitmap>,
    /// `Edit abbreviation`'s subject and the abbreviation it edits, while
    /// its Input step is up.
    editing: Option<(String, String)>,
    /// The launcher plugin and words last sent as a `query` event.
    plugin_query: Option<(String, String)>,
    /// The panel the settings and info routes hold.
    pub panel: Option<Embedded>,
}




/// The token an `Edit abbreviation` answer comes back under.
pub const ABBREV_EDIT: &str = "abbrev-edit";

/// What a subject stepped into with → lists.
#[derive(Clone, Debug)]
enum Into {
    /// An app's windows, by the app's subject id.
    Windows(String),
    /// A folder's entries, by its path.
    Folder(String),
    /// A Wi-Fi network's details, by its SSID.
    Network(String),
}

/// A subject stepped into with →, which the token names.
#[derive(Clone, Debug)]
struct Children {
    into: Into,
    label: String,
}

impl Children {
    fn key(&self) -> &str {
        match &self.into {
            Into::Windows(id) | Into::Folder(id) | Into::Network(id) => id,
        }
    }
}

/// The selected subject's actions beside the list (launcher spec §5). The
/// field filters it; the list's own query waits in `list_query`.
struct Column {
    node: Node,
    actions: Vec<Action>,
    list_query: String,
    /// An action asking for a target, and the targets, which the column
    /// lists instead of the actions.
    target: Option<(Action, Vec<Node>)>,
    /// What the filter leaves, as positions in the step's list.
    shown: Vec<usize>,
    cursor: usize,
    opened: Instant,
    /// Its own scroll, the list's model: the offset, a touchpad's stretch
    /// and coast over it, and how the last move asks to be drawn.
    scroll: f64,
    touch: Touchpad,
    wheeled: bool,
    travels: bool,
    /// A row's pitch and the rows' viewport, as the window last laid it out.
    row_h: f64,
    view_h: f64,
    /// The launcher plugins sent an `actions` event for this subject that
    /// have not answered, and when it went.
    asked: Vec<String>,
    asked_at: Instant,
    /// Where the first plugin answer joined the actions and when, for its
    /// fade.
    joined: Option<(usize, Instant)>,
}

/// How long a plugin has to answer `actions` before the column opens
/// without it.
const PLUGIN_ANSWER: std::time::Duration = std::time::Duration::from_millis(300);

impl Column {
    fn labels(&self) -> Vec<&str> {
        match &self.target {
            Some((_, targets)) => targets.iter().map(|t| t.label.as_str()).collect(),
            None => self.actions.iter().map(|a| a.label.as_str()).collect(),
        }
    }

    fn ids(&self) -> Vec<&str> {
        self.shown
            .iter()
            .filter_map(|&i| match &self.target {
                Some((_, targets)) => targets.get(i).map(|t| t.id.as_str()),
                None => self.actions.get(i).map(|a| a.id.as_str()),
            })
            .collect()
    }

    fn refilter(&mut self, query: &str) {
        self.shown = fs_menu::actions::filter(&self.labels(), query);
        self.cursor = 0;
        self.scroll = 0.0;
        self.touch.settle();
    }

    fn max_scroll(&self) -> f64 {
        (self.shown.len() as f64 * self.row_h - self.view_h).max(0.0)
    }

    /// The last whole row in view, when the list runs past it and the
    /// cursor is elsewhere: that row reads `n more` instead, `n` counting it
    /// and everything below.
    fn more(&self, scroll: f64) -> Option<(usize, usize)> {
        if self.row_h <= 0.0 {
            return None;
        }
        let last = ((scroll + self.view_h) / self.row_h).floor() as usize;
        let last = last.checked_sub(1)?;
        (last + 1 < self.shown.len() && last != self.cursor).then(|| (last, self.shown.len() - last))
    }

    /// The smallest scroll that shows the cursor's row.
    fn follow(&mut self) {
        let next = crate::scroll::follow(self.cursor as f64 * self.row_h, self.row_h, self.scroll, self.view_h, self.shown.len() as f64 * self.row_h);
        self.scroll = next;
        self.travels = true;
        self.wheeled = false;
        self.touch.settle();
    }
}

/// The drawn offset headed for `target`: a wheel's notch glides there on
/// `effectsFast`, a key's follow travels on `spatialFast`, a touchpad's
/// travel and a coast's frames land at once.
fn glide(a: &mut Animated, now: Instant, target: f64, wheeled: bool, travels: bool, theme: &Theme) {
    let gap = (a.target() - target).abs();
    if wheeled && gap > 0.5 {
        a.set_on(now, target, Clock::EffectsFast.ms(theme), Clock::EffectsFast.curve());
    } else if travels && gap > 0.5 {
        a.set_on(now, target, Clock::SpatialFast.ms(theme), Clock::SpatialFast.curve());
    } else if !wheeled && !travels && gap > 0.0 {
        // A coast's tail moves under half a pixel a frame.
        a.jump(target);
    }
}

/// The rows whose labels are shaped at both weights on a resolve: more
/// than a card at `LAUNCHER.row_height` shows.
const ROWS_SHAPED_AHEAD: usize = 24;




/// What the card's size travels on when it changes (`Shown::layout`).
#[derive(Clone, Debug, PartialEq, Eq)]
struct SizeKey {
    level: Option<String>,
    halved: bool,
    app_view: bool,
}







impl Default for Model {
    fn default() -> Self {
        Self {
            open: false,
            mode: Mode::Menu,
            level: None,
            query: field::Field::default(),
            select_prompt: String::new(),
            select_options: Vec::new(),
            select_token: String::new(),
            requests: Default::default(),
            rows: Vec::new(),
            sections: Vec::new(),
            view: View::AppGrid,
            app_count: 0,
            empty: None,
            searching: false,
            key: String::new(),
            cursor: 0,
            cursor_id: String::new(),
            paste: false,
            after_unmap: None,
            grab: Grab::default(),
            sessions: 0,
            picker: Picker::default(),
            picker_sections: Vec::new(),
            output_w: 0.0,
            monitor_strip_h: 0.0,
            monitor_wants: None,
            monitor_armed: Armed::default(),
            mirror_current: String::new(),
            panel: None,
            monitor_history: MonitorHistory::default(),
            lookup: Default::default(),
            abbrev_key: None,
            column: None,
            children: Vec::new(),
            pane_frame: None,
            editing: None,
            plugin_query: None,
            placed: false,
            want: String::new(),
            from_keys: true,
            travels: false,
            wheeled: false,
            confirm: Armed::default(),
            scroll: 0.0,
            touch: Touchpad::new(),
            layout: Layout::default(),
            body_h: 0.0,
            dirty: true,
        }
    }
}

/// What an activation asks of the shell outside the launcher.
pub enum Out {
    None,
    Close,
    /// An `@ipc:` call the App answers in process.
    Call(IpcCall),
    /// A verb of the launcher's the App answers, the subject of the row
    /// that ran it, and whether the launcher stays open after. The App
    /// closes the launcher, since the order differs by verb.
    Intent(Intent, SubjectId, bool),
    /// A command line the App spawns.
    Spawn(Vec<String>),
    /// A target action's pick, the launcher already closed.
    Target(Target),
}

/// What a target action's pick does.
pub enum Target {
    /// Desktop entry `id` launched on `argv`, the file last.
    Open { id: String, argv: Vec<String> },
    Send { peer: fs_devices::localsend::Peer, payload: crate::services::localsend::Payload },
    /// The windows onto workspace `idx` of `output`.
    Move { windows: Vec<WindowId>, idx: i64, output: String },
}

/// A service ask the model makes as it changes level, query or row. The
/// host sends them in order ([`Model::take_requests`]).
pub enum Request {
    Index(Ask),
    Mirror(crate::services::mirror::Cmd),
    Picker(crate::services::picker::Cmd),
    Packages(crate::services::packages::Cmd),
    /// Install mode's search for a query.
    PackageSearch(String),
    /// One event line for plugin `id`.
    Plugin(String, String),
    /// The launcher opened or closed, for the launcher plugins.
    Plugins(bool),
    PeerScan,
    Clipboard(clipboard::Cmd),
    /// A select or input step's answer, for menu-selection.txt and the
    /// in-process token owners.
    Selected(Selected),
    /// A fresh select or input step: menu-selection.txt holds no answer.
    SelectionCleared,
    /// `openImageSelect`'s answer, for picker-selection.txt.
    PickerSelected(Selected),
    /// A picture picked, and the mode it sets.
    Wallpaper(state::Choice, Option<fs_theme::theme::Mode>),
    /// A Style preview's mode, under `theme mode`'s rules.
    Mode(fs_theme::theme::Mode),
}

/// One answer of a select, input or image select step. A secret value is
/// held for the in-process owners alone.
pub struct Selected {
    pub token: String,
    pub value: Option<String>,
    pub cancelled: bool,
    pub secret: bool,
}

/// The row action a first press armed.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Confirm {
    node: String,
    action: String,
}

impl Confirm {
    fn of(node: &Node, action: &Action) -> Confirm {
        Confirm { node: node.id.clone(), action: action.id.clone() }
    }
}











impl Model {
    fn ask(&self, request: Request) {
        self.requests.borrow_mut().push(request);
    }

    /// Every ask since the last call, in the order made.
    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(self.requests.get_mut())
    }

    fn view_id(&self) -> ViewId {
        ViewId {
            key: self.key.clone(),
            query: self.query.text().to_owned(),
            cursor: self.cursor_id.clone(),
            confirm: self.confirm.get().cloned(),
            column: self.column.as_ref().map(|c| (c.cursor, c.target.is_some())),
        }
    }

    fn key_mode(&self) -> KeyMode {
        match self.mode {
            Mode::Menu => KeyMode::Menu,
            Mode::Select => KeyMode::Select,
            Mode::Input => KeyMode::Input,
        }
    }

    /// The clipboard routes, whose card keeps the split height.
    fn split(&self) -> bool {
        self.mode == Mode::Menu && matches!(self.level.as_deref(), Some("clipboard" | "share.history")) && self.children.is_empty()
    }

    /// The selected subject, when it has a preview (launcher spec §7).
    /// The actions column takes the side column over it.
    pub fn pane(&self) -> Option<&Node> {
        if self.mode != Mode::Menu || self.column.is_some() || self.view != View::Rows {
            return None;
        }
        self.rows.get(self.cursor).filter(|n| pane_kind(n).is_some())
    }

    /// The list takes the left half of the card: the selected subject's
    /// preview or the actions column holds the right.
    pub fn halved(&self) -> bool {
        self.column.is_some() || self.pane().is_some()
    }

    fn size_key(&self) -> SizeKey {
        SizeKey { level: self.level.clone(), halved: self.halved(), app_view: self.app_view() }
    }

    /// The query the list is ranked by: the field's, or while the column
    /// filters through the field, the one it held before.
    fn list_text(&self) -> &str {
        self.column.as_ref().map_or(self.query.text(), |c| c.list_query.as_str())
    }

    /// A held panel stands alone: no foot under it.
    pub fn footless(&self) -> bool {
        self.mode == Mode::Menu && self.view == View::Panel
    }

    /// A card whose height follows its list rather than the level's kind.
    /// A preview pane counts: the card takes the taller of the list and
    /// what the pane has to say. The clipboard keeps Pond's one height and
    /// the actions column the level's.
    fn content_tall(&self) -> bool {
        self.mode != Mode::Input && !self.app_view() && !self.split() && self.column.is_none() && matches!(self.view, View::Rows | View::AppGrid)
    }

    /// The routes that draw a whole view of their own.
    pub fn app_view(&self) -> bool {
        self.mode == Mode::Menu && matches!(self.level.as_deref(), Some(MONITOR_ROUTE | MIRROR_ROUTE | PICKER_ROUTE))
    }

    /// One card width per level kind, which the content is laid out at
    /// while the card itself morphs into it, cut to the output less
    /// `screenPadding` either side.
    pub fn card_width(&self, theme: &Theme) -> f64 {
        let kind = self.kind_width(theme);
        if self.output_w > 0.0 { kind.min(self.output_w - theme.space.screen_padding * 2.0) } else { kind }
    }

    fn kind_width(&self, theme: &Theme) -> f64 {
        if let Some(p) = self.panel.as_ref().filter(|p| self.on_panel() && p.width > 0.0) {
            return p.width;
        }
        if self.app_view() { theme.space.popup_width_menu_app } else { launcher_px(fs_theme::tokens::LAUNCHER.width) }
    }

    /// The list's share of a halved body, the side column taking the rest
    /// (Pond's clipboard, 348 of 638).
    fn list_share(&self, _theme: &Theme) -> f64 {
        0.55
    }

    /// The width inside the card's padding.
    pub fn content_width(&self, theme: &Theme) -> f64 {
        self.card_width(theme) - theme.space.panel_padding * 2.0
    }

    /// A row list runs the card's whole width, its rows an `sm` inset off
    /// the edge; the other views keep the card's padding.
    pub fn bleeds(&self) -> bool {
        matches!(self.view, View::Rows | View::AppGrid)
    }

    fn body_width(&self, theme: &Theme) -> f64 {
        if self.bleeds() { self.card_width(theme) } else { self.content_width(theme) }
    }

    /// A held panel's body: `panelRowInset` in from the card's sides, its
    /// rows' text then on `panelInset` as in the panel host.
    fn panel_body(&self, theme: &Theme) -> (f64, f64) {
        let i = theme.space.panel_row_inset;
        (i, self.card_width(theme) - i * 2.0)
    }

    /// How far in the field and its rules sit: a held panel's text column
    /// (its body plus the kit's text inset), so its title and the rule
    /// above its groups and tiles line up with them.
    fn field_inset(&self, theme: &Theme) -> f64 {
        if self.on_panel() { self.panel_body(theme).0 + w::text_inset(&theme.space) } else { theme.space.xxl }
    }

    /// The preview pane's offset into the body and its size: the side
    /// column past the list's share, `lg` in from its divider and the
    /// body's edges. The pane's draw and its picture's decode both read it.
    pub fn pane_extent(&self, theme: &Theme) -> (f64, f64, f64) {
        let lg = theme.space.lg;
        let body_w = self.body_width(theme);
        let half = (body_w * self.list_share(theme)).round();
        (half + lg, (body_w - half - lg * 2.0).max(0.0), (self.body_h - lg * 2.0).max(0.0))
    }

    /// The card's height for the level kind, capped by a share of the
    /// output's.
    fn kind_height(&self, theme: &Theme, output_h: f64) -> f64 {
        let s = &theme.space;
        let (kind, share) = if self.app_view() || self.on_panel() {
            (s.popup_height_menu_app, fs_theme::tokens::LAUNCHER.app_height_share)
        } else if self.split() {
            (s.popup_height_menu_split, fs_theme::tokens::LAUNCHER.height_share)
        } else {
            (s.popup_height_menu, fs_theme::tokens::LAUNCHER.height_share)
        };
        if output_h > 0.0 { kind.min(output_h * share) } else { kind }
    }

    /// The level holds a panel (Settings, Information) as its body.
    pub fn on_panel(&self) -> bool {
        self.mode == Mode::Menu && self.children.is_empty() && self.panel.is_some()
    }

    /// `id`'s panel in the launcher: its route as the level, then the
    /// page itself in the module's place.
    pub fn show_panel(&mut self, store: &Store, id: PanelId) -> bool {
        let Some(route) = id.route() else { return false };
        if self.mode != Mode::Menu || self.level.as_deref() != Some(route.as_str()) {
            self.abandon_select();
            self.children.clear();
            self.enter_level(store, Some(route.as_str().to_owned()));
        }
        if self.panel.as_ref().is_none_or(|p| p.id != id) {
            self.panel = Embedded::new(id);
        }
        self.dirty = true;
        true
    }

    pub fn on_mirror(&self) -> bool {
        self.mode == Mode::Menu && self.level.as_deref() == Some(MIRROR_ROUTE)
    }

    /// MirrorService.cycle: false when there is nothing to step to.
    pub fn mirror_cycle(&mut self, store: &Store, delta: i64) -> bool {
        let next = fs_system::camera::step(&store.mirror.cameras, &self.mirror_current, delta);
        if next == self.mirror_current {
            return false;
        }
        self.mirror_current = next;
        self.dirty = true;
        true
    }

    /// The feed box: the body's width, a 4:3 share of its height.
    pub fn mirror_box(&self, theme: &Theme) -> (f64, f64) {
        let s = &theme.space;
        let w = self.content_width(theme) - s.sm * 2.0;
        (w.round(), (w * 3.0 / 4.0).min(self.body_h - s.md * 2.0).max(1.0).round())
    }

    pub fn placeholder(&self, store: &Store) -> String {
        if self.mode != Mode::Menu {
            return self.select_prompt.clone();
        }
        if let Some(c) = &self.column {
            return if c.target.is_some() { "Search targets".into() } else { "Search actions".into() };
        }
        if !self.children.is_empty() {
            return "Search".into();
        }
        match self.level.as_deref().and_then(|l| store.menu.nodes().get(l)) {
            Some(node) => model::prompt_for(Some(node)),
            None => "Search apps, files, commands".into(),
        }
    }

    pub fn section_names(&self) -> Vec<String> {
        model::section_names(&self.sections)
    }

    pub fn columns(&self) -> usize {
        match self.view {
            View::Rows => 1,
            _ => self.layout.columns.max(1),
        }
    }

    /// The launcher plugin whose mode the list holds.
    /// The query Install mode's rows resolve for, while they are the list.
    pub fn install_query(&self) -> Option<&str> {
        let install = self.mode == Mode::Menu && self.children.is_empty() && self.level.as_deref() == Some(fs_menu::providers::INSTALL_ROUTE);
        install.then(|| self.list_text())
    }

    fn plugin_mode(&self) -> Option<&str> {
        let route = self.level.as_deref().filter(|_| self.mode == Mode::Menu && self.children.is_empty())?;
        route.strip_prefix(fs_chrome::plugins::PLUGIN_PREFIX)
    }

    /// The mode token's labels, outermost first; none at the root or
    /// outside Menu mode.
    pub fn token(&self, store: &Store) -> Vec<String> {
        if self.mode != Mode::Menu {
            return Vec::new();
        }
        let mut path = model::mode_path(store.menu.nodes(), self.level.as_deref());
        path.extend(self.children.iter().map(|c| c.label.clone()));
        path
    }

    /// The column's tokens after the mode's: `Actions`, then the target
    /// an action asks for.
    pub fn column_tokens(&self) -> Vec<&'static str> {
        let Some(c) = &self.column else { return Vec::new() };
        let mut out = vec!["Actions"];
        if let Some((Action { run: Run::Target(kind), .. }, _)) = &c.target {
            out.push(kind.label());
        }
        out
    }

    /// The level and its rows, resolved over the warm index.
    fn resolve_rows(&self, store: &Store) -> (View, Vec<Node>, usize, Option<Node>, bool, String) {
        let menu = self.mode == Mode::Menu;
        let q = self.list_text();
        let level = self.level.as_deref();
        let nodes = store.menu.nodes();
        let cond = &store.menu.cond;
        let emoji = menu && level == Some("emoji");
        let children = self.children.last().filter(|_| menu);
        let plugin = self.plugin_mode().is_some();
        let route_rows = children.is_some() || plugin || matches!(level, Some("keybinds" | "calc" | "radio.search" | "install" | "clipboard" | "share.history"));
        let grid_wanted = crate::settings::APP_GRID.get(&store.config);
        let picker = menu && children.is_none() && level == Some(PICKER_ROUTE);
        let monitor = menu && children.is_none() && level == Some(MONITOR_ROUTE);
        let mirror = menu && children.is_none() && level == Some(MIRROR_ROUTE);
        let embedded = menu && children.is_none() && self.panel.is_some();
        let emoji = emoji && children.is_none() && !picker && !monitor && !mirror;
        let view = if embedded {
            View::Panel
        } else if mirror {
            View::Mirror
        } else if monitor {
            View::Monitor
        } else if picker {
            View::Picker
        } else if emoji {
            View::Emoji
        } else if menu && grid_wanted && !route_rows {
            View::AppGrid
        } else {
            View::Rows
        };
        let mut rows: Vec<Node> = match self.mode {
            Mode::Select => {
                let ql = q.to_lowercase();
                self.select_options
                    .iter()
                    .enumerate()
                    .map(|(i, o)| Node::new(format!("select.{i}"), o.clone(), NodeKind::Option))
                    .filter(|n| ql.is_empty() || n.label.to_lowercase().contains(&ql))
                    .collect()
            }
            Mode::Input => Vec::new(),
            Mode::Menu if children.is_some() => children.map_or_else(Vec::new, |c| match &c.into {
                Into::Windows(id) => window_rows(store, id, q),
                Into::Folder(path) => folder_rows(store, path, q),
                Into::Network(ssid) => index::network_rows(store, ssid, q),
            }),
            Mode::Menu if picker => self.picker_rows(store, q),
            Mode::Menu if monitor => monitor_rows(store, q),
            Mode::Menu if mirror || embedded => Vec::new(),
            Mode::Menu if matches!(level, Some("clipboard" | "share.history")) => {
                let history: Vec<Node> = model::visible_children(nodes, level, cond).into_iter().cloned().collect();
                if history.is_empty() {
                    vec![fs_menu::providers::clipboard_empty_row()]
                } else {
                    let matched: Vec<Node> = fs_menu::providers::clipboard_search(&history, q).into_iter().cloned().collect();
                    if matched.is_empty() { vec![fs_menu::providers::clipboard_no_match_row()] } else { matched }
                }
            }
            Mode::Menu if emoji => {
                let paste = crate::settings::CLIPBOARD_PASTE.get(&store.config);
                store.menu.emoji.as_ref().map_or_else(Vec::new, |e| e.rows(q, paste, &store.state.data.emoji_uses, crate::runtime::now_ms() as f64))
            }
            Mode::Menu if level == Some("keybinds") => {
                let reply = store.menu.binds.as_ref().map(|r| r.as_deref().map_err(|_| ()));
                fs_menu::keybinds::rows_for(reply, q)
            }
            Mode::Menu if level == Some("radio.search") => index::radio_search_rows(store, q),
            Mode::Menu if level == Some(fs_menu::providers::INSTALL_ROUTE) => crate::surfaces::packages::install_rows(store, q),
            // The plugin ranks its own rows against the query it was sent.
            Mode::Menu if plugin => model::visible_children(nodes, level, cond).into_iter().cloned().collect(),
            Mode::Menu if q.is_empty() && level.is_none() => {
                let outside = |id: &str| index::outside_subject(store, id);
                let rest = model::root_rest(nodes, &store.state.data.recent_pinned, &store.state.data.picks, crate::runtime::now_ms() as f64, model::RECENT_MAX, cond, outside);
                let recent = rest.first().is_some_and(|r| r.section.as_deref() == Some(model::RECENT_SECTION));
                if view == View::AppGrid && recent {
                    rest.into_iter().filter(|r| r.kind != NodeKind::App).collect()
                } else {
                    rest
                }
            }
            Mode::Menu if q.is_empty() => {
                let gated = level.and_then(|l| nodes.get(l)).is_some_and(|n| !model::is_when_visible(n, cond));
                if gated {
                    vec![model::gated_note_row(&nodes[level.unwrap_or_default()])]
                } else {
                    model::visible_children(nodes, level, cond).into_iter().cloned().collect()
                }
            }
            Mode::Menu if level == Some("calc") => fs_menu::calc::result_node(q).into_iter().collect(),
            Mode::Menu => {
                let boost = fs_menu::search::Boost { abbrev: Some(&self.lookup), picks: &store.state.data.picks, now: crate::runtime::now_ms() as f64 };
                fs_menu::search::rows_for_query(nodes, q, cond, level, boost)
            }
        };
        let mut empty = None;
        let live: Vec<Node> = rows.iter().filter(|r| !(r.kind == NodeKind::Note && r.dim == Some(true))).cloned().collect();
        if live.len() < rows.len() {
            if live.is_empty() {
                empty = rows.first().cloned();
            }
            rows = live;
        }
        if view == View::AppGrid && level.is_none() && q.is_empty() {
            let apps: Vec<Node> = model::visible_children(nodes, Some("apps"), cond)
                .into_iter()
                .filter(|r| r.kind == NodeKind::App)
                .take(model::RECENT_MAX)
                .cloned()
                .collect();
            rows = apps.into_iter().chain(rows).collect();
        }
        let mut app_count = 0;
        if view == View::AppGrid {
            let parts = fs_menu::appgrid::partition(rows);
            rows = parts.rows;
            app_count = parts.app_count;
        }
        let searching = menu && !q.is_empty() && !emoji && !picker && !monitor && !route_rows;
        let key = format!(
            "{:?}\u{1}{}\u{1}{}\u{1}{}\u{1}{}",
            self.mode,
            level.unwrap_or(""),
            children.map_or("", Children::key),
            q,
            if picker && self.picker.select { "select" } else { "" }
        );
        (view, rows, app_count, empty, searching, key)
    }

    /// The query's abbreviations, read once per resolve.
    fn lookup(&self, store: &Store) -> fs_menu::abbrev::Lookup {
        if self.mode != Mode::Menu {
            return Default::default();
        }
        index::abbreviation_lookup(store, self.list_text(), crate::runtime::now_ms() as f64)
    }

    /// Commits the rows, their headings and the cursor.
    pub fn resolve(&mut self, store: &Store, theme: &Theme, kit: &mut Kit) {
        // Only while open: the plugin runs only then, and a query cached
        // while it is down would never be sent once it is back.
        let asked = self.plugin_mode().filter(|_| self.open).map(|id| (id.to_owned(), self.query.text().to_owned()));
        if let Some((id, text)) = asked.as_ref().filter(|a| self.plugin_query.as_ref() != Some(a)) {
            self.ask(Request::Plugin(id.clone(), plugins::query_event(text)));
        }
        self.plugin_query = asked;
        self.refresh_peers(store);
        self.lookup = self.lookup(store);
        self.abbrev_key = if self.mode == Mode::Menu { fs_menu::abbrev::key(self.list_text()) } else { None };
        let (view, rows, app_count, empty, searching, key) = self.resolve_rows(store);
        let fresh = key != self.key;
        let level_label = self.level.as_deref().and_then(|l| store.menu.nodes().get(l)).map(|n| n.label.clone()).unwrap_or_default();
        let sections = if view == View::Picker {
            rows.iter().map(|r| r.section.clone().unwrap_or_default()).collect()
        } else if !self.children.is_empty() {
            vec![String::new(); rows.len()]
        } else {
            model::sections_for(
                &rows,
                &SectionCtx {
                    mode: Some(self.mode),
                    grid: view == View::Emoji,
                    cells: app_count,
                    searching,
                    level: self.level.as_deref(),
                    level_label: &level_label,
                },
            )
        };
        let view_changed = view != self.view;
        let changed = rows.len() != self.rows.len() || rows.iter().zip(&self.rows).any(|(a, b)| a.id != b.id);
        self.rows = rows;
        self.sections = sections;
        self.view = view;
        self.app_count = app_count;
        self.empty = empty;
        self.searching = searching;
        if changed {
            self.touch.settle();
        }
        if fresh {
            self.placed = false;
            self.want.clear();
        }
        let ids: Vec<&str> = self.rows.iter().map(|r| r.id.as_str()).collect();
        let mut index = nav::rederive(&self.want, self.cursor as i64, &ids, fresh, self.placed);
        if view == View::Picker && !self.placed && !self.picker.select {
            // The cursor opens on the tile on the desktop, else on the first
            // past the Style previews.
            let tiles = self.picker_tiles(store);
            let current = tiles.iter().find(|(_, i)| Self::picker_current(store, i)).map(|(_, i)| format!("wallpaper.{}", i.id));
            let first = self.sections.iter().position(|s| s != STYLE);
            index = current.and_then(|id| ids.iter().position(|r| *r == id)).or(first).unwrap_or(0);
        }
        self.key = key;
        self.monitor_strip_h = if view == View::Monitor { ui::measure(&monitor_strip(store, theme, &self.monitor_history), self.content_width(theme), theme, kit).1 } else { 0.0 };
        self.picker_sections = if view == View::Picker { self.picker_sections(store) } else { Vec::new() };
        self.layout(theme, kit);
        if changed && matches!(view, View::Rows | View::AppGrid) {
            // The selection sets its row's label at 600: both weights are
            // shaped here, so a step never waits on the shaper.
            for r in self.rows.iter().take(ROWS_SHAPED_AHEAD) {
                for weight in [Weight::Normal, Weight::Semibold] {
                    ui::measure(&row_words(r.label.clone()).weight(weight), f64::MAX, theme, kit);
                }
            }
        }
        // A key's step resolves the level again before it draws: the cursor
        // landing where the step put it keeps the step's travel.
        let travels = self.travels && !fresh && !view_changed && index == self.cursor;
        self.place(index, travels);
        self.relayout(theme, kit);
        if fresh || view_changed {
            self.scroll = 0.0;
            self.touch.settle();
            self.follow();
        } else if changed && view == View::Picker {
            // A rescan that shrank the grid leaves the scroll past its end.
            self.follow();
        }
        self.dirty = true;
    }

    /// A `Send to…` list open in the column follows LocalSend's scan.
    fn refresh_peers(&mut self, store: &Store) {
        let q = self.query.text().to_owned();
        let Some(c) = &mut self.column else { return };
        let Some((action, targets)) = &mut c.target else { return };
        if action.run != Run::Target(TargetKind::Peer) {
            return;
        }
        let fresh = peer_targets(store);
        if fresh.iter().map(|n| (&n.id, &n.label)).ne(targets.iter().map(|n| (&n.id, &n.label))) {
            *targets = fresh;
            c.refilter(&q);
        }
    }

    /// The list or the column is still scrolling, so the body draws again.
    pub fn rows_moving(&self, now: Instant) -> bool {
        self.touch.running(now) || self.column.as_ref().is_some_and(|c| c.touch.running(now))
    }

    fn place(&mut self, index: usize, travels: bool) {
        let valid = index < self.rows.len();
        self.travels = travels;
        self.wheeled = false;
        self.cursor = if valid { index } else { 0 };
        self.cursor_id = if valid { self.rows[index].id.clone() } else { String::new() };
    }

    /// The body's slots: the app grid's cells and tail, the row list's
    /// rows, the emoji grid's tiles.
    fn layout(&mut self, theme: &Theme, kit: &mut Kit) {
        let s = &theme.space;
        let body_w = self.body_width(theme);
        let halved = self.halved();
        let width = if halved { (body_w * self.list_share(theme)).round() } else { body_w };
        // Pond's list sits a clear `lg` in from the card and under the
        // field's rule; the grids keep their own gutters.
        let inset = if self.view == View::Rows { s.lg } else if self.bleeds() { s.sm } else { s.md };
        let gutter = s.sm;
        let heading_x = if self.bleeds() { s.sm + s.lg - s.control_padding_x } else { 0.0 };
        let heading_h = ui::measure(&heading_el(s, "Ag"), width, theme, kit).1;
        let mut out = Layout::default();
        let mut y = inset;
        let band_of = |section: &str, first: bool| -> f64 {
            if section.is_empty() { 0.0 } else { (if first { 0.0 } else { s.section_gap }) + heading_h + s.row_gap }
        };
        let heading = |i: usize, secs: &[String]| -> String {
            let band = secs.get(i).cloned().unwrap_or_default();
            let prev = if i > 0 { secs.get(i - 1).cloned().unwrap_or_default() } else { String::new() };
            if band == prev { String::new() } else { band }
        };
        match self.view {
            View::Picker => {
                y = self.picker_layout(&mut out, theme, kit, width, heading_h, y);
            }
            View::AppGrid | View::Emoji => {
                let (cells, min_cell) = if self.view == View::AppGrid {
                    (self.app_count, s.control_height * 4.0)
                } else {
                    (self.rows.len(), 0.0)
                };
                let grid_x = if self.view == View::AppGrid { gutter } else { 0.0 };
                let width = width - grid_x * 2.0;
                let columns = if self.view == View::Emoji {
                    model::EMOJI_COLUMNS
                } else {
                    fs_menu::appgrid::columns_for(width, min_cell)
                };
                let cell_w = width / columns.max(1) as f64;
                let name_h = ui::measure(&w::label("Ag"), width, theme, kit).1;
                let cell_h = if self.view == View::AppGrid {
                    s.control_height * 2.0 + s.row_gap + name_h + s.control_padding_y * 2.0 + gutter * 2.0
                } else {
                    cell_w
                };
                let heading_text = if self.view == View::AppGrid && cells > 0 { self.sections.first().cloned().unwrap_or_default() } else { String::new() };
                if !heading_text.is_empty() {
                    out.headings.push((heading_text, y, gutter, String::new()));
                    y += heading_h + s.row_gap;
                }
                for i in 0..cells {
                    let (r, c) = (i / columns.max(1), i % columns.max(1));
                    out.slots.push(Slot { x: grid_x + c as f64 * cell_w, y: y + r as f64 * cell_h, w: cell_w, h: cell_h, band: 0.0 });
                }
                y += cells.div_ceil(columns.max(1)) as f64 * cell_h;
                out.columns = columns;
                out.cell_w = cell_w;
                out.cell_h = cell_h;
                let width = width + grid_x * 2.0;
                for i in cells..self.rows.len() {
                    let h_text = heading(i, &self.sections);
                    let band = band_of(&h_text, i == 0);
                    if !h_text.is_empty() {
                        out.headings.push((h_text, y + band - heading_h - s.row_gap, heading_x, String::new()));
                    }
                    let row_h = row_height(&self.rows[i], theme, kit);
                    out.slots.push(Slot { x: gutter, y, w: width - gutter * 2.0, h: band + row_h, band });
                    y += band + row_h;
                }
            }
            View::Mirror | View::Panel => out.columns = 1,
            View::Rows | View::Monitor => {
                out.columns = 1;
                let side = if self.view == View::Rows { s.lg } else { s.sm };
                let heading_x = heading_x + side - s.sm;
                if self.monitor_strip_h > 0.0 {
                    y += self.monitor_strip_h + s.section_gap;
                }
                for i in 0..self.rows.len() {
                    let h_text = heading(i, &self.sections);
                    let band = band_of(&h_text, i == 0);
                    if !h_text.is_empty() {
                        out.headings.push((h_text, y + band - heading_h - s.row_gap, heading_x, String::new()));
                    }
                    let row_h = row_height(&self.rows[i], theme, kit);
                    out.slots.push(Slot { x: side, y, w: width - side * 2.0, h: band + row_h, band });
                    y += band + row_h;
                }
            }
        }
        out.content_h = y + inset;
        out.foot = if self.view == View::Rows { inset } else { 0.0 };
        out.halved = halved;
        out.body_w = body_w;
        self.layout = out;
    }

    /// The body laid out again where the selection has moved onto or off a
    /// subject with a preview, which halves the list or gives it back, or
    /// the output under it has changed the card's width.
    fn relayout(&mut self, theme: &Theme, kit: &mut Kit) {
        if self.layout.halved != self.halved() || self.layout.body_w != self.body_width(theme) {
            self.layout(theme, kit);
            self.follow();
        }
    }

    /// The tallest body under `cap` that ends on a whole row and the list's
    /// `inset` under it, so a long list never rests on half a row.
    fn whole_rows(&self, cap: f64, inset: f64) -> f64 {
        let bottom = self.layout.slots.iter().map(|s| s.y + s.h).filter(|b| b + inset <= cap).fold(0.0, f64::max);
        if bottom > 0.0 { bottom + inset } else { cap }
    }

    /// The smallest scroll that shows the cursor's slot.
    fn follow(&mut self) {
        let Some(slot) = self.layout.slots.get(self.cursor) else {
            self.scroll = 0.0;
            return;
        };
        // A row heading its section brings the heading with it, and the
        // first row the list's own inset.
        let (top, bottom) = (slot.y, slot.y + slot.h);
        let view = self.body_h - self.layout.foot;
        let mut next = self.scroll;
        let first = self.cursor == 0 || (!matches!(self.view, View::Rows | View::Monitor) && self.cursor < self.cells() && self.cursor < self.layout.columns);
        let top = if first { 0.0 } else { top };
        let h = bottom - top;
        if top < next {
            next = top;
        } else if top + h > next + view {
            next = top + h - view;
        }
        let max = (self.layout.content_h - self.body_h).max(0.0);
        self.scroll = next.clamp(0.0, max);
    }

    fn cells(&self) -> usize {
        match self.view {
            View::AppGrid => self.app_count,
            View::Emoji | View::Picker => self.rows.len(),
            View::Rows | View::Monitor | View::Mirror | View::Panel => 0,
        }
    }

    pub fn move_cursor(&mut self, dir: Dir) {
        let n = self.rows.len();
        if n == 0 {
            return;
        }
        let next = if self.view == View::Picker {
            self.picker_step(dir)
        } else {
            nav::step(self.cursor, dir, n, self.cells(), self.columns(), self.page_step())
        };
        self.place(next.index, next.travels);
        self.placed = true;
        self.want = self.cursor_id.clone();
        self.confirm.clear();
        self.monitor_armed.clear();
        self.from_keys = true;
        self.touch.settle();
        self.follow();
        self.dirty = true;
    }

    /// Ctrl+Tab and Ctrl+Shift+Tab: the first tile of the next or the
    /// previous section.
    fn section_step(&mut self, forward: bool) -> Out {
        let Some(here) = self.sections.get(self.cursor).cloned() else { return Out::None };
        let firsts: Vec<usize> = (0..self.rows.len()).filter(|&k| k == 0 || self.sections[k] != self.sections[k - 1]).collect();
        let current = firsts.iter().rposition(|&k| self.sections[k] == here).unwrap_or(0);
        let target = if forward { firsts.get(current + 1) } else { current.checked_sub(1).and_then(|c| firsts.get(c)) };
        if let Some(&index) = target {
            self.place(index, false);
            self.placed = true;
            self.want = self.cursor_id.clone();
            self.confirm.clear();
            self.from_keys = true;
            self.touch.settle();
            self.follow();
            self.dirty = true;
        }
        Out::None
    }

    fn page_step(&self) -> usize {
        let h = self.body_h;
        if self.view != View::Rows && self.cursor < self.cells() {
            return ((h / self.layout.cell_h.max(1.0)).floor() as usize).max(1) * self.columns();
        }
        1usize.max((h / 32.0).floor() as usize)
    }

    /// The pixels one wheel notch scrolls: a row, or a grid row of cells.
    pub fn wheel_step(&self) -> f64 {
        if matches!(self.view, View::Rows | View::Monitor | View::Panel) { 32.0 } else { self.layout.cell_h.max(1.0) }
    }

    /// One axis frame over the view, with the cursor left where it is: a
    /// wheel's notches glide a step each and stop on either end, a
    /// touchpad's travel lands at once and stretches past an end until the
    /// fingers lift, springing back over `release_ms` or coasting on.
    pub fn scroll(&mut self, travel: Travel, release_ms: f64, slow: f64) {
        let (step, max) = (self.wheel_step(), self.max_scroll());
        let Some((next, glide)) = self.touch.travel(travel, self.scroll, max, step, self.body_h, release_ms, slow) else {
            return;
        };
        self.travels = false;
        self.wheeled = glide;
        self.scroll = next;
        self.dirty = true;
    }

    fn max_scroll(&self) -> f64 {
        (self.layout.content_h - self.body_h).max(0.0)
    }

    /// The same frame over the actions column, which scrolls on its own.
    pub fn scroll_actions(&mut self, travel: Travel, release_ms: f64, slow: f64) {
        let Some(c) = &mut self.column else { return };
        let max = c.max_scroll();
        let Some((next, glide)) = c.touch.travel(travel, c.scroll, max, c.row_h, c.view_h, release_ms, slow) else {
            return;
        };
        c.scroll = next;
        c.wheeled = glide;
        c.travels = false;
        self.dirty = true;
    }

    /// A click on row `index` (launcher spec §6): the row takes the
    /// selection and runs its first action, unless the actions column was
    /// open, which the click only closes.
    pub fn click(&mut self, store: &Store, index: usize) -> Out {
        let column = self.column.is_some();
        self.close_actions();
        if index != self.cursor {
            self.place(index, false);
            self.placed = true;
            self.want = self.cursor_id.clone();
            self.confirm.clear();
        }
        self.from_keys = false;
        self.dirty = true;
        if column { Out::None } else { self.activate(store, index) }
    }

    pub fn view_cursor(&self) -> Value {
        let k = self.view.name();
        let Some(slot) = self.layout.slots.get(self.cursor).filter(|_| self.cursor < self.rows.len()) else {
            return json!({"view": k, "index": -1, "id": ""});
        };
        let top = (slot.y + slot.band - self.scroll).round() as i64;
        let bottom = (slot.y + slot.h - self.scroll).round() as i64;
        json!({
            "view": k, "index": self.cursor, "id": self.rows[self.cursor].id,
            "top": top, "bottom": bottom, "viewport": self.body_h.round() as i64,
            "left": slot.x.round() as i64, "right": (slot.x + slot.w).round() as i64,
        })
    }

    pub fn status(&self, store: &Store) -> Value {
        let checked: Vec<&str> = {
            let snap = snapshot(store);
            self.rows
                .iter()
                .filter(|n| crate::surfaces::packages::row_checked(store, n, toggles::checked_for(Some(n), Some(&snap), Some(&store.menu.checked))))
                .map(|n| n.id.as_str())
                .collect()
        };
        let cells: Vec<&str> = match self.view {
            View::AppGrid => self.rows[..self.app_count].iter().map(|r| r.id.as_str()).collect(),
            _ => self.rows.iter().map(|r| r.id.as_str()).collect(),
        };
        let drawn: Vec<Value> = match self.view {
            View::AppGrid => self.rows[..self.app_count].iter().map(|r| json!({"id": r.id, "label": r.label})).collect(),
            View::Rows | View::Monitor | View::Mirror => {
                self.rows
                    .iter()
                    .map(|r| {
                        let picture = !r.icon_source.is_empty() && store.menu.row_icons.get(&r.icon_source).is_some_and(Option::is_some);
                        json!({"id": r.id, "label": r.label, "value": crate::surfaces::packages::row_value(store, r), "picture": picture})
                    })
                    .collect()
            }
            View::Emoji | View::Picker | View::Panel => Vec::new(),
        };
        json!({
            "isOpen": self.open,
            "level": self.level,
            "scrollTop": (self.scroll + self.touch.value(Instant::now())).round() as i64,
            "scrollMax": self.max_scroll().round() as i64,
            "overscroll": self.touch.value(Instant::now()).round() as i64,
            "coasting": self.touch.coasting(),
            "viewHeight": self.body_h.round() as i64,
            "wheelStep": self.wheel_step().round() as i64,
            "placeholder": self.placeholder(store),
            "query": w::masked(&self.query),
            "prompt": (self.mode == Mode::Input).then_some(self.select_prompt.as_str()),
            "secretShown": self.query.secret && self.query.shown,
            "caret": self.query.caret_chars().0,
            "selection": self.query.selection().is_some(),
            "sections": self.section_names(),
            "view": self.view.name(),
            "panel": self.panel.as_ref().filter(|_| self.on_panel()).map(|p| p.id.as_str()),
            "panelCursor": self.panel.as_ref().filter(|_| self.on_panel()).and_then(|p| p.cursor.shown()),
            "columns": self.columns(),
            "cursor": self.cursor,
            "cursorId": self.cursor_id,
            "empty": self.empty.as_ref().map_or("", |n| n.id.as_str()),
            "mode": match self.mode { Mode::Menu => "menu", Mode::Select => "select", Mode::Input => "input" },
            "viewCursor": self.view_cursor(),
            "rows": self.rows.len(),
            "cells": cells,
            "ids": self.rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            "checked": checked,
            "drawn": drawn,
            "abbrev": self.rows.get(self.cursor).and_then(|n| self.lookup.source_of(&fs_menu::actions::subject_id(n).to_string())).map(fs_menu::abbrev::Source::as_str),
            "token": self.token(store),
            "actionsOpen": self.column.is_some(),
            "actions": self.column.as_ref().map_or_else(Vec::new, Column::ids),
            "actionCursor": self.column.as_ref().map_or(-1, |c| c.cursor as i64),
            "actionsScroll": self.column.as_ref().map_or(0, |c| (c.scroll + c.touch.value(Instant::now())).round() as i64),
            "actionsScrollMax": self.column.as_ref().map_or(0, |c| c.max_scroll().round() as i64),
            "actionsMore": self.column.as_ref().and_then(|c| c.more(c.scroll + c.touch.value(Instant::now()))).map_or(0, |(_, n)| n),
            "pane": self.pane().map(|n| self.pane_status(store, n)),
            "cardWidth": self.card_width(&store.theme.theme).round() as i64,
        })
    }

    /// What the preview pane holds, for `menu status`.
    fn pane_status(&self, store: &Store, n: &Node) -> Value {
        use fs_menu::providers::Peek;
        let kind = pane_kind(n).map_or("", PaneKind::name);
        let mut v = json!({"kind": kind, "id": n.id, "path": n.path});
        let peek = store.menu.peek.as_ref().filter(|(p, _)| *p == n.path).map(|(_, p)| &p.peek);
        match peek {
            Some(Peek::Text(lines)) => v["lines"] = json!(lines),
            Some(Peek::Folder { count, names }) => {
                v["count"] = json!(count);
                v["names"] = json!(names.iter().map(|e| e.name.as_str()).collect::<Vec<_>>());
            }
            Some(Peek::Image) => v["image"] = json!(true),
            Some(Peek::Binary) => v["binary"] = json!(true),
            Some(Peek::Unreadable(why)) => v["error"] = json!(why),
            None => {}
        }
        match pane_picture(n) {
            Some(PanePicture::Decoded(path)) => {
                v["picture"] = json!(store.clipboard.preview.as_ref().is_some_and(|p| p.path == path && p.bitmap.is_some()));
            }
            Some(PanePicture::Window(_)) => v["picture"] = json!(self.pane_frame.is_some()),
            None => {}
        }
        v
    }

    fn resolve_route(store: &Store, route: &str) -> Option<String> {
        let nodes = store.menu.nodes();
        if nodes.contains_key(route) {
            return Some(route.to_owned());
        }
        nodes.values().find(|n| n.aliases.iter().any(|a| a == route)).map(|n| n.id.clone())
    }

    /// The level a route row stands for: its own id, or a link's target.
    fn level_of(store: &Store, node: &Node) -> Option<String> {
        match node.kind {
            NodeKind::Submenu | NodeKind::Provider => Some(node.id.clone()),
            NodeKind::Link => Some(node.target.clone().filter(|t| store.menu.nodes().contains_key(t)).unwrap_or(node.id.clone())),
            _ => None,
        }
    }

    /// `open(route)`: a fresh session on `route`'s level, or a `:`-led
    /// prefill at the root, which a route's prefix turns into its mode.
    pub fn open(&mut self, store: &Store, route: Option<&str>) {
        self.after_unmap = None;
        self.grab.cancel();
        self.abandon_select();
        self.ask_fresh(store);
        self.ask(Request::Plugins(true));
        let route = route.filter(|r| !r.is_empty());
        let prefill = route.filter(|r| r.starts_with(':')).unwrap_or("").to_owned();
        let mut target = None;
        if prefill.is_empty()
            && let Some(id) = route.and_then(|r| Self::resolve_route(store, r))
            && let Some(node) = store.menu.nodes().get(&id)
        {
            target = Self::level_of(store, node);
        }
        self.enter_level(store, target);
        self.query.set(&prefill);
        self.query_changed(store);
        self.open = true;
        self.key.clear();
    }

    /// The conditions and the binds, asked for once the tree first arrives
    /// and again on every open. An open draws on the last results; each
    /// check runs on the service thread and only a changed one lands, so
    /// just its row moves. The apps follow their own entry watch and launch
    /// counts.
    pub fn ask_fresh(&self, store: &Store) {
        let conds: Vec<(String, String, bool)> = store
            .menu
            .nodes()
            .values()
            .flat_map(|n| {
                let when = n.when.clone().filter(|c| !c.starts_with(toggles::PREFIX)).map(|c| (n.id.clone(), c, true));
                let checked = match &n.checked {
                    Some(toggles::Checked::Shell(line)) => Some((n.id.clone(), line.clone(), false)),
                    _ => None,
                };
                when.into_iter().chain(checked)
            })
            .collect();
        self.ask(Request::Index(Ask::Conds(conds)));
        self.ask(Request::Index(Ask::Binds));
        self.ask(Request::Index(Ask::Files));
        let paths: Vec<String> = store.state.data.picks.iter().filter_map(|r| match SubjectId::parse(&r.id) { SubjectId::File(path) => Some(path), _ => None }).collect();
        if !paths.is_empty() {
            self.ask(Request::Index(Ask::Stat(paths)));
        }
    }

    pub fn open_select(&mut self, prompt: &str, options: Vec<String>, token: &str) {
        self.begin_selection();
        self.mode = Mode::Select;
        self.select_prompt = prompt.into();
        self.select_options = options;
        self.select_token = token.into();
        self.confirm.clear();
        self.query.clear();
        self.open = true;
        self.key.clear();
    }

    pub fn open_input(&mut self, prompt: &str, token: &str, secret: bool) {
        self.begin_selection();
        self.mode = Mode::Input;
        self.query.secret = secret;
        self.query.shown = false;
        self.select_prompt = prompt.into();
        self.select_options.clear();
        self.select_token = token.into();
        self.confirm.clear();
        self.query.clear();
        self.open = true;
        self.key.clear();
    }

    /// A secret input step holding `text`, drawn in the clear.
    pub fn open_shown(&mut self, prompt: &str, token: &str, text: &str) {
        self.open_input(prompt, token, true);
        self.query.set(text);
        self.query.shown = true;
    }

    pub fn close(&mut self) {
        self.sessions += 1;
        self.abandon_select();
        if self.on_picker() {
            self.picker_abandon();
        }
        self.monitor_wants = None;
        self.monitor_armed.clear();
        if self.on_mirror() {
            self.ask(Request::Mirror(crate::services::mirror::Cmd::Stream(None)));
        }
        self.open = false;
        self.confirm.clear();
        self.column = None;
        self.children.clear();
        self.panel = None;
        self.ask(Request::Plugins(false));
        self.plugin_query = None;
    }

    fn write_selection(&mut self, value: Option<String>) {
        let secret = self.query.secret;
        let token = self.select_token.clone();
        self.ask(Request::Selected(Selected { token, cancelled: value.is_none(), value, secret }));
        if secret {
            self.query.clear();
            self.query.secret = false;
            self.query.shown = false;
        }
    }

    fn abandon_select(&mut self) {
        if self.mode != Mode::Menu {
            self.write_selection(None);
            self.mode = Mode::Menu;
        }
    }

    fn begin_selection(&mut self) {
        self.after_unmap = None;
        self.grab.cancel();
        let pending = self.mode != Mode::Menu;
        self.abandon_select();
        if !pending {
            self.ask(Request::SelectionCleared);
        }
    }

    fn complete_select(&mut self, value: &str) {
        self.write_selection(Some(value.to_owned()));
        self.mode = Mode::Menu;
        self.close();
    }

    fn enter_level(&mut self, store: &Store, id: Option<String>) {
        if self.level.as_deref() == Some(PICKER_ROUTE) && id.as_deref() != Some(PICKER_ROUTE) {
            self.picker_abandon();
        }
        let entering = id.as_deref() == Some(PICKER_ROUTE);
        self.monitor_armed.clear();
        if id.as_deref() == Some(MIRROR_ROUTE) {
            self.mirror_current.clear();
            self.ask(Request::Mirror(crate::services::mirror::Cmd::List));
        } else if self.level.as_deref() == Some(MIRROR_ROUTE) {
            self.ask(Request::Mirror(crate::services::mirror::Cmd::Stream(None)));
        }
        if id.as_deref() == Some(MONITOR_ROUTE) && self.level.as_deref() != Some(MONITOR_ROUTE) {
            self.monitor_history = MonitorHistory::default();
        }
        self.column = None;
        self.children.clear();
        self.monitor_wants = (id.as_deref() == Some(MONITOR_ROUTE)).then(|| {
            use crate::services::wants::{Source, Want};
            (Want::new(Source::Monitor), Want::new(Source::Processes))
        });
        // A level that is a launcher route of its own holds its page.
        let route = id.as_deref().and_then(PanelId::parse).filter(|p| p.route() == Some(*p));
        match route {
            Some(r) if self.level.as_deref() != Some(r.as_str()) || self.panel.is_none() => self.panel = Embedded::new(r),
            Some(_) => {}
            None => self.panel = None,
        }
        self.level = id;
        if entering {
            self.picker_enter(store);
        }
        self.confirm.clear();
        self.from_keys = true;
        self.query.clear();
        if self.level.as_deref() == Some("emoji") && store.menu.emoji.is_none() {
            self.ask(Request::Index(Ask::Emoji));
        }
    }

    /// One level up: Backspace, Escape and ← on an empty field, and a click
    /// on the token. At the root it closes.
    pub fn pop(&mut self, store: &Store) -> Out {
        if self.children.pop().is_some() {
            // One listing is held at a time: the folder stepped back into
            // is read again.
            if let Some(Children { into: Into::Folder(path), .. }) = self.children.last() {
                self.ask(Request::Index(Ask::Folder(path.clone())));
            }
            self.query.clear();
            self.confirm.clear();
            self.from_keys = true;
            self.dirty = true;
            return Out::None;
        }
        let Some(level) = self.level.clone() else {
            self.close();
            return Out::Close;
        };
        if self.on_picker()
            && let Some(page) = self.picker.from.take()
        {
            return Out::Call(IpcCall::new("panel", "open").arg(page.as_str()));
        }
        let parent = store.menu.nodes().get(&level).and_then(|n| n.parent_id.clone());
        self.enter_level(store, parent);
        self.dirty = true;
        Out::None
    }

    /// A click on the token's ancestor at `depth`: every mode inside it
    /// left, that one kept.
    pub fn leave_to(&mut self, store: &Store, depth: usize) {
        let mut held = self.token(store).len();
        while held > depth + 1 {
            self.pop(store);
            let now = self.token(store).len();
            if now >= held {
                break;
            }
            held = now;
        }
    }

    pub fn set_query(&mut self, store: &Store, text: &str) {
        self.query.set(text);
        self.query_changed(store);
    }

    /// The clipboard's text into the query.
    pub fn paste(&mut self, store: &Store, text: &str) {
        if self.query.paste(text) {
            self.query_changed(store);
            self.dirty = true;
        }
    }

    /// A route's prefix and a space become its mode, what follows them the
    /// query inside it.
    fn query_changed(&mut self, store: &Store) {
        self.confirm.clear();
        self.monitor_armed.clear();
        if let Some(c) = &mut self.column {
            c.refilter(self.query.text());
            return;
        }
        if self.mode != Mode::Menu {
            return;
        }
        let Some((level, rest)) = model::prefixed(store.menu.nodes(), self.query.text()).and_then(|(n, rest)| Some((Self::level_of(store, n)?, rest.to_owned()))) else {
            return;
        };
        self.enter_level(store, Some(level));
        self.query.set(&rest);
    }

    /// The row as a subject, with what the shell knows about it.
    pub fn subject<'a>(&'a self, store: &'a Store, node: &'a Node) -> fs_menu::actions::Subject<'a> {
        let windows = match &node.entry {
            Some(entry) if node.kind == NodeKind::App => app_windows(store, entry).len(),
            _ => 0,
        };
        let removable = match &node.entry {
            Some(entry) if node.kind == NodeKind::App && store.activity.fspec.installed => {
                if !store.packages.owners.contains_key(&entry.id) {
                    self.ask(Request::Packages(crate::services::packages::Cmd::Owner(entry.id.clone())));
                }
                store.packages.removable(&entry.id)
            }
            _ => None,
        };
        let id = fs_menu::actions::subject_id(node).to_string();
        let abbrev = self.abbrev_key.as_deref().zip(self.lookup.source_of(&id));
        fs_menu::actions::Subject {
            node,
            windows,
            discrete_gpu: store.monitor.cards.iter().any(|c| c.discrete),
            picker_select: self.picker.select,
            abbrev,
            pinned: store.state.data.recent_pinned.contains(&id),
            removable,
            single: node.kind == NodeKind::Image
                && self.on_picker()
                && self.picker_tile_of(store, &node.id).is_some_and(|t| matches!(t.kind, fs_menu::providers::ItemKind::Single { .. })),
        }
    }

    /// The cursor onto `index` as a key would put it, the column left as
    /// it is.
    fn select(&mut self, index: usize) {
        if index != self.cursor {
            self.place(index, false);
            self.placed = true;
            self.want = self.cursor_id.clone();
            self.confirm.clear();
        }
    }

    /// Tab, a right-click and `menu actions`: the row's actions in the
    /// column beside the list. False with nothing to list.
    pub fn open_actions(&mut self, store: &Store, index: usize) -> bool {
        if self.mode != Mode::Menu || matches!(self.view, View::Mirror) {
            return false;
        }
        let Some(node) = self.rows.get(index).cloned() else { return false };
        // The sky has no file to open or slot of its own to fill.
        if self.view == View::Picker && !node.path.starts_with('/') {
            return false;
        }
        let actions = fs_menu::actions::actions_for(&self.subject(store, &node));
        let kind = node.kind.as_str();
        let asked: Vec<String> = fs_chrome::plugins::launcher_plugins(&store.plugins.resolved.plugins)
            .filter(|p| p.subjects.as_ref().is_some_and(|k| k.iter().any(|k| k == kind)))
            .map(|p| p.id.clone())
            .collect();
        if actions.is_empty() && asked.is_empty() {
            return false;
        }
        let subject = fs_menu::actions::subject_id(&node).to_string();
        let asked_at = Instant::now();
        for id in &asked {
            self.ask(Request::Plugin(id.clone(), plugins::actions_event(&subject, kind, &node.path)));
        }
        self.close_actions();
        self.select(index);
        self.confirm.clear();
        let shown = (0..actions.len()).collect();
        let list_query = self.query.text().to_owned();
        self.query.clear();
        self.column = Some(Column {
            node,
            actions,
            list_query,
            target: None,
            shown,
            cursor: 0,
            opened: Instant::now(),
            scroll: 0.0,
            touch: Touchpad::new(),
            wheeled: false,
            travels: false,
            row_h: 0.0,
            view_h: 0.0,
            asked,
            asked_at,
            joined: None,
        });
        self.dirty = true;
        true
    }

    /// The plugins' answers to the open column's `actions` events, joined
    /// under its actions as they arrive. One later than [`PLUGIN_ANSWER`]
    /// is left out of this opening; the selected action stays where it is.
    pub fn plugin_answers(&mut self, store: &Store) -> bool {
        let filter = self.query.text().to_owned();
        let Some(c) = self.column.as_mut().filter(|c| c.target.is_none() && !c.asked.is_empty()) else { return false };
        let subject = fs_menu::actions::subject_id(&c.node).to_string();
        let mut joined = false;
        for id in std::mem::take(&mut c.asked) {
            let answer = store.plugins.answers.get(&id).filter(|a| a.at >= c.asked_at && a.subject == subject);
            let Some(answer) = answer else {
                c.asked.push(id);
                continue;
            };
            if answer.at > c.asked_at + PLUGIN_ANSWER {
                continue;
            }
            let base = c.actions.len();
            for a in &answer.actions {
                if c.actions.iter().any(|have| have.id == a.id) {
                    continue;
                }
                let label = if a.label.is_empty() { a.id.clone() } else { a.label.clone() };
                let mut action = Action::new(&a.id, label, Run::Intent(index::plugin_send(&id, &plugins::action_event(&subject, &a.id))));
                action.icon = Some(a.icon.clone()).filter(|i| !i.is_empty());
                c.actions.push(action);
            }
            let labels: Vec<&str> = c.actions[base..].iter().map(|a| a.label.as_str()).collect();
            c.shown.extend(fs_menu::actions::filter(&labels, &filter).into_iter().map(|i| i + base));
            if c.actions.len() > base {
                c.joined.get_or_insert((base, Instant::now()));
                joined = true;
            }
        }
        if joined {
            self.dirty = true;
        }
        joined
    }

    /// Uninstall joining an app's open column when the owner query answers
    /// after Tab; the selected action stays where it is.
    pub fn owner_answer(&mut self, store: &Store) -> bool {
        let filter = self.query.text().to_owned();
        let Some(c) = self.column.as_ref().filter(|c| c.target.is_none() && !c.actions.iter().any(|a| a.id == "uninstall")) else { return false };
        let Some(action) = fs_menu::actions::uninstall_action(&self.subject(store, &c.node)) else { return false };
        let Some(c) = self.column.as_mut() else { return false };
        let at = c.actions.len();
        if !fs_menu::actions::filter(&[action.label.as_str()], &filter).is_empty() {
            c.shown.push(at);
        }
        c.actions.push(action);
        self.dirty = true;
        true
    }

    /// The column gone, the list's query back in the field.
    pub fn close_actions(&mut self) {
        if let Some(c) = self.column.take() {
            self.query.set(&c.list_query);
            self.confirm.clear();
            self.dirty = true;
        }
    }

    /// Shift+Tab, Escape and ← in the column: a target list back to its
    /// actions, the actions back to the list.
    fn column_back(&mut self) {
        let Some(c) = &mut self.column else { return };
        if c.target.take().is_none() {
            return self.close_actions();
        }
        self.query.clear();
        c.refilter("");
        c.opened = Instant::now();
        self.confirm.clear();
        self.dirty = true;
    }

    fn column_move(&mut self, dir: Dir) {
        let page = (self.body_h / fs_theme::tokens::LAUNCHER.row_height).floor().max(1.0) as usize;
        let Some(c) = &mut self.column else { return };
        c.cursor = nav::step(c.cursor, dir, c.shown.len(), 0, 1, page).index;
        c.follow();
        self.confirm.clear();
        self.dirty = true;
    }

    /// Enter in the column: the action, or the target, under its cursor.
    fn column_run(&mut self, store: &Store) -> Out {
        let Some(c) = &self.column else { return Out::None };
        let Some(&i) = c.shown.get(c.cursor) else { return Out::None };
        let node = c.node.clone();
        match &c.target {
            Some((_, targets)) if targets[i].kind == NodeKind::Note => Out::None,
            Some((action, targets)) => {
                let (action, target) = (action.clone(), targets[i].clone());
                self.close_actions();
                self.run_target(store, &node, &action, &target)
            }
            None => {
                let action = c.actions[i].clone();
                self.run_from_column(store, &node, &action)
            }
        }
    }

    /// One action off the column: a target action lists its targets in
    /// it, a confirm action arms there, anything else closes it and runs.
    fn run_from_column(&mut self, store: &Store, node: &Node, action: &Action) -> Out {
        match action.run {
            Run::Note => return Out::None,
            Run::Target(kind) => {
                if kind == TargetKind::Peer {
                    self.ask(Request::PeerScan);
                }
                let targets = targets_for(store, kind);
                if let Some(c) = &mut self.column {
                    c.target = Some((action.clone(), targets));
                    c.refilter("");
                    c.opened = Instant::now();
                }
                self.query.clear();
                self.dirty = true;
                return Out::None;
            }
            _ => {}
        }
        let armed = Confirm::of(node, action);
        if action.confirm && !self.confirm.is(&armed) {
            self.confirm.arm(armed);
            self.dirty = true;
            return Out::None;
        }
        self.close_actions();
        // Closing the column disarms; this run is the confirmed one.
        if action.confirm {
            self.confirm.arm(armed);
        }
        self.run(store, node, action)
    }

    /// The column action a first Enter armed, by position in the actions.
    fn column_armed(&self) -> Option<usize> {
        let c = self.column.as_ref()?;
        let armed = self.confirm.get().filter(|k| k.node == c.node.id)?;
        c.actions.iter().position(|a| a.id == armed.action)
    }

    /// A click on the column's `i`th entry.
    pub fn column_pick(&mut self, store: &Store, i: usize) -> Out {
        if let Some(c) = &mut self.column
            && i < c.shown.len()
        {
            c.cursor = i;
        }
        self.column_run(store)
    }

    /// `menu runAction`: the action `id` on row `index`, as the column
    /// would run it, which opens the column for one that asks a target.
    pub fn run_action(&mut self, store: &Store, index: usize, id: &str) -> Result<Out, String> {
        let on_row = self.column.as_ref().is_some_and(|c| self.rows.get(index).is_some_and(|r| r.id == c.node.id));
        if !on_row && !self.open_actions(store, index) {
            return Err(format!("row {index} has no actions"));
        }
        let Some((node, action)) = self.column.as_ref().and_then(|c| Some((c.node.clone(), c.actions.iter().find(|a| a.id == id)?.clone()))) else {
            if !on_row {
                self.close_actions();
            }
            return Err(format!("no action {id}"));
        };
        self.query.clear();
        if let Some(c) = &mut self.column {
            c.target = None;
            c.refilter("");
            c.cursor = c.actions.iter().position(|a| a.id == id).unwrap_or(0);
        }
        Ok(self.run_from_column(store, &node, &action))
    }

    /// →, and `menu into`: row `index`'s own children, `None` when it has
    /// none to step into.
    pub fn step_into(&mut self, store: &Store, index: usize) -> Option<Out> {
        if self.mode != Mode::Menu {
            return None;
        }
        let node = self.rows.get(index).cloned()?;
        let action = fs_menu::actions::into_action(&self.subject(store, &node))?;
        self.close_actions();
        self.select(index);
        Some(self.run(store, &node, &action))
    }

    /// A target action's pick: the subject's windows onto a workspace, a
    /// file opened in an app or sent to a peer.
    fn run_target(&mut self, store: &Store, node: &Node, action: &Action, target: &Node) -> Out {
        let pick = match action.run {
            Run::Target(TargetKind::App) => {
                let Some(id) = target.id.strip_prefix("target-app:") else { return Out::None };
                let Some(mut argv) = crate::services::launch::app_argv(&store.menu, id) else { return Out::None };
                argv.push(node.path.clone());
                Target::Open { id: id.to_owned(), argv }
            }
            Run::Target(TargetKind::Peer) => {
                use crate::services::localsend;
                let Some(peer) = target.id.strip_prefix("peer:").and_then(|i| i.parse::<usize>().ok()).and_then(|i| store.localsend.peers.get(i)) else {
                    return Out::None;
                };
                let payload = match fs_menu::providers::clipboard_entry_id(node) {
                    Some(id) => {
                        let Some(entry) = store.clipboard.items.iter().find(|e| e.id == id) else { return Out::None };
                        localsend::Payload::of(entry)
                    }
                    None => localsend::Payload::Path(node.path.clone()),
                };
                Target::Send { peer: peer.clone(), payload }
            }
            Run::Target(TargetKind::Workspace) => {
                let Some(idx) = target.id.strip_prefix("workspace:").and_then(|i| i.parse::<i64>().ok()) else { return Out::None };
                let windows = match &node.entry {
                    Some(entry) if node.kind == NodeKind::App => app_windows(store, entry),
                    _ => vec![WindowId::new(node.path.clone())],
                };
                let windows = windows.into_iter().filter(|w| !w.is_empty()).collect();
                Target::Move { windows, idx, output: store.compositor.snapshot.focused_output_name.clone() }
            }
            _ => return Out::None,
        };
        self.picked(store, node);
        self.close();
        Out::Target(pick)
    }

    /// `Edit abbreviation`'s answer: the subject pinned under the new
    /// abbreviation, the one it edits forgotten.
    pub fn edit_answer(&mut self, store: &Store, value: Option<String>, cancelled: bool) {
        let Some((subject, old)) = self.editing.take() else { return };
        let Some(new) = value.filter(|_| !cancelled).as_deref().and_then(fs_menu::abbrev::key) else { return };
        index::move_abbreviation(store, &old, &new, &subject);
    }

    /// Enter: the row's first action.
    pub fn activate(&mut self, store: &Store, index: usize) -> Out {
        self.close_actions();
        let Some(node) = self.rows.get(index).cloned() else { return Out::None };
        if node.kind == NodeKind::Option {
            self.complete_select(&node.label);
            return Out::Close;
        }
        if self.view == View::Picker
            && let Some(fs_menu::providers::ItemKind::Mode { mode, .. }) = self.picker_tile_of(store, &node.id).map(|t| t.kind)
        {
            return self.picker_style(mode);
        }
        match fs_menu::actions::first_action(&self.subject(store, &node)) {
            Some(action) => self.run(store, &node, &action),
            None => Out::None,
        }
    }

    /// Shift+Enter: the row's second action, else its first.
    pub fn activate_alternate(&mut self, store: &Store, index: usize) -> Out {
        self.close_actions();
        let Some(node) = self.rows.get(index).cloned() else { return Out::None };
        match fs_menu::actions::second_action(&self.subject(store, &node)) {
            // One that asks a target lists them in the column, as Tab then
            // Enter on it would.
            Some(action) if matches!(action.run, Run::Target(_)) => self.run_action(store, index, &action.id).unwrap_or(Out::None),
            Some(action) => self.run(store, &node, &action),
            None => self.activate(store, index),
        }
    }

    fn picked(&self, store: &Store, node: &Node) {
        if self.mode == Mode::Menu {
            index::record_pick(store, node, self.list_text(), crate::runtime::now_ms() as f64);
        }
    }

    /// Whether a confirm action on row `id` is armed.
    pub fn confirming(&self, id: &str) -> bool {
        self.confirm.get().is_some_and(|k| k.node == id)
    }

    /// One action on one row. A `confirm` action arms on the first run and
    /// runs on the second; a `keepOpen` one leaves the launcher up.
    pub fn run(&mut self, store: &Store, node: &Node, action: &fs_menu::actions::Action) -> Out {
        use fs_menu::actions::Run;
        if action.confirm && !self.confirm.press(Confirm::of(node, action)) {
            self.dirty = true;
            return Out::None;
        }
        self.confirm.clear();
        let out = match &action.run {
            Run::Into(id) => {
                self.picked(store, node);
                let into = if *id != fs_menu::actions::subject_id(node).to_string() {
                    None
                } else if node.kind == NodeKind::App {
                    Some(Into::Windows(id.clone()))
                } else if fs_menu::providers::is_folder_subject(node) {
                    self.ask(Request::Index(Ask::Folder(node.path.clone())));
                    Some(Into::Folder(node.path.clone()))
                } else if fs_menu::providers::is_wifi_network(node) {
                    Some(Into::Network(node.label.clone()))
                } else {
                    None
                };
                if let Some(into) = into {
                    self.children.push(Children { into, label: node.label.clone() });
                    self.query.clear();
                    self.from_keys = true;
                    self.dirty = true;
                    return Out::None;
                }
                let to = if store.menu.nodes().contains_key(id) { id.clone() } else { node.id.clone() };
                self.enter_level(store, Some(to));
                return Out::None;
            }
            Run::Input(prompt) => {
                let subject = fs_menu::actions::subject_id(node).to_string();
                let editing = self.abbrev_key.clone().map(|q| (subject, q));
                self.open_input(prompt, ABBREV_EDIT, false);
                self.editing = editing;
                return Out::None;
            }
            Run::Target(_) | Run::Note => return Out::None,
            Run::Ipc(call) => Out::Call(call.clone()),
            Run::Intent(intent) => {
                let subject = fs_menu::actions::subject_id(node);
                match intent {
                    // An abbreviation's own actions are no pick: picking
                    // would teach the pair straight back. A pin is no pick
                    // either: it would count twice.
                    Intent::AbbrevForget(_) | Intent::AbbrevPin(_) | Intent::RecentPin(_) | Intent::RecentUnpin(_) => {
                        self.dirty = true;
                        return Out::Intent(intent.clone(), subject, action.keep_open);
                    }
                    // The picker closes the card itself once a tile takes
                    // the picture.
                    Intent::WallpaperSet(_) => {
                        self.picked(store, node);
                        return Out::Intent(intent.clone(), subject, true);
                    }
                    Intent::WallpaperSetFor(..) => return Out::Intent(intent.clone(), subject, true),
                    Intent::EndProcess(pid) => {
                        self.dirty = true;
                        if !self.monitor_armed.press(*pid) {
                            return Out::None;
                        }
                    }
                    _ => {}
                }
                if action.paste_after {
                    self.paste = true;
                }
                self.picked(store, node);
                return Out::Intent(intent.clone(), subject, action.keep_open);
            }
            Run::Spawn(argv) => Out::Spawn(argv.clone()),
            Run::Shell(line) if line.is_empty() => Out::None,
            Run::Shell(line) => Out::Spawn(["sh", "-c", line].map(String::from).to_vec()),
        };
        if action.paste_after {
            self.paste = true;
        }
        self.picked(store, node);
        if action.keep_open {
            return out;
        }
        self.close();
        match out {
            Out::Call(_) | Out::Intent(..) | Out::Spawn(_) | Out::Target(_) => out,
            Out::None | Out::Close => Out::Close,
        }
    }

    /// One key, through `keys::key_action`, or into the field as `edit`.
    pub fn key(&mut self, store: &Store, key: keys::Key, edit: field::Key, mods: field::Modifiers, repeat: bool, text: Option<&str>) -> Out {
        let column = self.column.is_some();
        let caret_end = self.query.caret() == self.query.text().len() && self.query.selection().is_none();
        let into = key == keys::Key::Right
            && !column
            && caret_end
            && self.view == View::Rows
            && self.rows.get(self.cursor).is_some_and(|n| fs_menu::actions::into_action(&self.subject(store, n)).is_some());
        let ctx = KeyCtx {
            mode: self.key_mode(),
            query: !self.query.is_empty(),
            grid: !matches!(self.view, View::Rows | View::Monitor),
            app_view: false,
            scrollable: false,
            sections: self.view == View::Picker && self.picker_sections.len() > 1,
            level: self.level.is_some() || !self.children.is_empty(),
            column,
            into,
            mirror: self.on_mirror(),
        };
        let action = keys::key_action(key, mods, repeat, &ctx);
        if column {
            match action {
                KeyAction::Up => self.column_move(Dir::Up),
                KeyAction::Down => self.column_move(Dir::Down),
                KeyAction::PageUp => self.column_move(Dir::PageUp),
                KeyAction::PageDown => self.column_move(Dir::PageDown),
                KeyAction::Home => self.column_move(Dir::Home),
                KeyAction::End => self.column_move(Dir::End),
                KeyAction::Back => self.column_back(),
                KeyAction::Clear => self.set_query(store, ""),
                KeyAction::Activate | KeyAction::ActivateAlternate => return self.column_run(store),
                KeyAction::Pass => return self.edit(store, edit, mods, text),
                _ => {}
            }
            self.dirty = true;
            return Out::None;
        }
        let out = match action {
            KeyAction::Pass => return self.edit(store, edit, mods, text),
            KeyAction::Actions => {
                self.open_actions(store, self.cursor);
                Out::None
            }
            KeyAction::Into => self.step_into(store, self.cursor).unwrap_or(Out::None),
            KeyAction::Up => self.dir(Dir::Up),
            KeyAction::Down => self.dir(Dir::Down),
            KeyAction::Left => self.dir(Dir::Left),
            KeyAction::Right => self.dir(Dir::Right),
            KeyAction::PageUp => self.dir(Dir::PageUp),
            KeyAction::PageDown => self.dir(Dir::PageDown),
            KeyAction::Home => self.dir(Dir::Home),
            KeyAction::End => self.dir(Dir::End),
            KeyAction::Activate => self.activate(store, self.cursor),
            KeyAction::ActivateAlternate => self.activate_alternate(store, self.cursor),
            KeyAction::Submit => {
                let q = self.query.text().to_owned();
                self.complete_select(&q);
                Out::Close
            }
            KeyAction::Clear => {
                self.set_query(store, "");
                Out::None
            }
            KeyAction::MirrorNext => {
                self.mirror_cycle(store, 1);
                Out::None
            }
            KeyAction::Pop => self.pop(store),
            KeyAction::NextSection => self.section_step(true),
            KeyAction::PrevSection => self.section_step(false),
            KeyAction::Close => {
                self.close();
                Out::Close
            }
            _ => Out::None,
        };
        self.dirty = true;
        out
    }

    /// A key the field takes.
    fn edit(&mut self, store: &Store, edit: field::Key, mods: field::Modifiers, text: Option<&str>) -> Out {
        match self.query.key(edit, mods, text) {
            field::Outcome::Pass => return Out::None,
            field::Outcome::Held => {}
            field::Outcome::Edited => self.query_changed(store),
            field::Outcome::Copy(t) => self.ask(Request::Clipboard(clipboard::Cmd::SetText(t))),
            field::Outcome::Cut(t) => {
                self.ask(Request::Clipboard(clipboard::Cmd::SetText(t)));
                self.query_changed(store);
            }
            field::Outcome::Paste => self.ask(Request::Clipboard(clipboard::Cmd::Paste)),
        }
        self.dirty = true;
        Out::None
    }

    fn dir(&mut self, d: Dir) -> Out {
        self.move_cursor(d);
        Out::None
    }

    /// `debug query`: the live tree ranked, whether or not the window is up:
    /// inside the mode a leading prefix names, else the open launcher's.
    pub fn query(&self, store: &Store, q: &str) -> Value {
        let nodes = store.menu.nodes();
        let (level, q) = match model::prefixed(nodes, q) {
            Some((node, rest)) => (Some(node.id.as_str()), rest),
            None => (self.level.as_deref().filter(|_| self.open), q),
        };
        let noted = |rows: Vec<Node>| -> Value {
            Value::Array(rows.iter().map(|n| json!({"id": n.id, "label": n.label, "desc": n.desc.clone().unwrap_or_default(), "kind": n.kind.as_str()})).collect())
        };
        match level {
            Some("emoji") => {
                let Some(index) = &store.menu.emoji else {
                    self.ask(Request::Index(Ask::Emoji));
                    return json!([]);
                };
                return Value::Array(index.rows(q, true, &store.state.data.emoji_uses, crate::runtime::now_ms() as f64).iter().map(|n| json!({"id": n.id, "label": n.label, "icon": n.icon, "kind": n.kind.as_str()})).collect());
            }
            Some("keybinds") => {
                self.ask(Request::Index(Ask::Binds));
                let reply = store.menu.binds.as_ref().map(|r| r.as_deref().map_err(|_| ()));
                return noted(fs_menu::keybinds::rows_for(reply, q));
            }
            Some("radio.search") => return noted(index::radio_search_rows(store, q)),
            Some(fs_menu::providers::INSTALL_ROUTE) => {
                self.ask(Request::PackageSearch(q.to_owned()));
                return noted(crate::surfaces::packages::install_rows(store, q));
            }
            _ => {}
        }
        let snap = snapshot(store);
        let lookup = index::abbreviation_lookup(store, q, crate::runtime::now_ms() as f64);
        let boost = fs_menu::search::Boost { abbrev: Some(&lookup), picks: &store.state.data.picks, now: crate::runtime::now_ms() as f64 };
        let rows = fs_menu::search::rows_for_query(nodes, q, &store.menu.cond, level, boost).into_iter().map(|n| {
            if n.id == fs_menu::calc::RESULT_ID {
                return json!({"id": n.id, "label": n.label, "kind": n.kind.as_str()});
            }
            json!({
                "id": n.id, "label": n.label, "kind": n.kind.as_str(), "iconSource": n.icon_source,
                "checked": toggles::checked_for(Some(&n), Some(&snap), Some(&store.menu.checked))
            })
        });
        Value::Array(rows.collect())
    }
}

/// The compositor's windows an app's entry claims.
fn app_windows(store: &Store, entry: &fs_menu::node::DesktopEntry) -> Vec<WindowId> {
    crate::services::launch::match_windows(Some(entry), &store.compositor.snapshot.windows)
}





pub fn selection_path() -> std::path::PathBuf {
    state::state_path().with_file_name("menu-selection.txt")
}





#[cfg(test)]
mod grab_tests {
    use super::*;

    fn call(function: &str) -> IpcCall {
        IpcCall::new("screenshot", function)
    }

    #[test]
    fn a_stale_id_gets_nothing() {
        let mut g = Grab::default();
        let first = g.hold(call("full"));
        let second = g.hold(call("region"));
        assert_eq!(g.take(first), None);
        assert_eq!(g.take(second), Some(call("region")));
        assert_eq!(g.take(second), None);
    }

    #[test]
    fn a_cancelled_hold_is_never_released() {
        let mut g = Grab::default();
        let id = g.hold(call("full"));
        g.cancel();
        assert_eq!(g.take(id), None);
    }

    #[test]
    fn reopening_as_a_prompt_drops_the_held_grab() {
        for open in [|m: &mut Model| m.open_select("p", Vec::new(), "t"), |m: &mut Model| m.open_input("p", "t", false)] {
            let mut m = Model::default();
            let id = m.grab.hold(call("full"));
            m.after_unmap = Some(call("full"));
            open(&mut m);
            assert_eq!(m.grab.take(id), None);
            assert!(m.after_unmap.is_none());
        }
    }
}
