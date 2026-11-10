//! The panel host's controller: which panel is open, opening one over
//! another as a hand-over, and running what a panel asks of the shell.

use std::time::Instant;

use fs_chrome::bar::panels::{Home, PanelId};
use fs_types::Region as BarRegion;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer};

use super::{App, Holder, capture_host, overview};
use crate::surface::{Backend, Surface};
use crate::surfaces::panel::{self, Name, Out, host::{Host, Opener, Place}, quick};

impl App {
    /// The open panel `id`'s own [`panel::Panel::status`]; `None` when it
    /// is not up.
    pub fn panel_status(&self, id: PanelId) -> Option<serde_json::Value> {
        Some(self.panel.as_ref().filter(|p| p.is_open() && p.panel() == Some(id))?.module.status())
    }

    /// The open panel `id` as its module `T`, read without drawing it again.
    pub fn panel_ref<T: panel::Panel>(&self, id: PanelId) -> Option<&T> {
        let host = self.panel.as_ref().filter(|p| p.is_open() && p.panel() == Some(id))?;
        let module: &dyn std::any::Any = &*host.module;
        module.downcast_ref::<T>()
    }

    /// The open panel `id` as its module `T`, for a verb only that panel
    /// takes (`calendar select`); `None` when it is not up.
    pub fn panel_mut<T: panel::Panel>(&mut self, id: PanelId) -> Option<&mut T> {
        let host = self.panel.as_mut().filter(|p| p.is_open() && p.panel() == Some(id))?;
        self.panel_dirty = true;
        let module: &mut dyn std::any::Any = &mut *host.module;
        module.downcast_mut::<T>()
    }

    /// The open panel; the gallery, the centre and a peek are no panel's.
    pub fn panel_open(&self) -> Option<Name> {
        self.panel
            .as_ref()
            .filter(|p| p.is_open())
            .and_then(|p| match p.panel() {
                Some(id) => Some(Name::Panel(id)),
                None => p.id().starts_with(fs_chrome::plugins::PLUGIN_PREFIX).then_some(Name::Plugin(p.id())),
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
    pub(super) fn quick_from(&self) -> Option<quick::Origin> {
        self.panel.as_ref().filter(|p| p.is_open() && p.panel() == Some(PanelId::QuickSettings)).map(|p| p.from)
    }

    /// A panel by name, hung at `anchor` along the line (its own cell's
    /// centre) or at the line's end. A panel opened over another hands the
    /// card over.
    pub fn set_panel(&mut self, name: impl Into<Name>, open: bool, anchor: Option<f64>) {
        self.set_panel_from(name.into(), open, anchor, quick::Origin::Ipc);
    }

    /// Where the control `from` names sits along the line.
    pub(super) fn opener_anchor(&self, from: quick::Origin) -> Option<f64> {
        match from {
            quick::Origin::Chevron(region) => live!(self).bar.chevron_anchor(region),
            quick::Origin::Sidebar => live!(self).bar.sidebar_anchor(crate::surfaces::sidebar::Intent::Overflow),
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

    pub(super) fn set_panel_from(&mut self, name: Name, open: bool, anchor: Option<f64>, from: quick::Origin) {
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
    pub(super) fn open_host(&mut self, module: Box<dyn panel::Panel>, name: &'static str, from: quick::Origin, anchor: Option<f64>, now: Instant) {
        let place = self.chrome().place(anchor);
        self.open_host_at(module, name, from, place, now);
    }

    pub(super) fn open_host_at(&mut self, module: Box<dyn panel::Panel>, id: &'static str, from: quick::Origin, place: Place, now: Instant) {
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
        let outs = host.start(&self.store, self.runtime.as_ref());
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
        self.panel_outs(Holder::Host, outs);
        self.tips.hide(None, now);
        self.sync_bar_open(now);
        crate::trace::mapped(crate::trace::Surface::Panel(Some(id)), crate::trace::State::Mapped);
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

    /// What a panel asked of the shell outside it, in the order it asked.
    /// `holder` is whatever holds the card, and closes it its own way.
    pub(crate) fn panel_outs(&mut self, holder: Holder, outs: Vec<Out>) {
        for out in outs {
            self.panel_out(holder, out);
        }
        self.panel_dirty = true;
    }

    /// The holder's card closed: the panel host's, or the launcher with it.
    pub(super) fn holder_close(&mut self, holder: Holder) {
        match holder {
            Holder::Host => self.close_panels(),
            Holder::Launcher => self.menu_close(),
        }
    }

    pub(super) fn panel_out(&mut self, holder: Holder, out: Out) {
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
                    self.panel_activate(id, stop);
                }
            }
            Out::Back(id, from) => {
                let anchor = self.opener_anchor(from);
                self.set_panel_from(Name::Panel(id), true, anchor, from);
            }
            Out::Menu(key, anchor) => {
                let _ = self.open_tray_menu(&key, anchor, true);
            }
            Out::Do(verb) => self.panel_verb(verb),
            Out::Apply(diff) => {
                self.apply(*diff);
            }
            Out::Launch(id, command) => {
                if !command.is_empty() {
                    crate::services::launch::spawn(&self.store, &id, &crate::services::launch::scoped(&command));
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

    /// A panel's call of the shell, through the functions its IPC verb
    /// calls; a refusal is logged.
    pub(super) fn panel_verb(&mut self, verb: panel::Verb) {
        use panel::Verb;
        let done = match &verb {
            Verb::Screenshot => capture_host::shot_pick(self, "smart", capture_host::Processing::Default).map(drop),
            Verb::Lock => self.lock(),
            Verb::Logout => {
                self.logout();
                Ok(())
            }
            Verb::Overview => {
                self.overview_action(overview::Action::Open);
                Ok(())
            }
            Verb::Caffeinate => {
                self.set_caffeinated(!self.store.caffeinate.active);
                Ok(())
            }
            Verb::Hdr => self.hdr.toggle(&self.store),
            Verb::HdrOutput(output, on) => self.hdr.set(&self.store, output, *on),
            Verb::RecordStop => capture_host::record::stop(self),
        };
        if let Err(e) = done {
            eprintln!("panel: {verb:?}: {e}");
        }
    }

    /// Enter on `stop` in the open panel `id`, as the keyboard would;
    /// `false` when that panel is not up.
    pub fn panel_activate(&mut self, id: PanelId, stop: crate::ui::On) -> bool {
        let Some(h) = self.panel.as_mut().filter(|p| p.is_open() && p.panel() == Some(id)) else { return false };
        let outs = h.activate(stop, &self.store, self.runtime.as_ref());
        self.panel_outs(Holder::Host, outs);
        true
    }
}
