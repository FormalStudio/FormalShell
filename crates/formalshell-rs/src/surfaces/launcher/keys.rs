//! What a key press means to the launcher: which keys belong to the results
//! and which to the search field. Pure functions over the surface's shape.

use crate::ui::field::Modifiers;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    Return,
    Enter,
    Escape,
    Backspace,
    Tab,
    Backtab,
    /// A printable or otherwise unhandled key: always the field's own.
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyMode {
    Menu,
    Select,
    Input,
}

/// What the surface looks like to a key press.
#[derive(Clone, Copy, Debug)]
pub struct KeyCtx {
    pub mode: KeyMode,
    /// The search field holds text.
    pub query: bool,
    /// The level's view is a grid, so Left/Right move between cells.
    pub grid: bool,
    /// An app view owns the body (it scrolls rather than moving a row).
    pub app_view: bool,
    /// That app view declared something to scroll.
    pub scrollable: bool,
    /// The level's tiles are laid out in sections Ctrl+Tab steps through.
    pub sections: bool,
    /// Standing inside a mode rather than at the root.
    pub level: bool,
    /// The actions column is open and has the keys.
    pub column: bool,
    /// → would step into the selected subject: a row list, the caret at
    /// the query's end, a subject with children.
    pub into: bool,
    /// The camera mirror is the view: Tab and Enter step to the next camera.
    pub mirror: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    Pass,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    ScrollUp,
    ScrollDown,
    ScrollPageUp,
    ScrollPageDown,
    ScrollHome,
    ScrollEnd,
    Submit,
    Activate,
    ActivateAlternate,
    Close,
    Clear,
    Pop,
    Swallow,
    /// Ctrl+Tab and Ctrl+Shift+Tab: the next and previous section's first
    /// tile.
    NextSection,
    PrevSection,
    /// Tab: the selected subject's actions beside the list.
    Actions,
    /// One step back inside the column: a target list to its actions,
    /// the actions to the list.
    Back,
    /// →: into the selected subject's children.
    Into,
    /// Tab and Enter on the mirror: the next camera.
    MirrorNext,
}

#[cfg(test)]
impl KeyAction {
    /// The name the tests read an action by.
    pub fn as_str(self) -> &'static str {
        match self {
            KeyAction::Pass => "pass",
            KeyAction::Up => "up",
            KeyAction::Down => "down",
            KeyAction::Left => "left",
            KeyAction::Right => "right",
            KeyAction::PageUp => "pageUp",
            KeyAction::PageDown => "pageDown",
            KeyAction::Home => "home",
            KeyAction::End => "end",
            KeyAction::ScrollUp => "scrollUp",
            KeyAction::ScrollDown => "scrollDown",
            KeyAction::ScrollPageUp => "scrollPageUp",
            KeyAction::ScrollPageDown => "scrollPageDown",
            KeyAction::ScrollHome => "scrollHome",
            KeyAction::ScrollEnd => "scrollEnd",
            KeyAction::Submit => "submit",
            KeyAction::Activate => "activate",
            KeyAction::ActivateAlternate => "activateAlternate",
            KeyAction::Close => "close",
            KeyAction::Clear => "clear",
            KeyAction::Pop => "pop",
            KeyAction::Swallow => "swallow",
            KeyAction::NextSection => "nextSection",
            KeyAction::PrevSection => "prevSection",
            KeyAction::Actions => "actions",
            KeyAction::Back => "back",
            KeyAction::Into => "into",
            KeyAction::MirrorNext => "mirrorNext",
        }
    }
}

/// What a key press in the search field means. Everything answering `Pass`
/// is left to the field itself, which is what makes Ctrl+Left/Right move the
/// caret by word and Ctrl+Backspace delete one while the arrows belong to
/// the results.
pub fn key_action(key: Key, mods: Modifiers, auto_repeat: bool, ctx: &KeyCtx) -> KeyAction {
    let plain = !mods.ctrl && !mods.shift && !mods.alt;
    let input = ctx.mode == KeyMode::Input;

    if ctx.mirror && matches!(key, Key::Tab | Key::Return | Key::Enter) {
        return KeyAction::MirrorNext;
    }

    if ctx.column {
        return match key {
            Key::Left if plain && !ctx.query => KeyAction::Back,
            Key::Escape if ctx.query => KeyAction::Clear,
            Key::Escape | Key::Backtab => KeyAction::Back,
            Key::Backspace if !ctx.query => if auto_repeat { KeyAction::Swallow } else { KeyAction::Back },
            Key::Tab => KeyAction::Swallow,
            Key::Return | Key::Enter => KeyAction::Activate,
            Key::Up => KeyAction::Up,
            Key::Down => KeyAction::Down,
            Key::PageUp => KeyAction::PageUp,
            Key::PageDown => KeyAction::PageDown,
            Key::Home if plain => KeyAction::Home,
            Key::End if plain => KeyAction::End,
            _ => KeyAction::Pass,
        };
    }

    match key {
        Key::Up => if ctx.app_view { KeyAction::ScrollUp } else { KeyAction::Up },
        Key::Down => if ctx.app_view { KeyAction::ScrollDown } else { KeyAction::Down },
        Key::Left => {
            if plain && ctx.grid && !ctx.app_view {
                KeyAction::Left
            } else if plain && ctx.level && !ctx.query && ctx.mode == KeyMode::Menu {
                if auto_repeat { KeyAction::Swallow } else { KeyAction::Pop }
            } else {
                KeyAction::Pass
            }
        }
        Key::Right => {
            if plain && ctx.grid && !ctx.app_view {
                KeyAction::Right
            } else if plain && ctx.into && ctx.mode == KeyMode::Menu {
                KeyAction::Into
            } else {
                KeyAction::Pass
            }
        }
        Key::PageUp => {
            if ctx.app_view {
                return if ctx.scrollable { KeyAction::ScrollPageUp } else { KeyAction::Pass };
            }
            if input { KeyAction::Pass } else { KeyAction::PageUp }
        }
        Key::PageDown => {
            if ctx.app_view {
                return if ctx.scrollable { KeyAction::ScrollPageDown } else { KeyAction::Pass };
            }
            if input { KeyAction::Pass } else { KeyAction::PageDown }
        }
        Key::Home => {
            if ctx.app_view {
                return if ctx.scrollable && plain { KeyAction::ScrollHome } else { KeyAction::Pass };
            }
            if plain && !input { KeyAction::Home } else { KeyAction::Pass }
        }
        Key::End => {
            if ctx.app_view {
                return if ctx.scrollable && plain { KeyAction::ScrollEnd } else { KeyAction::Pass };
            }
            if plain && !input { KeyAction::End } else { KeyAction::Pass }
        }
        Key::Return | Key::Enter => {
            if input {
                KeyAction::Submit
            } else if mods.shift {
                KeyAction::ActivateAlternate
            } else {
                KeyAction::Activate
            }
        }
        Key::Escape => {
            if input {
                KeyAction::Close
            } else if ctx.query {
                KeyAction::Clear
            } else if ctx.mode == KeyMode::Menu {
                KeyAction::Pop
            } else {
                KeyAction::Close
            }
        }
        Key::Backspace => {
            if ctx.query || ctx.mode != KeyMode::Menu {
                return KeyAction::Pass;
            }
            // One level per physical press: a held key repeats, and a repeat
            // that outlived the text it was deleting would walk the whole
            // tree and close the launcher.
            if auto_repeat { KeyAction::Swallow } else { KeyAction::Pop }
        }
        Key::Tab if ctx.sections && mods.ctrl && !mods.shift => KeyAction::NextSection,
        Key::Tab | Key::Backtab if ctx.sections && mods.ctrl => KeyAction::PrevSection,
        Key::Tab if plain && ctx.mode == KeyMode::Menu => KeyAction::Actions,
        Key::Tab | Key::Backtab => KeyAction::Swallow,
        Key::Other => KeyAction::Pass,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers { ctrl: false, shift: false, alt: false };
    const CTRL: Modifiers = Modifiers { ctrl: true, shift: false, alt: false };
    const SHIFT: Modifiers = Modifiers { ctrl: false, shift: true, alt: false };

    fn ctx() -> KeyCtx {
        KeyCtx { mode: KeyMode::Menu, query: false, grid: false, app_view: false, scrollable: false, sections: false, level: false, column: false, into: false, mirror: false }
    }

    fn act(key: Key, mods: Modifiers, repeat: bool, ctx: KeyCtx) -> &'static str {
        key_action(key, mods, repeat, &ctx).as_str()
    }

    #[test]
    fn the_mirror_steps_cameras_on_tab_and_enter() {
        let mirror = KeyCtx { mirror: true, ..ctx() };
        assert_eq!(act(Key::Tab, NONE, false, mirror), "mirrorNext");
        assert_eq!(act(Key::Tab, CTRL, false, mirror), "mirrorNext");
        assert_eq!(act(Key::Return, SHIFT, false, mirror), "mirrorNext");
        assert_eq!(act(Key::Escape, NONE, false, mirror), "pop");
    }

    #[test]
    fn arrows_walk_the_results() {
        assert_eq!(act(Key::Up, NONE, false, ctx()), "up");
        assert_eq!(act(Key::Down, NONE, false, KeyCtx { query: true, ..ctx() }), "down");
        assert_eq!(act(Key::Left, NONE, false, KeyCtx { grid: true, ..ctx() }), "left");
        assert_eq!(act(Key::Right, NONE, false, KeyCtx { grid: true, query: true, ..ctx() }), "right");
    }

    // On a row list Left/Right are the caret's; on a grid, Ctrl+Left/Right still
    // are, which is what keeps a typo fixable while the grid owns the plain arrows.
    #[test]
    fn the_caret_keeps_ctrl_arrows_and_plain_arrows_on_a_list() {
        let grid_query = KeyCtx { grid: true, query: true, ..ctx() };
        assert_eq!(act(Key::Left, NONE, false, KeyCtx { query: true, ..ctx() }), "pass");
        assert_eq!(act(Key::Left, CTRL, false, grid_query), "pass");
        assert_eq!(act(Key::Right, CTRL, false, grid_query), "pass");
        assert_eq!(act(Key::Left, SHIFT, false, grid_query), "pass");
    }

    #[test]
    fn ctrl_backspace_with_a_query_is_the_fields() {
        let query = KeyCtx { query: true, ..ctx() };
        assert_eq!(act(Key::Backspace, CTRL, false, query), "pass");
        assert_eq!(act(Key::Backspace, NONE, true, query), "pass");
    }

    // A held Backspace that outlived its text used to walk the whole tree and close
    // the launcher.
    #[test]
    fn backspace_on_an_empty_field_pops_once_per_press() {
        assert_eq!(act(Key::Backspace, NONE, false, ctx()), "pop");
        assert_eq!(act(Key::Backspace, NONE, true, ctx()), "swallow");
        assert_eq!(act(Key::Backspace, NONE, false, KeyCtx { mode: KeyMode::Select, ..ctx() }), "pass");
    }

    // Left on an empty field inside a mode leaves it, once per press; with text,
    // at the root or on a grid it stays the caret's or the grid's.
    #[test]
    fn left_on_an_empty_field_leaves_the_mode() {
        let mode = KeyCtx { level: true, ..ctx() };
        assert_eq!(act(Key::Left, NONE, false, mode), "pop");
        assert_eq!(act(Key::Left, NONE, true, mode), "swallow");
        assert_eq!(act(Key::Left, NONE, false, KeyCtx { query: true, ..mode }), "pass");
        assert_eq!(act(Key::Left, CTRL, false, mode), "pass");
        assert_eq!(act(Key::Left, NONE, false, ctx()), "pass");
        assert_eq!(act(Key::Left, NONE, false, KeyCtx { grid: true, ..mode }), "left");
        assert_eq!(act(Key::Left, NONE, false, KeyCtx { mode: KeyMode::Select, ..mode }), "pass");
    }

    #[test]
    fn escape_clears_then_pops() {
        assert_eq!(act(Key::Escape, NONE, false, KeyCtx { query: true, ..ctx() }), "clear");
        assert_eq!(act(Key::Escape, NONE, false, ctx()), "pop");
        assert_eq!(act(Key::Escape, NONE, false, KeyCtx { mode: KeyMode::Select, query: true, ..ctx() }), "clear");
        assert_eq!(act(Key::Escape, NONE, false, KeyCtx { mode: KeyMode::Select, ..ctx() }), "close");
        assert_eq!(act(Key::Escape, NONE, false, KeyCtx { mode: KeyMode::Input, query: true, ..ctx() }), "close");
    }

    #[test]
    fn tab_opens_the_actions_and_never_leaves_the_field() {
        assert_eq!(act(Key::Tab, NONE, false, ctx()), "actions");
        assert_eq!(act(Key::Tab, NONE, false, KeyCtx { sections: true, ..ctx() }), "actions");
        assert_eq!(act(Key::Tab, CTRL, false, KeyCtx { sections: true, ..ctx() }), "nextSection");
        let ctrl_shift = Modifiers { ctrl: true, shift: true, alt: false };
        assert_eq!(act(Key::Backtab, ctrl_shift, false, KeyCtx { sections: true, ..ctx() }), "prevSection");
        assert_eq!(act(Key::Tab, ctrl_shift, false, KeyCtx { sections: true, ..ctx() }), "prevSection");
        assert_eq!(act(Key::Tab, CTRL, false, ctx()), "swallow");
        assert_eq!(act(Key::Backtab, SHIFT, false, ctx()), "swallow");
        assert_eq!(act(Key::Tab, NONE, false, KeyCtx { mode: KeyMode::Select, ..ctx() }), "swallow");
    }

    #[test]
    fn the_column_has_the_keys_while_open() {
        let column = KeyCtx { column: true, ..ctx() };
        assert_eq!(act(Key::Down, NONE, false, column), "down");
        assert_eq!(act(Key::Return, SHIFT, false, column), "activate");
        assert_eq!(act(Key::Tab, NONE, false, column), "swallow");
        assert_eq!(act(Key::Backtab, SHIFT, false, column), "back");
        assert_eq!(act(Key::Escape, NONE, false, KeyCtx { query: true, ..column }), "clear");
        assert_eq!(act(Key::Escape, NONE, false, column), "back");
        assert_eq!(act(Key::Left, NONE, false, column), "back");
        assert_eq!(act(Key::Left, NONE, false, KeyCtx { query: true, ..column }), "pass");
        assert_eq!(act(Key::Backspace, NONE, false, column), "back");
        assert_eq!(act(Key::Backspace, NONE, true, column), "swallow");
        assert_eq!(act(Key::Backspace, NONE, false, KeyCtx { query: true, ..column }), "pass");
    }

    #[test]
    fn right_steps_into_a_subject_only_when_it_can() {
        let into = KeyCtx { into: true, ..ctx() };
        assert_eq!(act(Key::Right, NONE, false, into), "into");
        assert_eq!(act(Key::Right, CTRL, false, into), "pass");
        assert_eq!(act(Key::Right, NONE, false, ctx()), "pass");
        assert_eq!(act(Key::Right, NONE, false, KeyCtx { mode: KeyMode::Select, ..into }), "pass");
        assert_eq!(act(Key::Right, NONE, false, KeyCtx { grid: true, ..into }), "right");
    }

    #[test]
    fn page_home_and_end_go_to_the_results() {
        assert_eq!(act(Key::PageDown, NONE, false, ctx()), "pageDown");
        assert_eq!(act(Key::PageUp, NONE, false, KeyCtx { grid: true, ..ctx() }), "pageUp");
        assert_eq!(act(Key::Home, NONE, false, KeyCtx { query: true, ..ctx() }), "home");
        assert_eq!(act(Key::End, NONE, false, KeyCtx { query: true, ..ctx() }), "end");
        assert_eq!(act(Key::Home, SHIFT, false, KeyCtx { query: true, ..ctx() }), "pass");
        assert_eq!(act(Key::End, NONE, false, KeyCtx { mode: KeyMode::Input, query: true, ..ctx() }), "pass");
    }

    #[test]
    fn an_app_view_scrolls_instead() {
        let view = KeyCtx { app_view: true, ..ctx() };
        let scrolling = KeyCtx { app_view: true, scrollable: true, ..ctx() };
        assert_eq!(act(Key::Down, NONE, false, view), "scrollDown");
        assert_eq!(act(Key::PageDown, NONE, false, scrolling), "scrollPageDown");
        assert_eq!(act(Key::PageDown, NONE, false, view), "pass");
        assert_eq!(act(Key::Home, NONE, false, scrolling), "scrollHome");
        assert_eq!(act(Key::Left, NONE, false, KeyCtx { app_view: true, grid: true, ..ctx() }), "pass");
    }

    #[test]
    fn enter_activates_and_shift_enter_is_the_alternate() {
        assert_eq!(act(Key::Return, NONE, false, ctx()), "activate");
        assert_eq!(act(Key::Enter, SHIFT, false, ctx()), "activateAlternate");
        assert_eq!(act(Key::Return, NONE, false, KeyCtx { mode: KeyMode::Input, ..ctx() }), "submit");
    }

    #[test]
    fn printable_keys_are_the_fields() {
        assert_eq!(key_action(Key::Other, NONE, false, &KeyCtx { grid: true, ..ctx() }), KeyAction::Pass);
    }
}
