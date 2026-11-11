//! Window rules: matchers on what a window is when it opens, to where it
//! goes and how it is treated. Every matching rule applies, in order, a
//! later one winning where two set the same thing.

use regex::Regex;

use crate::geom::Rect;

/// A pattern that has to match the whole string, as Hyprland's do.
#[derive(Clone, Debug)]
pub struct Pattern(Regex);

impl Pattern {
    pub fn new(pattern: &str) -> Result<Self, String> {
        Regex::new(&format!("^(?:{pattern})$"))
            .map(Self)
            .map_err(|error| format!("bad pattern `{pattern}`: {error}"))
    }

    pub fn matches(&self, text: &str) -> bool {
        self.0.is_match(text)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Matcher {
    pub app_id: Option<Pattern>,
    pub title: Option<Pattern>,
    pub xwayland: Option<bool>,
    /// The xdg-toplevel-tag.
    pub tag: Option<Pattern>,
    pub floating: Option<bool>,
}

/// A length in pixels or as a share of the output.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Px(f64),
    Share(f64),
}

impl Length {
    /// `120` or `"28%"`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        let (number, share) = match text.strip_suffix('%') {
            Some(number) => (number, true),
            None => (text, false),
        };
        let value: f64 = number
            .trim()
            .parse()
            .map_err(|_| format!("bad length `{text}`"))?;
        Ok(if share {
            Length::Share(value / 100.0)
        } else {
            Length::Px(value)
        })
    }

    fn of(self, whole: f64) -> f64 {
        match self {
            Length::Px(px) => px,
            Length::Share(share) => whole * share,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Center,
}

impl Anchor {
    pub fn parse(text: &str) -> Result<Self, String> {
        Ok(match text {
            "top-left" => Anchor::TopLeft,
            "top-right" => Anchor::TopRight,
            "bottom-left" => Anchor::BottomLeft,
            "bottom-right" => Anchor::BottomRight,
            "center" => Anchor::Center,
            other => return Err(format!("unknown anchor `{other}`")),
        })
    }
}

/// Where a float goes on its output: against a corner at an inset, or in
/// the middle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Position {
    pub anchor: Anchor,
    pub inset: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Effect {
    /// The number of a named workspace, from 1.
    pub workspace: Option<usize>,
    pub overlay: Option<String>,
    pub float: Option<bool>,
    pub pin: Option<bool>,
    pub size: Option<(Length, Length)>,
    pub position: Option<Position>,
    pub opacity: Option<f64>,
    pub no_blur: Option<bool>,
    /// The content type it is treated as (`game` turns VRR on).
    pub content: Option<String>,
    pub confine_pointer: Option<bool>,
    /// Seconds after which an idle pointer hides while it has the focus.
    pub idle_pointer: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct Rule {
    pub matcher: Matcher,
    pub effect: Effect,
}

/// What a window is when it opens.
#[derive(Clone, Copy, Debug, Default)]
pub struct Facts<'a> {
    pub app_id: &'a str,
    pub title: &'a str,
    pub xwayland: bool,
    pub tag: &'a str,
    /// It asked to float: a dialog, a fixed-size window.
    pub floating: bool,
}

impl Matcher {
    fn matches(&self, facts: &Facts<'_>, floating: bool) -> bool {
        self.app_id.as_ref().is_none_or(|p| p.matches(facts.app_id))
            && self.title.as_ref().is_none_or(|p| p.matches(facts.title))
            && self.tag.as_ref().is_none_or(|p| p.matches(facts.tag))
            && self.xwayland.is_none_or(|x| x == facts.xwayland)
            && self.floating.is_none_or(|f| f == floating)
    }
}

/// The rules' sum for one window. `float` is always decided: the window's
/// own wish stands where no rule speaks.
pub fn resolve(rules: &[Rule], facts: &Facts<'_>) -> Effect {
    let mut sum = Effect {
        float: Some(facts.floating),
        ..Effect::default()
    };
    for rule in rules {
        if !rule.matcher.matches(facts, sum.float == Some(true)) {
            continue;
        }
        let effect = &rule.effect;
        macro_rules! take {
            ($($field:ident),*) => {
                $(if effect.$field.is_some() {
                    sum.$field = effect.$field.clone();
                })*
            };
        }
        take!(
            workspace,
            overlay,
            float,
            pin,
            size,
            position,
            opacity,
            no_blur,
            content,
            confine_pointer,
            idle_pointer
        );
    }
    sum
}

/// The rect a rule gives a float, relative to its output's origin: its size
/// (the window's own where the rule has none) at its position (centred
/// where the rule has none). None when the rule says neither.
pub fn float_rect(
    effect: &Effect,
    output: (f64, f64),
    natural: Option<(f64, f64)>,
) -> Option<Rect> {
    if effect.size.is_none() && effect.position.is_none() {
        return None;
    }
    let (ow, oh) = output;
    let (w, h) = match effect.size {
        Some((w, h)) => (w.of(ow), h.of(oh)),
        None => natural.unwrap_or((ow * 0.5, oh * 0.6)),
    };
    let position = effect.position.unwrap_or(Position {
        anchor: Anchor::Center,
        inset: 0.0,
    });
    let inset = position.inset;
    let (x, y) = match position.anchor {
        Anchor::TopLeft => (inset, inset),
        Anchor::TopRight => (ow - w - inset, inset),
        Anchor::BottomLeft => (inset, oh - h - inset),
        Anchor::BottomRight => (ow - w - inset, oh - h - inset),
        Anchor::Center => ((ow - w) / 2.0, (oh - h) / 2.0),
    };
    Some(Rect::new(x.round(), y.round(), w.round(), h.round()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(app_id: Option<&str>, title: Option<&str>, effect: Effect) -> Rule {
        Rule {
            matcher: Matcher {
                app_id: app_id.map(|p| Pattern::new(p).unwrap()),
                title: title.map(|p| Pattern::new(p).unwrap()),
                ..Matcher::default()
            },
            effect,
        }
    }

    fn facts<'a>(app_id: &'a str, title: &'a str) -> Facts<'a> {
        Facts {
            app_id,
            title,
            ..Facts::default()
        }
    }

    #[test]
    fn a_pattern_has_to_match_the_whole_string() {
        let pattern = Pattern::new("[Hh]elium").unwrap();
        assert!(pattern.matches("helium"));
        assert!(pattern.matches("Helium"));
        assert!(!pattern.matches("helium-browser"));
        assert!(
            Pattern::new("^(steam_app_[0-9]+)$")
                .unwrap()
                .matches("steam_app_730")
        );
        assert!(Pattern::new("^$").unwrap().matches(""));
        assert!(Pattern::new("(").is_err());
    }

    #[test]
    fn later_rules_win_and_unset_fields_stand() {
        let rules = [
            rule(
                Some("[Bb]eeper"),
                None,
                Effect {
                    workspace: Some(4),
                    ..Effect::default()
                },
            ),
            rule(
                Some("[Bb]eeper"),
                None,
                Effect {
                    float: Some(false),
                    ..Effect::default()
                },
            ),
        ];
        let sum = resolve(
            &rules,
            &Facts {
                floating: true,
                ..facts("Beeper", "Beeper")
            },
        );
        assert_eq!(sum.workspace, Some(4));
        assert_eq!(sum.float, Some(false));
    }

    #[test]
    fn a_floating_matcher_sees_what_earlier_rules_decided() {
        let pin = Effect {
            pin: Some(true),
            ..Effect::default()
        };
        let mut pinned = rule(Some("helium"), None, pin);
        pinned.matcher.floating = Some(true);
        let rules = [pinned];
        assert_eq!(resolve(&rules, &facts("helium", "Page")).pin, None);
        let popup = Facts {
            floating: true,
            ..facts("helium", "Meet")
        };
        assert_eq!(resolve(&rules, &popup).pin, Some(true));
    }

    #[test]
    fn empty_app_id_and_title_tell_the_notification_from_the_pip() {
        let pip = rule(
            None,
            Some("[Pp]icture[ -][Ii]n[ -][Pp]icture"),
            Effect {
                float: Some(true),
                position: Some(Position {
                    anchor: Anchor::BottomRight,
                    inset: 16.0,
                }),
                ..Effect::default()
            },
        );
        let notification = rule(
            Some(""),
            Some(""),
            Effect {
                float: Some(true),
                position: Some(Position {
                    anchor: Anchor::TopRight,
                    inset: 16.0,
                }),
                ..Effect::default()
            },
        );
        let rules = [pip, notification];
        let sum = resolve(&rules, &facts("", "Picture in picture"));
        assert_eq!(sum.position.unwrap().anchor, Anchor::BottomRight);
        let sum = resolve(&rules, &facts("", ""));
        assert_eq!(sum.position.unwrap().anchor, Anchor::TopRight);
        assert_eq!(resolve(&rules, &facts("ghostty", "")).position, None);
    }

    #[test]
    fn tag_and_xwayland_matchers() {
        let mut atoll = rule(
            None,
            None,
            Effect {
                float: Some(true),
                ..Effect::default()
            },
        );
        atoll.matcher.tag = Some(Pattern::new("^atoll$").unwrap());
        let mut popover = rule(
            Some("nd-hello"),
            Some("nd-hello"),
            Effect {
                no_blur: Some(true),
                ..Effect::default()
            },
        );
        popover.matcher.xwayland = Some(true);
        popover.matcher.floating = Some(true);
        let rules = [atoll, popover];
        assert_eq!(
            resolve(&rules, &facts("com.apple.TextEdit", "x")).float,
            Some(false)
        );
        let tagged = Facts {
            tag: "atoll",
            ..facts("com.apple.TextEdit", "x")
        };
        assert_eq!(resolve(&rules, &tagged).float, Some(true));
        let main = Facts {
            xwayland: true,
            ..facts("nd-hello", "Page")
        };
        assert_eq!(resolve(&rules, &main).no_blur, None);
        let popup = Facts {
            xwayland: true,
            floating: true,
            ..facts("nd-hello", "nd-hello")
        };
        assert_eq!(resolve(&rules, &popup).no_blur, Some(true));
    }

    #[test]
    fn a_share_of_the_output_tucked_into_a_corner() {
        let effect = Effect {
            size: Some((Length::parse("28%").unwrap(), Length::parse("28%").unwrap())),
            position: Some(Position {
                anchor: Anchor::BottomRight,
                inset: 16.0,
            }),
            ..Effect::default()
        };
        let rect = float_rect(&effect, (2560.0, 1440.0), Some((1240.0, 700.0))).unwrap();
        assert_eq!(rect, Rect::new(1827.0, 1021.0, 717.0, 403.0));
    }

    #[test]
    fn a_position_alone_keeps_the_windows_own_size() {
        let effect = Effect {
            position: Some(Position {
                anchor: Anchor::TopRight,
                inset: 16.0,
            }),
            ..Effect::default()
        };
        let rect = float_rect(&effect, (1920.0, 1080.0), Some((360.0, 90.0))).unwrap();
        assert_eq!(rect, Rect::new(1544.0, 16.0, 360.0, 90.0));
        assert_eq!(float_rect(&Effect::default(), (1920.0, 1080.0), None), None);
    }
}
