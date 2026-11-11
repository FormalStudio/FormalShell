//! Colours as the theme writes them: "#rrggbb", "#rrggbbaa", Hyprland's
//! "rgb(rrggbb)" and "rgba(rrggbbaa)", or a role of the theme's colours.

use fs_wm_proto::Colors;

/// Straight (not premultiplied) RGBA, each 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba(pub [f64; 4]);

impl Rgba {
    pub const BLACK: Rgba = Rgba([0.0, 0.0, 0.0, 1.0]);

    pub fn parse(text: &str) -> Option<Rgba> {
        let text = text.trim();
        let hex = text
            .strip_prefix('#')
            .or_else(|| {
                text.strip_prefix("rgba(")
                    .or_else(|| text.strip_prefix("rgb("))
                    .and_then(|rest| rest.strip_suffix(')'))
            })
            .or_else(|| text.strip_prefix("0x"))?;
        if !hex.is_ascii() || !matches!(hex.len(), 6 | 8) {
            return None;
        }
        let byte = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
        let alpha = if hex.len() == 8 { byte(6)? } else { 255 };
        Some(Rgba([
            byte(0)? as f64 / 255.0,
            byte(2)? as f64 / 255.0,
            byte(4)? as f64 / 255.0,
            alpha as f64 / 255.0,
        ]))
    }

    /// A colour string, or the name of one of the theme's roles.
    pub fn resolve(text: &str, colors: &Colors) -> Option<Rgba> {
        let role = match text {
            "primary" => &colors.primary,
            "primaryForeground" => &colors.primary_foreground,
            "background" => &colors.background,
            "foreground" => &colors.foreground,
            "border" => &colors.border,
            "destructive" => &colors.destructive,
            "warning" => &colors.warning,
            other => other,
        };
        Rgba::parse(role)
    }

    /// "#rrggbb", or "#rrggbbaa" when not opaque.
    pub fn hex(&self) -> String {
        let [r, g, b, a] = self
            .0
            .map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8);
        if a == 255 {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        }
    }

    pub fn mix(&self, to: &Rgba, t: f64) -> Rgba {
        let mut out = [0.0; 4];
        for (index, channel) in out.iter_mut().enumerate() {
            *channel = self.0[index] + (to.0[index] - self.0[index]) * t;
        }
        Rgba(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_spelling_the_theme_uses() {
        assert_eq!(Rgba::parse("#ff0000"), Some(Rgba([1.0, 0.0, 0.0, 1.0])));
        assert_eq!(Rgba::parse("#000000ed").unwrap().hex(), "#000000ed");
        assert_eq!(Rgba::parse("rgba(205e4fff)").unwrap().hex(), "#205e4f");
        assert_eq!(Rgba::parse("rgb(205e4f)").unwrap().hex(), "#205e4f");
        assert_eq!(Rgba::parse("primary"), None);
        assert_eq!(Rgba::parse("#12345"), None);
        assert_eq!(Rgba::parse("#ññññññ"), None);
    }

    #[test]
    fn a_role_resolves_through_the_theme() {
        let colors = Colors::default();
        assert_eq!(
            Rgba::resolve("primary", &colors),
            Rgba::parse(&colors.primary)
        );
        assert_eq!(
            Rgba::resolve("#01020304", &colors).unwrap().hex(),
            "#01020304"
        );
        assert_eq!(Rgba::resolve("nope", &colors), None);
    }
}
