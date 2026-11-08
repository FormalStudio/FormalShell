//! A colour as the renderer takes it, and the two pieces of Qt colour
//! arithmetic the chrome tables lean on: `Qt.alpha` and `Qt.tint`.

/// Straight (not premultiplied) RGBA, each channel 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const TRANSPARENT: Self = Self { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
    pub const BLACK: Self = Self { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const WHITE: Self = Self { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };

    pub const fn hex(rgb: u32) -> Self {
        Self {
            r: ((rgb >> 16) & 0xff) as f32 / 255.0,
            g: ((rgb >> 8) & 0xff) as f32 / 255.0,
            b: (rgb & 0xff) as f32 / 255.0,
            a: 1.0,
        }
    }

    /// `#rrggbb` or `#rrggbbaa` (alpha last, the CSS order), or the word
    /// `transparent`. Anything else is `None`.
    pub fn parse(text: &str) -> Option<Self> {
        if text == "transparent" {
            return Some(Self::TRANSPARENT);
        }
        let digits = text.strip_prefix('#')?;
        if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let value = u32::from_str_radix(digits, 16).ok()?;
        match digits.len() {
            6 => Some(Self::hex(value)),
            8 => Some(Self { a: (value & 0xff) as f32 / 255.0, ..Self::hex(value >> 8) }),
            _ => None,
        }
    }

    /// `Qt.alpha(c, a)`: the same colour with its alpha replaced, not
    /// multiplied.
    pub fn with_alpha(self, alpha: f32) -> Self {
        Self { a: alpha.clamp(0.0, 1.0), ..self }
    }

    /// `Qt.tint(self, over)`: `over` blended on top by its own alpha, the
    /// result as opaque as the two stacked.
    pub fn tint(self, over: Self) -> Self {
        if over.a >= 1.0 {
            return over;
        }
        if over.a <= 0.0 {
            return self;
        }
        let inv = 1.0 - over.a;
        Self {
            r: over.r * over.a + self.r * inv,
            g: over.g * over.a + self.g * inv,
            b: over.b * over.a + self.b * inv,
            a: over.a + inv * self.a,
        }
    }

    /// `t` of the way from `self` to `to`, through premultiplied channels:
    /// a straight lerp out of `transparent` (black at alpha 0) drags the
    /// colour through a dark midpoint, where this fades the far colour in
    /// over whatever is behind.
    pub fn mix(self, to: Self, t: f32) -> Self {
        let a = self.a + (to.a - self.a) * t;
        if a <= 0.0 {
            return Self::TRANSPARENT;
        }
        let ch = |x: f32, y: f32| ((x * self.a + (y * to.a - x * self.a) * t) / a).clamp(0.0, 1.0);
        Self { r: ch(self.r, to.r), g: ch(self.g, to.g), b: ch(self.b, to.b), a }
    }

    /// sRGB to linear light for one channel in 0..1.
    pub fn linear(v: f64) -> f64 {
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    }

    /// WCAG's relative luminance.
    pub fn luminance(self) -> f64 {
        0.2126 * Self::linear(f64::from(self.r)) + 0.7152 * Self::linear(f64::from(self.g)) + 0.0722 * Self::linear(f64::from(self.b))
    }

    /// WCAG's contrast ratio between two relative luminances.
    pub fn contrast(a: f64, b: f64) -> f64 {
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// Rec. 601 luma of the colour, scaled to 0..255.
    pub fn luma601(self) -> f64 {
        255.0 * (0.299 * f64::from(self.r) + 0.587 * f64::from(self.g) + 0.114 * f64::from(self.b))
    }

    /// `#rrggbb`, alpha dropped.
    pub fn to_hex(self) -> String {
        let [r, g, b, _] = self.to_u8();
        format!("#{r:02x}{g:02x}{b:02x}")
    }

    /// Channels as bytes, rounded the way Qt rounds a float channel.
    pub fn to_u8(self) -> [u8; 4] {
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        [byte(self.r), byte(self.g), byte(self.b), byte(self.a)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_and_transparent() {
        assert_eq!(Rgba::parse("#ff0000"), Some(Rgba { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }));
        assert_eq!(Rgba::parse("#00000080").map(|c| c.to_u8()), Some([0, 0, 0, 128]));
        assert_eq!(Rgba::parse("transparent"), Some(Rgba::TRANSPARENT));
        assert_eq!(Rgba::parse("ff0000"), None);
        assert_eq!(Rgba::parse("#ff00"), None);
        assert_eq!(Rgba::parse("#gg0000"), None);
    }

    #[test]
    fn mix_out_of_transparent_never_darkens() {
        let accent = Rgba::hex(0xd0e0f0);
        for i in 0..=10 {
            let m = Rgba::TRANSPARENT.mix(accent, i as f32 / 10.0);
            if m.a > 0.0 {
                assert!((m.r - accent.r).abs() < 1e-5 && (m.b - accent.b).abs() < 1e-5, "{m:?}");
            }
            let back = accent.mix(Rgba::TRANSPARENT, i as f32 / 10.0);
            if back.a > 0.0 {
                assert!((back.g - accent.g).abs() < 1e-5, "{back:?}");
            }
        }
        let a = Rgba::hex(0x202020);
        let b = Rgba::hex(0xe0e0e0);
        assert_eq!(a.mix(b, 0.5).to_u8(), [128, 128, 128, 255]);
    }

    #[test]
    fn alpha_replaces_rather_than_multiplies() {
        let half = Rgba::hex(0x18181b).with_alpha(0.5);
        assert_eq!(half.with_alpha(0.85).a, 0.85);
    }

    #[test]
    fn tint_blends_over_and_keeps_an_opaque_base_opaque() {
        let base = Rgba::hex(0xe4e4e7);
        let over = Rgba::hex(0x09090b).with_alpha(0.1);
        let out = base.tint(over);
        assert_eq!(out.a, 1.0);
        assert!((out.r - (0x09 as f32 / 255.0 * 0.1 + 0xe4 as f32 / 255.0 * 0.9)).abs() < 1e-6);
        assert_eq!(base.tint(Rgba::WHITE), Rgba::WHITE);
        assert_eq!(base.tint(Rgba::TRANSPARENT), base);
    }
}
