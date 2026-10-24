//! The state surfaces read. Services own their slice's shape and how a
//! diff applies to it; the UI thread is the only writer, through
//! [`Store::apply`].

use crate::services::{
    appicon, barpaint, brightness, caffeinate, capture, clipboard, clipssh, clock, commands, config, devices, display, herdr,
    compositor, info, lights, localsend, lyrics, media, menu, mirror, nightlight, notifications, picker, plugins, recording, screensaver, state, theme, tray,
    visualizer, wallpaper,
};

#[derive(Default)]
pub struct Store {
    pub clock: clock::State,
    pub config: config::State,
    pub compositor: compositor::State,
    pub state: state::State,
    pub theme: theme::State,
    pub media: media::State,
    pub lyrics: lyrics::State,
    pub visualizer: visualizer::State,
    pub devices: devices::State,
    pub display: display::State,
    pub commands: commands::State,
    pub bar_paint: barpaint::State,
    pub wallpaper: wallpaper::State,
    pub tray: tray::State,
    pub herdr: herdr::State,
    pub appicon: appicon::State,
    pub info: info::State,
    pub caffeinate: caffeinate::State,
    pub plugins: plugins::State,
    pub recording: recording::State,
    pub capture: capture::Inbox,
    pub notifications: notifications::State,
    pub menu: menu::State,
    pub brightness: brightness::State,
    pub nightlight: nightlight::State,
    pub lights: lights::State,
    pub screensaver: screensaver::State,
    pub clipboard: clipboard::State,
    pub clipssh: clipssh::State,
    pub picker: picker::State,
    pub localsend: localsend::State,
    pub mirror: mirror::State,
    /// Radio Browser answers and the country outlines, waiting for the
    /// atlas to take them.
    pub atlas: Vec<crate::services::radio::Reply>,
}

/// One service's change, tagged with the slice it applies to.
pub enum Diff {
    Clock(clock::Diff),
    Config(config::Diff),
    Compositor(compositor::Diff),
    State(state::Diff),
    Theme(theme::Diff),
    Media(media::Diff),
    Lyrics(lyrics::Diff),
    Visualizer(visualizer::Diff),
    Devices(devices::Diff),
    Display(display::Diff),
    Commands(commands::Diff),
    BarPaint(barpaint::Diff),
    Wallpaper(wallpaper::Diff),
    Tray(tray::Diff),
    Herdr(herdr::Diff),
    AppIcon(appicon::Diff),
    Info(info::Diff),
    Caffeinate(caffeinate::Diff),
    Plugins(plugins::Diff),
    #[allow(dead_code)]
    Recording(recording::Diff),
    Capture(capture::Event),
    Notifications(notifications::Diff),
    Menu(menu::Diff),
    Brightness(brightness::Diff),
    NightLight(nightlight::Diff),
    Lights(lights::Diff),
    Screensaver(screensaver::Diff),
    Clipboard(clipboard::Diff),
    Clipssh(clipssh::Diff),
    Picker(picker::Diff),
    Localsend(localsend::Diff),
    Mirror(mirror::Diff),
    Atlas(crate::services::radio::Reply),
}

/// The slice a diff changed, for the surfaces that read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Topic {
    Clock,
    Config,
    Compositor,
    State,
    Theme,
    Media,
    Lyrics,
    Visualizer,
    Devices,
    Display,
    Commands,
    BarPaint,
    Wallpaper,
    Tray,
    Herdr,
    AppIcon,
    Info,
    Caffeinate,
    /// The plugin list changed: the layout resolves again.
    Plugins,
    /// What a plugin shows changed.
    PluginOutput,
    Recording,
    Capture,
    Notifications,
    Menu,
    Brightness,
    NightLight,
    Lights,
    Screensaver,
    Clipboard,
    Clipssh,
    Picker,
    Localsend,
    Mirror,
    Atlas,
}

impl Store {
    /// What the desktop shows ([`wallpaper::source`]).
    pub fn desktop(&self) -> String {
        let data = &self.state.data;
        wallpaper::source(self.theme.theme.wallpaper_color.as_deref(), &data.wallpaper_color, &data.wallpaper)
    }

    /// `None` when the diff left the slice as it was, so nothing redraws.
    pub fn apply(&mut self, diff: Diff) -> Option<Topic> {
        let topic = self.apply_slice(diff);
        if matches!(topic, Some(Topic::Media | Topic::Config | Topic::Lyrics)) {
            lyrics::sync(&self.media, &self.config, &self.lyrics);
        }
        topic
    }

    fn apply_slice(&mut self, diff: Diff) -> Option<Topic> {
        match diff {
            Diff::Clock(d) => self.clock.apply(d).then_some(Topic::Clock),
            Diff::Config(d) => self.config.apply(d).then_some(Topic::Config),
            Diff::State(d) => self.state.apply(d).then_some(Topic::State),
            Diff::Compositor(d) => self.compositor.apply(d).then_some(Topic::Compositor),
            Diff::Theme(d) => self.theme.apply(d).then_some(Topic::Theme),
            Diff::Media(d) => self.media.apply(d).then_some(Topic::Media),
            Diff::Lyrics(d) => self.lyrics.apply(d).then_some(Topic::Lyrics),
            Diff::Visualizer(d) => self.visualizer.apply(d).then_some(Topic::Visualizer),
            Diff::Devices(d) => self.devices.apply(d).then_some(Topic::Devices),
            Diff::Display(d) => self.display.apply(d).then_some(Topic::Display),
            Diff::Commands(d) => self.commands.apply(d).then_some(Topic::Commands),
            Diff::BarPaint(d) => self.bar_paint.apply(d).then_some(Topic::BarPaint),
            Diff::Wallpaper(d) => self.wallpaper.apply(d).then_some(Topic::Wallpaper),
            Diff::Tray(d) => self.tray.apply(d).then_some(Topic::Tray),
            Diff::Herdr(d) => self.herdr.apply(d).then_some(Topic::Herdr),
            Diff::AppIcon(d) => self.appicon.apply(d).then_some(Topic::AppIcon),
            Diff::Info(d) => self.info.apply(d).then_some(Topic::Info),
            Diff::Plugins(d) => self.plugins.apply(d).map(|c| match c {
                plugins::Change::List => Topic::Plugins,
                plugins::Change::Output => Topic::PluginOutput,
            }),
            Diff::Recording(d) => self.recording.apply(d).then_some(Topic::Recording),
            Diff::Menu(d) => self.menu.apply(d).then_some(Topic::Menu),
            Diff::Caffeinate(d) => self.caffeinate.apply(d).then_some(Topic::Caffeinate),
            Diff::Notifications(d) => self.notifications.apply(d).then_some(Topic::Notifications),
            Diff::Brightness(d) => self.brightness.apply(d).then_some(Topic::Brightness),
            Diff::NightLight(d) => self.nightlight.apply(d).then_some(Topic::NightLight),
            Diff::Lights(d) => self.lights.apply(d).then_some(Topic::Lights),
            Diff::Capture(e) => self.capture.apply(e).then_some(Topic::Capture),
            Diff::Screensaver(d) => self.screensaver.apply(d).then_some(Topic::Screensaver),
            Diff::Clipboard(d) => self.clipboard.apply(d).then_some(Topic::Clipboard),
            Diff::Clipssh(d) => self.clipssh.apply(d).then_some(Topic::Clipssh),
            Diff::Picker(d) => self.picker.apply(d).then_some(Topic::Picker),
            Diff::Localsend(d) => self.localsend.apply(d).then_some(Topic::Localsend),
            Diff::Mirror(d) => self.mirror.apply(d).then_some(Topic::Mirror),
            Diff::Atlas(r) => {
                self.atlas.push(r);
                Some(Topic::Atlas)
            }
        }
    }
}
