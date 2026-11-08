//! Geometry, units and the compositor snapshot shared by every crate and the
//! shell.

mod compositor;
mod geometry;
mod units;

pub use compositor::{Window, WindowId, Workspace, WorkspaceId};
pub use geometry::{Edge, IRect, Insets, Rect, Region};
pub use units::{Fraction, Percent};
