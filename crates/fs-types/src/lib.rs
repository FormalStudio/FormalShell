//! Geometry and the compositor snapshot shared by every crate and the shell.

mod compositor;
mod geometry;

pub use compositor::{Window, WindowId, Workspace, WorkspaceId};
pub use geometry::{Edge, IRect, Insets, Rect, Region};
