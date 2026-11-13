//! What speed does to a window's picture: the streak a shutter would leave
//! along its way, and the stretch of a drawn thing in a hurry. Both are
//! read off the springs that move it, so neither has a clock of its own:
//! a window at rest has neither, and one that stops loses both at once.

use serde::Deserialize;

/// How long the shutter is open, in seconds: a quarter of a 240 Hz frame,
/// a 90 degree shutter there. It is a time and not a share of the output's
/// own frame, so a streak is as long on a 120 Hz output as on a 240 Hz one.
const SHUTTER: f64 = 1.0 / 960.0;
/// The length taken off every streak, in pixels, so it starts from nothing
/// and a slow move has none: at full strength, below 720 px/s.
const CORE: f64 = 0.75;
/// The longest streak at any strength, in pixels: under a line of text,
/// and a fling is no blurrier than a workspace switch at its fastest.
const LONGEST: f64 = 12.0;

/// The stretch at full amount that no speed passes, as the logarithm of
/// the scale: 3%.
const STRETCH: f64 = 0.03;
/// The speed of half that, in px/s. Below a third of it the stretch is
/// under a tenth of the full one.
const STRETCH_SPEED: f64 = 6000.0;
/// The squash of a window braking as hard as can be, half the stretch.
const SQUASH: f64 = 0.015;
/// The braking of half that, in px/s².
const SQUASH_BRAKING: f64 = 40_000.0;
/// How long a window has to go on braking before it counts in full, in
/// seconds. One that turns round on an overshoot brakes hardest at the
/// moment it stands still, and has to pass through that with no squash.
const SQUASH_ONSET: f64 = 0.03;

/// The most either setting goes to: twice what it was drawn for.
const MOST: f64 = 2.0;

/// One thing a window rides, as it moves it on screen: px/s and px/s².
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Drive {
    pub velocity: (f64, f64),
    pub accel: (f64, f64),
}

/// How a window travels this frame: with everything else on its tape and
/// its output's stack, and on the springs of its own.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Flow {
    pub group: Drive,
    pub own: Drive,
}

impl Flow {
    /// The window's speed across the screen, in px/s.
    pub fn velocity(&self) -> (f64, f64) {
        (
            self.group.velocity.0 + self.own.velocity.0,
            self.group.velocity.1 + self.own.velocity.1,
        )
    }
}

/// `motion` in the settings. Each is a share of the effect as it was
/// drawn: 0 is off, 1 is the design, 2 the most.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Smear {
    /// The blur along a moving window's way.
    pub blur: f64,
    /// The stretch along it and the squash as it stops.
    pub stretch: f64,
}

impl Default for Smear {
    fn default() -> Self {
        Self {
            blur: 1.0,
            stretch: 0.0,
        }
    }
}

impl Smear {
    pub fn check(&self) -> Result<(), String> {
        for (name, value) in [("blur", self.blur), ("stretch", self.stretch)] {
            if !(0.0..=MOST).contains(&value) {
                return Err(format!("motion.{name}: between 0 and {MOST}"));
            }
        }
        Ok(())
    }

    /// The streak of a window at `velocity`: its whole length along x and
    /// y, in pixels. What the shutter sees less `CORE`, and no longer than
    /// `LONGEST`.
    pub fn streak(&self, velocity: (f64, f64)) -> (f64, f64) {
        let speed = velocity.0.hypot(velocity.1);
        let length = (speed * SHUTTER * self.blur - CORE).clamp(0.0, LONGEST);
        if length <= 0.0 {
            return (0.0, 0.0);
        }
        (velocity.0 / speed * length, velocity.1 / speed * length)
    }

    /// The scale along x and y of what `drive` moves. Speed stretches it
    /// along its way, late: the square of the speed at first, so a slow
    /// move is left alone. Braking squashes it, early: the squash outlasts
    /// the stretch as the window comes to rest. Across its way it gives
    /// what it takes, so its area holds. A window can only be scaled along
    /// its own sides, so a way between the two axes stretches by the share
    /// of it that lies along one, and one at 45 degrees not at all.
    pub fn stretch(&self, drive: Drive) -> (f64, f64) {
        let (vx, vy) = drive.velocity;
        let speed = vx.hypot(vy);
        if self.stretch <= 0.0 || speed <= 0.0 {
            return (1.0, 1.0);
        }
        let fast = speed / STRETCH_SPEED;
        let stretch = STRETCH * fast * fast / (1.0 + fast * fast);
        // The power the brake takes out of it, over its speed: how hard it
        // brakes, fading out under the gate.
        let power = -(drive.accel.0 * vx + drive.accel.1 * vy);
        let (ax, ay) = drive.accel;
        let gate = speed + ax.hypot(ay) * SQUASH_ONSET;
        let braking = power.max(0.0) / gate / SQUASH_BRAKING;
        let squash = SQUASH * braking / (1.0 + braking);
        let along = (vx * vx - vy * vy) / (speed * speed);
        let log = self.stretch * (stretch - squash) * along;
        (log.exp(), (-log).exp())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::motion::{Spring, SpringParams};

    const ON: Smear = Smear {
        blur: 1.0,
        stretch: 1.0,
    };

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn a_window_at_rest_or_moving_slowly_leaves_no_streak() {
        assert_eq!(ON.streak((0.0, 0.0)), (0.0, 0.0));
        assert_eq!(ON.streak((700.0, 0.0)), (0.0, 0.0));
        assert_eq!(ON.streak((-500.0, 500.0)), (0.0, 0.0));
        let (x, _) = ON.streak((800.0, 0.0));
        assert!(x > 0.0 && x < 0.1, "{x}");
    }

    #[test]
    fn a_streak_is_what_a_quarter_of_a_240_hz_frame_sees_less_the_core() {
        let (x, y) = ON.streak((-4800.0, 0.0));
        assert!(close(x, -4.25) && y == 0.0, "{x} {y}");
        let (x, y) = ON.streak((0.0, 9600.0));
        assert!(close(y, 9.25) && x == 0.0, "{x} {y}");
        // Along the way, whatever the way.
        let (x, y) = ON.streak((3000.0, -4000.0));
        assert!(close(x.hypot(y), 5000.0 / 960.0 - 0.75) && close(x / y, -0.75));
    }

    #[test]
    fn a_streak_is_never_longer_than_the_longest() {
        for speed in [13_000.0, 40_000.0, 1e7] {
            let (x, y) = ON.streak((speed, 0.0));
            assert_eq!((x, y), (12.0, 0.0));
        }
        // The most blur opens the shutter twice as long and reaches the
        // same longest streak sooner.
        let most = Smear { blur: 2.0, ..ON };
        assert_eq!(most.streak((1e6, 0.0)), (12.0, 0.0));
        assert!(close(most.streak((4800.0, 0.0)).0, 9.25));
    }

    #[test]
    fn off_is_off() {
        let off = Smear {
            blur: 0.0,
            stretch: 0.0,
        };
        let drive = Drive {
            velocity: (9000.0, 0.0),
            accel: (-60_000.0, 0.0),
        };
        assert_eq!(off.streak(drive.velocity), (0.0, 0.0));
        assert_eq!(off.stretch(drive), (1.0, 1.0));
        assert_eq!(Smear::default().stretch(drive), (1.0, 1.0));
        assert!(Smear::default().streak(drive.velocity).0 > 0.0);
    }

    #[test]
    fn a_window_stretches_along_its_way_and_keeps_its_area() {
        let coasting = |velocity| Drive {
            velocity,
            accel: (0.0, 0.0),
        };
        let (x, y) = ON.stretch(coasting((6000.0, 0.0)));
        assert!(close(x, 0.015_f64.exp()) && close(x * y, 1.0), "{x} {y}");
        let (x, y) = ON.stretch(coasting((0.0, -6000.0)));
        assert!(close(y, 0.015_f64.exp()) && close(x * y, 1.0), "{x} {y}");
        // Never past the full stretch, and nothing to speak of when slow.
        let (x, _) = ON.stretch(coasting((1e7, 0.0)));
        assert!(x < 0.03_f64.exp());
        let (x, _) = ON.stretch(coasting((1000.0, 0.0)));
        assert!(x - 1.0 < 0.001, "{x}");
        // Halfway between the axes there is no side to stretch along.
        assert_eq!(ON.stretch(coasting((4000.0, 4000.0))), (1.0, 1.0));
    }

    #[test]
    fn at_rest_the_scale_is_exactly_one() {
        assert_eq!(ON.stretch(Drive::default()), (1.0, 1.0));
        let pulled = Drive {
            velocity: (0.0, 0.0),
            accel: (50_000.0, 0.0),
        };
        assert_eq!(ON.stretch(pulled), (1.0, 1.0));
    }

    /// The speed and the scale along x through a spring's whole way, a
    /// 240 Hz frame at a time.
    fn ride(params: SpringParams, from: f64) -> Vec<(f64, f64)> {
        let mut spring = Spring::at(from);
        spring.target = 0.0;
        let mut frames = Vec::new();
        while !spring.resting() {
            spring.step(params, 1.0 / 240.0);
            let drive = Drive {
                velocity: (spring.velocity, 0.0),
                accel: (spring.accel(params), 0.0),
            };
            frames.push((spring.velocity, ON.stretch(drive).0));
        }
        frames
    }

    #[test]
    fn it_stretches_into_a_move_and_squashes_a_little_as_it_lands() {
        let critical = SpringParams {
            omega: 6.64 / 0.35,
            zeta: 1.0,
        };
        let scales: Vec<f64> = ride(critical, 1300.0)
            .into_iter()
            .map(|(_, scale)| scale)
            .collect();
        let (most, least) = scales
            .iter()
            .fold((1.0_f64, 1.0_f64), |(most, least), scale| {
                (most.max(*scale), least.min(*scale))
            });
        assert!(most > 1.015 && most < 1.03, "{most}");
        assert!(least < 0.998 && least > 0.99, "{least}");
        // Stretched first, squashed after, and back to itself at the end.
        let peak = scales.iter().position(|scale| *scale == most).unwrap();
        let trough = scales.iter().position(|scale| *scale == least).unwrap();
        assert!(peak < trough);
        assert!((scales.last().unwrap() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn a_move_that_overshoots_turns_round_without_a_jump() {
        let bouncy = SpringParams {
            omega: 24.0,
            zeta: 0.45,
        };
        let frames = ride(bouncy, 1300.0);
        let mut turns = 0;
        for pair in frames.windows(2) {
            let ((before, was), (after, is)) = (pair[0], pair[1]);
            // A hundredth of the window a frame at the very most, which is
            // the spring's own kick at the start.
            assert!((is - was).abs() < 0.01, "{was} to {is}");
            if before * after <= 0.0 {
                turns += 1;
                assert!(
                    (was - 1.0).abs() < 2e-3 && (is - 1.0).abs() < 2e-3,
                    "{was} to {is}"
                );
            }
        }
        assert!(turns >= 3, "it turned round {turns} times");
    }
}
