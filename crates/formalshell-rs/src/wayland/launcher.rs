//! The launcher's window on the App: a modal card on the overlay layer
//! (`surfaces::modal`), the keyboard and pointer routed to it, and the
//! `menu` verbs.

use std::time::Instant;

use fs_chrome::bar::panels::{Home, PanelId};
use fs_types::WindowId;
use smithay_client_toolkit::compositor::Region;
use smithay_client_toolkit::seat::keyboard::KeyEvent;
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::Layer;

use super::{App, Target};
use crate::services::menu::{self as index, Ask};
use crate::services::notifications;
use crate::store::{Diff, Topic};
use crate::surface::{Backend, Surface};
use crate::surfaces::launcher::{ABBREV_EDIT, Out, Shown, keys};
use crate::surfaces::modal::{Modal, Part};
use crate::text::{Family, TextStyle};
use crate::ui::field::{self, Modifiers};

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
    scan: Option<crate::services::devices::network::Hold>,
    /// What a button went down on, and the button.
    pressed: Option<(String, u32)>,
}

impl Window {
    pub fn finished(&self, now: Instant) -> bool {
        self.shown.finished(now)
    }
}

impl App {
    /// The launcher's window gone, its card's pool, buffers and canvas kept
    /// for the next open.
    fn drop_launch(&mut self) {
        self.capture_clear(crate::wayland::capture::Owner::Launcher);
        self.launcher.pane_frame = None;
        if let Some(w) = self.launch.take() {
            let modal = w.shown.modal;
            if let Some(layer) = modal.layer {
                self.layer_kept = Some(layer.keep());
            }
            self.launch_kept = Some(modal.win.surface.keep());
            self.release_kept_later();
        }
    }

    /// What is kept for the next open goes after a while shut: a launcher
    /// used now and then opens on warm buffers, one left alone for long
    /// gives their memory back.
    fn release_kept_later(&mut self) {
        use calloop::timer::{TimeoutAction, Timer};
        let Some(handle) = &self.handle else { return };
        self.kept_generation += 1;
        let generation = self.kept_generation;
        let _ = handle.insert_source(Timer::from_duration(KEPT_FOR), move |_, _, app| {
            if app.kept_generation == generation && app.launch.is_none() {
                app.launch_kept = None;
                app.layer_kept = None;
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
        if self.launch_kept.is_some() || self.launch.is_some() {
            return;
        }
        let (w, h) = self.output_size();
        let size = (w as i32, h as i32);
        self.launch_kept = crate::surface::Kept::prefaulted(&self.shm, size, self.store.display.scales.primary(), runtime.pool());
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
        self.launcher_resolve = true;
        if self.launch.as_ref().is_some_and(|w| w.shown.modal.open) {
            return;
        }
        self.drop_launch();
        let names = ["menu", "menu-scrim-band", "menu-scrim"];
        let mut modal = Modal::open(&self.modal_make(), names, "formalshell:menu", Layer::Overlay, None, Shown::DEFORM_AMOUNT, true);
        if let Some(kept) = self.launch_kept.take() {
            modal.win.surface.adopt(kept);
        }
        modal.win.surface.raster_budget = Some(crate::surfaces::modal::SLICE);
        let mut content = self.content_layer("menu-content", &modal.win.surface);
        if let Some(kept) = self.layer_kept.take() {
            content.surface.adopt(kept);
        }
        content.surface.raster_budget = Some(crate::surfaces::modal::SLICE);
        modal.layer = Some(content);
        let shown = Shown::new(&self.store.theme.theme, modal, self.output_size(), self.motion_scale);
        self.launch = Some(Window { shown, scan: None, pressed: None });
        self.log("menu mapped");
    }

    fn hide_launcher(&mut self) {
        if let Some(w) = &mut self.launch {
            w.shown.modal.close(Instant::now());
            self.log("menu closing");
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
            self.launcher_panel_start();
            self.show_launcher();
        }
    }

    /// The panel the open launcher holds.
    pub fn launcher_panel(&self) -> Option<PanelId> {
        self.launcher.panel.as_ref().filter(|_| self.launcher.open && self.launcher.on_panel()).map(|p| p.id)
    }

    /// `Panel::start` once for a panel the launcher took up.
    fn launcher_panel_start(&mut self) {
        if let Some(p) = self.launcher.panel.as_mut().filter(|p| !p.started) {
            p.started = true;
            let mut fx = crate::surfaces::panel::Effect::new(&self.store, self.runtime.as_ref());
            p.module.start(&mut fx);
        }
    }

    /// What a key or click in the held panel asked for. `Back` with nothing
    /// asked of it climbs out of the route, as Backspace does in a mode.
    fn launcher_panel_out(&mut self, out: crate::surfaces::panel::host::Out, back: bool) {
        use crate::surfaces::panel::{self, host::Out as P};
        self.launcher.dirty = true;
        match out {
            P::None if back => {
                let out = self.launcher.pop(&self.store);
                self.launcher_out(out);
            }
            P::None => {}
            P::Close => self.menu_close(),
            P::Panel(id, _) if id.home() == Home::Launcher => self.menu_panel(id),
            P::Panel(id, then) => {
                self.menu_close();
                self.set_panel(id, true, None);
                if let Some(stop) = then {
                    self.panel_activate(id, &stop);
                }
            }
            P::Back(..) => {}
            P::Summon(panel::ATLAS) => {
                self.menu_close();
                self.atlas_show();
            }
            P::Summon(route) => {
                let from = self.launcher_panel();
                self.launcher.close();
                self.menu_open(Some(route));
                if route == crate::surfaces::launcher::PICKER_ROUTE {
                    self.launcher.picker.from = from;
                }
            }
            P::Call(target, function) => {
                self.menu_close();
                self.call_self(target, function, &[]);
            }
            P::Menu(..) => {}
            P::Ipc(calls, close) => {
                if close {
                    self.menu_close();
                }
                self.run_ipc(&calls);
            }
            P::Act(action) => {
                self.menu_close();
                self.act(action, live!(self).bar.length() as f64);
            }
        }
        self.launcher_resolve = true;
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
        let Some(w) = &self.launch else { return };
        let Some(p) = self.launcher.panel.as_mut() else { return };
        let mut fx = crate::surfaces::panel::Effect::new(&self.store, self.runtime.as_ref());
        let driven = drive(p.module.as_mut(), &mut p.cursor, w.shown.stops(), key, &mut fx);
        let out = fx.out();
        match driven {
            Driven::Escape => return self.launcher_up(),
            Driven::Walked => {
                if let Some(w) = &self.launch {
                    w.shown.follow_panel(&mut self.launcher);
                }
            }
            Driven::Done => {}
        }
        self.launcher_panel_out(out, back);
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
        self.launcher_resolve = true;
        self.show_launcher();
    }

    pub fn menu_filter(&mut self, text: &str) -> bool {
        if !self.launcher.open {
            return false;
        }
        self.launcher.set_query(&self.store, text);
        self.launcher_resolve = true;
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
        self.launcher_resolve = true;
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
        if self.launcher_resolve {
            self.launcher_resolve = false;
            self.launcher.motion_scale = self.motion_scale;
            self.launcher.resolve(&self.store, &self.store.theme.theme, &mut self.kit);
        }
    }

    fn launcher_out(&mut self, out: Out) {
        self.launcher_resolve = true;
        match out {
            Out::None => {}
            Out::Close => self.hide_launcher(),
            Out::Internal(name) => {
                if !self.launcher.open {
                    self.hide_launcher();
                }
                self.dispatch_internal(&name);
            }
        }
    }

    /// An internal action, for the targets this shell serves.
    fn dispatch_internal(&mut self, name: &str) {
        if let Some((verb, value)) = name.split_once('.').map(|(_, rest)| rest.split_once(':').unwrap_or((rest, "")))
            && let Some(group) = name.split('.').next()
            && matches!(group, "wifi" | "bluetooth" | "audio" | "radio")
        {
            return self.device_action(group, verb, value);
        }
        if let Some((verb, value)) = name.strip_prefix("lights.").and_then(|r| r.split_once(':')) {
            return self.call_self("lights", verb, &[value]);
        }
        if let Some(rest) = name.strip_prefix("ipad.") {
            let mut parts = rest.split(':');
            let verb = parts.next().unwrap_or_default();
            let args: Vec<&str> = parts.collect();
            return self.call_self("ipad", verb, &args);
        }
        if let Some((i, id)) = name.strip_prefix("localsend.send:").and_then(|r| r.split_once(':')) {
            return self.localsend_entry(i, id);
        }
        if let Some(attr) = name.strip_prefix("nix.run:") {
            return self.console_run_once(&format!("nix run nixpkgs#{attr}; read"));
        }
        if let Some((pid, signal)) = name.strip_prefix("monitor.kill:").and_then(|r| r.split_once(':')) {
            return self.call_self("monitor", "kill", &[pid, signal]);
        }
        if let Some(rest) = name.strip_prefix("clipssh.image:") {
            return self.clipssh_image(rest);
        }
        if let Some(id) = name.strip_prefix("app.gpu:") {
            return self.call_self("monitor", "launch", &[id, ""]);
        }
        if let Some(panel) = name.strip_prefix("panel.open:") {
            return self.call_self("panel", "open", &[panel]);
        }
        if let Some(id) = name.strip_prefix("clipboard.remove:") {
            return crate::services::clipboard::command(crate::services::clipboard::Cmd::Remove(id.to_owned()));
        }
        if let Some(id) = name.strip_prefix("package.install:") {
            return crate::surfaces::packages::install(&self.store, id);
        }
        if let Some(id) = name.strip_prefix("package.run:") {
            crate::services::compositor::spawn(&["fspec", "run", "--detach", id].map(String::from));
            return;
        }
        if let Some(id) = name.strip_prefix("package.uninstall:") {
            return crate::services::packages::send(crate::services::packages::Cmd::Uninstall(id.to_owned()));
        }
        if let Some(alias) = name.strip_prefix("clipssh.send:") {
            return crate::services::clipssh::command(crate::services::clipssh::Cmd::Send(alias.to_owned()));
        }
        if let Some(action) = name.strip_prefix("power.").and_then(crate::services::power::Action::parse) {
            return crate::services::power::request(action);
        }
        if let Some(id) = name.strip_prefix("clipboard.copy:") {
            return crate::services::clipboard::command(crate::services::clipboard::Cmd::Copy(id.to_owned()));
        }
        match name {
            "theme.toggleMode" => {
                let key = crate::services::theme::mode_key(self.store.config.settings());
                if let Ok(mode) = crate::services::theme::request_mode("toggle", &self.store.state.data.mode, key) {
                    crate::services::state::set(vec![crate::services::state::Field::Mode(mode)]);
                }
            }
            "caffeinate.toggle" => {
                let on = !self.store.caffeinate.active;
                self.set_caffeinated(on);
            }
            // The rest go through the same in-process handlers the IPC
            // targets answer with.
            "lock.lock" => self.call_self("lock", "lock", &[]),
            "lock.session" => self.call_self("lock", "session", &[]),
            "power.logout" => self.call_self("power", "logout", &[]),
            "nightlight.toggle" => self.call_self("nightlight", "toggle", &[]),
            "lights.toggle" => self.call_self("lights", "toggle", &[]),
            "lights.colorInput" => self.menu_input_as("Hex color (ff8800)", LIGHTS_COLOR, false),
            "hdr.toggle" => self.call_self("hdr", "toggle", &[]),
            "overnight.toggle" => self.call_self("overnight", "toggle", &[]),
            "notifications.toggleDnd" => self.call_self("notifications", "toggleDnd", &[]),
            "notifications.showHistory" => self.call_self("notifications", "showHistory", &[]),
            "reminder.show" => self.call_self("reminder", "show", &[]),
            "reminder.clear" => self.call_self("reminder", "clear", &[]),
            "reminder.set" => self.menu_input_as("Reminder (25m coffee)", REMINDER_SET, false),
            other => eprintln!("Menu: no service for internal action: {}", other.split(':').next().unwrap_or(other)),
        }
    }

    /// A clipboard image (`<entry id>:<path>`) over ssh rather than onto the
    /// clipboard: straight there with an alias resolved, else copied and
    /// the alias route asked.
    fn clipssh_image(&mut self, rest: &str) {
        use crate::services::clipssh;
        let Some((id, path)) = rest.split_once(':') else { return };
        let configured = crate::settings::CLIPSSH_ALIAS.get(&self.store.config);
        match clipssh::resolve_alias(configured, &self.store.clipssh.aliases) {
            Some(alias) => clipssh::command(clipssh::Cmd::SendImage(alias, path.to_owned())),
            None => {
                crate::services::clipboard::command(crate::services::clipboard::Cmd::Copy(id.to_owned()));
                self.menu_open(Some("clipssh"));
            }
        }
    }

    /// One call into this shell's own IPC targets, answered in process.
    pub(super) fn call_self(&mut self, target: &str, function: &str, args: &[&str]) {
        self.call_self_reply(target, function, args);
    }

    fn call_self_reply(&mut self, target: &str, function: &str, args: &[&str]) -> String {
        let request = crate::ipc::wire::Request::Call {
            target: target.into(),
            function: function.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        };
        let reply = crate::ipc::dispatch(self, &request);
        if reply.starts_with("error") {
            eprintln!("Menu: {target}.{function}: {}", reply.trim());
        }
        reply
    }

    /// LightsService.resolveInput.
    fn lights_answer(&mut self, value: Option<String>, cancelled: bool) {
        let value = value.unwrap_or_default();
        if cancelled {
            return;
        }
        if self.call_self_reply("lights", "color", &[&value]).starts_with("error") {
            let body = format!("\"{value}\" is not a hex colour like ff8800");
            self.apply(Diff::Notifications(notifications::Diff::Op(notifications::Op::Notify("Keyboard Lights".into(), body, fs_info::notifications::Urgency::Normal))));
        }
    }

    /// ReminderService.resolveInput: "25m coffee" set, and a toast either way.
    fn reminder_answer(&mut self, value: Option<String>, cancelled: bool) {
        use fs_info::notifications::Urgency;
        let value = value.unwrap_or_default();
        if cancelled {
            return;
        }
        let fallback = crate::settings::REMINDER_MESSAGE.get(&self.store.config).to_owned();
        let entry = fs_info::reminders::parse_spec(&value).and_then(|spec| {
            let duration = value.split_whitespace().next().unwrap_or_default().to_owned();
            self.store.notifications.set_reminder(&duration, &spec.message, &fallback)
        });
        match entry {
            Some(e) => {
                let body = format!("{} at {}", e.message, fs_info::reminders::due_clock(e.due_at));
                self.store.notifications.notify("Reminder set", &body, Urgency::Normal);
            }
            None => self.store.notifications.notify("Reminder", &format!("Could not read \"{value}\" as a duration"), Urgency::Normal),
        }
        crate::surfaces::changed(self, crate::store::Topic::Notifications);
    }

    /// The `wifi.`, `bluetooth.`, `audio.` and `radio.` actions.
    fn device_action(&mut self, group: &str, verb: &str, value: &str) {
        use crate::services::devices::{bluetooth, network as net};
        use crate::services::radio::{self, Cmd};
        let value = value.to_owned();
        let service = |f: Box<dyn FnOnce(&crate::runtime::Ctx) + Send>| {
            if let Some(rt) = &self.runtime {
                rt.service(f);
            }
        };
        match (group, verb) {
            ("wifi", "enable") => service(Box::new(|ctx| net::set_wifi(ctx, true))),
            ("wifi", "forget") => service(Box::new(move |ctx| net::forget(ctx, value))),
            ("wifi", "reveal") => {
                self.wifi_reveals += 1;
                let request = self.wifi_reveals;
                self.wifi_reveal = Some((request, self.launcher.sessions));
                service(Box::new(move |ctx| net::reveal(ctx, value, request)));
            }
            ("wifi", "activate") => {
                let n = &self.store.network;
                let Some(row) = n.row(&value).cloned() else { return };
                if n.action.is_some() {
                    return;
                }
                if row.connected {
                    return service(Box::new(move |ctx| net::disconnect(ctx, value)));
                }
                // A secured network nobody knows, or one whose last attempt
                // failed on the secret, asks for it: a wrong password is
                // retyped, never retried.
                let retype = n.failure.as_ref().is_some_and(|f| f.ssid == value && f.secret);
                if retype || (row.secured && !row.known) {
                    let (prompt, token) = if row.enterprise {
                        (format!("Identity for {value}"), WIFI_IDENTITY)
                    } else {
                        (format!("Password for {value}"), WIFI_PASSWORD)
                    };
                    self.wifi_pending = Some((value, String::new()));
                    self.menu_input_as(&prompt, token, token == WIFI_PASSWORD);
                } else {
                    service(Box::new(move |ctx| net::connect(ctx, value, net::Secret::Saved)));
                }
            }
            ("bluetooth", "power") if value == "on" => bluetooth::ask(bluetooth::Ask::Power(true)),
            ("bluetooth", "forget") => {
                if let Some(address) = fs_types::BtAddress::parse(&value) {
                    bluetooth::ask(bluetooth::Ask::Run(address, bluetooth::ActionKind::Forget));
                }
            }
            ("bluetooth", "toggle") => {
                let Some(address) = fs_types::BtAddress::parse(&value) else { return };
                let Some(d) = self.store.bluetooth.devices.iter().find(|d| d.address == address) else { return };
                let kind = if d.connected { bluetooth::ActionKind::Disconnect } else { bluetooth::ActionKind::Connect };
                bluetooth::ask(bluetooth::Ask::Run(address, kind));
            }
            ("audio", "sink" | "source") => {
                let c = if verb == "sink" {
                    fs_audio::Command::SetDefaultSink(Some(value))
                } else {
                    fs_audio::Command::SetDefaultSource(Some(value))
                };
                service(Box::new(move |_| crate::services::devices::audio::command(c)));
            }
            ("radio", _) => {
                let r = &self.store.media.radio;
                let results = r.search.as_ref().and_then(|(_, f)| f.clone()).unwrap_or_default();
                let station = r.favorites.iter().chain(results.iter()).find(|s| s.uuid == value).cloned();
                match (verb, station) {
                    ("stop", _) => radio::send(Cmd::Stop),
                    ("fav", Some(s)) => radio::send(Cmd::PlayFromSaved(s, r.favorites.clone())),
                    ("play", Some(s)) => radio::send(Cmd::PlayFromSaved(s, results)),
                    ("favorite" | "unfavorite", Some(s)) => {
                        if r.favorites.iter().any(|f| f.uuid == s.uuid) == (verb == "unfavorite") {
                            radio::send(Cmd::ToggleFavorite(s));
                        }
                    }
                    _ => {}
                }
            }
            _ => eprintln!("Menu: unknown internal action: {group}.{verb}"),
        }
    }

    /// The answers whose token an in-process owner holds.
    fn launcher_answers(&mut self) {
        for (token, value, cancelled) in self.launcher.take_resolved() {
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
    }

    /// Show password's answer: the passphrase in the secret step, drawn in
    /// the clear, or why there is none. Dropped once the session it was
    /// asked in has closed.
    fn wifi_revealed(&mut self, r: crate::services::devices::network::Revealed) {
        if !self.launcher.open || self.wifi_reveal != Some((r.request, self.launcher.sessions)) {
            return;
        }
        self.wifi_reveal = None;
        match r.secret {
            Ok(Some(psk)) => {
                self.launcher.open_shown(&format!("Password for {}", r.ssid), WIFI_SHOWN, &psk);
                self.launcher_resolve = true;
                self.show_launcher();
            }
            Ok(None) => self.menu_input_as(&format!("No password saved for {}", r.ssid), WIFI_SHOWN, true),
            Err(_) => self.menu_input_as(&format!("Could not read the password for {}", r.ssid), WIFI_SHOWN, true),
        }
    }

    /// WifiService.resolveInput, then the launcher back on the Wi-Fi list.
    fn wifi_answer(&mut self, token: &str, value: Option<String>, cancelled: bool) {
        use crate::services::devices::network as net;
        let Some((ssid, identity)) = self.wifi_pending.take() else { return };
        let value = value.unwrap_or_default();
        if cancelled || value.is_empty() {
            return;
        }
        if token == WIFI_IDENTITY {
            self.wifi_pending = Some((ssid.clone(), value));
            self.menu_input_as(&format!("Password for {ssid}"), WIFI_PASSWORD, true);
            return;
        }
        let Some(row) = self.store.network.row(&ssid) else { return };
        let secret = if !identity.is_empty() && row.enterprise {
            net::Secret::Eap { identity, password: value }
        } else {
            net::Secret::Psk(value)
        };
        if let Some(rt) = &self.runtime {
            rt.service(move |ctx| net::connect(ctx, ssid, secret));
        }
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
        self.apply(Diff::Menu(index::Diff::Source("clipboard".into(), rows)));
        self.apply(Diff::Menu(index::Diff::Source("shareHistory".into(), shared)));
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

    pub fn picker_choose_for(&mut self, path: &str, half: crate::services::state::Half) -> bool {
        let chosen = self.launcher.choose_for(&self.store, path, half);
        if chosen {
            self.hide_launcher();
        }
        chosen
    }

    /// LocalsendService.sendClipboardEntryAt: a ledger entry to the peer at
    /// `index` of the last scan.
    fn localsend_entry(&mut self, index: &str, id: &str) {
        use crate::services::localsend::{self, Cmd, Payload};
        use fs_info::notifications::Urgency;
        let peer = index.parse::<usize>().ok().and_then(|i| self.store.localsend.peers.get(i)).cloned();
        let Some(peer) = peer else {
            self.store.notifications.notify("LOCALSEND FAILED", "That device is no longer in range, rescan and try again", Urgency::Critical);
            return crate::surfaces::changed(self, crate::store::Topic::Notifications);
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
            self.peek_asked = None;
            self.info_asked = None;
        }
        let theme = &self.store.theme.theme;
        let (_, width, height) = self.launcher.pane_extent(theme);
        let size = (crate::surfaces::launcher::preview_box(theme, &mut self.kit, width, height), self.launcher_scale());
        if let Some(n) = row.as_ref().filter(|n| matches!(pane_kind(n), Some(PaneKind::File | PaneKind::Folder)))
            && self.peek_asked.as_deref() != Some(n.path.as_str())
        {
            self.peek_asked = Some(n.path.clone());
            index::ask(Ask::Peek(n.path.clone()));
        }
        // An Install row's details, asked once per row shown, and its
        // screenshot decoded at the pane's width once the file is cached.
        if let Some(n) = row.as_ref().filter(|n| matches!(pane_kind(n), Some(PaneKind::Package))) {
            if self.info_asked.as_deref() != Some(n.path.as_str()) {
                self.info_asked = Some(n.path.clone());
                crate::services::packages::send(crate::services::packages::Cmd::Info(n.path.clone()));
            }
            let p = &self.store.packages;
            let shot = p.info.get(&n.path).and_then(|i| i.as_ref().ok()).and_then(|i| i.screenshot.as_ref()).and_then(|u| p.shots.get(u)).cloned().flatten();
            if let Some(path) = shot {
                let size = (crate::surfaces::launcher::shot_box(width), size.1);
                let have = self.store.clipboard.preview.as_ref().is_some_and(|p| p.path == path && p.size == size);
                if !have && self.preview_asked.as_ref() != Some(&(path.clone(), size)) {
                    self.preview_asked = Some((path.clone(), size));
                    crate::services::clipboard::command(crate::services::clipboard::Cmd::Preview(path, size));
                }
            }
        }
        let mut window = None;
        match row.as_ref().and_then(pane_picture) {
            Some(PanePicture::Decoded(path)) if height > 0.0 => {
                let have = self.store.clipboard.preview.as_ref().is_some_and(|p| p.path == path && p.size == size);
                if !have && self.preview_asked.as_ref() != Some(&(path.to_owned(), size)) {
                    self.preview_asked = Some((path.to_owned(), size));
                    crate::services::clipboard::command(crate::services::clipboard::Cmd::Preview(path.to_owned(), size));
                }
            }
            Some(PanePicture::Window(id)) if height > 0.0 => window = Some(WindowId::new(id)),
            _ => {}
        }
        match &window {
            Some(id) => {
                let from = self.store.compositor.window(id).and_then(|w| w.rect.as_ref()).map_or((0.0, 0.0), |r| (r.width as f64, r.height as f64));
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
        self.launcher.open && self.launcher.on_mirror() && self.launch.as_ref().is_some_and(|w| w.shown.modal.open)
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
    pub fn launcher_shown(&self) -> Option<&Shown> {
        self.launch.as_ref().filter(|w| w.shown.modal.open).map(|w| &w.shown)
    }

    /// `mirror status`.
    pub fn mirror_status(&self) -> serde_json::Value {
        let mi = &self.store.mirror;
        let showing = self.mirror_showing();
        let current = if showing { self.launcher.mirror_current.as_str() } else { "" };
        let streaming = showing && mi.streaming.as_deref() == Some(current) && !current.is_empty();
        let rect = |r: crate::scene::IRect| r.to_json();
        let feed = self.launch.as_ref().filter(|_| showing).and_then(|w| w.shown.feed);
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
        self.apply(Diff::Menu(index::Diff::Source("clipssh".into(), rows)));
    }

    /// The device routes' rows, again from the store.
    pub(super) fn launcher_devices(&mut self) {
        if let Some(r) = self.store.revealed.take() {
            self.wifi_revealed(r);
        }
        for (name, rows) in crate::surfaces::launcher::device_sources(&self.store) {
            self.apply(Diff::Menu(index::Diff::Source(name.into(), rows)));
        }
        if self.launcher.open {
            self.launcher_store_changed();
        }
    }

    /// The plugin modes' rows off what their plugins print, and any
    /// `actions` answers into the open column.
    pub(super) fn launcher_plugins(&mut self) {
        for (route, rows) in crate::surfaces::launcher::plugin_sources(&self.store) {
            self.apply(Diff::Menu(index::Diff::Source(route, rows)));
        }
        if self.launcher.plugin_answers(&self.store) {
            self.launcher_store_changed();
        }
    }

    /// `menu refresh`: the base and the apps read again.
    pub fn menu_refresh(&mut self) {
        self.menu_buttons = None;
        self.menu_launches = None;
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
            Topic::Menu => {
                crate::surfaces::packages::land(&mut self.store);
                self.launcher_store_changed();
            }
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
                if crate::surfaces::packages::land(&mut self.store) || self.launcher.level.as_deref() == Some(fs_menu::providers::INSTALL_ROUTE) {
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
        if held || (topic == Topic::Theme && self.launch.is_some()) {
            self.launcher.dirty = true;
        }
    }

    /// The store moved under an open launcher: the rows again.
    pub(super) fn launcher_store_changed(&mut self) {
        if self.launcher.open {
            self.launcher_resolve = true;
        } else {
            self.launcher_warm = true;
        }
    }

    /// The rows' words shaped on the pool in every style the launcher last
    /// drew in (the theme's row styles before its first open), so an open
    /// finds them in the shared cache instead of shaping on this thread.
    fn warm_launcher_text(&mut self) {
        let Some(runtime) = &self.runtime else { return };
        let styles = if self.launcher_styles.is_empty() { warm_styles(&self.kit.look, &self.store.theme.theme.font_size) } else { self.launcher_styles.clone() };
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
        self.launch.as_ref().map_or_else(|| self.store.display.scales.primary(), |w| w.shown.modal.win.surface.scale())
    }

    pub(super) fn present_launcher(&mut self, now: Instant) {
        self.launcher_answers();
        if let Some(w) = &mut self.launch {
            let want = self.launcher.open && w.shown.modal.open && self.launcher.level.as_deref() == Some("wifi");
            if want != w.scan.is_some() {
                w.scan = want.then(crate::services::devices::network::Hold::new);
            }
        }
        if self.launch.as_ref().is_some_and(|w| w.finished(now)) {
            self.drop_launch();
            self.sync_join();
            self.log("menu unmapped");
            if std::mem::take(&mut self.launcher.paste) && !self.launcher.open {
                self.paste_chord();
            }
            return;
        }
        if self.launch.is_none() {
            if self.launcher_warm && !self.launcher.open {
                self.launcher_warm = false;
                self.launcher.resolve(&self.store, &self.store.theme.theme, &mut self.kit);
                self.warm_launcher_text();
                if !self.fresh_asked && !self.store.menu.nodes().is_empty() {
                    self.fresh_asked = true;
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
        let Some(w) = &mut self.launch else { return };
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
            self.kit.seen = Some(std::mem::take(&mut self.launcher_styles));
            w.shown.layout(&mut self.launcher, &self.store, theme, &mut self.kit, now);
            self.launcher_styles = self.kit.seen.take().unwrap_or_default();
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
        self.launch.as_ref()?.shown.modal.part(surface)
    }

    pub(super) fn launcher_frame(&mut self, part: Part, now: Instant) {
        let Some(w) = &mut self.launch else { return };
        if w.shown.modal.frame(part, now) {
            self.launcher.dirty = true;
            self.sync_join();
        }
    }

    pub(super) fn launcher_configure(&mut self, part: Part, width: i32, height: i32) {
        let Some(w) = &mut self.launch else { return };
        let first = part == Part::Card && !w.shown.modal.win.surface.configured;
        w.shown.modal.configure(part, width, height);
        if part == Part::Card {
            self.launcher.dirty = true;
        }
        if first {
            self.log("menu configured");
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
        if !self.launcher.open || self.launch.as_ref().is_none_or(|w| !w.shown.modal.open) {
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
        self.launcher_resolve = true;
        self.launcher_out(out);
        true
    }

    /// The clipboard's text into the query, while the launcher is up.
    pub(super) fn launcher_paste(&mut self, text: &str) -> bool {
        if !self.launcher.open || self.launch.as_ref().is_none_or(|w| !w.shown.modal.open) {
            return false;
        }
        self.resolve_launcher();
        self.launcher.paste(&self.store, text);
        self.launcher_resolve = true;
        true
    }

    /// The pointer over the launcher's window. True when it took the event.
    /// It tints what it hovers and never moves the selection (launcher
    /// spec §6).
    pub(super) fn launcher_pointer(&mut self, e: &PointerEvent, owner: Option<Target>) -> bool {
        if owner != Some(Target::Launcher(Part::Card)) {
            return false;
        }
        let Some(w) = &mut self.launch else { return true };
        let (x, y) = e.position;
        let on = |w: &Window| w.shown.hit(x, y).and_then(|h| h.on).or_else(|| (!w.shown.modal.win.on_card(x, y)).then(|| "outside".into()));
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
                    && let Some(e) = w.shown.panel_event(x, y)
                {
                    if let Some(p) = self.launcher.panel.as_mut() {
                        let mut fx = crate::surfaces::panel::Effect::new(&self.store, self.runtime.as_ref());
                        p.module.event(&e, &mut fx);
                        p.cursor.take(p.module.as_mut(), w.shown.stops());
                        let out = fx.out();
                        self.launcher_panel_out(out, false);
                    }
                    return true;
                }
                if button == crate::input::BTN_RIGHT {
                    if let Some(i) = on.strip_prefix("row:").and_then(|i| i.parse::<usize>().ok()) {
                        self.resolve_launcher();
                        self.launcher.open_actions(&self.store, i);
                        self.launcher_resolve = true;
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
            self.launcher_resolve = true;
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
                self.launcher_resolve = true;
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
        let rows = fs_menu::providers::tray_provider(&items, index::SELF);
        self.apply(Diff::Menu(index::Diff::Source("tray".into(), rows)));
    }

    /// What every store change the launcher's index reads asks of it.
    pub(super) fn launcher_inputs(&mut self) {
        let l = &self.store.lights;
        let inputs = index::BaseInputs {
            buttons: self.store.config.get("menu.customPowerButtons").cloned().unwrap_or_default(),
            lights: l.available.then(|| {
                l.effects.iter().map(|(id, label)| fs_menu::providers::LightEffect { id: id.clone(), label: label.clone() }).collect()
            }),
            ipad: self.store.ipad.bridge.is_some(),
            share: {
                let ls = &self.store.localsend;
                Some(index::ShareInputs {
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
            plugin_modes: crate::surfaces::launcher::plugin_modes(&self.store),
        };
        if self.menu_buttons.as_ref() != Some(&inputs) || !self.store.config.loaded {
            self.menu_buttons = Some(inputs.clone());
            index::ask(Ask::Base(inputs));
        }
        if self.menu_launches.as_ref() != Some(&self.store.state.data.picks) {
            self.menu_launches = Some(self.store.state.data.picks.clone());
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
