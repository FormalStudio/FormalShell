//! What the bar's device cells read: the default sink and source
//! (PipeWire), the default Bluetooth adapter (fs-bluez), NetworkManager's
//! devices, UPower's battery and controllers, and every connected pair of
//! earbuds. Each answers its own honest unavailable state when the daemon
//! behind it is not there.

pub mod audio;
pub mod bluetooth;
pub mod earbuds;
pub mod headsets;
pub mod network;
pub mod power;

use crate::runtime::Ctx;

pub use audio::Audio;
pub use bluetooth::Bluetooth;
pub use network::Network;
pub use power::Power;

#[derive(Default)]
pub struct State {
    pub bluetooth: Bluetooth,
    pub network: Network,
    pub audio: Audio,
    pub power: Power,
    pub earbuds: earbuds::Earbuds,
    pub lights: power::Lights,
    pub profiles: power::Profiles,
    /// A saved Wi-Fi passphrase on its way to the launcher, which takes it.
    pub revealed: Option<network::Revealed>,
}

pub enum Diff {
    Bluetooth(Bluetooth),
    Network(Network),
    Audio(Audio),
    Power(Power),
    Earbuds(earbuds::Diff),
    Lights(power::Lights),
    Profiles(power::Profiles),
    Revealed(network::Revealed),
}

fn set<T: PartialEq>(slot: &mut T, v: T) -> bool {
    let changed = *slot != v;
    *slot = v;
    changed
}

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        match diff {
            Diff::Bluetooth(v) => set(&mut self.bluetooth, v),
            Diff::Network(v) => set(&mut self.network, v),
            Diff::Audio(v) => set(&mut self.audio, v),
            Diff::Power(v) => set(&mut self.power, v),
            Diff::Earbuds(d) => self.earbuds.apply(d),
            Diff::Lights(v) => set(&mut self.lights, v),
            Diff::Profiles(v) => set(&mut self.profiles, v),
            Diff::Revealed(v) => {
                self.revealed = Some(v);
                true
            }
        }
    }
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(bluetooth::run(ctx.clone()));
    ctx.spawn(network::run(ctx.clone()));
    ctx.spawn(audio::run(ctx.clone()));
    ctx.spawn(power::run(ctx.clone()));
    ctx.spawn(power::lights(ctx.clone()));
    ctx.spawn(power::run_profiles(ctx.clone()));
    earbuds::start(ctx);
}

/// What a device cell's click or wheel asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    Volume(f64),
    ToggleMute,
    ToggleSourceMute,
    ToggleWifi,
    /// The network panel's radio switch.
    Wifi(bool),
    /// The network panel's rescan button.
    Rescan,
    ToggleBluetooth,
    /// The battery cell's right click: state.json's `batteryShowPercent`.
    BatteryPercent(bool),
    /// The power panel's profile group.
    Profile(fs_upower::Profile),
}

pub fn run(ctx: &Ctx, op: Op) {
    match op {
        Op::Volume(v) => audio::set_volume(ctx, v),
        Op::ToggleMute => audio::toggle_mute(ctx),
        Op::ToggleSourceMute => audio::toggle_source_mute(ctx),
        Op::ToggleWifi => network::toggle_wifi(ctx),
        Op::Wifi(on) => network::set_wifi(ctx, on),
        Op::Rescan => network::rescan(ctx),
        Op::ToggleBluetooth => bluetooth::toggle_power(ctx),
        Op::Profile(p) => power::set_profile(ctx, p),
        Op::BatteryPercent(on) => {
            crate::services::state::set(vec![crate::services::state::Field::BatteryShowPercent(serde_json::Value::Bool(on))])
        }
    }
}
