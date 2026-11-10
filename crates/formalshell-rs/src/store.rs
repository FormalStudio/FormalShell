//! The state surfaces read. Services own their slice's shape and how a
//! diff applies to it; the UI thread is the only writer, through
//! `App::apply`, which runs [`Store::apply`] and then what the change asks of
//! services and surfaces.

use crate::services::devices::{self, earbuds, network};
use crate::services::info::{calendar, github, monitor, powerflow, procs, tailscale, update, usage, weather};
use crate::services::{
    activity, agents, appicon, barpaint, brightness, caffeinate, capture, clipboard, clipssh, clock, commands, config, depth, display,
    compositor, ipad, iphone, launch, lights, localsend, lyrics, media, menu, mirror, nightlight, notifications, packages, picker, plugins, radio, recording, screensaver, state, theme,
    tray, visualizer, wallpaper,
};

/// Declares every slice once: its `Store` field, its `Diff` variant, its
/// `Topic` and how a diff applies. An entry applies through the slice's
/// own `apply` unless it says `{ set }` (the diff is the whole new value)
/// or gives a closure returning the topic itself; names after a comma are
/// topics that closure may return beside the slice's own.
macro_rules! slices {
    (@apply $store:ident . $field:ident, $variant:ident, $d:ident,) => {
        $store.$field.apply($d).then_some(Topic::$variant)
    };
    (@apply $store:ident . $field:ident, $variant:ident, $d:ident, set) => {
        set(&mut $store.$field, $d).then_some(Topic::$variant)
    };
    (@apply $store:ident . $field:ident, $variant:ident, $d:ident, $f:expr) => {
        route(&mut $store.$field, $d, $f)
    };
    ($( $(#[$m:meta])* $field:ident: $state:ty => $variant:ident($diff:ty) $({ $($how:tt)* })? $(, $(#[$xm:meta])* $extra:ident)* ; )*) => {
        #[derive(Default)]
        pub struct Store {
            $( $(#[$m])* pub $field: $state, )*
        }

        /// One service's change, tagged with the slice it applies to.
        pub enum Diff {
            $( $(#[$m])* $variant($diff), )*
        }

        /// The slice a diff changed, for the surfaces that read it.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Topic {
            $( $(#[$m])* $variant, $( $(#[$xm])* $extra, )* )*
        }

        impl Store {
            fn apply_slice(&mut self, diff: Diff) -> Option<Topic> {
                let store = self;
                match diff {
                    $( Diff::$variant(d) => slices!(@apply store.$field, $variant, d, $($($how)*)?), )*
                }
            }
        }
    };
}

slices! {
    clock: clock::State => Clock(clock::Diff);
    config: config::State => Config(config::Diff);
    compositor: compositor::State => Compositor(compositor::Diff);
    state: state::State => State(state::Diff);
    theme: theme::State => Theme(theme::Diff);
    media: media::State => Media(media::Diff);
    lyrics: lyrics::State => Lyrics(lyrics::Diff);
    visualizer: visualizer::State => Visualizer(visualizer::Diff);
    bluetooth: devices::Bluetooth => Bluetooth(devices::Bluetooth) { set };
    network: devices::Network => Network(devices::Network) { set };
    audio: devices::Audio => Audio(devices::Audio) { set };
    power: devices::Power => Power(devices::Power) { set };
    earbuds: earbuds::Earbuds => Earbuds(earbuds::Diff);
    /// The DualSense's lightbar and player LEDs.
    pad_lights: devices::power::Lights => PadLights(devices::power::Lights) { set };
    profiles: Option<fs_upower::ProfilesState> => Profiles(Option<fs_upower::ProfilesState>) { set };
    /// A saved Wi-Fi passphrase on its way to the launcher, which takes it.
    revealed: Option<network::Revealed> => Revealed(network::Revealed) { |slot, v| {
        *slot = Some(v);
        Some(Topic::Revealed)
    } };
    /// What logind lets this session do, as last asked.
    logind: crate::services::power::Can => Logind(crate::services::power::Can) { set };
    display: display::State => Display(display::Diff);
    commands: commands::State => Commands(commands::Diff);
    bar_paint: barpaint::State => BarPaint(barpaint::Diff);
    wallpaper: wallpaper::State => Wallpaper(wallpaper::Diff);
    tray: tray::State => Tray(tray::Diff);
    agents: agents::State => Agents(agents::Diff);
    appicon: appicon::State => AppIcon(appicon::Diff);
    weather: weather::State => Weather(weather::State) { set };
    github: github::State => Github(github::State) { set };
    usage: usage::State => Usage(usage::State) { set };
    system_update: update::State => SystemUpdate(update::State) { set };
    tailscale: tailscale::State => Tailscale(tailscale::State) { set };
    monitor: monitor::State => Monitor(Box<monitor::State>) { |slot, v| set(slot, *v).then_some(Topic::Monitor) };
    iphone: iphone::State => Iphone(iphone::State) { set };
    calendar: calendar::State => Calendar(calendar::State) { set };
    procs: procs::State => Procs(procs::Diff);
    power_flow: powerflow::State => PowerFlow(powerflow::State) { set };
    /// None until the first read lands.
    about: Option<fs_info::about::About> => About(fs_info::about::About) { |slot, v| set(slot, Some(v)).then_some(Topic::About) };
    caffeinate: caffeinate::State => Caffeinate(caffeinate::Diff);
    /// The plugin list changed: the layout resolves again.
    plugins: plugins::State => Plugins(plugins::Diff) { |slot, d| slot.apply(d).map(|c| match c {
        plugins::Change::List => Topic::Plugins,
        plugins::Change::Output => Topic::PluginOutput,
    }) },
        /// What a plugin shows changed.
        PluginOutput;
    recording: recording::State => Recording(recording::Diff);
    /// Desktop entries spawned and not yet seen with a new window.
    launches: launch::State => Launch(launch::Diff);
    capture: capture::Inbox => Capture(capture::Event);
    notifications: notifications::State => Notifications(notifications::Diff);
    activity: activity::State => Activity(activity::Diff);
    menu: menu::State => Menu(menu::Diff);
    brightness: brightness::State => Brightness(brightness::Diff);
    nightlight: nightlight::State => NightLight(nightlight::Diff);
    lights: lights::State => Lights(lights::Diff);
    screensaver: screensaver::State => Screensaver(screensaver::Diff);
    clipboard: clipboard::State => Clipboard(clipboard::Diff);
    clipssh: clipssh::State => Clipssh(clipssh::Diff);
    packages: packages::State => Packages(packages::Diff);
    picker: picker::State => Picker(picker::Diff);
    localsend: localsend::State => Localsend(localsend::Diff);
    depth: depth::State => Depth(depth::Diff);
    mirror: mirror::State => Mirror(mirror::Diff);
    ipad: ipad::State => Ipad(ipad::Diff);
    /// Radio Browser answers and the country outlines, waiting for the
    /// atlas to take them.
    atlas: Vec<radio::Reply> => Atlas(radio::Reply) { |slot, r| {
        slot.push(r);
        Some(Topic::Atlas)
    } };
}

fn set<T: PartialEq>(slot: &mut T, v: T) -> bool {
    let changed = *slot != v;
    *slot = v;
    changed
}

/// A slice that reads another: a change to the first is applied to the
/// second as this diff.
fn implied(store: &Store, topic: Topic) -> Option<Diff> {
    match topic {
        // The theme resolves its settings out of settings.json.
        Topic::Config => Some(Diff::Theme(theme::Diff::Settings(store.config.settings().clone()))),
        _ => None,
    }
}

/// What one diff moved: its own slice, and the slice its change implies.
#[derive(Clone, Copy, Debug, Default)]
pub struct Moved {
    pub topic: Option<Topic>,
    pub implied: Option<Topic>,
}

fn route<S, D>(slot: &mut S, diff: D, apply: impl FnOnce(&mut S, D) -> Option<Topic>) -> Option<Topic> {
    apply(slot, diff)
}

impl Store {
    /// What the desktop shows ([`wallpaper::desktop`]).
    pub fn desktop(&self) -> String {
        self.desktop_for(&self.state.data.mode)
    }

    /// What the desktop shows in `mode`.
    pub fn desktop_for(&self, mode: &str) -> String {
        wallpaper::desktop(crate::settings::wallpaper_color(&self.config).as_deref(), &self.state.data, mode, self.sky_step())
    }

    /// The theme engine's inputs, once both files have been read.
    pub fn theme_inputs(&self) -> Option<theme::Inputs> {
        if !self.config.loaded || !self.state.loaded {
            return None;
        }
        let data = &self.state.data;
        Some(theme::Inputs {
            settings: self.config.settings().clone(),
            wallpaper: self.desktop(),
            mode: data.mode.clone(),
            mode_override: theme::parse_override(&data.mode_override),
            location: crate::settings::location(&self.config),
            sky_at: self.theme.sky_at,
        })
    }

    /// The step a sky desktop shows: the engine's last reading, else the
    /// sun read now, until the engine has looked.
    pub fn sky_step(&self) -> i32 {
        let read = || theme::Sky::read(crate::settings::location(&self.config), self.theme.sky_at);
        self.theme.sky.unwrap_or_else(read).step
    }

    /// Applies `diff` and the diff its change implies. A topic is `None`
    /// when its slice was left as it was, so nothing redraws for it.
    pub fn apply(&mut self, diff: Diff) -> Moved {
        let Some(topic) = self.apply_slice(diff) else { return Moved::default() };
        let implied = implied(self, topic).and_then(|d| self.apply_slice(d));
        Moved { topic: Some(topic), implied }
    }
}
