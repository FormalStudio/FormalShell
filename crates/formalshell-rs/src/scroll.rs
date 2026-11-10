//! A scroll area under the pointer: what one axis event asks of it, the
//! touchpad's rubber band past either end, and the coast a flick carries on
//! with after the fingers lift (libinput sends no kinetic events, so the
//! shell coasts itself).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use smithay_client_toolkit::reexports::client::protocol::wl_pointer::AxisSource;
use smithay_client_toolkit::seat::pointer::AxisScroll;

use crate::motion::{Animated, Kind};

/// Content pixels a touchpad's pixel of travel moves. A wheel's notches
/// keep their own step.
pub const FINGER_GAIN: f64 = 1.12;

/// UIKit's rubber band constant: the share of the pull the content follows
/// at the very start of a stretch.
const PULL: f64 = 0.55;

/// The stretch's asymptote, a share of the viewport capped in pixels: it
/// can never get past this however far the fingers travel.
const REACH: f64 = 0.1;
pub const REACH_MAX: f64 = 40.0;

/// The coast's decay time constant, in seconds. UIScrollView's normal
/// rate (0.998 a millisecond) is 500ms; 300 keeps the macOS feel and stops
/// sooner, a coast covering about 0.27s worth of its launch speed.
const TAU: f64 = 0.3;
/// The coast's whole run, in time constants: by e^-3.5 it is down to 3% of
/// its launch speed, and a linear term takes that rest to an exact stop at
/// the end of the run (1.05s) rather than leaving the content creeping a
/// pixel at a time.
const SPAN: f64 = 3.5;
/// The slowest lift that coasts, in content pixels a second (the globe's
/// own launch speed): about 30px of coast.
const MIN_SPEED: f64 = 120.0;
/// The motion a lift carries, in Wayland event milliseconds, as the globe's
/// drag reads it: the samples this far back, nothing when the fingers had
/// stopped this long before the lift, and never a span shorter than this.
const WINDOW_MS: u32 = 80;
const STOPPED_MS: u32 = 40;
const MIN_SPAN_MS: u32 = 16;
/// A coast running into an end carries on into the stretch for this long,
/// slowing to its peak, before the spring takes it back.
const BOUNCE: f64 = 0.08;

/// What one axis frame asks of a scroll area.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Travel {
    /// Wheel notches: value120 over 120, an older seat's discrete steps,
    /// or a sourceless smooth value at 15 a notch.
    Notches(f64),
    /// Content pixels off a touchpad at event `time`. `finger` is the one
    /// source that promises an axis_stop on lift, so the only one that may
    /// stretch or coast.
    Pixels { px: f64, finger: bool, time: u32 },
    /// The fingers left the pad at event time.
    Lift(u32),
    None,
}

/// One axis of a scroll in wheel notches: the steps a wheel reports, or a
/// continuous axis at 15 units to the notch, the libinput/Qt ratio a
/// touchpad's swipe is read at, whatever its source.
pub fn notches(axis: &AxisScroll) -> f64 {
    if axis.value120 != 0 {
        f64::from(axis.value120) / 120.0
    } else if axis.discrete != 0 {
        f64::from(axis.discrete)
    } else {
        axis.absolute / 15.0
    }
}

/// The smallest scroll that shows `[at, at + size)` in a `view` over
/// `content`.
pub fn follow(at: f64, size: f64, scroll: f64, view: f64, content: f64) -> f64 {
    let mut next = scroll;
    if at < next {
        next = at;
    } else if at + size > next + view {
        next = at + size - view;
    }
    next.clamp(0.0, (content - view).max(0.0))
}

/// What a wheel notch moves the panel body. A scroll region inside a panel
/// moves a theme control height a notch instead (QtQuick's WheelHandler
/// reading angleDelta over 120 against a row), which the body does not
/// share: its rows have no common height.
pub const BODY_NOTCH: f64 = 32.0;

/// A wheel carries a continuous value too, so only one of them is ever read.
pub fn travel(axis: &AxisScroll, source: Option<AxisSource>, time: u32) -> Travel {
    let finger = source == Some(AxisSource::Finger);
    if axis.value120 != 0 {
        Travel::Notches(f64::from(axis.value120) / 120.0)
    } else if axis.discrete != 0 {
        Travel::Notches(f64::from(axis.discrete))
    } else if axis.absolute != 0.0 && (finger || source == Some(AxisSource::Continuous)) {
        Travel::Pixels { px: axis.absolute * FINGER_GAIN, finger, time }
    } else if axis.absolute != 0.0 {
        Travel::Notches(axis.absolute / 15.0)
    } else if axis.stop && finger {
        Travel::Lift(time)
    } else {
        Travel::None
    }
}

fn reach(view: f64) -> f64 {
    (view * REACH).clamp(1.0, REACH_MAX)
}

/// `(1 - 1/(x*c/d + 1)) * d`: the drawn stretch for `pull` px of finger
/// travel past an end, each pixel giving less than the one before.
pub fn rubber(pull: f64, view: f64) -> f64 {
    let d = reach(view);
    pull.signum() * (1.0 - 1.0 / (pull.abs() * PULL / d + 1.0)) * d
}

fn unrubber(stretch: f64, view: f64) -> f64 {
    let d = reach(view);
    let s = stretch.abs().min(d * 0.999);
    stretch.signum() * (d / PULL) * (1.0 / (1.0 - s / d) - 1.0)
}

/// `k`, the speed left at the end of the exponential run, and the share
/// of `TAU * v` a whole coast covers (0.89).
fn decay() -> (f64, f64) {
    let k = (-SPAN).exp();
    (k, (1.0 - k - SPAN * k) / (1.0 - k))
}

/// `a(1 - e^(-t/TAU)) - b t`: the exponential decay with the linear term
/// that stops it dead at `SPAN * TAU`, in seconds of its own clock.
#[derive(Clone, Copy, Debug)]
struct Coast {
    start: Instant,
    /// Real seconds to one of the coast's own (the debug motion scale).
    slow: f64,
    from: f64,
    a: f64,
    b: f64,
    max: f64,
    /// The own-clock second it runs into an end, and that end.
    hit: Option<(f64, f64)>,
}

impl Coast {
    /// `v` content pixels a second off `from`, its run capped at one
    /// `view`. None when it is too slow to coast at all.
    fn launch(now: Instant, slow: f64, from: f64, max: f64, v: f64, view: f64) -> Option<Self> {
        let (k, share) = decay();
        let cap = view.max(1.0) / (TAU * share);
        let v = v.clamp(-cap, cap);
        if v.abs() < MIN_SPEED {
            return None;
        }
        let a = v * TAU / (1.0 - k);
        let mut c = Self { start: now, slow, from, a, b: a / TAU * k, max, hit: None };
        let end = if v > 0.0 { max } else { 0.0 };
        let need = (end - from).abs();
        if need <= c.travel(SPAN * TAU).abs() {
            // The travel only grows on the way, so the second it covers
            // `need` is a bisection away.
            let (mut lo, mut hi) = (0.0, SPAN * TAU);
            for _ in 0..50 {
                let mid = (lo + hi) / 2.0;
                if c.travel(mid).abs() < need { lo = mid } else { hi = mid }
            }
            c.hit = Some((hi, end));
        }
        Some(c)
    }

    fn travel(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, SPAN * TAU);
        self.a * (1.0 - (-t / TAU).exp()) - self.b * t
    }

    fn speed(&self, t: f64) -> f64 {
        self.a / TAU * (-t / TAU).exp() - self.b
    }

    /// The offset at `now`, and whether the coast is over.
    fn at(&self, now: Instant) -> (f64, bool) {
        let t = now.saturating_duration_since(self.start).as_secs_f64() / self.slow;
        match self.hit {
            Some((at, end)) if t >= at => (end, true),
            _ if t >= SPAN * TAU => (self.from + self.travel(SPAN * TAU), true),
            _ => (self.from + self.travel(t), false),
        }
    }
}

/// A coast's run into an end, carried on as a stretch.
#[derive(Clone, Copy, Debug)]
struct Bounce {
    at: Instant,
    slow: f64,
    /// The coast's speed at the end, which the drawn stretch leaves it at.
    v: f64,
}

/// The fingers' speed by event time, never the loop's: motion the
/// compositor coalesced lands in one loop turn. One axis through `push`, or
/// a position in the plane through `track` (the globe's drag).
#[derive(Default)]
pub struct Velocity {
    samples: VecDeque<(u32, f64, f64)>,
    travelled: f64,
}

impl Velocity {
    pub fn push(&mut self, time: u32, px: f64) {
        self.travelled += px;
        self.track(time, self.travelled, 0.0);
    }

    /// The pointer at `(x, y)` at event `time`.
    pub fn track(&mut self, time: u32, x: f64, y: f64) {
        while self.samples.front().is_some_and(|&(t, ..)| time.wrapping_sub(t) > WINDOW_MS * 2) {
            self.samples.pop_front();
        }
        self.samples.push_back((time, x, y));
    }

    /// The speed a lift at `time` carries, in pixels a second: the travel
    /// over the window before it, spread over the time up to the lift so
    /// fingers slowing into it carry less, and nothing once they had
    /// stopped.
    pub fn at_lift(&self, time: u32) -> f64 {
        self.at_lift_2d(time).0
    }

    /// [`Velocity::at_lift`] along both axes.
    pub fn at_lift_2d(&self, time: u32) -> (f64, f64) {
        let Some(&(last_t, lx, ly)) = self.samples.back() else { return (0.0, 0.0) };
        if time.wrapping_sub(last_t) > STOPPED_MS {
            return (0.0, 0.0);
        }
        let Some(&(first_t, fx, fy)) = self.samples.iter().find(|&&(t, ..)| time.wrapping_sub(t) <= WINDOW_MS) else {
            return (0.0, 0.0);
        };
        let span = f64::from(time.wrapping_sub(first_t).max(MIN_SPAN_MS)) / 1000.0;
        ((lx - fx) / span, (ly - fy) / span)
    }

    pub fn clear(&mut self) {
        self.samples.clear();
        self.travelled = 0.0;
    }
}

/// A touchpad gesture over a scroll area: the stretch it pulls past either
/// end while the fingers are down, springing back on the spatial clock
/// once they lift, and the coast a flick carries on with after the lift.
/// The offset itself never leaves `0..=max`: the stretch is drawn on top.
pub struct Touchpad {
    /// The fingers' travel past the end, negative past the top.
    pull: f64,
    view: f64,
    shown: Animated,
    bounce: Option<Bounce>,
    coast: Option<Coast>,
    velocity: Velocity,
}

impl Touchpad {
    pub fn new() -> Self {
        Self {
            pull: 0.0,
            view: 0.0,
            shown: Animated::new(0.0, Kind::SpatialFast.curve()),
            bounce: None,
            coast: None,
            velocity: Velocity::default(),
        }
    }

    /// `offset` moved by `px` of finger travel at event `time`. Whatever
    /// lands past an end pulls the stretch, and travel back toward the
    /// content unwinds the pull before it moves the offset. A coast stops
    /// where it was last drawn, which is `offset`. Returns the new offset.
    pub fn drag(&mut self, now: Instant, time: u32, offset: f64, max: f64, px: f64, view: f64) -> f64 {
        self.view = view;
        self.coast = None;
        if self.bounce.is_some() {
            let shown = self.value(now);
            self.bounce = None;
            self.shown.jump(shown);
        }
        self.velocity.push(time, px);
        // Fingers back on the pad mid spring take it from where it is.
        if self.pull == 0.0 {
            let shown = self.shown.value(now);
            if shown != 0.0 {
                self.pull = unrubber(shown, view);
            }
        }
        let mut px = px;
        if self.pull != 0.0 && px.signum() != self.pull.signum() {
            let left = self.pull + px;
            if left.signum() == self.pull.signum() && left != 0.0 {
                self.pull = left;
                self.shown.jump(rubber(self.pull, view));
                return offset;
            }
            self.pull = 0.0;
            px = left;
        }
        let next = offset + px;
        let (offset, over) = if max <= 0.0 {
            (0.0, 0.0)
        } else if next > max {
            (max, next - max)
        } else if next < 0.0 {
            (0.0, next)
        } else {
            (next, 0.0)
        };
        self.pull += over;
        self.shown.jump(rubber(self.pull, view));
        offset
    }

    /// The fingers lifted at event `time` over `offset`: a stretch springs
    /// back to the end over `ms`, and a flick with nothing stretched coasts
    /// on, `slow` times slower than real (the debug motion scale), into a
    /// bounce off an end it reaches. With motion off (`ms` 0) nothing
    /// coasts.
    pub fn release(&mut self, now: Instant, time: u32, offset: f64, max: f64, ms: f64, slow: f64) {
        let speed = self.velocity.at_lift(time);
        self.velocity.clear();
        let stretched = self.pull != 0.0 || self.value(now) != 0.0;
        self.pull = 0.0;
        if self.shown.target() != 0.0 {
            self.shown.set_on(now, 0.0, ms, Kind::SpatialFast.curve());
        }
        if stretched || ms <= 0.0 || max <= 0.0 {
            return;
        }
        let slow = slow.max(f64::MIN_POSITIVE);
        let Some(coast) = Coast::launch(now, slow, offset, max, speed, self.view) else { return };
        if let Some((t, _)) = coast.hit {
            let v = coast.speed(t);
            let peak = rubber(v / PULL * BOUNCE / std::f64::consts::E, self.view);
            if peak.abs() >= 0.5 {
                self.bounce = Some(Bounce { at: now + Duration::from_secs_f64(t * slow), slow, v });
                self.shown.jump(peak);
                self.shown.set_after(now, 0.0, ms, (t + BOUNCE) * slow * 1000.0);
            }
        }
        self.coast = Some(coast);
    }

    /// The coast's offset for this frame, None while there is none. A
    /// content whose `max` moved under it stops it where it was last drawn.
    pub fn tick(&mut self, now: Instant, max: f64) -> Option<f64> {
        let coast = self.coast?;
        if (coast.max - max).abs() > 0.5 {
            self.settle();
            return None;
        }
        let (offset, done) = coast.at(now);
        if done {
            self.coast = None;
        }
        Some(offset.clamp(0.0, max.max(0.0)))
    }

    /// A wheel, a key or a new view takes over: no coast and no stretch.
    /// One axis frame over a scroll area at `offset`, `max` from its end:
    /// the offset it lands on and whether it glides there, None for a frame
    /// that asks nothing. A wheel's notches step `step` each and stop on
    /// either end; a touchpad's travel lands at once and stretches past an
    /// end until the fingers lift, springing back over `release_ms` or
    /// coasting on.
    #[allow(clippy::too_many_arguments)]
    pub fn travel(&mut self, travel: Travel, offset: f64, max: f64, step: f64, view: f64, release_ms: f64, slow: f64) -> Option<(f64, bool)> {
        let now = Instant::now();
        Some(match travel {
            Travel::Notches(n) => {
                self.settle();
                ((offset + n * step).clamp(0.0, max), true)
            }
            Travel::Pixels { px, finger: true, time } => (self.drag(now, time, offset, max, px, view), false),
            Travel::Pixels { px, finger: false, .. } => {
                self.settle();
                ((offset + px).clamp(0.0, max), false)
            }
            Travel::Lift(time) => {
                self.release(now, time, offset, max, release_ms, slow);
                (offset, false)
            }
            Travel::None => return None,
        })
    }

    pub fn settle(&mut self) {
        self.pull = 0.0;
        self.coast = None;
        self.bounce = None;
        self.velocity.clear();
        self.shown.jump(0.0);
    }

    pub fn coasting(&self) -> bool {
        self.coast.is_some()
    }

    pub fn value(&self, now: Instant) -> f64 {
        if let Some(b) = self.bounce {
            if now < b.at {
                return 0.0;
            }
            let s = now.saturating_duration_since(b.at).as_secs_f64() / b.slow;
            if s < BOUNCE {
                // Critically damped out of the end: it leaves at the
                // coast's speed and comes to rest on the peak.
                return rubber(b.v / PULL * s * (-s / BOUNCE).exp(), self.view);
            }
        }
        self.shown.value(now)
    }

    pub fn running(&self, now: Instant) -> bool {
        self.coast.is_some() || self.shown.running(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// `n` frames of `px` each, `every` ms apart from event time 1000, then
    /// the lift `pause` ms after the last.
    fn flick(t: &mut Touchpad, now: Instant, offset: f64, max: f64, n: u32, px: f64, every: u32, pause: u32) -> f64 {
        let mut o = offset;
        for i in 0..n {
            o = t.drag(now, 1000 + i * every, o, max, px, 600.0);
        }
        t.release(now, 1000 + (n - 1) * every + pause, o, max, 350.0, 1.0);
        o
    }

    #[test]
    fn the_stretch_resists_harder_the_further_it_goes_and_never_reaches_its_cap() {
        let view = 600.0;
        let (a, b, c) = (rubber(40.0, view), rubber(80.0, view), rubber(10_000.0, view));
        assert!(a > 0.0 && b > a && c > b);
        assert!(b - a < a, "the second 40px gave {} against the first's {a}", b - a);
        assert!(c < REACH_MAX);
        assert!(rubber(-40.0, view) == -a);
        assert!((unrubber(a, view) - 40.0).abs() < 1e-6);
    }

    #[test]
    fn travel_inside_the_area_moves_the_offset_and_past_it_only_stretches() {
        let now = Instant::now();
        let mut s = Touchpad::new();
        assert_eq!(s.drag(now, 0, 100.0, 200.0, 50.0, 400.0), 150.0);
        assert_eq!(s.value(now), 0.0);
        // The end is reached at once, the rest pulls.
        assert_eq!(s.drag(now, 8, 150.0, 200.0, 80.0, 400.0), 200.0);
        let pulled = s.value(now);
        assert!(pulled > 0.0 && pulled < 30.0, "{pulled}");
        // Back toward the content unwinds the pull before the offset moves.
        assert_eq!(s.drag(now, 16, 200.0, 200.0, -10.0, 400.0), 200.0);
        assert!(s.value(now) < pulled && s.value(now) > 0.0);
        assert_eq!(s.drag(now, 24, 200.0, 200.0, -40.0, 400.0), 180.0);
        assert_eq!(s.value(now), 0.0);
        // Past the top too.
        assert_eq!(s.drag(now, 32, 180.0, 200.0, -400.0, 400.0), 0.0);
        assert!(s.value(now) < 0.0);
    }

    #[test]
    fn a_lift_springs_back_onto_the_end_and_does_not_coast() {
        let now = Instant::now();
        let mut s = Touchpad::new();
        flick(&mut s, now, 150.0, 200.0, 6, 40.0, 8, 0);
        assert!(s.running(now));
        assert!(!s.coasting(), "a lift out of a stretch coasted");
        let later = now + ms(351);
        assert!(!s.running(later));
        assert_eq!(s.value(later), 0.0);
    }

    #[test]
    fn an_area_with_nothing_to_scroll_does_not_stretch() {
        let now = Instant::now();
        let mut s = Touchpad::new();
        assert_eq!(s.drag(now, 0, 0.0, 0.0, 120.0, 400.0), 0.0);
        assert_eq!(s.value(now), 0.0);
    }

    #[test]
    fn a_flick_coasts_slowing_all_the_way_and_stops_dead_inside_a_viewport() {
        let now = Instant::now();
        let mut s = Touchpad::new();
        // 2240 px/s, inside the cap.
        let lifted = flick(&mut s, now, 0.0, 5000.0, 6, 17.92, 8, 0);
        assert!(s.coasting());
        let (mut last, mut step) = (lifted, f64::MAX);
        for f in 1..=70 {
            let o = s.tick(now + ms(f * 16), 5000.0).unwrap_or(last);
            let d = o - last;
            assert!(d >= 0.0 && d <= step + 1e-9, "frame {f} moved {d} after {step}");
            (last, step) = (o, d);
        }
        assert!(!s.coasting(), "still coasting 1.1s in");
        assert!(!s.running(now + ms(1100)));
        let run = last - lifted;
        assert!(run > 400.0 && run <= 600.0, "coasted {run}");
        assert_eq!(s.tick(now + ms(2000), 5000.0), None);
    }

    #[test]
    fn a_flick_never_coasts_past_one_viewport() {
        let now = Instant::now();
        let mut s = Touchpad::new();
        let lifted = flick(&mut s, now, 0.0, 5000.0, 6, 200.0, 8, 0);
        let end = s.tick(now + ms(5000), 5000.0).unwrap();
        assert!((end - lifted - 600.0).abs() < 1e-6, "coasted {}", end - lifted);
    }

    #[test]
    fn slow_fingers_or_a_pause_before_the_lift_do_not_coast() {
        let now = Instant::now();
        let mut s = Touchpad::new();
        // 1px every 16ms is 70 px/s.
        flick(&mut s, now, 100.0, 5000.0, 10, 1.12, 16, 0);
        assert!(!s.coasting());
        flick(&mut s, now, 100.0, 5000.0, 6, 30.0, 8, 60);
        assert!(!s.coasting(), "fingers that stopped 60ms before the lift coasted");
    }

    #[test]
    fn a_coast_into_an_end_bounces_and_settles_exactly_on_it() {
        let now = Instant::now();
        let mut s = Touchpad::new();
        let max = 1000.0;
        flick(&mut s, now, 800.0, max, 6, 17.92, 8, 0);
        let mut over = 0.0_f64;
        let mut last = 800.0;
        for f in 1..=80 {
            let t = now + ms(f * 16);
            if let Some(o) = s.tick(t, max) {
                assert!(o >= last && o <= max, "frame {f} at {o}");
                last = o;
            }
            // The spring back rides spatialFast, which passes its rest.
            over = over.max(s.value(t));
            assert!(s.value(t).abs() < REACH_MAX);
        }
        assert_eq!(last, max);
        assert!(over > 2.0, "the bounce peaked at {over}");
        let settled = now + ms(1500);
        assert!(!s.running(settled));
        assert_eq!(s.value(settled), 0.0);
    }

    #[test]
    fn fingers_back_on_the_pad_stop_the_coast_where_it_is_drawn() {
        let now = Instant::now();
        let mut s = Touchpad::new();
        flick(&mut s, now, 0.0, 5000.0, 6, 17.92, 8, 0);
        let drawn = s.tick(now + ms(100), 5000.0).unwrap();
        assert_eq!(s.drag(now + ms(100), 1200, drawn, 5000.0, 1.0, 600.0), drawn + 1.0);
        assert!(!s.coasting());
        assert_eq!(s.tick(now + ms(400), 5000.0), None);
    }

    #[test]
    fn a_content_change_stops_the_coast() {
        let now = Instant::now();
        let mut s = Touchpad::new();
        flick(&mut s, now, 0.0, 5000.0, 6, 17.92, 8, 0);
        assert!(s.tick(now + ms(50), 5000.0).is_some());
        assert_eq!(s.tick(now + ms(66), 3000.0), None);
        assert!(!s.running(now + ms(66)));
    }

    #[test]
    fn the_lift_speed_reads_the_window_before_it_and_nothing_once_stopped() {
        let mut v = Velocity::default();
        for i in 0..6 {
            v.push(1000 + i * 10, 5.0);
        }
        // 25px over the 50ms from the first sample to the lift.
        assert!((v.at_lift(1050) - 500.0).abs() < 1e-9, "{}", v.at_lift(1050));
        assert_eq!(v.at_lift(1050 + STOPPED_MS + 1), 0.0);
        // Only the window before the lift counts.
        let mut v = Velocity::default();
        v.push(0, 1000.0);
        for i in 0..5 {
            v.push(200 + i * 10, -2.0);
        }
        assert!(v.at_lift(240) < 0.0);
        v.clear();
        assert_eq!(v.at_lift(240), 0.0);
    }

    #[test]
    fn a_wheel_never_reads_as_a_finger() {
        let axis = |absolute: f64, value120: i32, stop: bool| AxisScroll { absolute, discrete: 0, value120, relative_direction: None, stop };
        assert_eq!(travel(&axis(15.0, 120, false), Some(AxisSource::Wheel), 5), Travel::Notches(1.0));
        assert_eq!(travel(&axis(15.0, 0, false), None, 5), Travel::Notches(1.0));
        assert_eq!(
            travel(&axis(10.0, 0, false), Some(AxisSource::Finger), 5),
            Travel::Pixels { px: 10.0 * FINGER_GAIN, finger: true, time: 5 }
        );
        assert_eq!(travel(&axis(0.0, 0, true), Some(AxisSource::Finger), 5), Travel::Lift(5));
        assert_eq!(travel(&axis(0.0, 0, true), Some(AxisSource::Wheel), 5), Travel::None);
    }
}
