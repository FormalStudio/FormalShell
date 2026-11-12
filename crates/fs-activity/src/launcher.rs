//! `com.canonical.Unity.LauncherEntry`: an app's `Update(app_uri,
//! properties)` signal carries only the properties that changed, so each
//! app's entry is merged property by property.

use fs_dbus::{Props, prop};
use fs_types::Fraction;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    pub count: i64,
    pub count_visible: bool,
    pub progress: Option<Fraction>,
    pub progress_visible: bool,
    pub urgent: bool,
    /// atoll's `x-atoll-badge`: a Mac app's badge that is not a count.
    pub badge: String,
}

/// One property off the signal, already typed.
#[derive(Clone, Debug)]
pub enum Prop {
    Count(i64),
    CountVisible(bool),
    /// 0..1 as the protocol sends it.
    Progress(f64),
    ProgressVisible(bool),
    Urgent(bool),
    Badge(String),
}

impl Prop {
    /// The property `name` of an `Update` signal's `a{sv}`, none for one
    /// this does not use or of another type.
    pub fn read(props: &Props, name: &str) -> Option<Prop> {
        Some(match name {
            "count" => Prop::Count(
                prop::<i64>(props, name)
                    .or_else(|| prop::<i32>(props, name).map(i64::from))
                    .or_else(|| prop::<u32>(props, name).map(i64::from))?,
            ),
            "count-visible" => Prop::CountVisible(prop(props, name)?),
            "progress" => Prop::Progress(prop(props, name)?),
            "progress-visible" => Prop::ProgressVisible(prop(props, name)?),
            "urgent" => Prop::Urgent(prop(props, name)?),
            "x-atoll-badge" => Prop::Badge(prop(props, name)?),
            _ => return None,
        })
    }
}

impl Entry {
    /// Merges the properties an `Update` signal carries.
    pub fn apply_update(&mut self, props: &Props) {
        for prop in props.keys().filter_map(|name| Prop::read(props, name)) {
            self.apply(prop);
        }
    }

    pub fn apply(&mut self, prop: Prop) {
        match prop {
            Prop::Count(n) => self.count = n,
            Prop::CountVisible(v) => self.count_visible = v,
            Prop::Progress(p) => self.progress = (!p.is_nan()).then(|| Fraction::from_fraction(p)),
            Prop::ProgressVisible(v) => self.progress_visible = v,
            Prop::Urgent(v) => self.urgent = v,
            Prop::Badge(text) => self.badge = text,
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
    fn reads_the_signal_s_typed_properties() {
        use zbus::zvariant::Value;
        let props = |items: Vec<(&str, Value<'_>)>| -> Props {
            items.into_iter().map(|(k, v)| (k.to_owned(), v.try_to_owned().unwrap())).collect()
        };
        let mut e = Entry::default();
        e.apply_update(&props(vec![("count", Value::I32(4)), ("count-visible", Value::Bool(true)), ("progress", Value::F64(0.5)), ("quicklist", Value::from("/x"))]));
        assert_eq!((e.count, e.count_visible), (4, true));
        assert_eq!(e.progress.unwrap().to_percent().rounded(), 50);
        e.apply_update(&props(vec![("progress", Value::Bool(true)), ("urgent", Value::Bool(true))]));
        assert!(e.urgent);
        assert_eq!(e.progress.unwrap().to_percent().rounded(), 50);
        assert!(Prop::read(&props(vec![("count", Value::from("x"))]), "count").is_none());
        e.apply_update(&props(vec![("x-atoll-badge", Value::from("new"))]));
        assert_eq!(e.badge, "new");
    }

    #[test]
    fn merges_partial_updates() {
        let mut e = Entry::default();
        e.apply(Prop::Progress(0.5));
        e.apply(Prop::ProgressVisible(true));
        e.apply(Prop::Urgent(true));
        e.apply(Prop::Progress(0.75));
        assert_eq!(e.progress.unwrap().to_percent().rounded(), 75);
        assert!(e.progress_visible && e.urgent && !e.count_visible);
    }
}
