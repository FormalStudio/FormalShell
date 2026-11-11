//! The shell's clock families as motion: a cubic bezier with a duration for
//! opacity and colour, and the spring that curve describes for anything with
//! a position or a size, so a move can be interrupted and keep its velocity.

use fs_wm_proto::Family;

/// A CSS-style cubic bezier through (0,0) and (1,1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bezier {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

impl Bezier {
    pub fn new(curve: [f64; 4]) -> Self {
        Self {
            x1: curve[0].clamp(0.0, 1.0),
            y1: curve[1],
            x2: curve[2].clamp(0.0, 1.0),
            y2: curve[3],
        }
    }

    fn axis(a: f64, b: f64, t: f64) -> f64 {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    }

    /// The curve's value at time fraction `x`.
    pub fn at(&self, x: f64) -> f64 {
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }
        let (mut low, mut high) = (0.0, 1.0);
        for _ in 0..40 {
            let mid = (low + high) / 2.0;
            if Self::axis(self.x1, self.x2, mid) < x {
                low = mid;
            } else {
                high = mid;
            }
        }
        Self::axis(self.y1, self.y2, (low + high) / 2.0)
    }
}

/// A damped spring's constants.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpringParams {
    /// Undamped angular frequency, radians per second.
    pub omega: f64,
    /// Damping ratio: 1 is critical, below 1 overshoots.
    pub zeta: f64,
}

impl SpringParams {
    /// The spring that overshoots as far as the family's curve does and
    /// peaks when it does. A curve that never passes 1 gives a critically
    /// damped spring that has arrived by the family's duration.
    pub fn from_family(family: &Family) -> Self {
        let seconds = (family.ms / 1000.0).max(0.001);
        let bezier = Bezier::new(family.curve);
        let (mut peak, mut peak_at) = (1.0, 1.0);
        for step in 1..400 {
            let x = step as f64 / 400.0;
            let y = bezier.at(x);
            if y > peak {
                peak = y;
                peak_at = x;
            }
        }
        let overshoot = peak - 1.0;
        if overshoot < 0.005 {
            // (1 + wt) e^(-wt) = 0.01 at wt = 6.64.
            return Self {
                omega: 6.64 / seconds,
                zeta: 1.0,
            };
        }
        let log = overshoot.ln();
        let zeta = (-log / (std::f64::consts::PI.powi(2) + log * log).sqrt()).clamp(0.2, 0.99);
        let omega = std::f64::consts::PI / (peak_at * seconds * (1.0 - zeta * zeta).sqrt());
        Self { omega, zeta }
    }
}

/// One value on a spring.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub value: f64,
    pub velocity: f64,
    pub target: f64,
}

/// Below this distance and speed a spring has arrived.
const REST_DISTANCE: f64 = 0.05;
const REST_SPEED: f64 = 1.0;

impl Spring {
    pub fn at(value: f64) -> Self {
        Self {
            value,
            velocity: 0.0,
            target: value,
        }
    }

    pub fn resting(&self) -> bool {
        self.value == self.target && self.velocity == 0.0
    }

    pub fn jump(&mut self, value: f64) {
        *self = Self::at(value);
    }

    /// Advances by `dt` seconds on the exact solution, so a long frame
    /// cannot blow it up.
    pub fn step(&mut self, params: SpringParams, dt: f64) {
        self.step_in(params, dt, 1.0);
    }

    /// `step` for a value whose unit is not a pixel: `unit` is how much of
    /// it one pixel is worth, so it comes to rest as finely.
    pub fn step_in(&mut self, params: SpringParams, dt: f64, unit: f64) {
        if self.resting() {
            return;
        }
        let SpringParams { omega, zeta } = params;
        let x0 = self.value - self.target;
        let v0 = self.velocity;
        let (x, v) = if zeta < 1.0 {
            let damped = omega * (1.0 - zeta * zeta).sqrt();
            let decay = (-zeta * omega * dt).exp();
            let (sin, cos) = (damped * dt).sin_cos();
            let b = (v0 + zeta * omega * x0) / damped;
            let x = decay * (x0 * cos + b * sin);
            let v = decay
                * ((b * damped - zeta * omega * x0) * cos - (x0 * damped + zeta * omega * b) * sin);
            (x, v)
        } else {
            let decay = (-omega * dt).exp();
            let b = v0 + omega * x0;
            let x = decay * (x0 + b * dt);
            let v = decay * (b - omega * (x0 + b * dt));
            (x, v)
        };
        if x.abs() < REST_DISTANCE * unit && v.abs() < REST_SPEED * unit {
            self.jump(self.target);
        } else {
            self.value = self.target + x;
            self.velocity = v;
        }
    }
}

/// A value on a timed curve: opacity and colour. Retargeting restarts the
/// clock from wherever the value is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tween {
    pub from: f64,
    pub to: f64,
    pub elapsed: f64,
}

impl Tween {
    pub fn at(value: f64) -> Self {
        Self {
            from: value,
            to: value,
            elapsed: f64::INFINITY,
        }
    }

    pub fn value(&self, family: &Family) -> f64 {
        let seconds = family.ms / 1000.0;
        if self.elapsed >= seconds || seconds <= 0.0 {
            return self.to;
        }
        let eased = Bezier::new(family.curve).at(self.elapsed / seconds);
        self.from + (self.to - self.from) * eased
    }

    pub fn resting(&self, family: &Family) -> bool {
        self.elapsed >= family.ms / 1000.0
    }

    pub fn retarget(&mut self, to: f64, family: &Family) {
        if to == self.to {
            return;
        }
        self.from = self.value(family);
        self.to = to;
        self.elapsed = 0.0;
    }

    pub fn jump(&mut self, value: f64) {
        *self = Self::at(value);
    }

    pub fn step(&mut self, dt: f64) {
        if self.elapsed.is_finite() {
            self.elapsed += dt;
        }
    }
}

/// Where a release at `velocity` (px/s) carries a value that decelerates
/// like a flung scroll view.
pub fn project(velocity: f64) -> f64 {
    const DECELERATION: f64 = 0.997;
    velocity / 1000.0 * DECELERATION / (1.0 - DECELERATION)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs_wm_proto::Families;

    fn settle(spring: &mut Spring, params: SpringParams, seconds: f64) -> f64 {
        let mut peak = spring.value;
        let steps = (seconds * 240.0) as usize;
        for _ in 0..steps {
            spring.step(params, 1.0 / 240.0);
            peak = peak.max(spring.value);
        }
        peak
    }

    #[test]
    fn bezier_ends_and_linear_middle() {
        let linear = Bezier::new([0.25, 0.25, 0.75, 0.75]);
        assert_eq!(linear.at(0.0), 0.0);
        assert_eq!(linear.at(1.0), 1.0);
        assert!((linear.at(0.4) - 0.4).abs() < 1e-6);
    }

    #[test]
    fn a_spatial_spring_overshoots_as_far_as_its_curve() {
        let family = Families::default().spatial_fast;
        let bezier = Bezier::new(family.curve);
        let curve_peak = (1..400)
            .map(|step| bezier.at(step as f64 / 400.0))
            .fold(1.0, f64::max);
        assert!(curve_peak > 1.02, "spatialFast overshoots");
        let params = SpringParams::from_family(&family);
        let mut spring = Spring::at(0.0);
        spring.target = 100.0;
        let peak = settle(&mut spring, params, 2.0);
        assert!(
            (peak / 100.0 - curve_peak).abs() < 0.01,
            "{peak} vs {curve_peak}"
        );
        assert!(spring.resting());
        assert_eq!(spring.value, 100.0);
    }

    #[test]
    fn an_effects_spring_never_overshoots_and_arrives_on_time() {
        let family = Families::default().effects;
        let params = SpringParams::from_family(&family);
        assert_eq!(params.zeta, 1.0);
        let mut spring = Spring::at(0.0);
        spring.target = 1000.0;
        let peak = settle(&mut spring, params, family.ms / 1000.0);
        assert!(peak <= 1000.0);
        assert!((spring.value - 1000.0).abs() < 12.0);
    }

    #[test]
    fn retargeting_mid_flight_keeps_the_velocity() {
        let params = SpringParams::from_family(&Families::default().spatial);
        let mut spring = Spring::at(0.0);
        spring.target = 100.0;
        for _ in 0..10 {
            spring.step(params, 1.0 / 120.0);
        }
        let velocity = spring.velocity;
        assert!(velocity > 0.0);
        spring.target = -50.0;
        let before = spring.value;
        spring.step(params, 1.0 / 1000.0);
        assert!(spring.value > before, "still carried by its velocity");
    }

    #[test]
    fn one_long_step_equals_many_short_ones() {
        let params = SpringParams::from_family(&Families::default().spatial);
        let mut long = Spring::at(0.0);
        long.target = 300.0;
        let mut short = long;
        long.step(params, 0.2);
        for _ in 0..200 {
            short.step(params, 0.001);
        }
        assert!((long.value - short.value).abs() < 1e-6);
        assert!((long.velocity - short.velocity).abs() < 1e-4);
    }

    #[test]
    fn a_tween_restarts_from_where_it_is() {
        let family = Families::default().effects;
        let mut tween = Tween::at(0.0);
        tween.retarget(1.0, &family);
        tween.step(0.05);
        let mid = tween.value(&family);
        assert!(mid > 0.0 && mid < 1.0);
        tween.retarget(0.0, &family);
        assert_eq!(tween.value(&family), mid);
        tween.step(1.0);
        assert_eq!(tween.value(&family), 0.0);
        assert!(tween.resting(&family));
    }
}
