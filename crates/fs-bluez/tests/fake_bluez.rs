//! The client against a zbus-served fake `org.bluez` on a private
//! dbus-daemon. Needs `dbus-daemon` (FS_BLUEZ_DBUS_DAEMON names the binary),
//! so the tests are ignored in a plain `cargo test` and run by
//! `dev/check.sh`.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_channel::Receiver;
use async_executor::LocalExecutor;
use async_io::Timer;
use fs_bluez::agent::{AGENT_PATH, AgentRequest, Capability};
use fs_bluez::state::{AdapterState, Event};
use fs_bluez::{Bluez, Error};
use fs_devices::bluetooth::DeviceState;
use futures_lite::future::{block_on, or};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedObjectPath};
use zbus::{Connection, fdo, interface};

const HCI0: &str = "/org/bluez/hci0";
const PHONE: &str = "/org/bluez/hci0/dev_11_22_33_44_55_66";
const BUDS: &str = "/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF";

struct Daemon(Child, String);

impl Daemon {
    fn start() -> Daemon {
        let bin = std::env::var("FS_BLUEZ_DBUS_DAEMON").expect("FS_BLUEZ_DBUS_DAEMON names dbus-daemon");
        let mut child = Command::new(bin)
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn dbus-daemon");
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
        Daemon(child, line.trim().to_string())
    }

    async fn conn(&self) -> Connection {
        zbus::connection::Builder::address(self.1.as_str()).unwrap().build().await.unwrap()
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct FakeAdapter {
    powered: bool,
    discovering: bool,
    discoverable: bool,
    pairable: bool,
}

#[interface(name = "org.bluez.Adapter1")]
impl FakeAdapter {
    async fn start_discovery(&mut self, #[zbus(signal_emitter)] em: SignalEmitter<'_>) {
        self.discovering = true;
        self.discovering_changed(&em).await.unwrap();
    }

    async fn stop_discovery(&mut self, #[zbus(signal_emitter)] em: SignalEmitter<'_>) {
        self.discovering = false;
        self.discovering_changed(&em).await.unwrap();
    }

    async fn remove_device(&self, device: ObjectPath<'_>, #[zbus(object_server)] os: &zbus::ObjectServer) {
        os.remove::<FakeDevice, _>(&device).await.unwrap();
    }

    #[zbus(property)]
    fn address(&self) -> String {
        "00:11:22:33:44:55".into()
    }
    #[zbus(property)]
    fn name(&self) -> String {
        "laptop".into()
    }
    #[zbus(property)]
    fn alias(&self) -> String {
        "laptop".into()
    }
    #[zbus(property)]
    fn powered(&self) -> bool {
        self.powered
    }
    #[zbus(property)]
    fn set_powered(&mut self, v: bool) {
        self.powered = v;
    }
    #[zbus(property)]
    fn power_state(&self) -> String {
        if self.powered { "on" } else { "off" }.into()
    }
    #[zbus(property)]
    fn discovering(&self) -> bool {
        self.discovering
    }
    #[zbus(property)]
    fn discoverable(&self) -> bool {
        self.discoverable
    }
    #[zbus(property)]
    fn set_discoverable(&mut self, v: bool) {
        self.discoverable = v;
    }
    #[zbus(property)]
    fn pairable(&self) -> bool {
        self.pairable
    }
    #[zbus(property)]
    fn set_pairable(&mut self, v: bool) {
        self.pairable = v;
    }
}

struct FakeDevice {
    address: &'static str,
    name: &'static str,
    paired: bool,
    trusted: bool,
    connected: bool,
    fail_connect: bool,
}

#[interface(name = "org.bluez.Device1")]
impl FakeDevice {
    async fn connect(&mut self, #[zbus(signal_emitter)] em: SignalEmitter<'_>) -> fdo::Result<()> {
        Timer::after(Duration::from_millis(50)).await;
        if self.fail_connect {
            return Err(fdo::Error::Failed("page timeout".into()));
        }
        self.connected = true;
        self.connected_changed(&em).await?;
        Ok(())
    }

    async fn disconnect(&mut self, #[zbus(signal_emitter)] em: SignalEmitter<'_>) {
        self.connected = false;
        self.connected_changed(&em).await.unwrap();
    }

    async fn pair(&mut self, #[zbus(signal_emitter)] em: SignalEmitter<'_>) {
        Timer::after(Duration::from_millis(50)).await;
        self.paired = true;
        self.paired_changed(&em).await.unwrap();
        self.bonded_changed(&em).await.unwrap();
    }

    async fn cancel_pairing(&self) {}

    #[zbus(property)]
    fn address(&self) -> String {
        self.address.into()
    }
    #[zbus(property)]
    fn name(&self) -> String {
        self.name.into()
    }
    #[zbus(property)]
    fn alias(&self) -> String {
        self.name.into()
    }
    #[zbus(property)]
    fn icon(&self) -> String {
        "audio-headset".into()
    }
    #[zbus(property)]
    fn paired(&self) -> bool {
        self.paired
    }
    #[zbus(property)]
    fn bonded(&self) -> bool {
        self.paired
    }
    #[zbus(property)]
    fn trusted(&self) -> bool {
        self.trusted
    }
    #[zbus(property)]
    fn set_trusted(&mut self, v: bool) {
        self.trusted = v;
    }
    #[zbus(property)]
    fn connected(&self) -> bool {
        self.connected
    }
    #[zbus(property, name = "RSSI")]
    fn rssi(&self) -> i16 {
        -58
    }
    #[zbus(property)]
    fn adapter(&self) -> OwnedObjectPath {
        ObjectPath::try_from(HCI0).unwrap().into()
    }
}

struct FakeBattery(u8);

#[interface(name = "org.bluez.Battery1")]
impl FakeBattery {
    #[zbus(property)]
    fn percentage(&self) -> u8 {
        self.0
    }
}

#[derive(Default)]
struct FakeManager {
    registered: Arc<Mutex<Vec<(String, String)>>>,
    default_requested: Arc<Mutex<bool>>,
}

#[interface(name = "org.bluez.AgentManager1")]
impl FakeManager {
    fn register_agent(&self, agent: ObjectPath<'_>, capability: String) {
        self.registered.lock().unwrap().push((agent.to_string(), capability));
    }
    fn unregister_agent(&self, agent: ObjectPath<'_>) {
        self.registered.lock().unwrap().retain(|(p, _)| *p != agent.as_str());
    }
    fn request_default_agent(&self, _agent: ObjectPath<'_>) {
        *self.default_requested.lock().unwrap() = true;
    }
}

fn device(address: &'static str, name: &'static str, paired: bool) -> FakeDevice {
    FakeDevice { address, name, paired, trusted: false, connected: false, fail_connect: false }
}

async fn serve_bluez(daemon: &Daemon) -> Connection {
    let conn = daemon.conn().await;
    let os = conn.object_server();
    os.at("/", fdo::ObjectManager).await.unwrap();
    os.at(HCI0, FakeAdapter { powered: true, discovering: false, discoverable: false, pairable: true }).await.unwrap();
    os.at(BUDS, device("AA:BB:CC:DD:EE:FF", "Buds", true)).await.unwrap();
    os.at(BUDS, FakeBattery(50)).await.unwrap();
    os.at(PHONE, device("11:22:33:44:55:66", "Phone", false)).await.unwrap();
    conn.request_name("org.bluez").await.unwrap();
    conn
}

async fn within<T>(what: &str, fut: impl Future<Output = T>) -> T {
    or(async { Some(fut.await) }, async {
        Timer::after(Duration::from_secs(10)).await;
        None
    })
    .await
    .unwrap_or_else(|| panic!("timed out waiting for {what}"))
}

/// Drops events until one matches.
async fn expect(rx: &Receiver<Event>, what: &str, pred: impl Fn(&Event) -> bool) -> Event {
    within(what, async {
        loop {
            let e = rx.recv().await.expect("event channel closed");
            if pred(&e) {
                return e;
            }
        }
    })
    .await
}

fn run<F: AsyncFnOnce(&Daemon, &LocalExecutor<'_>)>(f: F) {
    let daemon = Daemon::start();
    let ex = LocalExecutor::new();
    block_on(ex.run(f(&daemon, &ex)));
}

async fn client(daemon: &Daemon, ex: &LocalExecutor<'_>) -> (Bluez, Receiver<Event>) {
    let conn = daemon.conn().await;
    let (bluez, rx, monitor) = Bluez::connect(&conn).await.unwrap();
    ex.spawn(monitor.run()).detach();
    // The monitor holds the connection's streams, not the connection itself.
    std::mem::forget(conn);
    (bluez, rx)
}

#[test]
#[ignore = "needs dbus-daemon; run by dev/check.sh"]
fn no_bluetoothd_is_the_no_adapter_state() {
    run(async |daemon, ex| {
        let (bluez, _rx) = client(daemon, ex).await;
        let s = bluez.state();
        assert!(s.default_adapter().is_none());
        assert!(s.devices().is_empty());
        let err = bluez.connect_device("AA:BB:CC:DD:EE:FF").await.unwrap_err();
        assert!(matches!(err, Error::NoAdapter));
        assert_eq!(err.to_string(), "no bluetooth adapter");
        assert!(bluez.earbuds_devices(None).is_empty());
    });
}

#[test]
#[ignore = "needs dbus-daemon; run by dev/check.sh"]
fn enumerates_adapter_devices_battery_and_rssi() {
    run(async |daemon, ex| {
        let _bluez_conn = serve_bluez(daemon).await;
        let (bluez, _rx) = client(daemon, ex).await;
        let s = bluez.state();
        let a = s.default_adapter().unwrap();
        assert_eq!(a.path, HCI0);
        assert_eq!((a.powered, a.pairable, a.discovering, a.state), (true, true, false, AdapterState::Enabled));
        let buds = s.find_device(HCI0, "aa:bb:cc:dd:ee:ff").unwrap();
        assert_eq!(buds.info.name, "Buds");
        assert!(buds.info.paired && buds.info.bonded && !buds.info.connected);
        assert_eq!(buds.icon, "audio-headset");
        assert_eq!(buds.rssi, Some(-58));
        assert!(buds.info.battery_available);
        assert_eq!(buds.info.battery, 0.5);
        let phone = s.device(PHONE).unwrap();
        assert!(!phone.info.battery_available && !phone.info.paired);
        assert_eq!(bluez.earbuds_devices(None).len(), 2);
        let seeded = bluez.earbuds_devices(Some(r#"[{"address":"aa:bb","name":"X","connected":true}]"#));
        assert_eq!((seeded.len(), seeded[0].address.as_str()), (1, "AA:BB"));
    });
}

#[test]
#[ignore = "needs dbus-daemon; run by dev/check.sh"]
fn adapter_controls_and_discovery() {
    run(async |daemon, ex| {
        let _bluez_conn = serve_bluez(daemon).await;
        let (bluez, rx) = client(daemon, ex).await;
        bluez.set_powered(HCI0, false).await.unwrap();
        expect(&rx, "powered off", |e| matches!(e, Event::AdapterChanged(a) if !a.powered && a.state == AdapterState::Disabled)).await;
        bluez.set_discoverable(HCI0, true).await.unwrap();
        expect(&rx, "discoverable", |e| matches!(e, Event::AdapterChanged(a) if a.discoverable)).await;
        bluez.set_pairable(HCI0, false).await.unwrap();
        expect(&rx, "not pairable", |e| matches!(e, Event::AdapterChanged(a) if !a.pairable)).await;
        bluez.start_discovery(HCI0).await.unwrap();
        expect(&rx, "discovering", |e| matches!(e, Event::AdapterChanged(a) if a.discovering)).await;
        bluez.stop_discovery(HCI0).await.unwrap();
        expect(&rx, "scan stopped", |e| matches!(e, Event::AdapterChanged(a) if !a.discovering)).await;
    });
}

#[test]
#[ignore = "needs dbus-daemon; run by dev/check.sh"]
fn device_commands_and_their_events() {
    run(async |daemon, ex| {
        let _bluez_conn = serve_bluez(daemon).await;
        let (bluez, rx) = client(daemon, ex).await;

        let connecting = bluez.connect_device("aa:bb:cc:dd:ee:ff");
        let watch = async {
            expect(&rx, "connecting", |e| matches!(e, Event::DeviceChanged(d) if d.info.state == DeviceState::Connecting)).await;
            expect(&rx, "connected", |e| matches!(e, Event::DeviceChanged(d) if d.info.connected && d.info.state == DeviceState::Connected)).await;
        };
        let (done, ()) = futures_lite::future::zip(connecting, watch).await;
        done.unwrap();
        assert_eq!(bluez.earbuds_devices(None).iter().filter(|d| d.connected).count(), 1);

        bluez.disconnect_device("AA:BB:CC:DD:EE:FF").await.unwrap();
        expect(&rx, "disconnected", |e| matches!(e, Event::DeviceChanged(d) if !d.info.connected && d.info.state == DeviceState::Disconnected)).await;

        let pairing = bluez.pair("11:22:33:44:55:66");
        let watch = async {
            expect(&rx, "pairing flag", |e| matches!(e, Event::DeviceChanged(d) if d.info.pairing)).await;
            // Pair()'s reply and the Paired/Bonded signals race, so the flag
            // clears before or after them; the last event carries all three.
            expect(&rx, "paired, pairing flag cleared", |e| {
                matches!(e, Event::DeviceChanged(d) if d.info.paired && d.info.bonded && !d.info.pairing)
            })
            .await;
        };
        let (done, ()) = futures_lite::future::zip(pairing, watch).await;
        done.unwrap();

        bluez.set_trusted("11:22:33:44:55:66", true).await.unwrap();
        expect(&rx, "trusted", |e| matches!(e, Event::DeviceChanged(d) if d.info.trusted)).await;

        bluez.remove("11:22:33:44:55:66").await.unwrap();
        let gone = expect(&rx, "removed", |e| matches!(e, Event::DeviceRemoved { .. })).await;
        assert_eq!(gone, Event::DeviceRemoved { path: PHONE.into(), address: "11:22:33:44:55:66".into() });
        assert!(matches!(bluez.connect_device("11:22:33:44:55:66").await, Err(Error::UnknownDevice(_))));
    });
}

#[test]
#[ignore = "needs dbus-daemon; run by dev/check.sh"]
fn a_failed_connect_returns_bluezs_error_and_clears_the_state() {
    run(async |daemon, ex| {
        let bluez_conn = serve_bluez(daemon).await;
        bluez_conn.object_server().at("/org/bluez/hci0/dev_DE_AD_00_00_00_01", FakeDevice { fail_connect: true, ..device("DE:AD:00:00:00:01", "Flaky", true) }).await.unwrap();
        let (bluez, _rx) = client(daemon, ex).await;
        let err = bluez.connect_device("DE:AD:00:00:00:01").await.unwrap_err();
        assert_eq!(err.bluez_name(), Some("org.freedesktop.DBus.Error.Failed"));
        let d = bluez.state().device("/org/bluez/hci0/dev_DE_AD_00_00_00_01").unwrap();
        assert_eq!(d.info.state, DeviceState::Disconnected);
    });
}

#[test]
#[ignore = "needs dbus-daemon; run by dev/check.sh"]
fn devices_come_and_go_and_properties_follow() {
    run(async |daemon, ex| {
        let bluez_conn = serve_bluez(daemon).await;
        let (bluez, rx) = client(daemon, ex).await;
        let path = "/org/bluez/hci0/dev_CA_FE_00_00_00_02";
        bluez_conn.object_server().at(path, device("CA:FE:00:00:00:02", "Speaker", false)).await.unwrap();
        expect(&rx, "added", |e| matches!(e, Event::DeviceAdded(d) if d.info.name == "Speaker")).await;
        bluez_conn.object_server().at(path, FakeBattery(80)).await.unwrap();
        expect(&rx, "battery", |e| matches!(e, Event::DeviceChanged(d) if d.info.battery_available)).await;
        let iface = bluez_conn.object_server().interface::<_, FakeBattery>(path).await.unwrap();
        iface.get_mut().await.0 = 20;
        let em = iface.signal_emitter();
        iface.get().await.percentage_changed(em).await.unwrap();
        expect(&rx, "battery change", |e| matches!(e, Event::DeviceChanged(d) if d.info.battery == 0.2)).await;
        bluez_conn.object_server().remove::<FakeBattery, _>(path).await.unwrap();
        expect(&rx, "battery gone", |e| matches!(e, Event::DeviceChanged(d) if !d.info.battery_available)).await;
        bluez_conn.object_server().remove::<FakeDevice, _>(path).await.unwrap();
        expect(&rx, "removed", |e| matches!(e, Event::DeviceRemoved { .. })).await;
        assert!(bluez.state().device(path).is_none());
    });
}

#[test]
#[ignore = "needs dbus-daemon; run by dev/check.sh"]
fn bluetoothd_leaving_and_returning_empties_and_refills_the_state() {
    run(async |daemon, ex| {
        let bluez_conn = serve_bluez(daemon).await;
        let (bluez, rx) = client(daemon, ex).await;
        assert!(bluez.state().default_adapter().is_some());
        bluez_conn.release_name("org.bluez").await.unwrap();
        expect(&rx, "adapter gone", |e| matches!(e, Event::AdapterRemoved { .. })).await;
        assert!(bluez.state().default_adapter().is_none() && bluez.state().devices().is_empty());
        bluez_conn.request_name("org.bluez").await.unwrap();
        expect(&rx, "adapter back", |e| matches!(e, Event::AdapterAdded(_))).await;
        assert_eq!(bluez.state().devices().len(), 2);
    });
}

#[test]
#[ignore = "needs dbus-daemon; run by dev/check.sh"]
fn the_agent_registers_answers_and_rejects() {
    run(async |daemon, ex| {
        let bluez_conn = serve_bluez(daemon).await;
        let manager = FakeManager::default();
        let (registered, default_requested) = (manager.registered.clone(), manager.default_requested.clone());
        bluez_conn.object_server().at("/org/bluez", manager).await.unwrap();

        let conn = daemon.conn().await;
        let (bluez, _rx, monitor) = Bluez::connect(&conn).await.unwrap();
        ex.spawn(monitor.run()).detach();
        let requests = bluez.register_agent(Capability::DisplayYesNo).await.unwrap();
        assert_eq!(*registered.lock().unwrap(), vec![(AGENT_PATH.to_string(), "DisplayYesNo".to_string())]);
        assert!(*default_requested.lock().unwrap());

        let caller = bluez_conn.clone();
        let target = conn.unique_name().unwrap().to_owned();
        let call = |member: &'static str, body: (ObjectPath<'static>, u32)| {
            let caller = caller.clone();
            let target = target.clone();
            async move { caller.call_method(Some(target), AGENT_PATH, Some("org.bluez.Agent1"), member, &body).await }
        };

        let asked = call("RequestConfirmation", (ObjectPath::try_from(PHONE).unwrap(), 123456));
        let answer = async {
            match requests.recv().await.unwrap() {
                AgentRequest::Confirm { device, passkey, reply } => {
                    assert_eq!((device.as_str(), passkey), (PHONE, 123456));
                    reply.send(true).await.unwrap();
                }
                other => panic!("unexpected {other:?}"),
            }
        };
        let (out, ()) = within("confirm", futures_lite::future::zip(asked, answer)).await;
        out.unwrap();

        let asked = call("RequestConfirmation", (ObjectPath::try_from(PHONE).unwrap(), 654321));
        let answer = async {
            match requests.recv().await.unwrap() {
                AgentRequest::Confirm { reply, .. } => reply.send(false).await.unwrap(),
                other => panic!("unexpected {other:?}"),
            }
        };
        let (out, ()) = within("reject", futures_lite::future::zip(asked, answer)).await;
        let err = out.unwrap_err();
        assert!(matches!(&err, zbus::Error::MethodError(n, _, _) if n.as_str() == "org.bluez.Error.Rejected"), "{err:?}");

        bluez.unregister_agent().await.unwrap();
        assert!(registered.lock().unwrap().is_empty());
    });
}
