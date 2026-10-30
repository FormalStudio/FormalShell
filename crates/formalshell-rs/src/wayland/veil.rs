//! The session's arrival from black and its departure into it
//! (docs/superpowers/specs/2026-10-30-seams.md, stages 7 and 8).
//!
//! The first start for a compositor instance covers every output with an
//! opaque veil before anything else maps, and lifts it once the bar and
//! every wallpaper have shown their first frame. A restart inside the same
//! session finds the instance's marker and maps none. Shutdown, reboot and
//! logout all leave the same way: the veil fades in over everything, then
//! logind's inhibitor goes (shutdown) or the compositor is told to exit
//! (logout).

use std::io::ErrorKind;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use calloop::channel;
use calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::compositor::Region;
use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer};

use super::App;
use crate::services::compositor::{self, Command};
use crate::surface::Surface;
use crate::surfaces::veil::Veil;

/// The lift once the desktop is in, and how long it waits for the bar and
/// the wallpapers before lifting without them.
const ARRIVE_MS: f64 = 500.0;
const ARRIVE_WAIT: Duration = Duration::from_secs(2);
/// The fade to black, and how long the departure waits on the compositor
/// to show its end.
const DEPART_MS: f64 = 300.0;
const DEPART_WAIT: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Arriving,
    /// logind's PrepareForShutdown.
    Departing,
    /// `power logout`.
    Leaving,
}

impl Phase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Arriving => "arriving",
            Self::Departing => "departing",
            Self::Leaving => "leaving",
        }
    }
}

#[derive(Default)]
pub(super) struct State {
    phase: Option<Phase>,
    veils: Vec<(WlOutput, Veil)>,
    /// Fading out, to be dropped once that has been shown.
    lifting: bool,
    /// The departure's end acted on: the inhibitor let go, or the exit sent.
    ended: bool,
    /// Bumped per fade, so only the current one's wait acts.
    generation: u64,
    /// The pointer is still to be shown once the shell is up.
    reveal: bool,
}

/// The first start for this compositor instance, marked so a restart
/// inside the session is not.
fn first_arrival() -> bool {
    let id = ["HYPRLAND_INSTANCE_SIGNATURE", "MANGO_INSTANCE_SIGNATURE", "WAYLAND_DISPLAY"]
        .into_iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .unwrap_or_default();
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("formalshell");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("arrived-{}", id.replace('/', "_")));
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(_) => true,
        Err(err) if err.kind() == ErrorKind::AlreadyExists => false,
        Err(err) => {
            eprintln!("veil: {}: {err}", path.display());
            false
        }
    }
}

impl App {
    /// Called before the first head is made, so an arrival's veils are the
    /// first surfaces committed.
    pub(super) fn begin_arrival(&mut self) {
        self.veil.reveal = true;
        if first_arrival() {
            self.veil.phase = Some(Phase::Arriving);
        }
    }

    /// The arrival's wait, and logind's shutdown signal on the service thread.
    pub(super) fn start_veil(&mut self) {
        let Some(handle) = &self.handle else { return };
        let _ = handle.insert_source(Timer::from_deadline(self.started + ARRIVE_WAIT), |_, _, _: &mut App| TimeoutAction::Drop);
        let (tx, rx) = channel::channel::<bool>();
        let inserted = handle.insert_source(rx, |event, _, app: &mut App| {
            if let channel::Event::Msg(going) = event {
                app.prepare_for_shutdown(going);
            }
        });
        if inserted.is_ok()
            && let Some(rt) = &self.runtime
        {
            rt.service(move |ctx| crate::services::shutdown::start(ctx, tx));
        }
    }

    pub(crate) fn veil_phase(&self) -> Option<&'static str> {
        self.veil.phase.map(Phase::as_str)
    }

    /// A veil over a new output, while an arrival is still waiting for the
    /// shell's first frame; an output added after that gets none.
    pub(super) fn veil_output(&mut self, output: &WlOutput) {
        let up = self.veil.phase == Some(Phase::Arriving) && !self.veil.lifting && !self.faces_warmed;
        if up && !self.veil.veils.iter().any(|(o, _)| o == output) {
            let veil = self.new_veil(output, 1.0);
            self.veil.veils.push((output.clone(), veil));
        }
    }

    fn new_veil(&self, output: &WlOutput, opacity: f32) -> Veil {
        let wl = self.compositor.create_surface(&self.qh);
        let ls = self.layer_shell.create_layer_surface(&self.qh, wl, Layer::Overlay, Some("formalshell:veil"), Some(output));
        ls.set_anchor(Anchor::all());
        ls.set_exclusive_zone(-1);
        ls.set_keyboard_interactivity(KeyboardInteractivity::None);
        if let Ok(region) = Region::new(&self.compositor) {
            ls.set_input_region(Some(region.wl_region()));
        }
        ls.commit();
        Veil::new(Surface::on("veil", ls, &self.shm, self.started, output), opacity, Instant::now())
    }

    fn veil_index(&self, surface: &WlSurface) -> Option<usize> {
        self.veil.veils.iter().position(|(_, v)| v.surface.layer.wl_surface() == surface)
    }

    pub(super) fn veil_owns(&self, surface: &WlSurface) -> bool {
        self.veil_index(surface).is_some()
    }

    /// The veil's first frame goes out at once, ahead of whatever else this
    /// dispatch configures.
    pub(super) fn veil_configure(&mut self, surface: &WlSurface, width: i32, height: i32) {
        let Some(i) = self.veil_index(surface) else { return };
        let v = &mut self.veil.veils[i].1;
        v.configure(width, height);
        v.present(Instant::now(), &self.qh);
    }

    pub(super) fn veil_frame(&mut self, surface: &WlSurface) -> bool {
        let Some(i) = self.veil_index(surface) else { return false };
        self.veil.veils[i].1.landed(Instant::now());
        true
    }

    pub(super) fn veil_closed(&mut self, surface: &WlSurface) {
        self.veil.veils.retain(|(_, v)| v.surface.layer.wl_surface() != surface);
    }

    pub(super) fn veil_output_gone(&mut self, output: &WlOutput) {
        self.veil.veils.retain(|(o, _)| o != output);
    }

    fn fade_veils(&mut self, to: f32, ms: f64) {
        let now = Instant::now();
        for (_, v) in &mut self.veil.veils {
            v.fade(to, ms * self.motion_scale, now);
        }
        self.veil.generation += 1;
    }

    /// Black over every output, then `phase`'s end once it has been shown.
    fn cover(&mut self, phase: Phase) {
        if self.veil.phase == Some(phase) && !self.veil.lifting {
            return;
        }
        let outputs: Vec<WlOutput> = self.heads.iter().map(|h| h.output.clone()).collect();
        for output in outputs {
            if !self.veil.veils.iter().any(|(o, _)| *o == output) {
                let veil = self.new_veil(&output, 0.0);
                self.veil.veils.push((output, veil));
            }
        }
        (self.veil.phase, self.veil.lifting, self.veil.ended) = (Some(phase), false, false);
        self.fade_veils(1.0, DEPART_MS);
        compositor::send(Command::Cursor { hidden: true });
        let generation = self.veil.generation;
        if let Some(handle) = &self.handle {
            let _ = handle.insert_source(Timer::from_duration(DEPART_WAIT), move |_, _, app: &mut App| {
                if app.veil.generation == generation && !app.veil.ended {
                    eprintln!("veil: not shown in {}ms, going on under it", DEPART_WAIT.as_millis());
                    app.end_departure();
                }
                TimeoutAction::Drop
            });
        }
    }

    fn lift(&mut self) {
        self.veil.lifting = true;
        let ms = if self.veil.phase == Some(Phase::Arriving) { ARRIVE_MS } else { DEPART_MS };
        self.fade_veils(0.0, ms);
    }

    fn end_departure(&mut self) {
        self.veil.ended = true;
        match self.veil.phase {
            Some(Phase::Departing) => {
                if let Some(rt) = &self.runtime {
                    rt.service(|_| crate::services::shutdown::release());
                }
            }
            Some(Phase::Leaving) => compositor::send(Command::Exit),
            Some(Phase::Arriving) | None => {}
        }
    }

    fn prepare_for_shutdown(&mut self, going: bool) {
        if going {
            self.cover(Phase::Departing);
        } else if self.veil.phase == Some(Phase::Departing) {
            self.lift();
        }
    }

    /// `power logout`.
    pub fn logout(&mut self) {
        self.cover(Phase::Leaving);
    }

    /// Each veil drawn at this moment's opacity, and the phase moved on
    /// once the compositor has shown where its fade ends.
    pub(super) fn present_veils(&mut self, now: Instant) {
        if self.veil.reveal && self.veil.phase.is_none() && (self.faces_warmed || now >= self.started + ARRIVE_WAIT) {
            self.veil.reveal = false;
            compositor::send(Command::Cursor { hidden: false });
        }
        let Some(phase) = self.veil.phase else { return };
        if phase == Phase::Arriving && !self.veil.lifting {
            let bar = self.bar_surface.as_ref().map_or(self.bar.hidden(), |s| s.mapped);
            // With no picture to decode, the plain colour is the wallpaper.
            let plain = self.store.desktop().is_empty();
            let walls = self.heads.iter().all(|h| h.backdrop.as_ref().is_none_or(|b| b.landed.1 || (plain && b.landed.0)));
            if (bar && walls && !self.heads.is_empty()) || now >= self.started + ARRIVE_WAIT {
                self.lift();
            }
        }
        for (_, v) in &mut self.veil.veils {
            v.present(now, &self.qh);
        }
        let settled = self.veil.veils.iter().all(|(_, v)| v.settled());
        if self.veil.lifting && settled {
            self.veil.veils.clear();
            (self.veil.phase, self.veil.lifting) = (None, false);
            self.veil.reveal = false;
            compositor::send(Command::Cursor { hidden: false });
        } else if phase != Phase::Arriving && !self.veil.lifting && !self.veil.ended && settled {
            self.end_departure();
        }
    }
}
