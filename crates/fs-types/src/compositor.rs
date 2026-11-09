//! The compositor snapshot the backend publishes and every surface reads.
//! Window and workspace ids are opaque: minted by the backend that reported
//! them and taken apart only by that backend, never parsed, ordered or read
//! as numbers anywhere else.

use std::fmt;

use serde::Serialize;

use crate::IRect;

macro_rules! opaque_id {
    ($name:ident) => {
        /// Empty where there is none (nothing focused, no workspace).
        #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(id: impl Into<String>) -> Self {
                Self(id.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn is_empty(&self) -> bool {
                self.0.is_empty()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(id: String) -> Self {
                Self(id)
            }
        }

        impl From<&str> for $name {
            fn from(id: &str) -> Self {
                Self(id.to_owned())
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }
    };
}

opaque_id!(WindowId);
opaque_id!(WorkspaceId);

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: WorkspaceId,
    pub idx: i64,
    pub name: String,
    pub output: String,
    pub is_active: bool,
    pub is_focused: bool,
    pub is_urgent: bool,
    /// Its windows sit in a scrolling layout's columns, which reorder by
    /// moving a column rather than swapping two windows.
    pub columns: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    pub id: WindowId,
    pub title: String,
    pub app_id: String,
    pub initial_class: String,
    pub initial_title: String,
    pub pid: i64,
    pub workspace_id: WorkspaceId,
    /// On an overlay workspace (Hyprland's special ones) rather than a
    /// place: never listed among the workspaces, never offered to switch to.
    #[serde(skip)]
    pub special: bool,
    pub is_focused: bool,
    pub is_floating: bool,
    pub is_urgent: bool,
    pub rect: Option<IRect>,
}

/// One of the compositor's key binds, its sourced files and submaps already
/// expanded. `chord` is `mods` and `key` joined with `+`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bind {
    pub chord: String,
    pub mods: Vec<&'static str>,
    pub key: String,
    pub action: String,
    pub args: Vec<String>,
    pub title: String,
    pub submap: String,
    pub catchall: bool,
}
