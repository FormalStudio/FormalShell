//! The launcher's suspend, reboot and shutdown rows, asked of logind over
//! the system bus rather than spawned as `systemctl` through the
//! compositor, where whatever logind answered went nowhere. The call allows
//! interactive authorization, so a password polkit wants is asked for by
//! the shell's own agent; the answer is logged, and a refusal is a toast.

use std::sync::LazyLock;

use async_channel::{Receiver, Sender};
use fs_info::notifications::Urgency;
use zbus::Proxy;
use zbus::proxy::MethodFlags;

use super::notifications::{self, Op};
use crate::runtime::Ctx;
use crate::store;

#[derive(Clone, Copy, Debug)]
pub enum Action {
    Suspend,
    Reboot,
    PowerOff,
}

impl Action {
    pub fn parse(verb: &str) -> Option<Self> {
        match verb {
            "suspend" => Some(Self::Suspend),
            "reboot" => Some(Self::Reboot),
            "poweroff" => Some(Self::PowerOff),
            _ => None,
        }
    }

    pub(crate) fn method(self) -> &'static str {
        match self {
            Self::Suspend => "Suspend",
            Self::Reboot => "Reboot",
            Self::PowerOff => "PowerOff",
        }
    }

    fn failed(self) -> &'static str {
        match self {
            Self::Suspend => "Suspend failed",
            Self::Reboot => "Reboot failed",
            Self::PowerOff => "Shutdown failed",
        }
    }
}

static CHANNEL: LazyLock<(Sender<Action>, Receiver<Action>)> = LazyLock::new(async_channel::unbounded);

/// Callable from any thread.
pub fn request(action: Action) {
    let _ = CHANNEL.0.try_send(action);
}

pub async fn run(ctx: Ctx) {
    while let Ok(action) = CHANNEL.1.recv().await {
        // Off this loop: the call stays pending for as long as the
        // password prompt is up.
        let c = ctx.clone();
        ctx.spawn(async move {
            let method = action.method();
            match call(method).await {
                Ok(()) => eprintln!("power: {method} accepted"),
                Err(err) => {
                    eprintln!("power: {method}: {err}");
                    let body = match &err {
                        zbus::Error::MethodError(_, Some(msg), _) => msg.clone(),
                        other => other.to_string(),
                    };
                    let op = Op::Notify(action.failed().into(), body, Urgency::Critical);
                    c.publish(store::Diff::Notifications(notifications::Diff::Op(op)));
                }
            }
        });
    }
}

/// The interactive argument and the header flag both: logind's plain
/// methods read the first, polkit's check the second.
pub(crate) async fn call(method: &str) -> zbus::Result<()> {
    let proxy = manager().await?;
    proxy.call_with_flags::<_, _, ()>(method, MethodFlags::AllowInteractiveAuth.into(), &(true,)).await?;
    Ok(())
}

/// The lock's buttons: no polkit agent can show over the lock, so a call
/// that would need a password is refused rather than left waiting on one.
pub(crate) async fn call_quiet(method: &str) -> zbus::Result<()> {
    manager().await?.call::<_, _, ()>(method, &(false,)).await
}

async fn manager() -> zbus::Result<Proxy<'static>> {
    let conn = crate::services::bus::system().await?;
    Proxy::new(&conn, "org.freedesktop.login1", "/org/freedesktop/login1", "org.freedesktop.login1.Manager").await
}
