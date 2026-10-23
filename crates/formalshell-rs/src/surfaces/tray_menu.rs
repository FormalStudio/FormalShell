//! A tray item's dbusmenu tree in a card of the `menu` role.
//! Submenus expand in place as indented rows (one surface, no cascade), a
//! check rides the trailing slot of a ticked row and a chevron the slot of a
//! submenu, separators are one `border` rule with an `sm` gap either side.
//! The cursor walks the rows over IPC, the pointer and, once the keyboard
//! reaches panels, real keys; a hovered row takes it.

use std::collections::HashMap;

use fs_theme::color::Rgba;
use fs_theme::theme::Theme;
use fs_theme::tokens::WEIGHTS;

use crate::motion::{Animated, SPATIAL};
use crate::scene::{Bitmap, IRect, NodeId, Paint, Scene};
use crate::services::tray::MenuNode;
use crate::surfaces::bar::cell::{Kit, Painter};
use crate::text::{ShapedText, TextStyle};

/// What a menu takes off the theme beyond the cell kit's look.
struct Look {
    padding: f64,
    control_height: f64,
    control_px: f64,
    indent: f64,
    icon_gap: f64,
    sm: f64,
    border_width: f64,
    border: Rgba,
    accent: Rgba,
    accent_fg: Rgba,
    subtitle: f32,
    width: f64,
}

impl Look {
    fn new(theme: &Theme) -> Self {
        let s = &theme.space;
        Self {
            padding: s.panel_padding,
            control_height: s.control_height,
            control_px: s.control_padding_x,
            indent: s.xxl,
            icon_gap: s.icon_gap,
            sm: s.sm,
            border_width: theme.border_width,
            border: theme.colors.get("border"),
            accent: theme.colors.get("accent"),
            accent_fg: theme.colors.get("accentForeground"),
            subtitle: theme.font_size.subtitle as f32,
            width: s.popup_width_default,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Row {
    node: MenuNode,
    depth: usize,
    expanded: bool,
}

/// What activating a row asks the app to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    None,
    /// A submenu opened or shut; `true` when it opened and the item should
    /// be asked to fill it.
    Toggled(i32, bool),
    /// A leaf fired: send its click and close.
    Click(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Close,
    Row(usize),
}

pub struct Menu {
    /// The tray item's own `Id`, which IPC names it by.
    pub id: String,
    pub key: String,
    title: String,
    tree: Option<MenuNode>,
    expanded: Vec<i32>,
    rows: Vec<Row>,
    pub cursor: Option<usize>,
    pub cursor_active: bool,
    /// The tallest the card may be, which rows past it are cut at.
    pub cap: f64,
    /// Where along the line the card centres, for a resize to clamp again.
    pub anchor: f64,
    /// The card's height on its way to what the rows ask for.
    pub morph: Animated,
    /// Rows scrolled off the top of a card capped below its rows.
    scroll: f64,
    /// The rows' viewport, in the card's surface.
    close_hover: bool,
    body: IRect,
    look: Look,
    nodes: Vec<NodeId>,
    rects: Vec<IRect>,
    close: IRect,
    icons: HashMap<i32, ((u32, f64), Option<Bitmap>)>,
}

impl Menu {
    pub fn new(theme: &Theme, id: String, key: String, title: String) -> Self {
        Self {
            id,
            key,
            title,
            tree: None,
            expanded: Vec::new(),
            rows: Vec::new(),
            cursor: None,
            cursor_active: false,
            cap: f64::INFINITY,
            anchor: 0.0,
            morph: Animated::new(0.0, SPATIAL),
            scroll: 0.0,
            close_hover: false,
            body: IRect::default(),
            look: Look::new(theme),
            nodes: Vec::new(),
            rects: Vec::new(),
            close: IRect::default(),
            icons: HashMap::new(),
        }
    }

    /// The card's width, which is the panel default.
    pub fn width(&self) -> f64 {
        self.look.width
    }

    #[cfg(test)]
    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    /// The tree as the item last sent it; the rows rebuild off it.
    pub fn set_tree(&mut self, tree: Option<&MenuNode>) {
        self.tree = tree.cloned();
        self.rebuild();
    }

    fn flatten(&self, node: &MenuNode, depth: usize, out: &mut Vec<Row>) {
        for child in &node.children {
            let expanded = child.has_children && self.expanded.contains(&child.id);
            out.push(Row { node: child.clone(), depth, expanded });
            if expanded {
                self.flatten(child, depth + 1, out);
            }
        }
    }

    fn rebuild(&mut self) {
        let mut rows = Vec::new();
        if let Some(root) = self.tree.clone() {
            self.flatten(&root, 0, &mut rows);
        }
        self.rows = rows;
        self.scroll = self.scroll.min(self.max_scroll());
        let bad = self.cursor.is_none_or(|c| c >= self.rows.len());
        if bad {
            self.cursor = (!self.rows.is_empty()).then_some(0);
        }
    }

    fn row_height(&self, row: &Row) -> f64 {
        if row.node.separator { self.look.border_width + self.look.sm * 2.0 } else { self.look.control_height }
    }

    fn header_gap(&self) -> f64 {
        self.look.padding * 2.0 + self.look.border_width
    }

    /// The card's height with every row it holds, or its one "Empty menu"
    /// row.
    pub fn height(&self) -> f64 {
        self.chrome() + self.rows_height()
    }

    /// The header and the card's own padding, which the rows do not scroll under.
    fn chrome(&self) -> f64 {
        self.look.padding * 2.0 + self.look.control_height + self.header_gap()
    }

    fn rows_height(&self) -> f64 {
        if self.rows.is_empty() { self.look.control_height } else { self.rows.iter().map(|r| self.row_height(r)).sum() }
    }

    /// How far the rows can scroll inside the card at its capped height.
    fn max_scroll(&self) -> f64 {
        let view = self.height().min(self.cap) - self.chrome();
        (self.rows_height() - view).max(0.0)
    }

    /// A wheel notch moves the rows by `controlHeight`; true when they moved.
    pub fn scroll_notches(&mut self, notches: f64) -> bool {
        let next = (self.scroll + notches * self.look.control_height).clamp(0.0, self.max_scroll());
        let moved = next != self.scroll;
        self.scroll = next;
        moved
    }

    /// Whether a point is over the rows' viewport.
    fn in_body(&self, x: i32, y: i32) -> bool {
        x >= self.body.x && x < self.body.right() && y >= self.body.y && y < self.body.bottom()
    }

    /// Cursor movement skips separators; a disabled row stays reachable and
    /// `activate` is what refuses it.
    pub fn move_cursor(&mut self, delta: i32) {
        self.cursor_active = true;
        let n = self.rows.len() as i32;
        if n == 0 {
            self.cursor = None;
            return;
        }
        let mut idx = match self.cursor {
            None => {
                if delta > 0 {
                    -1
                } else {
                    0
                }
            }
            Some(c) => c as i32,
        };
        for _ in 0..n {
            idx = (idx + delta).rem_euclid(n);
            if !self.rows[idx as usize].node.separator {
                break;
            }
        }
        self.cursor = Some(idx as usize);
    }

    pub fn activate_cursor(&mut self) -> Outcome {
        match self.cursor {
            Some(i) if i < self.rows.len() => self.activate(i),
            _ => Outcome::None,
        }
    }

    pub fn activate(&mut self, i: usize) -> Outcome {
        let Some(row) = self.rows.get(i) else { return Outcome::None };
        if row.node.separator || !row.node.enabled {
            return Outcome::None;
        }
        let id = row.node.id;
        if row.node.has_children {
            let opened = !self.expanded.contains(&id);
            if opened {
                self.expanded.push(id);
            } else {
                self.expanded.retain(|e| *e != id);
            }
            self.rebuild();
            return Outcome::Toggled(id, opened);
        }
        Outcome::Click(id)
    }

    pub fn hit(&self, x: i32, y: i32) -> Option<Hit> {
        let inside = |r: &IRect| x >= r.x && x < r.right() && y >= r.y && y < r.bottom();
        if inside(&self.close) {
            return Some(Hit::Close);
        }
        self.rects.iter().position(inside).filter(|i| !self.rows[*i].node.separator && self.in_body(x, y)).map(Hit::Row)
    }

    /// The pointer over a row takes the cursor, as TrayMenu's `onEntered`
    /// does; a disabled row takes nothing. True when the picture changed.
    pub fn hover(&mut self, hit: Option<Hit>) -> bool {
        let close = hit == Some(Hit::Close);
        let mut changed = close != self.close_hover;
        self.close_hover = close;
        if let Some(Hit::Row(i)) = hit {
            if self.rows[i].node.enabled && (self.cursor != Some(i) || !self.cursor_active) {
                self.cursor = Some(i);
                self.cursor_active = true;
                changed = true;
            }
        }
        changed
    }

    fn elide(kit: &mut Kit, text: &str, style: TextStyle, max: f64) -> ShapedText {
        let full = kit.shape(text, style);
        if full.width as f64 <= max {
            return full;
        }
        let chars: Vec<char> = text.chars().collect();
        let (mut lo, mut hi) = (0, chars.len());
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            let cut: String = chars[..mid].iter().collect::<String>() + "…";
            if kit.shape(&cut, style).width as f64 <= max {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        kit.shape(&(chars[..lo].iter().collect::<String>() + "…"), style)
    }

    /// Draws the header and the rows into the card's content rect.
    pub fn layout(&mut self, kit: &mut Kit, scene: &mut Scene, rect: IRect, alpha: f32, clip: IRect) {
        let look = &self.look;
        let k = kit.look.clone();
        let (pad, ch, bw) = (look.padding as i32, look.control_height as i32, look.border_width.round().max(1.0) as i32);
        let a = |c: Rgba| c.with_alpha(c.a * alpha);
        let mut nodes = std::mem::take(&mut self.nodes);
        let mut p = Painter::new(scene, &mut nodes, Some(clip.intersect(&rect)));

        // The header: the item's own name and a close button.
        let top = rect.y + pad;
        self.close = IRect::new(rect.right() - pad - ch, top, ch, ch);
        if self.close_hover {
            if let Some(wash) = k.hover_wash {
                p.rect(self.close, a(wash), k.radius);
            }
        }
        let x_icon = kit.icon("x");
        p.text(&x_icon, (self.close.x + (ch - x_icon.width) / 2, top + (ch - x_icon.line_height()) / 2), a(k.foreground), &[]);
        let title_style = TextStyle { family: k.sans, size: look.subtitle, weight: WEIGHTS.semibold as f32, tracking: 0.0 };
        let title_room = (self.close.x - rect.x - pad) as f64 - look.icon_gap;
        let title = Self::elide(kit, &self.title, title_style, title_room);
        p.text(&title, (rect.x + pad, top + (ch - title.line_height()) / 2), a(k.foreground), &[]);
        let rule_y = top + ch + pad;
        p.rect(IRect::new(rect.x + bw, rule_y, rect.w - bw * 2, bw), a(look.border), 0.0);

        // The rows, abutting like the launcher's.
        let left = rect.x + pad;
        let width = rect.w - pad * 2;
        let body_top = (top as f64 + look.control_height + self.header_gap()).round() as i32;
        self.body = IRect::new(left, body_top, width, (rect.bottom() - pad - body_top).max(0));
        p.clip = Some(clip.intersect(&rect).intersect(&self.body));
        let mut y = body_top - self.scroll.round() as i32;
        self.rects.clear();
        if self.rows.is_empty() {
            let style = TextStyle { family: k.sans, size: k.caption, weight: WEIGHTS.medium as f32, tracking: 0.0 };
            let label = kit.shape("Empty menu", style);
            p.text(&label, (left, y + (ch - label.line_height()) / 2), a(k.muted), &[]);
        }
        let rows = self.rows.clone();
        for (i, row) in rows.iter().enumerate() {
            let h = self.row_height(row).round() as i32;
            let r = IRect::new(left, y, width, h);
            self.rects.push(r);
            y += h;
            if row.node.separator {
                p.rect(IRect::new(r.x, r.y + (r.h - bw) / 2, r.w, bw), a(look.border), 0.0);
                continue;
            }
            let cursor_here = self.cursor_active && self.cursor == Some(i);
            if cursor_here {
                p.rect(r, a(look.accent), 0.0);
            }
            let fg = if !row.node.enabled {
                k.muted
            } else if cursor_here {
                look.accent_fg
            } else {
                k.foreground
            };
            let size = k.body.round().max(1.0) as u32;
            let mut x = left + look.control_px as i32 + (row.depth as f64 * look.indent) as i32;
            let mut right = r.right() - look.control_px as i32;
            let trailing = if row.node.has_children {
                Some(if row.expanded { "chevron-down" } else { "chevron-right" })
            } else if row.node.checked {
                Some("check")
            } else {
                None
            };
            if let Some(name) = trailing {
                let t = kit.icon(name);
                right -= t.width;
                p.text(&t, (right, r.y + (r.h - t.line_height()) / 2), a(fg), &[]);
                right -= look.icon_gap as i32;
            }
            if let Some(raw) = &row.node.icon {
                let scale = crate::scene::raster_scale();
                let entry = self.icons.entry(row.node.id).or_insert(((0, 0.0), None));
                if entry.0 != (size, scale) {
                    *entry = ((size, scale), raw.bitmap(size, scale));
                }
                if let Some(image) = entry.1.clone() {
                    p.shape(IRect::new(x, r.y + (r.h - size as i32) / 2, size as i32, size as i32), Paint::Image { image, alpha });
                    x += size as i32 + look.icon_gap as i32;
                }
            }
            let label = Self::elide(kit, &row.node.label, k.sans(WEIGHTS.medium as f32), (right - x).max(0) as f64);
            p.text(&label, (x, r.y + (r.h - label.line_height()) / 2), a(fg), &[]);
        }
        p.finish();
        self.nodes = nodes;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::theme::getter;
    use serde_json::json;

    fn node(id: i32, label: &str) -> MenuNode {
        MenuNode { id, label: label.into(), enabled: true, separator: false, icon: None, checked: false, has_children: false, children: Vec::new() }
    }

    /// The tray-stub's fixture tree: plain, disabled, checked, a rule and a
    /// submenu with one child.
    fn tree() -> MenuNode {
        let mut disabled = node(2, "Disabled Item");
        disabled.enabled = false;
        let mut checked = node(3, "Checked Item");
        checked.checked = true;
        let mut rule = node(4, "");
        rule.separator = true;
        let mut sub = node(5, "Submenu");
        sub.has_children = true;
        sub.children = vec![node(6, "Child")];
        let mut root = node(0, "");
        root.children = vec![node(1, "Plain Item"), disabled, checked, rule, sub];
        root
    }

    fn menu() -> Menu {
        let theme = Theme::resolve(getter(json!({})), &fs_theme::palette::fallback("dark"));
        let mut m = Menu::new(&theme, "id".into(), "key".into(), "Item".into());
        m.set_tree(Some(&tree()));
        m
    }

    #[test]
    fn the_cursor_parks_on_the_first_row_and_skips_rules() {
        let mut m = menu();
        assert_eq!(m.cursor, Some(0));
        m.move_cursor(1);
        m.move_cursor(1);
        assert_eq!(m.cursor, Some(2));
        m.move_cursor(1);
        assert_eq!(m.cursor, Some(4));
        m.move_cursor(1);
        assert_eq!(m.cursor, Some(0));
        m.move_cursor(-1);
        assert_eq!(m.cursor, Some(4));
    }

    #[test]
    fn a_disabled_row_refuses_and_a_leaf_clicks() {
        let mut m = menu();
        assert_eq!(m.activate(1), Outcome::None);
        assert_eq!(m.activate(2), Outcome::Click(3));
    }

    #[test]
    fn a_submenu_expands_in_place_and_folds_again() {
        let mut m = menu();
        assert_eq!(m.rows(), 5);
        assert_eq!(m.activate(4), Outcome::Toggled(5, true));
        assert_eq!(m.rows(), 6);
        assert_eq!(m.activate(4), Outcome::Toggled(5, false));
        assert_eq!(m.rows(), 5);
    }

    #[test]
    fn a_card_capped_below_its_rows_scrolls_by_notches_and_stops_at_the_last_row() {
        let mut m = menu();
        m.cap = m.height() - m.look.control_height * 2.0;
        assert!(m.scroll_notches(1.0));
        assert_eq!(m.scroll, m.look.control_height);
        assert!(m.scroll_notches(5.0));
        let end = m.rows_height() - (m.height().min(m.cap) - m.chrome());
        assert_eq!(m.scroll, end);
        assert!(!m.scroll_notches(1.0));
        assert!(m.scroll_notches(-9.0));
        assert_eq!(m.scroll, 0.0);
    }

    #[test]
    fn a_card_that_holds_all_its_rows_does_not_scroll() {
        let mut m = menu();
        assert!(!m.scroll_notches(1.0));
        assert_eq!(m.scroll, 0.0);
    }

    #[test]
    fn height_counts_the_header_and_every_row() {
        let mut m = menu();
        let before = m.height();
        m.activate(4);
        assert!(m.height() > before);
        assert!(m.height() > m.look.control_height * 4.0);
    }
}
