//! Tells the compositor service each time input settles: an input idle
//! notification fires once after a burst of keys or pointer motion, and
//! never while nothing is touched. The backend decides what that is worth
//! (`compositor::Command::InputSettled`); a read that finds nothing new
//! publishes nothing (`compositor::State::apply`).

use smithay_client_toolkit::dispatch2::Dispatch2;
use smithay_client_toolkit::reexports::client::{Connection, Proxy, QueueHandle};
use smithay_client_toolkit::reexports::protocols::ext::idle_notify::v1::client::ext_idle_notification_v1::{
    self, ExtIdleNotificationV1,
};

use super::App;
use crate::services::compositor;

/// Past a bind's key release, short of a typing pause.
const SETTLE_MS: u32 = 250;

#[derive(Default)]
pub struct Settle {
    watch: Option<ExtIdleNotificationV1>,
}

pub struct Settled;

impl Dispatch2<ExtIdleNotificationV1, App> for Settled {
    fn event(&self, _: &mut App, _: &ExtIdleNotificationV1, event: ext_idle_notification_v1::Event, _: &Connection, _: &QueueHandle<App>) {
        if let ext_idle_notification_v1::Event::Idled = event {
            compositor::send(compositor::Command::InputSettled);
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
        // Version 2's input idle ignores inhibitors: a playing video would
        // otherwise hold it off.
        let notifier = self.idle_notifier.clone().filter(|n| n.version() >= 2);
        let (Some(notifier), Some(seat)) = (notifier, self.seats.seats().next()) else { return };
        self.settle.watch = Some(notifier.get_input_idle_notification(SETTLE_MS, &seat, &self.qh, Settled));
    }
}
