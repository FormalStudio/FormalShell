//! Where the live bar's chrome sits on its output: the line cards bud off,
//! what each edge gives up to the strip or a frame ring, the strip itself
//! and the outputs a fullscreen window has taken. Made off the live bar on
//! each read, so a compositor event and the read after it agree.

use fs_types::Edge;

use crate::scene::IRect;
use crate::surfaces::bar::Bar;
use crate::surfaces::card::Ends;
use crate::surfaces::panel::host::Place;

pub struct Chrome {
    /// The bar's edge.
    pub edge: Edge,
    /// The live bar's output's logical size.
    pub output: (f64, f64),
    /// The strip's extent across its edge.
    thickness: i32,
    /// The bar window's extent across its edge: the strip and any fillets.
    across: i32,
    framed: bool,
    /// The frame ring's band, 0 on a bare edge.
    frame: f64,
    radius: f64,
    /// The live bar's output's name.
    name: String,
    /// The bar's window is down under a fullscreen window.
    hidden: bool,
    /// The outputs whose chrome a fullscreen window has taken.
    covered: Vec<String>,
}

impl Chrome {
    /// `covered` is every output a fullscreen window holds, or none when
    /// `fullscreen.hideChrome` keeps the chrome over it.
    pub fn new(bar: &Bar, output: (f64, f64), frame_radius: f64, covered: Vec<String>) -> Self {
        let framed = bar.framed();
        Self {
            edge: bar.edge(),
            output,
            thickness: bar.thickness(),
            across: bar.across(),
            framed,
            frame: if framed { bar.frame_thickness() } else { 0.0 },
            radius: if framed { frame_radius } else { 0.0 },
            name: bar.output.clone(),
            hidden: bar.hidden(),
            covered,
        }
    }

    /// The line a panel hangs off, in from the bar's edge.
    pub fn line(&self) -> f64 {
        f64::from(self.thickness)
    }

    /// What `edge` gives up: the strip on the bar's own edge, the frame
    /// ring's band on the others, nothing on a bare edge.
    pub fn inset(&self, edge: Edge) -> f64 {
        if edge == self.edge { self.line() } else { self.frame }
    }

    /// The ends of a line along `edge`: the output's length that way, less
    /// a frame ring's band at either end, and the corner its lines meet in.
    pub fn ends(&self, edge: Edge) -> Ends {
        let along = if edge.is_vertical() { self.output.1 } else { self.output.0 };
        Ends { along, inset_start: self.frame, inset_end: self.frame, radius: self.radius }
    }

    /// The line a modal card buds off, how far in it lies, and its ends.
    /// Modal cards bud off the top line, the strip's on a top bar, else the
    /// frame ring's or the bare edge; an unframed side bar's inner edge is
    /// the one other line they take.
    pub fn modal_ends(&self) -> (Edge, f64, Ends) {
        let (edge, at) = if self.hidden {
            (Edge::Top, 0.0)
        } else if self.edge.is_vertical() && !self.framed {
            (self.edge, self.line())
        } else {
            (Edge::Top, self.inset(Edge::Top))
        };
        (edge, at, self.ends(edge))
    }

    /// Where a panel hangs: off the bar's line, centred on `anchor` or at
    /// the line's end.
    pub fn place(&self, anchor: Option<f64>) -> Place {
        Place { edge: self.edge, output: self.output, line_at: self.line(), ends: self.ends(self.edge), far_inset: self.frame, anchor }
    }

    /// The strip in output coordinates, which an open panel's full-output
    /// surface leaves to the bar.
    pub fn strip(&self) -> IRect {
        let (ow, oh) = (self.output.0 as i32, self.output.1 as i32);
        let t = self.thickness;
        match self.edge {
            Edge::Bottom => IRect::new(0, oh - t, ow, t),
            Edge::Left => IRect::new(0, 0, t, oh),
            Edge::Right => IRect::new(ow - t, 0, t, oh),
            Edge::Top => IRect::new(0, 0, ow, t),
        }
    }

    /// The top left, on the output, of a box `depth` deep against the bar's
    /// edge.
    pub fn origin(&self, depth: i32) -> (i32, i32) {
        let (w, h) = self.output;
        match self.edge {
            Edge::Bottom => (0, h as i32 - depth),
            Edge::Right => (w as i32 - depth, 0),
            Edge::Top | Edge::Left => (0, 0),
        }
    }

    /// The bar window's top left on its output: the whole output under a
    /// frame, else its edge's strip with any fillets past it.
    pub fn window(&self) -> (i32, i32) {
        if self.framed { (0, 0) } else { self.origin(self.across) }
    }

    /// Where a surface of `area` sits on an output of `output` with the
    /// bar's band taken out: the band's whole extent on its own side, any
    /// other inset split evenly.
    pub fn offset(&self, output: (f64, f64), area: (f64, f64)) -> (f64, f64) {
        let (dx, dy) = ((output.0 - area.0).max(0.0), (output.1 - area.1).max(0.0));
        let x = match self.edge {
            Edge::Left => dx,
            Edge::Right => 0.0,
            _ => dx / 2.0,
        };
        let y = match self.edge {
            Edge::Top => dy,
            Edge::Bottom => 0.0,
            _ => dy / 2.0,
        };
        (x, y)
    }

    /// Whether a fullscreen window has taken `output`'s chrome.
    pub fn covered(&self, output: &str) -> bool {
        self.covered.iter().any(|o| o == output)
    }

    /// Whether a fullscreen window has taken the live bar's output.
    pub fn bar_covered(&self) -> bool {
        self.covered(&self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chrome(edge: Edge, framed: bool) -> Chrome {
        Chrome {
            edge,
            output: (1920.0, 1080.0),
            thickness: 40,
            across: 46,
            framed,
            frame: if framed { 6.0 } else { 0.0 },
            radius: if framed { 12.0 } else { 0.0 },
            name: "eDP-1".into(),
            hidden: false,
            covered: Vec::new(),
        }
    }

    #[test]
    fn the_band_offset_follows_the_bars_edge() {
        assert_eq!(chrome(Edge::Left, false).offset((1920.0, 1080.0), (1766.0, 1080.0)), (154.0, 0.0));
        assert_eq!(chrome(Edge::Top, false).offset((1920.0, 1080.0), (1920.0, 1040.0)), (0.0, 40.0));
        assert_eq!(chrome(Edge::Bottom, false).offset((1920.0, 1080.0), (1920.0, 1040.0)), (0.0, 0.0));
    }

    #[test]
    fn modal_cards_bud_off_the_top_line_or_a_bare_side() {
        let at = |c: &Chrome| {
            let (edge, at, ends) = c.modal_ends();
            (edge, at, ends.inset_start)
        };
        assert_eq!(at(&chrome(Edge::Top, true)), (Edge::Top, 40.0, 6.0));
        assert_eq!(at(&chrome(Edge::Bottom, true)), (Edge::Top, 6.0, 6.0));
        assert_eq!(at(&chrome(Edge::Bottom, false)), (Edge::Top, 0.0, 0.0));
        assert_eq!(at(&chrome(Edge::Left, false)), (Edge::Left, 40.0, 0.0));
        assert_eq!(at(&chrome(Edge::Right, true)), (Edge::Top, 6.0, 6.0));
        let hidden = Chrome { hidden: true, ..chrome(Edge::Left, false) };
        assert_eq!(at(&hidden), (Edge::Top, 0.0, 0.0));
    }

    #[test]
    fn the_strip_and_the_window_sit_on_the_bars_edge() {
        assert_eq!(chrome(Edge::Bottom, false).strip(), IRect::new(0, 1040, 1920, 40));
        assert_eq!(chrome(Edge::Right, false).window(), (1874, 0));
        assert_eq!(chrome(Edge::Right, true).window(), (0, 0));
        assert_eq!(chrome(Edge::Bottom, true).inset(Edge::Top), 6.0);
    }
}
