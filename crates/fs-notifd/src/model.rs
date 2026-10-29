use std::collections::HashMap;

use zbus::zvariant::{OwnedValue, Value};

use crate::image::{self, Image};

/// Reasons from the spec's `NotificationClosed` signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    Expired = 1,
    Dismissed = 2,
    CloseRequested = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Urgency {
    Low,
    #[default]
    Normal,
    Critical,
}

/// The `value` hint: how far along a progress notification is, 0..=100.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Percent(u8);

impl Percent {
    pub fn get(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub key: String,
    pub label: String,
}

/// One live notification as the sender last described it.
///
/// The raw image hints are decoded into `image` and dropped from `hints`,
/// so a replace never keeps a large pixel buffer twice.
#[derive(Debug)]
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<Action>,
    pub urgency: Urgency,
    pub category: Option<String>,
    pub desktop_entry: String,
    pub image: Option<Image>,
    pub transient: bool,
    pub resident: bool,
    pub action_icons: bool,
    /// The sender's `expire_timeout` in milliseconds: -1 server default, 0 never.
    pub expire_timeout: i32,
    /// `x-canonical-private-synchronous`: a later notification with the same
    /// tag takes over this one's id.
    pub synchronous: Option<String>,
    pub value: Option<Percent>,
    pub sound_file: Option<String>,
    pub sound_name: Option<String>,
    pub suppress_sound: bool,
    pub hints: HashMap<String, OwnedValue>,
}

pub(crate) struct NotifyArgs {
    pub app_name: String,
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<String>,
    pub hints: HashMap<String, OwnedValue>,
    pub expire_timeout: i32,
}

fn as_str(v: &OwnedValue) -> Option<&str> {
    let v: &Value = v;
    v.downcast_ref::<&str>().ok()
}

fn as_int(v: &Value) -> Option<i64> {
    Some(match v {
        Value::U8(n) => i64::from(*n),
        Value::I16(n) => i64::from(*n),
        Value::U16(n) => i64::from(*n),
        Value::I32(n) => i64::from(*n),
        Value::U32(n) => i64::from(*n),
        Value::I64(n) => *n,
        Value::U64(n) => i64::try_from(*n).ok()?,
        _ => return None,
    })
}

fn as_bool(v: &OwnedValue) -> bool {
    let v: &Value = v;
    match v.downcast_ref::<bool>() {
        Ok(b) => b,
        Err(_) => as_int(v).is_some_and(|n| n != 0),
    }
}

/// Out-of-range values are clamped rather than dropped: a sender counting
/// past 100 is still saying it is done.
fn percent_of(v: &OwnedValue) -> Option<Percent> {
    let v: &Value = v;
    as_int(v).map(|n| Percent(n.clamp(0, 100) as u8))
}

fn urgency_of(v: &OwnedValue) -> Urgency {
    let v: &Value = v;
    match as_int(v) {
        Some(0) => Urgency::Low,
        Some(2) => Urgency::Critical,
        _ => Urgency::Normal,
    }
}

/// Image precedence: `image-data`, then `image_data`, then
/// `icon_data`, and only without decodable pixels `image-path` / `image_path`.
fn image_from_hints(hints: &HashMap<String, OwnedValue>) -> Option<Image> {
    for name in ["image-data", "image_data", "icon_data"] {
        if let Some(data) = hints.get(name).and_then(image::decode) {
            return Some(Image::Data(data));
        }
    }
    for name in ["image-path", "image_path"] {
        if let Some(path) = hints.get(name).and_then(as_str) {
            if path.is_empty() {
                return None;
            }
            return Some(Image::Path(path.strip_prefix("file://").unwrap_or(path).to_owned()));
        }
    }
    None
}

impl Notification {
    pub(crate) fn from_args(id: u32, args: NotifyArgs) -> Self {
        let NotifyArgs { app_name, app_icon, summary, body, actions, mut hints, expire_timeout } = args;
        let image = image_from_hints(&hints);
        for name in ["image-data", "image_data", "icon_data"] {
            hints.remove(name);
        }

        // An odd-length list is a malformed action set, dropped whole.
        let actions = if actions.len() % 2 == 0 {
            actions.chunks(2).map(|p| Action { key: p[0].clone(), label: p[1].clone() }).collect()
        } else {
            Vec::new()
        };

        let get_str = |k: &str| hints.get(k).and_then(as_str).map(str::to_owned);
        let flag = |k: &str| hints.get(k).is_some_and(as_bool);

        Notification {
            id,
            app_name,
            app_icon,
            summary,
            body,
            actions,
            urgency: hints.get("urgency").map(urgency_of).unwrap_or_default(),
            category: get_str("category"),
            desktop_entry: get_str("desktop-entry").unwrap_or_default(),
            image,
            transient: flag("transient"),
            resident: flag("resident"),
            action_icons: flag("action-icons"),
            expire_timeout,
            synchronous: get_str("x-canonical-private-synchronous"),
            value: hints.get("value").and_then(percent_of),
            sound_file: get_str("sound-file"),
            sound_name: get_str("sound-name"),
            suppress_sound: flag("suppress-sound"),
            hints,
        }
    }
}
