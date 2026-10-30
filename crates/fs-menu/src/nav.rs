//! The launcher's cursor and keys as pure functions over the committed row
//! list: where the cursor lands when the rows change, where one key press
//! moves it, and what a key means at all.
//!
//! The cursor is a row id, and an index only as a consequence of where that
//! id sits in the rows on screen. Rows change for more reasons than typing,
//! and an index held across such a change points at whatever row slid into
//! its slot.

/// Where the cursor goes when the row list becomes `ids`. `fresh` is a new
/// query, level or open, which always starts on the first row; `placed` is
/// whether a key or the pointer has moved the cursor since. Until one has,
/// the cursor is the list's head rather than any particular row. Once
/// placed, the row it was put on (`want_id`) keeps it wherever that row now
/// sits, and while that row is gone the cursor holds `prev_index`, clamped
/// to the list.
pub fn rederive(want_id: &str, prev_index: i64, ids: &[&str], fresh: bool, placed: bool) -> usize {
    let n = ids.len();
    if n == 0 || fresh || !placed {
        return 0;
    }
    if !want_id.is_empty()
        && let Some(at) = ids.iter().position(|id| *id == want_id)
    {
        return at;
    }
    prev_index.clamp(0, n as i64 - 1) as usize
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Home,
    End,
    PageDown,
    PageUp,
    Left,
    Right,
    Down,
    Up,
    /// A direction nothing handles: the index stays where it is.
    Other,
}

impl Dir {
    pub fn parse(name: &str) -> Dir {
        match name {
            "home" => Dir::Home,
            "end" => Dir::End,
            "pageDown" => Dir::PageDown,
            "pageUp" => Dir::PageUp,
            "left" => Dir::Left,
            "right" => Dir::Right,
            "down" => Dir::Down,
            "up" => Dir::Up,
            _ => Dir::Other,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub index: usize,
    /// A step travels; a wrap, a page or an end snaps. The cursor fill
    /// animates on this.
    pub travels: bool,
}

const fn snap(index: usize) -> Step {
    Step { index, travels: false }
}

const fn travel(index: usize) -> Step {
    Step { index, travels: true }
}

/// One key's move over a list whose first `cells` entries are a grid of
/// `columns` and whose rest are rows under it: `cells` is the whole list for
/// the picker and emoji grids, 0 for a row list, the app count for the app
/// grid.
///
/// Vertical moves keep the column: off the bottom of the grid onto the top
/// row of the same column, off the top onto the last row that has one, so a
/// short last row never swallows the column. Out of the grid's last row the
/// cursor goes to the first row under it, and up out of that row back to the
/// last cell. Horizontal moves walk the list in reading order and stop at
/// either end.
pub fn step(index: usize, dir: Dir, count: usize, cells: usize, columns: usize, page: usize) -> Step {
    let n = count;
    if n == 0 {
        return snap(0);
    }
    let i = index.min(n - 1);
    let g = cells.min(n);
    let c = if g > 0 { columns.max(1) } else { 1 };
    let tail = g < n;

    match dir {
        Dir::Home => snap(0),
        Dir::End => snap(n - 1),
        Dir::PageDown => snap((i + page.max(1)).min(n - 1)),
        Dir::PageUp => snap(i.saturating_sub(page.max(1))),
        Dir::Left => if i > 0 { travel(i - 1) } else { snap(i) },
        Dir::Right => if i < n - 1 { travel(i + 1) } else { snap(i) },
        Dir::Down => {
            if i < g {
                let last_row_start = (g - 1) / c * c;
                if i < last_row_start {
                    return travel((i + c).min(g - 1));
                }
                if tail {
                    return travel(g);
                }
                return snap(i % c);
            }
            if i + 1 < n {
                return travel(i + 1);
            }
            snap(0)
        }
        Dir::Up => {
            if i < g {
                if i >= c {
                    return travel(i - c);
                }
                if tail {
                    return snap(n - 1);
                }
                return snap(last_in_column(i % c, g, c));
            }
            if i > g {
                return travel(i - 1);
            }
            if g > 0 {
                return travel(g - 1);
            }
            snap(n - 1)
        }
        Dir::Other => snap(i),
    }
}

/// The lowest cell in `column` of a grid of `cells`: the last row's own cell
/// when that row reaches the column, the row above it when it is short.
pub fn last_in_column(column: usize, cells: usize, columns: usize) -> usize {
    let at = (cells - 1) / columns * columns + column;
    if at > cells - 1 { at - columns } else { at }
}

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
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
    /// The picker's Dark | Light switcher is up.
    pub variants: bool,
    /// Standing inside a mode rather than at the root.
    pub level: bool,
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
    Variant,
}

impl KeyAction {
    /// The name the IPC status uses.
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
            KeyAction::Variant => "variant",
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
        Key::Right => if plain && ctx.grid && !ctx.app_view { KeyAction::Right } else { KeyAction::Pass },
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
        Key::Tab | Key::Backtab => if ctx.variants { KeyAction::Variant } else { KeyAction::Swallow },
        Key::Other => KeyAction::Pass,
    }
}
