//! Quick settings' pieces (specs/2026-11-04-quick-settings.md §2-§6), each
//! a Settings tile: an interactive `panel.group` cell, its `open` fill while
//! on. `id` is a piece's click and its cursor stop; an unavailable piece
//! takes neither and fades the way a disabled button does.

use fs_theme::tokens::Space;

use crate::ui::w::{caption, cell, column, icon, icon_button, label, panel_chip, panel_line, panel_row, panel_value, row, separator, slider, space, spacer, text_inset};
use crate::ui::{El, Ink, Size, Type, Weight};

fn tile(content: El, id: &str, on: bool, available: bool) -> El {
    let c = cell(content).cell_state(|c| {
        c.group = true;
        c.open = on;
        c.disabled = !available;
        c.inset = Some((0.0, 0.0));
    });
    if available { c.interactive().on(id).stop(id) } else { c.key(id).fade(0.5) }
}

/// A tile's line: the chip `sm` in, so the chip column is the same in
/// every tile and row of the card, the words after it.
fn line(s: &Space, glyph: &str, title: &str, subtitle: Option<&str>, on: bool) -> El {
    let detail = subtitle.filter(|t| !t.is_empty()).map(caption);
    panel_line(s, Some(panel_chip(s, glyph, on)), label(title), detail, Vec::new()).pad(s.sm, 0.0, s.lg, 0.0)
}

/// A glyph centred in a `panelRow` square tile: a split's arrow, a system
/// button.
fn square(s: &Space, id: &str, glyph: &str, on: bool, available: bool) -> El {
    let mark = row(0.0, vec![spacer(), icon(glyph).size(Type::Heading), spacer()]).fill().height(s.panel_row);
    tile(mark, id, on, available).width(Size::Px(s.panel_row)).height(s.panel_row)
}

/// A plain toggle, `panelRow` tall, as wide as its grid column.
pub fn toggle(s: &Space, id: &str, glyph: &str, title: &str, subtitle: Option<&str>, on: bool, available: bool) -> El {
    tile(line(s, glyph, title, subtitle, on), id, on, available).height(s.panel_row)
}

/// A toggle with a menu: the toggle (`id`) and its arrow tile
/// (`<id>:menu`) `rowGap` apart, both lit while on.
pub fn split(s: &Space, id: &str, glyph: &str, title: &str, subtitle: Option<&str>, on: bool, available: bool) -> El {
    let part = toggle(s, id, glyph, title, subtitle, on, available);
    let arrow = square(s, &format!("{id}:menu"), "chevron-right", on, available);
    row(s.row_gap, vec![part, arrow]).fill().height(s.panel_row)
}

/// An extra's tile (§7): a toggle that is never on, the cell's value as
/// its subtitle, a click handing over to the cell's panel.
pub fn info(s: &Space, id: &str, glyph: &str, title: &str, value: Option<&str>) -> El {
    toggle(s, id, glyph, title, value, false, true)
}

/// Background (§6): a full-width row, its chip on the tiles' chip column
/// and its chevron on their arrow column, the count as its name, opening
/// its menu.
pub fn background(s: &Space, id: &str, title: &str) -> El {
    let mark = row(0.0, vec![spacer(), icon("chevron-right").ink(Ink::Muted), spacer()]).width(Size::Px(s.panel_mark * 2.0));
    panel_row(s, "activity", title, None, vec![mark]).cell_state(|c| c.inset = Some((s.sm, 0.0))).on(id).stop(id)
}

/// The system row's button: a square tile, its name the tooltip.
pub fn button(s: &Space, id: &str, glyph: &str, tip: &str) -> El {
    square(s, id, glyph, false, true).tip(tip)
}

/// The system row's battery: a tile as wide as its chip and its value in
/// `mono`.
pub fn battery(s: &Space, id: &str, glyph: &str, value: &str) -> El {
    let line = row(s.panel_gap, vec![panel_chip(s, glyph, false), panel_value(value)]).height(s.panel_row).pad(s.sm, 0.0, s.lg, 0.0);
    tile(line, id, false, true).width(Size::Hug).height(s.panel_row)
}

/// A slider row, `panelRowCompact` tall: its glyph a button (`<id>:icon`,
/// a mute), the kit's slider (`id`), and with `menu` the arrow
/// (`<id>:menu`) opening its devices. The glyphs centre on the tiles' chip
/// column and arrow column.
pub fn slider_row(s: &Space, id: &str, glyph: &str, value: f64, menu: bool) -> El {
    let button = |key: String, glyph: &str| icon_button(glyph).on(key.clone()).stop(key);
    let mut parts = vec![button(format!("{id}:icon"), glyph), slider(value).fill().on(id).stop(id)];
    if menu {
        parts.push(button(format!("{id}:menu"), "chevron-right"));
    }
    let x = s.sm + s.panel_mark - s.control_height / 2.0;
    row(s.panel_gap, parts).fill().height(s.panel_row_compact).pad(x, 0.0, x, 0.0)
}

/// An open menu (§4), keyed `menu`: a `panel.group` box holding a header
/// (the toggle's glyph in its chip, lit while the toggle is on, and its
/// title; GNOME's menus set no subtitle), the menu's rows, and with
/// `settings` a hairline and that row last. `shown` fades what it holds.
pub fn menu(s: &Space, glyph: &str, title: &str, on: bool, rows: Vec<El>, settings: Option<El>, shown: f32) -> El {
    let words = label(title).weight(Weight::Semibold);
    let header = panel_line(s, Some(panel_chip(s, glyph, on)), words, None, Vec::new()).pad(text_inset(s), 0.0, text_inset(s), 0.0);
    let mut parts = vec![header, space(s.panel_gap)];
    parts.extend(rows);
    if let Some(settings) = settings {
        parts.push(separator());
        parts.push(settings);
    }
    let content = column(0.0, parts).fill();
    let content = if shown < 1.0 { content.fade(shown) } else { content };
    cell(content)
        .cell_state(|c| {
            c.group = true;
            c.inset = Some((s.lg, s.lg));
        })
        .key("menu")
}
