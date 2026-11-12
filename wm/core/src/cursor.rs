//! The dynamic cursor: the image stretches along the way the pointer
//! travels, or leans into it, and grows while the pointer is shaken so it is
//! easy to find. The rules and the numbers are hypr-dynamic-cursors'.

use std::collections::VecDeque;

use fs_wm_proto::Family;
use serde::Deserialize;

use crate::motion::Tween;

/// The compositor draws a cursor at up to this many times its size.
const MAX_SCALE: f64 = 8.0;
/// How far back a shake is looked for, in milliseconds.
const SHAKE_WINDOW: f64 = 1000.0;
/// The magnification settles on this curve; the plugin's own.
const ZOOM: Family = Family {
    curve: [0.22, 1.0, 0.36, 1.0],
    ms: 400.0,
};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// Longer along the way it travels, thinner across it, still upright.
    #[default]
    Stretch,
    /// Leaning into sideways travel.
    Tilt,
    None,
}

/// How a speed turns into a share of the full effect.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Activation {
    Linear,
    /// Late: little at ordinary speeds.
    Quadratic,
    /// Early: most of it at ordinary speeds, all of it at the limit.
    #[default]
    NegativeQuadratic,
}

impl Activation {
    /// The share at `speed`, -1 to 1, reaching 1 at `limit`.
    fn at(self, limit: f64, speed: f64) -> f64 {
        let x = (speed.abs() / limit).min(1.0);
        let share = match self {
            Activation::Linear => x,
            Activation::Quadratic => x * x,
            Activation::NegativeQuadratic => 1.0 - (x - 1.0) * (x - 1.0),
        };
        share * speed.signum()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Stretch {
    /// The speed of the full stretch, in pixels a second.
    pub limit: f64,
    pub activation: Activation,
    /// The milliseconds the speed is taken over.
    pub window: f64,
}

impl Default for Stretch {
    fn default() -> Self {
        Self {
            limit: 3000.0,
            activation: Activation::default(),
            window: 100.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Tilt {
    /// The full lean to each side, in degrees.
    pub full: f64,
    pub limit: f64,
    pub activation: Activation,
    pub window: f64,
}

impl Default for Tilt {
    fn default() -> Self {
        Self {
            full: 20.0,
            limit: 3000.0,
            activation: Activation::default(),
            window: 100.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Shake {
    pub enabled: bool,
    /// How many times its own span the pointer travels within a second
    /// before that is a shake.
    pub threshold: f64,
    /// The magnification a shake starts at.
    pub base: f64,
    /// What a second of shaking adds to it.
    pub speed: f64,
    /// How much a harder shake adds on top.
    pub influence: f64,
    /// The most it magnifies; 0 for the compositor's own bound.
    pub limit: f64,
    /// The milliseconds it stays magnified after the shake.
    pub timeout: f64,
    /// The stretch or the lean shows on a magnified cursor too.
    pub effects: bool,
}

impl Default for Shake {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold: 6.0,
            base: 4.0,
            speed: 4.0,
            influence: 0.0,
            limit: 0.0,
            timeout: 2000.0,
            effects: false,
        }
    }
}

/// `cursor.dynamic` in the settings.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Dynamic {
    pub mode: Mode,
    /// The change in degrees below which the image is left as it is.
    pub threshold: f64,
    pub stretch: Stretch,
    pub tilt: Tilt,
    pub shake: Shake,
}

impl Default for Dynamic {
    fn default() -> Self {
        Self {
            mode: Mode::default(),
            threshold: 2.0,
            stretch: Stretch::default(),
            tilt: Tilt::default(),
            shake: Shake::default(),
        }
    }
}

impl Dynamic {
    pub fn enabled(&self) -> bool {
        self.mode != Mode::None || self.shake.enabled
    }

    pub fn check(&self) -> Result<(), String> {
        let positive = [
            ("stretch.limit", self.stretch.limit),
            ("tilt.limit", self.tilt.limit),
            ("shake.base", self.shake.base),
        ];
        for (name, value) in positive {
            if !(value.is_finite() && value > 0.0) {
                return Err(format!("cursor.dynamic.{name}: has to be above 0"));
            }
        }
        for (name, window) in [
            ("stretch.window", self.stretch.window),
            ("tilt.window", self.tilt.window),
        ] {
            if !(1.0..=SHAKE_WINDOW).contains(&window) {
                return Err(format!(
                    "cursor.dynamic.{name}: between 1 and {SHAKE_WINDOW} milliseconds"
                ));
            }
        }
        let at_least_zero = [
            ("threshold", self.threshold),
            ("shake.threshold", self.shake.threshold),
            ("shake.speed", self.shake.speed),
            ("shake.influence", self.shake.influence),
            ("shake.limit", self.shake.limit),
            ("shake.timeout", self.shake.timeout),
        ];
        for (name, value) in at_least_zero {
            if !(value.is_finite() && value >= 0.0) {
                return Err(format!("cursor.dynamic.{name}: has to be 0 or more"));
            }
        }
        if !(self.tilt.full.is_finite() && self.tilt.full.abs() <= 180.0) {
            return Err("cursor.dynamic.tilt.full: between -180 and 180 degrees".to_owned());
        }
        Ok(())
    }
}

/// How the image is drawn, around its hotspot: scaled along an axis and
/// across it, then turned. Degrees are clockwise, the axis from the screen's
/// x.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub rotate: f64,
    pub along: f64,
    pub across: f64,
    pub axis: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Look {
    rotate: f64,
    zoom: f64,
    axis: f64,
    along: f64,
    across: f64,
}

const REST: Look = Look {
    rotate: 0.0,
    zoom: 1.0,
    axis: 0.0,
    along: 1.0,
    across: 1.0,
};

impl Look {
    fn differs(&self, other: &Look, angle: f64) -> bool {
        (other.rotate - self.rotate).abs() > angle
            || (other.axis - self.axis).abs() > angle
            || (other.zoom - self.zoom).abs() > 0.01
            || (other.along - self.along).abs() > 0.01
            || (other.across - self.across).abs() > 0.01
    }

    /// Next to nothing is nothing, so a cursor at rest is drawn untouched.
    fn settled(mut self, angle: f64) -> Look {
        if self.rotate.abs() < angle {
            self.rotate = 0.0;
        }
        if (1.0 - self.zoom).abs() < 0.01 {
            self.zoom = 1.0;
        }
        if (1.0 - self.across).abs() < 0.01 {
            (self.along, self.across, self.axis) = (1.0, 1.0, 0.0);
        }
        self
    }
}

#[derive(Clone, Copy, Debug)]
struct Sample {
    ms: f64,
    x: f64,
    y: f64,
}

/// The cursor's motion as its clock sees it.
#[derive(Clone, Debug)]
pub struct Cursor {
    samples: VecDeque<Sample>,
    shown: Look,
    zoom: Tween,
    shaking: bool,
    /// When the magnification lets go, on the clock's milliseconds.
    until: f64,
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            samples: VecDeque::new(),
            shown: REST,
            zoom: Tween::at(1.0),
            shaking: false,
            until: 0.0,
        }
    }
}

impl Cursor {
    /// Forgets the motion: the session locked, the cursor went.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// One turn of the clock with the pointer at `at`. None draws the image
    /// as it is.
    pub fn step(&mut self, settings: &Dynamic, now: f64, at: (f64, f64)) -> Option<Shape> {
        let dt = self
            .samples
            .back()
            .map_or(0.0, |last| ((now - last.ms) / 1000.0).clamp(0.0, 0.05));
        self.samples.push_back(Sample {
            ms: now,
            x: at.0,
            y: at.1,
        });
        // One sample from before the window stays: where the pointer was
        // when the window began.
        while self.samples.len() > 1 && self.samples[1].ms <= now - SHAKE_WINDOW {
            self.samples.pop_front();
        }
        self.zoom.step(dt);

        let mut look = match settings.mode {
            Mode::Stretch => {
                let (vx, vy) = self.velocity(settings.stretch.window);
                let speed = vx.hypot(vy);
                let share = settings
                    .stretch
                    .activation
                    .at(settings.stretch.limit, speed);
                Look {
                    axis: if speed > 0.0 {
                        vy.atan2(vx).to_degrees()
                    } else {
                        0.0
                    },
                    along: 1.0 + share,
                    across: 1.0 - share * 0.5,
                    ..REST
                }
            }
            Mode::Tilt => {
                let (vx, _) = self.velocity(settings.tilt.window);
                Look {
                    rotate: settings.tilt.activation.at(settings.tilt.limit, vx)
                        * settings.tilt.full,
                    ..REST
                }
            }
            Mode::None => REST,
        };
        if settings.shake.enabled {
            let zoom = self.shake(&settings.shake, now, dt);
            if zoom > 1.0 && !settings.shake.effects {
                look = REST;
            }
            look.zoom = zoom;
        }
        if self.shown.differs(&look, settings.threshold) {
            self.shown = look.settled(settings.threshold);
        }
        self.shape()
    }

    /// Nothing left to play: the clock can stop until the pointer moves.
    pub fn resting(&self) -> bool {
        let still = self
            .samples
            .back()
            .is_none_or(|last| self.samples.iter().all(|s| (s.x, s.y) == (last.x, last.y)));
        still && self.shown == REST && !self.shaking && self.zoom.resting(&ZOOM)
    }

    fn shape(&self) -> Option<Shape> {
        let look = self.shown;
        (look != REST).then(|| Shape {
            rotate: look.rotate,
            along: (look.zoom * look.along).min(MAX_SCALE),
            across: (look.zoom * look.across).min(MAX_SCALE),
            axis: look.axis,
        })
    }

    /// Pixels a second over the last `window` milliseconds.
    fn velocity(&self, window: f64) -> (f64, f64) {
        let Some(now) = self.samples.back() else {
            return (0.0, 0.0);
        };
        let then = self
            .samples
            .iter()
            .rev()
            .find(|sample| sample.ms <= now.ms - window)
            .or(self.samples.front())
            .unwrap_or(now);
        (
            (now.x - then.x) / window * 1000.0,
            (now.y - then.y) / window * 1000.0,
        )
    }

    /// The magnification: a pointer that travels many times the span it
    /// stays within is being shaken.
    fn shake(&mut self, shake: &Shake, now: f64, dt: f64) -> f64 {
        let mut trail = 0.0;
        let (mut left, mut top) = (f64::INFINITY, f64::INFINITY);
        let (mut right, mut bottom) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        let mut last: Option<&Sample> = None;
        for sample in &self.samples {
            if let Some(last) = last {
                trail += (sample.x - last.x).hypot(sample.y - last.y);
            }
            last = Some(sample);
            (left, right) = (left.min(sample.x), right.max(sample.x));
            (top, bottom) = (top.min(sample.y), bottom.max(sample.y));
        }
        let span = (right - left).hypot(bottom - top);
        let amount = trail / span - shake.threshold;
        if span > 100.0 && amount > 0.0 {
            let from = if self.shaking {
                self.zoom.to
            } else {
                shake.base
            };
            let mut to = from + dt * (shake.speed + amount * amount * shake.influence);
            if shake.limit > 1.0 {
                to = to.min(shake.limit);
            }
            self.zoom.retarget(to.min(MAX_SCALE), &ZOOM);
            self.until = now + shake.timeout;
            self.shaking = true;
        } else if self.shaking && self.until < now {
            self.zoom.retarget(1.0, &ZOOM);
            self.shaking = false;
        }
        self.zoom.value(&ZOOM)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Moves by `step` every 10 ms for `turns` turns from where it is.
    fn travel(
        cursor: &mut Cursor,
        settings: &Dynamic,
        clock: &mut f64,
        at: &mut (f64, f64),
        step: (f64, f64),
        turns: usize,
    ) -> Option<Shape> {
        let mut shape = None;
        for _ in 0..turns {
            *clock += 10.0;
            (at.0, at.1) = (at.0 + step.0, at.1 + step.1);
            shape = cursor.step(settings, *clock, *at);
        }
        shape
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn the_speed_s_share_of_the_full_effect() {
        let early = Activation::NegativeQuadratic;
        assert!(close(early.at(3000.0, 1500.0), 0.75));
        assert!(close(early.at(3000.0, -1500.0), -0.75));
        assert!(close(early.at(3000.0, 9000.0), 1.0));
        assert!(close(early.at(3000.0, 0.0), 0.0));
        assert!(close(Activation::Quadratic.at(3000.0, 1500.0), 0.25));
        assert!(close(Activation::Linear.at(3000.0, -1500.0), -0.5));
    }

    #[test]
    fn a_pointer_at_speed_stretches_along_its_way_and_stays_upright() {
        let settings = Dynamic {
            shake: Shake {
                enabled: false,
                ..Shake::default()
            },
            ..Dynamic::default()
        };
        let (mut cursor, mut clock, mut at) = (Cursor::default(), 0.0, (500.0, 500.0));
        assert_eq!(cursor.step(&settings, clock, at), None);
        assert!(cursor.resting());

        // 1500 px/s to the right: three quarters of the way to the limit.
        let shape = travel(&mut cursor, &settings, &mut clock, &mut at, (15.0, 0.0), 20).unwrap();
        assert!(close(shape.along, 1.75) && close(shape.across, 0.625));
        assert_eq!((shape.axis, shape.rotate), (0.0, 0.0));
        assert!(!cursor.resting());

        // Down and to the right: the axis turns, the image does not.
        let shape = travel(
            &mut cursor,
            &settings,
            &mut clock,
            &mut at,
            (15.0, 15.0),
            20,
        )
        .unwrap();
        assert!(close(shape.axis, 45.0) && shape.rotate == 0.0);
        let shape = travel(
            &mut cursor,
            &settings,
            &mut clock,
            &mut at,
            (0.0, -15.0),
            20,
        )
        .unwrap();
        assert!(close(shape.axis, -90.0) && close(shape.along, 1.75));

        // At a stop it is drawn as it is again, and the clock can stop.
        assert_eq!(
            travel(&mut cursor, &settings, &mut clock, &mut at, (0.0, 0.0), 11),
            None
        );
        assert!(
            !cursor.resting(),
            "the second it looks back over is not still yet"
        );
        travel(&mut cursor, &settings, &mut clock, &mut at, (0.0, 0.0), 100);
        assert!(cursor.resting());

        // The first move after a rest counts from where it rested.
        clock += 60_000.0;
        at.0 += 30.0;
        let shape = cursor.step(&settings, clock, at).unwrap();
        assert!(shape.along > 1.0 && shape.axis == 0.0);
    }

    #[test]
    fn a_tilt_leans_into_sideways_travel() {
        let settings = Dynamic {
            mode: Mode::Tilt,
            ..Dynamic::default()
        };
        let (mut cursor, mut clock, mut at) = (Cursor::default(), 0.0, (500.0, 500.0));
        let shape = travel(&mut cursor, &settings, &mut clock, &mut at, (15.0, 0.0), 20).unwrap();
        assert!(close(shape.rotate, 15.0));
        assert_eq!((shape.along, shape.across), (1.0, 1.0));
        let shape = travel(
            &mut cursor,
            &settings,
            &mut clock,
            &mut at,
            (-15.0, 40.0),
            20,
        )
        .unwrap();
        assert!(close(shape.rotate, -15.0));
        // A lean under the threshold is no lean.
        let slow = travel(&mut cursor, &settings, &mut clock, &mut at, (0.5, 0.0), 30);
        assert_eq!(slow, None);
    }

    #[test]
    fn a_shaken_pointer_grows_and_goes_back_after_the_timeout() {
        let settings = Dynamic::default();
        let (mut cursor, mut clock, mut at) = (Cursor::default(), 0.0, (500.0, 500.0));
        let mut shape = None;
        for _ in 0..40 {
            travel(&mut cursor, &settings, &mut clock, &mut at, (100.0, 0.0), 2);
            shape = travel(
                &mut cursor,
                &settings,
                &mut clock,
                &mut at,
                (-100.0, 0.0),
                2,
            );
        }
        // 1.6 s of it: from 4 on its way up by 4 a second, the same on both
        // axes, with no stretch while it is magnified.
        let grown = shape.unwrap();
        assert!(grown.along > 4.0 && grown.along <= MAX_SCALE, "{grown:?}");
        assert_eq!(
            (grown.along, grown.axis, grown.rotate),
            (grown.across, 0.0, 0.0)
        );

        // Still for less than the timeout: still magnified.
        let held = travel(&mut cursor, &settings, &mut clock, &mut at, (0.0, 0.0), 150).unwrap();
        assert!(held.along > 4.0);
        assert!(!cursor.resting());
        // Past it, counted from when the last second stopped looking like a
        // shake, and past the 400 ms it takes to shrink: as it was.
        assert_eq!(
            travel(&mut cursor, &settings, &mut clock, &mut at, (0.0, 0.0), 200),
            None
        );
        assert!(cursor.resting());

        // Shaking on never passes what the compositor draws.
        for _ in 0..200 {
            travel(&mut cursor, &settings, &mut clock, &mut at, (100.0, 0.0), 2);
            shape = travel(
                &mut cursor,
                &settings,
                &mut clock,
                &mut at,
                (-100.0, 0.0),
                2,
            );
        }
        let most = shape.unwrap().along;
        assert!(most > MAX_SCALE - 0.02 && most <= MAX_SCALE, "{most}");
    }

    #[test]
    fn the_settings_default_to_the_owner_s_and_refuse_what_cannot_be() {
        let dynamic = Dynamic::default();
        assert_eq!(dynamic.mode, Mode::Stretch);
        assert_eq!(
            (
                dynamic.tilt.full,
                dynamic.tilt.limit,
                dynamic.tilt.activation
            ),
            (20.0, 3000.0, Activation::NegativeQuadratic)
        );
        assert!(dynamic.shake.enabled && dynamic.enabled());
        assert_eq!(dynamic.check(), Ok(()));

        let off: Dynamic =
            serde_json::from_str(r#"{ "mode": "none", "shake": { "enabled": false } }"#).unwrap();
        assert!(!off.enabled());
        let bad: Dynamic = serde_json::from_str(r#"{ "stretch": { "limit": 0 } }"#).unwrap();
        assert!(bad.check().unwrap_err().contains("stretch.limit"));
        let bad: Dynamic = serde_json::from_str(r#"{ "tilt": { "window": 5000 } }"#).unwrap();
        assert!(bad.check().unwrap_err().contains("tilt.window"));
        assert!(serde_json::from_str::<Dynamic>(r#"{ "mode": "rotate" }"#).is_err());
    }
}
