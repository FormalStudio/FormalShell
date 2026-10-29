//! A panel whose body has not been ported yet: its own header (icon,
//! title, width and header controls) over an
//! empty body that holds the card at a working depth. Each one is replaced
//! by its own module in R3 Tasks 2 to 5, R7 and R8.

use super::{Panel, View};
use crate::store::Topic;
use crate::ui::{El, w};

pub struct StandIn {
    id: &'static str,
}

impl StandIn {
    pub fn new(id: &'static str) -> Self {
        Self { id }
    }
}

/// Each panel's `panelTitle` and `panelIcon`.
fn header(id: &str) -> (&'static str, &'static str) {
    match id {
        "appmenu" => ("App menu", "menu"),
        "calendar" => ("Calendar", "calendar"),
        "iphone" => ("iPhone", "smartphone"),
        "power" => ("Power", "zap"),
        "weather" => ("Weather", "cloud"),
        "github" => ("GitHub", "git-branch"),
        "usage" => ("Usage", "gauge"),
        "tailscale" => ("Tailscale", "network"),
        "systemupdate" => ("System update", "package"),
        "monitor" => ("Monitor", "activity"),
        "trayoverflow" => ("Tray", "ellipsis"),
        _ => ("", ""),
    }
}

impl Panel for StandIn {
    fn id(&self) -> &'static str {
        self.id
    }

    fn title(&self, _: &View) -> String {
        header(self.id).0.into()
    }

    fn icon(&self, _: &View) -> String {
        header(self.id).1.into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[]
    }

    fn width(&self, v: &View) -> f64 {
        let s = &v.theme.space;
        match self.id {
            "calendar" | "earbuds" | "media" | "monitor" => s.popup_width_wide,
            _ => s.popup_width_default,
        }
    }

    fn actions(&self, _: &View) -> Vec<El> {
        match self.id {
            "calendar" => vec![w::icon_button("chevron-left").key("previous"), w::icon_button("chevron-right").key("next")],
            _ => Vec::new(),
        }
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        w::column(0.0, vec![w::space(s.control_height * 5.0)])
    }
}
