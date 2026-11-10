//! UPower through fs-upower: the laptop battery the bar reads off the
//! display device, `OnBattery`, and the DualSense battery. hid-playstation's
//! `ps-controller-battery-<MAC>` supply is one of UPower's own devices, so
//! the controller arrives and changes on UPower's signals rather than on
//! a 30 second sysfs poll.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use async_io::Timer;
use async_lock::OnceCell;

use fs_system::dualsense::{self, Supply};
use fs_types::Percent;
use fs_upower::{Device, DeviceState, PowerProfiles, Profile, Snapshot, UPower};
use futures_lite::StreamExt;

use crate::runtime::Ctx;
use crate::store;

#[derive(Clone, Debug, PartialEq)]
pub struct Battery {
    /// Whole percent, as the cell shows it.
    pub level: Percent,
    pub state: DeviceState,
    /// Watts, a magnitude (`changeRate`).
    pub rate: f64,
    /// None when UPower has no estimate.
    pub time_to_full: Option<Duration>,
    pub time_to_empty: Option<Duration>,
    /// The pack's capacity against its design, off the physical battery:
    /// the display device is an aggregate and publishes none.
    pub health: Option<Percent>,
    /// Watt-hours, 0 when UPower has no reading.
    pub size_wh: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Power {
    /// None without a laptop battery (`isLaptopBattery`), which hides the cell.
    pub battery: Option<Battery>,
    pub on_battery: bool,
    /// None while no DualSense is connected.
    pub dualsense: Option<Supply>,
}

fn battery(d: &Device, physical: Option<&Device>) -> Option<Battery> {
    d.is_laptop_battery().then(|| Battery {
        level: d.percentage.round(),
        state: d.state,
        rate: d.energy_rate,
        time_to_full: d.time_to_full,
        time_to_empty: d.time_to_empty,
        health: physical.and_then(|p| p.capacity),
        size_wh: d.energy_full,
    })
}

/// First match wins.
fn dualsense(devices: &BTreeMap<String, Device>) -> Option<Supply> {
    let d = devices.values().find(|d| d.native_path.starts_with("ps-controller-battery-") && d.is_present)?;
    Some(Supply::new(d.percentage.round(), d.state))
}

fn power(snapshot: Snapshot) -> Power {
    let devices: BTreeMap<String, Device> = snapshot.devices.into_iter().map(|d| (d.path.to_string(), d)).collect();
    Power {
        battery: battery(&snapshot.display, devices.values().find(|d| d.is_laptop_battery())),
        on_battery: snapshot.on_battery,
        dualsense: dualsense(&devices),
    }
}

/// UPower's devices and power-profiles-daemon; the DualSense's lights are
/// a poll of their own ([`lights`]).
pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
    ctx.spawn(run_profiles(ctx.clone()));
    ctx.spawn(commands());
}

async fn run(ctx: Ctx) {
    let publish = |p| ctx.publish(store::Diff::Power(p));
    let upower = match crate::services::bus::system().await {
        Ok(conn) => UPower::connect(&conn).await,
        Err(err) => Err(err),
    };
    let Ok(upower) = upower else { return publish(Power::default()) };
    let mut shown = None;
    let mut changes = std::pin::pin!(upower.changes());
    while let Some(snapshot) = changes.next().await {
        // A signal for a property the view leaves out reaches no one.
        let now = snapshot.map_or_else(Power::default, power);
        if shown.as_ref() != Some(&now) {
            shown = Some(now.clone());
            publish(now);
        }
    }
}

/// The DualSense's lightbar colour and lit player LEDs, which sysfs alone
/// holds and nothing signals. Read while the DualSense panel is open, on
/// open and then every 30 seconds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lights {
    pub lightbar: Option<String>,
    /// None while no lightbar node exists to find the LEDs beside.
    pub player_leds: Option<usize>,
}

const LIGHTS_POLL: Duration = Duration::from_secs(30);

fn first_dir(dir: &str, matches: impl Fn(&str) -> bool) -> Option<PathBuf> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| matches(n))
        .collect();
    names.sort();
    names.first().map(|n| Path::new(dir).join(n))
}

fn read_lights() -> Lights {
    let Some(led) = first_dir("/sys/class/leds", |n| n.starts_with("input") && n.ends_with(":rgb:indicator")) else {
        return Lights::default();
    };
    let rgb = std::fs::read_to_string(led.join("multi_intensity")).ok();
    let base = led.to_string_lossy().trim_end_matches(":rgb:indicator").to_owned();
    let pips: Vec<Option<String>> = (1..=5).map(|i| std::fs::read_to_string(format!("{base}:white:player-{i}/brightness")).ok()).collect();
    let pips: Vec<Option<&str>> = pips.iter().map(|p| p.as_deref()).collect();
    Lights { lightbar: dualsense::parse_lightbar(rgb.as_deref()), player_leds: Some(dualsense::parse_player_leds(Some(&pips))) }
}

/// `Source::PadLights`: the panel's open reads at once, then every
/// [`LIGHTS_POLL`].
pub async fn lights(ctx: Ctx) {
    loop {
        let lights = ctx.pool().run(read_lights).await.unwrap_or_default();
        ctx.publish(store::Diff::PadLights(lights));
        Timer::after(LIGHTS_POLL).await;
    }
}

static PROFILES: OnceCell<PowerProfiles> = OnceCell::new();

/// The one power-profiles-daemon client the shell keeps, for the watch, the
/// profile group and overnight alike.
pub async fn profiles() -> zbus::Result<PowerProfiles> {
    PROFILES
        .get_or_try_init(|| async { PowerProfiles::connect(&crate::services::bus::system().await?).await })
        .await
        .cloned()
}

/// power-profiles-daemon's active profile and the ones it offers, none
/// while the daemon is not on the bus.
async fn run_profiles(ctx: Ctx) {
    let publish = |p| ctx.publish(store::Diff::Profiles(p));
    let Ok(profiles) = profiles().await else { return publish(None) };
    let mut changes = std::pin::pin!(profiles.changes());
    while let Some(state) = changes.next().await {
        publish(state);
    }
}

/// What the panels ask of the service.
pub enum Cmd {
    /// The profile group's pick; a daemon that refuses leaves the live
    /// property, and with it the group, where it was.
    Profile(Profile),
}

static CMDS: LazyLock<(async_channel::Sender<Cmd>, async_channel::Receiver<Cmd>)> = LazyLock::new(async_channel::unbounded);

/// Callable from any thread.
pub fn send(c: Cmd) {
    let _ = CMDS.0.try_send(c);
}

async fn commands() {
    while let Ok(cmd) = CMDS.1.recv().await {
        match cmd {
            Cmd::Profile(profile) => {
                let set = match profiles().await {
                    Ok(profiles) => profiles.set_active(profile).await,
                    Err(err) => Err(err),
                };
                if let Err(err) = set {
                    eprintln!("power profile: {}: {err}", profile.to_wire());
                }
            }
        }
    }
}

