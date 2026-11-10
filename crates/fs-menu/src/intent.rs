//! The launcher's own verbs: what a row asks of the launcher or the shell
//! that no IPC target answers. A menu file spells one as an `@ipc:` call
//! (`@ipc:wifi.activate:Home`), so the text is read once into a variant
//! here and runs through one match on the other side.

use crate::actions::IpcCall;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    // The row's own: the launcher answers these without the App.
    /// Forget the abbreviation this query is for the row's subject.
    AbbrevForget(String),
    AbbrevPin(String),
    /// A subject pinned to the root's `Recent`, by subject id.
    RecentPin(String),
    RecentUnpin(String),
    /// A line to a launcher plugin: its id, then the line.
    PluginSend(String, String),
    WindowFocus(String),
    WindowClose(String),
    WindowFloat(String),
    WindowFullscreen(String),
    /// An app's window focused, else the app launched, by desktop id.
    AppOpen(String),
    AppNew(String),
    /// A desktop action: the app's desktop id, then the action's key.
    AppAction(String, String),
    WallpaperSet(String),
    /// One mode's slot alone: `light` or `dark`, then the picture.
    WallpaperSetFor(String, String),
    /// TERM to a process, asked twice.
    EndProcess(u64),
    // The App's.
    WifiEnable,
    WifiActivate(String),
    WifiForget(String),
    WifiReveal(String),
    BluetoothPower(String),
    BluetoothToggle(String),
    BluetoothForget(String),
    AudioSink(String),
    AudioSource(String),
    RadioStop,
    /// A favourite played, by station uuid.
    RadioFav(String),
    /// A search result played, by station uuid.
    RadioPlay(String),
    RadioFavorite(String),
    RadioUnfavorite(String),
    /// A clipboard entry to a LocalSend peer: the peer's index, then the
    /// entry's id.
    LocalsendSend(String, String),
    NixRun(String),
    /// A clipboard image over ssh: the entry's id, then its file.
    ClipsshImage(String, String),
    ClipsshSend(String),
    AppGpu(String),
    PackageInstall(String),
    PackageUninstall(String),
    PowerSuspend,
    PowerReboot,
    PowerOff,
    ThemeToggleMode,
    LightsColorInput,
    ReminderSet,
}

impl Intent {
    /// The verb `call` names, read off its arguments. `None` when it names
    /// none of these; an error when it names one with the wrong arguments.
    pub fn from_call(call: &IpcCall) -> Option<Result<Intent, String>> {
        use Intent::*;
        type Make = fn(&[String]) -> Option<Intent>;
        let (n, make): (usize, Make) = match (call.target.as_str(), call.function.as_str()) {
            ("abbrev", "forget") => (1, |a| Some(AbbrevForget(a[0].clone()))),
            ("abbrev", "pin") => (1, |a| Some(AbbrevPin(a[0].clone()))),
            ("recent", "pin") => (1, |a| Some(RecentPin(a[0].clone()))),
            ("recent", "unpin") => (1, |a| Some(RecentUnpin(a[0].clone()))),
            ("plugin", "send") => (2, |a| Some(PluginSend(a[0].clone(), a[1].clone()))),
            ("window", "focus") => (1, |a| Some(WindowFocus(a[0].clone()))),
            ("window", "close") => (1, |a| Some(WindowClose(a[0].clone()))),
            ("window", "float") => (1, |a| Some(WindowFloat(a[0].clone()))),
            ("window", "fullscreen") => (1, |a| Some(WindowFullscreen(a[0].clone()))),
            ("app", "open") => (1, |a| Some(AppOpen(a[0].clone()))),
            ("app", "new") => (1, |a| Some(AppNew(a[0].clone()))),
            ("app", "action") => (2, |a| Some(AppAction(a[0].clone(), a[1].clone()))),
            ("wallpaper", "set") => (1, |a| Some(WallpaperSet(a[0].clone()))),
            ("wallpaper", "setFor") => (2, |a| Some(WallpaperSetFor(a[0].clone(), a[1].clone()))),
            ("monitor", "end") => (1, |a| a[0].parse().ok().map(EndProcess)),
            ("wifi", "enable") => (0, |_| Some(WifiEnable)),
            ("wifi", "activate") => (1, |a| Some(WifiActivate(a[0].clone()))),
            ("wifi", "forget") => (1, |a| Some(WifiForget(a[0].clone()))),
            ("wifi", "reveal") => (1, |a| Some(WifiReveal(a[0].clone()))),
            ("bluetooth", "power") => (1, |a| Some(BluetoothPower(a[0].clone()))),
            ("bluetooth", "toggle") => (1, |a| Some(BluetoothToggle(a[0].clone()))),
            ("bluetooth", "forget") => (1, |a| Some(BluetoothForget(a[0].clone()))),
            ("audio", "sink") => (1, |a| Some(AudioSink(a[0].clone()))),
            ("audio", "source") => (1, |a| Some(AudioSource(a[0].clone()))),
            ("radio", "stop") => (0, |_| Some(RadioStop)),
            ("radio", "fav") => (1, |a| Some(RadioFav(a[0].clone()))),
            ("radio", "play") => (1, |a| Some(RadioPlay(a[0].clone()))),
            ("radio", "favorite") => (1, |a| Some(RadioFavorite(a[0].clone()))),
            ("radio", "unfavorite") => (1, |a| Some(RadioUnfavorite(a[0].clone()))),
            ("localsend", "send") => (2, |a| Some(LocalsendSend(a[0].clone(), a[1].clone()))),
            ("nix", "run") => (1, |a| Some(NixRun(a[0].clone()))),
            ("clipssh", "image") => (2, |a| Some(ClipsshImage(a[0].clone(), a[1].clone()))),
            ("clipssh", "send") => (1, |a| Some(ClipsshSend(a[0].clone()))),
            ("app", "gpu") => (1, |a| Some(AppGpu(a[0].clone()))),
            ("package", "install") => (1, |a| Some(PackageInstall(a[0].clone()))),
            ("package", "uninstall") => (1, |a| Some(PackageUninstall(a[0].clone()))),
            ("power", "suspend") => (0, |_| Some(PowerSuspend)),
            ("power", "reboot") => (0, |_| Some(PowerReboot)),
            ("power", "poweroff") => (0, |_| Some(PowerOff)),
            ("theme", "toggleMode") => (0, |_| Some(ThemeToggleMode)),
            ("lights", "colorInput") => (0, |_| Some(LightsColorInput)),
            ("reminder", "set") => (0, |_| Some(ReminderSet)),
            _ => return None,
        };
        let fit = call.clone().fitted(n);
        if fit.args.len() != n {
            return Some(Err(format!("{call} takes {n} arguments, given {}", call.args.len())));
        }
        Some(make(&fit.args).ok_or_else(|| format!("{call}: {:?} is not a pid", fit.at(0))))
    }

    /// The call a menu file spells this as.
    pub fn call(&self) -> IpcCall {
        use Intent::*;
        let c = IpcCall::new;
        match self {
            AbbrevForget(q) => c("abbrev", "forget").arg(q),
            AbbrevPin(q) => c("abbrev", "pin").arg(q),
            RecentPin(id) => c("recent", "pin").arg(id),
            RecentUnpin(id) => c("recent", "unpin").arg(id),
            PluginSend(id, line) => c("plugin", "send").arg(id).arg(line),
            WindowFocus(id) => c("window", "focus").arg(id),
            WindowClose(id) => c("window", "close").arg(id),
            WindowFloat(id) => c("window", "float").arg(id),
            WindowFullscreen(id) => c("window", "fullscreen").arg(id),
            AppOpen(id) => c("app", "open").arg(id),
            AppNew(id) => c("app", "new").arg(id),
            AppAction(id, key) => c("app", "action").arg(id).arg(key),
            WallpaperSet(path) => c("wallpaper", "set").arg(path),
            WallpaperSetFor(half, path) => c("wallpaper", "setFor").arg(half).arg(path),
            EndProcess(pid) => c("monitor", "end").arg(pid.to_string()),
            WifiEnable => c("wifi", "enable"),
            WifiActivate(ssid) => c("wifi", "activate").arg(ssid),
            WifiForget(ssid) => c("wifi", "forget").arg(ssid),
            WifiReveal(ssid) => c("wifi", "reveal").arg(ssid),
            BluetoothPower(on) => c("bluetooth", "power").arg(on),
            BluetoothToggle(address) => c("bluetooth", "toggle").arg(address),
            BluetoothForget(address) => c("bluetooth", "forget").arg(address),
            AudioSink(name) => c("audio", "sink").arg(name),
            AudioSource(name) => c("audio", "source").arg(name),
            RadioStop => c("radio", "stop"),
            RadioFav(uuid) => c("radio", "fav").arg(uuid),
            RadioPlay(uuid) => c("radio", "play").arg(uuid),
            RadioFavorite(uuid) => c("radio", "favorite").arg(uuid),
            RadioUnfavorite(uuid) => c("radio", "unfavorite").arg(uuid),
            LocalsendSend(peer, id) => c("localsend", "send").arg(peer).arg(id),
            NixRun(package) => c("nix", "run").arg(package),
            ClipsshImage(id, path) => c("clipssh", "image").arg(id).arg(path),
            ClipsshSend(alias) => c("clipssh", "send").arg(alias),
            AppGpu(id) => c("app", "gpu").arg(id),
            PackageInstall(id) => c("package", "install").arg(id),
            PackageUninstall(id) => c("package", "uninstall").arg(id),
            PowerSuspend => c("power", "suspend"),
            PowerReboot => c("power", "reboot"),
            PowerOff => c("power", "poweroff"),
            ThemeToggleMode => c("theme", "toggleMode"),
            LightsColorInput => c("lights", "colorInput"),
            ReminderSet => c("reminder", "set"),
        }
    }

    /// Answered by a launcher row alone: anywhere else (a hot corner) the
    /// same words are an IPC call.
    pub fn row_only(&self) -> bool {
        use Intent::*;
        matches!(
            self,
            AbbrevForget(_)
                | AbbrevPin(_)
                | RecentPin(_)
                | RecentUnpin(_)
                | PluginSend(..)
                | WindowFocus(_)
                | WindowClose(_)
                | WindowFloat(_)
                | WindowFullscreen(_)
                | AppOpen(_)
                | AppNew(_)
                | AppAction(..)
                | WallpaperSet(_)
                | WallpaperSetFor(..)
                | EndProcess(_)
        )
    }
}

impl std::fmt::Display for Intent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let call = self.call();
        write!(f, "@ipc:{call}")?;
        call.args.iter().try_for_each(|a| write!(f, ":{a}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_intent_reads_back_from_its_own_call() {
        use Intent::*;
        let s = || "a:b".to_owned();
        let all = [
            AbbrevForget(s()),
            AbbrevPin(s()),
            RecentPin(s()),
            RecentUnpin(s()),
            PluginSend("p".into(), s()),
            WindowFocus(s()),
            WindowClose(s()),
            WindowFloat(s()),
            WindowFullscreen(s()),
            AppOpen(s()),
            AppNew(s()),
            AppAction("app".into(), s()),
            WallpaperSet(s()),
            WallpaperSetFor("dark".into(), s()),
            EndProcess(42),
            WifiEnable,
            WifiActivate(s()),
            WifiForget(s()),
            WifiReveal(s()),
            BluetoothPower("on".into()),
            BluetoothToggle(s()),
            BluetoothForget(s()),
            AudioSink(s()),
            AudioSource(s()),
            RadioStop,
            RadioFav(s()),
            RadioPlay(s()),
            RadioFavorite(s()),
            RadioUnfavorite(s()),
            LocalsendSend("0".into(), s()),
            NixRun(s()),
            ClipsshImage("1".into(), s()),
            ClipsshSend(s()),
            AppGpu(s()),
            PackageInstall(s()),
            PackageUninstall(s()),
            PowerSuspend,
            PowerReboot,
            PowerOff,
            ThemeToggleMode,
            LightsColorInput,
            ReminderSet,
        ];
        for intent in all {
            let text = intent.to_string();
            let call = IpcCall::parse(text.strip_prefix("@ipc:").unwrap());
            assert_eq!(Intent::from_call(&call), Some(Ok(intent)), "{text}");
        }
    }

    #[test]
    fn a_verb_with_the_wrong_arguments_says_so() {
        let err = |t: &str| Intent::from_call(&IpcCall::parse(t)).unwrap().unwrap_err();
        assert_eq!(err("app.open"), "app.open takes 1 arguments, given 0");
        assert_eq!(err("wallpaper.setFor:dark"), "wallpaper.setFor takes 2 arguments, given 1");
        assert_eq!(err("monitor.end:me"), "monitor.end: \"me\" is not a pid");
        assert_eq!(Intent::from_call(&IpcCall::parse("panel.open:audio")), None);
    }
}
