//! Head motion off the librepods daemon's `motion:watch` stream: one JSON
//! line per sample, `{"q":[w,x,y,z],"g":[x,y,z],"gyro":[x,y,z],"t":<ms>}`,
//! 25 a second.
//!
//! Frames: `q` takes a vector in the buds' body frame to a world frame whose
//! z points up (rotating world (0, 0, -1) by its conjugate gives the
//! measured gravity direction). The body frame follows the bud, not the
//! head, so [`Quat::relative_to`] measures in a head frame (x forward, y
//! left, z up): yaw is the turn about the vertical (positive to the left),
//! pitch about the side axis (positive nodding down) and roll about forward
//! (positive tilting the right ear down). There is no magnetometer:
//! yaw only means something against a reference taken from an earlier sample.

use serde_json::Value as Json;

/// A unit quaternion, normalised on the way in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quat {
    w: f32,
    x: f32,
    y: f32,
    z: f32,
}

/// Yaw, pitch and roll in degrees, see the module header for the axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EulerDegrees {
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
}

impl Quat {
    pub const IDENTITY: Quat = Quat { w: 1.0, x: 0.0, y: 0.0, z: 0.0 };

    /// None for a non-finite component or a zero vector, which the daemon
    /// never sends but a corrupt line could.
    pub fn new(w: f32, x: f32, y: f32, z: f32) -> Option<Quat> {
        let norm = (w * w + x * x + y * y + z * z).sqrt();
        if !norm.is_finite() || norm < 1e-6 {
            return None;
        }
        Some(Quat { w: w / norm, x: x / norm, y: y / norm, z: z / norm })
    }

    /// `[w, x, y, z]`.
    pub fn components(self) -> [f32; 4] {
        [self.w, self.x, self.y, self.z]
    }

    pub(crate) fn conjugate(self) -> Quat {
        Quat { w: self.w, x: -self.x, y: -self.y, z: -self.z }
    }

    /// The Hamilton product `self ⊗ other`, not renormalised.
    fn product(self, other: Quat) -> Quat {
        let (a, b) = (self, other);
        Quat {
            w: a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
            x: a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
            y: a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
            z: a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
        }
    }

    /// `self ⊗ other`, renormalised against drift.
    fn then(self, other: Quat) -> Quat {
        let p = self.product(other);
        Quat::new(p.w, p.x, p.y, p.z).unwrap_or(Quat::IDENTITY)
    }

    /// How far the head has turned from `reference` to `self`, in a head
    /// frame of x forward, y left, z up. A bud sits in the ear at a tilt of
    /// its own, so its body axes are not the head's (worn, an A3064 turns
    /// about its body y with the head and about its body z with a nod).
    /// The turn is taken in the world frame, `self ⊗ reference⁻¹`, and
    /// expressed in a head frame built at the reference off gravity: z up,
    /// y the body's z (out of the ear) laid level, x their cross product.
    pub(crate) fn relative_to(self, reference: Quat) -> Quat {
        let head = head_frame(reference);
        head.conjugate().then(self.then(reference.conjugate())).then(head)
    }

    /// The rotation whose columns are `x`, `y` and `z`, an orthonormal
    /// right-handed basis.
    fn from_basis(x: [f32; 3], y: [f32; 3], z: [f32; 3]) -> Quat {
        let (m00, m11, m22) = (x[0], y[1], z[2]);
        let trace = m00 + m11 + m22;
        let (w, qx, qy, qz) = if trace > 0.0 {
            let s = (trace + 1.0).sqrt() * 2.0;
            (0.25 * s, (y[2] - z[1]) / s, (z[0] - x[2]) / s, (x[1] - y[0]) / s)
        } else if m00 > m11 && m00 > m22 {
            let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
            ((y[2] - z[1]) / s, 0.25 * s, (y[0] + x[1]) / s, (z[0] + x[2]) / s)
        } else if m11 > m22 {
            let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
            ((z[0] - x[2]) / s, (y[0] + x[1]) / s, 0.25 * s, (z[1] + y[2]) / s)
        } else {
            let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
            ((x[1] - y[0]) / s, (z[0] + x[2]) / s, (z[1] + y[2]) / s, 0.25 * s)
        };
        Quat::new(w, qx, qy, qz).unwrap_or(Quat::IDENTITY)
    }

    /// `v` rotated by this quaternion.
    pub(crate) fn rotate(self, v: [f32; 3]) -> [f32; 3] {
        let p = Quat { w: 0.0, x: v[0], y: v[1], z: v[2] };
        let out = self.product(p).product(self.conjugate());
        [out.x, out.y, out.z]
    }

    /// Z-Y-X angles: yaw about z, then pitch about the new y, then roll
    /// about the new x. Pitch is clamped to ±90° at the poles.
    pub(crate) fn euler_degrees(self) -> EulerDegrees {
        let Quat { w, x, y, z } = self;
        let yaw = (2.0 * (w * z + x * y)).atan2(1.0 - 2.0 * (y * y + z * z));
        let pitch = (2.0 * (w * y - z * x)).clamp(-1.0, 1.0).asin();
        let roll = (2.0 * (w * x + y * z)).atan2(1.0 - 2.0 * (x * x + y * y));
        EulerDegrees { yaw: yaw.to_degrees(), pitch: pitch.to_degrees(), roll: roll.to_degrees() }
    }
}

/// The head frame at `reference`, taking head axes to world ones. The
/// body's z laid level is the side axis; a bud worn with it near vertical
/// falls back to its x laid level as forward.
fn head_frame(reference: Quat) -> Quat {
    const UP: [f32; 3] = [0.0, 0.0, 1.0];
    let level = |v: [f32; 3]| {
        let n = (v[0] * v[0] + v[1] * v[1]).sqrt();
        (n > 0.3).then(|| [v[0] / n, v[1] / n, 0.0])
    };
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
    };
    match level(reference.rotate([0.0, 0.0, 1.0])) {
        Some(side) => Quat::from_basis(cross(side, UP), side, UP),
        None => {
            let forward = level(reference.rotate([1.0, 0.0, 0.0])).unwrap_or([1.0, 0.0, 0.0]);
            Quat::from_basis(forward, cross(UP, forward), UP)
        }
    }
}

/// Acceleration in thousandths of g, as the buds report it. Not a fraction of
/// g: convert through [`MilliG::as_g`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MilliG(i32);

impl MilliG {
    pub fn new(n: i32) -> MilliG {
        MilliG(n)
    }

    pub(crate) fn get(self) -> i32 {
        self.0
    }

    #[cfg(test)]
    pub(crate) fn as_g(self) -> f32 {
        self.0 as f32 / 1000.0
    }

}

/// Gravity in the body frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Gravity {
    pub x: MilliG,
    pub y: MilliG,
    pub z: MilliG,
}

/// Angular rate as the buds report it, raw units per axis (the scale is not
/// published; gestures read it against thresholds measured off the same
/// units).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Gyro {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// One line of the stream.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub orientation: Quat,
    pub gravity: Gravity,
    pub gyro: Gyro,
    /// Wall-clock milliseconds the daemon stamped it with, 0 for none.
    pub at_ms: u64,
}

fn triple(raw: Option<&Json>) -> Option<Option<[i32; 3]>> {
    let Some(raw) = raw else { return Some(None) };
    let items = raw.as_array().filter(|a| a.len() == 3)?;
    let mut out = [0i32; 3];
    for (slot, item) in out.iter_mut().zip(items) {
        let n = item.as_f64().filter(|n| n.is_finite() && n.abs() < f64::from(i32::MAX))?;
        *slot = n.round() as i32;
    }
    Some(Some(out))
}

/// One `motion:watch` line, None for anything else (the daemon's `error:`
/// line included, which the caller tells apart first).
pub fn parse_sample(line: &str) -> Option<Sample> {
    let raw: Json = serde_json::from_str(line.trim()).ok()?;
    let q = raw.get("q")?.as_array().filter(|a| a.len() == 4)?;
    let mut c = [0f32; 4];
    for (slot, item) in c.iter_mut().zip(q) {
        *slot = item.as_f64()? as f32;
    }
    let orientation = Quat::new(c[0], c[1], c[2], c[3])?;
    let [gx, gy, gz] = triple(raw.get("g"))?.unwrap_or_default();
    let [rx, ry, rz] = triple(raw.get("gyro"))?.unwrap_or_default();
    Some(Sample {
        orientation,
        gravity: Gravity { x: MilliG(gx), y: MilliG(gy), z: MilliG(gz) },
        gyro: Gyro { x: rx, y: ry, z: rz },
        at_ms: raw.get("t").and_then(Json::as_u64).unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = r#"{"q":[0.51474,-0.445526,0.732483,-0.00421143],"g":[759,481,501],"gyro":[155,26,6],"t":1793898601508}"#;

    fn about(axis: [f32; 3], degrees: f32) -> Quat {
        let h = degrees.to_radians() / 2.0;
        Quat::new(h.cos(), axis[0] * h.sin(), axis[1] * h.sin(), axis[2] * h.sin()).unwrap()
    }

    fn about_x(degrees: f32) -> Quat {
        about([1.0, 0.0, 0.0], degrees)
    }

    fn about_y(degrees: f32) -> Quat {
        about([0.0, 1.0, 0.0], degrees)
    }

    fn about_z(degrees: f32) -> Quat {
        about([0.0, 0.0, 1.0], degrees)
    }

    fn near(a: f32, b: f32) {
        assert!((a - b).abs() < 0.01, "{a} vs {b}");
    }

    #[test]
    fn parses_a_captured_line() {
        let s = parse_sample(LINE).unwrap();
        let [w, x, y, z] = s.orientation.components();
        near(w, 0.51474);
        near(x, -0.445526);
        near(y, 0.732483);
        near(z, -0.00421143);
        assert_eq!(s.gravity.x.get(), 759);
        assert_eq!(s.gravity.z.as_g(), 0.501);
        assert_eq!(s.gyro, Gyro { x: 155, y: 26, z: 6 });
        assert_eq!(s.at_ms, 1_793_898_601_508);
    }

    #[test]
    fn gravity_agrees_with_the_quaternion() {
        let s = parse_sample(LINE).unwrap();
        let g = s.orientation.conjugate().rotate([0.0, 0.0, -1.0]);
        let measured = [s.gravity.x.as_g(), s.gravity.y.as_g(), s.gravity.z.as_g()];
        let norm = measured.iter().map(|n| n * n).sum::<f32>().sqrt();
        let cosine: f32 = g.iter().zip(measured).map(|(a, b)| a * b / norm).sum();
        assert!(cosine > 0.999, "{cosine}");
    }

    #[test]
    fn normalises_on_parse() {
        let s = parse_sample(r#"{"q":[2,0,0,0],"g":[0,0,1000],"gyro":[0,0,0],"t":1}"#).unwrap();
        assert_eq!(s.orientation, Quat::IDENTITY);
    }

    #[test]
    fn refuses_malformed_lines() {
        for line in [
            "",
            "error: motion unavailable",
            "{",
            r#"{"q":[1,0,0]}"#,
            r#"{"q":[0,0,0,0]}"#,
            r#"{"q":[1,0,0,"x"]}"#,
            r#"{"q":[1,0,0,0],"g":[1,2]}"#,
            r#"{"q":[1,0,0,0],"gyro":"no"}"#,
            r#"{"g":[1,2,3]}"#,
        ] {
            assert_eq!(parse_sample(line), None, "{line}");
        }
        let bare = parse_sample(r#"{"q":[1,0,0,0]}"#).unwrap();
        assert_eq!((bare.gravity, bare.gyro, bare.at_ms), (Gravity::default(), Gyro::default(), 0));
    }

    #[test]
    fn yaw_is_the_turn_about_world_z() {
        let e = about_z(40.0).euler_degrees();
        near(e.yaw, 40.0);
        near(e.pitch, 0.0);
        near(e.roll, 0.0);
        let e = about_z(-25.0).then(about_y(15.0)).euler_degrees();
        near(e.yaw, -25.0);
        near(e.pitch, 15.0);
        near(e.roll, 0.0);
    }

    #[test]
    fn relative_to_removes_the_reference() {
        let reference = about_z(100.0);
        let now = about_z(140.0).then(about_y(10.0));
        let e = now.relative_to(reference).euler_degrees();
        near(e.yaw, 40.0);
        near(e.pitch, 10.0);
        near(e.roll, 0.0);
        let e = now.relative_to(now).euler_degrees();
        near(e.yaw, 0.0);
        near(e.pitch, 0.0);
        near(e.roll, 0.0);
    }

    /// Worn as measured: the body's y up and its z out of the ear (world y),
    /// facing an arbitrary heading. A head turn and a nod are world
    /// rotations applied over the reference, and read as yaw and pitch.
    #[test]
    fn a_tilted_bud_reads_head_turns_and_nods() {
        let worn = about_z(70.0).then(about_x(90.0));
        let up = worn.rotate([0.0, 1.0, 0.0]);
        near(up[2], 1.0);
        let e = about_z(40.0).then(worn).relative_to(worn).euler_degrees();
        near(e.yaw, 40.0);
        near(e.pitch, 0.0);
        near(e.roll, 0.0);
        let nod = about(worn.rotate([0.0, 0.0, 1.0]), 15.0);
        let e = nod.then(worn).relative_to(worn).euler_degrees();
        near(e.yaw, 0.0);
        near(e.pitch, 15.0);
        near(e.roll, 0.0);
    }

    #[test]
    fn rotate_turns_a_vector() {
        let v = about_z(90.0).rotate([1.0, 0.0, 0.0]);
        near(v[0], 0.0);
        near(v[1], 1.0);
        near(v[2], 0.0);
    }
}
