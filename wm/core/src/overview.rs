//! The overview's lens: the output zoomed out onto its stack of workspaces.
//! At progress 0 it is the identity, so opening and closing start and end
//! on the desktop as it is.

use crate::geom::Rect;

/// How small a workspace is drawn when the overview is fully open.
pub const ZOOM: f64 = 0.5;
/// The space between two workspaces, as a share of one's drawn height.
const GAP: f64 = 0.08;
/// Two-finger travel, in logical pixels, that walks one workspace. A wheel
/// walks one a notch.
pub const SCROLL_STEP: f64 = 40.0;
/// A wheel notch's axis value with no steps reported: libinput's 15 degrees.
const NOTCH: f64 = 15.0;

/// Scroll carried between events until it walks a whole workspace.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Walk {
    travel: f64,
}

impl Walk {
    /// Takes one scroll event and gives the workspaces it walks, positive
    /// down. `v120` is the wheel's steps in 120ths of a notch, as ShojiWM's
    /// `discrete_y` carries them, not a count of notches; `delta` is the
    /// axis value in logical pixels. A wheel sending a part of a notch as
    /// an amount alone reports 0 steps.
    pub fn scroll(&mut self, delta: f64, v120: Option<f64>, wheel: bool) -> isize {
        self.travel += match v120.filter(|v120| *v120 != 0.0) {
            Some(v120) => v120 / 120.0 * SCROLL_STEP,
            None if wheel => delta / NOTCH * SCROLL_STEP,
            None => delta,
        };
        let steps = (self.travel / SCROLL_STEP).trunc();
        self.travel -= steps * SCROLL_STEP;
        steps as isize
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lens {
    /// The output's logical size.
    pub w: f64,
    pub h: f64,
    pub zoom: f64,
    /// From one workspace's top to the next one's.
    pub pitch: f64,
}

impl Lens {
    pub fn new(w: f64, h: f64, progress: f64) -> Self {
        let progress = progress.clamp(0.0, 1.0);
        let zoom = 1.0 - (1.0 - ZOOM) * progress;
        Self {
            w,
            h,
            zoom,
            pitch: h * zoom * (1.0 + GAP * progress),
        }
    }

    /// A workspace's screen on the output, `lane` workspaces below the one
    /// the output shows.
    pub fn panel(&self, lane: f64) -> Rect {
        let (w, h) = (self.w * self.zoom, self.h * self.zoom);
        Rect::new(
            (self.w - w) / 2.0,
            self.h / 2.0 + lane * self.pitch - h / 2.0,
            w,
            h,
        )
    }

    /// Where a box of that workspace lands. `rect` is relative to the
    /// output as the scene draws it, its lane's offset included.
    pub fn map(&self, rect: Rect, lane: f64) -> Rect {
        let panel = self.panel(lane);
        Rect::new(
            panel.x + rect.x * self.zoom,
            panel.y + (rect.y - lane * self.h) * self.zoom,
            rect.w * self.zoom,
            rect.h * self.zoom,
        )
    }

    /// A point on the output in the coordinates of the workspace in `lane`:
    /// where it would be with that workspace shown and the overview closed.
    pub fn locate(&self, x: f64, y: f64, lane: f64) -> (f64, f64) {
        let panel = self.panel(lane);
        ((x - panel.x) / self.zoom, (y - panel.y) / self.zoom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_it_changes_nothing() {
        let lens = Lens::new(1920.0, 1080.0, 0.0);
        let rect = Rect::new(8.0, 1088.0, 948.0, 1064.0);
        assert_eq!(lens.map(rect, 1.0), rect);
        assert_eq!(lens.panel(0.0), Rect::new(0.0, 0.0, 1920.0, 1080.0));
    }

    #[test]
    fn open_it_shows_the_workspace_at_half_size_with_its_neighbours() {
        let lens = Lens::new(1920.0, 1080.0, 1.0);
        assert_eq!(lens.panel(0.0), Rect::new(480.0, 270.0, 960.0, 540.0));
        let below = lens.panel(1.0);
        assert!(below.y > lens.panel(0.0).bottom());
        assert!(below.y < 1080.0);
        // A window on the next workspace sits in that workspace's panel.
        let window = lens.map(Rect::new(8.0, 1088.0, 948.0, 1064.0), 1.0);
        assert_eq!((window.x, window.y), (below.x + 4.0, below.y + 4.0));
        assert_eq!((window.w, window.h), (474.0, 532.0));
    }

    #[test]
    fn a_point_is_found_where_its_window_was_mapped_from() {
        let lens = Lens::new(1024.0, 768.0, 0.7);
        let rect = Rect::new(100.0, 768.0 * 2.0 + 50.0, 300.0, 200.0);
        let drawn = lens.map(rect, 2.0);
        let (x, y) = lens.locate(drawn.x, drawn.y, 2.0);
        assert!((x - 100.0).abs() < 1e-9 && (y - 50.0).abs() < 1e-9);
    }

    #[test]
    fn a_wheel_notch_walks_one_workspace() {
        // ShojiWM's notch: 15 px of axis value and 120 steps.
        let mut walk = Walk::default();
        assert_eq!(walk.scroll(15.0, Some(120.0), true), 1);
        assert_eq!(walk.scroll(-15.0, Some(-120.0), true), -1);
        // A high-resolution wheel's notch in eight parts.
        let walked: isize = (0..8).map(|_| walk.scroll(1.875, Some(15.0), true)).sum();
        assert_eq!(walked, 1);
        // A wheel that reports no steps: its 15 px is a notch too.
        assert_eq!(walk.scroll(15.0, None, true), 1);
        let walked: isize = (0..8).map(|_| walk.scroll(1.875, Some(0.0), true)).sum();
        assert_eq!(walked, 1);
        assert_eq!(walk, Walk::default());
    }

    #[test]
    fn two_fingers_walk_one_workspace_every_forty_pixels() {
        let mut walk = Walk::default();
        let walked: Vec<isize> = [10.0, 25.0, 10.0, 30.0, 10.0, -5.0]
            .into_iter()
            .map(|dy| walk.scroll(dy, None, false))
            .collect();
        assert_eq!(walked, [0, 0, 1, 0, 1, 0]);
        assert_eq!(walk.scroll(-40.0, None, false), -1);
    }
}
