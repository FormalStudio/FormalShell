//! The Wayland side: the bar's layer surface (gone while a fullscreen window
//! covers its output), the frame's four exclusion zones, the cards hanging
//! off the bar (a panel, a tray item's menu), the tooltip, the scrim,
//! the pointer and the keyboard, and which owner a configure, a frame
//! callback or an input event belongs to.

/// The live bar's window, whichever head it is on.
macro_rules! live {
    ($s:expr) => {
        $s.bars[$crate::wayland::Live]
    };
}
pub(crate) use live;
pub(crate) use heads::Live;

mod atlas;
pub mod backdrop;
mod caffeinate;
pub mod capture;
mod console;
pub(crate) mod desktop_clock;
mod headset;
mod profiles;
mod heads;
mod hotcorners;
mod launcher;
pub mod lock;
mod osd;
mod plugin_overlay;
mod polkit;
mod region;
mod preview;
mod rims;
mod screensaver;
mod settle;
mod sidebar;
pub mod overview;
mod owner;
pub mod switcher;
mod toasts;
mod veil;
mod zen;

use std::time::Instant;

use calloop::LoopHandle;
use calloop::timer::{TimeoutAction, Timer};
use fs_chrome::bar::layout;
use fs_chrome::bar::panels::{Home, PanelId};
use fs_types::{Edge, Region as BarRegion};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState, Region};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface};
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::reexports::protocols::ext::idle_notify::v1::client::ext_idle_notifier_v1::ExtIdleNotifierV1;
use smithay_client_toolkit::reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{Shape, WpCursorShapeDeviceV1};
use smithay_client_toolkit::seat::pointer::cursor_shape::CursorShapeManager;
use smithay_client_toolkit::seat::keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers};
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure,
};
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{delegate_dispatch2, delegate_registry, registry_handlers};

use crate::chrome::Chrome;
use crate::ipc;
use crate::paint::Kit;
use crate::runtime::{Msg, Runtime};
use crate::scene::{IRect, NodeId};
use crate::services::{barpaint, devices, compositor, notifications, overnight, theme, tray, visualizer};
use crate::settings;
use crate::store::{Diff, Moved, Store, Topic};
use crate::surface::{Backend, PixelSurface, Pixels, Surface};
use crate::surfaces;
use crate::surfaces::bar::Bar;
use crate::surfaces::bar::cell::{Action, Button, TrayClick};
use crate::surfaces::bar::cells::tray as tray_cell;
use crate::surfaces::card::{Card, CardSpec, Join, Scrim, Sizing};
use crate::surfaces::card_window::CardWindow;
use crate::surfaces::modal::Modal;
use crate::surfaces::panel::{self, Name, Out, host::{Host, Opener, Place}, quick};
use crate::surfaces::tooltip;
use crate::surfaces::tray_menu::{Hit, Menu, Outcome};

pub(crate) fn edge_anchor(edge: Edge) -> Anchor {
    match edge {
        Edge::Top => Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
        Edge::Bottom => Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
        Edge::Left => Anchor::LEFT | Anchor::TOP | Anchor::BOTTOM,
        Edge::Right => Anchor::RIGHT | Anchor::TOP | Anchor::BOTTOM,
    }
}

/// A tray item's menu, hung off the strip or off the tray's own panel.
pub struct Popout {
    pub win: CardWindow,
    /// The item's dbusmenu (`traymenu`).
    menu: Option<Menu>,
}

impl Popout {
    /// Lays its content out at the card's content rect this frame.
    fn layout(&mut self, kit: &mut Kit) {
        let card = &mut self.win.card;
        let (rect, alpha) = card.content;
        let clip = card.content_clip();
        if let Some(menu) = &mut self.menu {
            menu.layout(kit, &mut card.scene, rect, alpha, clip, self.win.surface.scale());
        }
    }

    fn menu_hit(&self, x: f64, y: f64) -> Option<Hit> {
        self.menu.as_ref()?.hit(x.floor() as i32, y.floor() as i32)
    }
}

/// What a button went down on, kept until it comes up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Press {
    /// A bar slot by index.
    Slot(usize),
    /// A tray item's menu: its close button or a row.
    Menu(Hit),
    /// A surface that takes the whole gesture itself.
    Whole,
}

/// What holds a card on screen. `ALL`'s order is the bar's: the first card
/// joined to an edge opens that edge's gap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Panel,
    /// A handoff's outgoing panel.
    Outgoing,
    Launcher,
    Atlas,
    Polkit,
    Overlay,
    Headset,
    Osd,
    Menu,
    /// The power profile pill beside the sidebar's Settings tiles.
    Profiles,
}

impl Role {
    pub const ALL: [Role; 10] = [
        Role::Panel,
        Role::Outgoing,
        Role::Launcher,
        Role::Atlas,
        Role::Polkit,
        Role::Overlay,
        Role::Headset,
        Role::Osd,
        Role::Menu,
        Role::Profiles,
    ];

    /// The card's rim reads the reactive rims' field. The headset card and
    /// the profile pill keep their flat border.
    pub fn lit(self) -> bool {
        !matches!(self, Role::Headset | Role::Profiles)
    }

    /// The bar opens a gap in its line where the card joins it. A tray menu
    /// hung off the tray's panel buds off that card's edge, not the bar's
    /// line, and the profile pill hangs off the screen's edge.
    pub fn opens_gap(self) -> bool {
        !matches!(self, Role::Menu | Role::Profiles)
    }

    /// Drawn in the popover's look rather than the joined card's.
    fn popover(self) -> bool {
        matches!(self, Role::Headset | Role::Profiles)
    }
}

/// What holds a panel's card, for the effects it pushes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Holder {
    /// The panel host's own card.
    Host,
    /// The launcher's card (Settings, its pages, Information).
    Launcher,
}

/// Whose surface an input event landed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Bar,
    Panel,
    Menu,
    Outgoing,
    Tooltip,
    Toasts,
    Preview,
    Scrim,
    Osd,
    Headset,
    Profiles,
    Zone(usize),
    /// An output's wallpaper, or its bar while that is a spare.
    Head(usize, heads::Part),
    /// The launcher's card or one of its two scrims.
    Launcher(crate::surfaces::modal::Part),
    Switcher,
    Overview,
    Picker(usize),
    Atlas(crate::surfaces::modal::Part),
}

pub struct App {
    registry: RegistryState,
    outputs: OutputState,
    seats: SeatState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    pixels: Pixels,
    /// Bound once at version 2 where the compositor has it: its input idle
    /// (the lock's blank, settle) ignores inhibitors, which a playing video
    /// holds; caffeinate's idle takes either version.
    idle_notifier: Option<ExtIdleNotifierV1>,
    qh: QueueHandle<Self>,
    handle: Option<LoopHandle<'static, App>>,
    pub runtime: Option<Runtime>,
    pub store: Store,
    caffeinate: caffeinate::Caffeinate,
    settle: settle::Settle,
    console: console::Console,
    zen: zen::ZenState,
    saver: screensaver::Saver,
    pub lock: lock::Lock,
    hot: hotcorners::HotCorners,
    polkit: Option<polkit::Dialog>,
    plugin_overlay: Option<plugin_overlay::Window>,
    osd: osd::Osd,
    headset: headset::Headset,
    /// The power profile pill beside the Settings tiles (`profiles.rs`).
    profiles: Option<profiles::Pill>,
    thumbnails: capture::Capture,
    pub switcher: switcher::State,
    pub overview: overview::State,
    pub atlas: atlas::State,
    peek: preview::State,
    /// Every head's bar window, the live one among them.
    pub bars: heads::Bars,
    /// Shapes and draws for every surface; rebuilt with the theme.
    pub kit: Kit,
    /// The bar layer that holds the keyboard for the minimap, if one does.
    sidebar_keys: Option<wl_surface::WlSurface>,
    pub panel: Option<Host>,
    /// A handoff's outgoing card, until the incoming one starts moving.
    outgoing: Option<Host>,
    panel_dirty: bool,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    released: crate::keyboard::Released,
    tips: tooltip::Group,
    tip_card: Option<tooltip::Card>,
    toasts: Option<surfaces::toasts::Toasts>,
    toasts_dirty: bool,
    /// `column_cast` for the theme in force, dropped on a theme change.
    toast_cast: Option<f64>,
    /// `debug join`'s card, hung in the gap it opens.
    join_preview: Option<(Surface, crate::scene::Scene, Vec<NodeId>)>,
    /// The tray item menu, over whatever popout it hangs off.
    pub tray_menu: Option<Popout>,
    scrim: Option<(Scrim, PixelSurface)>,
    pointer: Option<wl_pointer::WlPointer>,
    /// The pointing hand over a cell that answers a click,
    /// where the compositor offers cursor shapes.
    cursor_shapes: Option<CursorShapeManager>,
    cursor_device: Option<WpCursorShapeDeviceV1>,
    cursor: Option<(u32, Shape)>,
    pointer_on: Option<Target>,
    pressed: Option<(Target, Press)>,
    /// Panel surfaces dropped since the pointer last entered one.
    dropped: Vec<wl_surface::WlSurface>,
    pub battery_watch: surfaces::battery::Watch,
    pub hdr: crate::services::display::Hdr,
    wake: Option<Instant>,
    rims: rims::Rims,
    /// `debug motionScale`: every duration multiplied by this.
    pub motion_scale: f64,
    /// The backdrop mark's word, by the style it was shaped in.
    shard_word: Option<(crate::text::TextStyle, crate::shards::Words)>,
    /// pantheon's cast under a joined card.
    pub cast: bool,
    /// `debug join`'s x and width on the bar's line, held under any card's.
    debug_join: Option<(i32, i32)>,
    pub exit: bool,
    started: Instant,
    pub launcher: crate::surfaces::launcher::Model,
    launch: Option<launcher::Window>,
    launcher_resolve: bool,
    /// The index moved while the launcher was shut: resolve its root once
    /// at the next idle frame, so the first open finds the text and rows
    /// already laid out.
    launcher_warm: bool,
    /// The styles the launcher's last layout shaped in, for warming its
    /// rows' words on the pool before the next open.
    launcher_styles: Vec<crate::text::TextStyle>,
    /// The launcher's faces are warmed once, after the bar is on screen.
    faces_warmed: bool,
    /// A modal card was up at the last present, which holds the backdrop's
    /// sculpture still.
    modal_up: bool,
    /// The launcher card's pool, buffers and canvas while it is shut.
    launch_kept: Option<crate::surface::Kept>,
    /// The same for the launcher's content layer.
    layer_kept: Option<crate::surface::Kept>,
    /// Bumped on each close, so only the last close's timer releases.
    kept_generation: u64,
    /// The launcher's conditions and binds were asked for once its tree
    /// first arrived, so the first open has results to draw on.
    fresh_asked: bool,
    /// The network the launcher's password step is for, and the identity
    /// an enterprise one was given first (WifiService.pendingSsid).
    wifi_pending: Option<(String, String)>,
    /// The Show password request still awaited, and the launcher session it
    /// was asked in; `wifi_reveals` numbers the requests.
    wifi_reveal: Option<(u64, u64)>,
    wifi_reveals: u64,
    mods: crate::ui::field::Modifiers,
    menu_buttons: Option<crate::services::menu::BaseInputs>,
    /// The split pane's last picture request, so a frame asks once.
    preview_asked: Option<(String, ((u32, u32), f64))>,
    /// The file or folder the preview pane last asked a peek of, this open.
    peek_asked: Option<String>,
    /// The Install row whose `fspec info` was last asked for.
    info_asked: Option<String>,
    menu_launches: Option<Vec<fs_menu::frecency::Record>>,
    pub capture: crate::surfaces::capture::Capture,
    pickers: region::Pickers,
    desktop_clock: desktop_clock::State,
    veil: veil::State,
}

impl App {
    /// `started` is the clock every logged `t=` counts from.
    pub fn new(globals: &GlobalList, qh: &QueueHandle<Self>, started: Instant, motion_scale: f64, prompter: Option<fs_auth::prompter::Prompter>) -> Self {
        let compositor = CompositorState::bind(globals, qh).expect("wl_compositor is not available");
        let layer_shell = LayerShell::bind(globals, qh).expect("zwlr_layer_shell_v1 is not available");
        let shm = Shm::bind(globals, qh).expect("wl_shm is not available");
        let pixels = Pixels::bind(globals, qh)
            .expect("wp_viewporter, wp_single_pixel_buffer_manager_v1 and wp_alpha_modifier_v1 are required");
        let store = Store::default();
        let bar = Bar::new(Edge::Top, &store.theme.theme, settings::layout(&store.config));
        let kit = Kit::new(&store.theme.theme);
        let mut app = Self {
            registry: RegistryState::new(globals),
            outputs: OutputState::new(globals, qh),
            seats: SeatState::new(globals, qh),
            compositor,
            layer_shell,
            shm,
            pixels,
            idle_notifier: globals.bind(qh, 1..=2, crate::surface::Ignore).ok(),
            qh: qh.clone(),
            handle: None,
            runtime: None,
            store,
            kit,
            caffeinate: caffeinate::Caffeinate::bind(globals, qh),
            settle: settle::Settle::default(),
            console: console::Console::default(),
            zen: zen::ZenState::default(),
            saver: screensaver::Saver::default(),
            lock: lock::Lock::bind(globals, qh, prompter),
            hot: Default::default(),
            polkit: None,
            plugin_overlay: None,
            osd: osd::Osd::default(),
            headset: headset::Headset::default(),
            profiles: None,
            thumbnails: capture::Capture::bind(globals, qh),
            switcher: switcher::State::default(),
            overview: overview::State::default(),
            atlas: atlas::State::default(),
            peek: preview::State::default(),
            bars: heads::Bars::new(bar),
            sidebar_keys: None,
            panel: None,
            tray_menu: None,
            outgoing: None,
            panel_dirty: false,
            keyboard: None,
            released: Default::default(),
            tips: tooltip::Group::default(),
            tip_card: None,
            toasts: None,
            toasts_dirty: false,
            toast_cast: None,
            join_preview: None,
            scrim: None,
            pointer: None,
            cursor_shapes: CursorShapeManager::bind(globals, qh).ok(),
            cursor_device: None,
            cursor: None,
            pointer_on: None,
            pressed: None,
            dropped: Vec::new(),
            battery_watch: Default::default(),
            wake: None,
            rims: Default::default(),
            motion_scale,
            shard_word: None,
            cast: false,
            debug_join: None,
            exit: false,
            started,
            launcher: Default::default(),
            launch: None,
            launcher_resolve: true,
            launcher_warm: false,
            launcher_styles: Vec::new(),
            faces_warmed: false,
            modal_up: false,
            launch_kept: None,
            layer_kept: None,
            kept_generation: 0,
            fresh_asked: false,
            wifi_pending: None,
            wifi_reveal: None,
            wifi_reveals: 0,
            mods: Default::default(),
            menu_buttons: None,
            preview_asked: None,
            peek_asked: None,
            info_asked: None,
            menu_launches: None,
            capture: Default::default(),
            hdr: Default::default(),
            pickers: Default::default(),
            desktop_clock: Default::default(),
            veil: Default::default(),
        };
        app.raise_login_lock();
        app.begin_arrival();
        app.sync_heads();
        app
    }

    pub fn set_handle(&mut self, handle: LoopHandle<'static, App>) {
        self.handle = Some(handle);
        self.start_lock();
        self.start_veil();
    }

    /// A layer surface that takes no input and reserves nothing, on the
    /// live bar's output.
    fn overlay(&self, namespace: &'static str, layer: Layer, anchor: Anchor, size: (u32, u32), zone: i32) -> LayerSurface {
        self.overlay_on(namespace, layer, anchor, size, zone, self.bar_wl())
    }

    fn overlay_on(&self, namespace: &'static str, layer: Layer, anchor: Anchor, size: (u32, u32), zone: i32, output: Option<&wl_output::WlOutput>) -> LayerSurface {
        let surface = self.compositor.create_surface(&self.qh);
        let ls = self.layer_shell.create_layer_surface(&self.qh, surface, layer, Some(namespace), output);
        ls.set_anchor(anchor);
        ls.set_size(size.0, size.1);
        ls.set_exclusive_zone(zone);
        ls.set_keyboard_interactivity(KeyboardInteractivity::None);
        if let Ok(region) = Region::new(&self.compositor) {
            ls.set_input_region(Some(region.wl_region()));
        }
        ls.commit();
        ls
    }

    /// The bar's faces built on a thread of their own while the compositor
    /// answers the bar's first configure: their hinting instances are most
    /// of the first frame's cost.
    pub fn warm_bar_faces(&self) {
        use crate::text::{Family, TextStyle};
        use fs_theme::tokens::WEIGHTS;
        let look = &self.kit.look;
        let mut jobs: Vec<(String, TextStyle)> = Vec::new();
        for w in [WEIGHTS.normal, WEIGHTS.medium, WEIGHTS.semibold] {
            jobs.push(("0".into(), look.label(w as f32)));
            jobs.push(("a".into(), look.sans(w as f32)));
            jobs.push(("0".into(), TextStyle { family: look.sans, size: look.caption, weight: w as f32, tracking: look.meta_tracking, wdth: TextStyle::NORMAL_WIDTH, wght: None }));
        }
        for name in std::iter::once("circle").chain(crate::surfaces::bar::cells::warm_icons()) {
            let g = fs_theme::icons::glyph(name);
            jobs.push((g.text.to_owned(), TextStyle { family: Family::Named(g.family), size: look.body, weight: 400.0, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None }));
        }
        let text = self.kit.text.clone();
        let _ = std::thread::Builder::new().name("fs-warm".into()).spawn(move || text.warm(&jobs));
    }

    fn log(&self, what: &str) {
        crate::trace::ipc(self.started, what);
    }

    /// While a focused fullscreen window covers the bar's output the bar,
    /// its frame zones and its cards leave the layers (`fullscreen.hideChrome`).
    pub(crate) fn sync_agents(&self) {
        crate::services::agents::shown(!live!(self).bar.hidden() && !self.saver.active && !self.lock.locked);
    }

    /// The live bar's chrome on its output. `fullscreen.hideChrome` false
    /// leaves no output covered.
    pub(crate) fn chrome(&self) -> Chrome {
        let hide = settings::HIDE_CHROME.get(&self.store.config);
        let covered = if hide { self.store.compositor.snapshot.fullscreen_outputs.clone() } else { Vec::new() };
        Chrome::new(&live!(self).bar, self.output_size(), self.store.theme.theme.frame_radius, covered)
    }

    /// A fullscreen window holds `output`, so what lies under it (the
    /// backdrop, the desktop clock, the windows a rim reflects) is out of
    /// sight whatever `fullscreen.hideChrome` keeps over it.
    pub(crate) fn fullscreen_on(&self, output: &str) -> bool {
        self.store.compositor.snapshot.fullscreen_outputs.iter().any(|o| o == output)
    }

    /// The bar's window and the frame's zones as the config and the
    /// fullscreen state want them: re-created rather than re-anchored, so a
    /// framed window and a strip never share a configure.
    fn place_chrome(&mut self) {
        let hidden = self.chrome().bar_covered();
        live!(self).bar.set_hidden(hidden);
        self.sync_agents();
        if hidden {
            if live!(self).surface.take().is_some() {
                self.log("bar unmapped under fullscreen");
            }
            live!(self).zones.clear();
            self.panel = None;
            self.drop_tray_menu();
            self.outgoing = None;
            self.tip_card = None;
            self.sync_join();
            return;
        }
        let Some(output) = self.bar_wl().cloned() else { return };
        let (surface, zones) = self.build_chrome(&live!(self).bar, &output);
        live!(self).mount(surface, zones);
    }

    /// A bar's window on `output` and, for a framed one, the four zones
    /// reserving the frame's edges.
    fn build_chrome(&self, bar: &Bar, output: &wl_output::WlOutput) -> (Surface, Vec<(Edge, PixelSurface)>) {
        let framed = bar.framed();
        let edge = bar.edge();
        let t = bar.thickness();
        let surface = self.compositor.create_surface(&self.qh);
        let layer = self.layer_shell.create_layer_surface(&self.qh, surface, Layer::Top, Some("formalshell:bar"), Some(output));
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        if framed {
            layer.set_anchor(Anchor::all());
            layer.set_size(0, 0);
            // The frame's zones reserve each edge; a window anchored to all
            // four has no one edge to reserve against.
            layer.set_exclusive_zone(-1);
        } else {
            let across = bar.across();
            let (w, h) = if edge.is_vertical() { (across, 0) } else { (0, across) };
            layer.set_anchor(edge_anchor(edge));
            layer.set_size(w as u32, h as u32);
            layer.set_exclusive_zone(bar.zone());
        }
        layer.commit();
        let mut surface = Surface::new("bar", Backend::Gpu, layer, &self.shm, self.started, self.store.display.scales.on(output));
        surface.presize(bar.scene.size.w, bar.scene.size.h);
        let mut zones = Vec::new();
        if framed {
            let ft = bar.frame_thickness() as i32;
            for e in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
                let zone = if e == edge { t } else { ft };
                let size = if e.is_vertical() { (1, 0) } else { (0, 1) };
                let ls = self.overlay_on("formalshell:frame-zone", Layer::Overlay, edge_anchor(e), size, zone, Some(output));
                let ps = PixelSurface::new("frame-zone", ls, 0.0, &self.pixels, &self.qh, self.started);
                zones.push((e, ps));
            }
        }
        (surface, zones)
    }

    fn set_input_region_for(compositor: &CompositorState, bar: &mut Bar, s: &Surface) -> bool {
        let Some(rects) = bar.input_rects() else {
            // A window that took a region and now takes input everywhere.
            if bar.input_sent.take().is_some() {
                s.layer.set_input_region(None);
                s.layer.commit();
                return true;
            }
            return false;
        };
        if !s.configured || bar.input_sent.as_ref() == Some(&rects) {
            return false;
        }
        if let Ok(region) = Region::new(compositor) {
            for r in &rects {
                region.add(r.x, r.y, r.w, r.h);
            }
            s.layer.set_input_region(Some(region.wl_region()));
            s.layer.commit();
        }
        bar.input_sent = Some(rects);
        true
    }

    /// The live bar's input region off what it last drew, and the open
    /// panel's hole for it following.
    fn sync_bar_input(&mut self) {
        let win = &mut live!(self);
        if win.surface.as_ref().is_some_and(|s| Self::set_input_region_for(&self.compositor, &mut win.bar, s)) {
            self.panel_dirty = true;
        }
    }

    /// What of the live output the bar takes the pointer over, in the
    /// output's own coordinates.
    fn bar_input(&self) -> Vec<IRect> {
        match live!(self).bar.input_rects().filter(|_| live!(self).bar.sidebar().is_some()) {
            Some(rects) => {
                let chrome = self.chrome();
                let (ox, oy) = chrome.origin(live!(self).bar.thickness());
                rects.into_iter().map(|r| IRect::new(r.x + ox, r.y + oy, r.w, r.h)).collect()
            }
            None => vec![self.chrome().strip()],
        }
    }

    /// The settings the bar is built from: its edge and its layout.
    /// Anything structural re-creates the window.
    fn apply_config(&mut self) {
        // A layout change leaves the theme as it was, so nothing else
        // restyles the strip for it.
        if live!(self).bar.chrome_layout() != settings::layout(&self.store.config) {
            self.set_bar_theme();
        }
        self.sync_hot_corners();
        let bar_cfg = self.store.config.get("bar").cloned();
        let resolved = layout::resolve(bar_cfg.as_ref(), &self.store.plugins.bar_plugins());
        visualizer::set_motion(settings::MOTION.get(&self.store.config));
        self.arm_caffeinate();
        self.arm_settle();
        let edge = self.chrome_edge();
        let moved = self.bars.apply(|b| b.set_edge(edge));
        let relaid = self.bars.apply(|b| b.set_layout(resolved.clone()));
        if moved || relaid {
            self.panel = None;
            self.drop_tray_menu();
            self.outgoing = None;
        }
        if moved {
            self.place_chrome();
            self.place_spares();
        }
        self.refresh_bar(None);
    }

    /// `bar.position`'s edge, a side one under `layout: sidebar`: top and
    /// bottom put the sidebar on the left.
    fn chrome_edge(&self) -> Edge {
        let edge = settings::bar_position(&self.store.config);
        match (settings::layout(&self.store.config), edge) {
            (settings::Layout::Sidebar, Edge::Top | Edge::Bottom) => Edge::Left,
            _ => edge,
        }
    }

    /// The bar repainted off the store's theme, re-placed when its framing
    /// or thickness moved with it.
    fn set_bar_theme(&mut self) {
        let before = (live!(self).bar.across(), live!(self).bar.framed(), live!(self).bar.frame_thickness(), live!(self).bar.edge());
        let edge = self.chrome_edge();
        let collapsed = self.store.state.data.sidebar_collapsed;
        self.kit.set_theme(&self.store.theme.theme);
        let layout = settings::layout(&self.store.config);
        self.bars.each(|b| {
            b.set_theme(&self.store.theme.theme, layout);
            b.set_edge(edge);
            b.set_collapsed(collapsed);
        });
        if (live!(self).bar.across(), live!(self).bar.framed(), live!(self).bar.frame_thickness(), live!(self).bar.edge()) != before {
            self.place_chrome();
            self.place_spares();
        }
        self.refresh_bar(None);
    }

    /// The GPU is up: every surface drawn so far draws once more, moving
    /// across to it on that present.
    pub fn gpu_up(&mut self) {
        for b in self.bars.heads.iter_mut().filter_map(|h| h.backdrop.as_mut()) {
            b.repaint(&self.qh);
        }
        live!(self).dirty = true;
        self.panel_dirty = true;
        self.toasts_dirty = true;
        self.launcher.dirty = true;
    }

    /// Every card window on screen across a theme change takes the new
    /// look, the tray menu's rows with it. What each card holds redraws off
    /// its owner's own `changed`.
    pub(super) fn restyle_cards(&mut self) {
        let now = Instant::now();
        let theme = self.store.theme.theme.clone();
        let popover = crate::surfaces::card::popover_theme(&theme);
        let cast = self.cast;
        for role in Role::ALL {
            let theme = if role.popover() { &popover } else { &theme };
            if let Some(w) = self.card_mut(role) {
                w.restyle(theme, cast, now);
            }
        }
        if let Some(m) = self.tray_menu.as_mut().and_then(|p| p.menu.as_mut()) {
            m.restyle(&theme);
        }
        self.resize_menu();
    }

    /// state.json's `sidebarCollapsed` on every bar, the windows re-placed
    /// at their new thickness.
    fn sync_collapsed(&mut self) {
        let collapsed = self.store.state.data.sidebar_collapsed;
        let moved = self.bars.apply(|b| b.set_collapsed(collapsed));
        if moved {
            self.place_chrome();
            self.place_spares();
        }
    }

    /// The cells reading `topic` read again, and everything the strip draws
    /// with them.
    fn refresh_bar(&mut self, topic: Option<Topic>) {
        let now = Instant::now();
        if matches!(topic, None | Some(Topic::State)) {
            self.sync_collapsed();
        }
        if matches!(topic, None | Some(Topic::Compositor) | Some(Topic::Config)) {
            if self.chrome().bar_covered() != live!(self).bar.hidden() {
                self.place_chrome();
            }
            self.sync_hot_corners();
        }
        live!(self).bar.update_paint(&mut self.kit, &self.store);
        let win = &mut live!(self);
        sync_zone(&mut win.bar, win.surface.as_mut());
        self.request_paint();
        live!(self).bar.read(&mut self.kit, &self.store, topic, now);
        // The card hanging off the strip lays out again with it; a topic
        // reaches the panel through its own reads.
        if topic.is_none() {
            self.panel_dirty |= self.panel.is_some();
        }
        self.sync_bar_open(now);
        self.refresh_spares(topic, now);
        self.update_backdrops();
        if matches!(topic, None | Some(Topic::Compositor | Topic::Wallpaper | Topic::Theme | Topic::Config | Topic::Display | Topic::State)) {
            self.sync_rims();
        }
        live!(self).dirty = true;
    }

    /// The main output's wallpaper reading, for the wingpanel band.
    fn request_paint(&self) {
        if !live!(self).bar.reads_wallpaper() {
            return;
        }
        let size = self.output_size();
        barpaint::sample(barpaint::Request {
            wallpaper: self.store.desktop(),
            edge: live!(self).bar.edge(),
            thickness: live!(self).bar.thickness() as f64,
            screen: size,
            output: live!(self).bar.output.clone(),
        });
    }

    /// The live bar's output, which every card hanging off it is sized to.
    fn output_size(&self) -> (f64, f64) {
        self.bar_wl()
            .and_then(|o| self.outputs.info(o))
            .and_then(|i| i.logical_size.or_else(|| i.modes.iter().find(|m| m.current).map(|m| m.dimensions)))
            .map_or((0.0, 0.0), |(w, h)| (w as f64, h as f64))
    }

    /// The output the live bar is on.
    fn bar_wl(&self) -> Option<&wl_output::WlOutput> {
        self.bars.heads.get(self.bars.live).map(|h| &h.output)
    }

    /// The output the live bar's cells answer for.
    pub fn bar_output_name(&self) -> String {
        self.bar_wl().map(|o| self.output_name(o)).unwrap_or_default()
    }

    /// An IPC verb for the open panel `id`; `None` when it is not up or
    /// answers none.
    pub fn panel_call(&mut self, id: PanelId, verb: &str, arg: &str) -> Option<Result<serde_json::Value, String>> {
        let reply = self.panel.as_mut().filter(|p| p.is_open() && p.panel() == Some(id))?.module.call(verb, arg);
        self.panel_dirty = true;
        reply
    }

    /// The open panel; the gallery, the centre and a peek are no panel's.
    pub fn panel_open(&self) -> Option<Name> {
        self.panel
            .as_ref()
            .filter(|p| p.is_open())
            .and_then(|p| match p.panel() {
                Some(id) => Some(Name::Panel(id)),
                None => p.id().starts_with(panel::plugin::PREFIX).then_some(Name::Plugin(p.id())),
            })
            .or_else(|| self.launcher_panel().map(Name::Panel))
    }

    /// Whether `id` is the open panel.
    pub fn shows(&self, id: PanelId) -> bool {
        self.panel_open() == Some(Name::Panel(id))
    }

    pub fn gallery_open(&self) -> bool {
        self.panel.as_ref().is_some_and(|p| p.is_open() && p.id() == "gallery")
    }

    /// The region whose chevron opened the quick settings card, while it
    /// is open; the sidebar's chevron button stands for the right region's.
    pub fn overflow_open(&self) -> Option<BarRegion> {
        match self.quick_from()? {
            quick::Origin::Chevron(r) => Some(r),
            quick::Origin::Sidebar => Some(BarRegion::Right),
            quick::Origin::Ipc => None,
        }
    }

    /// What opened the quick settings card, while it is open.
    fn quick_from(&self) -> Option<quick::Origin> {
        self.panel.as_ref().filter(|p| p.is_open() && p.panel() == Some(PanelId::QuickSettings)).map(|p| p.from)
    }

    /// Marks the cell whose card is open.
    fn sync_bar_open(&mut self, now: Instant) {
        let panel = self.panel_open().and_then(Name::panel);
        let overflow = self.overflow_open();
        live!(self).bar.set_open(&self.kit, panel, overflow, now);
    }

    /// The modal card `role` names, while it is on screen.
    fn modal(&self, role: Role) -> Option<&Modal> {
        match role {
            Role::Launcher => self.launch.as_ref().map(|w| &w.shown.modal),
            Role::Atlas => self.atlas.win.as_ref().map(|w| &w.shown.modal),
            Role::Polkit => self.polkit.as_ref().map(|d| &d.modal),
            Role::Overlay => self.plugin_overlay.as_ref().map(|w| &w.modal),
            _ => None,
        }
    }

    fn modal_mut(&mut self, role: Role) -> Option<&mut Modal> {
        match role {
            Role::Launcher => self.launch.as_mut().map(|w| &mut w.shown.modal),
            Role::Atlas => self.atlas.win.as_mut().map(|w| &mut w.shown.modal),
            Role::Polkit => self.polkit.as_mut().map(|d| &mut d.modal),
            Role::Overlay => self.plugin_overlay.as_mut().map(|w| &mut w.modal),
            _ => None,
        }
    }

    /// The card `role` names, while it is on screen: opening, open or
    /// closing.
    pub fn card(&self, role: Role) -> Option<&CardWindow> {
        match role {
            Role::Panel => self.panel.as_ref().map(|h| &h.win),
            Role::Outgoing => self.outgoing.as_ref().map(|h| &h.win),
            Role::Headset => self.headset.card.as_ref().map(|c| &c.popup.win),
            Role::Osd => self.osd.pill.as_ref().map(|p| &p.win),
            Role::Menu => self.tray_menu.as_ref().map(|p| &p.win),
            Role::Profiles => self.profiles.as_ref().map(|p| &p.popup.win),
            Role::Launcher | Role::Atlas | Role::Polkit | Role::Overlay => self.modal(role).map(|m| &m.win),
        }
    }

    fn card_mut(&mut self, role: Role) -> Option<&mut CardWindow> {
        match role {
            Role::Panel => self.panel.as_mut().map(|h| &mut h.win),
            Role::Outgoing => self.outgoing.as_mut().map(|h| &mut h.win),
            Role::Headset => self.headset.card.as_mut().map(|c| &mut c.popup.win),
            Role::Osd => self.osd.pill.as_mut().map(|p| &mut p.win),
            Role::Menu => self.tray_menu.as_mut().map(|p| &mut p.win),
            Role::Profiles => self.profiles.as_mut().map(|p| &mut p.popup.win),
            Role::Launcher | Role::Atlas | Role::Polkit | Role::Overlay => self.modal_mut(role).map(|m| &mut m.win),
        }
    }

    /// Every card on screen, in `Role::ALL`'s order.
    pub fn cards(&self) -> impl Iterator<Item = (Role, &CardWindow)> {
        Role::ALL.into_iter().filter_map(|r| self.card(r).map(|w| (r, w)))
    }

    /// Every modal card on screen.
    pub fn modals(&self) -> impl Iterator<Item = &Modal> {
        Role::ALL.into_iter().filter_map(|r| self.modal(r))
    }

    /// Any modal card on screen, opening, open or closing.
    pub fn modal_shown(&self) -> bool {
        self.modals().next().is_some()
    }

    pub fn debug_join(&self) -> Option<(i32, i32)> {
        self.debug_join
    }

    pub fn set_debug_join(&mut self, join: Option<(i32, i32)>) {
        self.debug_join = join;
        self.sync_join();
        if join.is_none() {
            self.join_preview = None;
            return;
        }
        if self.join_preview.is_none() {
            let (w, h) = self.output_size();
            let layer = self.overlay("formalshell:debug-join", Layer::Overlay, Anchor::all(), (0, 0), -1);
            let surface = Surface::new("debug-join", Backend::Gpu, layer, &self.shm, self.started, self.store.display.scales.primary());
            self.join_preview = Some((surface, crate::scene::Scene::clear(w as i32, h as i32), Vec::new()));
        }
        let chrome = self.chrome();
        let (edge, line) = (chrome.edge, chrome.line());
        if let Some((_, scene, nodes)) = &mut self.join_preview {
            let join = join.map(|(x, w)| (x as f64, w as f64));
            draw_join_preview(scene, nodes, &self.store.theme.theme, edge, line, join);
        }
    }

    /// A card's own join wins over `debug join`'s while the card is up.
    fn sync_join(&mut self) {
        let mut joins = self.joins();
        let edge = live!(self).bar.edge();
        if !joins.iter().any(|j| j.edge == edge)
            && let Some((x, width)) = self.debug_join
        {
            joins.push(Join { edge, x: x as f64, width: width as f64, reach: self.store.theme.theme.card_radius() });
        }
        // One gap per edge: the strip's own goes to the first card on it.
        let mut seen = Vec::new();
        joins.retain(|j| if seen.contains(&j.edge) { false } else { seen.push(j.edge); true });
        live!(self).bar.set_joins(&joins);
        live!(self).dirty = true;
    }

    /// Every card joined to the live bar's lines.
    fn joins(&self) -> Vec<Join> {
        self.cards().filter(|(r, _)| r.opens_gap()).flat_map(|(_, w)| w.card.joins.iter().copied()).collect()
    }

    /// What a modal card takes to make its surfaces on the live bar's output.
    fn modal_make(&self) -> crate::surfaces::modal::Make<'_> {
        let scales = &self.store.display.scales;
        crate::surfaces::modal::Make {
            compositor: &self.compositor,
            layer_shell: &self.layer_shell,
            pixels: &self.pixels,
            shm: &self.shm,
            qh: &self.qh,
            output: self.bar_wl(),
            scale: self.bar_wl().map_or_else(|| scales.primary(), |o| scales.on(o)),
            started: self.started,
            theme: &self.store.theme.theme,
            chrome: self.chrome(),
            motion_scale: self.motion_scale,
            cast: self.cast,
        }
    }

    /// A card's layer surface on the bar's edge, taking input over its
    /// resting rect.
    fn popout_surface(&self, card: &Card, layer: Layer) -> Surface {
        let edge = card.edge;
        let depth = card.depth() as u32;
        let size = if edge.is_vertical() { (depth, 0) } else { (0, depth) };
        let layer = self.overlay("formalshell:panel", layer, edge_anchor(edge), size, -1);
        if let Ok(region) = Region::new(&self.compositor) {
            let r = card.rest_rect();
            region.add(r.x, r.y, r.w, r.h);
            layer.set_input_region(Some(region.wl_region()));
            layer.commit();
        }
        let mut surface = Surface::new("panel", Backend::Gpu, layer, &self.shm, self.started, self.store.display.scales.primary());
        surface.wait_map = true;
        surface
    }


    /// A panel by name, hung at `anchor` along the line (its own cell's
    /// centre) or at the line's end. A panel opened over another hands the
    /// card over.
    pub fn set_panel(&mut self, name: impl Into<Name>, open: bool, anchor: Option<f64>) {
        self.set_panel_from(name.into(), open, anchor, quick::Origin::Ipc);
    }

    /// Where the control `from` names sits along the line.
    fn opener_anchor(&self, from: quick::Origin) -> Option<f64> {
        match from {
            quick::Origin::Chevron(region) => live!(self).bar.chevron_anchor(region),
            quick::Origin::Sidebar => live!(self).bar.sidebar_anchor("overflow"),
            quick::Origin::Ipc => None,
        }
    }

    /// The quick settings card from `from`, under its chevron or the
    /// sidebar's button, or shut. A chevron's click closes the card
    /// whichever chevron opened it.
    pub fn set_quick(&mut self, from: quick::Origin, open: bool) {
        if !open {
            self.set_panel(PanelId::QuickSettings, false, None);
            return;
        }
        // Open from elsewhere, the card is made again for this opener's
        // extras and place.
        if self.quick_from().is_some_and(|f| f != from) {
            self.close_panels();
        }
        let anchor = self.opener_anchor(from);
        self.set_panel_from(Name::Panel(PanelId::QuickSettings), true, anchor, from);
    }

    fn set_panel_from(&mut self, name: Name, open: bool, anchor: Option<f64>, from: quick::Origin) {
        if let Name::Panel(id) = name {
            match id.home() {
                Home::Card => {}
                Home::Launcher => {
                    if open {
                        self.menu_panel(id);
                    } else if self.launcher_panel() == Some(id) {
                        self.menu_close();
                    }
                    return;
                }
                Home::Modal => {
                    if open {
                        self.atlas_show();
                    } else {
                        self.atlas_close();
                    }
                    return;
                }
            }
        }
        let now = Instant::now();
        // A menu hangs off the popout under it and goes with it, or with
        // whatever replaces it.
        let kept = open && self.panel_open() == Some(name);
        if !kept {
            self.close_tray_menu();
        }
        if !open {
            if self.panel_open() == Some(name) {
                self.close_panels();
            }
            return;
        }
        if kept {
            return;
        }
        self.atlas_close();
        self.overlay_close();
        let module: Option<Box<dyn panel::Panel>> = match name {
            Name::Panel(id) => {
                let bar = &live!(self).bar;
                panel::build(id, panel::Args { tiles: bar.quick_extras(from.region()), edge: bar.edge() })
            }
            Name::Plugin(key) => Some(Box::new(panel::plugin::PluginPanel::new(key))),
        };
        let Some(module) = module else { return };
        self.open_host(module, name.as_str(), from, anchor, now);
    }

    pub fn set_gallery(&mut self, open: bool) {
        let now = Instant::now();
        if !open {
            if self.gallery_open() {
                self.close_panels();
            }
            return;
        }
        if !self.gallery_open() {
            self.open_host(Box::new(panel::gallery::Gallery::new()), "gallery", quick::Origin::Ipc, None, now);
        }
    }

    /// `module` in the panel host as `name`, opened by `from`.
    fn open_host(&mut self, module: Box<dyn panel::Panel>, name: &'static str, from: quick::Origin, anchor: Option<f64>, now: Instant) {
        let place = self.chrome().place(anchor);
        self.open_host_at(module, name, from, place, now);
    }

    fn open_host_at(&mut self, module: Box<dyn panel::Panel>, id: &'static str, from: quick::Origin, place: Place, now: Instant) {
        if self.panel.as_ref().is_some_and(|p| p.id() == id && p.is_open()) {
            return;
        }
        let layer = self.overlay("formalshell:panel", Layer::Overlay, Anchor::all(), (0, 0), -1);
        let keyboard = if module.takes_keyboard() { KeyboardInteractivity::Exclusive } else { KeyboardInteractivity::None };
        layer.set_keyboard_interactivity(keyboard);
        layer.commit();
        let surface = Surface::new("panel", Backend::Gpu, layer, &self.shm, self.started, self.store.display.scales.primary());
        let mut host = Host::new(module, id, place, &self.store.theme.theme, surface, self.motion_scale, self.cast);
        host.from = from;
        host.start(&self.store, self.runtime.as_ref());
        match self.panel.take() {
            Some(mut old) if old.is_open() => {
                host.take_over(old.win.card.live());
                old.hand_over();
                self.outgoing = Some(old);
            }
            Some(old) => self.outgoing = Some(old),
            None => {}
        }
        host.win.card.set_open(now, true);
        self.panel = Some(host);
        self.panel_dirty = true;
        self.tips.hide(None, now);
        self.sync_bar_open(now);
        self.log(&format!("panel {id} mapped"));
    }

    /// The input region over a popout's resting rect.
    fn set_popout_input(compositor: &CompositorState, p: &mut Popout) {
        // A menu takes input everywhere on its surface: a click off the
        // card dismisses it.
        let size = p.win.card.scene.size;
        let r = if p.menu.is_some() { IRect::new(0, 0, size.w, size.h) } else { p.win.card.rest_rect() };
        p.win.sync_region(compositor, r, &[]);
    }

    /// A tray item's menu, hung off `anchor` along the line (its icon's
    /// centre) or at the strip's end, and off the tray's panel when
    /// `over_panel`, which it then stands clear of.
    fn open_tray_menu(&mut self, key: &str, anchor: Option<f64>, over_panel: bool) -> Result<(), String> {
        let Some(item) = self.store.tray.by_key(key).cloned() else { return Ok(()) };
        if !item.has_menu {
            return Err(format!("tray item '{}' has no menu", item.id));
        }
        let now = Instant::now();
        self.drop_tray_menu();
        self.tips.hide(None, now);
        let owner_edge = self.panel.as_ref().filter(|h| over_panel && h.is_open()).map(|h| h.win.card.rest().y1);
        if owner_edge.is_none() {
            self.close_panels();
        }
        let chrome = self.chrome();
        let theme = &self.store.theme.theme;
        let edge = chrome.edge;
        let vertical = edge.is_vertical();
        let line_at = owner_edge.unwrap_or(chrome.line());
        let (_, output_height) = self.output_size();
        let pad = theme.space.screen_padding;
        let cap = if vertical { output_height - pad * 2.0 } else { output_height - line_at - theme.space.bar_margin - pad };
        let mut menu = Menu::new(theme, item.key.clone(), item.tooltip.clone());
        menu.cap = cap.max(menu.width());
        let width = menu.width();
        let oriented = |h: f64| if vertical { (h, width) } else { (width, h) };
        let anchor = anchor.unwrap_or(live!(self).bar.length() as f64);
        menu.anchor = anchor;
        let size = Sizing::Card { size: oriented(menu.cap), anchor, length: live!(self).bar.length() };
        let mut card = Card::new(theme, CardSpec { role: fs_theme::style::Role::Menu, ..CardSpec::new(edge, line_at, size) }, self.motion_scale, self.cast);
        card.resize(now, anchor, live!(self).bar.length(), pad, oriented(menu.height().min(menu.cap)));
        menu.morph.jump(menu.height().min(menu.cap));
        let surface = self.popout_surface(&card, Layer::Overlay);
        let mut popout = Popout { win: CardWindow::new(card, surface), menu: Some(menu) };
        Self::set_popout_input(&self.compositor, &mut popout);
        self.tray_menu = Some(popout);
        let (key, largest) = (item.key, self.store.display.scales.largest());
        if let Some(rt) = &self.runtime {
            rt.service(move |ctx| tray::open_menu(ctx, key, largest));
        }
        self.log("tray menu mapped");
        Ok(())
    }

    /// Closes the menu through its exit, telling the item.
    pub fn close_tray_menu(&mut self) {
        let now = Instant::now();
        let Some(p) = self.tray_menu.as_mut().filter(|p| p.win.card.is_open()) else { return };
        p.win.card.set_open(now, false);
        let key = p.menu.as_ref().map(|m| m.key.clone());
        if let (Some(key), Some(rt)) = (key, &self.runtime) {
            rt.service(move |ctx| tray::close_menu(ctx, key));
        }
    }

    /// Takes the menu off screen at once, telling the item if it was up.
    fn drop_tray_menu(&mut self) {
        self.close_tray_menu();
        self.tray_menu = None;
    }

    /// The menu's card at the height its rows ask for, capped to the room.
    fn resize_menu(&mut self) {
        let now = Instant::now();
        let vertical = live!(self).bar.edge().is_vertical();
        let length = live!(self).bar.length();
        let pad = self.store.theme.theme.space.screen_padding;
        if let Some(p) = &mut self.tray_menu {
            let mapped = p.win.card.is_open() && p.win.surface.mapped();
            if let Some(m) = &mut p.menu {
                let (w, h) = (m.width(), m.height().min(m.cap));
                // The first size lands at once; every later one morphs.
                if mapped && m.morph.target() > 0.0 {
                    let ms = crate::motion::Kind::Spatial.ms(&self.store.theme.theme) * self.motion_scale;
                    m.morph.set(now, h, ms);
                } else {
                    m.morph.jump(h);
                }
                let hv = m.morph.value(now);
                let anchor = m.anchor;
                p.win.card.resize(now, anchor, length, pad, if vertical { (hv, w) } else { (w, hv) });
            }
            Self::set_popout_input(&self.compositor, p);
        }
    }

    /// The menu's card on its way to the height its rows ask for, a frame at a time.
    fn step_menu(&mut self, now: Instant) {
        let vertical = live!(self).bar.edge().is_vertical();
        let length = live!(self).bar.length();
        let pad = self.store.theme.theme.space.screen_padding;
        let Some(p) = &mut self.tray_menu else { return };
        let Some(m) = &p.menu else { return };
        if !m.morph.running(now) {
            return;
        }
        let (w, h) = (m.width(), m.morph.value(now));
        p.win.card.resize(now, m.anchor, length, pad, if vertical { (h, w) } else { (w, h) });
    }

    /// The tray changed: a menu waiting on its tree gets it, one whose item
    /// went away shuts.
    fn tray_changed(&mut self) {
        let Some(p) = &mut self.tray_menu else { return };
        let Some(menu) = &mut p.menu else { return };
        if self.store.tray.by_key(&menu.key).is_none() {
            self.close_tray_menu();
            return;
        }
        if let Some(tree) = self.store.tray.menu.as_ref().filter(|m| m.key == menu.key) {
            menu.set_tree(Some(&tree.root));
        }
        self.resize_menu();
    }

    fn menu_outcome(&mut self, outcome: Outcome) {
        let Some(key) = self.tray_menu.as_ref().and_then(|p| p.menu.as_ref()).map(|m| m.key.clone()) else { return };
        match outcome {
            Outcome::None => {}
            Outcome::Click(id) => {
                if let Some(rt) = &self.runtime {
                    rt.service(move |ctx| tray::menu_click(ctx, key, id));
                }
                self.close_tray_menu();
            }
            Outcome::Toggled(id, opened) => {
                if let (true, Some(rt)) = (opened, &self.runtime) {
                    let largest = self.store.display.scales.largest();
                    rt.service(move |ctx| tray::menu_about_to_show(ctx, key, id, largest));
                }
                self.resize_menu();
            }
        }
    }

    fn open_menu_mut(&mut self) -> Option<&mut Menu> {
        self.tray_menu.as_mut().filter(|p| p.win.card.is_open()).and_then(|p| p.menu.as_mut())
    }

    /// `tray activate`: the item's own Activate, as its cell's click does.
    pub fn tray_activate(&self, id: &str) -> Result<(), String> {
        let Some(item) = self.store.tray.by_id(id) else { return Err(format!("no tray item with id '{id}'")) };
        let key = item.key.clone();
        if let Some(rt) = &self.runtime {
            rt.service(move |ctx| tray::activate(ctx, key));
        }
        Ok(())
    }

    /// `tray menu`: opens over the second bar while that is up, as a right
    /// click on its cell does.
    pub fn tray_menu(&mut self, id: &str) -> Result<(), String> {
        let Some(key) = self.store.tray.by_id(id).map(|i| i.key.clone()) else {
            return Err(format!("no tray item with id '{id}'"));
        };
        let over = self.shows(PanelId::TrayOverflow);
        self.open_tray_menu(&key, None, over)
    }

    pub fn tray_menu_cursor(&mut self, delta: i32) -> Result<(), String> {
        let m = self.open_menu_mut().ok_or("no tray menu open")?;
        m.move_cursor(delta);
        Ok(())
    }

    pub fn tray_menu_activate(&mut self) -> Result<(), String> {
        let m = self.open_menu_mut().ok_or("no tray menu open")?;
        let outcome = m.activate_cursor();
        self.menu_outcome(outcome);
        Ok(())
    }

    /// `tray status`'s `overflow` block: whether the second bar is up and how
    /// many icons the strip kept.
    pub fn tray_inline(&self) -> usize {
        tray_cell::inline_count().min(self.store.tray.items.len())
    }

    pub fn close_panels(&mut self) {
        self.close_tray_menu();
        let now = Instant::now();
        if let Some(p) = &mut self.panel {
            p.close(now);
        }
        self.panel_dirty = true;
        self.tips.hide(None, now);
        self.sync_bar_open(now);
    }

    pub fn set_scrim(&mut self, now: Instant, open: bool) {
        if let Some((scrim, _)) = &mut self.scrim {
            scrim.set_open(now, open);
            return;
        }
        if !open {
            return;
        }
        let scrim = Scrim::new(&self.store.theme.theme, self.motion_scale);
        let layer = self.overlay("formalshell:scrim", Layer::Top, Anchor::all(), (0, 0), -1);
        let surface = PixelSurface::new("scrim", layer, scrim.tone(), &self.pixels, &self.qh, self.started);
        self.scrim = Some((scrim, surface));
    }

    pub fn set_spinner(&mut self, on: bool) {
        live!(self).bar.spinner(on, Instant::now());
        live!(self).dirty = true;
    }

    pub fn receive(&mut self, msg: Msg) {
        match msg {
            Msg::Diff(diff) => {
                let t0 = Instant::now();
                let topic = self.apply(diff);
                crate::trace::slow("receive", t0.elapsed(), format_args!("diff {topic:?} us={}", t0.elapsed().as_micros()));
            }
            Msg::Lock(event) => self.logind_event(event),
            Msg::Polkit(event) => self.polkit_event(event),
        }
    }

    /// The one write into the store: the diff, the slice it implies, then
    /// what the change asks of services and surfaces. Answers the diff's own
    /// topic, `None` when it changed nothing.
    pub fn apply(&mut self, diff: Diff) -> Option<Topic> {
        let moved = self.store.apply(diff);
        self.moved(moved);
        moved.topic
    }

    /// The reactions to a store change. The implied slice goes first, so
    /// the cause's surfaces find it already redrawn.
    fn moved(&mut self, moved: Moved) {
        for topic in [moved.implied, moved.topic].into_iter().flatten() {
            if let Some(diff) = crate::services::react(&self.store, topic) {
                self.apply(diff);
            }
            surfaces::changed(self, topic);
        }
    }

    /// A store change to every window owner reading its topic.
    pub fn owners_changed(&mut self, topic: Topic) {
        owner::changed(self, topic);
    }

    /// One IPC call, answered here on the UI loop.
    pub fn call(&mut self, request: &ipc::Request) -> String {
        let t0 = Instant::now();
        self.log(&format!("{request:?}"));
        let reply = ipc::dispatch(self, request);
        crate::trace::slow("call", t0.elapsed(), format_args!("{request:?} us={}", t0.elapsed().as_micros()));
        reply
    }

    /// What an input on a cell asks for. `anchor` is that cell's centre
    /// along the line.
    fn act(&mut self, action: Action, anchor: f64) {
        match action {
            Action::None => {}
            Action::Panel(id) => {
                let open = !self.shows(id);
                self.set_panel(id, open, Some(anchor));
            }
            Action::Overflow(region) => {
                let open = !self.shows(PanelId::QuickSettings);
                let from = if live!(self).bar.sidebar().is_some() { quick::Origin::Sidebar } else { quick::Origin::Chevron(region) };
                self.set_panel_from(Name::Panel(PanelId::QuickSettings), open, Some(anchor), from);
            }
            Action::Workspace(id) => compositor::focus_workspace(&id),
            Action::WorkspaceAt(idx) => compositor::focus_workspace_at(idx),
            Action::Window(id) => compositor::focus_window(&id),
            Action::Volume(v) => devices::audio::send(devices::audio::Cmd::Volume(devices::audio::Target::Sink, v)),
            Action::ToggleMute => devices::audio::send(devices::audio::Cmd::ToggleMute(devices::audio::Target::Sink)),
            Action::ToggleSourceMute => devices::audio::send(devices::audio::Cmd::ToggleMute(devices::audio::Target::Source)),
            Action::ToggleWifi => devices::network::send(devices::network::Cmd::ToggleWifi),
            Action::ToggleBluetooth => devices::bluetooth::send(devices::bluetooth::Cmd::TogglePower),
            Action::BatteryPercent(on) => crate::surfaces::bar::cells::battery::show_percent(on),
            Action::Tray { key, click: TrayClick::Menu, offset } => {
                let _ = self.open_tray_menu(&key, Some(anchor + offset), false);
            }
            Action::Tray { key, click, .. } => {
                if self.store.tray.by_key(&key).is_none() {
                    return;
                }
                if let Some(rt) = &self.runtime {
                    rt.service(move |ctx| match click {
                        TrayClick::Secondary => tray::secondary_activate(ctx, key),
                        _ => tray::activate(ctx, key),
                    });
                }
            }
            Action::Caffeinate(on) => self.set_caffeinated(on),
            Action::OvernightOff => {
                if let (Some(rt), Some(record)) = (&self.runtime, self.store.state.data.overnight.clone()) {
                    rt.service(move |ctx| overnight::disable(ctx, record));
                }
            }
            Action::RecordStop => {
                let _ = surfaces::capture::record::stop(self);
            }
            Action::ReminderSummary => {
                self.apply(Diff::Notifications(notifications::Diff::Op(notifications::Op::ShowReminders)));
            }
            Action::MediaNext => self.store.media.next(),
            Action::MediaPrevious => self.store.media.previous(),
            Action::Center => {
                let open = !self.store.notifications.center_open;
                self.set_center(open, Some(anchor));
            }
            Action::Dnd(on) => {
                self.apply(Diff::Notifications(notifications::Diff::Op(notifications::Op::SetDnd(on))));
            }
            Action::Launcher => self.menu_toggle(),
        }
    }

    pub fn present(&mut self) {
        if self.lock_splash_pending() {
            self.present_lock();
            return;
        }
        self.released.sweep();
        self.preview_present();
        crate::surfaces::capture::flush(self);
        let now = Instant::now();
        self.tick_notifications();
        for o in owner::LIFECYCLE {
            o.finished(self, now);
        }
        if self.modal_up != self.modal_shown() {
            self.modal_up = !self.modal_up;
            self.sync_shards();
        }
        self.follow_focus();
        self.light_cards(now);
        self.sync_rasters(now);
        let idle = self.saver.active || self.lock.locked;
        self.bars.each(|b| b.set_idle(idle));
        for o in owner::LIFECYCLE {
            o.present(self, now);
        }
        self.arm_wake(now);
    }

    /// Every bar's cells made at its own surface's scale: a bar whose
    /// surface moved to another scale reads them all again.
    fn sync_rasters(&mut self, now: Instant) {
        for win in self.bars.wins_mut() {
            let Some(scale) = win.surface.as_ref().map(|s| s.scale()).filter(|k| *k != win.bar.raster) else { continue };
            win.bar.raster = scale;
            win.bar.read(&mut self.kit, &self.store, None, now);
            win.dirty = true;
        }
    }

    /// The tooltip group's answer on screen: a card created for a show,
    /// travelling for a hand-off, and gone once its exit has run.
    fn present_tooltip(&mut self, now: Instant) {
        self.tips.tick(now);
        self.tips.drawn = self.tip_card.is_some();
        let theme = &self.store.theme.theme;
        match self.tips.shown.clone() {
            Some(ask) => {
                if self.tip_card.is_none() {
                    let size = self.output_size();
                    let layer = self.overlay("formalshell:tooltip", Layer::Overlay, Anchor::all(), (0, 0), -1);
                    let mut surface = Surface::new("tooltip", Backend::Gpu, layer, &self.shm, self.started, self.store.display.scales.primary());
                    surface.wait_map = true;
                    self.tip_card = Some(tooltip::Card::new(surface, (size.0 as i32, size.1 as i32), self.motion_scale));
                }
                let travel = self.tips.travel;
                let card = self.tip_card.as_mut().expect("tooltip card");
                card.take(theme, &mut self.kit, &ask, travel, now);
                card.set_open(theme, true, now);
            }
            None => {
                if let Some(c) = &mut self.tip_card {
                    c.set_open(theme, false, now);
                }
            }
        }
        if self.tip_card.as_ref().is_some_and(|c| c.surface.mapped() && c.finished(now)) {
            self.tip_card = None;
        }
        let qh = self.qh.clone();
        if let Some(c) = &mut self.tip_card {
            c.draw(theme, &mut self.kit, now);
            let animating = c.animating(now);
            c.surface.present(&mut c.scene, animating, &qh);
        }
    }

    /// A timer for the next thing that starts on its own (a marquee
    /// leaving its hold, a tooltip's delay, a panel's keyboard prime), so
    /// nothing asks for frames while it waits.
    fn arm_wake(&mut self, now: Instant) {
        let at = owner::LIFECYCLE.iter().filter_map(|o| o.wake(self, now)).min();
        let Some(at) = at else { return };
        if self.wake.is_some_and(|w| w <= at && w > now) {
            return;
        }
        let Some(handle) = &self.handle else { return };
        self.wake = Some(at);
        let _ = handle.insert_source(Timer::from_deadline(at), |_, _, app: &mut App| {
            app.wake = None;
            live!(app).bar.tick(&mut app.kit, &app.store, Instant::now());
            app.tick_spares(Instant::now());
            live!(app).dirty = true;
            app.panel_dirty = true;
            TimeoutAction::Drop
        });
    }

    /// Whose surface a pointer event landed on.
    fn target(&self, surface: &wl_surface::WlSurface) -> Option<Target> {
        if let Some(p) = self.atlas_part(surface) {
            return Some(Target::Atlas(p));
        }
        if let Some(p) = self.launcher_part(surface) {
            return Some(Target::Launcher(p));
        }
        if live!(self).surface.as_ref().is_some_and(|s| s.layer.wl_surface() == surface) {
            return Some(Target::Bar);
        }
        if self.panel.as_ref().is_some_and(|p| p.win.owns(surface)) {
            return Some(Target::Panel);
        }
        if self.tray_menu.as_ref().is_some_and(|p| p.win.owns(surface)) {
            return Some(Target::Menu);
        }
        if self.outgoing.as_ref().is_some_and(|p| p.win.owns(surface)) {
            return Some(Target::Outgoing);
        }
        if self.tip_card.as_ref().is_some_and(|c| c.surface.layer.wl_surface() == surface) {
            return Some(Target::Tooltip);
        }
        if self.toasts.as_ref().is_some_and(|t| t.surface.layer.wl_surface() == surface) {
            return Some(Target::Toasts);
        }
        if self.join_preview.as_ref().is_some_and(|(s, _, _)| s.layer.wl_surface() == surface) {
            return Some(Target::Preview);
        }
        if self.scrim.as_ref().is_some_and(|(_, s)| s.layer.wl_surface() == surface) {
            return Some(Target::Scrim);
        }
        if self.osd_owns(surface) {
            return Some(Target::Osd);
        }
        if self.headset_owns(surface) {
            return Some(Target::Headset);
        }
        if self.profiles_owns(surface) {
            return Some(Target::Profiles);
        }
        if let Some((i, part)) = self.head_part(surface) {
            return Some(Target::Head(i, part));
        }
        if self.switcher.owns(surface) {
            return Some(Target::Switcher);
        }
        if self.overview.owns(surface) {
            return Some(Target::Overview);
        }
        if let Some(i) = self.picker_owner(surface) {
            return Some(Target::Picker(i));
        }
        if let Some(i) = live!(self).zones.iter().position(|(_, z)| z.layer.wl_surface() == surface) {
            return Some(Target::Zone(i));
        }
        // The compositor sends a press to the panel surface it still has
        // under the pointer after the shell has dropped it; it is the
        // outgoing card's, and one on the strip the bar's all the same.
        self.dropped.contains(surface).then_some(Target::Outgoing)
    }

    /// Every window owner's blocks of `debug dump`.
    pub fn report(&self, dump: &mut serde_json::Map<String, serde_json::Value>) {
        for o in owner::LIFECYCLE {
            o.report(self, dump);
        }
    }

    pub fn report_exit(&self) {
        let mut parts = Vec::new();
        for o in owner::LIFECYCLE {
            o.counts(self, &mut parts);
        }
        eprintln!("exit t={}ms {}", self.started.elapsed().as_millis(), parts.join(" "));
    }

    fn hover(&mut self, owner: Option<Target>, at: (f64, f64)) {
        let now = Instant::now();
        let bar_hover = (owner == Some(Target::Bar)).then(|| live!(self).bar.hit(at.0, at.1)).flatten();
        if live!(self).bar.set_hover(bar_hover) {
            live!(self).dirty = true;
        }
        if live!(self).bar.pointer(&mut self.kit, bar_hover, at, &self.store, Instant::now()) {
            live!(self).dirty = true;
        }
        if let Some(menu) = self.tray_menu.as_mut() {
            let hit = (owner == Some(Target::Menu)).then(|| menu.menu_hit(at.0, at.1)).flatten();
            if let Some(m) = &mut menu.menu {
                m.hover(hit);
            }
        }
        self.hover_toasts((owner == Some(Target::Toasts)).then_some(at));
        if let Some(h) = &mut self.panel {
            let point = (owner == Some(Target::Panel)).then_some(at);
            if h.pointer(point, &self.store, self.runtime.as_ref()) {
                self.panel_dirty = true;
            }
        }
        // No bar tooltip while a panel hangs off the strip: it would hang
        // over the card.
        let opened = self.panel.as_ref().is_some_and(|p| p.is_open());
        // The tooltip the item under the pointer carries, in the output's
        // coordinates.
        let ask = match owner {
            Some(Target::Bar) if opened => None,
            Some(Target::Bar) => live!(self).bar.tooltip().map(|(text, r, edge)| {
                let (ox, oy) = self.chrome().window();
                let rect = IRect::new(r.x + ox, r.y + oy, r.w, r.h);
                tooltip::Ask { owner: format!("bar:{}:{}", rect.x, rect.y), text, rect, bar: Some(edge) }
            }),
            Some(Target::Panel) => self.panel.as_ref().and_then(|h| h.tooltip()).map(|(text, rect)| tooltip::Ask {
                owner: format!("panel:{}:{}", rect.x, rect.y),
                text,
                rect,
                bar: None,
            }),
            _ => None,
        };
        match ask {
            Some(ask) => self.tips.show(ask, now),
            None => self.tips.hide(None, now),
        }
        self.preview_pointer(owner == Some(Target::Bar), owner == Some(Target::Panel), at);
    }

    /// Each event down `owner::INPUT`, on to the strip's handling when no
    /// owner took it.
    fn pointer_events(&mut self, events: &[PointerEvent]) {
        'events: for e in events {
            let mut target = self.target(&e.surface);
            for o in owner::INPUT {
                match o.pointer(self, e, target) {
                    owner::Pointer::Pass => {}
                    owner::Pointer::Taken => continue 'events,
                    owner::Pointer::Rerouted => target = self.target(&e.surface),
                }
            }
            self.pointer_rest(e, target);
        }
    }

    /// An event no owner took: the strip, its cards and the toasts, and the
    /// hover and cursor every surface shares.
    fn pointer_rest(&mut self, e: &PointerEvent, mut owner: Option<Target>) {
        let (mut x, mut y) = e.position;
        // A pointer left still over the strip once a panel maps gets its
        // presses on the panel's surface, though that surface's input
        // region leaves the strip out; they are the bar's.
        let on_bar = self.bar_input().iter().any(|r| x >= r.x as f64 && x < r.right() as f64 && y >= r.y as f64 && y < r.bottom() as f64);
        if matches!(owner, Some(Target::Panel | Target::Outgoing)) && on_bar {
            owner = Some(Target::Bar);
            if !live!(self).bar.framed() {
                let (ox, oy) = self.chrome().origin(live!(self).bar.thickness());
                (x, y) = (x - ox as f64, y - oy as f64);
            }
        }
        match e.kind {
            PointerEventKind::Enter { serial } => {
                self.cursor = Some((serial, Shape::Default));
                self.pointer_on = owner;
                self.hover(owner, (x, y));
                self.set_cursor(true);
            }
            PointerEventKind::Motion { .. } => {
                if matches!(self.pressed, Some((Target::Bar, _))) && live!(self).bar.sidebar_motion((x, y)) {
                    live!(self).dirty = true;
                }
                self.pointer_on = owner;
                self.hover(owner, (x, y));
                self.set_cursor(false);
            }
            PointerEventKind::Leave { .. } => {
                self.pointer_on = None;
                self.hover(None, (x, y));
            }
            PointerEventKind::Press { .. } => {
                if owner == Some(Target::Toasts) {
                    if let Some(t) = &mut self.toasts {
                        t.press(x, y);
                    }
                    self.pressed = Some((Target::Toasts, Press::Whole));
                    return;
                }
                if owner == Some(Target::Panel) {
                    if let Some(h) = &mut self.panel {
                        h.press(x, y, &self.store, self.runtime.as_ref());
                    }
                    self.pressed = Some((Target::Panel, Press::Whole));
                    self.panel_dirty = true;
                    return;
                }
                let hit = match owner {
                    Some(Target::Bar) => live!(self).bar.hit(x, y).map(Press::Slot),
                    Some(Target::Menu) => self.tray_menu.as_ref().and_then(|p| p.menu_hit(x, y)).map(Press::Menu),
                    _ => None,
                };
                if owner == Some(Target::Menu) && hit.is_none() {
                    let outside = self.tray_menu.as_ref().is_some_and(|p| !p.win.on_card(x, y));
                    if outside {
                        self.close_tray_menu();
                        return;
                    }
                }
                self.pressed = owner.zip(hit);
                if let Some((Target::Bar, Press::Slot(i))) = self.pressed
                    && live!(self).bar.sidebar_press(i, (x, y))
                {
                    live!(self).dirty = true;
                }
            }
            PointerEventKind::Release { button, .. } => {
                let Some((o, press)) = self.pressed.take() else { return };
                let button = crate::input::button(button);
                if o == Target::Toasts {
                    self.release_toasts(x, y);
                    return;
                }
                if o == Target::Panel {
                    let outs = self.panel.as_mut().map_or(Vec::new(), |h| h.release(x, y, button, &self.store, self.runtime.as_ref()));
                    self.panel_outs(Holder::Host, outs);
                    return;
                }
                if o == Target::Bar
                    && let Some(commands) = live!(self).bar.sidebar_release((x, y))
                {
                    self.minimap_drop(commands);
                    return;
                }
                self.click(o, press, button, (x, y));
            }
            PointerEventKind::Axis { time, vertical, horizontal, source } => {
                if owner == Some(Target::Panel) {
                    let (dx, dy) = (crate::scroll::notches(&horizontal), crate::scroll::notches(&vertical));
                    let travel = crate::scroll::travel(&vertical, source, time);
                    if let Some(h) = self.panel.as_mut().filter(|_| dx != 0.0 || dy != 0.0 || travel != crate::scroll::Travel::None) {
                        h.scroll(x, y, dx, dy, travel, &self.store, self.runtime.as_ref());
                    }
                    self.panel_dirty = true;
                    return;
                }
                if owner == Some(Target::Toasts) {
                    let finger = source == Some(wl_pointer::AxisSource::Finger);
                    let lift = ((horizontal.stop || vertical.stop) && finger).then_some(time);
                    self.swipe_toasts(x, y, crate::scroll::travel(&horizontal, source, time), lift);
                    return;
                }
                let (v, h) = (crate::scroll::notches(&vertical), crate::scroll::notches(&horizontal));
                if v == 0.0 && h == 0.0 {
                    return;
                }
                if owner == Some(Target::Menu) {
                    if let Some(m) = self.open_menu_mut() {
                        m.scroll_notches(v);
                    }
                    return;
                }
                if let Some(i) = live!(self).bar.hit(x, y).filter(|_| owner == Some(Target::Bar)) {
                    let action = live!(self).bar.wheel(i, v, h, &self.store);
                    let anchor = live!(self).bar.slot_anchor(i);
                    self.act(action, anchor);
                    if live!(self).bar.sidebar().is_some() {
                        live!(self).dirty = true;
                        self.request_paint();
                    }
                }
            }
        }
    }

    /// The hand over an interactive cell, the arrow anywhere else; sent
    /// only when it changes, or on every enter.
    fn set_cursor(&mut self, entered: bool) {
        let (Some(device), Some((serial, current))) = (&self.cursor_device, self.cursor) else { return };
        let hand = match self.pointer_on {
            Some(Target::Bar) => live!(self).bar.hover.is_some_and(|i| live!(self).bar.clickable(i)),
            Some(Target::Panel) => self.panel.as_ref().is_some_and(|h| h.hand()),
            Some(Target::Headset) => true,
            Some(Target::Profiles) => true,
            _ => false,
        };
        let shape = if hand { Shape::Pointer } else { Shape::Default };
        if entered || shape != current {
            device.set_shape(serial, shape);
            self.cursor = Some((serial, shape));
        }
    }

    fn click(&mut self, owner: Target, press: Press, button: Button, at: (f64, f64)) {
        match (owner, press) {
            (Target::Bar, Press::Slot(i)) if live!(self).bar.clickable(i) => {
                let a = live!(self).bar.click(i, button, at.0, at.1, &self.store);
                let anchor = live!(self).bar.slot_anchor(i);
                self.act(a, anchor);
            }
            (Target::Menu, Press::Menu(hit)) => {
                if button != Button::Left {
                    return;
                }
                let outcome = match (self.tray_menu.as_mut().and_then(|p| p.menu.as_mut()), hit) {
                    (Some(_), Hit::Close) => return self.close_tray_menu(),
                    (Some(m), Hit::Row(i)) => m.activate(i),
                    (None, _) => return,
                };
                self.menu_outcome(outcome);
            }
            _ => {}
        }
    }

    /// One key on the keyboard, down `owner::INPUT` to the first owner
    /// holding the keyboard.
    fn key_event_from(&mut self, event: KeyEvent, repeat: bool) {
        for o in owner::INPUT {
            if o.key(self, &event, repeat) {
                return;
            }
        }
    }

    /// The clipboard's text read for a Ctrl+V, into whichever owner holds
    /// the keyboard now, in the order keys reach them.
    pub(crate) fn field_paste(&mut self) {
        let Some(text) = self.store.clipboard.pasted.take() else { return };
        for o in owner::INPUT {
            if o.paste(self, &text) {
                return;
            }
        }
    }

    /// What a panel asked of the shell outside it, in the order it asked.
    /// `holder` is whatever holds the card, and closes it its own way.
    pub(crate) fn panel_outs(&mut self, holder: Holder, outs: Vec<Out>) {
        for out in outs {
            self.panel_out(holder, out);
        }
        self.panel_dirty = true;
    }

    /// The holder's card closed: the panel host's, or the launcher with it.
    fn holder_close(&mut self, holder: Holder) {
        match holder {
            Holder::Host => self.close_panels(),
            Holder::Launcher => self.menu_close(),
        }
    }

    fn panel_out(&mut self, holder: Holder, out: Out) {
        match out {
            Out::Close => self.holder_close(holder),
            Out::Summon(panel::ATLAS) => self.atlas_show(),
            Out::Summon(route) => {
                // The launcher keeps its window for the route it goes to.
                let from = match holder {
                    Holder::Host => {
                        self.close_panels();
                        None
                    }
                    Holder::Launcher => {
                        let from = self.launcher_panel();
                        self.launcher.close();
                        from
                    }
                };
                self.menu_open(Some(route));
                if route == crate::surfaces::launcher::PICKER_ROUTE {
                    self.launcher.picker.from = from;
                }
            }
            Out::Panel(id, _) if holder == Holder::Launcher && id.home() == Home::Launcher => self.menu_panel(id),
            Out::Panel(id, then) => {
                // The card handing over is the one the new card goes back to.
                let opener = match holder {
                    Holder::Host => self.panel.as_ref().filter(|h| h.is_open()).and_then(|h| {
                        Some(Opener { id: h.panel()?, from: h.from, title: h.title(&self.store, &self.store.theme.theme) })
                    }),
                    Holder::Launcher => {
                        self.menu_close();
                        None
                    }
                };
                self.set_panel(id, true, None);
                if holder == Holder::Host
                    && let Some(h) = self.panel.as_mut().filter(|h| h.is_open() && h.panel() == Some(id))
                {
                    h.back = opener;
                }
                if let Some(stop) = then {
                    self.panel_activate(id, &stop);
                }
            }
            Out::Back(id, from) => {
                let anchor = self.opener_anchor(from);
                self.set_panel_from(Name::Panel(id), true, anchor, from);
            }
            Out::Menu(key, anchor) => {
                let _ = self.open_tray_menu(&key, anchor, true);
            }
            Out::Ipc(argv) => {
                if let [target, function, args @ ..] = argv.as_slice() {
                    let args: Vec<&str> = args.iter().map(String::as_str).collect();
                    self.call_self(target, function, &args);
                }
            }
            Out::Act(action) => {
                let anchor = match holder {
                    Holder::Host => self.panel.as_ref().and_then(|h| h.place.anchor),
                    Holder::Launcher => None,
                };
                let anchor = anchor.unwrap_or(live!(self).bar.length() as f64);
                self.holder_close(holder);
                self.act(action, anchor);
            }
        }
    }

    /// Enter on `stop` in the open panel `id`, as the keyboard would;
    /// `false` when that panel is not up.
    pub fn panel_activate(&mut self, id: PanelId, stop: &str) -> bool {
        let Some(h) = self.panel.as_mut().filter(|p| p.is_open() && p.panel() == Some(id)) else { return false };
        let outs = h.activate(stop, &self.store, self.runtime.as_ref());
        self.panel_outs(Holder::Host, outs);
        true
    }

}

impl crate::surface::ScaleHandler for App {
    /// Every surface draws again at its new scale on its own next present;
    /// only the owners that present when something changed need telling.
    fn rescaled(&mut self) {
        for o in owner::LIFECYCLE {
            o.rescaled(self);
        }
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {
        crate::surface::ScaleHandler::rescaled(self);
    }

    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}

    /// Every callback ticks its owner once more, so the frame that lands an
    /// animation on its rest is drawn, and that commit asks for no other.
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &wl_surface::WlSurface, _: u32) {
        if let Some(o) = owner::of(self, surface) {
            o.frame(self, surface, Instant::now());
        }
    }

    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}

    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        if let Some(o) = owner::of(self, layer.wl_surface()) {
            o.closed(self, layer.wl_surface());
        }
    }

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface, configure: LayerSurfaceConfigure, _: u32) {
        let size = (configure.new_size.0 as i32, configure.new_size.1 as i32);
        if let Some(o) = owner::of(self, layer.wl_surface()) {
            o.configure(self, layer.wl_surface(), size);
        }
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {
        self.arm_idle();
        self.arm_settle();
    }

    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, capability: Capability) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seats.get_pointer(qh, &seat).ok();
            self.cursor_device = match (&self.cursor_shapes, &self.pointer) {
                (Some(m), Some(p)) => Some(m.get_shape_device(p, qh)),
                _ => None,
            };
        }
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            // Repeats come off the loop, so a held arrow glides the cursor.
            let repeat: smithay_client_toolkit::seat::keyboard::repeat::RepeatCallback<App> = Box::new(|app, _, event| app.key_event_from(event, true));
            self.keyboard = match self.handle.clone() {
                Some(handle) => self.seats.get_keyboard_with_repeat(qh, &seat, None, handle, repeat).ok(),
                None => self.seats.get_keyboard(qh, &seat, None).ok(),
            };
        }
    }

    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, capability: Capability) {
        if let Some(p) = self.pointer.take().filter(|_| capability == Capability::Pointer) {
            p.release();
        }
        if let Some(k) = self.keyboard.take().filter(|_| capability == Capability::Keyboard) {
            self.released.release(k);
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for App {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        self.pointer_events(events);
    }
}

impl KeyboardHandler for App {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {}

    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {}

    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        self.key_event_from(event, false);
    }

    fn repeat_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        self.key_event_from(event, true);
    }

    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}

    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, m: Modifiers, _: RawModifiers, _: u32) {
        self.mods = crate::input::modifiers(m);
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.on_outputs();
    }

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.on_outputs();
    }

    /// sctk calls this while the output is still in its list, so nothing
    /// here may rebuild heads from that list.
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        for o in owner::LIFECYCLE {
            o.output_gone(self, &output);
        }
    }
}

impl App {
    fn on_outputs(&mut self) {
        // Written before the surfaces below are made, which open at it; what
        // reads it reacts once they are.
        let scales = self.store.display.scales.read(&self.outputs, self.bar_wl());
        let moved = self.store.apply(Diff::Display(crate::services::display::Diff::Scales(scales)));
        self.lock_outputs();
        self.sync_hot_corners();
        self.sync_heads();
        self.saver_outputs();
        if live!(self).surface.is_none() && !live!(self).bar.hidden() {
            self.place_chrome();
        }
        let name = self.bar_output_name();
        if name != live!(self).bar.output {
            live!(self).bar.output = name;
            self.refresh_bar(None);
        } else {
            self.request_paint();
        }
        self.moved(moved);
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(App);
delegate_dispatch2!(App);

/// The theme engine's inputs once both files have been read.
pub fn theme_inputs(app: &App) {
    let (config, state) = (&app.store.config, &app.store.state);
    if !config.loaded || !state.loaded {
        return;
    }
    // Off settings.json itself rather than the resolved theme, which may
    // not have taken this settings change yet.
    let color = settings::wallpaper_color(config);
    theme::send_inputs(theme::Inputs {
        settings: config.settings().clone(),
        wallpaper: crate::services::wallpaper::source(color.as_deref(), &state.data.wallpaper_color, &state.data.wallpaper, &state.data.wallpaper_dark, &state.data.mode, app.store.sky_step()),
        mode: state.data.mode.clone(),
        mode_override: theme::parse_override(&state.data.mode_override),
        location: settings::location(config),
        sky_at: app.store.theme.sky_at,
    });
}

/// A bar's exclusive zone told to its layer again when its paint moved it
/// (a band turning solid under a maximized window reserves its margin too).
pub(crate) fn sync_zone(bar: &mut Bar, surface: Option<&mut Surface>) {
    let zone = bar.zone();
    if zone == bar.reserved || bar.framed() {
        return;
    }
    bar.reserved = zone;
    if let Some(s) = surface {
        s.layer.set_exclusive_zone(zone);
        s.layer.commit();
    }
}

/// An attached card hung at a published join, so `debug join` has a card in
/// the gap it opens. Drawn into a full-output scene; `line` is the line's row across from `edge`.
fn draw_join_preview(scene: &mut crate::scene::Scene, nodes: &mut Vec<crate::scene::NodeId>, theme: &fs_theme::theme::Theme, edge: fs_types::Edge, line: f64, join: Option<(f64, f64)>) {
    use vello_cpu::kurbo::{Affine, Rect};
    let mut p = crate::paint::Painter::new(scene, nodes, None);
    if let Some((x, width)) = join {
        const DEPTH: f64 = 140.0;
        let card = theme.box_style(fs_theme::style::Role::Card, None);
        let radius = theme.card_radius();
        let (color, bw) = card.border.map_or((fs_theme::color::Rgba::TRANSPARENT, 0.0), |l| (l.color, l.width));
        let paths = crate::paint::shoulders::paths(width + radius * 2.0, DEPTH, radius, bw, 1.0, 0.0);
        let (sw, sh) = (p.scene().size.w as f64, p.scene().size.h as f64);
        let top = line - theme.border_width;
        // The top-line outline turned onto the join's edge.
        let at = crate::paint::edge_map(edge, if edge.is_vertical() { sw } else { sh }) * Affine::translate((x - radius, top));
        let bounds = crate::scene::Scene::cover(Rect::new(0.0, 0.0, width + radius * 2.0, DEPTH), at, 2.0);
        let mut fill = paths.fill;
        fill.apply_affine(at);
        let mut outer = paths.outer;
        outer.apply_affine(at);
        p.shape(bounds, crate::scene::Paint::Shape { fill: Some((fill, card.fill)), strokes: vec![(outer, color, bw)] });
    }
    p.finish();
}
