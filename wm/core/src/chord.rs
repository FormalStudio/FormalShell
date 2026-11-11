//! Key chords as the settings write them and the socket lists them:
//! `SUPER+SHIFT+code:10`. The modifiers are `SUPER`, `CTRL`, `ALT`, `SHIFT`;
//! the key is an xkb keysym name, `code:<n>` for a keycode, `mouse:<n>` for
//! a pointer button, or `mouse_up` and `mouse_down` for the wheel.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub logo: bool,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Sym(String),
    Code(u32),
    /// A Linux button code, `BTN_LEFT` is 272.
    Button(u32),
    /// Hyprland's `mouse_up`: the wheel turned toward the user.
    WheelUp,
    WheelDown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chord {
    pub mods: Mods,
    pub key: Key,
}

impl Chord {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut mods = Mods::default();
        let mut key = None;
        for part in text.split('+').map(str::trim) {
            if part.is_empty() {
                return Err(format!("chord `{text}` has an empty part"));
            }
            let slot = match part.to_ascii_uppercase().as_str() {
                "SUPER" | "MOD" | "LOGO" => Some(&mut mods.logo),
                "SHIFT" => Some(&mut mods.shift),
                "CTRL" | "CONTROL" => Some(&mut mods.ctrl),
                "ALT" => Some(&mut mods.alt),
                _ => None,
            };
            if let Some(slot) = slot {
                if std::mem::replace(slot, true) {
                    return Err(format!("chord `{text}` names `{part}` twice"));
                }
                continue;
            }
            if key.is_some() {
                return Err(format!("chord `{text}` has two keys"));
            }
            key = Some(match part {
                "mouse_up" => Key::WheelUp,
                "mouse_down" => Key::WheelDown,
                _ => match part.split_once(':') {
                    Some(("code", code)) => Key::Code(
                        code.parse()
                            .map_err(|_| format!("chord `{text}`: bad keycode `{code}`"))?,
                    ),
                    Some(("mouse", button)) => Key::Button(
                        button
                            .parse()
                            .map_err(|_| format!("chord `{text}`: bad button `{button}`"))?,
                    ),
                    Some(_) => return Err(format!("chord `{text}`: unknown key `{part}`")),
                    None => Key::Sym(part.to_owned()),
                },
            });
        }
        match key {
            Some(key) => Ok(Self { mods, key }),
            None => Err(format!("chord `{text}` has no key")),
        }
    }

    /// The modifiers as the socket lists them.
    pub fn mod_names(&self) -> Vec<String> {
        [
            (self.mods.logo, "SUPER"),
            (self.mods.ctrl, "CTRL"),
            (self.mods.alt, "ALT"),
            (self.mods.shift, "SHIFT"),
        ]
        .into_iter()
        .filter(|(held, _)| *held)
        .map(|(_, name)| name.to_owned())
        .collect()
    }

    pub fn key_name(&self) -> String {
        match &self.key {
            Key::Sym(name) => name.clone(),
            Key::Code(code) => format!("code:{code}"),
            Key::Button(button) => format!("mouse:{button}"),
            Key::WheelUp => "mouse_up".to_owned(),
            Key::WheelDown => "mouse_down".to_owned(),
        }
    }

    /// `mods` and `key` joined with `+`.
    pub fn canonical(&self) -> String {
        let mut parts = self.mod_names();
        parts.push(self.key_name());
        parts.join("+")
    }

    /// The modifier keysym a chord releases when its key is that modifier
    /// itself (`ALT+Alt_L`): the class of the modifier.
    pub fn modifier_key(&self) -> Option<&'static str> {
        let Key::Sym(name) = &self.key else {
            return None;
        };
        match name.as_str() {
            "Alt_L" | "Alt_R" => Some("Alt"),
            "Super_L" | "Super_R" => Some("Super"),
            "Shift_L" | "Shift_R" => Some("Shift"),
            "Control_L" | "Control_R" => Some("Ctrl"),
            _ => None,
        }
    }

    /// ShojiWM's shortcut for the chord. A pointer button has none: the
    /// core binds drags by modifier, not through shortcuts. A keycode is
    /// xkb's on both sides.
    pub fn shortcut(&self) -> Option<String> {
        let key = match &self.key {
            Key::Sym(name) => name.clone(),
            Key::Code(code) => format!("code:{code}"),
            Key::WheelUp => "WheelScrollUp".to_owned(),
            Key::WheelDown => "WheelScrollDown".to_owned(),
            Key::Button(_) => return None,
        };
        let mut parts: Vec<&str> = [
            (self.mods.logo, "Super"),
            (self.mods.shift, "Shift"),
            (self.mods.ctrl, "Ctrl"),
            (self.mods.alt, "Alt"),
        ]
        .into_iter()
        .filter(|(held, _)| *held)
        .map(|(_, name)| name)
        .collect();
        parts.push(&key);
        Some(parts.join("+"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_number_row_by_keycode() {
        let chord = Chord::parse("SUPER + SHIFT + code:10").unwrap();
        assert_eq!(chord.key, Key::Code(10));
        assert_eq!(chord.mod_names(), ["SUPER", "SHIFT"]);
        assert_eq!(chord.canonical(), "SUPER+SHIFT+code:10");
        assert_eq!(chord.shortcut().unwrap(), "Super+Shift+code:10");
    }

    #[test]
    fn modifiers_list_in_the_wire_order_whatever_the_file_says() {
        let chord = Chord::parse("alt+Shift+super+Tab").unwrap();
        assert_eq!(chord.canonical(), "SUPER+ALT+SHIFT+Tab");
    }

    #[test]
    fn keysyms_keep_their_spelling() {
        assert_eq!(
            Chord::parse("XF86AudioRaiseVolume").unwrap().canonical(),
            "XF86AudioRaiseVolume"
        );
        assert_eq!(
            Chord::parse("SUPER+ntilde").unwrap().shortcut().unwrap(),
            "Super+ntilde"
        );
        assert_eq!(
            Chord::parse("SUPER+plus").unwrap().key,
            Key::Sym("plus".into())
        );
    }

    #[test]
    fn the_wheel_and_the_buttons() {
        assert_eq!(
            Chord::parse("SUPER+mouse_down")
                .unwrap()
                .shortcut()
                .unwrap(),
            "Super+WheelScrollDown"
        );
        let drag = Chord::parse("SUPER+mouse:272").unwrap();
        assert_eq!(drag.key, Key::Button(272));
        assert_eq!(drag.shortcut(), None);
        assert_eq!(drag.canonical(), "SUPER+mouse:272");
    }

    #[test]
    fn a_modifier_as_the_key() {
        let chord = Chord::parse("ALT+Alt_L").unwrap();
        assert_eq!(chord.modifier_key(), Some("Alt"));
        assert_eq!(Chord::parse("ALT+Tab").unwrap().modifier_key(), None);
    }

    #[test]
    fn what_is_refused() {
        for bad in [
            "",
            "SUPER",
            "SUPER+SUPER+a",
            "SUPER+a+b",
            "SUPER++a",
            "code:x",
            "foo:1",
        ] {
            assert!(Chord::parse(bad).is_err(), "{bad:?} should not parse");
        }
    }
}
