//! The sonner depth stack's geometry: where every toast card lands in both
//! modes. The slot pool and animation bookkeeping live with the surface.
//!
//! COLLAPSED is the depth stack: the front card at full width, each level
//! behind it narrower by one `peek_inset` per side per level, a whole pixel
//! count so the surface's scale of the card to that width keeps its side
//! borders on the pixel grid, horizontally centred on the front card, as tall as the front card and
//! offset away from the anchored edge by `peek_offset`, so exactly that
//! sliver shows whatever the card behind holds. At most `max_peek_levels`
//! levels peek; deeper cards take the last level's geometry, sit behind it
//! and are not `visible`. EXPANDED is the plain list: every card full width
//! at its own height, `gap` apart, in the order given.
//!
//! A `None` key is an entry with no slot of its own: it draws nothing but
//! still consumes its rank, so the cards around it keep the level they would
//! have had. Each geometry carries its `rank`, its place in the pass that laid
//! it out, which is what a staggered restack shares its window out by.

use std::collections::HashMap;
use std::hash::Hash;

#[derive(Clone, Debug)]
pub struct StackParams<K> {
    pub frame_width: f64,
    pub peek_inset: f64,
    pub peek_offset: f64,
    pub max_peek_levels: usize,
    pub gap: f64,
    pub top: bool,
    pub heights: HashMap<K, f64>,
    pub collapsed: Vec<Option<K>>,
    pub expanded: Vec<Option<K>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geom {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub z: i64,
    pub rank: usize,
    pub content_visible: bool,
    /// Drawn at all: false for a card deeper than the last peek level.
    pub visible: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SlotGeom {
    pub collapsed: Option<Geom>,
    pub expanded: Option<Geom>,
}

#[derive(Clone, Debug)]
pub struct StackLayout<K> {
    pub by_key: HashMap<K, SlotGeom>,
    /// Every level present pokes out past the front card by its own offset,
    /// so the pile is that much taller than the card in front of it.
    pub collapsed_height: f64,
    pub expanded_height: f64,
}

impl<K> Default for StackParams<K> {
    fn default() -> Self {
        StackParams {
            frame_width: 0.0,
            peek_inset: 0.0,
            peek_offset: 0.0,
            max_peek_levels: 0,
            gap: 0.0,
            top: false,
            heights: HashMap::new(),
            collapsed: Vec::new(),
            expanded: Vec::new(),
        }
    }
}

pub fn layout<K: Clone + Eq + Hash>(p: &StackParams<K>) -> StackLayout<K> {
    let mut by_key: HashMap<K, SlotGeom> = HashMap::new();
    let height_of = |key: &K| p.heights.get(key).copied().unwrap_or(0.0);
    let front_height = p.collapsed.first().and_then(Option::as_ref).map_or(0.0, height_of);
    // Only the levels with a card on them reserve room, so a lone toast sits
    // flush against the anchored edge.
    let levels = p.collapsed.len().saturating_sub(1).min(p.max_peek_levels);

    for (r, key) in p.collapsed.iter().enumerate() {
        let Some(key) = key else { continue };
        let level = r.min(p.max_peek_levels);
        let inset = level as f64 * p.peek_inset;
        by_key.entry(key.clone()).or_default().collapsed = Some(Geom {
            x: inset,
            width: p.frame_width - inset * 2.0,
            height: if r == 0 { height_of(key) } else { front_height },
            // The reveal recedes away from the anchored edge: that edge
            // already carries the front card flush against it.
            y: if p.top { level as f64 * p.peek_offset } else { (levels - level) as f64 * p.peek_offset },
            z: (p.collapsed.len() - r) as i64,
            rank: r,
            content_visible: r == 0,
            visible: r <= p.max_peek_levels,
        });
    }

    // The expanded pass reuses the collapsed z rather than deriving its own.
    // The two orders are different lists (a critical toast floats to the
    // front of the collapsed one, the expanded one is chronological), so a z
    // of its own restacked every card on the frame the pointer arrived, while
    // its x and y were still gliding. Expanded cards do not overlap, so z
    // buys nothing there, and holding the collapsed order keeps the fan-out
    // reading as one stack opening rather than a reshuffle.
    let mut y = 0.0;
    for (i, key) in p.expanded.iter().enumerate() {
        let Some(key) = key else { continue };
        let slot = by_key.entry(key.clone()).or_default();
        let z = slot
            .collapsed
            .map_or((p.expanded.len() - i) as i64, |c| c.z);
        slot.expanded = Some(Geom {
            x: 0.0,
            width: p.frame_width,
            height: height_of(key),
            y,
            z,
            rank: i,
            content_visible: true,
            visible: true,
        });
        y += height_of(key) + p.gap;
    }

    StackLayout {
        by_key,
        collapsed_height: front_height + levels as f64 * p.peek_offset,
        expanded_height: if y > 0.0 { y - p.gap } else { 0.0 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // At the default scale `peek_inset` is 8, `peek_offset` and `gap` 12 and the
    // frame is `popupWidthNarrow` (320).
    const FRAME_WIDTH: f64 = 320.0;
    const PEEK_OFFSET: f64 = 12.0;
    const MAX_PEEK_LEVELS: usize = 2;
    const GAP: f64 = 12.0;

    type Key = &'static str;

    fn params() -> StackParams<Key> {
        StackParams {
            frame_width: FRAME_WIDTH,
            peek_inset: 8.0,
            peek_offset: PEEK_OFFSET,
            max_peek_levels: MAX_PEEK_LEVELS,
            gap: GAP,
            ..StackParams::default()
        }
    }

    fn keys(list: &[Key]) -> Vec<Option<Key>> {
        list.iter().map(|k| Some(*k)).collect()
    }

    fn heights(list: &[(Key, f64)]) -> HashMap<Key, f64> {
        list.iter().copied().collect()
    }

    fn collapsed(out: &StackLayout<Key>, key: Key) -> Geom {
        out.by_key[key].collapsed.unwrap()
    }

    fn expanded(out: &StackLayout<Key>, key: Key) -> Geom {
        out.by_key[key].expanded.unwrap()
    }

    fn with_collapsed(list: &[Key]) -> StackLayout<Key> {
        layout(&StackParams {
            collapsed: keys(list),
            ..params()
        })
    }

    // --- collapsed: the depth stack ---------------------------------------

    #[test]
    fn the_front_card_is_full_width_against_the_anchored_edge() {
        let out = with_collapsed(&["a", "b", "c"]);
        let front = collapsed(&out, "a");
        assert_eq!(front.x, 0.0);
        assert_eq!(front.width, FRAME_WIDTH);
        // Bottom-anchored: the front card sits the full peek reserve away from
        // the top of the stack, so both levels behind it can poke out.
        assert_eq!(front.y, MAX_PEEK_LEVELS as f64 * PEEK_OFFSET);
        assert!(front.visible);
        assert!(front.content_visible);
    }

    #[test]
    fn each_level_narrows_by_one_whole_inset_per_side() {
        let out = with_collapsed(&["a", "b", "c"]);
        assert_eq!(collapsed(&out, "a").width, 320.0);
        assert_eq!(collapsed(&out, "b").width, 304.0);
        assert_eq!(collapsed(&out, "c").width, 288.0);
    }

    // Stepped integer sizing, never a fractional scale: a width or an x that came
    // out fractional would be a border rasterized off the pixel grid.
    #[test]
    fn every_collapsed_edge_lands_on_a_whole_pixel() {
        let out = with_collapsed(&["a", "b", "c", "d"]);
        for key in ["a", "b", "c", "d"] {
            let geom = collapsed(&out, key);
            assert_eq!(geom.x, geom.x.round(), "{key} x is fractional");
            assert_eq!(geom.y, geom.y.round(), "{key} y is fractional");
            assert_eq!(geom.width, geom.width.round(), "{key} width is fractional");
        }
    }

    #[test]
    fn each_level_stays_centred_on_the_front_card() {
        let out = with_collapsed(&["a", "b", "c"]);
        for key in ["a", "b", "c"] {
            let geom = collapsed(&out, key);
            assert_eq!(geom.x * 2.0 + geom.width, FRAME_WIDTH);
        }
    }

    #[test]
    fn only_the_front_card_shows_its_content() {
        let out = with_collapsed(&["a", "b", "c"]);
        assert!(collapsed(&out, "a").content_visible);
        assert!(!collapsed(&out, "b").content_visible);
        assert!(!collapsed(&out, "c").content_visible);
    }

    #[test]
    fn the_front_card_is_in_front() {
        let out = with_collapsed(&["a", "b", "c"]);
        assert!(collapsed(&out, "a").z > collapsed(&out, "b").z);
        assert!(collapsed(&out, "b").z > collapsed(&out, "c").z);
    }

    // Expanding must not restack. The two orders are different lists (the
    // collapsed one floats a critical toast to the front, the expanded one is
    // chronological), so deriving z from the expanded index made every card
    // change stacking order on the frame the pointer arrived.
    #[test]
    fn expanding_never_restacks_the_pile() {
        let out = layout(&StackParams {
            collapsed: keys(&["c", "a", "b"]),
            expanded: keys(&["a", "b", "c"]),
            ..params()
        });
        for key in ["a", "b", "c"] {
            assert_eq!(
                expanded(&out, key).z,
                collapsed(&out, key).z,
                "{key} changed stacking order on expand"
            );
        }
        // The collapsed order is still the one that paints, so the card the stack
        // put in front stays in front all the way through the fan-out.
        assert!(expanded(&out, "c").z > expanded(&out, "a").z);
    }

    // A key that only ever appears expanded has no collapsed z to inherit, and
    // still has to get one.
    #[test]
    fn an_expanded_only_card_still_gets_a_z() {
        let out = layout(&StackParams {
            collapsed: keys(&["a"]),
            expanded: keys(&["a", "b"]),
            ..params()
        });
        assert!(out.by_key["b"].collapsed.is_none());
        assert!(expanded(&out, "b").z > i64::MIN);
    }

    // At most two levels peek; a fourth popup exists only in the count the
    // expanded stack reveals, so it takes the last level's geometry and hides
    // behind it.
    #[test]
    fn levels_clamp_at_the_peek_limit() {
        let out = with_collapsed(&["a", "b", "c", "d", "e"]);
        let third = collapsed(&out, "c");
        assert_eq!(collapsed(&out, "d").width, third.width);
        assert_eq!(collapsed(&out, "d").x, third.x);
        assert_eq!(collapsed(&out, "d").y, third.y);
        assert_eq!(collapsed(&out, "e").width, third.width);
        assert!(collapsed(&out, "d").z < third.z);
        assert!(third.visible);
        assert!(!collapsed(&out, "d").visible);
        assert!(!collapsed(&out, "e").visible);
    }

    // A card behind takes the front card's height whatever its own, so the
    // sliver past the front card is the offset and nothing else: a taller card
    // behind a short one never shows its bottom under it.
    #[test]
    fn every_level_behind_is_as_tall_as_the_front_card() {
        let out = layout(&StackParams {
            collapsed: keys(&["a", "b", "c"]),
            heights: heights(&[("a", 60.0), ("b", 140.0), ("c", 30.0)]),
            ..params()
        });
        assert_eq!(collapsed(&out, "a").height, 60.0);
        assert_eq!(collapsed(&out, "b").height, 60.0);
        assert_eq!(collapsed(&out, "c").height, 60.0);
        let front = collapsed(&out, "a");
        for key in ["b", "c"] {
            let g = collapsed(&out, key);
            assert!(g.y + g.height <= front.y + front.height, "{key} pokes out past the anchored edge");
        }
    }

    #[test]
    fn an_expanded_card_keeps_its_own_height() {
        let out = layout(&StackParams {
            collapsed: keys(&["a", "b"]),
            expanded: keys(&["a", "b"]),
            heights: heights(&[("a", 60.0), ("b", 140.0)]),
            ..params()
        });
        assert_eq!(expanded(&out, "b").height, 140.0);
    }

    #[test]
    fn a_peek_pokes_out_one_offset_per_level() {
        let out = with_collapsed(&["a", "b", "c"]);
        assert_eq!(collapsed(&out, "a").y - collapsed(&out, "b").y, PEEK_OFFSET);
        assert_eq!(collapsed(&out, "b").y - collapsed(&out, "c").y, PEEK_OFFSET);
    }

    // Top-anchored, the reveal recedes downward instead: the front card is flush
    // against the top edge and the levels behind it grow away from it.
    #[test]
    fn a_top_anchor_flips_the_reveal() {
        let out = layout(&StackParams {
            collapsed: keys(&["a", "b", "c"]),
            top: true,
            ..params()
        });
        assert_eq!(collapsed(&out, "a").y, 0.0);
        assert_eq!(collapsed(&out, "b").y, PEEK_OFFSET);
        assert_eq!(collapsed(&out, "c").y, MAX_PEEK_LEVELS as f64 * PEEK_OFFSET);
    }

    #[test]
    fn the_pile_is_the_front_card_plus_one_offset_per_level_present() {
        let two = layout(&StackParams {
            collapsed: keys(&["a", "b"]),
            heights: heights(&[("a", 90.0), ("b", 120.0)]),
            ..params()
        });
        assert_eq!(two.collapsed_height, 90.0 + PEEK_OFFSET);
        // Bottom-anchored with one level behind it, the front card sits one
        // offset down and the card behind flush against the top.
        assert_eq!(collapsed(&two, "a").y, PEEK_OFFSET);
        assert_eq!(collapsed(&two, "b").y, 0.0);
        let five = with_collapsed(&["a", "b", "c", "d", "e"]);
        assert_eq!(five.collapsed_height, MAX_PEEK_LEVELS as f64 * PEEK_OFFSET);
    }

    #[test]
    fn a_lone_card_sits_flush_against_the_anchored_edge() {
        let out = layout(&StackParams {
            collapsed: keys(&["a"]),
            heights: heights(&[("a", 90.0)]),
            ..params()
        });
        assert_eq!(collapsed(&out, "a").y, 0.0);
        assert_eq!(out.collapsed_height, 90.0);
    }

    // --- expanded: the plain list -----------------------------------------

    #[test]
    fn every_expanded_card_is_full_width_in_the_order_given() {
        let out = layout(&StackParams {
            expanded: keys(&["a", "b", "c"]),
            heights: heights(&[("a", 90.0), ("b", 60.0), ("c", 70.0)]),
            ..params()
        });
        assert_eq!(expanded(&out, "a").x, 0.0);
        assert_eq!(expanded(&out, "a").width, FRAME_WIDTH);
        assert_eq!(expanded(&out, "a").y, 0.0);
        assert_eq!(expanded(&out, "b").y, 90.0 + GAP);
        assert_eq!(expanded(&out, "c").y, 90.0 + GAP + 60.0 + GAP);
        assert_eq!(expanded(&out, "c").width, FRAME_WIDTH);
        assert!(expanded(&out, "b").content_visible);
    }

    #[test]
    fn the_expanded_column_carries_no_trailing_gap() {
        let out = layout(&StackParams {
            expanded: keys(&["a", "b"]),
            heights: heights(&[("a", 90.0), ("b", 60.0)]),
            ..params()
        });
        assert_eq!(out.expanded_height, 90.0 + GAP + 60.0);
    }

    // With no collapsed order to inherit, the expanded order decides. This is the
    // fallback rather than the rule.
    #[test]
    fn the_expanded_order_decides_when_there_is_no_collapsed_one() {
        let out = layout(&StackParams {
            expanded: keys(&["a", "b"]),
            heights: heights(&[("a", 90.0), ("b", 60.0)]),
            ..params()
        });
        assert!(expanded(&out, "a").z > expanded(&out, "b").z);
    }

    // --- edges -------------------------------------------------------------

    // A group with no slot of its own draws nothing, but the cards around it keep
    // the level they would have had.
    #[test]
    fn a_missing_slot_still_consumes_its_rank() {
        let out = layout(&StackParams {
            collapsed: vec![Some("a"), None, Some("c")],
            ..params()
        });
        assert_eq!(collapsed(&out, "a").width, 320.0);
        assert_eq!(collapsed(&out, "c").width, 288.0);
        assert_eq!(out.by_key.len(), 2);
    }

    #[test]
    fn a_missing_slot_costs_the_expanded_column_no_height() {
        let out = layout(&StackParams {
            expanded: vec![Some("a"), None, Some("c")],
            heights: heights(&[("a", 90.0), ("c", 70.0)]),
            ..params()
        });
        assert_eq!(expanded(&out, "c").y, 90.0 + GAP);
        assert_eq!(out.expanded_height, 90.0 + GAP + 70.0);
    }

    #[test]
    fn an_empty_stack_has_nothing_in_it() {
        let out = layout(&params());
        assert_eq!(out.expanded_height, 0.0);
        assert_eq!(out.collapsed_height, 0.0);
    }

    #[test]
    fn one_card_alone_carries_the_whole_pile() {
        let out = layout(&StackParams {
            collapsed: keys(&["a"]),
            expanded: keys(&["a"]),
            heights: heights(&[("a", 90.0)]),
            ..params()
        });
        assert_eq!(collapsed(&out, "a").width, FRAME_WIDTH);
        assert_eq!(expanded(&out, "a").width, FRAME_WIDTH);
        assert_eq!(out.expanded_height, 90.0);
    }

    // The rank each geometry carries, its place in the pass that laid it out. A
    // slotless entry consumes a rank the way it consumes a level, so the card
    // behind it waits its own turn rather than the missing one's.
    #[test]
    fn every_geometry_carries_its_place_in_the_pile() {
        let out = layout(&StackParams {
            collapsed: keys(&["c", "b", "a"]),
            expanded: keys(&["a", "b", "c"]),
            heights: heights(&[("a", 90.0), ("b", 80.0), ("c", 70.0)]),
            ..params()
        });
        assert_eq!(collapsed(&out, "c").rank, 0);
        assert_eq!(collapsed(&out, "b").rank, 1);
        assert_eq!(collapsed(&out, "a").rank, 2);
        assert_eq!(expanded(&out, "a").rank, 0);
        assert_eq!(expanded(&out, "c").rank, 2);
    }

    #[test]
    fn a_missing_slot_consumes_a_rank_too() {
        let out = layout(&StackParams {
            collapsed: vec![Some("a"), None, Some("c")],
            ..params()
        });
        assert_eq!(collapsed(&out, "a").rank, 0);
        assert_eq!(collapsed(&out, "c").rank, 2);
    }
}
