use zbus::proxy;
use zbus::zvariant::ObjectPath;

#[proxy(interface = "org.bluez.Adapter1", default_service = "org.bluez", gen_blocking = false)]
pub trait Adapter1 {
    fn start_discovery(&self) -> zbus::Result<()>;
    fn stop_discovery(&self) -> zbus::Result<()>;
    fn remove_device(&self, device: &ObjectPath<'_>) -> zbus::Result<()>;

    #[zbus(property)]
    fn set_powered(&self, value: bool) -> zbus::Result<()>;
    #[zbus(property)]
    fn set_discoverable(&self, value: bool) -> zbus::Result<()>;
    #[zbus(property)]
    fn set_pairable(&self, value: bool) -> zbus::Result<()>;
}

#[proxy(interface = "org.bluez.Device1", default_service = "org.bluez", gen_blocking = false)]
pub trait Device1 {
    fn connect(&self) -> zbus::Result<()>;
    fn disconnect(&self) -> zbus::Result<()>;
    fn pair(&self) -> zbus::Result<()>;
    fn cancel_pairing(&self) -> zbus::Result<()>;

    #[zbus(property)]
    fn set_trusted(&self, value: bool) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.bluez.AgentManager1",
    default_service = "org.bluez",
    default_path = "/org/bluez",
    gen_blocking = false
)]
pub trait AgentManager1 {
    fn register_agent(&self, agent: &ObjectPath<'_>, capability: &str) -> zbus::Result<()>;
    fn unregister_agent(&self, agent: &ObjectPath<'_>) -> zbus::Result<()>;
    fn request_default_agent(&self, agent: &ObjectPath<'_>) -> zbus::Result<()>;
}

#[proxy(interface = "org.bluez.MediaPlayer1", default_service = "org.bluez", gen_blocking = false)]
pub trait MediaPlayer1 {
    fn play(&self) -> zbus::Result<()>;
    fn pause(&self) -> zbus::Result<()>;
    fn next(&self) -> zbus::Result<()>;
    fn previous(&self) -> zbus::Result<()>;

    #[zbus(property)]
    fn position(&self) -> zbus::Result<u32>;
}
