//! The palette engine behind the "retro" dither pass.
//!
//! The pass used to posterize each RGB channel independently onto a few
//! evenly spaced steps with a Bayer bias tipping a channel across a step
//! boundary. A flat region whose color sits near a step boundary then
//! dithered forever (a monotone wallpaper speckled end to end), and the two
//! colors being mixed were a full step apart, so every dot was maximum
//! contrast. A period-correct image instead carries a small palette chosen
//! for that image, and dithers only between entries that are already
//! neighbors:
//!
//! - [`palette`] derives up to `max_colors` colors from the image itself by
//!   median cut. A solid or near-solid source collapses to a single entry,
//!   which is the whole fix for the dots: its own color is in the palette, so
//!   there is nothing to mix it with.
//! - [`quantize`] picks each cell's nearest palette entry, and only tips it to
//!   the second nearest by the 4x4 Bayer threshold, weighted by how far
//!   between the two the cell actually sits. Cells that land on a palette
//!   entry never dither at all; cells exactly halfway land on a 50/50 checker.
//!
//! Hue survives because entries are real averages of the image's own colors:
//! nothing is forced onto a gray axis and nothing can drift to a hue the
//! source never contained. This is display-side only, nothing here is ever
//! written to disk, and matugen still reads the untouched wallpaper file, so
//! a dithered rendering cannot seed the color scheme.

/// 4x4 ordered Bayer matrix, values 0..15 mapped to a per-cell threshold.
/// Shared with the duotone pass: one matrix, one convention.
pub const BAYER: [u8; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];

/// Histogram resolution: 5 bits per channel, 32768 buckets. Coarse enough
/// that a photograph collapses to a few thousand populated buckets (which is
/// what keeps the median cut cheap), fine enough that two colors a viewer can
/// tell apart never merge. Bucket averages, not bucket centers, become the
/// palette, so this quantization never shifts a color: a solid #8e44ad image
/// yields #8e44ad exactly.
const HIST_BITS: usize = 5;
const HIST_SIZE: usize = 1 << (HIST_BITS * 3);

/// Population, and the per-channel sums that give a box its average color.
/// Parallel vectors rather than a vector of structs: one allocation per
/// channel for the whole histogram, on a path that runs over every cell of a
/// full-screen wallpaper.
struct Buckets {
    n: Vec<u64>,
    sr: Vec<u64>,
    sg: Vec<u64>,
    sb: Vec<u64>,
}

fn buckets(cells: &[u8], cell_count: usize) -> Buckets {
    let mut map: Vec<Option<usize>> = vec![None; HIST_SIZE];
    let mut h = Buckets {
        n: Vec::new(),
        sr: Vec::new(),
        sg: Vec::new(),
        sb: Vec::new(),
    };
    for px in cells.as_chunks::<3>().0.iter().take(cell_count) {
        let (r, g, b) = (px[0], px[1], px[2]);
        let key = (usize::from(r >> 3) << 10) | (usize::from(g >> 3) << 5) | usize::from(b >> 3);
        let bucket = *map[key].get_or_insert_with(|| {
            h.n.push(0);
            h.sr.push(0);
            h.sg.push(0);
            h.sb.push(0);
            h.n.len() - 1
        });
        h.n[bucket] += 1;
        h.sr[bucket] += u64::from(r);
        h.sg[bucket] += u64::from(g);
        h.sb[bucket] += u64::from(b);
    }
    h
}

/// A median-cut box: the buckets it holds, its population, its own average
/// color (population-weighted, so a box's entry sits where its pixels
/// actually are rather than at its geometric center), and the channel it is
/// longest along, the axis a split would cut.
struct Box3 {
    buckets: Vec<usize>,
    population: u64,
    axis: usize,
    range: f64,
    rgb: [u8; 3],
}

fn make_box(bucket_ids: Vec<usize>, h: &Buckets) -> Box3 {
    let (mut pop, mut tr, mut tg, mut tb) = (0u64, 0u64, 0u64, 0u64);
    let mut min = [256.0_f64; 3];
    let mut max = [-1.0_f64; 3];
    for &k in &bucket_ids {
        let c = h.n[k];
        pop += c;
        tr += h.sr[k];
        tg += h.sg[k];
        tb += h.sb[k];
        let avg = [
            h.sr[k] as f64 / c as f64,
            h.sg[k] as f64 / c as f64,
            h.sb[k] as f64 / c as f64,
        ];
        for i in 0..3 {
            min[i] = min[i].min(avg[i]);
            max[i] = max[i].max(avg[i]);
        }
    }
    let mut axis = 0;
    let mut range = max[0] - min[0];
    for i in 1..3 {
        if max[i] - min[i] > range {
            axis = i;
            range = max[i] - min[i];
        }
    }
    let pop_f = pop as f64;
    let round = |t: u64| fs_js::round(t as f64 / pop_f) as u8;
    Box3 {
        buckets: bucket_ids,
        population: pop,
        axis,
        range,
        rgb: [round(tr), round(tg), round(tb)],
    }
}

/// Cuts a box in two at the median of its population along its longest axis
/// (hence the algorithm's name): both halves hold about the same number of
/// pixels, so palette entries land where the image spends its pixels instead
/// of spreading evenly over a color volume most of the image never visits.
fn split(b: &Box3, h: &Buckets) -> Option<[Box3; 2]> {
    let sums = match b.axis {
        0 => &h.sr,
        1 => &h.sg,
        _ => &h.sb,
    };
    let mean = |k: usize| sums[k] as f64 / h.n[k] as f64;
    let mut sorted = b.buckets.clone();
    sorted.sort_by(|&a, &c| {
        mean(a)
            .partial_cmp(&mean(c))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let half = b.population as f64 / 2.0;
    let mut acc = 0.0;
    let mut cut = 0;
    for (i, &k) in sorted[..sorted.len().saturating_sub(1)].iter().enumerate() {
        acc += h.n[k] as f64;
        cut = i + 1;
        if acc >= half {
            break;
        }
    }
    if cut == 0 || cut >= sorted.len() {
        return None;
    }
    let tail = sorted.split_off(cut);
    Some([make_box(sorted, h), make_box(tail, h)])
}

/// `cells` is flat r,g,b triples (0..255), `cell_count` triples long. Returns
/// a flat r,g,b palette, at most `max_colors` entries and often fewer: an
/// image with less color than that in it gets a shorter palette rather than
/// duplicate entries a dither would then mix between for no reason. Capped at
/// 256 entries because [`quantize`] answers in byte indices; nothing near that
/// is a palette anyone would call reduced anyway.
pub fn palette(cells: &[u8], cell_count: usize, max_colors: usize) -> Vec<u8> {
    let want = max_colors.clamp(1, 256);
    let h = buckets(cells, cell_count);
    if h.n.is_empty() {
        return Vec::new();
    }

    let mut boxes = vec![make_box((0..h.n.len()).collect(), &h)];
    while boxes.len() < want {
        // Split the box with the most pixels times the widest color range:
        // population alone spends the whole palette on a large flat sky,
        // range alone spends it on a handful of stray bright pixels.
        let mut pick = None;
        let mut best = 0.0;
        for (i, b) in boxes.iter().enumerate() {
            if b.buckets.len() < 2 || b.range <= 0.0 {
                continue;
            }
            let score = b.range * b.population as f64;
            if score > best {
                best = score;
                pick = Some(i);
            }
        }
        let Some(pick) = pick else { break };
        let Some([a, b]) = split(&boxes[pick], &h) else {
            break;
        };
        boxes.splice(pick..=pick, [a, b]);
    }

    boxes.iter().flat_map(|b| b.rgb).collect()
}

/// Maps every cell onto a palette index, ordered-dithering between the two
/// nearest entries. `grid_width` is what makes the Bayer threshold a function
/// of the cell's position in the grid rather than of its position in the flat
/// array.
///
/// `t` is the cell projected onto the line from its nearest entry to its
/// second nearest, so it is 0 when the cell is the nearest color and 0.5 when
/// it sits exactly between the two (it can never exceed 0.5, past that the
/// other entry would have been the nearer one). Choosing the second entry
/// when `t` exceeds the Bayer threshold makes the proportion of stepped-up
/// cells equal `t`, so a flat area of a color the palette holds shows no
/// pattern at all and everything in between averages back to the color the
/// source actually had.
pub fn quantize(cells: &[u8], cell_count: usize, grid_width: usize, pal: &[u8]) -> Vec<u8> {
    let count = pal.len() / 3;
    let mut out = vec![0u8; cell_count];
    if count == 0 {
        return out;
    }
    let entry = |p: usize| {
        [
            f64::from(pal[p * 3]),
            f64::from(pal[p * 3 + 1]),
            f64::from(pal[p * 3 + 2]),
        ]
    };
    for (i, slot) in out.iter_mut().enumerate() {
        let px = [
            f64::from(cells[i * 3]),
            f64::from(cells[i * 3 + 1]),
            f64::from(cells[i * 3 + 2]),
        ];
        let mut near: Option<usize> = None;
        let mut near_d = f64::INFINITY;
        let mut second: Option<usize> = None;
        let mut second_d = f64::INFINITY;
        for p in 0..count {
            let e = entry(p);
            let d = (0..3).map(|c| (px[c] - e[c]) * (px[c] - e[c])).sum::<f64>();
            if d < near_d {
                second_d = near_d;
                second = near;
                near_d = d;
                near = Some(p);
            } else if d < second_d {
                second_d = d;
                second = Some(p);
            }
        }
        let (Some(near), Some(second)) = (near, second) else {
            *slot = near.unwrap_or(0) as u8;
            continue;
        };
        if near_d == 0.0 {
            *slot = near as u8;
            continue;
        }
        let (a, b) = (entry(near), entry(second));
        let e = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let len = e[0] * e[0] + e[1] * e[1] + e[2] * e[2];
        let mut t = if len > 0.0 {
            ((px[0] - a[0]) * e[0] + (px[1] - a[1]) * e[1] + (px[2] - a[2]) * e[2]) / len
        } else {
            0.0
        };
        if t <= 0.0 {
            *slot = near as u8;
            continue;
        }
        if t > 0.5 {
            t = 0.5;
        }
        let (x, y) = (i % grid_width, i / grid_width);
        let threshold = (f64::from(BAYER[(y % 4) * 4 + (x % 4)]) + 0.5) / 16.0;
        *slot = (if t > threshold { second } else { near }) as u8;
    }
    out
}

pub fn hex(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(colors: &[[u8; 3]]) -> Vec<u8> {
        colors.iter().flatten().copied().collect()
    }

    fn entries(pal: &[u8]) -> Vec<[u8; 3]> {
        pal.as_chunks::<3>().0.to_vec()
    }

    fn second_count(indices: &[u8]) -> usize {
        indices.iter().filter(|i| **i == 1).count()
    }

    // THE monotone guard: one color in, one color out, exactly, not a nearby
    // step, and not two steps mixed.
    #[test]
    fn a_solid_source_yields_its_own_single_color() {
        let c = cells(&[[142, 68, 173]; 4]);
        let pal = palette(&c, 4, 6);
        assert_eq!(pal, [142, 68, 173]);
    }

    // ...and nothing dithers against it, at any grid position: every cell
    // takes the one entry, so a monotone wallpaper paints flat.
    #[test]
    fn a_solid_source_never_dithers() {
        let count = 64;
        let c = cells(&vec![[142, 68, 173]; count]);
        let pal = palette(&c, count, 6);
        let indices = quantize(&c, count, 8, &pal);
        assert!(indices.iter().all(|i| *i == 0));
    }

    // A source with fewer distinct colors than the palette allows gets a
    // shorter palette, not padding: duplicate entries would be two names for
    // one color, and quantize would then have a "second nearest" at zero
    // distance to dither against for no reason.
    #[test]
    fn palette_never_invents_entries_the_source_lacks() {
        let c = cells(&[[0, 0, 0], [255, 255, 255], [0, 0, 0], [255, 255, 255]]);
        let pal = palette(&c, 4, 6);
        assert_eq!(pal.len(), 6);
        // Order is the median cut's own, so check membership rather than
        // position.
        let hexes: Vec<_> = entries(&pal)
            .iter()
            .map(|e| hex(e[0], e[1], e[2]))
            .collect();
        assert!(hexes.contains(&"#000000".to_string()));
        assert!(hexes.contains(&"#ffffff".to_string()));
    }

    #[test]
    fn palette_is_capped_at_max_colors() {
        let colors: Vec<[u8; 3]> = (0..64u32)
            .map(|i| [(i * 4) as u8, (255 - i * 3) as u8, ((i * 7) % 256) as u8])
            .collect();
        let pal = palette(&cells(&colors), 64, 6);
        assert_eq!(pal.len(), 18);
    }

    #[test]
    fn empty_input_yields_no_palette() {
        assert_eq!(palette(&[], 0, 6).len(), 0);
        assert_eq!(quantize(&[], 0, 1, &[]).len(), 0);
    }

    // The dither itself: a cell exactly between two entries lands on a 50/50
    // Bayer checker (half the grid positions take each), which is the correct
    // ordered dither and the densest pattern the engine can produce.
    #[test]
    fn a_color_midway_between_two_entries_checkers() {
        let pal = [0, 0, 0, 100, 100, 100];
        let c = [50u8; 16 * 3];
        let indices = quantize(&c, 16, 4, &pal);
        assert_eq!(second_count(&indices), 8);
    }

    // A cell a quarter of the way between two entries takes the far one a
    // quarter of the time, so a gradient averages back to the color the
    // source actually had instead of banding.
    #[test]
    fn dither_density_tracks_the_distance_between_entries() {
        let pal = [0, 0, 0, 100, 100, 100];
        let c = [25u8; 16 * 3];
        let indices = quantize(&c, 16, 4, &pal);
        assert_eq!(second_count(&indices), 4);
    }

    // An exact palette match short-circuits regardless of neighbors: this is
    // what keeps a flat region of a color the palette holds free of dots even
    // when the image also contains a gradient that put entries nearby.
    #[test]
    fn an_exact_match_never_takes_the_second_nearest() {
        let pal = [0, 0, 0, 10, 10, 10, 200, 200, 200];
        let c = [10u8; 16 * 3];
        let indices = quantize(&c, 16, 4, &pal);
        assert!(indices.iter().all(|i| *i == 1));
    }

    // Hue cannot drift to something the source never held: entries are
    // averages of the source's own colors, so a blue ramp quantizes to blue
    // entries.
    #[test]
    fn entries_stay_inside_the_sources_own_hue() {
        let colors: Vec<[u8; 3]> = (0..32u32)
            .map(|i| [i as u8, (i * 2) as u8, (60 + i * 6) as u8])
            .collect();
        let pal = palette(&cells(&colors), 32, 6);
        let e = entries(&pal);
        assert!(e.len() > 1);
        for entry in e {
            assert!(entry[2] > entry[1]);
            assert!(entry[1] >= entry[0]);
        }
    }

    #[test]
    fn hex_pads_single_digit_channels() {
        assert_eq!(hex(0, 0, 0), "#000000");
        assert_eq!(hex(1, 15, 255), "#010fff");
    }

    #[test]
    fn bayer_matrix_covers_every_threshold_once() {
        assert_eq!(BAYER.len(), 16);
        let mut seen = BAYER.to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 16);
    }

    // The engine half of the canvas component's retro cases (the Canvas, the
    // chunk grid and the cover crop are painted by the renderer and have no
    // counterpart here). The fixtures are the component test's own PNGs
    // decoded by hand.

    // One solid mid-tone color must come out perfectly flat: no dots on a
    // monotone source, and the color is the source's, not a nearby step.
    #[test]
    fn retro_paints_a_monotone_source_perfectly_flat() {
        let c = cells(&[[200, 40, 40]; 16]);
        let pal = palette(&c, 16, 6);
        assert_eq!(pal, [200, 40, 40]);
        assert!(quantize(&c, 16, 4, &pal).iter().all(|i| *i == 0));
    }

    // 8x8, eight full-height columns stepping (0,0,60) to (28,56,255): more
    // distinct colors than the default 6-color palette can hold, so the pass
    // has to quantize and dither, and every column is blue-dominant so any
    // entry that drifted off the source's hue is visible.
    #[test]
    fn retro_reduces_a_gradient_to_at_most_palette_size_colors() {
        let blues = [60u8, 88, 116, 144, 172, 200, 227, 255];
        let mut c = Vec::new();
        for _row in 0..8 {
            for (i, blue) in blues.iter().enumerate() {
                c.extend([(i * 4) as u8, (i * 8) as u8, *blue]);
            }
        }
        let pal = palette(&c, 64, 6);
        assert!(pal.len() / 3 <= 6);
        let indices = quantize(&c, 64, 8, &pal);
        let e = entries(&pal);
        let mut seen: Vec<[u8; 3]> = indices.iter().map(|i| e[usize::from(*i)]).collect();
        seen.sort_unstable();
        seen.dedup();
        assert!(seen.len() > 2);
        assert!(seen.len() <= 6);
        // Blue-dominant throughout: no entry drifted off the source's own
        // hue on the way through the palette.
        assert!(seen.iter().all(|px| px[2] > px[0]));
    }

    // A red source stays red: R outranks both G and B, never collapsing to a
    // gray.
    #[test]
    fn retro_preserves_hue_never_grayscale() {
        let c = cells(&[[200, 40, 40]; 16]);
        let pal = palette(&c, 16, 6);
        for i in quantize(&c, 16, 4, &pal) {
            let e = entries(&pal)[usize::from(i)];
            assert!(e[0] > e[1] && e[0] > e[2]);
        }
    }
}
