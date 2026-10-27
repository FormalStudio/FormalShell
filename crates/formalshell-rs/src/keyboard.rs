//! A released wl_keyboard, held until its last reference is ours to drop.
//!
//! sctk 0.21.1's `RepeatData` removes its repeat timer from the loop when it
//! drops, and its token is still armed whenever a key's press reached us and
//! its release did not (Return unlocking the lock takes the focus away
//! before the release). The backend drops a released keyboard's data when the
//! compositor's `delete_id` is read, which calloop-wayland-source does in
//! `before_handle_events` with calloop's source list borrowed, so that
//! `LoopHandle::remove` panics on "RefCell already borrowed". Holding a clone
//! of the proxy keeps the data alive past that read; `sweep` drops it from
//! the loop's own turn once the backend has let go.

use smithay_client_toolkit::reexports::client::Proxy;
use smithay_client_toolkit::reexports::client::protocol::wl_keyboard::WlKeyboard;

#[derive(Default)]
pub struct Released(Vec<WlKeyboard>);

impl Released {
    pub fn release(&mut self, keyboard: WlKeyboard) {
        keyboard.release();
        self.0.push(keyboard);
    }

    /// Once per loop turn.
    pub fn sweep(&mut self) {
        self.0.retain(|k| k.object_data().is_some_and(|d| std::sync::Arc::strong_count(d) > 1));
    }
}
