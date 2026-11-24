//! The time a frame is drawn for. A frame callback reaches the shell
//! somewhere inside the refresh interval, later when the compositor was busy
//! with another output's frame (1.5 to 7ms after the vblank on g815's two
//! outputs, of a 6.9ms interval), so motion sampled at the callback's own
//! arrival moves by uneven steps between evenly spaced vblanks. Each
//! surface's frames are stepped along its output's vblanks instead, read off
//! wp_presentation: a callback straight after the last one is the next
//! vblank, whenever inside the interval it arrived.
//!
//! The times stay within an interval of the wall clock, which everything
//! outside a frame callback still reads. An output with no fixed refresh
//! (variable refresh reports 0) and a compositor without wp_presentation
//! keep the wall clock.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use smithay_client_toolkit::dispatch2::Dispatch2;
use smithay_client_toolkit::output::OutputState;
use smithay_client_toolkit::reexports::client::backend::ObjectId;
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::reexports::client::{Connection, Proxy, QueueHandle};
use smithay_client_toolkit::reexports::protocols::wp::presentation_time::client::wp_presentation::{self, WpPresentation};
use smithay_client_toolkit::reexports::protocols::wp::presentation_time::client::wp_presentation_feedback::{
    self, WpPresentationFeedback,
};

use super::App;

/// A vblank older than this places no frame: the refresh a compositor
/// reports is a mode's nominal one, and its error grows with every interval
/// counted from the vblank.
const STALE_NS: i64 = 10_000_000_000;
/// Latencies kept per output for the dump.
const LATENCIES: usize = 512;
/// Surfaces are never told apart from destroyed ones, so the ones not drawn
/// for this long are dropped once the map has grown.
const FORGET: Duration = Duration::from_secs(60);

/// One vblank of an output and its refresh interval, in nanoseconds of the
/// presentation clock.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Lattice {
    at: i64,
    period: i64,
}

/// The frame before this one on the same surface.
#[derive(Clone, Copy, Debug)]
struct Prev {
    now: i64,
    time: i64,
    /// Whether `time` was a vblank's.
    snapped: bool,
}

/// The time to draw the frame asked for at `now`, and whether its callback
/// followed the previous frame's straight away.
fn step(lattice: Option<Lattice>, prev: Option<Prev>, now: i64) -> (i64, bool, bool) {
    let Some(l) = lattice.filter(|l| l.period > 0 && (now - l.at).abs() < STALE_NS) else {
        return (now, false, false);
    };
    let at_or_before = (now - l.at).div_euclid(l.period);
    let Some(p) = prev.filter(|p| p.snapped) else {
        return (l.at + at_or_before * l.period, true, false);
    };
    let last = ((p.time - l.at) as f64 / l.period as f64).round() as i64;
    let consecutive = (now - p.now) * 2 < l.period * 3;
    let index = if consecutive && (at_or_before - (last + 1)).abs() <= 1 { last + 1 } else { at_or_before.max(last + 1) };
    (l.at + index * l.period, true, consecutive)
}

#[derive(Default)]
struct Counts {
    /// Frames stepped along the vblanks, and frames left on the wall clock.
    snapped: u64,
    wall: u64,
    presented: u64,
    discarded: u64,
    /// Vblanks between two frames whose callbacks followed one another: one,
    /// two, and three or more.
    steps: [u64; 3],
    /// Microseconds from a frame's callback to its vblank.
    latencies: VecDeque<u32>,
}

struct Output {
    output: WlOutput,
    lattice: Option<Lattice>,
    counts: Counts,
}

struct Surface {
    output: Option<ObjectId>,
    prev: Option<Prev>,
    presented: Option<i64>,
    seen: Instant,
}

/// A frame's feedback: the surface, when its callback arrived, and whether
/// that callback followed the previous frame's.
pub struct Asked {
    surface: ObjectId,
    drawn: i64,
    consecutive: bool,
}

/// wp_presentation's own user data: its one event names the clock.
pub struct Clock;

pub struct Pacing {
    presentation: Option<WpPresentation>,
    /// Whether presentation times are CLOCK_MONOTONIC's, `Instant`'s clock.
    monotonic: bool,
    /// One instant and its nanoseconds on that clock.
    anchor: (Instant, i64),
    outputs: HashMap<ObjectId, Output>,
    surfaces: HashMap<ObjectId, Surface>,
}

fn monotonic_ns() -> i64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: `ts` is a valid timespec for the call to fill.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec * 1_000_000_000 + ts.tv_nsec
}

impl Pacing {
    pub fn bind(globals: &GlobalList, qh: &QueueHandle<App>) -> Self {
        Self {
            presentation: globals.bind(qh, 1..=1, Clock).ok(),
            monotonic: false,
            anchor: (Instant::now(), monotonic_ns()),
            outputs: HashMap::new(),
            surfaces: HashMap::new(),
        }
    }

    fn ns(&self, at: Instant) -> i64 {
        match at.checked_duration_since(self.anchor.0) {
            Some(d) => self.anchor.1 + d.as_nanos() as i64,
            None => self.anchor.1 - self.anchor.0.duration_since(at).as_nanos() as i64,
        }
    }

    fn instant(&self, ns: i64) -> Instant {
        let d = Duration::from_nanos((ns - self.anchor.1).unsigned_abs());
        if ns >= self.anchor.1 { self.anchor.0 + d } else { self.anchor.0.checked_sub(d).unwrap_or(self.anchor.0) }
    }

    /// The time `surface`'s frame is drawn for, its callback having arrived
    /// at `now`, with feedback asked for the commit that frame makes.
    pub fn frame(&mut self, surface: &WlSurface, now: Instant, qh: &QueueHandle<App>) -> Instant {
        let Some(presentation) = self.presentation.clone().filter(|_| self.monotonic) else {
            return now;
        };
        let (id, ns) = (surface.id(), self.ns(now));
        if self.surfaces.len() > 64 {
            self.surfaces.retain(|_, s| now.saturating_duration_since(s.seen) < FORGET);
        }
        self.outputs.retain(|_, o| o.output.is_alive());
        let s = self.surfaces.entry(id.clone()).or_insert(Surface { output: None, prev: None, presented: None, seen: now });
        s.seen = now;
        let mut output = s.output.as_ref().and_then(|o| self.outputs.get_mut(o));
        let (time, snapped, consecutive) = step(output.as_ref().and_then(|o| o.lattice), s.prev, ns);
        s.prev = Some(Prev { now: ns, time, snapped });
        if let Some(o) = output.as_mut() {
            *(if snapped { &mut o.counts.snapped } else { &mut o.counts.wall }) += 1;
        }
        presentation.feedback(surface, qh, Asked { surface: id, drawn: ns, consecutive });
        if snapped { self.instant(time) } else { now }
    }

    fn presented(&mut self, asked: &Asked, at: i64, refresh: u32) {
        let Some(s) = self.surfaces.get_mut(&asked.surface) else {
            return;
        };
        let last = s.presented.replace(at);
        let Some(o) = s.output.as_ref().and_then(|o| self.outputs.get_mut(o)) else {
            return;
        };
        o.lattice = (refresh > 0).then_some(Lattice { at, period: refresh as i64 });
        let c = &mut o.counts;
        c.presented += 1;
        if c.latencies.len() == LATENCIES {
            c.latencies.pop_front();
        }
        c.latencies.push_back(((at - asked.drawn).max(0) / 1000).min(u32::MAX as i64) as u32);
        // Two callbacks with one commit between them share its vblank.
        if let (Some(last), true, true) = (last.filter(|l| *l != at), asked.consecutive, refresh > 0) {
            let vblanks = ((at - last) as f64 / refresh as f64).round() as i64;
            c.steps[(vblanks.clamp(1, 3) - 1) as usize] += 1;
        }
    }

    /// `debug dump`'s `pacing`: per output, how its frames were timed and
    /// when they reached the screen.
    pub fn report(&self, outputs: &OutputState) -> Value {
        let mut rows: Vec<Value> = self
            .outputs
            .values()
            .map(|o| {
                let c = &o.counts;
                let mut sorted: Vec<u32> = c.latencies.iter().copied().collect();
                sorted.sort_unstable();
                let at = |q: usize| sorted.get(sorted.len().saturating_sub(1) * q / 100).map(|us| *us as f64 / 1000.0);
                json!({
                    "output": outputs.info(&o.output).and_then(|i| i.name),
                    "refreshHz": o.lattice.map(|l| (1e12 / l.period as f64).round() / 1000.0),
                    "snapped": c.snapped,
                    "wall": c.wall,
                    "presented": c.presented,
                    "discarded": c.discarded,
                    "steps": {"1": c.steps[0], "2": c.steps[1], "3": c.steps[2]},
                    "latencyMs": {"p5": at(5), "p50": at(50), "p95": at(95)},
                })
            })
            .collect();
        rows.sort_by_key(|r| r["output"].as_str().map(str::to_owned));
        json!({"available": self.presentation.is_some() && self.monotonic, "outputs": rows})
    }
}

impl App {
    pub fn pacing_report(&self) -> Value {
        self.pacing.report(&self.outputs)
    }
}

impl Dispatch2<WpPresentation, App> for Clock {
    fn event(&self, app: &mut App, _: &WpPresentation, event: wp_presentation::Event, _: &Connection, _: &QueueHandle<App>) {
        if let wp_presentation::Event::ClockId { clk_id } = event {
            app.pacing.monotonic = clk_id == libc::CLOCK_MONOTONIC as u32;
        }
    }
}

impl Dispatch2<WpPresentationFeedback, App> for Asked {
    fn event(&self, app: &mut App, _: &WpPresentationFeedback, event: wp_presentation_feedback::Event, _: &Connection, _: &QueueHandle<App>) {
        let pacing = &mut app.pacing;
        match event {
            wp_presentation_feedback::Event::SyncOutput { output } => {
                if let Some(s) = pacing.surfaces.get_mut(&self.surface) {
                    s.output = Some(output.id());
                    pacing.outputs.entry(output.id()).or_insert(Output { output, lattice: None, counts: Counts::default() });
                }
            }
            wp_presentation_feedback::Event::Presented { tv_sec_hi, tv_sec_lo, tv_nsec, refresh, .. } => {
                let secs = ((tv_sec_hi as i64) << 32) | tv_sec_lo as i64;
                pacing.presented(self, secs * 1_000_000_000 + tv_nsec as i64, refresh);
            }
            wp_presentation_feedback::Event::Discarded => {
                let output = pacing.surfaces.get(&self.surface).and_then(|s| s.output.clone());
                if let Some(o) = output.and_then(|o| pacing.outputs.get_mut(&o)) {
                    o.counts.discarded += 1;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERIOD: i64 = 6_944_444;
    const LATTICE: Lattice = Lattice { at: 1_000_000_000, period: PERIOD };

    /// Callbacks from 1.5 to 6.5ms into each interval, as g815 sends them.
    #[test]
    fn jittered_callbacks_step_one_vblank_each() {
        let offsets = [1_500_000, 6_500_000, 2_000_000, 4_900_000, 6_800_000, 1_600_000, 3_000_000, 6_900_000, 200_000];
        let (mut prev, mut times) = (None, Vec::new());
        for (i, offset) in offsets.iter().enumerate() {
            let now = LATTICE.at + i as i64 * PERIOD + offset;
            let (time, snapped, _) = step(Some(LATTICE), prev, now);
            assert!(snapped);
            prev = Some(Prev { now, time, snapped });
            times.push(time);
        }
        for pair in times.windows(2) {
            assert_eq!(pair[1] - pair[0], PERIOD, "{times:?}");
        }
    }

    #[test]
    fn a_callback_a_vblank_late_steps_two() {
        let first = LATTICE.at + 2_000_000;
        let (t0, ..) = step(Some(LATTICE), None, first);
        let (t1, _, consecutive) = step(Some(LATTICE), Some(Prev { now: first, time: t0, snapped: true }), first + 2 * PERIOD);
        assert!(!consecutive);
        assert_eq!(t1 - t0, 2 * PERIOD);
    }

    #[test]
    fn a_frame_after_a_rest_is_the_vblank_before_its_callback() {
        let prev = Prev { now: LATTICE.at, time: LATTICE.at, snapped: true };
        let now = LATTICE.at + 100 * PERIOD + 3_000_000;
        assert_eq!(step(Some(LATTICE), Some(prev), now).0, LATTICE.at + 100 * PERIOD);
    }

    /// A lattice refit a little off under a running chain moves no frame by
    /// more than the refit.
    #[test]
    fn a_refit_keeps_the_chain() {
        let now = LATTICE.at + 5 * PERIOD + 6_000_000;
        let prev = Prev { now: now - PERIOD, time: LATTICE.at + 5 * PERIOD, snapped: true };
        let refit = Lattice { at: LATTICE.at + 4 * PERIOD + 300_000, period: PERIOD };
        assert_eq!(step(Some(refit), Some(prev), now).0, refit.at + 2 * PERIOD);
    }

    #[test]
    fn no_fixed_refresh_and_a_stale_vblank_keep_the_wall_clock() {
        assert_eq!(step(None, None, 42), (42, false, false));
        assert_eq!(step(Some(Lattice { at: 0, period: 0 }), None, 42), (42, false, false));
        let late = LATTICE.at + STALE_NS + 1;
        assert_eq!(step(Some(LATTICE), None, late), (late, false, false));
    }

    #[test]
    fn times_never_step_back() {
        let prev = Prev { now: LATTICE.at + PERIOD + 6_900_000, time: LATTICE.at + 2 * PERIOD, snapped: true };
        let (time, ..) = step(Some(LATTICE), Some(prev), LATTICE.at + 2 * PERIOD + 100_000);
        assert_eq!(time, LATTICE.at + 3 * PERIOD);
    }
}
