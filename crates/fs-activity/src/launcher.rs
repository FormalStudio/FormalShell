//! `com.canonical.Unity.LauncherEntry`: an app's `Update(app_uri,
//! properties)` signal carries only the properties that changed, so each
//! app's entry is merged property by property.

use crate::progress::Fraction;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    pub count: i64,
    pub count_visible: bool,
    pub progress: Option<Fraction>,
    pub progress_visible: bool,
    pub urgent: bool,
}

/// One property off the signal, already typed.
#[derive(Clone, Copy, Debug)]
pub enum Prop {
    Count(i64),
    CountVisible(bool),
    /// 0..1 as the protocol sends it.
    Progress(f64),
    ProgressVisible(bool),
    Urgent(bool),
}

impl Entry {
    pub fn apply(&mut self, prop: Prop) {
        match prop {
            Prop::Count(n) => self.count = n,
            Prop::CountVisible(v) => self.count_visible = v,
            Prop::Progress(p) => self.progress = Fraction::from_unit(p),
            Prop::ProgressVisible(v) => self.progress_visible = v,
            Prop::Urgent(v) => self.urgent = v,
        }
    }
}

/// `application://firefox.desktop` as the desktop entry id `firefox`.
pub fn desktop_id(app_uri: &str) -> &str {
    let id = app_uri.strip_prefix("application://").unwrap_or(app_uri);
    id.strip_suffix(".desktop").unwrap_or(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_to_desktop_id() {
        assert_eq!(desktop_id("application://org.mozilla.firefox.desktop"), "org.mozilla.firefox");
        assert_eq!(desktop_id("firefox.desktop"), "firefox");
        assert_eq!(desktop_id("firefox"), "firefox");
    }

    #[test]
    fn merges_partial_updates() {
        let mut e = Entry::default();
        e.apply(Prop::Progress(0.5));
        e.apply(Prop::ProgressVisible(true));
        e.apply(Prop::Urgent(true));
        e.apply(Prop::Progress(0.75));
        assert_eq!(e.progress.unwrap().as_percent(), 75);
        assert!(e.progress_visible && e.urgent && !e.count_visible);
    }
}
