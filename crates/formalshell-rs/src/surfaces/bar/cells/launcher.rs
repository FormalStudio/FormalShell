//! `bar.launcherIcon`, by default the distro's own
//! logo out of font-logos, else an icon by name. A left click opens or
//! closes the launcher.

use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Look, Part, Summary, View};

#[derive(Default)]
pub struct Launcher {
    configured: String,
    os_id: Option<String>,
}

/// os-release's `ID`, quotes stripped (Bar/launchericon.js).
fn os_id() -> String {
    let text = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    for line in text.lines() {
        if let Some(v) = line.trim().strip_prefix("ID=") {
            return v.trim().trim_matches(['"', '\'']).to_owned();
        }
    }
    String::new()
}

impl Cell for Launcher {
    fn reads(&self) -> &'static [Topic] {
        &[Topic::Config]
    }

    fn read(&mut self, env: &Env) -> bool {
        let configured = env.store.config.str("bar.launcherIcon").unwrap_or("").trim().to_owned();
        if self.os_id.is_none() {
            self.os_id = Some(os_id());
        }
        let changed = configured != self.configured;
        self.configured = configured;
        changed
    }

    fn view(&self, look: &Look) -> View {
        let value = if self.configured.is_empty() { "distro" } else { self.configured.as_str() };
        let part = match value {
            "distro" => {
                let id = self.os_id.as_deref().unwrap_or("");
                let logo = fs_theme::icons::distro_logo(id).unwrap_or(fs_theme::icons::TUX);
                Part::Glyph { text: logo.into(), family: fs_theme::icons::SYMBOLS_FAMILY }
            }
            "formalshell" => Part::Icon { name: "command".into(), dim: false, dot: false },
            // An image path waits for the icon loader; its name stands in.
            v if v.starts_with('/') || v.starts_with("~/") || v.starts_with("file://") => {
                Part::Icon { name: "command".into(), dim: false, dot: false }
            }
            v => Part::Icon { name: v.into(), dim: false, dot: false },
        };
        View::new(vec![part], look.xxs).tooltip("LAUNCHER")
    }

    fn summary(&self) -> Option<Summary> {
        Some(Summary::new("command", "Launcher", ""))
    }

    fn click(&mut self, button: Button, _: (f64, f64), _: &Env) -> Action {
        if button == Button::Left { Action::Launcher } else { Action::None }
    }
}
