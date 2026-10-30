//! Every bar cell, one module each, and the one place a `bar.layout` entry
//! becomes cells: one, or a rail of independent ones sharing the entry's
//! place (Indicators). Adding a cell is a module here implementing
//! [`Cell`](super::cell::Cell) and one arm in [`build`].
//!
//! The builtins not yet ported draw nothing ([`absent::Absent`]): no
//! stand-in readout, so the strip never shows a value nothing measured.

mod absent;
mod activity;
mod active_window;
mod audio;
mod battery;
mod bell;
mod bluetooth;
mod chevron;
mod clock;
mod display;
mod dualsense;
mod earbuds;
mod command;
mod github;
mod indicators;
mod iphone;
mod keyboard_layout;
mod launcher;
mod mic;
mod network;
mod now_playing;
pub mod tray;
mod monitor;
mod plugin;
mod system_update;
mod tailscale;
mod usage;
mod visualizer;
mod weather;
pub mod workspaces;

use fs_chrome::bar::layout::{Builtin, Entry, EntryKind};

use super::cell::Cell;

pub fn build(entry: &Entry, region_entries: &[Entry]) -> Vec<Box<dyn Cell>> {
    if matches!(entry.kind, EntryKind::Builtin(Builtin::Indicators)) {
        return indicators::rail();
    }
    let cell: Box<dyn Cell> = match &entry.kind {
        EntryKind::Builtin(b) => match b {
            Builtin::Launcher => Box::new(launcher::Launcher::default()),
            Builtin::Workspaces => Box::new(workspaces::Workspaces::default()),
            Builtin::ActiveWindow => Box::new(active_window::ActiveWindow::default()),
            Builtin::Clock => Box::new(clock::Clock::default()),
            Builtin::NowPlaying => Box::new(now_playing::NowPlaying::default()),
            Builtin::Audio => Box::new(audio::Audio::default()),
            Builtin::Network => Box::new(network::Network::default()),
            Builtin::Bluetooth => Box::new(bluetooth::Bluetooth::default()),
            Builtin::Tray => Box::new(tray::Tray::strip()),
            Builtin::Battery => Box::new(battery::Battery::default()),
            Builtin::Microphone => Box::new(mic::Mic::default()),
            Builtin::Earbuds => Box::new(earbuds::Earbuds::new()),
            Builtin::Display => Box::new(display::Display::default()),
            Builtin::Dualsense => Box::new(dualsense::Dualsense::default()),
            Builtin::Visualizer => Box::new(visualizer::Visualizer::default()),
            Builtin::Weather => Box::new(weather::Weather::default()),
            Builtin::Github => Box::new(github::Github::default()),
            Builtin::Usage => Box::new(usage::Usage::default()),
            Builtin::SystemUpdate => Box::new(system_update::SystemUpdate::default()),
            Builtin::Tailscale => Box::new(tailscale::Tailscale::default()),
            Builtin::KeyboardLayout => Box::new(keyboard_layout::KeyboardLayout::default()),
            Builtin::Monitor => Box::new(monitor::Monitor::default()),
            Builtin::Iphone => Box::new(iphone::Iphone::default()),
            Builtin::Bell => Box::new(bell::Bell::default()),
            Builtin::Chevron => Box::new(chevron::Chevron::new(entry.region, region_entries)),
            _ => Box::new(absent::Absent),
        },
        EntryKind::Module { id, .. } => Box::new(command::Command::new(id)),
        EntryKind::Plugin { id, .. } => Box::new(plugin::Plugin::new(id)),
    };
    vec![cell]
}
