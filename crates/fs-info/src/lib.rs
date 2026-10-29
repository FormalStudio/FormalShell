//! Pure logic behind the notification, reminder, calendar, weather,
//! location, agents, usage and flake-update surfaces: plain data in, plain
//! data out, no IO.
//!
//! Months handed to and returned from this crate are 1-based (chrono's
//! convention), unlike JS's 0-based months.

pub mod agents;
pub mod calendar;
pub mod clock;
pub mod location;
pub mod notification_geometry;
pub mod notification_icon;
pub mod notifications;
pub mod reminders;
pub mod system_update;
pub mod toast_stack;
pub mod usage;
pub mod weather;
