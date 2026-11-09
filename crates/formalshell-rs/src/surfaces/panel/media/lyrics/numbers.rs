//! The lyrics pane's numbers: how a row fades, blurs and rests with its
//! distance from the lit line, how a wrapped chunk is wiped, and the glow on
//! the chunk being sung. The provider parsing and timing stay in fs-media.

use fs_js::round;
use fs_media::lyrics::{Word, chunk_end};

fn positive(x: f64) -> bool {
    x > 0.0
}

fn finite(v: Option<f64>) -> Option<f64> {
    v.filter(|x| x.is_finite())
}

/// How long the glow on a chunk takes to fade once the chunk's own span is
/// over, and the step it is reported in, so a decaying glow rewrites the
/// effect's properties twenty times rather than once a frame.
const GLOW_DECAY_SECONDS: f64 = 0.6;
const GLOW_QUANTUM: f64 = 0.05;
/// The rightbar ramp from kopuz: per-line-of-distance blur step and cap, in px,
/// sized for the panel's own (smaller) type.
const BLUR_STEP_PX: f64 = 1.1;
const BLUR_MAX_PX: f64 = 6.0;
pub const BLUR_QUANTUM_PX: f64 = 0.5;
/// The lit line rests this far down the viewport rather than at its centre.
const COMFORT_OFFSET_FRACTION: f64 = 0.42;
const DEPTH_OPACITY: [f64; 4] = [1.0, 0.7, 0.45, 0.25];

/// The 0..1 glow on chunk `index` at `t`: full while the chunk is the one
/// being sung, then linear to 0 over `GLOW_DECAY_SECONDS`. Off its raw span
/// rather than the capped wipe: a chunk held over a pause keeps its glow for as
/// long as it is the chunk being sung.
pub fn chunk_glow(words: &[Word], index: usize, line_end: Option<f64>, t: f64) -> f64 {
    let Some(chunk) = words.get(index) else {
        return 0.0;
    };
    if t < chunk.time {
        return 0.0;
    }
    let end = chunk_end(words, index, line_end);
    let glow = if t <= end {
        1.0
    } else {
        1.0 - (t - end) / GLOW_DECAY_SECONDS
    };
    round(glow.clamp(0.0, 1.0) / GLOW_QUANTUM) * GLOW_QUANTUM
}

/// The depth ramp at `distance` rows from the anchor, spread over `row_span`
/// rows instead of the table's own four entries: `row_span` is how many rows
/// the pane actually has room for on that side of the anchor, so the ramp
/// reaches its floor at the viewport's edge rather than three rows in with the
/// rest of the pane left holding rows nobody can read. The table is sampled
/// rather than replaced, and an omitted span reproduces the four entries
/// exactly.
pub fn depth_opacity(distance: f64, row_span: Option<f64>) -> f64 {
    let last = DEPTH_OPACITY.len() - 1;
    let span = finite(row_span).filter(|s| *s > 0.0).unwrap_or(last as f64);
    // Multiplied before the divide, so an integer distance against the default
    // span lands on a table entry exactly rather than a float hair away from
    // one.
    let at = (distance.abs() * last as f64 / span).min(last as f64);
    let low = at.floor() as usize;
    if low >= last {
        return DEPTH_OPACITY[last];
    }
    DEPTH_OPACITY[low] + (DEPTH_OPACITY[low + 1] - DEPTH_OPACITY[low]) * (at - low as f64)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowSpans {
    pub above: f64,
    pub below: f64,
}

/// How many rows fit between the anchored row's own resting place (the comfort
/// offset) and each end of a `viewport_height`-tall viewport, at `row_pitch` px
/// a row. The two ramps above and `blur_for` below span these rather than a
/// fixed row count, so a taller pane reads more of the song. Never under 1: a
/// pane with room for less than one row either side still has to put its
/// neighbours somewhere on the ramp.
pub fn row_spans(viewport_height: f64, row_pitch: f64) -> RowSpans {
    if !positive(viewport_height) || !positive(row_pitch) {
        return RowSpans {
            above: 1.0,
            below: 1.0,
        };
    }
    let top = viewport_height * COMFORT_OFFSET_FRACTION;
    RowSpans {
        above: f64::max(1.0, top / row_pitch),
        below: f64::max(1.0, (viewport_height - top) / row_pitch),
    }
}

/// The 0..1 fade a `height`-tall row starting at `top` carries for its
/// clearance from the ends of a `viewport_height`-tall viewport: 1 while it
/// clears both by its own height, ramping to 0 as either end reaches it. The
/// ramp is spent before the viewport's clip rather than across it, so the pane
/// never ends on half a glyph. `top` is the item's position after the column's
/// own travel.
///
/// `ramp_height` is how much clearance buys a full fade in, one row's pitch
/// rather than the row's own height: a line wrapped over three rows would
/// otherwise sit at half opacity for resting where every other line rests.
pub fn edge_fraction(top: f64, height: f64, viewport_height: f64, ramp_height: Option<f64>) -> f64 {
    if !positive(height) {
        return 0.0;
    }
    let ramp = finite(ramp_height).filter(|r| *r > 0.0).unwrap_or(height);
    let clearance = top.min(viewport_height - (top + height));
    (clearance / ramp).clamp(0.0, 1.0)
}

/// A line's depth-of-field blur for its distance (in display rows, not
/// seconds) from the anchor: kopuz's rightbar ramp, capped before the strength
/// scale is applied, quantised to `BLUR_QUANTUM_PX` so the effect's own cache
/// doesn't rebuild every frame over a sub-pixel change. `row_span` is the row
/// count the pane has room for on that side (`row_spans`), so the cap lands at
/// the viewport's edge; omitting it keeps the ramp's own `BLUR_STEP_PX` slope.
pub fn blur_for(distance: f64, strength_percent: f64, row_span: Option<f64>) -> f64 {
    let span = finite(row_span)
        .filter(|s| *s > 0.0)
        .unwrap_or(BLUR_MAX_PX / BLUR_STEP_PX);
    let capped = (distance.abs() / span).min(1.0) * BLUR_MAX_PX;
    let scaled = capped * (strength_percent / 100.0);
    round(scaled / BLUR_QUANTUM_PX) * BLUR_QUANTUM_PX
}

/// One laid-out text row of a chunk: its `y`, `height` and ink `width`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextRow {
    pub y: f64,
    pub height: f64,
    pub width: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowBand {
    pub top: f64,
    pub height: f64,
    pub width: f64,
}

/// A chunk's own text rows as bands over its box. A chunk wider than the pane
/// wraps inside its own text item, and the wipe over it has to cross that
/// break the way reading does. The bands returned tile the whole box top to
/// bottom (a band runs to the next row's own top, and the last to the box's
/// floor) so the mask never leaves a sliver of a glyph uncovered, and each
/// carries the row's own ink width for `row_wipe`.
pub fn chunk_row_bands(rows: &[TextRow], box_height: f64, box_width: f64) -> Vec<RowBand> {
    if rows.is_empty() {
        return vec![RowBand {
            top: 0.0,
            height: box_height,
            width: box_width,
        }];
    }
    rows.iter()
        .enumerate()
        .map(|(i, row)| {
            let top = if i == 0 { 0.0 } else { row.y };
            let bottom = rows.get(i + 1).map_or(box_height, |next| next.y);
            RowBand {
                top,
                height: f64::max(0.0, bottom - top),
                width: if row.width > 0.0 {
                    row.width
                } else {
                    box_width
                },
            }
        })
        .collect()
}

/// The 0..1 wipe on one band of a wrapped chunk at the chunk's own `progress`:
/// band N finishes at its own right end before band N+1 starts at its left, so
/// the lit run reads in reading order rather than as one horizontal cut across
/// every row of the block at once. The travel is weighted by each band's own
/// ink width, so the edge crosses a full row and a short last one at one speed
/// instead of spending the same time on each.
pub fn row_wipe(bands: &[RowBand], index: usize, progress: f64) -> f64 {
    if index >= bands.len() {
        return 0.0;
    }
    if bands.len() == 1 {
        return progress;
    }
    let total: f64 = bands.iter().map(|b| b.width.max(0.0)).sum();
    let own = bands[index].width.max(0.0);
    if !positive(total) || !positive(own) {
        return if progress >= 1.0 { 1.0 } else { 0.0 };
    }
    let before: f64 = bands[..index].iter().map(|b| b.width.max(0.0)).sum();
    ((progress * total - before) / own).clamp(0.0, 1.0)
}

/// The column `y` that rests an item starting at `item_y` with its own top
/// `COMFORT_OFFSET_FRACTION` down a `viewport_height`-tall viewport (kopuz's
/// comfort scroll offset, translated from a scrollTop into a translated
/// column's own y since the pane never has real scroll content). Anchored on
/// the item's top rather than its centre.
pub fn comfort_y(viewport_height: f64, item_y: f64) -> f64 {
    viewport_height * COMFORT_OFFSET_FRACTION - item_y
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(time: f64, text: &str, joins_next: bool) -> Word {
        Word { time, text: text.into(), joins_next }
    }

    // chunkGlow

    #[test]
    fn chunk_glow_is_full_while_the_chunk_is_being_sung() {
        let words = [word(1.0, "Hel", true), word(2.0, "lo", false)];
        assert_eq!(chunk_glow(&words, 0, Some(3.0), 0.5), 0.0);
        assert_eq!(chunk_glow(&words, 0, Some(3.0), 1.5), 1.0);
        assert_eq!(chunk_glow(&words, 0, Some(3.0), 2.0), 1.0);
    }

    #[test]
    fn chunk_glow_decays_linearly_once_the_chunk_is_over() {
        let words = [word(1.0, "Hel", true), word(2.0, "lo", false)];
        assert_eq!(
            chunk_glow(&words, 0, Some(3.0), 2.0 + GLOW_DECAY_SECONDS / 2.0),
            0.5
        );
        assert_eq!(
            chunk_glow(&words, 0, Some(3.0), 2.0 + GLOW_DECAY_SECONDS),
            0.0
        );
        assert_eq!(chunk_glow(&words, 0, Some(3.0), 10.0), 0.0);
    }

    // depthOpacity

    #[test]
    fn depth_opacity_of_the_active_line_is_full() {
        assert_eq!(depth_opacity(0.0, None), 1.0);
    }

    #[test]
    fn depth_opacity_falls_with_distance() {
        assert_eq!(depth_opacity(1.0, None), 0.7);
        assert_eq!(depth_opacity(-1.0, None), 0.7);
        assert_eq!(depth_opacity(2.0, None), 0.45);
        assert_eq!(depth_opacity(3.0, None), 0.25);
    }

    #[test]
    fn depth_opacity_floors_at_three_lines_away() {
        assert_eq!(depth_opacity(4.0, None), 0.25);
        assert_eq!(depth_opacity(-10.0, None), 0.25);
    }

    // A pane with room for nine rows under the anchor spends the same four table
    // entries over those nine, so the sixth row down is still readable instead of
    // sitting on the floor with three rows of the pane to spare.
    #[test]
    fn depth_opacity_spreads_over_the_rows_that_fit() {
        assert_eq!(depth_opacity(3.0, Some(9.0)), 0.7);
        assert_eq!(depth_opacity(6.0, Some(9.0)), 0.45);
        assert_eq!(depth_opacity(9.0, Some(9.0)), 0.25);
    }

    #[test]
    fn depth_opacity_floors_at_the_end_of_its_span() {
        assert_eq!(depth_opacity(12.0, Some(9.0)), 0.25);
    }

    #[test]
    fn depth_opacity_of_a_shallow_pane_falls_faster() {
        assert_eq!(depth_opacity(1.0, Some(1.5)), 0.45);
    }

    // rowSpans

    #[test]
    fn row_spans_split_the_viewport_at_the_comfort_offset() {
        let spans = row_spans(400.0, 40.0);
        assert_eq!(spans.above, 4.2);
        assert_eq!(spans.below, 5.8);
    }

    #[test]
    fn row_spans_never_fall_under_one_row() {
        let spans = row_spans(40.0, 40.0);
        assert_eq!(spans.above, 1.0);
        assert_eq!(spans.below, 1.0);
    }

    #[test]
    fn row_spans_of_an_unmeasured_pane_are_one() {
        let spans = row_spans(0.0, 0.0);
        assert_eq!(spans.above, 1.0);
        assert_eq!(spans.below, 1.0);
    }

    // edgeFraction

    #[test]
    fn edge_fraction_clear_of_both_ends_is_one() {
        assert_eq!(edge_fraction(30.0, 20.0, 100.0, None), 1.0);
    }

    #[test]
    fn edge_fraction_half_a_row_from_the_top_is_half() {
        assert_eq!(edge_fraction(10.0, 20.0, 100.0, None), 0.5);
    }

    #[test]
    fn edge_fraction_half_a_row_from_the_bottom_is_half() {
        assert_eq!(edge_fraction(70.0, 20.0, 100.0, None), 0.5);
    }

    // The row the clip would cut is already gone: the ramp runs out as the end of
    // the viewport reaches the row, not as it crosses it.
    #[test]
    fn edge_fraction_touching_an_end_is_zero() {
        assert_eq!(edge_fraction(0.0, 20.0, 100.0, None), 0.0);
        assert_eq!(edge_fraction(80.0, 20.0, 100.0, None), 0.0);
    }

    #[test]
    fn edge_fraction_fully_out_above_is_zero() {
        assert_eq!(edge_fraction(-30.0, 20.0, 100.0, None), 0.0);
    }

    #[test]
    fn edge_fraction_fully_out_below_is_zero() {
        assert_eq!(edge_fraction(110.0, 20.0, 100.0, None), 0.0);
    }

    #[test]
    fn edge_fraction_of_a_zero_height_item_is_zero() {
        assert_eq!(edge_fraction(10.0, 0.0, 100.0, None), 0.0);
    }

    // A wrapped line is three rows tall and rests where every other line rests, so
    // the fade it carries there is one row's worth, not its own.
    #[test]
    fn edge_fraction_is_spent_over_the_ramp_it_is_given() {
        assert_eq!(edge_fraction(30.0, 60.0, 200.0, Some(20.0)), 1.0);
        assert_eq!(edge_fraction(10.0, 60.0, 200.0, Some(20.0)), 0.5);
        assert_eq!(edge_fraction(0.0, 60.0, 200.0, Some(20.0)), 0.0);
        assert_eq!(edge_fraction(150.0, 60.0, 200.0, Some(20.0)), 0.0);
    }

    // blurFor

    #[test]
    fn blur_for_of_the_anchor_is_zero() {
        assert_eq!(blur_for(0.0, 100.0, None), 0.0);
    }

    #[test]
    fn blur_for_scales_by_distance_and_quantises() {
        assert_eq!(blur_for(2.0, 100.0, None), 2.0);
    }

    #[test]
    fn blur_for_caps_before_scaling_by_strength() {
        assert_eq!(blur_for(10.0, 100.0, None), 6.0);
    }

    #[test]
    fn blur_for_scales_by_strength_percent() {
        assert_eq!(blur_for(5.0, 50.0, None), 3.0);
    }

    #[test]
    fn blur_for_of_zero_strength_is_zero() {
        assert_eq!(blur_for(5.0, 0.0, None), 0.0);
    }

    #[test]
    fn blur_for_caps_at_the_end_of_its_own_span() {
        assert_eq!(blur_for(10.0, 100.0, Some(10.0)), 6.0);
        assert_eq!(blur_for(5.0, 100.0, Some(10.0)), 3.0);
    }

    // chunkRowBands

    #[test]
    fn chunk_row_bands_of_an_unmeasured_chunk_cover_the_box() {
        let bands = chunk_row_bands(&[], 20.0, 120.0);
        assert_eq!(bands.len(), 1);
        assert_eq!(
            bands[0],
            RowBand {
                top: 0.0,
                height: 20.0,
                width: 120.0
            }
        );
    }

    // The bands tile the box: the first starts at its top whatever the font's own
    // first baseline offset is, and the last runs to its floor, so no sliver of a
    // glyph is left outside the mask.
    #[test]
    fn chunk_row_bands_tile_the_whole_box() {
        let bands = chunk_row_bands(
            &[
                TextRow {
                    y: 2.0,
                    height: 18.0,
                    width: 300.0,
                },
                TextRow {
                    y: 20.0,
                    height: 18.0,
                    width: 140.0,
                },
            ],
            40.0,
            320.0,
        );
        assert_eq!(bands.len(), 2);
        assert_eq!(
            bands[0],
            RowBand {
                top: 0.0,
                height: 20.0,
                width: 300.0
            }
        );
        assert_eq!(
            bands[1],
            RowBand {
                top: 20.0,
                height: 20.0,
                width: 140.0
            }
        );
    }

    // rowWipe

    fn two_bands() -> [RowBand; 2] {
        [
            RowBand {
                top: 0.0,
                height: 20.0,
                width: 300.0,
            },
            RowBand {
                top: 20.0,
                height: 20.0,
                width: 100.0,
            },
        ]
    }

    #[test]
    fn row_wipe_of_a_single_row_is_the_chunk_progress() {
        assert_eq!(
            row_wipe(
                &[RowBand {
                    top: 0.0,
                    height: 20.0,
                    width: 120.0
                }],
                0,
                0.4
            ),
            0.4
        );
    }

    // Reading order across a break: the first row is finishing while the second has
    // not started, which is the whole defect.
    #[test]
    fn row_wipe_finishes_a_row_before_the_next_one_starts() {
        let bands = two_bands();
        assert_eq!(row_wipe(&bands, 0, 0.5), 2.0 / 3.0);
        assert_eq!(row_wipe(&bands, 1, 0.5), 0.0);
    }

    #[test]
    fn row_wipe_starts_the_second_row_once_the_first_is_full() {
        let bands = two_bands();
        assert_eq!(row_wipe(&bands, 0, 0.75), 1.0);
        assert_eq!(row_wipe(&bands, 1, 0.75), 0.0);
        assert_eq!(row_wipe(&bands, 1, 0.875), 0.5);
    }

    // The edge travels at one speed over the chunk's whole ink, so a short last row
    // takes proportionally less of the chunk's own span than a full one rather than
    // an equal share of it.
    #[test]
    fn row_wipe_weights_a_row_by_its_own_ink_width() {
        let bands = two_bands();
        assert_eq!(row_wipe(&bands, 0, 1.0), 1.0);
        assert_eq!(row_wipe(&bands, 1, 1.0), 1.0);
        assert_eq!(row_wipe(&bands, 0, 0.0), 0.0);
        assert_eq!(row_wipe(&bands, 1, 0.0), 0.0);
    }

    #[test]
    fn row_wipe_of_a_row_that_is_not_there_is_zero() {
        assert_eq!(
            row_wipe(
                &[RowBand {
                    top: 0.0,
                    height: 20.0,
                    width: 120.0
                }],
                3,
                1.0
            ),
            0.0
        );
        assert_eq!(row_wipe(&[], 0, 1.0), 0.0);
    }

    // comfortY

    #[test]
    fn comfort_y_rests_the_item_top_at_the_comfort_fraction() {
        assert_eq!(comfort_y(200.0, 50.0), 34.0);
    }

    #[test]
    fn comfort_y_of_an_item_already_there_is_zero() {
        assert_eq!(comfort_y(100.0, 42.0), 0.0);
    }
}
