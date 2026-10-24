//! The session's boot (spec C6): the table's sequence played once per
//! login as the shell's first frame over the desktop, one black overlay per
//! output, the first taking the keyboard. Any key or press goes straight to
//! the way out; left alone it goes [`boot::HOLD`] after the emblem has
//! assembled. The bar waits under it at no opacity and slams in from the
//! leading side as the black lifts; the backdrop's sculpture starts turning
//! once the overlay is gone.
//!
//! state.json's `boot` keeps the login it played for, so a shell restarted
//! in the same session shows the desktop at once. A login through the
//! greeter carries [`boot::HANDOFF_ENV`]: the greeter played the sequence
//! and crumpled its emblem, so the session starts on the black and only
//! lifts it.

use std::time::Instant;

use fs_audio::sfx::Sound;
use serde_json::{Value, json};
use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerSurface};

use super::App;
use crate::boot::{self, Plan};
use crate::effects::since;
use crate::scene::{NodeId, Scene};
use crate::services::state;
use crate::surface::Surface;
use crate::surfaces::bar::cell::Painter;
use crate::surfaces::boot::{self as view, Emblem, Frame};

/// The bar's slam in: `slam` from the preview, 0.06 of the output's width
/// over 0.7s on the ease, fading up as it lands.
const SLAM: f64 = 0.06;
const SLAM_MS: f64 = 700.0;

struct Out {
    output: WlOutput,
    surface: Surface<LayerSurface>,
    scene: Scene,
    nodes: Vec<NodeId>,
    lines: Vec<(f64, f64, f64, f64)>,
    plan: Option<Plan>,
}

#[derive(Default)]
pub struct Boot {
    outs: Vec<Out>,
    emblem: Option<Emblem>,
    /// The clock, from the first overlay's configure.
    start: Option<Instant>,
    /// The way out: when it started and the way-in time it holds.
    out: Option<(Instant, f64)>,
    handoff: bool,
    /// The login it plays for, written to state.json once it leaves.
    key: Option<String>,
    /// `off`, `played`, `playing`, `leaving` or `done`, for `debug dump`.
    state: &'static str,
    why: &'static str,
    commits: u64,
    /// When the bar started sliding in, and whether it is still held back.
    slam: Option<Instant>,
    held: bool,
    /// The wordmark's blip has played.
    chimed: bool,
}

impl App {
    /// Plays the boot when the table has one and this login has not seen
    /// it, decided once settings.json has been read.
    pub fn boot_begin(&mut self) {
        if !self.boot.state.is_empty() || !self.store.config.loaded {
            return;
        }
        let theme = &self.store.theme.theme;
        if !theme.boot || !theme.motion_enabled {
            self.boot.state = "off";
            return;
        }
        let key = boot::this_login();
        if played(&state::state_path()) == key {
            self.boot.state = "played";
            self.log("boot already played for this login");
            return;
        }
        self.boot.handoff = std::env::var(boot::HANDOFF_ENV).is_ok_and(|v| v == boot::HANDOFF);
        self.boot.key = Some(key);
        self.boot.state = "playing";
        self.boot.held = true;
        if !self.boot.handoff {
            let host = boot::hostname();
            self.boot.emblem = Some(Emblem::new(&mut self.bar.kit, theme, [host.as_str(), "FORMAL"]));
        }
        let outputs: Vec<WlOutput> = self.heads.iter().map(|h| h.output.clone()).collect();
        for (i, output) in outputs.into_iter().enumerate() {
            self.boot_map(output, i == 0);
        }
        self.log(&format!("boot playing{}", if self.boot.handoff { " (greeter handoff)" } else { "" }));
    }

    fn boot_map(&mut self, output: WlOutput, keyboard: bool) {
        let wl = self.compositor.create_surface(&self.qh);
        let layer = self.layer_shell.create_layer_surface(&self.qh, wl, Layer::Overlay, Some("formalshell:boot"), Some(&output));
        layer.set_anchor(Anchor::all());
        layer.set_size(0, 0);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(if keyboard { KeyboardInteractivity::Exclusive } else { KeyboardInteractivity::None });
        layer.commit();
        let mut surface = Surface::on("boot", layer, &self.shm, self.started, &output);
        surface.wait_map = true;
        self.boot.outs.push(Out { output, surface, scene: Scene::new(1, 1), nodes: Vec::new(), lines: Vec::new(), plan: None });
    }

    fn boot_index(&self, surface: &WlSurface) -> Option<usize> {
        self.boot.outs.iter().position(|o| o.surface.layer.wl_surface() == surface)
    }

    pub(super) fn boot_frame(&mut self, surface: &WlSurface) -> bool {
        let Some(i) = self.boot_index(surface) else { return false };
        let s = &mut self.boot.outs[i].surface;
        s.landed(Instant::now());
        s.mapped = true;
        true
    }

    pub(super) fn boot_configure(&mut self, surface: &WlSurface, width: i32, height: i32) -> bool {
        let Some(i) = self.boot_index(surface) else { return false };
        if width <= 0 || height <= 0 {
            return true;
        }
        let theme = &self.store.theme.theme;
        let counts = self.boot.emblem.as_ref().map_or_else(Vec::new, Emblem::counts);
        let o = &mut self.boot.outs[i];
        o.surface.configure(width, height);
        if (o.scene.size.w, o.scene.size.h) != (width, height) || o.plan.is_none() {
            o.scene = Scene::new(width, height);
            o.nodes.clear();
            o.lines = view::grid_lines(theme, (width, height));
            o.plan = Some(Plan::new(&view::columns(&o.lines, (width, height)), &counts, boot::SEED));
        }
        self.boot.start.get_or_insert_with(Instant::now);
        true
    }

    pub(super) fn boot_closed(&mut self, surface: &WlSurface) -> bool {
        let Some(i) = self.boot_index(surface) else { return false };
        self.boot.outs.remove(i);
        true
    }

    pub(super) fn boot_output_gone(&mut self, output: &WlOutput) {
        self.boot.outs.retain(|o| o.output != *output);
    }

    /// A key while the overlay is up: it goes to the way out, and no
    /// further.
    pub(super) fn boot_key(&mut self) -> bool {
        if self.boot.outs.is_empty() {
            return false;
        }
        self.boot_leave("key");
        true
    }

    /// Presses on the overlay skip like a key; the rest of the events pass.
    pub(super) fn boot_pointer(&mut self, events: &[PointerEvent]) -> Vec<PointerEvent> {
        let mut rest = Vec::new();
        for e in events {
            if self.boot_index(&e.surface).is_none() {
                rest.push(e.clone());
                continue;
            }
            match e.kind {
                PointerEventKind::Enter { serial } => {
                    if let Some(p) = &self.pointer {
                        p.set_cursor(serial, None, 0, 0);
                    }
                }
                PointerEventKind::Press { .. } => self.boot_leave("press"),
                _ => {}
            }
        }
        rest
    }

    fn boot_leave(&mut self, why: &'static str) {
        if self.boot.out.is_some() {
            return;
        }
        let now = Instant::now();
        let t = self.boot.start.map_or(0.0, |s| since(s, now));
        self.boot.out = Some((now, t));
        self.boot.slam = Some(now);
        self.boot.state = "leaving";
        self.boot.why = why;
        if let Some(key) = self.boot.key.take() {
            state::set(vec![state::Field::Boot(key)]);
        }
        // The keyboard goes back to whatever had it before the boot.
        for o in &self.boot.outs {
            o.surface.layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        }
        self.log(&format!("boot leaving {why} t={}ms", t.round()));
        self.sfx(Sound::Open);
    }

    /// The overlay holds the desktop: the sculpture waits for it.
    pub(super) fn boot_holds(&self) -> bool {
        !self.boot.outs.is_empty()
    }

    /// The bar's place and opacity while the boot holds it or it slams in;
    /// `None` once it rests.
    pub(super) fn boot_bar(&mut self, now: Instant) -> Option<(f64, f32)> {
        if !self.boot.held {
            return None;
        }
        let width = f64::from(self.bar.scene.size.w.max(self.bar.scene.size.h));
        let Some(at) = self.boot.slam else {
            // Every overlay went with its output before the way out.
            if self.boot.outs.is_empty() {
                self.boot.held = false;
                return Some((0.0, 1.0));
            }
            return Some((-SLAM * width, 0.0));
        };
        let p = boot::progress(since(at, now), SLAM_MS * self.motion_scale.max(0.0));
        if p >= 1.0 {
            self.boot.held = false;
            return Some((0.0, 1.0));
        }
        let e = boot::EASE.ease(p);
        Some((-SLAM * width * (1.0 - e), e.clamp(0.0, 1.0) as f32))
    }

    pub(super) fn present_boot(&mut self, now: Instant) {
        if self.boot.outs.is_empty() {
            return;
        }
        let scale = self.motion_scale.max(0.01);
        let t = self.boot.start.map_or(0.0, |s| since(s, now) / scale);
        if self.boot.out.is_none() && self.boot.start.is_some() {
            if self.boot.handoff {
                self.boot_leave("handoff");
            } else {
                let assembled = self.boot.outs.iter().filter_map(|o| o.plan.as_ref()).map(Plan::assembled).fold(0.0, f64::max);
                if t >= assembled + boot::HOLD {
                    self.boot_leave("auto");
                }
            }
        }
        if !self.boot.chimed && self.boot.out.is_none() && !self.boot.handoff && t >= boot::LETTERS_AT {
            self.boot.chimed = true;
            self.sfx(Sound::Boot);
        }
        let (t, out) = match self.boot.out {
            Some((at, held)) => (held / scale, Some(since(at, now) / scale)),
            None => (t, None),
        };
        if out.is_some_and(boot::gone) {
            self.boot.outs.clear();
            self.boot.state = "done";
            self.log(&format!("boot done after {} commits", self.boot.commits));
            self.sync_shards();
            return;
        }
        let handoff = self.boot.handoff;
        let frame = |size: (i32, i32)| Frame {
            t,
            out,
            at: (f64::from(size.0) / 2.0, f64::from(size.1) / 2.0),
            scale: 1.0,
            black: out.map_or(1.0, boot::lift),
            lines: if handoff { 0.0 } else { out.map_or(1.0, boot::lines_out) },
            emblem: if handoff { 0.0 } else { 1.0 },
        };
        let theme = &self.store.theme.theme;
        let qh = self.qh.clone();
        let Boot { outs, emblem, commits, .. } = &mut self.boot;
        let empty = Emblem::empty();
        let emblem = emblem.as_ref().unwrap_or(&empty);
        for o in outs.iter_mut() {
            let Some(plan) = &o.plan else { continue };
            if !o.surface.configured || o.surface.frame_pending {
                continue;
            }
            let size = (o.scene.size.w, o.scene.size.h);
            let _raster = crate::scene::raster_at(o.surface.scale());
            let mut p = Painter::new(&mut o.scene, &mut o.nodes, None);
            view::paint(&mut p, theme, size, &o.lines, plan, emblem, &frame(size));
            p.finish();
            let before = o.surface.commits();
            o.surface.present(&mut o.scene, true, &qh);
            *commits += o.surface.commits() - before;
        }
    }

    /// `debug dump`'s `boot` block.
    pub(crate) fn boot_state(&self) -> Value {
        json!({
            "state": if self.boot.state.is_empty() { "off" } else { self.boot.state },
            "why": self.boot.why,
            "handoff": self.boot.handoff,
            "outputs": self.boot.outs.len(),
            "commits": self.boot.commits,
        })
    }
}

/// The login state.json says the boot last played for, read straight off
/// the file: the boot decides before the state service has loaded it.
fn played(path: &std::path::Path) -> String {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|v| v.get("boot").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_played_login_is_read_off_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        assert_eq!(played(&path), "");
        std::fs::write(&path, r#"{"boot": "b/s", "dnd": false}"#).unwrap();
        assert_eq!(played(&path), "b/s");
        std::fs::write(&path, "{").unwrap();
        assert_eq!(played(&path), "");
    }
}
