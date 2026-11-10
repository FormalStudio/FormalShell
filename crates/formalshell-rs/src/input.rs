//! Input as the surfaces read it: keysyms to the keys each consumer
//! understands, the printable-text rule, and the evdev button codes.

use smithay_client_toolkit::seat::keyboard::{Keysym, Modifiers};

use crate::ui::Button;
use crate::surfaces::launcher::keys::Key as LauncherKey;
use crate::surfaces::panel::host::Key as PanelKey;
use crate::ui::field::{self, Modifiers as Mods};

pub const BTN_LEFT: u32 = 0x110;
pub const BTN_RIGHT: u32 = 0x111;
pub const BTN_MIDDLE: u32 = 0x112;

/// A bar cell's button for an evdev code; anything unknown is the left one.
pub fn button(code: u32) -> Button {
    match code {
        BTN_RIGHT => Button::Right,
        BTN_MIDDLE => Button::Middle,
        _ => Button::Left,
    }
}

pub fn modifiers(m: Modifiers) -> Mods {
    Mods { ctrl: m.ctrl, shift: m.shift, alt: m.alt }
}

/// What a key types: not empty, no control characters and not DEL.
pub fn printable(utf8: Option<&str>) -> Option<&str> {
    utf8.filter(|t| !t.is_empty() && t.chars().all(|c| c as u32 >= 0x20 && c != '\u{7f}'))
}

/// A key as a text field reads it.
pub fn field_key(sym: Keysym) -> field::Key {
    use field::Key as F;
    match sym {
        Keysym::Left | Keysym::KP_Left => F::Left,
        Keysym::Right | Keysym::KP_Right => F::Right,
        Keysym::Home | Keysym::KP_Home => F::Home,
        Keysym::End | Keysym::KP_End => F::End,
        Keysym::BackSpace => F::Backspace,
        Keysym::Delete | Keysym::KP_Delete => F::Delete,
        sym => sym.key_char().filter(char::is_ascii_alphabetic).map_or(F::Other, |c| F::Letter(c.to_ascii_lowercase())),
    }
}

/// A key as the launcher's results read it. `shift` is the modifier state,
/// since Tab with Shift is the way back whether or not the layout sends
/// ISO_Left_Tab.
pub fn launcher_key(sym: Keysym, shift: bool) -> LauncherKey {
    match sym {
        Keysym::Up => LauncherKey::Up,
        Keysym::Down => LauncherKey::Down,
        Keysym::Left => LauncherKey::Left,
        Keysym::Right => LauncherKey::Right,
        Keysym::Page_Up => LauncherKey::PageUp,
        Keysym::Page_Down => LauncherKey::PageDown,
        Keysym::Home => LauncherKey::Home,
        Keysym::End => LauncherKey::End,
        Keysym::Return => LauncherKey::Return,
        Keysym::KP_Enter => LauncherKey::Enter,
        Keysym::Escape => LauncherKey::Escape,
        Keysym::BackSpace => LauncherKey::Backspace,
        Keysym::Tab if shift => LauncherKey::Backtab,
        Keysym::Tab => LauncherKey::Tab,
        Keysym::ISO_Left_Tab => LauncherKey::Backtab,
        _ => LauncherKey::Other,
    }
}

/// A key as a panel's cursor model reads it, None for one it ignores. A held
/// Return or Space activates once: a confirm that arms on the first press
/// must not fire on the repeat.
pub fn panel_key(sym: Keysym, utf8: Option<String>, repeat: bool, shift: bool) -> Option<PanelKey> {
    Some(match sym {
        Keysym::BackSpace => PanelKey::Back,
        Keysym::Escape => PanelKey::Escape,
        Keysym::Tab if shift => PanelKey::Tab(-1),
        Keysym::Tab => PanelKey::Tab(1),
        Keysym::ISO_Left_Tab => PanelKey::Tab(-1),
        Keysym::Down => PanelKey::Move(0, 1),
        Keysym::Up => PanelKey::Move(0, -1),
        Keysym::Right => PanelKey::Move(1, 0),
        Keysym::Left => PanelKey::Move(-1, 0),
        Keysym::Return | Keysym::KP_Enter | Keysym::space if repeat => return None,
        Keysym::Return | Keysym::KP_Enter | Keysym::space => PanelKey::Activate,
        _ => match utf8 {
            Some(t) if t.chars().count() == 1 && printable(Some(&t)).is_some() => PanelKey::Text(t),
            _ => return None,
        },
    })
}
