//! The switcher's window, timers and captures;
//! the card itself is `surfaces::switcher`.
//!
//! One full-output overlay surface, created when the card shows and gone
//! once its fade has run out, so a quick Alt+Tab maps nothing. It takes the
//! keyboard Exclusive for the prime and OnDemand after, never a modifier:
//! the release that commits is the compositor's own bind.

use std::time::Instant;

use smithay_client_toolkit::seat::keyboard::Keysym;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer};

use super::App;
use super::capture::Owner;
use crate::scene::{Bitmap, Scene};
use crate::services::{appicon, compositor};
use crate::surface::Surface;
use crate::surfaces::switcher::{self, Input, Switcher, model};

pub const OFF: &str = "error: switcher is off (switcher.enabled)";

#[derive(Default)]
pub struct State {
    pub model: Switcher,
    win: Option<(Surface, Scene)>,
    pub(super) dirty: bool,
}

impl State {
    pub fn owns(&self, surface: &smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface) -> bool {
        self.win.as_ref().is_some_and(|(s, _)| s.layer.wl_surface() == surface)
    }
}

/// The edges of a card's small icon and of the big one over its thumbnail.
fn icon_sizes(s: &fs_theme::tokens::Space) -> (u32, u32) {
    ((s.icon_gap * 2.0).round() as u32, (s.xl * 2.0).min(s.switcher_thumb / 2.0).round() as u32)
}

fn icon_key(id: &str, big: bool) -> String {
    format!("switcher{}:{id}", if big { "-big" } else { "" })
}

impl App {
    pub fn switcher_enabled(&self) -> bool {
        self.store.config.bool("switcher.enabled") != Some(false)
    }

    fn switcher_entries(&self) -> Vec<&compositor::Window> {
        let m = &self.switcher.model;
        model::entries(&self.store.compositor.snapshot.windows, &m.open_history, &m.open_workspace)
    }

    fn switcher_probe(&self) {
        let s = &self.store.theme.theme.space;
        let queries = |big: bool| -> Vec<appicon::Query> {
            self.switcher_entries()
                .iter()
                .map(|w| appicon::Query {
                    id: icon_key(&w.id, big),
                    app_id: w.app_id.clone(),
                    initial_class: w.initial_class.clone(),
                    initial_title: w.initial_title.clone(),
                    pid: w.pid,
                })
                .collect()
        };
        let scale = self.switcher.win.as_ref().map_or_else(crate::scene::raster_scale, |(s, _)| s.scale());
        let (small, big) = icon_sizes(s);
        appicon::probe(small, scale, queries(false));
        appicon::probe(big, scale, queries(true));
    }

    /// `next` and `prev`: the first opens with the cursor one step along,
    /// every one after walks the row.
    pub fn switcher_step(&mut self, direction: i32) {
        let now = Instant::now();
        if self.switcher.model.open {
            let count = self.switcher_entries().len();
            let m = &mut self.switcher.model;
            m.index = model::advance(m.index, count, direction);
            self.switcher.dirty = true;
            return;
        }
        let m = &mut self.switcher.model;
        // A repeat spawned just before Alt came up can land after the commit.
        if m.committed_at.is_some_and(|t| now.duration_since(t) < switcher::COMMIT_RACE) {
            return;
        }
        m.open_history = m.history().to_vec();
        m.open_workspace = if self.store.config.bool("switcher.currentWorkspace") == Some(true) {
            self.store.compositor.snapshot.focused_workspace_id.clone()
        } else {
            String::new()
        };
        let count = self.switcher_entries().len();
        let m = &mut self.switcher.model;
        m.index = model::advance(0, count, direction);
        m.primed = false;
        m.open = true;
        m.scroll = 0.0;
        m.show_at = Some(now + switcher::SHOW_AFTER);
        compositor::send(compositor::Command::RefreshWindows);
        self.switcher_probe();
        if self.switcher.model.commit_pending.take().is_some() {
            self.switcher_commit_now();
        }
    }

    /// Bound to a modifier's release, so it arrives on every tap: with
    /// nothing open it waits a moment for the `next` it may have overtaken.
    pub fn switcher_commit(&mut self) -> bool {
        if self.switcher.model.open {
            return self.switcher_commit_now();
        }
        self.switcher.model.commit_pending = Some(Instant::now() + switcher::COMMIT_RACE);
        true
    }

    fn switcher_commit_now(&mut self) -> bool {
        let now = Instant::now();
        let id = self.switcher_entries().get(self.switcher.model.index).map(|w| w.id.clone()).unwrap_or_default();
        let (primed, shown) = (self.switcher.model.primed, self.switcher.model.shown);
        self.switcher_close();
        self.switcher.model.committed_at = Some(now);
        if id.is_empty() {
            return false;
        }
        // Inside the prime the layer still holds the keyboard exclusively,
        // and Hyprland hands focus back to where it came from once it lets
        // go: one dispatch after that release, not one either side of it.
        if primed || !shown {
            compositor::focus_window(&id);
        } else {
            self.switcher.model.refocus = Some((id, now + switcher::REFOCUS));
        }
        true
    }

    pub fn switcher_close(&mut self) {
        let now = Instant::now();
        let theme = &self.store.theme.theme;
        let m = &mut self.switcher.model;
        m.show_at = None;
        m.open = false;
        if m.shown {
            m.shown = false;
            m.set_fade(theme, now, self.motion_scale, false);
        }
        if let Some((s, _)) = &self.switcher.win {
            s.layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            s.layer.commit();
        }
        self.switcher.dirty = true;
    }

    fn switcher_show(&mut self, now: Instant) {
        let m = &mut self.switcher.model;
        m.shown = true;
        m.shows += 1;
        m.capturing = false;
        m.prime_at = None;
        let theme = &self.store.theme.theme;
        m.set_fade(theme, now, self.motion_scale, true);
        if self.switcher.win.is_none() {
            let surface = self.compositor.create_surface(&self.qh);
            let layer = self.layer_shell.create_layer_surface(&self.qh, surface, Layer::Overlay, Some("formalshell:switcher"), None);
            layer.set_anchor(Anchor::all());
            layer.set_size(0, 0);
            layer.set_exclusive_zone(-1);
            layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
            layer.commit();
            let mut surface = Surface::new("switcher", layer, &self.shm, self.started);
            surface.wait_map = true;
            let (w, h) = self.output_size();
            self.switcher.win = Some((surface, Scene::clear(w as i32, h as i32)));
        } else if let Some((s, _)) = &self.switcher.win {
            s.layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
            s.layer.commit();
            self.switcher.model.prime_at = Some(now + switcher::PRIME);
            self.switcher.model.capture_at = Some(now + switcher::CAPTURE_AFTER);
        }
        self.switcher.dirty = true;
        self.log("switcher shown");
    }

    /// A key while the card holds the keyboard, as its KeyCatcher binds it.
    pub fn switcher_key(&mut self, key: Keysym) -> bool {
        if !self.switcher.model.shown {
            return false;
        }
        match key {
            Keysym::Tab | Keysym::Right | Keysym::Down => self.switcher_step(1),
            Keysym::ISO_Left_Tab | Keysym::Left | Keysym::Up => self.switcher_step(-1),
            Keysym::Return | Keysym::KP_Enter | Keysym::space => {
                self.switcher_commit();
            }
            Keysym::Escape => self.switcher_close(),
            _ => {}
        }
        true
    }

    pub fn switcher_state(&self) -> String {
        let m = &self.switcher.model;
        let entries = self.switcher_entries();
        let selected = entries.get(m.index);
        let cells: Vec<serde_json::Value> = entries
            .iter()
            .map(|w| serde_json::json!({"id": w.id, "captured": self.capture_image(Owner::Switcher, &w.id).is_some()}))
            .collect();
        let captured = entries.iter().filter(|w| self.capture_image(Owner::Switcher, &w.id).is_some()).count();
        serde_json::json!({
            "open": m.open,
            "shown": m.shown,
            "shows": m.shows,
            "capturedCells": cells,
            "index": m.index,
            "count": entries.len(),
            "id": selected.map_or("", |w| w.id.as_str()),
            "title": selected.map_or("", |w| w.title.as_str()),
            "captured": captured,
            "capturing": self.capture_sourced(Owner::Switcher),
            "cells": if m.shown { m.cells_json() } else { serde_json::json!([]) },
        })
        .to_string()
    }

    /// The compositor or an icon moved under the card.
    pub fn switcher_changed(&mut self, topic: crate::store::Topic) {
        use crate::store::Topic;
        if topic == Topic::Compositor {
            let id = self.store.compositor.snapshot.focused_window_id.clone();
            self.switcher.model.focused(&id);
            if self.switcher.model.open {
                self.switcher_probe();
            }
        }
        if self.switcher.model.open || self.switcher.win.is_some() {
            self.switcher.dirty = true;
        }
    }

    pub fn thumbs_changed(&mut self, owner: Owner) {
        match owner {
            Owner::Switcher => self.switcher.dirty = true,
            Owner::Preview => self.preview_thumbs_changed(),
        }
    }

    /// The switcher's own clocks, read off its deadlines.
    pub fn switcher_deadline(&self) -> Option<Instant> {
        let m = &self.switcher.model;
        let refresh = m.refresh_at.filter(|_| m.shown && m.capturing);
        [m.show_at, m.commit_pending, m.prime_at, m.capture_at, refresh, m.refocus.as_ref().map(|r| r.1)].into_iter().flatten().min()
    }

    pub fn switcher_frame(&mut self) {
        let now = Instant::now();
        let Some((s, _)) = &mut self.switcher.win else { return };
        (s.frame_pending, s.callbacks) = (false, s.callbacks + 1);
        if !s.mapped {
            s.mapped = true;
            let m = &mut self.switcher.model;
            m.prime_at = Some(now + switcher::PRIME);
            m.capture_at = Some(now + switcher::CAPTURE_AFTER);
        }
        self.switcher.dirty = true;
    }

    pub fn switcher_configure(&mut self, width: i32, height: i32) {
        let Some((s, scene)) = &mut self.switcher.win else { return };
        if (scene.size.w, scene.size.h) != (width, height) && width > 0 && height > 0 {
            scene.resize(width, height);
        }
        s.configure(scene.size.w, scene.size.h);
        self.switcher.dirty = true;
    }

    /// Runs what is due, then draws the card.
    pub fn switcher_present(&mut self) {
        let now = Instant::now();
        let due = |t: Option<Instant>| t.is_some_and(|t| t <= now);
        let m = &mut self.switcher.model;
        if due(m.commit_pending) {
            m.commit_pending = None;
        }
        if m.refocus.as_ref().is_some_and(|r| r.1 <= now) {
            let (id, _) = m.refocus.take().expect("refocus");
            if !m.open {
                compositor::focus_window(&id);
            }
        }
        if due(m.show_at) {
            m.show_at = None;
            if m.open {
                self.switcher_show(now);
            }
        }
        let m = &mut self.switcher.model;
        if due(m.prime_at) {
            m.prime_at = None;
            if m.shown {
                m.primed = true;
                if let Some((s, _)) = &self.switcher.win {
                    s.layer.set_keyboard_interactivity(KeyboardInteractivity::OnDemand);
                    s.layer.commit();
                }
            }
        }
        let m = &mut self.switcher.model;
        if due(m.capture_at) {
            m.capture_at = None;
            if m.shown {
                m.capturing = true;
                m.refresh_at = Some(now + switcher::REFRESH);
                self.switcher.dirty = true;
            }
        }
        let m = &mut self.switcher.model;
        if m.shown && m.capturing && due(m.refresh_at) {
            m.refresh_at = Some(now + switcher::REFRESH);
            if let Some(id) = self.switcher_entries().get(self.switcher.model.index).map(|w| w.id.clone()) {
                self.capture_refresh(Owner::Switcher, &id);
            }
        }

        let (alpha, fading) = self.switcher.model.alpha(now);
        if self.switcher.win.as_ref().is_some_and(|(s, _)| s.mapped) && !self.switcher.model.shown && !fading {
            self.switcher.win = None;
            self.switcher.model.capturing = false;
            self.switcher.model.cells.clear();
            self.switcher.model.nodes.clear();
            self.capture_clear(Owner::Switcher);
            self.log("switcher unmapped");
            return;
        }
        let ready = self.switcher.win.as_ref().is_some_and(|(s, _)| s.configured && !s.frame_pending);
        if !ready || !(self.switcher.dirty || fading) {
            return;
        }
        self.switcher.dirty = false;

        let entries: Vec<compositor::Window> = self.switcher_entries().into_iter().cloned().collect();
        let refs: Vec<&compositor::Window> = entries.iter().collect();
        let scale = self.switcher.win.as_ref().map_or(1.0, |(s, _)| s.scale());
        let (small, big) = icon_sizes(&self.store.theme.theme.space);
        let icon = |key: String, size: u32| self.store.appicon.get(&key, size, scale);
        let icons: Vec<(String, Option<Bitmap>, Option<Bitmap>)> = entries
            .iter()
            .map(|w| {
                let little = icon(icon_key(&w.id, false), small);
                let entry = little.as_ref().and_then(|i| i.entry.as_ref());
                let key = entry.map_or_else(|| if w.app_id.is_empty() { w.initial_class.clone() } else { w.app_id.clone() }, |e| format!("entry:{}", e.id));
                (key, little.and_then(|i| i.image), icon(icon_key(&w.id, true), big).and_then(|i| i.image))
            })
            .collect();
        let thumbs: Vec<Option<Bitmap>> = entries.iter().map(|w| self.capture_image(Owner::Switcher, &w.id).cloned()).collect();
        let output = self.output_size();
        let input = Input { theme: &self.store.theme.theme, output, entries: &refs, icons: &icons, thumbs: &thumbs, alpha };
        let Some((surface, scene)) = &mut self.switcher.win else { return };
        let _raster = crate::scene::raster_at(scale);
        self.switcher.model.draw(&input, &mut self.bar.kit, scene);
        let qh = self.qh.clone();
        surface.present(scene, fading, &qh);
        if self.switcher.model.capturing && self.switcher.model.shown {
            let wants = self.switcher.model.thumb_sizes(&refs);
            let scale = self.switcher.win.as_ref().map_or(1.0, |(s, _)| s.scale());
            self.capture_set(Owner::Switcher, &wants, false, scale);
        }
    }
}
