//! Picture in picture: which windows are one, the corners one rests in, where
//! a release carries it, the edge it stashes behind, and what a pinch does to
//! its size. A picture belongs to an output and not to a workspace: it is
//! drawn over every window of the output, fullscreen ones included.

use crate::{
    geom::Rect,
    motion::{SpringParams, project},
};

/// The spring a picture rides to its rest: a response of 0.45 s, damped just
/// short of critical so a flick lands with a trace of overshoot.
pub const SPRING: SpringParams = SpringParams {
    omega: 14.0,
    zeta: 0.82,
};

/// How far a finger travels before a touch is a drag and not a tap.
pub const SLOP: f64 = 10.0;
/// What a stashed picture leaves on screen.
pub const TAB: f64 = 28.0;
/// The speed toward an outer edge, in pixels a second, at which a release
/// stashes the picture there.
pub const STASH_SPEED: f64 = 900.0;

const MIN_WIDTH: f64 = 200.0;
const MIN_HEIGHT: f64 = 112.0;
/// The largest share of the work area a picture takes on either axis.
const MAX_SHARE: f64 = 0.6;
/// How much of a pull past a limit shows at first, and the share of the
/// limit the pull can never pass.
const RUBBER: f64 = 0.55;
const RUBBER_REACH: f64 = 0.35;
/// A velocity is measured over the last samples this far back, and is zero
/// when the hand rested this long before letting go.
const TRACK_MS: f64 = 90.0;
const REST_MS: f64 = 70.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// Where a picture rests on its output.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rest {
    Corner(Corner),
    /// Behind a side edge, at this share of the way down the work area.
    Stash { side: Side, down: f64 },
}

/// What a window says about itself when it opens.
#[derive(Clone, Copy, Debug, Default)]
pub struct Hints<'a> {
    pub title: &'a str,
    pub xwayland: bool,
    /// X11 `_NET_WM_STATE_ABOVE`.
    pub above: bool,
    /// X11 `_NET_WM_STATE_STICKY`.
    pub sticky: bool,
    /// X11 size hints with one aspect ratio for the minimum and the maximum.
    pub fixed_aspect: bool,
}

/// Chromium, Electron and Firefox title their player "Picture in picture",
/// "Picture in Picture" or "Picture-in-Picture" and say nothing else on
/// Wayland. An X11 window says it with its state: above every window and
/// either on every desktop (Chromium, `mpv --ontop --on-all-workspaces`) or
/// held to one aspect ratio (Firefox, `mpv --ontop`).
pub fn recognise(hints: &Hints<'_>) -> bool {
    titled(hints.title) || (hints.xwayland && hints.above && (hints.sticky || hints.fixed_aspect))
}

fn titled(title: &str) -> bool {
    let mut words = title.split([' ', '-']);
    let is = |word: Option<&str>, want: &str| word.is_some_and(|w| w.eq_ignore_ascii_case(want));
    is(words.next(), "picture")
        && is(words.next(), "in")
        && is(words.next(), "picture")
        && words.next().is_none()
}

/// The sizes a picture of one aspect ratio can take, by its width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    pub min: f64,
    pub max: f64,
    /// Width over height.
    pub aspect: f64,
}

impl Limits {
    pub fn clamp(&self, width: f64) -> f64 {
        width.clamp(self.min, self.max)
    }

    /// `width` as drawn: past a limit it gives less and less, and never
    /// more than `RUBBER_REACH` of the limit.
    pub fn rubber(&self, width: f64) -> f64 {
        let band = |over: f64, limit: f64| {
            let reach = limit * RUBBER_REACH;
            (1.0 - 1.0 / (over * RUBBER / reach + 1.0)) * reach
        };
        if width > self.max {
            self.max + band(width - self.max, self.max)
        } else if width < self.min {
            self.min - band(self.min - width, self.min)
        } else {
            width
        }
    }

    pub fn size(&self, width: f64) -> (f64, f64) {
        (width, width / self.aspect)
    }
}

/// An output as a picture sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    /// Global logical.
    pub rect: Rect,
    /// What the layers leave free, less the gap a picture keeps to it.
    pub area: Rect,
    /// No other output continues past this side, so a picture can hide there.
    pub stash_left: bool,
    pub stash_right: bool,
}

/// The screens of these outputs, each given as its rect and what its layers
/// leave free. `margin` is the gap a resting picture keeps.
pub fn screens(outputs: &[(Rect, Rect)], margin: f64) -> Vec<Screen> {
    let beside = |rect: &Rect, x: f64| {
        outputs.iter().any(|(other, _)| {
            other != rect
                && other.x <= x
                && x < other.right()
                && other.y < rect.bottom()
                && rect.y < other.bottom()
        })
    };
    outputs
        .iter()
        .map(|(rect, usable)| Screen {
            rect: *rect,
            area: usable.inset(margin),
            stash_left: !beside(rect, rect.x - 1.0),
            stash_right: !beside(rect, rect.right()),
        })
        .collect()
}

impl Screen {
    pub fn limits(&self, aspect: f64) -> Limits {
        let aspect = if aspect.is_finite() && aspect > 0.0 {
            aspect
        } else {
            16.0 / 9.0
        };
        let max = (self.area.w * MAX_SHARE).min(self.area.h * MAX_SHARE * aspect);
        let min = MIN_WIDTH.max(MIN_HEIGHT * aspect).min(max);
        Limits { min, max, aspect }
    }

    pub fn corner(&self, corner: Corner, size: (f64, f64)) -> Rect {
        let (w, h) = size;
        let (left, top) = (self.area.x, self.area.y);
        let (right, bottom) = (self.area.right() - w, self.area.bottom() - h);
        let (x, y) = match corner {
            Corner::TopLeft => (left, top),
            Corner::TopRight => (right, top),
            Corner::BottomLeft => (left, bottom),
            Corner::BottomRight => (right, bottom),
        };
        Rect::new(x.round(), y.round(), w, h)
    }

    fn travel(&self, height: f64) -> f64 {
        (self.area.h - height).max(0.0)
    }

    pub fn stash(&self, side: Side, down: f64, size: (f64, f64)) -> Rect {
        let (w, h) = size;
        let x = match side {
            Side::Left => self.rect.x + TAB - w,
            Side::Right => self.rect.right() - TAB,
        };
        let y = self.area.y + down.clamp(0.0, 1.0) * self.travel(h);
        Rect::new(x.round(), y.round(), w, h)
    }

    pub fn rest(&self, rest: Rest, size: (f64, f64)) -> Rect {
        match rest {
            Rest::Corner(corner) => self.corner(corner, size),
            Rest::Stash { side, down } => self.stash(side, down, size),
        }
    }

    /// The strip of a stashed picture that is still on screen.
    pub fn tab(&self, side: Side, down: f64, size: (f64, f64)) -> Rect {
        let rect = self.stash(side, down, size);
        let x = match side {
            Side::Left => self.rect.x,
            Side::Right => self.rect.right() - TAB,
        };
        Rect::new(x, rect.y, TAB, rect.h)
    }

    fn nearest_corner(&self, at: (f64, f64)) -> Corner {
        let (cx, cy) = self.area.center();
        match (at.0 < cx, at.1 < cy) {
            (true, true) => Corner::TopLeft,
            (false, true) => Corner::TopRight,
            (true, false) => Corner::BottomLeft,
            (false, false) => Corner::BottomRight,
        }
    }

    fn distance(&self, at: (f64, f64)) -> f64 {
        let dx = (self.rect.x - at.0).max(at.0 - self.rect.right()).max(0.0);
        let dy = (self.rect.y - at.1).max(at.1 - self.rect.bottom()).max(0.0);
        dx.hypot(dy)
    }
}

/// The screen `at` is on, or the one nearest to it.
pub fn screen_at(screens: &[Screen], at: (f64, f64)) -> Option<usize> {
    screens
        .iter()
        .position(|screen| screen.rect.contains(at.0, at.1))
        .or_else(|| {
            (0..screens.len()).min_by(|a, b| {
                screens[*a]
                    .distance(at)
                    .total_cmp(&screens[*b].distance(at))
            })
        })
}

/// Where a picture let go at `rect` with `velocity` (pixels a second) comes
/// to rest: the screen and the place on it. It goes where its speed would
/// carry it, to the nearest corner there. Past an edge no output continues
/// behind, at speed or dragged halfway off, it stashes.
pub fn land(screens: &[Screen], rect: Rect, velocity: (f64, f64)) -> Option<(usize, Rest)> {
    let centre = rect.center();
    let carried = (
        centre.0 + project(velocity.0),
        centre.1 + project(velocity.1),
    );
    let home = screen_at(screens, centre)?;
    let landing = screens
        .iter()
        .position(|screen| screen.rect.contains(carried.0, carried.1))
        .unwrap_or(home);
    let screen = &screens[landing];
    if landing == home {
        let off = |edge_passed: bool, toward: f64, beyond: bool| {
            edge_passed || (toward >= STASH_SPEED && beyond)
        };
        let down = {
            let travel = screen.travel(rect.h);
            let y = rect.y + project(velocity.1);
            if travel > 0.0 {
                ((y - screen.area.y) / travel).clamp(0.0, 1.0)
            } else {
                0.0
            }
        };
        if screen.stash_left
            && off(
                centre.0 < screen.rect.x,
                -velocity.0,
                carried.0 < screen.rect.x,
            )
        {
            return Some((
                landing,
                Rest::Stash {
                    side: Side::Left,
                    down,
                },
            ));
        }
        if screen.stash_right
            && off(
                centre.0 >= screen.rect.right(),
                velocity.0,
                carried.0 >= screen.rect.right(),
            )
        {
            return Some((
                landing,
                Rest::Stash {
                    side: Side::Right,
                    down,
                },
            ));
        }
    }
    Some((landing, Rest::Corner(screen.nearest_corner(carried))))
}

/// The corner a stashed picture comes back to.
pub fn unstash(side: Side, down: f64) -> Corner {
    match (side, down < 0.5) {
        (Side::Left, true) => Corner::TopLeft,
        (Side::Left, false) => Corner::BottomLeft,
        (Side::Right, true) => Corner::TopRight,
        (Side::Right, false) => Corner::BottomRight,
    }
}

/// A picture under a pinch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pinched {
    /// The size its client is asked for, inside the limits.
    pub rect: Rect,
    /// What is drawn: `rect` stretched or squeezed past a limit.
    pub drawn: Rect,
}

/// `start` scaled by `scale` about `focus`, the point the fingers hold, which
/// has since moved by `pan`. The point of the picture under the fingers
/// stays under them, and the aspect ratio is kept.
pub fn pinch(start: Rect, scale: f64, focus: (f64, f64), pan: (f64, f64), limits: Limits) -> Pinched {
    let wanted = start.w * scale.max(0.0);
    let at = |width: f64| {
        let by = width / start.w.max(1.0);
        Rect::new(
            focus.0 + pan.0 - (focus.0 - start.x) * by,
            focus.1 + pan.1 - (focus.1 - start.y) * by,
            width,
            width / limits.aspect,
        )
    };
    Pinched {
        rect: at(limits.clamp(wanted)),
        drawn: at(limits.rubber(wanted)),
    }
}

/// The size a new picture takes: its own where that fits the limits.
pub fn fit(natural: (f64, f64), limits: Limits) -> (f64, f64) {
    limits.size(limits.clamp(natural.0).round())
}

/// The speed of a hand from where it has been.
#[derive(Clone, Debug, Default)]
pub struct Tracker {
    samples: Vec<(f64, f64, f64)>,
}

impl Tracker {
    pub fn push(&mut self, at: (f64, f64), now_ms: f64) {
        self.samples.push((now_ms, at.0, at.1));
        let from = self
            .samples
            .iter()
            .position(|(t, ..)| now_ms - t <= TRACK_MS)
            .unwrap_or(0);
        self.samples.drain(..from);
    }

    /// Pixels a second at `now_ms`; zero for a hand that came to rest.
    pub fn velocity(&self, now_ms: f64) -> (f64, f64) {
        let (Some(first), Some(last)) = (self.samples.first(), self.samples.last()) else {
            return (0.0, 0.0);
        };
        let span = last.0 - first.0;
        if span <= 0.0 || now_ms - last.0 > REST_MS {
            return (0.0, 0.0);
        }
        (
            (last.1 - first.1) / span * 1000.0,
            (last.2 - first.2) / span * 1000.0,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::motion::Spring;

    const FULL_HD: Rect = Rect::new(0.0, 0.0, 1920.0, 1080.0);

    /// One output with a 40 px bar on top and a 16 px gap.
    fn one() -> Vec<Screen> {
        screens(&[(FULL_HD, Rect::new(0.0, 40.0, 1920.0, 1040.0))], 16.0)
    }

    /// A second output right of the first.
    fn two() -> Vec<Screen> {
        screens(
            &[
                (FULL_HD, Rect::new(0.0, 40.0, 1920.0, 1040.0)),
                (
                    Rect::new(1920.0, 0.0, 2560.0, 1440.0),
                    Rect::new(1920.0, 0.0, 2560.0, 1440.0),
                ),
            ],
            16.0,
        )
    }

    const SIZE: (f64, f64) = (480.0, 270.0);

    #[test]
    fn the_players_of_the_browsers_are_recognised_by_their_title() {
        let titled = |title| {
            recognise(&Hints {
                title,
                ..Hints::default()
            })
        };
        assert!(titled("Picture in picture"), "Chromium, Helium, Electron");
        assert!(titled("Picture in Picture"), "Chromium on macOS strings");
        assert!(titled("Picture-in-Picture"), "Firefox");
        assert!(!titled("Picture in picture - Helium"));
        assert!(!titled("Pictures in picture"));
        assert!(!titled("Picture in picture in picture"));
        assert!(!titled(""));
    }

    #[test]
    fn an_x11_window_is_one_by_its_state_and_never_by_above_alone() {
        let x11 = |above, sticky, fixed_aspect| {
            recognise(&Hints {
                title: "video.mkv - mpv",
                xwayland: true,
                above,
                sticky,
                fixed_aspect,
            })
        };
        assert!(x11(true, true, false), "Chromium on X11");
        assert!(x11(true, false, true), "Firefox on X11, mpv --ontop");
        assert!(!x11(true, false, false), "a pinned dialog");
        assert!(!x11(false, true, true));
        assert!(
            !recognise(&Hints {
                title: "x",
                above: true,
                sticky: true,
                ..Hints::default()
            }),
            "the state is X11's; a Wayland window has none"
        );
    }

    #[test]
    fn corners_keep_the_gap_to_the_work_area_and_clear_the_bar() {
        let screen = one()[0];
        assert_eq!(
            screen.corner(Corner::TopLeft, SIZE),
            Rect::new(16.0, 56.0, 480.0, 270.0)
        );
        assert_eq!(
            screen.corner(Corner::TopRight, SIZE),
            Rect::new(1424.0, 56.0, 480.0, 270.0)
        );
        assert_eq!(
            screen.corner(Corner::BottomLeft, SIZE),
            Rect::new(16.0, 794.0, 480.0, 270.0)
        );
        assert_eq!(
            screen.corner(Corner::BottomRight, SIZE),
            Rect::new(1424.0, 794.0, 480.0, 270.0)
        );
    }

    #[test]
    fn a_release_at_rest_goes_to_the_nearest_corner() {
        let screens = one();
        let at = |x, y| land(&screens, Rect::new(x, y, 480.0, 270.0), (0.0, 0.0));
        assert_eq!(at(100.0, 100.0), Some((0, Rest::Corner(Corner::TopLeft))));
        assert_eq!(at(900.0, 100.0), Some((0, Rest::Corner(Corner::TopRight))));
        assert_eq!(
            at(600.0, 500.0),
            Some((0, Rest::Corner(Corner::BottomLeft)))
        );
        assert_eq!(
            at(900.0, 500.0),
            Some((0, Rest::Corner(Corner::BottomRight)))
        );
    }

    #[test]
    fn a_flick_carries_to_the_corner_it_points_at() {
        let screens = one();
        let rest = screens[0].corner(Corner::BottomRight, SIZE);
        // 0.332 px of travel for each px/s: 3000 px/s carries 996 px.
        assert_eq!(
            land(&screens, rest, (-3000.0, 0.0)),
            Some((0, Rest::Corner(Corner::BottomLeft)))
        );
        assert_eq!(
            land(&screens, rest, (0.0, -2000.0)),
            Some((0, Rest::Corner(Corner::TopRight)))
        );
        assert_eq!(
            land(&screens, rest, (-3000.0, -2000.0)),
            Some((0, Rest::Corner(Corner::TopLeft)))
        );
        assert_eq!(
            land(&screens, rest, (-600.0, -300.0)),
            Some((0, Rest::Corner(Corner::BottomRight))),
            "a nudge falls back"
        );
    }

    #[test]
    fn a_hard_flick_at_an_outer_edge_stashes_and_a_slow_one_does_not() {
        let screens = one();
        let rest = screens[0].corner(Corner::BottomRight, SIZE);
        let Some((0, Rest::Stash { side, down })) = land(&screens, rest, (1200.0, 0.0)) else {
            panic!("not stashed");
        };
        assert_eq!(side, Side::Right);
        assert_eq!(down, 1.0);
        let stashed = screens[0].stash(side, down, SIZE);
        assert_eq!(stashed.x, 1920.0 - TAB);
        assert_eq!(stashed.y, rest.y);
        assert_eq!(
            screens[0].tab(side, down, SIZE),
            Rect::new(1920.0 - TAB, rest.y, TAB, 270.0)
        );

        assert_eq!(
            land(&screens, rest, (800.0, 0.0)),
            Some((0, Rest::Corner(Corner::BottomRight))),
            "under the stash speed"
        );
        // Fast, but not far enough to pass the edge from the other side.
        let left = screens[0].corner(Corner::TopLeft, SIZE);
        assert_eq!(
            land(&screens, left, (1500.0, 0.0)),
            Some((0, Rest::Corner(Corner::TopLeft)))
        );
        assert_eq!(
            land(&screens, left, (-1000.0, 0.0)),
            Some((
                0,
                Rest::Stash {
                    side: Side::Left,
                    down: 0.0
                }
            ))
        );
        assert_eq!(screens[0].stash(Side::Left, 0.0, SIZE).right(), TAB);
    }

    #[test]
    fn a_picture_dragged_halfway_off_an_outer_edge_stashes_at_rest() {
        let screens = one();
        let dragged = Rect::new(1700.0, 400.0, 480.0, 270.0);
        assert!(matches!(
            land(&screens, dragged, (0.0, 0.0)),
            Some((
                0,
                Rest::Stash {
                    side: Side::Right,
                    ..
                }
            ))
        ));
        assert_eq!(unstash(Side::Right, 0.3), Corner::TopRight);
        assert_eq!(unstash(Side::Left, 0.7), Corner::BottomLeft);
    }

    #[test]
    fn an_edge_another_output_continues_past_is_crossed_not_stashed_behind() {
        let screens = two();
        assert!(screens[0].stash_left && !screens[0].stash_right);
        assert!(!screens[1].stash_left && screens[1].stash_right);
        let rest = screens[0].corner(Corner::BottomRight, SIZE);
        // Carried 498 px right: over the shared edge, onto the second output.
        assert_eq!(
            land(&screens, rest, (1500.0, 0.0)),
            Some((1, Rest::Corner(Corner::BottomLeft)))
        );
        // Dragged over and let go at rest.
        assert_eq!(
            land(&screens, Rect::new(2500.0, 100.0, 480.0, 270.0), (0.0, 0.0)),
            Some((1, Rest::Corner(Corner::TopLeft)))
        );
        // The second output's own corners, on its own work area.
        assert_eq!(
            screens[1].corner(Corner::BottomRight, SIZE),
            Rect::new(4480.0 - 16.0 - 480.0, 1440.0 - 16.0 - 270.0, 480.0, 270.0)
        );
    }

    #[test]
    fn a_release_off_every_output_lands_on_the_nearest() {
        let screens = two();
        assert_eq!(
            land(&screens, Rect::new(500.0, 1300.0, 480.0, 270.0), (0.0, 0.0)),
            Some((0, Rest::Corner(Corner::BottomLeft)))
        );
        assert_eq!(land(&[], Rect::default(), (0.0, 0.0)), None);
    }

    #[test]
    fn the_limits_follow_the_work_area_and_the_aspect() {
        let screen = one()[0];
        let wide = screen.limits(16.0 / 9.0);
        assert_eq!(wide.min, 200.0);
        // 60% of the 1008 px of height is the tighter bound: 604.8 * 16/9.
        assert!((wide.max - 1075.2).abs() < 1e-9);
        let tall = screen.limits(9.0 / 16.0);
        assert_eq!(tall.min, 200.0);
        assert!((tall.max - 604.8 * 9.0 / 16.0).abs() < 1e-9);
        assert_eq!(fit((2000.0, 1125.0), wide), (1075.0, 1075.0 * 9.0 / 16.0));
        assert_eq!(fit((100.0, 56.0), wide), (200.0, 112.5));
        assert_eq!(fit((480.0, 270.0), wide), (480.0, 270.0));
        assert_eq!(screen.limits(f64::NAN).aspect, 16.0 / 9.0);
    }

    #[test]
    fn a_pinch_scales_about_the_fingers_and_keeps_the_aspect() {
        let limits = one()[0].limits(16.0 / 9.0);
        let start = Rect::new(1000.0, 500.0, 480.0, 270.0);
        // The fingers a quarter of the way in from the left, a third down.
        let focus = (1120.0, 590.0);
        let grown = pinch(start, 1.5, focus, (0.0, 0.0), limits);
        assert_eq!(grown.rect, grown.drawn);
        assert_eq!((grown.rect.w, grown.rect.h), (720.0, 405.0));
        // The point under the fingers has not moved.
        assert!((grown.rect.x + 0.25 * grown.rect.w - focus.0).abs() < 1e-9);
        assert!((grown.rect.y + grown.rect.h / 3.0 - focus.1).abs() < 1e-9);

        let moved = pinch(start, 1.0, focus, (30.0, -20.0), limits);
        assert_eq!(moved.rect, Rect::new(1030.0, 480.0, 480.0, 270.0));

        let shrunk = pinch(start, 0.5, focus, (0.0, 0.0), limits);
        assert_eq!((shrunk.rect.w, shrunk.rect.h), (240.0, 135.0));
    }

    #[test]
    fn past_a_limit_the_client_stops_and_the_drawing_rubber_bands() {
        let limits = one()[0].limits(16.0 / 9.0);
        let start = Rect::new(1000.0, 500.0, 480.0, 270.0);
        let centre = start.center();

        let small = pinch(start, 0.25, centre, (0.0, 0.0), limits);
        assert_eq!(small.rect.w, limits.min, "the client is held at the minimum");
        assert!(small.drawn.w < limits.min && small.drawn.w > 120.0 * 1.0);
        assert!((small.drawn.w / small.drawn.h - limits.aspect).abs() < 1e-9);
        assert_eq!(small.drawn.center(), centre, "still about the fingers");

        // It gives less the further it is pulled, and never passes the reach.
        let pulls: Vec<f64> = [1.0, 0.4, 0.3, 0.2, 0.1, 0.0]
            .iter()
            .map(|scale| pinch(start, *scale, centre, (0.0, 0.0), limits).drawn.w)
            .collect();
        assert!(pulls.windows(2).all(|pair| pair[1] < pair[0]));
        let steps: Vec<f64> = pulls[1..].windows(2).map(|pair| pair[0] - pair[1]).collect();
        assert!(steps.windows(2).all(|pair| pair[1] < pair[0]), "{steps:?}");
        assert!(*pulls.last().unwrap() > limits.min * (1.0 - RUBBER_REACH));

        let big = pinch(start, 10.0, centre, (0.0, 0.0), limits);
        assert_eq!(big.rect.w, limits.max);
        assert!(big.drawn.w > limits.max);
        assert!(big.drawn.w < limits.max * (1.0 + RUBBER_REACH));
        // Just past the limit a pull shows at a little over half.
        let nudge = limits.rubber(limits.max + 10.0) - limits.max;
        assert!((nudge - 10.0 * RUBBER).abs() < 0.2, "{nudge}");
        assert_eq!(limits.rubber(limits.max), limits.max);
        assert_eq!(limits.rubber(300.0), 300.0);
    }

    /// Steps a spring at 120 Hz from `from` to 0 with `velocity`, and says
    /// when it last was more than half a pixel out and how far past it went.
    fn ride(from: f64, velocity: f64) -> (f64, f64) {
        let mut spring = Spring::at(from);
        spring.target = 0.0;
        spring.velocity = velocity;
        let (mut settled, mut past) = (0.0, 0.0_f64);
        for frame in 1..=480 {
            spring.step(SPRING, 1.0 / 120.0);
            if spring.value.abs() > 0.5 {
                settled = frame as f64 / 120.0;
            }
            past = past.max(-spring.value * from.signum());
        }
        assert!(spring.resting(), "at rest inside four seconds");
        (settled, past)
    }

    #[test]
    fn the_spring_settles_inside_two_thirds_of_a_second_with_a_trace_of_overshoot() {
        // Corner to corner on a 1080p output.
        let (settled, past) = ride(1408.0, 0.0);
        println!("pip spring, 1408 px from rest: settles {settled:.3} s, overshoot {past:.2} px");
        assert!(settled > 0.45 && settled < 0.72, "{settled}");
        assert!(past > 1.0 && past / 1408.0 < 0.02, "{past}");

        // Let go at 3000 px/s toward the target: it arrives sooner and
        // overshoots further, still a small share of the way.
        let (flung, flung_past) = ride(1408.0, -3000.0);
        println!("pip spring, 1408 px at 3000 px/s: settles {flung:.3} s, overshoot {flung_past:.2} px");
        assert!(flung <= settled);
        assert!(flung_past > past && flung_past / 1408.0 < 0.06, "{flung_past}");

        // A rubber band 60 px past a limit coming back.
        let (band, band_past) = ride(60.0, 0.0);
        println!("pip spring, 60 px rubber band: settles {band:.3} s, overshoot {band_past:.2} px");
        assert!(band < 0.5 && band_past < 1.0, "{band} {band_past}");
    }

    #[test]
    fn a_grab_mid_flight_keeps_the_velocity_the_spring_had() {
        let mut spring = Spring::at(1408.0);
        spring.target = 0.0;
        for _ in 0..12 {
            spring.step(SPRING, 1.0 / 120.0);
        }
        let (value, velocity) = (spring.value, spring.velocity);
        assert!(velocity < -1000.0, "{velocity}");
        // A new target mid-flight: the next frame goes on from where it was,
        // at the speed it had.
        spring.target = 300.0;
        spring.step(SPRING, 1.0 / 1000.0);
        assert!(spring.value < value);
        assert!((spring.velocity - velocity).abs() < 0.05 * velocity.abs());
    }

    #[test]
    fn a_tracker_measures_the_last_moments_and_a_resting_hand_has_no_speed() {
        let mut tracker = Tracker::default();
        // 10 px every 8 ms: 1250 px/s to the right.
        for step in 0..30 {
            tracker.push((step as f64 * 10.0, 5.0), 1000.0 + step as f64 * 8.0);
        }
        let (vx, vy) = tracker.velocity(1000.0 + 29.0 * 8.0);
        assert!((vx - 1250.0).abs() < 1.0, "{vx}");
        assert_eq!(vy, 0.0);
        // Old samples are dropped, so a slow start does not dilute it.
        assert!(tracker.samples.len() <= 13);
        // Held still for 200 ms, then let go.
        assert_eq!(tracker.velocity(1000.0 + 29.0 * 8.0 + 200.0), (0.0, 0.0));
        assert_eq!(Tracker::default().velocity(0.0), (0.0, 0.0));
    }
}
