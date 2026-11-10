//! The launcher's window on the App: a modal card on the overlay layer
//! (`surfaces::modal`), the keyboard and pointer routed to it, and the
//! `menu` verbs.

use std::time::{Duration, Instant};

use calloop::timer::{TimeoutAction, Timer};

use fs_chrome::bar::panels::PanelId;
use fs_menu::actions::{IpcCall, SubjectId};
use fs_menu::intent::Intent;
use fs_menu::node::Provider;
use fs_types::WindowId;
use smithay_client_toolkit::compositor::Region;
use smithay_client_toolkit::dispatch2::Dispatch2;
use smithay_client_toolkit::reexports::client::protocol::wl_callback::{self, WlCallback};
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::seat::keyboard::KeyEvent;
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::Layer;

use super::{App, Target};
use crate::services::menu::{self as index, Ask};
use crate::services::notifications::{self, Op};
use crate::store::{Diff, Topic};
use crate::surface::{Backend, Surface};
use crate::surfaces::launcher::{ABBREV_EDIT, Out, Request, Selected, Shown, Target as Pick, keys};
use crate::surfaces::modal::{Modal, Part};
use crate::text::{Family, TextStyle};
use crate::ui::field::{self, Modifiers};

/// A screen-reading call held for the compositor's answer to a sync sent
/// after the launcher's surfaces went.
pub struct AfterUnmap(u64);

/// How long a held screen grab waits for the compositor's answer before it
/// is dropped.
const GRAB_WAIT: Duration = Duration::from_secs(1);

impl Dispatch2<WlCallback, App> for AfterUnmap {
    fn event(&self, app: &mut App, _: &WlCallback, event: wl_callback::Event, _: &Connection, _: &QueueHandle<App>) {
        if let wl_callback::Event::Done { .. } = event
            && let Some(call) = app.launcher.grab.take(self.0)
        {
            app.run_call(&call);
        }
    }
}

/// The info slices the monitor view samples while it is the level.
const INFO: &[Topic] = &[
    Topic::Weather,
    Topic::Github,
    Topic::Usage,
    Topic::SystemUpdate,
    Topic::Tailscale,
    Topic::Monitor,
    Topic::Iphone,
    Topic::Calendar,
    Topic::Procs,
    Topic::PowerFlow,
    Topic::About,
];

/// How long the launcher's buffers outlive its last close.
const KEPT_FOR: std::time::Duration = std::time::Duration::from_secs(300);
const WIFI_PASSWORD: &str = "wifi-password";
const WIFI_SHOWN: &str = "wifi-shown";
const WIFI_IDENTITY: &str = "wifi-identity";
const REMINDER_SET: &str = "reminder-set";
const LIGHTS_COLOR: &str = "lights-color";

pub struct Window {
    pub shown: Shown,
    /// The Wi-Fi route keeps the scanner awake while it is the level.
    scan: Option<crate::services::wants::Want>,
    /// What a button went down on, and the button.
    pressed: Option<(String, u32)>,
}

impl Window {
    pub fn finished(&self, now: Instant) -> bool {
        self.shown.finished(now)
    }
}

/// The launcher's window and what the host keeps for it across opens.
pub struct State {
    pub win: Option<Window>,
    /// The rows want resolving before the next read or frame.
    resolve: bool,
    /// The index moved while the launcher was shut: resolve its root once
    /// at the next idle frame, so the first open finds the text and rows
    /// already laid out.
    pub warm: bool,
    /// The styles the launcher's last layout shaped in, for warming its
    /// rows' words on the pool before the next open.
    styles: Vec<crate::text::TextStyle>,
    /// The launcher card's pool, buffers and canvas while it is shut.
    kept: Option<crate::surface::Kept>,
    /// The same for the launcher's content layer.
    layer_kept: Option<crate::surface::Kept>,
    /// Bumped on each close, so only the last close's timer releases.
    kept_generation: u64,
    /// The launcher's conditions and binds were asked for once its tree
    /// first arrived, so the first open has results to draw on.
    fresh_asked: bool,
    /// The network the launcher's password step is for, and the identity
    /// an enterprise one was given first (`wifi_pending`).
    wifi_pending: Option<(String, String)>,
    /// The Show password request still awaited, and the launcher session it
    /// was asked in; `wifi_reveals` numbers the requests.
    wifi_reveal: Option<(u64, u64)>,
    wifi_reveals: u64,
    buttons: Option<fs_menu::base::BaseInputs>,
    /// The split pane's last picture request, so a frame asks once.
    preview_asked: Option<(String, ((u32, u32), f64))>,
    /// The file or folder the preview pane last asked a peek of, this open.
    peek_asked: Option<String>,
    /// The Install row whose `fspec info` was last asked for.
    info_asked: Option<String>,
    launches: Option<Vec<fs_menu::frecency::Record>>,
}

impl Default for State {
    fn default() -> State {
        State {
            win: None,
            resolve: true,
            warm: false,
            styles: Vec::new(),
            kept: None,
            layer_kept: None,
            kept_generation: 0,
            fresh_asked: false,
            wifi_pending: None,
            wifi_reveal: None,
            wifi_reveals: 0,
            buttons: None,
            preview_asked: None,
            peek_asked: None,
            info_asked: None,
            launches: None,
        }
    }
}

impl App {
    /// The launcher's window gone, its card's pool, buffers and canvas kept
    /// for the next open.
    fn drop_launch(&mut self) {
        self.capture_clear(crate::wayland::capture::Owner::Launcher);
        self.launcher.pane_frame = None;
        if let Some(w) = self.launch.win.take() {
            let modal = w.shown.modal;
            if let Some(layer) = modal.layer {
                self.launch.layer_kept = Some(layer.keep());
            }
            self.launch.kept = Some(modal.win.surface.keep());
            self.release_kept_later();
        }
    }

    /// What is kept for the next open goes after a while shut: a launcher
    /// used now and then opens on warm buffers, one left alone for long
    /// gives their memory back.
    fn release_kept_later(&mut self) {
        let Some(handle) = &self.handle else { return };
        self.launch.kept_generation += 1;
        let generation = self.launch.kept_generation;
        let _ = handle.insert_source(Timer::from_duration(KEPT_FOR), move |_, _, app| {
            if app.launch.kept_generation == generation && app.launch.win.is_none() {
                app.launch.kept = None;
                app.launch.layer_kept = None;
                if let Some(runtime) = &app.runtime {
                    runtime.pool().submit(crate::trace::trim_heap);
                }
            }
            TimeoutAction::Drop
        });
    }

    /// The first open's pool and canvas, faulted in while the launcher is
    /// shut; every later open takes over the last one's.
    pub(super) fn prefault_launcher(&mut self) {
        let Some(runtime) = &self.runtime else { return };
        if self.launch.kept.is_some() || self.launch.win.is_some() {
            return;
        }
        let (w, h) = self.output_size();
        let size = (w as i32, h as i32);
        self.launch.kept = crate::surface::Kept::prefaulted(&self.shm, size, self.store.display.scales.primary(), runtime.pool());
        self.release_kept_later();
    }

    /// A content layer on `card`, sized by its first layout, taking no
    /// input: the pointer lands on the card underneath.
    pub(super) fn content_layer(&self, name: &'static str, card: &Surface) -> crate::surfaces::modal::Layer {
        use crate::surface::{Ignore, Sub};
        let surface = self.compositor.create_surface(&self.qh);
        let sub = self.pixels.subcompositor.get_subsurface(&surface, card.layer.wl_surface(), &self.qh, Ignore);
        if let Ok(region) = Region::new(&self.compositor) {
            surface.set_input_region(Some(region.wl_region()));
        }
        let viewport = self.pixels.viewporter.get_viewport(&surface, &self.qh, Ignore);
        let s = Surface::new(name, Backend::Gpu, Sub { surface, sub }, &self.shm, self.started, self.store.display.scales.primary()).with_viewport(viewport).under(card);
        crate::surfaces::modal::Layer::new(s)
    }

    /// The window for an open, created fresh unless one is already up.
    fn show_launcher(&mut self) {
        self.launch.resolve = true;
        if self.launch.win.as_ref().is_some_and(|w| w.shown.modal.open) {
            return;
        }
        self.drop_launch();
        let names = ["menu", "menu-scrim-band", "menu-scrim"];
        let mut modal = Modal::open(&self.modal_make(), names, "formalshell:menu", Layer::Overlay, None, Shown::DEFORM_AMOUNT, true);
        if let Some(kept) = self.launch.kept.take() {
            modal.win.surface.adopt(kept);
        }
        modal.win.surface.raster_budget = Some(crate::surfaces::modal::SLICE);
        let mut content = self.content_layer("menu-content", &modal.win.surface);
        if let Some(kept) = self.launch.layer_kept.take() {
            content.surface.adopt(kept);
        }
        content.surface.raster_budget = Some(crate::surfaces::modal::SLICE);
        modal.layer = Some(content);
        let shown = Shown::new(&self.store.theme.theme, modal, self.output_size(), self.motion_scale);
        self.launch.win = Some(Window { shown, scan: None, pressed: None });
        crate::trace::mapped(crate::trace::Surface::Menu, crate::trace::State::Mapped);
    }

    fn hide_launcher(&mut self) {
        if let Some(w) = &mut self.launch.win {
            w.shown.modal.close(Instant::now());
            crate::trace::mapped(crate::trace::Surface::Menu, crate::trace::State::Closing);
        }
        self.sync_join();
    }

    pub fn menu_open(&mut self, route: Option<&str>) {
        self.launcher.open(&self.store, route);
        self.show_launcher();
    }

    /// `id`'s panel (Settings, a settings page, Information) held in the
    /// launcher card, the launcher opening on its route if it is not up.
    pub fn menu_panel(&mut self, id: PanelId) {
        self.atlas_close();
        self.overlay_close();
        self.close_panels();
        if !self.launcher.open {
            self.launcher.open(&self.store, None);
        }
        if self.launcher.show_panel(&self.store, id) {
            self.show_launcher();
        }
    }

    /// The panel the open launcher holds.
    pub fn launcher_panel(&self) -> Option<PanelId> {
        self.launcher.panel.as_ref().filter(|_| self.launcher.open && self.launcher.on_panel()).map(|p| p.id)
    }

    /// The held panel's wants named again, and `Panel::start` once for a
    /// panel the launcher just took up, by whichever route it came in on
    /// (`menu summon`, a row, `menu_panel`). Its wants go when it drops.
    fn launcher_panel_sync(&mut self) {
        let Some(p) = self.launcher.panel.as_mut() else { return };
        p.wants.set(&p.module.wants(&self.store));
        if !p.started {
            p.started = true;
            let mut fx = crate::surfaces::panel::Effect::new(&self.store, self.runtime.as_ref());
            p.module.start(&mut fx);
            fx.none("start");
        }
    }

    /// What a key or click in the held panel asked for. `Back` with nothing
    /// asked of it climbs out of the route, as Backspace does in a mode.
    fn launcher_panel_outs(&mut self, outs: Vec<crate::surfaces::panel::Out>, back: bool) {
        self.launcher.dirty = true;
        if back && outs.is_empty() {
            let out = self.launcher.pop(&self.store);
            self.launcher_out(out);
        } else {
            self.panel_outs(super::Holder::Launcher, outs);
        }
        self.launch.resolve = true;
    }

    /// One level up: a settings page goes back to its tiles, anything else
    /// leaves the level.
    fn launcher_up(&mut self) {
        let page = self.launcher_panel().filter(|p| p.route() != Some(*p));
        match page.and_then(PanelId::route) {
            Some(route) => self.menu_panel(route),
            None => {
                let out = self.launcher.pop(&self.store);
                self.launcher_out(out);
            }
        }
    }

    /// The token's click, one level up as Backspace climbs: a settings page
    /// opened off another page's row goes back to that page.
    fn launcher_back(&mut self) {
        let parent = self.launcher_panel().and_then(|p| p.as_str().strip_prefix("settings:")).and_then(crate::services::overrides::page).and_then(|p| p.parent);
        match parent {
            Some(parent) => self.menu_panel(parent),
            None => self.launcher_up(),
        }
    }

    /// One key into the held panel, through the panel host's cursor model.
    fn launcher_panel_key(&mut self, event: &KeyEvent, repeat: bool) {
        use crate::surfaces::panel::host::{Driven, Key, drive};
        let Some(key) = crate::input::panel_key(event.keysym, event.utf8.clone(), repeat, self.mods.shift) else { return };
        let back = key == Key::Back;
        let Some(w) = &self.launch.win else { return };
        let Some(p) = self.launcher.panel.as_mut() else { return };
        let mut fx = crate::surfaces::panel::Effect::new(&self.store, self.runtime.as_ref());
        let driven = drive(p.module.as_mut(), &mut p.cursor, w.shown.stops(), key, &mut fx);
        let outs = fx.outs();
        match driven {
            Driven::Escape => return self.launcher_up(),
            Driven::Walked => {
                if let Some(w) = &self.launch.win {
                    w.shown.follow_panel(&mut self.launcher);
                }
            }
            Driven::Done => {}
        }
        self.launcher_panel_outs(outs, back);
    }

    pub fn menu_toggle(&mut self) {
        if self.launcher.open {
            self.menu_close();
        } else {
            self.menu_open(None);
        }
    }

    pub fn menu_close(&mut self) {
        self.launcher.close();
        self.hide_launcher();
    }

    pub fn menu_select(&mut self, prompt: &str, options: Vec<String>, token: &str) {
        self.launcher.open_select(prompt, options, token);
        self.show_launcher();
    }

    pub fn menu_input(&mut self, prompt: &str, token: &str) {
        self.menu_input_as(prompt, token, false);
    }

    /// An input step; a secret one is drawn masked and never filed.
    fn menu_input_as(&mut self, prompt: &str, token: &str, secret: bool) {
        self.launcher.open_input(prompt, token, secret);
        self.launch.resolve = true;
        self.show_launcher();
    }

    pub fn menu_filter(&mut self, text: &str) -> bool {
        if !self.launcher.open {
            return false;
        }
        self.launcher.set_query(&self.store, text);
        self.launch.resolve = true;
        true
    }

    pub fn menu_activate(&mut self, index: usize, alternate: bool) -> bool {
        if !self.launcher.open {
            return false;
        }
        self.resolve_launcher();
        let out = if alternate { self.launcher.activate_alternate(&self.store, index) } else { self.launcher.activate(&self.store, index) };
        self.launcher_out(out);
        true
    }

    /// `menu actions`: row `index`'s actions beside the list, as Tab.
    pub fn menu_actions(&mut self, index: usize) -> Result<(), String> {
        if !self.launcher.open {
            return Err("menu not open".into());
        }
        self.resolve_launcher();
        self.launch.resolve = true;
        if self.launcher.open_actions(&self.store, index) { Ok(()) } else { Err(format!("row {index} has no actions")) }
    }

    /// `menu runAction`: the action `id` on row `index`.
    pub fn menu_run_action(&mut self, index: usize, id: &str) -> Result<(), String> {
        if !self.launcher.open {
            return Err("menu not open".into());
        }
        self.resolve_launcher();
        let out = self.launcher.run_action(&self.store, index, id)?;
        self.launcher_out(out);
        Ok(())
    }

    /// `menu into`: →, on row `index`.
    pub fn menu_into(&mut self, index: usize) -> Result<(), String> {
        if !self.launcher.open {
            return Err("menu not open".into());
        }
        self.resolve_launcher();
        let out = self.launcher.step_into(&self.store, index).ok_or_else(|| format!("row {index} has nothing to step into"))?;
        self.launcher_out(out);
        Ok(())
    }

    /// The rows again, now, so an IPC read answers off the same state a
    /// frame would draw.
    pub fn resolve_launcher(&mut self) {
        self.launcher_panel_sync();
        if self.launch.resolve {
            self.launch.resolve = false;
            self.launcher.motion_scale = self.motion_scale;
            self.launcher.resolve(&self.store, &self.store.theme.theme, &mut self.kit);
            self.launcher_asks();
        }
    }

    /// What the rows just resolved need asked of the services.
    fn launcher_asks(&self) {
        if let Some(q) = self.launcher.install_query() {
            crate::services::packages::search_for(&self.store, q);
        }
    }

    fn launcher_out(&mut self, out: Out) {
        self.launch.resolve = true;
        match out {
            Out::None => {}
            Out::Close => self.hide_launcher(),
            // A row's own verb runs inside the launcher's session (a
            // launcher plugin hears a line only while the launcher is open);
            // the App's may open the launcher again, so it closes first.
            Out::Intent(intent, subject, keep_open) => {
                let close = !keep_open && self.launcher.open;
                if intent.row_only() {
                    self.run_intent(&intent, Some(&subject));
                }
                if close {
                    self.menu_close();
                }
                if !intent.row_only() {
                    self.run_intent(&intent, Some(&subject));
                }
            }
            Out::Target(pick) => {
                self.hide_launcher();
                self.run_target(pick);
            }
            Out::Spawn(argv) => {
                if !self.launcher.open {
                    self.hide_launcher();
                }
                crate::services::compositor::spawn(&argv);
            }
            Out::Call(call) => {
                if self.launcher.open {
                    return self.run_call(&call);
                }
                self.hide_launcher();
                if crate::ipc::reads_screen(&call) && self.launch.win.is_some() {
                    self.launcher.after_unmap = Some(call);
                } else {
                    self.run_call(&call);
                }
            }
        }
    }

    fn run_target(&self, pick: Pick) {
        use crate::services::{compositor, launch, localsend};
        match pick {
            Pick::Open { id, argv } => launch::spawn(&self.store, &id, &argv),
            Pick::Send { peer, payload } => localsend::command(localsend::Cmd::Send { peer, payload }),
            Pick::Move { windows, idx, output } => {
                for id in windows {
                    compositor::send(compositor::Command::MoveWindowTo { id, idx, output: output.clone() });
                }
            }
        }
    }

    /// What the model asked of the services since the last call.
    fn launcher_requests(&mut self) {
        use crate::services::{clipboard, localsend, mirror, packages, picker, plugins};
        for request in self.launcher.take_requests() {
            match request {
                Request::Index(ask) => index::ask(ask),
                Request::Mirror(cmd) => mirror::command(cmd),
                Request::Picker(cmd) => picker::command(cmd),
                Request::Packages(cmd) => packages::send(cmd),
                Request::PackageSearch(q) => packages::search_for(&self.store, &q),
                Request::Plugin(id, line) => plugins::send(&id, line),
                Request::Plugins(open) => plugins::launcher(open),
                Request::PeerScan => localsend::command(localsend::Cmd::Scan { force: false }),
                Request::Clipboard(cmd) => clipboard::command(cmd),
                Request::Selected(answer) => {
                    write_selection(&crate::surfaces::launcher::selection_path(), &answer);
                    self.selection_answer(answer);
                }
                Request::SelectionCleared => {
                    let _ = std::fs::remove_file(crate::surfaces::launcher::selection_path());
                }
                Request::PickerSelected(answer) => write_selection(&crate::surfaces::launcher::picker_selection_path(), &answer),
                Request::Wallpaper(choice, mode) => crate::services::state::set_wallpaper(choice, mode),
                Request::Mode(mode) => {
                    let _ = crate::services::theme::set_mode(&self.store, crate::services::theme::Ask::Set(mode));
                }
            }
        }
    }

    /// A configured `@ipc:` call, answered in process.
    pub(super) fn run_call(&mut self, call: &IpcCall) {
        if let Err(why) = crate::ipc::call(self, call) {
            eprintln!("Menu: {}.{}: error: {why}", call.target, call.function);
        }
    }

    /// A launcher verb, run for the launcher's row of `subject`, or with no
    /// row for a hot corner, which never holds a row's own
    /// ([`Intent::row_only`]).
    pub(super) fn run_intent(&mut self, intent: &Intent, subject: Option<&SubjectId>) {
        use crate::services::compositor::{self, Command};
        use crate::services::devices::{audio, bluetooth, network as net};
        use crate::services::radio::{self, Cmd};
        use crate::services::{clipssh, launch, packages, plugins, power};
        match intent {
            Intent::AbbrevForget(q) | Intent::AbbrevPin(q) => {
                if let Some(subject) = subject {
                    index::set_abbreviation(&self.store, q, &subject.to_string(), matches!(intent, Intent::AbbrevPin(_)));
                }
            }
            Intent::RecentPin(id) | Intent::RecentUnpin(id) => index::set_recent_pinned(&self.store, id, matches!(intent, Intent::RecentPin(_))),
            Intent::PluginSend(id, line) => plugins::send(id, line.clone()),
            Intent::WindowFocus(id) => compositor::focus_window(&WindowId::new(id)),
            Intent::WindowClose(id) => compositor::close_window(&WindowId::new(id)),
            Intent::WindowFloat(id) => compositor::send(Command::ToggleFloating(WindowId::new(id))),
            Intent::WindowFullscreen(id) => compositor::send(Command::ToggleFullscreen(WindowId::new(id))),
            Intent::AppOpen(id) => launch::open(&self.store, id),
            Intent::AppNew(id) => {
                if let Some(argv) = launch::app_argv(&self.store.menu, id) {
                    launch::spawn(&self.store, id, &argv);
                }
            }
            Intent::AppAction(id, key) => {
                if let Some(argv) = launch::action_argv(&self.store.menu, id, key) {
                    launch::spawn(&self.store, id, &argv);
                }
            }
            Intent::WallpaperSet(path) => {
                self.picker_choose(path);
            }
            Intent::WallpaperSetFor(half, path) => {
                if let Some(half) = fs_theme::theme::Mode::parse_exact(half) {
                    self.picker_choose_for(path, half);
                }
            }
            Intent::EndProcess(pid) => self.run_call(&IpcCall::new("monitor", "kill").arg(pid.to_string()).arg("TERM")),
            Intent::WifiEnable => net::send(net::Cmd::Wifi(true)),
            Intent::WifiForget(ssid) => net::send(net::Cmd::Forget(ssid.clone())),
            Intent::WifiReveal(ssid) => {
                self.launch.wifi_reveals += 1;
                let request = self.launch.wifi_reveals;
                self.launch.wifi_reveal = Some((request, self.launcher.sessions));
                net::send(net::Cmd::Reveal(ssid.clone(), request));
            }
            Intent::WifiActivate(ssid) => self.wifi_activate(ssid),
            Intent::BluetoothPower(on) => {
                if on == "on" {
                    bluetooth::send(bluetooth::Cmd::Power(true));
                }
            }
            Intent::BluetoothForget(address) => {
                if let Some(address) = fs_types::BtAddress::parse(address) {
                    bluetooth::send(bluetooth::Cmd::Run(address, bluetooth::ActionKind::Forget));
                }
            }
            Intent::BluetoothToggle(address) => {
                let Some(address) = fs_types::BtAddress::parse(address) else { return };
                let Some(d) = self.store.bluetooth.devices.iter().find(|d| d.address == address) else { return };
                let kind = if d.connected { bluetooth::ActionKind::Disconnect } else { bluetooth::ActionKind::Connect };
                bluetooth::send(bluetooth::Cmd::Run(address, kind));
            }
            Intent::AudioSink(name) => audio::send(audio::Cmd::Default { source: false, name: name.clone() }),
            Intent::AudioSource(name) => audio::send(audio::Cmd::Default { source: true, name: name.clone() }),
            Intent::RadioStop => radio::send(Cmd::Stop),
            Intent::RadioFav(uuid) | Intent::RadioPlay(uuid) | Intent::RadioFavorite(uuid) | Intent::RadioUnfavorite(uuid) => {
                let r = &self.store.media.radio;
                let results = r.search.as_ref().and_then(|(_, f)| f.clone()).unwrap_or_default();
                let Some(s) = r.favorites.iter().chain(results.iter()).find(|s| s.uuid == *uuid).cloned() else { return };
                match intent {
                    Intent::RadioFav(_) => radio::send(Cmd::PlayFromSaved(s, r.favorites.clone())),
                    Intent::RadioPlay(_) => radio::send(Cmd::PlayFromSaved(s, results)),
                    _ => {
                        let unfavorite = matches!(intent, Intent::RadioUnfavorite(_));
                        if r.favorites.iter().any(|f| f.uuid == s.uuid) == unfavorite {
                            radio::send(Cmd::ToggleFavorite(s));
                        }
                    }
                }
            }
            Intent::LocalsendSend(peer, id) => self.localsend_entry(peer, id),
            Intent::NixRun(package) => self.console_run_once(&format!("nix run nixpkgs#{package}; read")),
            Intent::ClipsshImage(id, path) => self.clipssh_image(id, path),
            Intent::ClipsshSend(alias) => clipssh::command(clipssh::Cmd::Send(alias.clone())),
            Intent::AppGpu(id) => {
                if let Err(why) = crate::services::launch::on_gpu(&self.store, id, "") {
                    eprintln!("Menu: launch {id} on the GPU: {why}");
                }
            }
            Intent::PackageInstall(id) => packages::install(&self.store, id),
            Intent::PackageUninstall(id) => packages::send(packages::Cmd::Uninstall(id.clone())),
            Intent::PowerSuspend => power::request(power::Action::Suspend),
            Intent::PowerReboot => power::request(power::Action::Reboot),
            Intent::PowerOff => power::request(power::Action::PowerOff),
            Intent::ThemeToggleMode => {
                let _ = crate::services::theme::set_mode(&self.store, crate::services::theme::Ask::Toggle);
            }
            Intent::LightsColorInput => self.menu_input_as("Hex color (ff8800)", LIGHTS_COLOR, false),
            Intent::ReminderSet => self.menu_input_as("Reminder (25m coffee)", REMINDER_SET, false),
        }
    }

    /// A clipboard image (entry `id`, its file at `path`) over ssh rather
    /// than onto the clipboard: straight there with an alias resolved, else
    /// copied and the alias route asked.
    fn clipssh_image(&mut self, id: &str, path: &str) {
        use crate::services::clipssh;
        let configured = crate::settings::CLIPSSH_ALIAS.get(&self.store.config);
        match clipssh::resolve_alias(configured, &self.store.clipssh.aliases) {
            Some(alias) => clipssh::command(clipssh::Cmd::SendImage(alias, path.to_owned())),
            None => {
                crate::services::clipboard::command(crate::services::clipboard::Cmd::Copy(id.to_owned()));
                self.menu_open(Some("clipssh"));
            }
        }
    }

    fn lights_answer(&mut self, value: Option<String>, cancelled: bool) {
        let value = value.unwrap_or_default();
        if cancelled {
            return;
        }
        if crate::services::lights::colour(&self.store.lights, &value).is_err() {
            let body = format!("\"{value}\" is not a hex colour like ff8800");
            self.apply(Op::Notify("Keyboard Lights".into(), body, fs_info::notifications::Urgency::Normal));
        }
    }

    /// "25m coffee" set, and a toast either way.
    fn reminder_answer(&mut self, value: Option<String>, cancelled: bool) {
        use fs_info::notifications::Urgency;
        let value = value.unwrap_or_default();
        if cancelled {
            return;
        }
        let Some(spec) = fs_info::reminders::parse_spec(&value) else {
            self.apply(Op::Notify("Reminder".into(), format!("Could not read \"{value}\" as a duration"), Urgency::Normal));
            return;
        };
        let message = notifications::reminder_message(&spec.message, crate::settings::REMINDER_MESSAGE.get(&self.store.config));
        self.apply(Op::SetReminder { seconds: spec.seconds, message });
        if let Some(e) = self.store.notifications.newest_reminder() {
            let body = format!("{} at {}", e.message, fs_info::reminders::due_clock(e.due_at));
            self.apply(Op::Notify("Reminder set".into(), body, Urgency::Normal));
        }
    }

    /// A Wi-Fi row's Enter: connected, it disconnects; a secured network
    /// nobody knows, or one whose last attempt failed on the secret, asks
    /// for it, since a wrong password is retyped, never retried.
    fn wifi_activate(&mut self, ssid: &str) {
        use crate::services::devices::network as net;
        let n = &self.store.network;
        let Some(row) = n.row(ssid).cloned() else { return };
        if n.action.is_some() {
            return;
        }
        let ssid = ssid.to_owned();
        if row.connected {
            return net::send(net::Cmd::Disconnect(ssid));
        }
        let retype = n.failure.as_ref().is_some_and(|f| f.ssid == ssid && f.secret);
        if retype || (row.secured && !row.known) {
            let (prompt, token) = if row.enterprise {
                (format!("Identity for {ssid}"), WIFI_IDENTITY)
            } else {
                (format!("Password for {ssid}"), WIFI_PASSWORD)
            };
            self.launch.wifi_pending = Some((ssid, String::new()));
            self.menu_input_as(&prompt, token, token == WIFI_PASSWORD);
        } else {
            net::send(net::Cmd::Connect(ssid, net::Secret::Saved));
        }
    }

    /// An answer whose token an in-process owner holds.
    fn selection_answer(&mut self, answer: Selected) {
        let Selected { token, value, cancelled, .. } = answer;
        if token == WIFI_IDENTITY || token == WIFI_PASSWORD {
            self.wifi_answer(&token, value, cancelled);
        } else if token == WIFI_SHOWN {
            if !cancelled {
                self.menu_open(Some("wifi"));
            }
        } else if token == REMINDER_SET {
            self.reminder_answer(value, cancelled);
        } else if token == LIGHTS_COLOR {
            self.lights_answer(value, cancelled);
        } else if token == ABBREV_EDIT {
            self.launcher.edit_answer(&self.store, value, cancelled);
        }
    }

    /// Show password's answer: the passphrase in the secret step, drawn in
    /// the clear, or why there is none. Dropped once the session it was
    /// asked in has closed.
    fn wifi_revealed(&mut self, r: crate::services::devices::network::Revealed) {
        if !self.launcher.open || self.launch.wifi_reveal != Some((r.request, self.launcher.sessions)) {
            return;
        }
        self.launch.wifi_reveal = None;
        match r.secret {
            Ok(Some(psk)) => {
                self.launcher.open_shown(&format!("Password for {}", r.ssid), WIFI_SHOWN, &psk);
                self.launch.resolve = true;
                self.show_launcher();
            }
            Ok(None) => self.menu_input_as(&format!("No password saved for {}", r.ssid), WIFI_SHOWN, true),
            Err(_) => self.menu_input_as(&format!("Could not read the password for {}", r.ssid), WIFI_SHOWN, true),
        }
    }

    /// The password step answered, then the launcher back on the Wi-Fi list.
    fn wifi_answer(&mut self, token: &str, value: Option<String>, cancelled: bool) {
        use crate::services::devices::network as net;
        let Some((ssid, identity)) = self.launch.wifi_pending.take() else { return };
        let value = value.unwrap_or_default();
        if cancelled || value.is_empty() {
            return;
        }
        if token == WIFI_IDENTITY {
            self.launch.wifi_pending = Some((ssid.clone(), value));
            self.menu_input_as(&format!("Password for {ssid}"), WIFI_PASSWORD, true);
            return;
        }
        let Some(row) = self.store.network.row(&ssid) else { return };
        let secret = if !identity.is_empty() && row.enterprise {
            net::Secret::Eap { identity, password: value }
        } else {
            net::Secret::Psk(value)
        };
        net::send(net::Cmd::Connect(ssid, secret));
        self.menu_open(Some("wifi"));
    }

    /// The instant paste: the chord typed into whatever
    /// focus returns to, a settle after the window is gone so it never lands
    /// in the launcher's own field.
    fn paste_chord(&mut self) {
        let chord = crate::settings::PASTE_CHORD.get(&self.store.config).to_owned();
        let Some(keys) = fs_menu::providers::paste_argv(&chord) else {
            eprintln!("Menu: clipboard.pasteChord is not a wtype chord: {chord} - copied but not pasted");
            return;
        };
        let mut argv = crate::services::proc::argv(&["sh", "-c", "command -v wtype >/dev/null 2>&1 || exit 127; exec wtype \"$@\"", "sh"]);
        argv.extend(keys);
        if let Some(rt) = &self.runtime {
            rt.service(move |ctx| {
                ctx.spawn(async move {
                    async_io::Timer::after(std::time::Duration::from_millis(150)).await;
                    let done = crate::services::proc::capture(&argv, std::time::Duration::from_secs(5)).await;
                    match done.code {
                        0 => {}
                        127 => eprintln!("Menu: wtype not on PATH, copied but not pasted"),
                        c => eprintln!("Menu: wtype failed (exit {c}), copied but not pasted"),
                    }
                });
            });
        }
    }

    /// The clipboard route's rows off the ledger.
    pub(super) fn launcher_clipboard(&mut self) {
        if std::mem::take(&mut self.store.clipboard.captured) {
            use crate::services::clipssh;
            clipssh::command(clipssh::Cmd::AutoImage {
                configured: crate::settings::CLIPSSH_ALIAS.get(&self.store.config).to_owned(),
                enabled: crate::settings::CLIPSSH_AUTO_SEND.get(&self.store.config),
            });
        }
        let c = &self.store.clipboard;
        let paste = crate::settings::CLIPBOARD_PASTE.get(&self.store.config);
        let rows = fs_menu::providers::clipboard_provider(&c.items, fs_menu::providers::ClipMode::Copy, paste);
        let shared = fs_menu::providers::clipboard_provider(&c.items, fs_menu::providers::ClipMode::Share, paste);
        self.apply(Diff::Menu(index::Diff::Source(Provider::Clipboard, rows)));
        self.apply(Diff::Menu(index::Diff::Source(Provider::ShareHistory, shared)));
    }

    /// The picker grid's pictures, asked for once the listing's thumbnails
    /// are in the cache (or the warm gave up on them).
    pub(super) fn launcher_picker(&mut self) {
        if self.launcher.open && self.launcher.on_picker() {
            use crate::services::picker::{self, Px};
            let theme = &self.store.theme.theme;
            let (w, h) = self.launcher.picker_tile_px(theme);
            let px = Px { w, h, radius: theme.radii.lg, scale: self.launcher_scale() };
            let p = &self.store.picker;
            let sky = crate::services::wallpaper::sky_source(self.store.sky_step());
            let want: Vec<String> = self
                .launcher
                .picker_tiles(&self.store)
                .into_iter()
                .filter(|(_, item)| p.warm(item) || !p.cached.is_empty() || matches!(item.kind, fs_menu::providers::ItemKind::Sky | fs_menu::providers::ItemKind::Mode { .. }))
                .map(|(_, item)| picker::tile_key(&item, &sky))
                .filter(|key| px != p.thumb_px || !p.thumbs.contains_key(key))
                .collect();
            if !want.is_empty() {
                picker::command(picker::Cmd::Thumbs(want, px));
            }
        }
        self.launcher_store_changed();
    }

    pub fn picker_summon(&mut self) {
        self.menu_open(Some(crate::surfaces::launcher::PICKER_ROUTE));
        self.launcher_picker();
    }

    pub fn picker_select(&mut self, dir: &str, token: &str) {
        self.launcher.open_image_select(&self.store, dir, token);
        self.show_launcher();
        self.launcher_picker();
    }

    pub fn picker_choose(&mut self, path: &str) -> bool {
        let chosen = self.launcher.choose_image(&self.store, path);
        if chosen {
            self.hide_launcher();
        }
        chosen
    }

    pub fn picker_choose_for(&mut self, path: &str, half: fs_theme::theme::Mode) -> bool {
        let chosen = self.launcher.choose_for(&self.store, path, half);
        if chosen {
            self.hide_launcher();
        }
        chosen
    }

    /// Sends a ledger entry to the peer at
    /// `index` of the last scan.
    fn localsend_entry(&mut self, index: &str, id: &str) {
        use crate::services::localsend::{self, Cmd, Payload};
        use fs_info::notifications::Urgency;
        let peer = index.parse::<usize>().ok().and_then(|i| self.store.localsend.peers.get(i)).cloned();
        let Some(peer) = peer else {
            self.apply(Op::Notify("LOCALSEND FAILED".into(), "That device is no longer in range, rescan and try again".into(), Urgency::Critical));
            return;
        };
        let Some(entry) = self.store.clipboard.items.iter().find(|e| e.id == id) else { return };
        localsend::command(Cmd::Send { peer, payload: Payload::of(entry) });
    }

    /// What the preview pane needs for the selected subject, asked for
    /// once: a file or folder's peek, a picture decoded at the pane's size,
    /// or one captured frame of a window. The window's capture is held only
    /// while the pane shows it, and dropped the moment it does not.
    fn launcher_preview(&mut self) {
        use crate::surfaces::launcher::{PanePicture, PaneKind, pane_kind, pane_picture};
        use crate::wayland::capture::Owner as Capture;
        let row = self.launcher.open.then(|| self.launcher.pane().cloned()).flatten();
        if !self.launcher.open {
            self.launch.peek_asked = None;
            self.launch.info_asked = None;
        }
        let theme = &self.store.theme.theme;
        let (_, width, height) = self.launcher.pane_extent(theme);
        let size = (crate::surfaces::launcher::preview_box(theme, &mut self.kit, width, height), self.launcher_scale());
        if let Some(n) = row.as_ref().filter(|n| matches!(pane_kind(n), Some(PaneKind::File | PaneKind::Folder)))
            && self.launch.peek_asked.as_deref() != Some(n.path.as_str())
        {
            self.launch.peek_asked = Some(n.path.clone());
            index::ask(Ask::Peek(n.path.clone()));
        }
        // An Install row's details, asked once per row shown, and its
        // screenshot decoded at the pane's width once the file is cached.
        if let Some(n) = row.as_ref().filter(|n| matches!(pane_kind(n), Some(PaneKind::Package))) {
            if self.launch.info_asked.as_deref() != Some(n.path.as_str()) {
                self.launch.info_asked = Some(n.path.clone());
                crate::services::packages::send(crate::services::packages::Cmd::Info(n.path.clone()));
            }
            let p = &self.store.packages;
            let shot = p.info.get(&n.path).and_then(|i| i.as_ref().ok()).and_then(|i| i.screenshot.as_ref()).and_then(|u| p.shots.get(u)).cloned().flatten();
            if let Some(path) = shot {
                let size = (crate::surfaces::launcher::shot_box(width), size.1);
                let have = self.store.clipboard.preview.as_ref().is_some_and(|p| p.path == path && p.size == size);
                if !have && self.launch.preview_asked.as_ref() != Some(&(path.clone(), size)) {
                    self.launch.preview_asked = Some((path.clone(), size));
                    crate::services::clipboard::command(crate::services::clipboard::Cmd::Preview(path, size));
                }
            }
        }
        let mut window = None;
        match row.as_ref().and_then(pane_picture) {
            Some(PanePicture::Decoded(path)) if height > 0.0 => {
                let have = self.store.clipboard.preview.as_ref().is_some_and(|p| p.path == path && p.size == size);
                if !have && self.launch.preview_asked.as_ref() != Some(&(path.to_owned(), size)) {
                    self.launch.preview_asked = Some((path.to_owned(), size));
                    crate::services::clipboard::command(crate::services::clipboard::Cmd::Preview(path.to_owned(), size));
                }
            }
            Some(PanePicture::Window(id)) if height > 0.0 => window = Some(WindowId::new(id)),
            _ => {}
        }
        match &window {
            Some(id) => {
                let from = self.store.compositor.window(id).and_then(|w| w.rect.as_ref()).map_or((0.0, 0.0), |r| (f64::from(r.w), f64::from(r.h)));
                let fitted = crate::surfaces::launcher::fit_inside(from, size.0);
                self.capture_set(Capture::Launcher, &[(id.clone(), fitted)], false, size.1);
            }
            None => self.capture_clear(Capture::Launcher),
        }
        let frame = window.as_ref().and_then(|id| self.capture_image(Capture::Launcher, id)).cloned();
        let same = match (&frame, &self.launcher.pane_frame) {
            (Some(a), Some(b)) => std::sync::Arc::ptr_eq(&a.pixmap, &b.pixmap),
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.launcher.pane_frame = frame;
            self.launcher.dirty = true;
        }
    }

    /// The mirror's stream follows the view: the current camera while it
    /// shows, nothing otherwise.
    pub(super) fn launcher_mirror(&mut self) {
        use crate::services::mirror::{self, Cmd};
        if self.launcher.open && self.launcher.on_mirror() && self.store.mirror.listed {
            let cams = &self.store.mirror.cameras;
            let current = fs_system::camera::pick(cams, &self.launcher.mirror_current);
            let ir = cams.iter().find(|c| c.id == current).is_some_and(|c| c.ir);
            self.launcher.mirror_current = current.clone();
            let (w, h) = self.launcher.mirror_box(&self.store.theme.theme);
            mirror::set_box(w as u32, h as u32, self.launcher_scale());
            mirror::command(Cmd::Stream((!current.is_empty()).then_some((current, ir))));
        }
        self.launcher_store_changed();
    }

    pub fn mirror_showing(&self) -> bool {
        self.launcher.open && self.launcher.on_mirror() && self.launch.win.as_ref().is_some_and(|w| w.shown.modal.open)
    }

    pub fn mirror_cycle(&mut self, delta: i64) -> bool {
        if !self.mirror_showing() {
            return false;
        }
        self.launcher.mirror_cycle(&self.store, delta);
        self.launcher_mirror();
        true
    }

    /// The launcher's window, while it is open.
    fn launcher_shown(&self) -> Option<&Shown> {
        self.launch.win.as_ref().filter(|w| w.shown.modal.open).map(|w| &w.shown)
    }

    /// `menu status`: the model's report and the open window's boxes.
    pub fn launcher_status(&self) -> serde_json::Value {
        let mut v = self.launcher.status(&self.store);
        let rect = |r: fs_types::IRect| serde_json::json!(r);
        let shown = self.launcher_shown();
        if let Some(r) = shown.and_then(|w| w.body_rect) {
            v["body"] = rect(r);
        }
        if let Some(r) = shown.and_then(|w| w.field_box()) {
            v["fieldBox"] = rect(r);
        }
        if let Some(r) = shown.and_then(|w| w.token_rect()) {
            v["tokenBox"] = rect(r);
        }
        let (hover, action_hover) = shown.map_or((None, None), |w| w.hovered());
        v["hover"] = hover.map_or(-1, |i| i as i64).into();
        v["actionHover"] = action_hover.map_or(-1, |i| i as i64).into();
        if let Some(r) = shown.and_then(|w| w.column_box()) {
            v["actionsBox"] = rect(r);
        }
        if let Some(r) = shown.and_then(|w| w.pane_box()) {
            v["paneBox"] = rect(r);
        }
        if let Some(w) = shown {
            v["cardBox"] = rect(w.card_box());
            v["rowBoxes"] = w.row_boxes().into_iter().map(|(i, r)| keyed("index", i.into(), r)).collect();
        }
        v["footHints"] = shown
            .map_or_else(Vec::new, |w| w.foot_hints())
            .into_iter()
            .map(|(on, r)| keyed("on", on.into(), r))
            .collect();
        v
    }

    /// `mirror status`.
    pub fn mirror_status(&self) -> serde_json::Value {
        let mi = &self.store.mirror;
        let showing = self.mirror_showing();
        let current = if showing { self.launcher.mirror_current.as_str() } else { "" };
        let streaming = showing && mi.streaming.as_deref() == Some(current) && !current.is_empty();
        let rect = |r: fs_types::IRect| serde_json::json!(r);
        let feed = self.launch.win.as_ref().filter(|_| showing).and_then(|w| w.shown.feed);
        serde_json::json!({
            "showing": showing,
            "open": showing,
            "streaming": streaming,
            "hasFrame": streaming && mi.has_frame(current),
            "irFilter": streaming && mi.cameras.iter().any(|c| c.id == current && c.ir),
            "error": mi.error,
            "current": current,
            "feed": feed.map(|(f, _)| rect(f)),
            "picture": feed.and_then(|(_, p)| p).filter(|_| streaming && mi.has_frame(current)).map(rect),
            "cameras": if showing { mi.cameras.iter().map(|c| serde_json::json!({"id": c.id, "ir": c.ir, "label": c.label})).collect::<Vec<_>>() } else { Vec::new() },
        })
    }

    /// The clipssh route's rows off the saved aliases.
    pub(super) fn launcher_clipssh(&mut self) {
        let rows = fs_menu::providers::clipssh_rows(&self.store.clipssh.aliases);
        self.apply(Diff::Menu(index::Diff::Source(Provider::Clipssh, rows)));
    }

    /// The device routes' rows, again from the store.
    pub(super) fn launcher_devices(&mut self) {
        if let Some(r) = self.store.revealed.take() {
            self.wifi_revealed(r);
        }
        for (name, rows) in index::device_sources(&self.store) {
            self.apply(Diff::Menu(index::Diff::Source(name, rows)));
        }
        if self.launcher.open {
            self.launcher_store_changed();
        }
    }

    /// The plugin modes' rows off what their plugins print, and any
    /// `actions` answers into the open column.
    pub(super) fn launcher_plugins(&mut self) {
        for (route, rows) in index::plugin_sources(&self.store) {
            self.apply(Diff::Menu(index::Diff::Source(route, rows)));
        }
        if self.launcher.plugin_answers(&self.store) {
            self.launcher_store_changed();
        }
    }

    /// `menu refresh`: the base and the apps read again.
    pub fn menu_refresh(&mut self) {
        self.launch.buttons = None;
        self.launch.launches = None;
        self.launcher_inputs();
    }

    /// The slices its index is built from, the rows it resolves off, the
    /// monitor view's samples, and what a panel it holds declares.
    pub(super) fn launcher_reads(&self, topic: Topic) -> bool {
        let held = self.launcher.open && self.launcher.panel.as_ref().is_some_and(|p| p.module.reads().contains(&topic));
        held || super::owner::DEVICES.contains(&topic)
            || INFO.contains(&topic)
            || matches!(
                topic,
                Topic::Menu
                    | Topic::Packages
                    | Topic::Config
                    | Topic::State
                    | Topic::Plugins
                    | Topic::PluginOutput
                    | Topic::Theme
                    | Topic::Media
                    | Topic::Tray
                    | Topic::Notifications
                    | Topic::Clipboard
                    | Topic::Localsend
                    | Topic::Activity
                    | Topic::Mirror
                    | Topic::Picker
                    | Topic::Clipssh
                    | Topic::Lights
                    | Topic::NightLight
                    | Topic::Ipad
            )
    }

    /// What a topic it reads asks of it: its index's inputs and sources,
    /// the rows again, and a redraw for a theme or for what a panel it
    /// holds reads.
    pub(super) fn launcher_changed(&mut self, topic: Topic) {
        match topic {
            Topic::Menu => self.launcher_store_changed(),
            Topic::Packages => {
                self.launcher.owner_answer(&self.store);
                self.launcher_store_changed();
            }
            Topic::Config => self.launcher_inputs(),
            Topic::State => {
                self.launcher_inputs();
                // The picker's Style previews show the slots.
                self.launcher_picker();
            }
            Topic::Plugins => {
                self.launcher_inputs();
                self.launcher_plugins();
            }
            Topic::PluginOutput => self.launcher_plugins(),
            Topic::Tray => self.launcher_tray(),
            Topic::Notifications => self.launcher_store_changed(),
            Topic::Clipboard => {
                self.launcher_clipboard();
                self.launcher_inputs();
            }
            Topic::Localsend | Topic::Lights | Topic::NightLight | Topic::Ipad => {
                self.launcher_inputs();
                self.launcher_store_changed();
            }
            Topic::Activity => {
                if self.launcher.level.as_deref() == Some(fs_menu::providers::INSTALL_ROUTE) {
                    self.launcher_store_changed();
                }
            }
            Topic::Mirror => self.launcher_mirror(),
            Topic::Picker => self.launcher_picker(),
            Topic::Clipssh => self.launcher_clipssh(),
            Topic::Media => self.launcher_devices(),
            t if super::owner::DEVICES.contains(&t) => self.launcher_devices(),
            t if INFO.contains(&t) && self.launcher.open && self.launcher.level.as_deref() == Some(crate::surfaces::launcher::MONITOR_ROUTE) => {
                self.launcher.monitor_sample(&self.store);
                self.launcher_store_changed();
            }
            _ => {}
        }
        // Settings, plugins and a theme lay a held panel out again whatever
        // it reads; a theme restyles the card it is in.
        let held = self.launcher.open
            && self.launcher.panel.as_ref().is_some_and(|p| p.module.reads().contains(&topic) || matches!(topic, Topic::Config | Topic::Plugins | Topic::Theme));
        if held || (topic == Topic::Theme && self.launch.win.is_some()) {
            self.launcher.dirty = true;
        }
    }

    /// The store moved under an open launcher: the rows again.
    pub(crate) fn launcher_store_changed(&mut self) {
        if self.launcher.open {
            self.launch.resolve = true;
        } else {
            self.launch.warm = true;
        }
    }

    /// The rows' words shaped on the pool in every style the launcher last
    /// drew in (the theme's row styles before its first open), so an open
    /// finds them in the shared cache instead of shaping on this thread.
    fn warm_launcher_text(&mut self) {
        let Some(runtime) = &self.runtime else { return };
        let styles = if self.launch.styles.is_empty() { warm_styles(&self.kit.look, &self.store.theme.theme.font_size) } else { self.launch.styles.clone() };
        let words: Vec<String> = self
            .launcher
            .rows
            .iter()
            .flat_map(|n| [n.label.clone(), n.desc.clone().unwrap_or_default()])
            .filter(|w| !w.is_empty())
            .collect();
        let jobs: Vec<(String, TextStyle)> = styles
            .iter()
            .filter(|st| !matches!(st.family, Family::Named(_)))
            .flat_map(|st| words.iter().map(move |w| (w.clone(), *st)))
            .collect();
        let text = self.kit.text.clone();
        runtime.pool().submit(move || text.warm(&jobs));
    }

    /// Every face and size the launcher draws its words in, warmed once the
    /// bar is up with the glyphs most rows use, a glyph per job so the
    /// shaper's lock is never held for more than one outline. Glyph by
    /// glyph across every style, so each face exists after the first round.
    pub(super) fn warm_faces(&mut self) {
        let Some(runtime) = &self.runtime else { return };
        let sample = "aABCDEFGHIJKLMNOPQRSTUVWXYZbcdefghijklmnopqrstuvwxyz0123456789.,:;-_/()'";
        let styles = warm_styles(&self.kit.look, &self.store.theme.theme.font_size);
        let jobs: Vec<(String, TextStyle)> =
            sample.chars().flat_map(|c| styles.iter().map(move |st| (c.to_string(), *st))).collect();
        let text = self.kit.text.clone();
        runtime.pool().submit(move || text.warm(&jobs));
    }

    /// The launcher's frame, its scrim on the card's own pose.
    /// The scale the launcher's pictures are made at: its card's, else the
    /// bar's output's, where it opens.
    fn launcher_scale(&self) -> f64 {
        self.launch.win.as_ref().map_or_else(|| self.store.display.scales.primary(), |w| w.shown.modal.win.surface.scale())
    }

    /// The model's asks go out on either side of the present, which can
    /// resolve the model again.
    pub(super) fn present_launcher(&mut self, now: Instant) {
        self.launcher_requests();
        self.present_launcher_window(now);
        self.launcher_requests();
    }

    fn present_launcher_window(&mut self, now: Instant) {
        if let Some(w) = &mut self.launch.win {
            let want = self.launcher.open && w.shown.modal.open && self.launcher.level.as_deref() == Some("wifi");
            if want != w.scan.is_some() {
                w.scan = want.then(|| crate::services::wants::Want::new(crate::services::wants::Source::WifiScan));
            }
        }
        if self.launch.win.as_ref().is_some_and(|w| w.finished(now)) {
            self.drop_launch();
            self.sync_join();
            crate::trace::mapped(crate::trace::Surface::Menu, crate::trace::State::Unmapped);
            if std::mem::take(&mut self.launcher.paste) && !self.launcher.open {
                self.paste_chord();
            }
            // A screen grab waits until the compositor has taken the card
            // down: a sync sent after the teardown is answered once it has.
            if let Some(call) = self.launcher.after_unmap.take() {
                let id = self.launcher.grab.hold(call);
                self.display.sync(&self.qh, AfterUnmap(id));
                if let Some(handle) = &self.handle {
                    let _ = handle.insert_source(Timer::from_duration(GRAB_WAIT), move |_, _, app: &mut App| {
                        if let Some(call) = app.launcher.grab.take(id) {
                            eprintln!("Launcher: dropped {}.{}: the compositor did not answer the sync within {GRAB_WAIT:?}", call.target, call.function);
                        }
                        TimeoutAction::Drop
                    });
                }
            }
            return;
        }
        if self.launch.win.is_none() {
            if self.launch.warm && !self.launcher.open {
                self.launch.warm = false;
                self.launcher.resolve(&self.store, &self.store.theme.theme, &mut self.kit);
                self.launcher_asks();
                self.warm_launcher_text();
                if !self.launch.fresh_asked && !self.store.menu.nodes().is_empty() {
                    self.launch.fresh_asked = true;
                    self.launcher.ask_fresh(&self.store);
                }
            }
            return;
        }
        let t0 = Instant::now();
        self.resolve_launcher();
        self.launcher_preview();
        let resolved = t0.elapsed();
        let qh = self.qh.clone();
        let theme = &self.store.theme.theme;
        let Some(w) = &mut self.launch.win else { return };
        let moving = w.shown.content_animating(now) || self.launcher.rows_moving(now);
        let t1 = Instant::now();
        // A frame still being drawn in slices keeps the scene it started on.
        let rastering = w.shown.modal.win.surface.rastering() || w.shown.modal.layer.as_ref().is_some_and(|l| l.surface.rastering());
        let wants = (self.launcher.dirty || moving || !w.shown.modal.win.surface.mapped()) && !w.shown.hold_layout(&self.launcher, now);
        // A layout held back by a sliced frame runs once it is out, even
        // when whatever moved has stopped by then.
        if rastering && wants {
            self.launcher.dirty = true;
        }
        if !rastering && wants {
            w.shown.modal.sync_region(&self.compositor);
            self.kit.seen = Some(std::mem::take(&mut self.launch.styles));
            w.shown.layout(&mut self.launcher, &self.store, theme, &mut self.kit, now);
            self.launch.styles = self.kit.seen.take().unwrap_or_default();
            self.launcher.dirty = false;
        } else if !rastering {
            // Every turn, not only while the morph runs: its last step lands
            // after the morph has stopped running, and a no-op once there.
            // Never under a sliced frame: a card moved every turn damages
            // the whole card again, and a card one slice budget can't draw
            // never lands until the morph stops. The commit that ends the
            // frame brings the next turn.
            w.shown.place_card(theme, now);
        }
        let laid = t1.elapsed();
        let animating = w.shown.animating(now) || self.launcher.rows_moving(now);
        w.shown.modal.present(animating, now, &qh);
        // The rest of a sliced frame on the next turn, after whatever the
        // loop has waiting (the bar's callback first).
        if (w.shown.modal.win.surface.rastering()
            || w.shown.modal.layer.as_ref().is_some_and(|l| l.surface.rastering())
            || w.shown.modal.look_rastering())
            && let Some(handle) = &self.handle
        {
            handle.insert_idle(|_| {});
        }
        crate::trace::slow(
            "launcher",
            t0.elapsed(),
            format_args!("resolve_us={} layout_us={} total_us={}", resolved.as_micros(), laid.as_micros(), t0.elapsed().as_micros()),
        );
        if animating {
            self.sync_join();
        }
    }

    pub(super) fn launcher_part(&self, surface: &smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface) -> Option<Part> {
        self.launch.win.as_ref()?.shown.modal.part(surface)
    }

    pub(super) fn launcher_frame(&mut self, part: Part, now: Instant) {
        let Some(w) = &mut self.launch.win else { return };
        if w.shown.modal.frame(part, now) {
            self.launcher.dirty = true;
            self.sync_join();
        }
    }

    pub(super) fn launcher_configure(&mut self, part: Part, width: i32, height: i32) {
        let Some(w) = &mut self.launch.win else { return };
        let first = part == Part::Card && !w.shown.modal.win.surface.configured;
        w.shown.modal.configure(part, width, height);
        if part == Part::Card {
            self.launcher.dirty = true;
        }
        if first {
            crate::trace::mapped(crate::trace::Surface::Menu, crate::trace::State::Configured);
        }
    }

    pub(super) fn launcher_closed(&mut self) {
        self.drop_launch();
        self.launcher.close();
        self.sync_join();
    }

    /// One key while the launcher holds the keyboard. False when it is not
    /// up, so the key goes on to the panel.
    pub(super) fn launcher_key(&mut self, event: &KeyEvent, repeat: bool) -> bool {
        if !self.launcher.open || self.launch.win.as_ref().is_none_or(|w| !w.shown.modal.open) {
            return false;
        }
        let key = crate::input::launcher_key(event.keysym, self.mods.shift);
        if self.launcher.on_panel() {
            self.launcher_panel_key(event, repeat);
            return true;
        }
        let text = crate::input::printable(event.utf8.as_deref());
        self.resolve_launcher();
        let out = self.launcher.key(&self.store, key, crate::input::field_key(event.keysym), self.mods, repeat, text);
        if self.launcher.on_mirror() {
            self.launcher_mirror();
        }
        self.launch.resolve = true;
        self.launcher_out(out);
        true
    }

    /// The clipboard's text into the query, while the launcher is up.
    pub(super) fn launcher_paste(&mut self, text: &str) -> bool {
        if !self.launcher.open || self.launch.win.as_ref().is_none_or(|w| !w.shown.modal.open) {
            return false;
        }
        self.resolve_launcher();
        self.launcher.paste(&self.store, text);
        self.launch.resolve = true;
        true
    }

    /// The pointer over the launcher's window. True when it took the event.
    /// It tints what it hovers and never moves the selection (launcher
    /// spec §6).
    pub(super) fn launcher_pointer(&mut self, e: &PointerEvent, owner: Option<Target>) -> bool {
        if owner != Some(Target::Launcher(Part::Card)) {
            return false;
        }
        let Some(w) = &mut self.launch.win else { return true };
        let (x, y) = e.position;
        let on = |w: &Window| w.shown.hit(x, y).and_then(|h| h.on.map(String::from)).or_else(|| (!w.shown.modal.win.on_card(x, y)).then(|| "outside".into()));
        match e.kind {
            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                if w.shown.set_hover(w.shown.hit(x, y).map(|h| h.path)) {
                    self.launcher.dirty = true;
                    if let Some(p) = self.launcher.panel.as_mut() {
                        p.module.pointed(w.shown.hovered_stop().as_deref());
                    }
                }
            }
            PointerEventKind::Leave { .. } => {
                if w.shown.set_hover(None) {
                    self.launcher.dirty = true;
                    if let Some(p) = self.launcher.panel.as_mut() {
                        p.module.pointed(None);
                    }
                }
            }
            PointerEventKind::Press { button, .. } => w.pressed = on(w).map(|o| (o, button)),
            PointerEventKind::Release { button, .. } => {
                let pressed = w.pressed.take();
                let Some(on) = on(w).filter(|o| pressed.as_ref() == Some(&(o.clone(), button))) else { return true };
                if self.launcher.on_panel()
                    && let Some(e) = w.shown.panel_event(x, y, crate::input::button(button))
                {
                    if let Some(p) = self.launcher.panel.as_mut() {
                        let mut fx = crate::surfaces::panel::Effect::new(&self.store, self.runtime.as_ref());
                        p.module.event(&e, &mut fx);
                        p.cursor.take(p.module.as_mut(), w.shown.stops());
                        let outs = fx.outs();
                        self.launcher_panel_outs(outs, false);
                    }
                    return true;
                }
                if button == crate::input::BTN_RIGHT {
                    if let Some(i) = on.strip_prefix("row:").and_then(|i| i.parse::<usize>().ok()) {
                        self.resolve_launcher();
                        self.launcher.open_actions(&self.store, i);
                        self.launch.resolve = true;
                    } else if on == "outside" {
                        self.menu_close();
                    }
                    return true;
                }
                if button == crate::input::BTN_LEFT {
                    self.launcher_click(&on);
                }
            }
            PointerEventKind::Axis { time, vertical, source, .. } => {
                let ms = crate::motion::Kind::SpatialFast.ms(&self.store.theme.theme) * self.motion_scale;
                let travel = crate::scroll::travel(&vertical, source, time);
                if w.shown.over_column(x, y) {
                    self.launcher.scroll_actions(travel, ms);
                } else {
                    self.launcher.scroll(travel, ms);
                }
            }
        }
        true
    }

    /// A press and release on one thing: the foot's buttons are their keys.
    fn launcher_click(&mut self, on: &str) {
        let key = |key, shift| Some((key, Modifiers { shift, ..Default::default() }));
        let hint = match on {
            "actions" => key(keys::Key::Tab, false),
            "into" => key(keys::Key::Right, false),
            "primary" | "column-run" => key(keys::Key::Return, false),
            "alternate" => key(keys::Key::Return, true),
            "column-back" | "column-token" => key(keys::Key::Backtab, false),
            "section" => Some((keys::Key::Tab, Modifiers { ctrl: true, ..Default::default() })),
            _ => None,
        };
        if let Some((key, mods)) = hint {
            self.resolve_launcher();
            let out = self.launcher.key(&self.store, key, field::Key::Other, mods, false, None);
            if self.launcher.on_mirror() {
                self.launcher_mirror();
            }
            self.launch.resolve = true;
            self.launcher_out(out);
            return;
        }
        match on {
            "outside" | "close" => self.menu_close(),
            "token" => self.launcher_back(),
            _ => {
                self.resolve_launcher();
                if let Some(depth) = on.strip_prefix("token:").and_then(|i| i.parse::<usize>().ok()) {
                    self.launcher.leave_to(&self.store, depth);
                } else if let Some(i) = on.strip_prefix("action:").and_then(|i| i.parse::<usize>().ok()) {
                    let out = self.launcher.column_pick(&self.store, i);
                    self.launcher_out(out);
                } else if let Some(i) = on.strip_prefix("row:").and_then(|i| i.parse::<usize>().ok()) {
                    let out = self.launcher.click(&self.store, i);
                    self.launcher_out(out);
                }
                self.launch.resolve = true;
            }
        }
    }

    /// The tray route's rows, off the tray the bar already holds.
    pub(super) fn launcher_tray(&mut self) {
        let items: Vec<fs_menu::providers::TrayItem> = self
            .store
            .tray
            .items
            .iter()
            .map(|i| fs_menu::providers::TrayItem { id: i.id.clone(), title: i.title.clone(), tooltip_title: i.tooltip.clone() })
            .collect();
        let rows = fs_menu::providers::tray_provider(&items);
        self.apply(Diff::Menu(index::Diff::Source(Provider::Tray, rows)));
    }

    /// What every store change the launcher's index reads asks of it.
    pub(super) fn launcher_inputs(&mut self) {
        let l = &self.store.lights;
        let inputs = fs_menu::base::BaseInputs {
            buttons: crate::settings::custom_power_buttons(&self.store.config),
            lights: l.available.then(|| {
                l.effects.iter().map(|(id, label)| fs_menu::providers::LightEffect { id: id.clone(), label: label.clone() }).collect()
            }),
            ipad: self.store.ipad.bridge.is_some(),
            share: {
                let ls = &self.store.localsend;
                Some(fs_menu::base::ShareInputs {
                    installed: ls.installed,
                    peers: ls.peers.iter().map(|p| p.name.clone()).collect(),
                    items: self.store.clipboard.items.clone(),
                    receive: fs_menu::providers::ReceiveStatus {
                        enabled: ls.enabled,
                        receiving: ls.receiving,
                        alias: ls.alias.clone(),
                        dir: ls.dir.clone(),
                    },
                })
            },
            plugin_modes: index::plugin_modes(&self.store),
        };
        if self.launch.buttons.as_ref() != Some(&inputs) || !self.store.config.loaded {
            self.launch.buttons = Some(inputs.clone());
            index::ask(Ask::Base(inputs));
        }
        if self.launch.launches.as_ref() != Some(&self.store.state.data.picks) {
            self.launch.launches = Some(self.store.state.data.picks.clone());
            index::ask(Ask::Apps(index::launches(&self.store.state.data.picks)));
        }
    }
}

/// The type roles a launcher row, its header and its trailing slot use, in
/// the weights they come in.
fn warm_styles(look: &crate::paint::Look, f: &fs_theme::tokens::FontTokens) -> Vec<TextStyle> {
    use fs_theme::tokens::WEIGHTS;
    let mut out = Vec::new();
    for size in [f.caption, f.body_small, f.body, f.subtitle] {
        for weight in [WEIGHTS.normal, WEIGHTS.medium] {
            out.push(TextStyle { family: look.sans, size: size as f32, weight: weight as f32, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None });
        }
    }
    for size in [f.caption, f.body_small] {
        for weight in [WEIGHTS.normal, WEIGHTS.medium] {
            out.push(TextStyle { family: look.mono, size: size as f32, weight: weight as f32, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None });
        }
    }
    out
}

/// menu-selection.txt's and picker-selection.txt's one line, which `dev/`
/// legs read: the token and the value, or `cancelled`, a secret answer
/// filed as cancelled with `secret`.
fn write_selection(path: &std::path::Path, answer: &Selected) {
    let line = match answer {
        Selected { secret: true, .. } => serde_json::json!({"token": answer.token, "cancelled": true, "secret": true}),
        Selected { value: Some(value), cancelled: false, .. } => serde_json::json!({"token": answer.token, "value": value}),
        _ => serde_json::json!({"token": answer.token, "cancelled": true}),
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, line.to_string());
}

/// `r`'s wire form led by one more key.
fn keyed(key: &str, value: serde_json::Value, r: fs_types::IRect) -> serde_json::Value {
    let mut out = serde_json::Map::from_iter([(key.to_owned(), value)]);
    if let serde_json::Value::Object(rect) = serde_json::json!(r) {
        out.extend(rect);
    }
    serde_json::Value::Object(out)
}
