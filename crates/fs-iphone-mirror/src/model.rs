//! Each iPhone's screen, keyed by ProductType: native pixels (portrait),
//! pixel density and the display's own corner radius in points
//! (`UIScreen._displayCornerRadius`). The physical size is pixels over ppi.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Model {
    pub name: &'static str,
    pub width: u32,
    pub height: u32,
    pub ppi: f64,
    /// Points; the screen's scale turns them into pixels.
    pub corner_pt: f64,
    pub scale: f64,
}

impl Model {
    pub fn width_mm(&self) -> f64 {
        self.width as f64 / self.ppi * 25.4
    }

    pub fn height_mm(&self) -> f64 {
        self.height as f64 / self.ppi * 25.4
    }

    /// The corner radius as a share of the screen's short side.
    pub fn corner_fraction(&self) -> f64 {
        self.corner_pt * self.scale / self.width as f64
    }
}

const fn m(name: &'static str, width: u32, height: u32, ppi: f64, corner_pt: f64, scale: f64) -> Model {
    Model { name, width, height, ppi, corner_pt, scale }
}

const TABLE: &[(&str, Model)] = &[
    ("iPhone10,3", m("iPhone X", 1125, 2436, 458.0, 39.0, 3.0)),
    ("iPhone10,6", m("iPhone X", 1125, 2436, 458.0, 39.0, 3.0)),
    ("iPhone11,2", m("iPhone XS", 1125, 2436, 458.0, 39.0, 3.0)),
    ("iPhone11,4", m("iPhone XS Max", 1242, 2688, 458.0, 39.0, 3.0)),
    ("iPhone11,6", m("iPhone XS Max", 1242, 2688, 458.0, 39.0, 3.0)),
    ("iPhone11,8", m("iPhone XR", 828, 1792, 326.0, 41.5, 2.0)),
    ("iPhone12,1", m("iPhone 11", 828, 1792, 326.0, 41.5, 2.0)),
    ("iPhone12,3", m("iPhone 11 Pro", 1125, 2436, 458.0, 39.0, 3.0)),
    ("iPhone12,5", m("iPhone 11 Pro Max", 1242, 2688, 458.0, 39.0, 3.0)),
    ("iPhone13,1", m("iPhone 12 mini", 1080, 2340, 476.0, 44.0, 3.0)),
    ("iPhone13,2", m("iPhone 12", 1170, 2532, 460.0, 47.33, 3.0)),
    ("iPhone13,3", m("iPhone 12 Pro", 1170, 2532, 460.0, 47.33, 3.0)),
    ("iPhone13,4", m("iPhone 12 Pro Max", 1284, 2778, 458.0, 53.33, 3.0)),
    ("iPhone14,4", m("iPhone 13 mini", 1080, 2340, 476.0, 44.0, 3.0)),
    ("iPhone14,5", m("iPhone 13", 1170, 2532, 460.0, 47.33, 3.0)),
    ("iPhone14,2", m("iPhone 13 Pro", 1170, 2532, 460.0, 47.33, 3.0)),
    ("iPhone14,3", m("iPhone 13 Pro Max", 1284, 2778, 458.0, 53.33, 3.0)),
    ("iPhone14,7", m("iPhone 14", 1170, 2532, 460.0, 47.33, 3.0)),
    ("iPhone14,8", m("iPhone 14 Plus", 1284, 2778, 458.0, 53.33, 3.0)),
    ("iPhone15,2", m("iPhone 14 Pro", 1179, 2556, 460.0, 55.0, 3.0)),
    ("iPhone15,3", m("iPhone 14 Pro Max", 1290, 2796, 460.0, 55.0, 3.0)),
    ("iPhone15,4", m("iPhone 15", 1179, 2556, 460.0, 55.0, 3.0)),
    ("iPhone15,5", m("iPhone 15 Plus", 1290, 2796, 460.0, 55.0, 3.0)),
    ("iPhone16,1", m("iPhone 15 Pro", 1179, 2556, 460.0, 55.0, 3.0)),
    ("iPhone16,2", m("iPhone 15 Pro Max", 1290, 2796, 460.0, 55.0, 3.0)),
    ("iPhone17,3", m("iPhone 16", 1179, 2556, 460.0, 55.0, 3.0)),
    ("iPhone17,4", m("iPhone 16 Plus", 1290, 2796, 460.0, 55.0, 3.0)),
    // Measured on an iPhone 17, not the 62 often quoted for this panel: at
    // 55 the dock's corners sit concentric inside the screen's.
    ("iPhone17,1", m("iPhone 16 Pro", 1206, 2622, 460.0, 55.0, 3.0)),
    ("iPhone17,2", m("iPhone 16 Pro Max", 1320, 2868, 460.0, 62.0, 3.0)),
    ("iPhone17,5", m("iPhone 16e", 1170, 2532, 460.0, 47.33, 3.0)),
    ("iPhone18,3", m("iPhone 17", 1206, 2622, 460.0, 55.0, 3.0)),
    ("iPhone18,1", m("iPhone 17 Pro", 1206, 2622, 460.0, 55.0, 3.0)),
    ("iPhone18,2", m("iPhone 17 Pro Max", 1320, 2868, 460.0, 62.0, 3.0)),
    ("iPhone18,4", m("iPhone Air", 1260, 2736, 460.0, 62.0, 3.0)),
];

pub fn lookup(product_type: &str) -> Option<Model> {
    TABLE.iter().find(|(k, _)| *k == product_type).map(|(_, m)| *m)
}

/// A phone the table does not know: its stream's size at the density and
/// corner share of the current generation.
pub fn fallback(width: u32, height: u32) -> Model {
    let (w, h) = (width.min(height), width.max(height));
    Model {
        name: "iPhone",
        width: w,
        height: h,
        ppi: 460.0,
        corner_pt: 55.0 * w as f64 / 1206.0,
        scale: 3.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iphone_17_is_the_6_3_inch_panel() {
        let m = lookup("iPhone18,3").unwrap();
        assert_eq!(m.name, "iPhone 17");
        let diag = (m.width_mm().powi(2) + m.height_mm().powi(2)).sqrt() / 25.4;
        assert!((diag - 6.27).abs() < 0.05, "{diag}");
        assert!((m.corner_fraction() - 165.0 / 1206.0).abs() < 1e-9);
    }
}
