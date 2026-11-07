use std::collections::HashMap;
use std::sync::Arc;

use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedValue;
use zbus::{Connection, interface};

use crate::model::NotifyArgs;
use crate::{CloseReason, Event, OBJECT_PATH, Shared};

pub(crate) struct Notifications {
    pub(crate) shared: Arc<Shared>,
}

#[interface(name = "org.freedesktop.Notifications")]
impl Notifications {
    fn get_capabilities(&self) -> Vec<String> {
        crate::CAPABILITIES.map(String::from).into()
    }

    fn get_server_information(&self) -> (String, String, String, String) {
        (crate::NAME.into(), crate::VENDOR.into(), self.shared.version.clone(), crate::SPEC_VERSION.into())
    }

    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let args = NotifyArgs { app_name, app_icon, summary, body, actions, hints, expire_timeout };
        let (n, replaced) = self.shared.state().upsert(replaces_id, args);
        let id = n.id;
        self.shared.emit(if replaced { Event::Replaced(n) } else { Event::Notified(n) });
        id
    }

    async fn close_notification(
        &self,
        id: u32,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        if self.shared.state().remove(id).is_some() {
            self.shared.emit(Event::Closed { id });
            Self::notification_closed(&emitter, id, CloseReason::CloseRequested as u32).await?;
        }
        Ok(())
    }

    #[zbus(signal)]
    async fn notification_closed(emitter: &SignalEmitter<'_>, id: u32, reason: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_invoked(emitter: &SignalEmitter<'_>, id: u32, action_key: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn activation_token(emitter: &SignalEmitter<'_>, id: u32, activation_token: &str) -> zbus::Result<()>;
}

async fn emitter(conn: &Connection) -> zbus::Result<SignalEmitter<'static>> {
    SignalEmitter::new(conn, OBJECT_PATH)
}

pub(crate) async fn closed(conn: &Connection, id: u32, reason: CloseReason) -> zbus::Result<()> {
    Notifications::notification_closed(&emitter(conn).await?, id, reason as u32).await
}

/// `ActivationToken` goes out first so a client can focus the right window
/// before it sees the action.
pub(crate) async fn action_invoked(conn: &Connection, id: u32, key: &str, token: Option<&str>) -> zbus::Result<()> {
    let e = emitter(conn).await?;
    if let Some(token) = token {
        Notifications::activation_token(&e, id, token).await?;
    }
    Notifications::action_invoked(&e, id, key).await
}
