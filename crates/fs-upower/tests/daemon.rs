//! Needs daemons, so every test is ignored under a plain `cargo test`. The
//! VM check (`dev/check.sh fs-upower`) runs them with `--ignored` inside
//! `dbus-run-session`, against the VM's real upowerd and
//! power-profiles-daemon on the system bus and a fake UPower on the session
//! bus.

use std::future::Future;
use std::time::Duration;

use async_io::{Timer, block_on};
use futures_lite::{Stream, StreamExt, future};
use fs_upower::{DeviceKind, DeviceState, PowerProfiles, Profile, Snapshot, UPower};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedObjectPath};
use zbus::{Connection, connection, interface};

async fn within<T>(what: &str, fut: impl Future<Output = T>) -> T {
    future::or(async { Some(fut.await) }, async {
        Timer::after(Duration::from_secs(10)).await;
        None
    })
    .await
    .unwrap_or_else(|| panic!("timed out waiting for {what}"))
}

async fn next<T>(what: &str, stream: &mut (impl Stream<Item = T> + Unpin)) -> T {
    within(what, stream.next())
        .await
        .unwrap_or_else(|| panic!("stream ended waiting for {what}"))
}

#[test]
#[ignore = "needs upowerd"]
fn real_upowerd_with_no_battery_answers_empty() {
    block_on(async {
        let conn = Connection::system().await.unwrap();
        let upower = UPower::connect(&conn).await.unwrap();
        let snapshot = upower.snapshot().await.unwrap();
        eprintln!("{snapshot:#?}");

        assert!(snapshot.devices.iter().all(|d| !d.is_laptop_battery()));
        assert!(!snapshot.on_battery);
        assert!(!snapshot.display.is_laptop_battery());
        assert!(!snapshot.display.is_present);
        assert_eq!(snapshot.display.percentage.as_percent(), 0.0);
        assert_eq!(snapshot.display.time_to_empty, None);
    });
}

#[test]
#[ignore = "needs power-profiles-daemon"]
fn real_power_profiles_switch_and_stream() {
    block_on(async {
        let conn = Connection::system().await.unwrap();
        let profiles = PowerProfiles::connect(&conn).await.unwrap();
        let start = profiles.state().await.unwrap();
        eprintln!("{start:?}");
        assert!(start.available.contains(&Profile::Balanced));
        assert!(start.available.contains(&Profile::PowerSaver));

        let mut changes = Box::pin(profiles.changes());
        assert_eq!(next("the initial state", &mut changes).await, Some(start.clone()));

        let other = if start.active == Profile::PowerSaver {
            Profile::Balanced
        } else {
            Profile::PowerSaver
        };
        profiles.set_active(other).await.unwrap();
        let changed = next("the profile change", &mut changes).await.unwrap();
        assert_eq!(changed.active, other);
        assert_eq!(profiles.state().await.unwrap().active, other);

        profiles.set_active(start.active).await.unwrap();
        assert_eq!(
            next("the change back", &mut changes).await.map(|s| s.active),
            Some(start.active)
        );

        if !start.has_performance() {
            assert!(profiles.set_active(Profile::Performance).await.is_err());
            assert_eq!(profiles.state().await.unwrap().active, start.active);
        }
    });
}

const ROOT: &str = "/org/freedesktop/UPower";
const BATTERY: &str = "/org/freedesktop/UPower/devices/battery_BAT0";
const MOUSE: &str = "/org/freedesktop/UPower/devices/mouse_hidpp";
const DISPLAY: &str = "/org/freedesktop/UPower/devices/DisplayDevice";

struct FakeRoot {
    on_battery: bool,
    devices: Vec<OwnedObjectPath>,
}

impl FakeRoot {
    fn new(on_battery: bool) -> Self {
        Self { on_battery, devices: vec![BATTERY.try_into().unwrap()] }
    }
}

#[interface(name = "org.freedesktop.UPower")]
impl FakeRoot {
    fn enumerate_devices(&self) -> Vec<OwnedObjectPath> {
        self.devices.clone()
    }

    fn get_display_device(&self) -> OwnedObjectPath {
        DISPLAY.try_into().unwrap()
    }

    #[zbus(property)]
    fn on_battery(&self) -> bool {
        self.on_battery
    }

    #[zbus(signal)]
    async fn device_added(emitter: &SignalEmitter<'_>, device: ObjectPath<'_>)
    -> zbus::Result<()>;

    #[zbus(signal)]
    async fn device_removed(emitter: &SignalEmitter<'_>, device: ObjectPath<'_>)
    -> zbus::Result<()>;
}

struct FakeDevice {
    kind: u32,
    state: u32,
    percentage: f64,
    power_supply: bool,
    time_to_empty: i64,
}

#[interface(name = "org.freedesktop.UPower.Device")]
impl FakeDevice {
    #[zbus(property, name = "Type")]
    fn kind(&self) -> u32 {
        self.kind
    }

    #[zbus(property)]
    fn state(&self) -> u32 {
        self.state
    }

    #[zbus(property)]
    fn percentage(&self) -> f64 {
        self.percentage
    }

    #[zbus(property)]
    fn power_supply(&self) -> bool {
        self.power_supply
    }

    #[zbus(property)]
    fn is_present(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn time_to_empty(&self) -> i64 {
        self.time_to_empty
    }

    #[zbus(property)]
    fn energy_rate(&self) -> f64 {
        7.5
    }

    #[zbus(property)]
    fn capacity(&self) -> f64 {
        93.0
    }

    #[zbus(property)]
    fn model(&self) -> String {
        "FAKE-BAT".into()
    }
}

fn battery(percentage: f64) -> FakeDevice {
    FakeDevice {
        kind: 2,
        state: 2,
        percentage,
        power_supply: true,
        time_to_empty: 5400,
    }
}

async fn fake_upower(on_battery: bool, percentage: f64) -> Connection {
    connection::Builder::session()
        .unwrap()
        .name("org.freedesktop.UPower")
        .unwrap()
        .serve_at(ROOT, FakeRoot::new(on_battery))
        .unwrap()
        .serve_at(BATTERY, battery(percentage))
        .unwrap()
        .serve_at(DISPLAY, battery(percentage))
        .unwrap()
        .build()
        .await
        .unwrap()
}

async fn changed(what: &str, changes: &mut (impl Stream<Item = Option<Snapshot>> + Unpin)) -> Snapshot {
    next(what, changes).await.unwrap_or_else(|| panic!("upowerd gone waiting for {what}"))
}

#[test]
#[ignore = "needs a session bus"]
fn fake_upower_percentage_is_percent_and_changes_stream() {
    block_on(async {
        let server = fake_upower(true, 41.0).await;
        let client = Connection::session().await.unwrap();
        let upower = UPower::connect(&client).await.unwrap();

        let mut changes = Box::pin(upower.changes());
        let snapshot = changed("the first snapshot", &mut changes).await;
        assert_eq!(snapshot, upower.snapshot().await.unwrap());
        assert!(snapshot.on_battery);
        let pack = &snapshot.devices[0];
        assert!(pack.is_laptop_battery());
        assert_eq!(pack.kind, DeviceKind::Battery);
        assert_eq!(pack.state, DeviceState::Discharging);
        assert_eq!(pack.percentage.as_percent(), 41.0);
        assert!((pack.percentage.as_fraction() - 0.41).abs() < 1e-12);
        assert_eq!(pack.time_to_empty, Some(Duration::from_secs(5400)));
        assert_eq!(pack.capacity.map(|c| c.rounded()), Some(93));
        assert_eq!(pack.model, "FAKE-BAT");
        assert_eq!(snapshot.display.percentage.rounded(), 41);

        let object_server = server.object_server();

        let device = object_server
            .interface::<_, FakeDevice>(BATTERY)
            .await
            .unwrap();
        device.get_mut().await.percentage = 40.0;
        device
            .get()
            .await
            .percentage_changed(device.signal_emitter())
            .await
            .unwrap();
        let snapshot = changed("the battery change", &mut changes).await;
        assert_eq!(snapshot.devices[0].path.as_str(), BATTERY);
        assert_eq!(snapshot.devices[0].percentage.as_percent(), 40.0);

        let display = object_server
            .interface::<_, FakeDevice>(DISPLAY)
            .await
            .unwrap();
        display.get_mut().await.percentage = 39.0;
        display
            .get()
            .await
            .percentage_changed(display.signal_emitter())
            .await
            .unwrap();
        assert_eq!(changed("the display change", &mut changes).await.display.percentage.rounded(), 39);

        let root = object_server.interface::<_, FakeRoot>(ROOT).await.unwrap();
        root.get_mut().await.on_battery = false;
        root.get()
            .await
            .on_battery_changed(root.signal_emitter())
            .await
            .unwrap();
        assert!(!changed("OnBattery", &mut changes).await.on_battery);

        object_server
            .at(
                MOUSE,
                FakeDevice {
                    kind: 5,
                    state: 2,
                    percentage: 88.0,
                    power_supply: false,
                    time_to_empty: 0,
                },
            )
            .await
            .unwrap();
        root.get_mut().await.devices.push(MOUSE.try_into().unwrap());
        FakeRoot::device_added(root.signal_emitter(), MOUSE.try_into().unwrap())
            .await
            .unwrap();
        let snapshot = changed("DeviceAdded", &mut changes).await;
        let mouse = snapshot.devices.iter().find(|d| d.path.as_str() == MOUSE).expect("the mouse");
        assert_eq!(mouse.kind, DeviceKind::Mouse);
        assert!(!mouse.is_laptop_battery());
        assert_eq!(mouse.percentage.rounded(), 88);

        root.get_mut().await.devices.retain(|d| d.as_str() != MOUSE);
        FakeRoot::device_removed(root.signal_emitter(), MOUSE.try_into().unwrap())
            .await
            .unwrap();
        let snapshot = changed("DeviceRemoved", &mut changes).await;
        assert!(snapshot.devices.iter().all(|d| d.path.as_str() != MOUSE));
    });
}

#[test]
#[ignore = "needs a session bus"]
fn fake_upower_restarting_is_read_whole_again() {
    block_on(async {
        let server = fake_upower(true, 41.0).await;
        let client = Connection::session().await.unwrap();
        let upower = UPower::connect(&client).await.unwrap();
        let mut changes = Box::pin(upower.changes());
        assert_eq!(changed("the first snapshot", &mut changes).await.devices[0].percentage.rounded(), 41);

        server.close().await.unwrap();
        assert_eq!(next("upowerd leaving", &mut changes).await, None);

        let _server = fake_upower(false, 77.0).await;
        let snapshot = changed("upowerd back", &mut changes).await;
        assert!(!snapshot.on_battery);
        assert_eq!(snapshot.devices[0].percentage.rounded(), 77);
        assert_eq!(snapshot.display.percentage.rounded(), 77);
    });
}
