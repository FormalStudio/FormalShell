//! One clipboard history row: the shape persisted in the history file and
//! shown by the clipboard route and the share list.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EntryKind {
    #[default]
    Text,
    Image,
}

impl Serialize for EntryKind {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(match self {
            EntryKind::Text => "text",
            EntryKind::Image => "image",
        })
    }
}

/// Entries persisted before images existed carry no `kind`: anything that is
/// not "image" reads as text, a pure migration with no file rewrite.
impl<'de> Deserialize<'de> for EntryKind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<EntryKind, D::Error> {
        let v = Option::<String>::deserialize(d)?;
        Ok(if v.as_deref() == Some("image") { EntryKind::Image } else { EntryKind::Text })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub kind: EntryKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captured_at: Option<i64>,
}

impl Entry {
    pub fn text(id: &str, text: &str) -> Entry {
        Entry { id: id.to_string(), kind: EntryKind::Text, text: Some(text.to_string()), ..Entry::default() }
    }

    pub fn image(id: &str, path: &str) -> Entry {
        Entry { id: id.to_string(), kind: EntryKind::Image, path: Some(path.to_string()), ..Entry::default() }
    }
}
