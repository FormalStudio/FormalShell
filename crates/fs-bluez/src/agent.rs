//! An `org.bluez.Agent1` the shell can offer while its panel is open. Every
//! BlueZ callback becomes an `AgentRequest` on a channel; the answer goes back
//! on the request's own reply channel. A dropped reply, or a closed request
//! channel, rejects the pairing.

use async_channel::{Receiver, Sender, bounded};
use zbus::zvariant::ObjectPath;

/// The object path agents are served under, one child per registration.
pub const AGENT_PATH: &str = "/org/formalshell/bluez/agent";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    DisplayOnly,
    DisplayYesNo,
    KeyboardOnly,
    NoInputNoOutput,
    KeyboardDisplay,
}

impl Capability {
    pub fn as_str(self) -> &'static str {
        match self {
            Capability::DisplayOnly => "DisplayOnly",
            Capability::DisplayYesNo => "DisplayYesNo",
            Capability::KeyboardOnly => "KeyboardOnly",
            Capability::NoInputNoOutput => "NoInputNoOutput",
            Capability::KeyboardDisplay => "KeyboardDisplay",
        }
    }
}

/// `device` is the device's object path, which is `Device::path`.
#[derive(Debug)]
pub enum AgentRequest {
    Confirm { device: String, passkey: u32, reply: Sender<bool> },
    PinCode { device: String, reply: Sender<Option<String>> },
    Passkey { device: String, reply: Sender<Option<u32>> },
    DisplayPinCode { device: String, pincode: String },
    DisplayPasskey { device: String, passkey: u32, entered: u16 },
    Authorize { device: String, reply: Sender<bool> },
    AuthorizeService { device: String, uuid: String, reply: Sender<bool> },
    Cancel,
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.Error")]
pub enum AgentError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Rejected(String),
    Canceled(String),
}

fn rejected() -> AgentError {
    AgentError::Rejected("rejected".into())
}

pub(crate) struct Agent {
    tx: Sender<AgentRequest>,
}

impl Agent {
    pub(crate) fn channel() -> (Agent, Receiver<AgentRequest>) {
        let (tx, rx) = async_channel::unbounded();
        (Agent { tx }, rx)
    }

    async fn ask<T>(&self, make: impl FnOnce(Sender<T>) -> AgentRequest) -> Result<T, AgentError> {
        let (reply, answer) = bounded(1);
        self.tx.send(make(reply)).await.map_err(|_| rejected())?;
        answer.recv().await.map_err(|_| rejected())
    }

    async fn tell(&self, request: AgentRequest) {
        let _ = self.tx.send(request).await;
    }
}

#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    async fn release(&self) {}

    async fn request_pin_code(&self, device: ObjectPath<'_>) -> Result<String, AgentError> {
        let device = device.to_string();
        self.ask(|reply| AgentRequest::PinCode { device, reply }).await?.ok_or_else(rejected)
    }

    async fn display_pin_code(&self, device: ObjectPath<'_>, pincode: String) {
        self.tell(AgentRequest::DisplayPinCode { device: device.to_string(), pincode }).await;
    }

    async fn request_passkey(&self, device: ObjectPath<'_>) -> Result<u32, AgentError> {
        let device = device.to_string();
        self.ask(|reply| AgentRequest::Passkey { device, reply }).await?.ok_or_else(rejected)
    }

    async fn display_passkey(&self, device: ObjectPath<'_>, passkey: u32, entered: u16) {
        self.tell(AgentRequest::DisplayPasskey { device: device.to_string(), passkey, entered }).await;
    }

    async fn request_confirmation(&self, device: ObjectPath<'_>, passkey: u32) -> Result<(), AgentError> {
        let device = device.to_string();
        self.ask(|reply| AgentRequest::Confirm { device, passkey, reply }).await?.then_some(()).ok_or_else(rejected)
    }

    async fn request_authorization(&self, device: ObjectPath<'_>) -> Result<(), AgentError> {
        let device = device.to_string();
        self.ask(|reply| AgentRequest::Authorize { device, reply }).await?.then_some(()).ok_or_else(rejected)
    }

    async fn authorize_service(&self, device: ObjectPath<'_>, uuid: String) -> Result<(), AgentError> {
        let device = device.to_string();
        self.ask(|reply| AgentRequest::AuthorizeService { device, uuid, reply }).await?.then_some(()).ok_or_else(rejected)
    }

    async fn cancel(&self) {
        self.tell(AgentRequest::Cancel).await;
    }
}
