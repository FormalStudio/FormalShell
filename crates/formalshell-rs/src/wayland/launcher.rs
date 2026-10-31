//! The launcher's window on the App: a modal card on the overlay layer
//! (`surfaces::modal`), the keyboard and pointer routed to it, and the
//! `menu` verbs. `new_modal` builds any modal card's three surfaces.

use std::time::Instant;

use fs_chrome::types::Edge;
use fs_menu::nav;
use smithay_client_toolkit::compositor::Region;
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer};

use super::{App, Owner};
use crate::services::menu::{self as index, Ask};
use crate::surface::{PixelSurface, Surface};
use crate::surfaces::card::Ends;
use crate::surfaces::launcher::{ABBREV_EDIT, Out, Shown};
use crate::surfaces::modal::{Modal, Part};
use crate::text::{Family, TextStyle};

/// How long one loop turn draws the launcher before handing the loop back.
const LAUNCHER_SLICE: std::time::Duration = std::time::Duration::from_millis(3);
/// How long the launcher's buffers outlive its last close.
const KEPT_FOR: std::time::Duration = std::time::Duration::from_secs(300);
const WIFI_PASSWORD: &str = "wifi-password";
const WIFI_SHOWN: &str = "wifi-shown";
const WIFI_IDENTITY: &str = "wifi-identity";
const REMINDER_SET: &str = "reminder-set";
const LIGHTS_COLOR: &str = "lights-color";
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;

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
    /// The line a modal card buds off and how far in it lies: a side bar's
    /// inner edge, else Theme.edgeInset's top (the bar's thickness on a top
    /// bar, the frame ring's on a framed one, nothing on a bare edge).
    pub(super) fn modal_line(&self) -> (Edge, f64) {
        let edge = self.bar.edge();
        if self.bar.hidden() {
            (Edge::Top, 0.0)
        } else if edge == Edge::Top || (edge.is_vertical() && !self.bar.framed()) {
            (edge, self.bar.thickness() as f64)
        } else if self.bar.framed() {
            (Edge::Top, self.bar.frame_thickness())
        } else {
            (Edge::Top, 0.0)
        }
    }

    /// A modal card's three surfaces on `layer` under `namespace`: the
    /// card's own full-output one holding the keyboard, the band over the
    /// line's inset (none on a bare edge) and the dim under the rest.
    pub(super) fn new_modal(&self, names: [&'static str; 3], namespace: &'static str, layer: Layer, deform_amount: f64) -> Modal {
        let theme = &self.store.theme.theme;
        let output = self.output_size();
        let (edge, inset) = self.modal_line();
        let framed = self.bar.framed() && edge == Edge::Top;
        let ft = if framed { self.bar.frame_thickness() } else { 0.0 };
        let along = if edge.is_vertical() { output.1 } else { output.0 };
        let ends = Ends { along, inset_start: ft, inset_end: ft, radius: if framed { theme.frame_radius } else { 0.0 } };
        // The scrim's alpha rides in its pixel, under the modal namespaces'
        // `ignore_alpha`, so it only darkens and the card alone is blurred.
        let tone = f64::from(theme.box_style("scrim", None).fill.a);
        let band = (inset > 0.0).then(|| {
            let size = if edge.is_vertical() { (inset as u32, 0) } else { (0, inset as u32) };
            let ls = self.overlay(namespace, layer, super::edge_anchor(edge), size, -1);
            PixelSurface::new(names[1], ls, tone, &self.pixels, &self.qh, self.started)
        });
        let ls = self.layer_shell.create_layer_surface(&self.qh, self.compositor.create_surface(&self.qh), layer, Some(namespace), self.bar_wl.as_ref());
        ls.set_anchor(Anchor::all());
        ls.set_size(0, 0);
        let i = inset as i32;
        match edge {
            Edge::Top => ls.set_margin(i, 0, 0, 0),
            Edge::Right => ls.set_margin(0, i, 0, 0),
            Edge::Bottom => ls.set_margin(0, 0, i, 0),
            Edge::Left => ls.set_margin(0, 0, 0, i),
        }
        ls.set_exclusive_zone(-1);
        ls.set_keyboard_interactivity(KeyboardInteractivity::None);
        if let Ok(region) = Region::new(&self.compositor) {
            ls.set_input_region(Some(region.wl_region()));
        }
        ls.commit();
        let dim = PixelSurface::new(names[2], ls, tone, &self.pixels, &self.qh, self.started);
        let card = self.overlay(namespace, layer, Anchor::all(), (0, 0), -1);
        card.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        card.commit();
        let mut surface = match &self.bar_wl {
            Some(output) => Surface::on(names[0], card, &self.shm, self.started, output),
            None => Surface::new(names[0], card, &self.shm, self.started),
        };
        surface.wait_map = true;
        Modal::new(theme, surface, band, dim, namespace, output, edge, inset, ends, self.motion_scale, self.cast, deform_amount)
    }

    /// A popover card's look handed to the compositor, for an owner whose
    /// content draws on a layer of its own. Content drawn in the card's own
    /// scene would sit under the look's subsurfaces, so those owners keep
    /// the look in that scene.
    pub(super) fn carry_popover_look(&self, modal: &mut Modal) {
        if modal.card.popover() {
            let quads = self.look_quads(modal.surface.name, &modal.surface, modal.card.popover_margin());
            modal.carry_look(quads);
        }
    }

    /// A popover card's look as four subsurfaces of `card`, under anything
    /// added to it later, taking no input.
    fn look_quads(&self, name: &'static str, card: &Surface, margin: i32) -> crate::surfaces::modal::Quads {
        use crate::surface::{Ignore, Sub};
        let parts = (0..4)
            .map(|_| {
                let surface = self.compositor.create_surface(&self.qh);
                let sub = self.pixels.subcompositor.get_subsurface(&surface, card.layer.wl_surface(), &self.qh, Ignore);
                if let Ok(region) = Region::new(&self.compositor) {
                    surface.set_input_region(Some(region.wl_region()));
                }
                let viewport = self.pixels.viewporter.get_viewport(&surface, &self.qh, Ignore);
                let mut s = Surface::new(name, Sub { surface, sub }, &self.shm, self.started).with_viewport(viewport).under(card);
                s.raster_budget = Some(LAUNCHER_SLICE);
                s
            })
            .collect();
        crate::surfaces::modal::Quads::new(name, parts, margin, self.started)
    }

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
            self.launch_kept = Some(modal.surface.keep());
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
                    runtime.pool().submit(crate::trim_heap);
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
        self.launch_kept = crate::surface::Kept::prefaulted(&self.shm, size, runtime.pool());
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
        let s = Surface::new(name, Sub { surface, sub }, &self.shm, self.started).with_viewport(viewport).under(card);
        crate::surfaces::modal::Layer::new(s)
    }

    /// The window for an open, created fresh unless one is already up.
    fn show_launcher(&mut self) {
        self.launcher_resolve = true;
        if self.launch.as_ref().is_some_and(|w| w.shown.modal.open) {
            return;
        }
        self.drop_launch();
        let mut modal = self.new_modal(["menu", "menu-scrim-band", "menu-scrim"], "formalshell:menu", Layer::Overlay, Shown::DEFORM_AMOUNT);
        self.carry_popover_look(&mut modal);
        if let Some(kept) = self.launch_kept.take() {
            modal.surface.adopt(kept);
        }
        modal.surface.raster_budget = Some(LAUNCHER_SLICE);
        let mut content = self.content_layer("menu-content", &modal.surface);
        if let Some(kept) = self.layer_kept.take() {
            content.surface.adopt(kept);
        }
        content.surface.raster_budget = Some(LAUNCHER_SLICE);
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

    /// `name`'s panel (Settings, a settings page, Information) held in the
    /// launcher card, the launcher opening on its route if it is not up.
    pub fn menu_panel(&mut self, name: &str) {
        self.atlas_close();
        self.overlay_close();
        self.close_panels();
        if !self.launcher.open {
            self.launcher.open(&self.store, None);
        }
        if self.launcher.show_panel(&self.store, name) {
            self.launcher_panel_start();
            self.show_launcher();
        }
    }

    /// The panel the open launcher holds, by its `panel` name.
    pub fn launcher_panel(&self) -> Option<&'static str> {
        self.launcher.panel.as_ref().filter(|_| self.launcher.open && self.launcher.on_panel()).map(|p| p.name)
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
            P::Panel(name) if panel::launcher_route(name).is_some() => self.menu_panel(name),
            P::Panel(name) => {
                self.menu_close();
                self.set_panel(name, true, None);
            }
            P::Summon(panel::ATLAS) => {
                self.menu_close();
                self.atlas_show();
            }
            P::Summon(route) => {
                self.launcher.close();
                self.menu_open(Some(route));
            }
            P::Call(target, function) => {
                self.menu_close();
                self.call_self(target, function, &[]);
            }
            P::Menu(..) => {}
        }
        self.launcher_resolve = true;
    }

    /// One level up: a settings page goes back to its tiles, anything else
    /// leaves the level.
    fn launcher_up(&mut self) {
        let page = self.launcher_panel().filter(|n| crate::surfaces::panel::launcher_route(n) != Some(*n));
        match page.and_then(crate::surfaces::panel::launcher_route) {
            Some(route) => self.menu_panel(route),
            None => {
                let out = self.launcher.pop(&self.store);
                self.launcher_out(out);
            }
        }
    }

    /// One key into the held panel, through the panel host's cursor model.
    fn launcher_panel_key(&mut self, event: &KeyEvent, repeat: bool) {
        use crate::surfaces::panel::host::{Driven, Key, drive};
        let key = match event.keysym {
            Keysym::BackSpace => Key::Back,
            Keysym::Escape => Key::Escape,
            Keysym::Tab if self.mods.shift => Key::Tab(-1),
            Keysym::Tab => Key::Tab(1),
            Keysym::ISO_Left_Tab => Key::Tab(-1),
            Keysym::Down => Key::Move(0, 1),
            Keysym::Up => Key::Move(0, -1),
            Keysym::Right => Key::Move(1, 0),
            Keysym::Left => Key::Move(-1, 0),
            Keysym::Return | Keysym::KP_Enter | Keysym::space if repeat => return,
            Keysym::Return | Keysym::KP_Enter | Keysym::space => Key::Activate,
            _ => match event.utf8.clone() {
                Some(t) if t.chars().count() == 1 && t.chars().all(|c| c as u32 >= 0x20) => Key::Text(t),
                _ => return,
            },
        };
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
            return Err("error: menu not open".into());
        }
        self.resolve_launcher();
        self.launcher_resolve = true;
        if self.launcher.open_actions(&self.store, index) { Ok(()) } else { Err(format!("error: row {index} has no actions")) }
    }

    /// `menu runAction`: the action `id` on row `index`.
    pub fn menu_run_action(&mut self, index: usize, id: &str) -> Result<(), String> {
        if !self.launcher.open {
            return Err("error: menu not open".into());
        }
        self.resolve_launcher();
        let out = self.launcher.run_action(&self.store, index, id)?;
        self.launcher_out(out);
        Ok(())
    }

    /// `menu into`: →, on row `index`.
    pub fn menu_into(&mut self, index: usize) -> Result<(), String> {
        if !self.launcher.open {
            return Err("error: menu not open".into());
        }
        self.resolve_launcher();
        let out = self.launcher.step_into(&self.store, index).ok_or_else(|| format!("error: row {index} has nothing to step into"))?;
        self.launcher_out(out);
        Ok(())
    }

    /// The rows again, now, so an IPC read answers off the same state a
    /// frame would draw.
    pub fn resolve_launcher(&mut self) {
        if self.launcher_resolve {
            self.launcher_resolve = false;
            self.launcher.motion_scale = self.motion_scale;
            self.launcher.resolve(&self.store, &self.store.theme.theme, &mut self.bar.kit);
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
        let configured = self.store.config.str("clipssh.alias").unwrap_or("");
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
            self.store.notifications.notify("Keyboard Lights", &body, fs_info::notifications::Urgency::Normal);
            crate::surfaces::changed(self, crate::store::Topic::Notifications);
        }
    }

    /// ReminderService.resolveInput: "25m coffee" set, and a toast either way.
    fn reminder_answer(&mut self, value: Option<String>, cancelled: bool) {
        use fs_info::notifications::Urgency;
        let value = value.unwrap_or_default();
        if cancelled {
            return;
        }
        let fallback = self.store.config.str("reminders.defaultMessage").unwrap_or("Time's up").to_owned();
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
                let n = &self.store.devices.network;
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
            ("bluetooth", "forget") => bluetooth::ask(bluetooth::Ask::Run(value, bluetooth::ActionKind::Forget)),
            ("bluetooth", "toggle") => {
                let Some(d) = self.store.devices.bluetooth.devices.iter().find(|d| d.address.eq_ignore_ascii_case(&value)) else { return };
                let kind = if d.connected { bluetooth::ActionKind::Disconnect } else { bluetooth::ActionKind::Connect };
                bluetooth::ask(bluetooth::Ask::Run(d.address.clone(), kind));
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
        let Some(row) = self.store.devices.network.row(&ssid) else { return };
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
        let chord = self.store.config.str("clipboard.pasteChord").unwrap_or("ctrl+v").to_owned();
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
    pub fn launcher_clipboard(&mut self) {
        if std::mem::take(&mut self.store.clipboard.captured) {
            use crate::services::clipssh;
            clipssh::command(clipssh::Cmd::AutoImage {
                configured: self.store.config.str("clipssh.alias").unwrap_or("").to_owned(),
                enabled: self.store.config.bool("clipssh.autoSendImages").unwrap_or(false),
            });
        }
        let c = &self.store.clipboard;
        let paste = self.store.config.bool("clipboard.paste").unwrap_or(true);
        let rows = fs_menu::providers::clipboard_provider(&c.items, fs_menu::providers::ClipMode::Copy, paste);
        let shared = fs_menu::providers::clipboard_provider(&c.items, fs_menu::providers::ClipMode::Share, paste);
        let moved = self.store.menu.apply(index::Diff::Source("clipboard".into(), rows));
        if self.store.menu.apply(index::Diff::Source("shareHistory".into(), shared)) || moved {
            self.launcher_store_changed();
        }
    }

    /// The picker grid's pictures, asked for once the listing's thumbnails
    /// are in the cache (or the warm gave up on them).
    pub fn launcher_picker(&mut self) {
        if self.launcher.open && self.launcher.on_picker() {
            let px = (crate::surfaces::launcher::picker_cell_px(&self.store.theme.theme), self.launcher_scale());
            let p = &self.store.picker;
            let want: Vec<String> = self
                .launcher
                .picker_listing(&self.store)
                .into_iter()
                .filter(|path| px != p.thumb_px || !p.thumbs.contains_key(path))
                .filter(|path| p.cached.contains(path) || !p.cached.is_empty() || crate::services::wallpaper::generated(path))
                .collect();
            if !want.is_empty() {
                crate::services::picker::command(crate::services::picker::Cmd::Thumbs(want, px));
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

    pub fn picker_variant(&mut self, light: bool) -> bool {
        let ok = self.launcher.set_picker_variant(&self.store, light);
        self.launcher_resolve = true;
        self.launcher_picker();
        ok
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
        let size = (crate::surfaces::launcher::preview_box(theme, &mut self.bar.kit, width, height), self.launcher_scale());
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
            Some(PanePicture::Window(id)) if height > 0.0 => window = Some(id.to_owned()),
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
    pub fn launcher_mirror(&mut self) {
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
    pub fn mirror_status(&self) -> String {
        let mi = &self.store.mirror;
        let showing = self.mirror_showing();
        let current = if showing { self.launcher.mirror_current.as_str() } else { "" };
        let streaming = showing && mi.streaming.as_deref() == Some(current) && !current.is_empty();
        let rect = |r: crate::scene::IRect| serde_json::json!({"x": r.x, "y": r.y, "width": r.w, "height": r.h});
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
        .to_string()
    }

    /// The clipssh route's rows off the saved aliases.
    pub fn launcher_clipssh(&mut self) {
        let rows = fs_menu::providers::clipssh_rows(&self.store.clipssh.aliases);
        if self.store.menu.apply(index::Diff::Source("clipssh".into(), rows)) {
            self.launcher_store_changed();
        }
    }

    /// The device routes' rows, again from the store.
    pub fn launcher_devices(&mut self) {
        if let Some(r) = self.store.devices.revealed.take() {
            self.wifi_revealed(r);
        }
        let mut moved = false;
        for (name, rows) in crate::surfaces::launcher::device_sources(&self.store) {
            moved |= self.store.menu.apply(index::Diff::Source(name.into(), rows));
        }
        if moved || self.launcher.open {
            self.launcher_store_changed();
        }
    }

    /// The plugin modes' rows off what their plugins print, and any
    /// `actions` answers into the open column.
    pub fn launcher_plugins(&mut self) {
        let mut moved = false;
        for (route, rows) in crate::surfaces::launcher::plugin_sources(&self.store) {
            moved |= self.store.menu.apply(index::Diff::Source(route, rows));
        }
        moved |= self.launcher.plugin_answers(&self.store);
        if moved {
            self.launcher_store_changed();
        }
    }

    /// `menu refresh`: the base and the apps read again.
    pub fn menu_refresh(&mut self) {
        self.menu_buttons = None;
        self.menu_launches = None;
        self.launcher_inputs();
    }

    /// The store moved under an open launcher: the rows again.
    pub fn launcher_store_changed(&mut self) {
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
        let styles = if self.launcher_styles.is_empty() { warm_styles(&self.bar.kit.look, &self.store.theme.theme.font_size) } else { self.launcher_styles.clone() };
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
        let text = self.bar.kit.text.clone();
        runtime.pool().submit(move || text.warm(&jobs));
    }

    /// Every face and size the launcher draws its words in, warmed once the
    /// bar is up with the glyphs most rows use, a glyph per job so the
    /// shaper's lock is never held for more than one outline. Glyph by
    /// glyph across every style, so each face exists after the first round.
    pub(super) fn warm_faces(&mut self) {
        let Some(runtime) = &self.runtime else { return };
        let sample = "aABCDEFGHIJKLMNOPQRSTUVWXYZbcdefghijklmnopqrstuvwxyz0123456789.,:;-_/()'";
        let styles = warm_styles(&self.bar.kit.look, &self.store.theme.theme.font_size);
        let jobs: Vec<(String, TextStyle)> =
            sample.chars().flat_map(|c| styles.iter().map(move |st| (c.to_string(), *st))).collect();
        let text = self.bar.kit.text.clone();
        runtime.pool().submit(move || text.warm(&jobs));
    }

    /// The launcher's frame, its scrim on the card's own pose.
    /// The scale the launcher's pictures are made at: its card's, else the
    /// bar's output's, where it opens.
    fn launcher_scale(&self) -> f64 {
        self.launch.as_ref().map_or_else(crate::scene::raster_scale, |w| w.shown.modal.surface.scale())
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
                self.launcher.resolve(&self.store, &self.store.theme.theme, &mut self.bar.kit);
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
        let rastering = w.shown.modal.surface.rastering() || w.shown.modal.layer.as_ref().is_some_and(|l| l.surface.rastering());
        let wants = (self.launcher.dirty || moving || !w.shown.modal.surface.mapped) && !w.shown.hold_layout(&self.launcher, now);
        // A layout held back by a sliced frame runs once it is out, even
        // when whatever moved has stopped by then.
        if rastering && wants {
            self.launcher.dirty = true;
        }
        if !rastering && wants {
            let _raster = crate::scene::raster_at(w.shown.modal.surface.scale());
            w.shown.modal.sync_region(&self.compositor);
            self.bar.kit.seen = Some(std::mem::take(&mut self.launcher_styles));
            w.shown.layout(&mut self.launcher, &self.store, theme, &mut self.bar.kit, now);
            self.launcher_styles = self.bar.kit.seen.take().unwrap_or_default();
            self.launcher.dirty = false;
        } else {
            // Every turn, not only while the morph runs: its last step lands
            // after the morph has stopped running, and a no-op once there.
            w.shown.place_card(theme, now);
        }
        let laid = t1.elapsed();
        let animating = w.shown.animating(now) || self.launcher.rows_moving(now);
        w.shown.modal.present(animating, now, &qh);
        // The rest of a sliced frame on the next turn, after whatever the
        // loop has waiting (the bar's callback first).
        if (w.shown.modal.surface.rastering()
            || w.shown.modal.layer.as_ref().is_some_and(|l| l.surface.rastering())
            || w.shown.modal.look_rastering())
            && let Some(handle) = &self.handle
        {
            handle.insert_idle(|_| {});
        }
        if t0.elapsed().as_millis() >= 8 {
            eprintln!(
                "event loop: slow launcher t={}ms resolve_us={} layout_us={} total_us={}",
                self.started.elapsed().as_millis(),
                resolved.as_micros(),
                laid.as_micros(),
                t0.elapsed().as_micros()
            );
        }
        if animating {
            self.sync_join();
        }
    }

    pub(super) fn launcher_owner(&self, surface: &smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface) -> Option<Owner> {
        self.launch.as_ref()?.shown.modal.part(surface).map(Owner::Launcher)
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
        let first = part == Part::Card && !w.shown.modal.surface.configured;
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
        let key = match event.keysym {
            Keysym::Up => nav::Key::Up,
            Keysym::Down => nav::Key::Down,
            Keysym::Left => nav::Key::Left,
            Keysym::Right => nav::Key::Right,
            Keysym::Page_Up => nav::Key::PageUp,
            Keysym::Page_Down => nav::Key::PageDown,
            Keysym::Home => nav::Key::Home,
            Keysym::End => nav::Key::End,
            Keysym::Return => nav::Key::Return,
            Keysym::KP_Enter => nav::Key::Enter,
            Keysym::Escape => nav::Key::Escape,
            Keysym::BackSpace => nav::Key::Backspace,
            Keysym::Tab if self.mods.shift => nav::Key::Backtab,
            Keysym::Tab => nav::Key::Tab,
            Keysym::ISO_Left_Tab => nav::Key::Backtab,
            _ => nav::Key::Other,
        };
        if self.launcher.on_panel() {
            self.launcher_panel_key(event, repeat);
            return true;
        }
        let text = event.utf8.as_deref().filter(|t| t.chars().all(|c| c as u32 >= 0x20 && c != '\u{7f}') && !t.is_empty());
        self.resolve_launcher();
        let out = self.launcher.key(&self.store, key, super::field_key(event.keysym), self.mods, repeat, text);
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
    pub(super) fn launcher_pointer(&mut self, e: &PointerEvent, owner: Option<Owner>) -> bool {
        if owner != Some(Owner::Launcher(Part::Card)) {
            return false;
        }
        let Some(w) = &mut self.launch else { return true };
        let (x, y) = e.position;
        let on = |w: &Window| w.shown.hit(x, y).and_then(|h| h.on).or_else(|| (!w.shown.modal.on_card(x, y)).then(|| "outside".into()));
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
                if button == BTN_RIGHT {
                    if let Some(i) = on.strip_prefix("row:").and_then(|i| i.parse::<usize>().ok()) {
                        self.resolve_launcher();
                        self.launcher.open_actions(&self.store, i);
                        self.launcher_resolve = true;
                    } else if on == "outside" {
                        self.menu_close();
                    }
                    return true;
                }
                if button == BTN_LEFT {
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
        let key = |key, shift| Some((key, nav::Modifiers { shift, ..Default::default() }));
        let hint = match on {
            "actions" => key(nav::Key::Tab, false),
            "into" => key(nav::Key::Right, false),
            "primary" | "column-run" => key(nav::Key::Return, false),
            "alternate" => key(nav::Key::Return, true),
            "column-back" | "column-token" => key(nav::Key::Backtab, false),
            _ => None,
        };
        if let Some((key, mods)) = hint {
            self.resolve_launcher();
            let out = self.launcher.key(&self.store, key, fs_menu::field::Key::Other, mods, false, None);
            if self.launcher.on_mirror() {
                self.launcher_mirror();
            }
            self.launcher_resolve = true;
            self.launcher_out(out);
            return;
        }
        match on {
            "outside" | "close" => self.menu_close(),
            "variant" => {
                let light = !self.launcher.picker.light;
                self.picker_variant(light);
            }
            "token" => self.launcher_up(),
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

    pub(super) fn launcher_joins(&self) -> Vec<(Edge, f64, f64, f64)> {
        self.launch.as_ref().map_or_else(Vec::new, |w| w.shown.modal.joins().collect())
    }

    /// The tray route's rows, off the tray the bar already holds.
    pub fn launcher_tray(&mut self) {
        let items: Vec<fs_menu::providers::TrayItem> = self
            .store
            .tray
            .items
            .iter()
            .map(|i| fs_menu::providers::TrayItem { id: i.id.clone(), title: i.title.clone(), tooltip_title: i.tooltip.clone() })
            .collect();
        let rows = fs_menu::providers::tray_provider(&items, index::SELF);
        if self.store.menu.apply(index::Diff::Source("tray".into(), rows)) {
            self.launcher_store_changed();
        }
    }

    /// What every store change the launcher's index reads asks of it.
    pub fn launcher_inputs(&mut self) {
        let l = &self.store.lights;
        let inputs = index::BaseInputs {
            buttons: self.store.config.get("menu.customPowerButtons").cloned().unwrap_or_default(),
            lights: l.available.then(|| {
                l.effects.iter().map(|(id, label)| fs_menu::providers::LightEffect { id: id.clone(), label: label.clone() }).collect()
            }),
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
            index::ask(Ask::Apps(crate::surfaces::launcher::launches(&self.store)));
        }
    }
}

/// The type roles a launcher row, its header and its trailing slot use, in
/// the weights they come in.
fn warm_styles(look: &crate::surfaces::bar::cell::Look, f: &fs_theme::tokens::FontTokens) -> Vec<TextStyle> {
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
