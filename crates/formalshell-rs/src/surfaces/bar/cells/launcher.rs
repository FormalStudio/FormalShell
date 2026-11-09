//! `bar.launcherIcon`, by default the distro's own
//! logo out of font-logos, else an icon by name. A left click opens or
//! closes the launcher.

use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Part, Summary, View};
use crate::paint::Look;

#[derive(Default)]
pub struct Launcher {
    configured: String,
    os_id: Option<String>,
}

impl Cell for Launcher {
    fn reads(&self) -> &'static [Topic] {
        &[Topic::Config]
    }

    fn read(&mut self, env: &Env) -> bool {
        let configured = env.store.config.str("bar.launcherIcon").unwrap_or("").trim().to_owned();
        if self.os_id.is_none() {
            self.os_id = Some(fs_info::about::os_id().unwrap_or_default());
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
            "formalshell" => Part::icon("command"),
            // An image path waits for the icon loader; its name stands in.
            v if v.starts_with('/') || v.starts_with("~/") || v.starts_with("file://") => {
                Part::icon("command")
            }
            v => Part::icon(v),
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
