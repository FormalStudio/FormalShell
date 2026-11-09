//! Level-colour bands: each column's own post-response-curve fraction sorts
//! into one of three inks, so a bar's colour reads as its energy, not a
//! per-index rainbow. Both cuts are relative to the running peak: 0.4 is a
//! band ~37% of the peak (-8.6dB); a steady loud passage's loudest band draws
//! 0.757, so 0.85 is only crossed by a band 36% over it.

pub const LEVEL_DIM_BELOW: f64 = 0.4;
pub const LEVEL_ACCENT_FROM: f64 = 0.85;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    Dim,
    Content,
    Accent,
}

pub fn level_color_band(level: f64) -> Band {
    if level >= LEVEL_ACCENT_FROM {
        Band::Accent
    } else if level >= LEVEL_DIM_BELOW {
        Band::Content
    } else {
        Band::Dim
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs_media::visualizer::model::{agc_step, frame_peak, level_frame};

    // `seconds` of identical frames at cava's 120fps through the gain stage,
    // returning the running peak it settles on.
    fn run(mut reference: f64, frame: &[f64], seconds: f64) -> f64 {
        let dt = 1.0 / 120.0;
        for _ in 0..(seconds / dt).round() as usize {
            reference = agc_step(reference, frame_peak(frame), dt);
        }
        reference
    }

    #[test]
    fn a_steady_passage_never_reaches_the_accent_and_a_transient_over_it_does() {
        let steady_frame = [0.3, 0.15];
        let r = run(0.0, &steady_frame, 2.0);
        assert_eq!(level_color_band(level_frame(&steady_frame, r)[0]), Band::Content);
        let hit_frame = [0.9, 0.15];
        let hit = level_frame(&hit_frame, agc_step(r, frame_peak(&hit_frame), 1.0 / 120.0))[0];
        assert_eq!(level_color_band(hit), Band::Accent);
    }

    #[test]
    fn a_band_a_quarter_of_the_peak_reads_dim_and_the_peak_does_not() {
        let frame = [0.05, 0.1, 0.2, 0.4];
        let drawn = level_frame(&frame, run(0.0, &frame, 2.0));
        assert_eq!(level_color_band(drawn[1]), Band::Dim);
        assert_eq!(level_color_band(drawn[3]), Band::Content);
    }

    #[test]
    fn each_cut_starts_its_band() {
        assert_eq!(level_color_band(0.0), Band::Dim);
        assert_eq!(level_color_band(LEVEL_DIM_BELOW - 0.01), Band::Dim);
        assert_eq!(level_color_band(LEVEL_DIM_BELOW), Band::Content);
        assert_eq!(level_color_band(0.6), Band::Content);
        assert_eq!(level_color_band(LEVEL_ACCENT_FROM - 0.01), Band::Content);
        assert_eq!(level_color_band(LEVEL_ACCENT_FROM), Band::Accent);
        assert_eq!(level_color_band(1.0), Band::Accent);
    }
}
