//! `bar.launcherIcon`, by default the distro's own
//! logo out of font-logos, else an icon by name. A left click opens or
//! closes the launcher.

use crate::services::wants::{Source, Want};
use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Part, Summary, View};
use crate::paint::Look;

pub struct Launcher {
    /// The about service's read, for os-release's `ID`.
    want: Want,
    configured: String,
    os_id: Option<String>,
}

impl Default for Launcher {
    fn default() -> Self {
        Self { want: Want::new(Source::About), configured: String::new(), os_id: None }
    }
}

impl Cell for Launcher {
    fn visible(&mut self, on: bool) {
        self.want.hold(on);
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Config, Topic::About]
    }

    fn read(&mut self, env: &Env) -> bool {
        let configured = crate::settings::LAUNCHER_ICON.get(&env.store.config).trim().to_owned();
        let os_id = env.store.about.as_ref().and_then(|a| a.os_id.clone());
        let changed = configured != self.configured || os_id != self.os_id;
        self.configured = configured;
        self.os_id = os_id;
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
