//! Geometry, units, the compositor snapshot and the fspec command lines
//! shared by every crate and the shell.

mod backoff;
mod compositor;
pub mod fspec;
mod geometry;
mod notification;
mod units;
mod urgency;

pub use backoff::Backoff;
pub use compositor::{Bind, MAC_TAG, Window, WindowId, Workspace, WorkspaceId};
pub use notification::NotificationId;
pub use geometry::{Edge, IRect, Insets, Rect, Region};
pub use units::{BtAddress, Fraction, Percent};
pub use urgency::Urgency;
