//! Hyprland posts no event for a window moved inside its workspace: a swap
//! or a scrolling column move leaves socket2 quiet (`movewindow` is posted
//! from `CWindow::moveToWorkspace` alone, 0.56.2), so the Spaces map and
//! chips would keep the old order. Every such move comes off a bind or a
//! drag, so the windows are read again each time input settles: an input
//! idle notification fires once after a burst of keys or pointer motion, and
//! never while nothing is touched. A read that finds nothing new publishes
//! nothing (`compositor::State::apply`).

use smithay_client_toolkit::dispatch2::Dispatch2;
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::reexports::protocols::ext::idle_notify::v1::client::ext_idle_notification_v1::{
    self, ExtIdleNotificationV1,
};
use smithay_client_toolkit::reexports::protocols::ext::idle_notify::v1::client::ext_idle_notifier_v1::ExtIdleNotifierV1;

use super::App;
use crate::services::compositor;
use crate::surface::Ignore;

/// Past a bind's key release, short of a typing pause.
const SETTLE_MS: u32 = 250;

pub struct Settle {
    /// Version 2, whose input idle ignores inhibitors: a playing video
    /// would otherwise hold it off.
    notifier: Option<ExtIdleNotifierV1>,
    watch: Option<ExtIdleNotificationV1>,
}

impl Settle {
    pub fn bind(globals: &GlobalList, qh: &QueueHandle<App>) -> Self {
        Self { notifier: globals.bind(qh, 2..=2, Ignore).ok(), watch: None }
    }
}

pub struct Settled;

impl Dispatch2<ExtIdleNotificationV1, App> for Settled {
    fn event(&self, _: &mut App, _: &ExtIdleNotificationV1, event: ext_idle_notification_v1::Event, _: &Connection, _: &QueueHandle<App>) {
        if let ext_idle_notification_v1::Event::Idled = event {
            compositor::send(compositor::Command::RefreshWindows);
        }
    }
}

impl App {
    /// Needs a seat: tried once settings have loaded (`SeatState::new`
    /// binds the seats already there without a `new_seat`) and again
    /// whenever one appears.
    pub(super) fn arm_settle(&mut self) {
        if self.settle.watch.is_some() {
            return;
        }
        let (Some(notifier), Some(seat)) = (self.settle.notifier.clone(), self.seats.seats().next()) else { return };
        self.settle.watch = Some(notifier.get_input_idle_notification(SETTLE_MS, &seat, &self.qh, Settled));
    }
}
