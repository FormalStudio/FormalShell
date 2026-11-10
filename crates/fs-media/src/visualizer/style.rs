//! The visualizer's style ids, in their cycle order. The renderer maps each
//! `Style` to its draw function with an exhaustive match, so a style added
//! here does not compile until it can be drawn.

macro_rules! styles {
    ($($variant:ident $id:literal $label:literal,)*) => {
        /// A style the table names, resolved from its id once at the boundary.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Style {
            $($variant,)*
        }

        impl Style {
            pub const ALL: &'static [Style] = &[$(Style::$variant,)*];

            pub fn id(self) -> &'static str {
                match self {
                    $(Style::$variant => $id,)*
                }
            }

            pub fn label(self) -> &'static str {
                match self {
                    $(Style::$variant => $label,)*
                }
            }
        }
    };
}

styles! {
    Bars "bars" "Bars",
    Peaks "peaks" "Peaks",
    Led "led" "LED",
    Mirror "mirror" "Mirror",
    Butterfly "butterfly" "Butterfly",
    Outline "outline" "Outline",
    Wave "wave" "Wave",
    Dots "dots" "Dots",
    Ascii "ascii" "ASCII",
    Matrix "matrix" "Matrix",
    Rain "rain" "Rain",
    Flame "flame" "Flame",
    Bubbles "bubbles" "Bubbles",
    Scope "scope" "Scope",
    Pulse "pulse" "Pulse",
    Heartbeat "heartbeat" "Heartbeat",
    Terrain "terrain" "Terrain",
    Bricks "bricks" "Bricks",
    Columns "columns" "Columns",
    Scatter "scatter" "Scatter",
    Retro "retro" "Retro",
    Binary "binary" "Binary",
    Sakura "sakura" "Sakura",
    Firework "firework" "Firework",
    Firefly "firefly" "Firefly",
    Mosaic "mosaic" "Mosaic",
    Sand "sand" "Sand",
    Geyser "geyser" "Geyser",
    Stereo "stereo" "Stereo",
    Redsector "redsector" "Red Sector",
    Stipple "stipple" "Stipple",
}

impl Style {
    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|s| s.id() == id)
    }

    /// `delta` steps through the cycle order and wraps.
    pub fn step(self, delta: i64) -> Self {
        let n = Self::ALL.len() as i64;
        Self::ALL[(self as i64 + delta).rem_euclid(n) as usize]
    }

    pub fn ids() -> Vec<&'static str> {
        Self::ALL.iter().map(|s| s.id()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::Style;

    #[test]
    fn ids_round_trip_and_step_wraps() {
        let mut seen = std::collections::HashSet::new();
        for &s in Style::ALL {
            assert!(seen.insert(s.id()), "duplicate id {}", s.id());
            assert!(!s.label().is_empty());
            assert_eq!(Style::parse(s.id()), Some(s));
        }
        assert_eq!(Style::Bars.step(-1), Style::Stipple);
        assert_eq!(Style::Stipple.step(1), Style::Bars);
        assert_eq!(Style::Bars.step(2), Style::Led);
        assert_eq!(Style::parse("nope"), None);
    }
}
