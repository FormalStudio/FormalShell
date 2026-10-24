//! `$XDG_STATE_HOME/formalshell/state.json`, the runtime-mutable counterpart
//! to settings.json and the only file the shell writes. The service thread owns the working copy, applies
//! setters in the order they arrive, publishes what moved, and writes the file
//! once per burst of setters.

use std::cell::RefCell;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Duration;

use async_io::Timer;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use super::watch::Watch;
use crate::runtime::Ctx;
use crate::store;

/// Setters landing within this of each other share one write.
const DEBOUNCE: Duration = Duration::from_millis(20);

#[derive(Clone, Debug, PartialEq)]
pub struct Data {
    pub wallpaper: String,
    pub mode: String,
    /// `{ mode, untilMs }` while a manual flip snoozes `theme.mode: "auto"`.
    pub mode_override: Value,
    pub dnd: bool,
    pub calendar_birth_year: i64,
    pub calendar_life_expectancy: i64,
    pub clock_format: String,
    pub app_launches: Value,
    pub emoji_uses: Value,
    pub reminders: Value,
    pub battery_show_percent: Value,
    pub overnight: Value,
    pub lights: Value,
    pub hdr: Value,
    /// The login the boot sequence last played for (`wayland::boot`).
    pub boot: String,
}

impl Default for Data {
    fn default() -> Self {
        Self {
            wallpaper: String::new(),
            mode: "dark".into(),
            mode_override: Value::Null,
            dnd: false,
            calendar_birth_year: 0,
            calendar_life_expectancy: 0,
            clock_format: String::new(),
            app_launches: Value::Array(vec![]),
            emoji_uses: Value::Array(vec![]),
            reminders: Value::Array(vec![]),
            battery_show_percent: Value::Null,
            overnight: Value::Null,
            lights: Value::Null,
            hdr: Value::Null,
            boot: String::new(),
        }
    }
}

/// One key of the file. Setters are lists of these so a pair that has to land
/// together (a wallpaper and its mode) is one write.
#[derive(Clone, Debug, PartialEq)]
pub enum Field {
    Wallpaper(String),
    Mode(String),
    ModeOverride(Value),
    Dnd(bool),
    CalendarBirthYear(i64),
    CalendarLifeExpectancy(i64),
    ClockFormat(String),
    AppLaunches(Value),
    EmojiUses(Value),
    Reminders(Value),
    BatteryShowPercent(Value),
    Overnight(Value),
    Lights(Value),
    Hdr(Value),
    Boot(String),
}

#[derive(Debug, Default, PartialEq)]
pub struct Diff(pub Vec<Field>);

impl Field {
    fn apply(self, data: &mut Data) {
        match self {
            Field::Wallpaper(v) => data.wallpaper = v,
            Field::Mode(v) => data.mode = v,
            Field::ModeOverride(v) => data.mode_override = v,
            Field::Dnd(v) => data.dnd = v,
            Field::CalendarBirthYear(v) => data.calendar_birth_year = v,
            Field::CalendarLifeExpectancy(v) => data.calendar_life_expectancy = v,
            Field::ClockFormat(v) => data.clock_format = v,
            Field::AppLaunches(v) => data.app_launches = v,
            Field::EmojiUses(v) => data.emoji_uses = v,
            Field::Reminders(v) => data.reminders = v,
            Field::BatteryShowPercent(v) => data.battery_show_percent = v,
            Field::Overnight(v) => data.overnight = v,
            Field::Lights(v) => data.lights = v,
            Field::Hdr(v) => data.hdr = v,
            Field::Boot(v) => data.boot = v,
        }
    }
}

impl Data {
    fn fields(&self) -> [Field; 15] {
        [
            Field::Wallpaper(self.wallpaper.clone()),
            Field::Mode(self.mode.clone()),
            Field::ModeOverride(self.mode_override.clone()),
            Field::Dnd(self.dnd),
            Field::CalendarBirthYear(self.calendar_birth_year),
            Field::CalendarLifeExpectancy(self.calendar_life_expectancy),
            Field::ClockFormat(self.clock_format.clone()),
            Field::AppLaunches(self.app_launches.clone()),
            Field::EmojiUses(self.emoji_uses.clone()),
            Field::Reminders(self.reminders.clone()),
            Field::BatteryShowPercent(self.battery_show_percent.clone()),
            Field::Overnight(self.overnight.clone()),
            Field::Lights(self.lights.clone()),
            Field::Hdr(self.hdr.clone()),
            Field::Boot(self.boot.clone()),
        ]
    }

    /// The fields of `self` that differ from `old`.
    fn diff(&self, old: &Data) -> Diff {
        Diff(self.fields().into_iter().zip(old.fields()).filter(|(n, o)| n != o).map(|(n, _)| n).collect())
    }

    fn set(&mut self, fields: Vec<Field>) -> Diff {
        let old = self.clone();
        for field in fields {
            field.apply(self);
        }
        self.diff(&old)
    }

    /// A key missing or of the wrong type takes its default, and a key the
    /// shell does not know is dropped on the next write, as `JsonAdapter` does.
    fn parse(text: &str) -> Result<Data, serde_json::Error> {
        let value: Value = serde_json::from_str(text)?;
        let empty = Map::new();
        let object = value.as_object().unwrap_or(&empty);
        let mut data = Data::default();
        let take = |key: &str| object.get(key).cloned();
        fn typed<T: DeserializeOwned>(v: Option<Value>) -> Option<T> {
            serde_json::from_value(v?).ok()
        }
        fn array(v: Option<Value>) -> Option<Value> {
            v.filter(Value::is_array)
        }
        data.wallpaper = typed(take("wallpaper")).unwrap_or(data.wallpaper);
        data.mode = typed(take("mode")).unwrap_or(data.mode);
        data.mode_override = take("modeOverride").unwrap_or(Value::Null);
        data.dnd = typed(take("dnd")).unwrap_or(data.dnd);
        data.calendar_birth_year = typed(take("calendarBirthYear")).unwrap_or(0);
        data.calendar_life_expectancy = typed(take("calendarLifeExpectancy")).unwrap_or(0);
        data.clock_format = typed(take("clockFormat")).unwrap_or(data.clock_format);
        data.app_launches = array(take("appLaunches")).unwrap_or(data.app_launches);
        data.emoji_uses = array(take("emojiUses")).unwrap_or(data.emoji_uses);
        data.reminders = array(take("reminders")).unwrap_or(data.reminders);
        data.battery_show_percent = take("batteryShowPercent").unwrap_or(Value::Null);
        data.overnight = take("overnight").unwrap_or(Value::Null);
        data.lights = take("lights").unwrap_or(Value::Null);
        data.hdr = take("hdr").unwrap_or(Value::Null);
        data.boot = typed(take("boot")).unwrap_or_default();
        Ok(data)
    }

    /// Four-space indent and sorted keys, the shape `JsonAdapter` writes.
    fn to_text(&self) -> String {
        let mut map = Map::new();
        map.insert("wallpaper".into(), self.wallpaper.clone().into());
        map.insert("mode".into(), self.mode.clone().into());
        map.insert("modeOverride".into(), self.mode_override.clone());
        map.insert("dnd".into(), self.dnd.into());
        map.insert("calendarBirthYear".into(), self.calendar_birth_year.into());
        map.insert("calendarLifeExpectancy".into(), self.calendar_life_expectancy.into());
        map.insert("clockFormat".into(), self.clock_format.clone().into());
        map.insert("appLaunches".into(), self.app_launches.clone());
        map.insert("emojiUses".into(), self.emoji_uses.clone());
        map.insert("reminders".into(), self.reminders.clone());
        map.insert("batteryShowPercent".into(), self.battery_show_percent.clone());
        map.insert("overnight".into(), self.overnight.clone());
        map.insert("lights".into(), self.lights.clone());
        map.insert("hdr".into(), self.hdr.clone());
        map.insert("boot".into(), self.boot.clone().into());
        let mut doc = Value::Object(map);
        sort_keys(&mut doc);
        let mut out = Vec::new();
        let format = serde_json::ser::PrettyFormatter::with_indent(b"    ");
        let mut ser = serde_json::Serializer::with_formatter(&mut out, format);
        serde::Serialize::serialize(&doc, &mut ser).expect("a Value serializes");
        out.push(b'\n');
        String::from_utf8(out).expect("serde_json writes UTF-8")
    }
}

/// Every object's keys in order at every depth: serde_json keeps insertion
/// order once any crate in the build asks for `preserve_order`.
fn sort_keys(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.sort_keys();
            map.values_mut().for_each(sort_keys);
        }
        Value::Array(items) => items.iter_mut().for_each(sort_keys),
        _ => {}
    }
}

/// The store's slice.
#[derive(Default)]
pub struct State {
    pub data: Data,
    /// True once state.json has been read, or found absent.
    pub loaded: bool,
}

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        let mut changed = !self.loaded;
        self.loaded = true;
        let mut fields = diff.0;
        changed |= !fields.is_empty();
        for field in fields.drain(..) {
            field.apply(&mut self.data);
        }
        changed
    }
}

pub fn state_path() -> PathBuf {
    let dir = match std::env::var("XDG_STATE_HOME") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/state"),
    };
    dir.join("formalshell").join("state.json")
}

/// Temp file beside the target, then a rename over it, so a reader (or a
/// crash) never sees half a file.
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

static SETTERS: OnceLock<async_channel::Sender<Vec<Field>>> = OnceLock::new();

/// Queues `fields` as one write. Callable from any thread; dropped before the
/// service has started.
pub fn set(fields: Vec<Field>) {
    if let Some(tx) = SETTERS.get() {
        let _ = tx.try_send(fields);
    }
}

/// Mode goes first so a retheme the wallpaper change triggers already reads the
/// final mode. `mode` is the picker's Dark/Light set, ignored when it is
/// neither.
pub fn set_wallpaper(path: &str, mode: Option<&str>) {
    let mut fields = Vec::new();
    if let Some(mode @ ("dark" | "light")) = mode {
        fields.push(Field::Mode(mode.into()));
    }
    fields.push(Field::Wallpaper(path.into()));
    set(fields);
}

/// Both keys in one write: a half-set pair has no valid life fraction.
#[allow(dead_code)]
pub fn set_calendar_life_progress(birth_year: i64, life_expectancy: i64) {
    set(vec![Field::CalendarBirthYear(birth_year), Field::CalendarLifeExpectancy(life_expectancy)]);
}

/// What the service task and its watcher share. Both run on the one service
/// thread, so a `RefCell` is enough.
struct Shared {
    data: Data,
    /// A setter has been applied and its write has not finished: a read of
    /// the file now would roll the setter back.
    dirty: bool,
    /// The text of the last write, so the watch event our own rename raises
    /// reads as no change.
    written: Option<String>,
}

fn read(ctx: &Ctx, path: &Path) -> impl Future<Output = Option<Option<String>>> + use<> {
    let path = path.to_path_buf();
    ctx.pool().run(move || match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            eprintln!("State: failed to read state.json: {e}");
            None
        }
    })
}

pub async fn run(ctx: Ctx) {
    let path = state_path();
    let (tx, rx) = async_channel::unbounded::<Vec<Field>>();
    let _ = SETTERS.set(tx);

    let shared = Rc::new(RefCell::new(Shared { data: Data::default(), dirty: false, written: None }));

    // Absent file: write the defaults.
    let initial = read(&ctx, &path).await.flatten();
    let absent = initial.is_none();
    if let Some(text) = &initial {
        match Data::parse(text) {
            Ok(data) => shared.borrow_mut().data = data,
            Err(e) => eprintln!("State: failed to parse state.json: {e}"),
        }
        shared.borrow_mut().written = Some(text.clone());
    }
    let first = shared.borrow().data.diff(&Data::default());
    ctx.publish(store::Diff::State(first));
    if absent {
        write(&ctx, &path, &shared).await;
    }

    ctx.spawn({
        let (ctx, path, shared) = (ctx.clone(), path.clone(), shared.clone());
        async move {
            let mut watch = match Watch::new(path.clone()) {
                Ok(watch) => watch,
                Err(e) => return eprintln!("State: no watch on {}: {e}", path.display()),
            };
            loop {
                watch.changed().await;
                let text = read(&ctx, &path).await.flatten();
                let mut shared = shared.borrow_mut();
                if shared.dirty || text.is_none() || text == shared.written {
                    continue;
                }
                let text = text.unwrap();
                match Data::parse(&text) {
                    Ok(next) => {
                        let diff = next.diff(&shared.data);
                        shared.data = next;
                        shared.written = Some(text);
                        if !diff.0.is_empty() {
                            ctx.publish(store::Diff::State(diff));
                        }
                    }
                    Err(e) => eprintln!("State: failed to parse state.json: {e}"),
                }
            }
        }
    });

    while let Ok(mut fields) = rx.recv().await {
        shared.borrow_mut().dirty = true;
        // Collect the burst before touching the file.
        Timer::after(DEBOUNCE).await;
        while let Ok(more) = rx.try_recv() {
            fields.extend(more);
        }
        let diff = shared.borrow_mut().data.set(fields);
        if !diff.0.is_empty() {
            ctx.publish(store::Diff::State(diff));
            write(&ctx, &path, &shared).await;
        }
        shared.borrow_mut().dirty = false;
    }
}

async fn write(ctx: &Ctx, path: &Path, shared: &Rc<RefCell<Shared>>) {
    let text = shared.borrow().data.to_text();
    let target = path.to_path_buf();
    let body = text.clone();
    match ctx.pool().run(move || write_atomic(&target, &body)).await {
        Some(Ok(())) => shared.borrow_mut().written = Some(text),
        Some(Err(e)) => eprintln!("State: failed to write state.json: {e}"),
        None => eprintln!("State: the write of state.json panicked"),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn defaults_match_the_adapter() {
        let d = Data::default();
        assert_eq!((d.wallpaper.as_str(), d.mode.as_str(), d.dnd), ("", "dark", false));
        assert_eq!(d.app_launches, json!([]));
        assert_eq!(d.mode_override, Value::Null);
        assert_eq!((d.calendar_birth_year, d.calendar_life_expectancy), (0, 0));
    }

    #[test]
    fn written_file_has_every_key_sorted_with_four_space_indent() {
        let text = Data::default().to_text();
        assert!(text.starts_with("{\n    \"appLaunches\": [],\n"), "{text}");
        assert!(text.ends_with("}\n"));
        let keys: Vec<String> = serde_json::from_str::<Map<String, Value>>(&text).unwrap().keys().cloned().collect();
        assert_eq!(keys.len(), 15);
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
    }

    #[test]
    fn round_trip_keeps_every_field() {
        let data = Data {
            wallpaper: "/w.png".into(),
            mode: "light".into(),
            mode_override: json!({"mode": "dark", "untilMs": 5}),
            dnd: true,
            calendar_birth_year: 1990,
            calendar_life_expectancy: 80,
            clock_format: "%H".into(),
            app_launches: json!([{"id": "a", "count": 2, "lastMs": 3}]),
            emoji_uses: json!([{"id": "emoji.x", "count": 1, "lastMs": 1}]),
            reminders: json!([{"id": 1, "message": "m", "setAt": 1, "dueAt": 2}]),
            battery_show_percent: json!(false),
            overnight: json!({"backlight": 40}),
            lights: json!({"source": "palette"}),
            hdr: json!({"DP-1": {"prior": {}}}),
            boot: "b0/s1".into(),
        };
        assert_eq!(Data::parse(&data.to_text()).unwrap(), data);
    }

    #[test]
    fn unknown_keys_are_dropped_and_wrong_types_take_the_default() {
        let d = Data::parse(r#"{"future":1,"dnd":"yes","mode":"light","appLaunches":{"a":1},"calendarBirthYear":"x"}"#).unwrap();
        assert_eq!(d.mode, "light");
        assert!(!d.dnd);
        assert_eq!(d.app_launches, json!([]));
        assert_eq!(d.calendar_birth_year, 0);
        assert!(!d.to_text().contains("future"));
    }

    #[test]
    fn a_non_object_file_is_all_defaults_and_bad_json_is_an_error() {
        assert_eq!(Data::parse("[]").unwrap(), Data::default());
        assert!(Data::parse("{").is_err());
    }

    #[test]
    fn set_reports_only_what_moved() {
        let mut data = Data::default();
        assert_eq!(data.set(vec![Field::Dnd(false), Field::Mode("dark".into())]), Diff(vec![]));
        let diff = data.set(vec![Field::Dnd(true), Field::ClockFormat("x".into())]);
        assert_eq!(diff, Diff(vec![Field::Dnd(true), Field::ClockFormat("x".into())]));
    }

    #[test]
    fn store_slice_applies_diffs_and_flags_the_first_one() {
        let mut slice = State::default();
        assert!(slice.apply(Diff(vec![])), "the first diff is the load");
        assert!(!slice.apply(Diff(vec![])));
        assert!(slice.apply(Diff(vec![Field::Dnd(true)])));
        assert!(slice.data.dnd);
    }

    #[test]
    fn atomic_write_replaces_the_file_and_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("formalshell").join("state.json");
        write_atomic(&path, "one\n").unwrap();
        write_atomic(&path, "two\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two\n");
        let left: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(left, ["state.json"]);
    }

    #[test]
    fn wallpaper_and_mode_land_mode_first() {
        let (tx, rx) = async_channel::unbounded();
        let _ = SETTERS.set(tx);
        set_wallpaper("/w.png", Some("light"));
        set_wallpaper("/x.png", Some("sepia"));
        assert_eq!(rx.try_recv().unwrap(), vec![Field::Mode("light".into()), Field::Wallpaper("/w.png".into())]);
        assert_eq!(rx.try_recv().unwrap(), vec![Field::Wallpaper("/x.png".into())]);
    }
}
