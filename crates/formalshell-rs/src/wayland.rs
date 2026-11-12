//! The Wayland side: the bar's layer surface (gone while a fullscreen window
//! covers its output), the frame's four exclusion zones, the cards hanging
//! off the bar (`panel.rs` and `tray_menu.rs` hold their controllers), the
//! tooltip, the scrim, the pointer and the keyboard, and which owner a
//! configure, a frame callback or an input event belongs to.

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
pub mod capture_host;
mod console;
mod dock;
pub(crate) mod desktop_clock;
mod headset;
mod profiles;
mod heads;
mod hotcorners;
pub(crate) mod launcher;
pub mod lock;
mod osd;
mod panel;
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
mod tray_menu;
mod veil;
mod zen;

use std::time::Instant;

use calloop::LoopHandle;
use calloop::timer::{TimeoutAction, Timer};
use fs_chrome::bar::panels::PanelId;
use fs_types::{Edge, IRect};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState, Region};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::protocol::wl_display::WlDisplay;
use smithay_client_toolkit::reexports::client::protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface};
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::reexports::protocols::ext::idle_notify::v1::client::ext_idle_notifier_v1::ExtIdleNotifierV1;
use smithay_client_toolkit::reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{Shape, WpCursorShapeDeviceV1};
use smithay_client_toolkit::seat::pointer::cursor_shape::CursorShapeManager;
use smithay_client_toolkit::seat::keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers};
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::activation::ActivationState;
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
use crate::scene::NodeId;
use crate::services::{barpaint, devices, compositor, notifications, overnight, tray, visualizer};
use crate::settings;
use crate::store::{Diff, Moved, Store, Topic};
use crate::surface::{Backend, PixelSurface, Pixels, Surface};
use crate::surfaces;
use crate::surfaces::bar::Bar;
use crate::surfaces::bar::cell::{Action, TrayClick};
use crate::surfaces::card::{Card, Join, Scrim};
use crate::surfaces::card_window::CardWindow;
use crate::surfaces::modal::Modal;
use crate::surfaces::panel::{Name, host::Host, quick};
use crate::surfaces::tooltip;
use crate::surfaces::tray_menu::Hit;
use tray_menu::Popout;

pub(crate) fn edge_anchor(edge: Edge) -> Anchor {
    match edge {
        Edge::Top => Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
        Edge::Bottom => Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
        Edge::Left => Anchor::LEFT | Anchor::TOP | Anchor::BOTTOM,
        Edge::Right => Anchor::RIGHT | Anchor::TOP | Anchor::BOTTOM,
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
    Dock,
}

impl Role {
    pub const ALL: [Role; 11] = [
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
        Role::Dock,
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
    Dock,
    /// The strip the pointer rests on to bring the dock out.
    DockEdge,
    Zone(usize),
    /// An output's wallpaper, or its bar while that is a spare.
    Head(usize, heads::Part),
    /// The launcher's card or one of its two scrims.
    Launcher(crate::surfaces::modal::Part),
    Switcher,
    Overview,
    Picker(usize),
    Atlas(crate::surfaces::modal::Part),
    Saver,
    Polkit,
    Overlay,
    Lock,
    Veil,
    Caffeinate,
    HotCorner,
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
    activation: Option<ActivationState>,
    seat: Option<wl_seat::WlSeat>,
    /// The last button press's serial and surface, which an activation
    /// token for what the click opens is asked with.
    press: Option<(u32, wl_surface::WlSurface)>,
    display: WlDisplay,
    qh: QueueHandle<Self>,
    handle: LoopHandle<'static, App>,
    pub runtime: Runtime,
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
    dock: dock::Dock,
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
    tray_menu_pending: Option<tray_menu::PendingMenu>,
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
    pub hdr: crate::services::display::Hdr,
    wake: Option<Instant>,
    rims: rims::Rims,
    /// The backdrop mark's word, by the style it was shaped in.
    shard_word: Option<(crate::text::TextStyle, crate::shards::Words)>,
    /// pantheon's cast under a joined card.
    pub cast: bool,
    /// `debug join`'s x and width on the bar's line, held under any card's.
    debug_join: Option<(i32, i32)>,
    pub exit: bool,
    started: Instant,
    pub launcher: crate::surfaces::launcher::Model,
    launch: launcher::State,
    /// The launcher's faces are warmed once, after the bar is on screen.
    faces_warmed: bool,
    /// A modal card was up at the last present, which holds the backdrop's
    /// sculpture still.
    modal_up: bool,
    mods: crate::ui::field::Modifiers,
    pub capture: capture_host::Capture,
    pickers: region::Pickers,
    desktop_clock: desktop_clock::State,
    veil: veil::State,
}

impl App {
    /// `started` is the clock every logged `t=` counts from.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        display: WlDisplay,
        globals: &GlobalList,
        qh: &QueueHandle<Self>,
        handle: LoopHandle<'static, App>,
        runtime: Runtime,
        started: Instant,
        motion_scale: f64,
        prompter: Option<fs_auth::prompter::Prompter>,
    ) -> Self {
        let compositor = CompositorState::bind(globals, qh).expect("wl_compositor is not available");
        let layer_shell = LayerShell::bind(globals, qh).expect("zwlr_layer_shell_v1 is not available");
        let shm = Shm::bind(globals, qh).expect("wl_shm is not available");
        let pixels = Pixels::bind(globals, qh)
            .expect("wp_viewporter, wp_single_pixel_buffer_manager_v1 and wp_alpha_modifier_v1 are required");
        let mut store = Store::default();
        store.apply(Diff::Theme(crate::services::theme::Diff::MotionScale(motion_scale)));
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
            activation: ActivationState::bind(globals, qh).ok(),
            seat: None,
            press: None,
            display,
            qh: qh.clone(),
            handle,
            runtime,
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
            dock: dock::Dock::default(),
            thumbnails: capture::Capture::bind(globals, qh),
            switcher: switcher::State::default(),
            overview: overview::State::default(),
            atlas: atlas::State::default(),
            peek: preview::State::default(),
            bars: heads::Bars::new(bar),
            sidebar_keys: None,
            panel: None,
            tray_menu: None,
            tray_menu_pending: None,
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
            wake: None,
            rims: Default::default(),
            shard_word: None,
            cast: false,
            debug_join: None,
            exit: false,
            started,
            launcher: Default::default(),
            launch: Default::default(),
            faces_warmed: false,
            modal_up: false,
            mods: Default::default(),
            capture: Default::default(),
            hdr: Default::default(),
            pickers: Default::default(),
            desktop_clock: Default::default(),
            veil: Default::default(),
        };
        app.raise_login_lock();
        app.begin_arrival();
        app.sync_heads();
        app.start_lock();
        app.start_veil();
        app
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
                crate::trace::mapped(crate::trace::Surface::Bar, crate::trace::State::UnmappedUnderFullscreen);
            }
            live!(self).zones.clear();
            self.close_bar_cards();
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
        let mut surface = Surface::new("bar", Backend::Gpu, layer, &self.shm, self.store.display.scales.on(output));
        surface.presize(bar.scene.size.w, bar.scene.size.h);
        let mut zones = Vec::new();
        if framed {
            let ft = bar.frame_thickness() as i32;
            for e in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
                let zone = if e == edge { t } else { ft };
                let size = if e.is_vertical() { (1, 0) } else { (0, 1) };
                let ls = self.overlay_on("formalshell:frame-zone", Layer::Overlay, edge_anchor(e), size, zone, Some(output));
                let ps = PixelSurface::new("frame-zone", ls, 0.0, &self.pixels, &self.qh);
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
        let resolved = settings::bar_layout(&self.store.config, &self.store.plugins.bar_plugins());
        visualizer::set_motion(settings::MOTION.get(&self.store.config));
        self.arm_caffeinate();
        self.arm_settle();
        let edge = self.chrome_edge();
        let moved = self.bars.apply(|b| b.set_edge(edge));
        let relaid = self.bars.apply(|b| b.set_layout(resolved.clone()));
        if moved || relaid {
            self.close_bar_cards();
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

    /// Marks the cell whose card is open.
    fn sync_bar_open(&mut self, now: Instant) {
        let panel = self.panel_open().and_then(Name::panel);
        let overflow = self.overflow_open();
        live!(self).bar.set_open(&self.kit, panel, overflow, now);
    }

    /// The modal card `role` names, while it is on screen.
    fn modal(&self, role: Role) -> Option<&Modal> {
        match role {
            Role::Launcher => self.launch.win.as_ref().map(|w| &w.shown.modal),
            Role::Atlas => self.atlas.win.as_ref().map(|w| &w.shown.modal),
            Role::Polkit => self.polkit.as_ref().map(|d| &d.modal),
            Role::Overlay => self.plugin_overlay.as_ref().map(|w| &w.modal),
            _ => None,
        }
    }

    fn modal_mut(&mut self, role: Role) -> Option<&mut Modal> {
        match role {
            Role::Launcher => self.launch.win.as_mut().map(|w| &mut w.shown.modal),
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
            Role::Dock => self.dock.card.as_ref().map(|d| &d.popup.win),
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
            Role::Dock => self.dock.card.as_mut().map(|d| &mut d.popup.win),
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
            let surface = Surface::new("debug-join", Backend::Gpu, layer, &self.shm, self.store.display.scales.primary());
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
            theme: &self.store.theme.theme,
            chrome: self.chrome(),
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
        let mut surface = Surface::new("panel", Backend::Gpu, layer, &self.shm, self.store.display.scales.primary());
        surface.wait_map = true;
        surface
    }

    pub fn set_scrim(&mut self, now: Instant, open: bool) {
        if let Some((scrim, _)) = &mut self.scrim {
            scrim.set_open(now, open);
            return;
        }
        if !open {
            return;
        }
        let scrim = Scrim::new(&self.store.theme.theme);
        let layer = self.overlay("formalshell:scrim", Layer::Top, Anchor::all(), (0, 0), -1);
        let surface = PixelSurface::new("scrim", layer, scrim.tone(), &self.pixels, &self.qh);
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
    pub fn apply(&mut self, diff: impl Into<Diff>) -> Option<Topic> {
        let moved = self.store.apply(diff.into());
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
            self.changed(topic);
        }
    }

    /// What a store change asks of the shell's own parts, then of every
    /// window owner that reads its topic (`Owner::reads`).
    pub(crate) fn changed(&mut self, topic: Topic) {
        if matches!(topic, Topic::Compositor | Topic::State | Topic::Display | Topic::Config) {
            self.hdr.reconcile(&self.store);
        }
        match topic {
            Topic::Compositor => self.zen_changed(),
            Topic::Capture => capture_host::events(self),
            Topic::Clipboard => self.field_paste(),
            _ => {}
        }
        owner::changed(self, topic);
    }

    /// One IPC call, answered here on the UI loop.
    pub fn call(&mut self, request: &ipc::Request) -> String {
        let t0 = Instant::now();
        crate::trace::call(request);
        let reply = ipc::dispatch(self, request);
        crate::trace::slow("call", t0.elapsed(), format_args!("{} us={}", crate::trace::request(request), t0.elapsed().as_micros()));
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
                match click {
                    TrayClick::Secondary => self.runtime.service(move |ctx| tray::secondary_activate(ctx, key)),
                    _ => self.tray_activate_key(key),
                }
            }
            Action::AppMenu { id, offset } => {
                let _ = self.open_app_menu(id, Some(anchor + offset));
            }
            Action::Caffeinate(on) => self.set_caffeinated(on),
            Action::OvernightOff => {
                if let Some(record) = self.store.state.data.overnight.clone() {
                    self.runtime.service(move |ctx| overnight::disable(ctx, record));
                }
            }
            Action::RecordStop => {
                let _ = capture_host::record::stop(self);
            }
            Action::ReminderSummary => {
                self.apply(notifications::Op::ShowReminders);
            }
            Action::MediaNext => self.store.media.next(),
            Action::MediaPrevious => self.store.media.previous(),
            Action::Center => {
                let open = !self.store.notifications.center_open;
                self.set_center(open, Some(anchor));
            }
            Action::Dnd(on) => {
                self.apply(notifications::Op::SetDnd(on));
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
        capture_host::flush(self);
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
                    let mut surface = Surface::new("tooltip", Backend::Gpu, layer, &self.shm, self.store.display.scales.primary());
                    surface.wait_map = true;
                    self.tip_card = Some(tooltip::Card::new(surface, (size.0 as i32, size.1 as i32)));
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
        self.wake = Some(at);
        let _ = self.handle.insert_source(Timer::from_deadline(at), |_, _, app: &mut App| {
            app.wake = None;
            live!(app).bar.tick(&mut app.kit, &app.store, Instant::now());
            app.tick_spares(Instant::now());
            live!(app).dirty = true;
            app.panel_dirty = true;
            TimeoutAction::Drop
        });
    }

    /// Every window owner's blocks of `debug dump`.
    pub fn report(&self, dump: &mut serde_json::Map<String, serde_json::Value>) {
        owner::report(self, dump);
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
        let mut off_card = false;
        if let Some(menu) = self.tray_menu.as_mut() {
            let hit = (owner == Some(Target::Menu)).then(|| menu.menu_hit(at.0, at.1)).flatten();
            if let Some(m) = &mut menu.menu {
                m.hover(hit);
            }
            off_card = owner == Some(Target::Menu) && hit.is_none() && !menu.win.on_card(at.0, at.1);
        }
        if off_card {
            self.app_menu_scrub(at.0, at.1);
        }
        self.hover_toasts((owner == Some(Target::Toasts)).then_some(at));
        if let Some(h) = &mut self.panel {
            let point = (owner == Some(Target::Panel)).then_some(at);
            if h.pointer(point, &self.store, &self.runtime) {
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
            Some(Target::Dock) => self.dock_tip(),
            _ => None,
        };
        match ask {
            Some(ask) => self.tips.show(ask, now),
            None => self.tips.hide(None, now),
        }
        self.preview_pointer(owner == Some(Target::Bar), owner == Some(Target::Panel), at);
    }

    /// Each event down `owner::INPUT`, on to the shared hover and cursor
    /// when no owner took it.
    fn pointer_events(&mut self, events: &[PointerEvent]) {
        'events: for e in events {
            if let PointerEventKind::Press { serial, .. } = e.kind {
                self.press = Some((serial, e.surface.clone()));
            }
            let (e, mut target) = self.over_strip(e, owner::target(self, &e.surface));
            for o in owner::INPUT {
                match o.pointer(self, &e, target) {
                    owner::Pointer::Pass => {}
                    owner::Pointer::Taken => continue 'events,
                    owner::Pointer::Rerouted => target = owner::target(self, &e.surface),
                }
            }
            self.pointer_rest(&e, target);
        }
    }

    /// A pointer left still over the strip once a panel maps gets its
    /// events on the panel's surface, though that surface's input region
    /// leaves the strip out; they are the bar's, in the bar's coordinates.
    fn over_strip(&self, e: &PointerEvent, target: Option<Target>) -> (PointerEvent, Option<Target>) {
        let mut e = e.clone();
        let (x, y) = e.position;
        let on_bar = self.bar_input().iter().any(|r| x >= r.x as f64 && x < r.right() as f64 && y >= r.y as f64 && y < r.bottom() as f64);
        if !(matches!(target, Some(Target::Panel | Target::Outgoing)) && on_bar) {
            return (e, target);
        }
        if !live!(self).bar.framed() {
            let (ox, oy) = self.chrome().origin(live!(self).bar.thickness());
            e.position = (x - ox as f64, y - oy as f64);
        }
        (e, Some(Target::Bar))
    }

    /// An event no owner took: the hover and cursor every surface shares.
    /// A press or release no owner kept ends any gesture still held.
    fn pointer_rest(&mut self, e: &PointerEvent, owner: Option<Target>) {
        match e.kind {
            PointerEventKind::Enter { serial } => {
                self.cursor = Some((serial, Shape::Default));
                self.pointer_on = owner;
                self.hover(owner, e.position);
                self.set_cursor(true);
            }
            PointerEventKind::Motion { .. } => {
                self.pointer_on = owner;
                self.hover(owner, e.position);
                self.set_cursor(false);
            }
            PointerEventKind::Leave { .. } => {
                self.pointer_on = None;
                self.hover(None, e.position);
            }
            PointerEventKind::Press { .. } | PointerEventKind::Release { .. } => self.pressed = None,
            PointerEventKind::Axis { .. } => {}
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
            Some(Target::Dock) => self.dock_hand(),
            _ => false,
        };
        let shape = if hand { Shape::Pointer } else { Shape::Default };
        if entered || shape != current {
            device.set_shape(serial, shape);
            self.cursor = Some((serial, shape));
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

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        self.seat.get_or_insert(seat);
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
            self.keyboard = self.seats.get_keyboard_with_repeat(qh, &seat, None, self.handle.clone(), repeat).ok();
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
