//! One module per service, beside the helpers they share. Every slice of
//! the [`Store`](crate::store::Store) is declared once in `store.rs`'s
//! `slices!`; services publish diffs into it from tasks on the service
//! thread and never read it. A service running for the whole session comes
//! up through its `start(&Ctx)`, listed in [`SERVICES`]; the info sources
//! and display's polls run only while something wants them ([`wants`]). A
//! service that follows a slice it does not own reacts to it in [`react`].

pub mod activity;
pub mod agents;
pub mod airplay;
pub mod ams;
pub mod appicon;
pub mod barpaint;
pub mod brightness;
pub mod bus;
pub mod caffeinate;
pub mod capture;
pub mod clipboard;
pub mod clipssh;
pub mod clock;
pub mod commands;
pub mod compositor;
pub mod config;
pub mod depth;
pub mod cover;
pub mod devices;
pub mod display;
pub mod icons;
pub mod inhibit;
pub mod ipad;
pub mod iphone;
pub mod lights;
pub mod localsend;
pub mod media;
pub mod menu;
pub mod mirror;
pub mod motion_art;
pub mod nightlight;
pub mod notifications;
pub mod overnight;
pub mod overrides;
pub mod packages;
pub mod picker;
pub mod info;
pub mod launch;
pub mod kopuz;
pub mod lyrics;
pub mod plugins;
pub mod polkit;
pub mod power;
pub mod proc;
pub mod radio;
pub mod recording;
pub mod screensaver;
pub mod session;
pub mod shutdown;
pub mod sleep;
pub mod state;
mod watch;
pub mod theme;
pub mod tray;
pub mod v4l2;
pub mod visualizer;
pub mod wallpaper;
pub mod wants;

use crate::runtime::Ctx;
use crate::store::{Diff, Store, Topic};

/// The whole-session services, in the order they come up.
const SERVICES: &[fn(&Ctx)] = &[
    clock::start,
    config::start,
    state::start,
    compositor::start,
    theme::start,
    media::start,
    cover::start,
    motion_art::start,
    lyrics::start,
    kopuz::start,
    radio::start,
    airplay::start,
    ams::start,
    visualizer::start,
    commands::start,
    barpaint::start,
    wallpaper::start,
    agents::start,
    appicon::start,
    wants::start,
    plugins::start,
    nightlight::start,
    lights::start,
    brightness::start,
    devices::bluetooth::start,
    devices::network::start,
    devices::audio::start,
    devices::power::start,
    devices::earbuds::start,
    display::start,
    tray::start,
    activity::start,
    notifications::start,
    menu::start,
    screensaver::start,
    inhibit::start,
    ipad::start,
    clipboard::start,
    clipssh::start,
    packages::start,
    power::start,
    picker::start,
    localsend::start,
    depth::start,
    mirror::start,
    launch::start,
    session::start,
    sleep::start,
    polkit::start,
];

pub fn start(ctx: &Ctx) {
    for start in SERVICES {
        start(ctx);
    }
}

/// What a change to one slice asks of the services that follow it, run on
/// the UI thread once the slice has applied and before any surface redraws.
/// A reaction that moves a slice of its own hands back that diff.
pub fn react(store: &Store, topic: Topic) -> Option<Diff> {
    match topic {
        Topic::Media => {
            kopuz::sync(&store.media);
            lyrics::sync(&store.media, &store.config, &store.lyrics);
            None
        }
        Topic::Config | Topic::Lyrics => {
            lyrics::sync(&store.media, &store.config, &store.lyrics);
            None
        }
        Topic::Compositor | Topic::Launch => launch::windows_changed(store),
        _ => None,
    }
}
