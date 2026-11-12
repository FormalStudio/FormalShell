//! `binds.handOver`: one window is handed the Super chords while it has the
//! keyboard, and the binds on them are off the keys meanwhile.

use formalwm_core::handover::Reserved;
use fs_wm_proto as proto;
use shojiwm_rs::{ipc::ClientId, prelude::*};

use crate::engine::{Engine, enter};

struct Held {
    window: String,
    /// As the shell wrote them.
    chords: Vec<String>,
    reserved: Reserved,
    /// The connection that asked. The hand-over ends with it, so a shell
    /// that died over the window leaves no bind off the keys.
    client: ClientId,
}

#[derive(Default)]
pub struct HandOver {
    held: Option<Held>,
    /// The window the seat's keyboard is on.
    keyboard: Option<String>,
    /// The keys are bound without the window's chords: `bind_keys` says.
    pub active: bool,
}

pub fn setup() {
    COMPOSITOR.event.on_keyboard_focus_change(|focus| {
        let window = focus.window_id.clone();
        enter(move |engine| {
            engine.handover.keyboard = window;
            engine.sync_handover();
        })
    });
}

impl Engine {
    /// The chords that stay bound while the window's go to it: none unless
    /// the keys are its own now. The keyboard on a layer surface or the lock
    /// screen is not, and under the overview no key reaches a window.
    pub fn handing(&self) -> Option<&Reserved> {
        let held = self.handover.held.as_ref()?;
        let typing = self.handover.keyboard.as_deref() == Some(held.window.as_str());
        (typing && !self.overview.open).then_some(&held.reserved)
    }

    /// Binds the keys again when the window's chords have to come off them
    /// or go back on.
    pub fn sync_handover(&mut self) {
        if self.handing().is_some() != self.handover.active {
            self.bind_keys();
        }
        self.publish();
    }

    pub fn hand_over(
        &mut self,
        client: ClientId,
        request: proto::BindsHandOver,
    ) -> Result<(), String> {
        let window = match request.window.filter(|id| request.on && !id.is_empty()) {
            Some(id) => Some(self.resolve(&id)?),
            None => self.model.focused_window().cloned(),
        };
        let was = self.handover.active;
        self.handover.held = window.filter(|_| request.on).map(|window| Held {
            window,
            reserved: Reserved::new(&request.reserved),
            chords: request.reserved,
            client,
        });
        // The list may be another one over the same window.
        if was || self.handing().is_some() {
            self.bind_keys();
        }
        self.publish();
        Ok(())
    }

    /// The hand-over ends when `gone` says its window or its connection is.
    fn end_handover(&mut self, gone: impl Fn(&Held) -> bool) {
        if self.handover.held.as_ref().is_some_and(gone) {
            self.handover.held = None;
            self.sync_handover();
        }
    }

    pub fn client_gone(&mut self, client: ClientId) {
        self.end_handover(|held| held.client == client);
    }

    pub fn window_gone(&mut self, id: &str) {
        self.end_handover(|held| held.window == id);
    }

    pub fn hand_over_state(&self) -> Option<proto::HandOver> {
        let held = self.handover.held.as_ref()?;
        Some(proto::HandOver {
            window: self.public(&held.window),
            reserved: held.chords.clone(),
            active: self.handover.active,
        })
    }
}
