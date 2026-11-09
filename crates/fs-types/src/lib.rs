//! Geometry, units and the compositor snapshot shared by every crate and the
//! shell.

mod compositor;
mod geometry;
mod units;
mod urgency;

pub use compositor::{Bind, Window, WindowId, Workspace, WorkspaceId};
pub use geometry::{Edge, IRect, Insets, Rect, Region};
pub use units::{BtAddress, Fraction, Percent};
pub use urgency::Urgency;
