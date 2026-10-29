//! Needs daemons, so every test is ignored under a plain `cargo test`. The
//! VM check (`dev/check.sh fs-upower`) runs them with `--ignored` inside
//! `dbus-run-session`, against the VM's real upowerd and
//! power-profiles-daemon on the system bus and a fake UPower on the session
//! bus.

use std::future::Future;
use std::time::Duration;

use async_io::{Timer, block_on};
use futures_lite::{Stream, StreamExt, future};
use fs_upower::{Change, DeviceKind, DeviceState, PowerProfiles, Profile, UPower};
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

        let mut changes = Box::pin(profiles.changes().await.unwrap());
        assert_eq!(next("the initial state", &mut changes).await, start);

        let other = if start.active == Profile::PowerSaver {
            Profile::Balanced
        } else {
            Profile::PowerSaver
        };
        profiles.set_active(other).await.unwrap();
        let changed = next("the profile change", &mut changes).await;
        assert_eq!(changed.active, other);
        assert_eq!(profiles.state().await.unwrap().active, other);

        profiles.set_active(start.active).await.unwrap();
        assert_eq!(
            next("the change back", &mut changes).await.active,
            start.active
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
}

#[interface(name = "org.freedesktop.UPower")]
impl FakeRoot {
    fn enumerate_devices(&self) -> Vec<OwnedObjectPath> {
        vec![BATTERY.try_into().unwrap()]
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

#[test]
#[ignore = "needs a session bus"]
fn fake_upower_percentage_is_percent_and_changes_stream() {
    block_on(async {
        let server = connection::Builder::session()
            .unwrap()
            .name("org.freedesktop.UPower")
            .unwrap()
            .serve_at(ROOT, FakeRoot { on_battery: true })
            .unwrap()
            .serve_at(BATTERY, battery(41.0))
            .unwrap()
            .serve_at(DISPLAY, battery(41.0))
            .unwrap()
            .build()
            .await
            .unwrap();
        let client = Connection::session().await.unwrap();
        let upower = UPower::connect(&client).await.unwrap();

        let mut changes = Box::pin(upower.changes().await.unwrap());
        let snapshot = upower.snapshot().await.unwrap();
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
        match next("the battery change", &mut changes).await {
            Change::Device(d) => {
                assert_eq!(d.path.as_str(), BATTERY);
                assert_eq!(d.percentage.as_percent(), 40.0);
            }
            other => panic!("expected a device change, got {other:?}"),
        }

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
        match next("the display change", &mut changes).await {
            Change::Display(d) => assert_eq!(d.percentage.rounded(), 39),
            other => panic!("expected a display change, got {other:?}"),
        }

        let root = object_server.interface::<_, FakeRoot>(ROOT).await.unwrap();
        root.get_mut().await.on_battery = false;
        root.get()
            .await
            .on_battery_changed(root.signal_emitter())
            .await
            .unwrap();
        assert_eq!(
            next("OnBattery", &mut changes).await,
            Change::OnBattery(false)
        );

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
        FakeRoot::device_added(root.signal_emitter(), MOUSE.try_into().unwrap())
            .await
            .unwrap();
        match next("DeviceAdded", &mut changes).await {
            Change::DeviceAdded(d) => {
                assert_eq!(d.kind, DeviceKind::Mouse);
                assert!(!d.is_laptop_battery());
                assert_eq!(d.percentage.rounded(), 88);
            }
            other => panic!("expected an added device, got {other:?}"),
        }

        FakeRoot::device_removed(root.signal_emitter(), MOUSE.try_into().unwrap())
            .await
            .unwrap();
        match next("DeviceRemoved", &mut changes).await {
            Change::DeviceRemoved(path) => assert_eq!(path.as_str(), MOUSE),
            other => panic!("expected a removed device, got {other:?}"),
        }
    });
}
