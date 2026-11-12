# dev/smoke-all.list under ShojiWM

Every line of `dev/smoke-all.list` run with `FS_COMPOSITOR=shoji` on the mac rig's slot 3 at scale 1.25 (the rig's default), each line's last result across the first pass and the reruns, the last of them on the engine binary built from `shoji` 0bdedfe7. A leg whose lines all pass carries `leg_<n>_compositors="hyprland shoji"`. Engine causes are with the engine agent; their rows rerun once its binary lands.

Counts: shell cause fixed: 32, engine cause: 10, Hyprland only: 3, needs g815: 18, pass: 146.

| Row | Scale | Sort | Detail |
|---|---|---|---|
| `--backdrop` | 1.25 | shell cause fixed | 988e4bde |
| `--bar-style solid --join --menu-emerge` | 1.25 | shell cause fixed | 716a66be (theme.blur off in the Shoji base) |
| `--card-corner` | 1.25 | shell cause fixed | 988e4bde |
| `--display` | 1.25 | shell cause fixed | f40742f0, 988e4bde |
| `--glass` | 1.25 | shell cause fixed | 716a66be |
| `--glass-behind` | 1.25 | shell cause fixed | 61b538ed (the cover float read as a game over the output, so the shell held the OSD back) |
| `--glass-behind --no-morph` | 1.25 | shell cause fixed | 61b538ed (the cover float read as a game over the output, so the shell held the OSD back) |
| `--hdr` | 1.25 | shell cause fixed | f40742f0 |
| `--launcher-density` | 1.25 | shell cause fixed | 988e4bde |
| `--launcher-overflow` | 1.25 | shell cause fixed | f40742f0 |
| `--lock-media` | 1.25 | shell cause fixed | f40742f0 |
| `--media-progress` | 1.25 | shell cause fixed | 716a66be |
| `--menu-emerge` | 1.25 | shell cause fixed | 716a66be (theme.blur off in the Shoji base) |
| `--minimap` | 1.25 | shell cause fixed | f40742f0, 988e4bde |
| `--panel activity` | 1.25 | shell cause fixed | 716a66be (theme.blur off in the Shoji base) |
| `--panel appmenu` | 1.25 | shell cause fixed | 716a66be (theme.blur off in the Shoji base) |
| `--panel bluetooth` | 1.25 | shell cause fixed | 716a66be (theme.blur off in the Shoji base) |
| `--panel dualsense` | 1.25 | shell cause fixed | 716a66be (theme.blur off in the Shoji base) |
| `--panel earbuds` | 1.25 | shell cause fixed | 716a66be (theme.blur off in the Shoji base) |
| `--panel github` | 1.25 | shell cause fixed | 716a66be (theme.blur off in the Shoji base) |
| `--panel iphone` | 1.25 | shell cause fixed | 716a66be (theme.blur off in the Shoji base) |
| `--panel usage` | 1.25 | shell cause fixed | 716a66be (theme.blur off in the Shoji base) |
| `--panel-emerge` | 1.25 | shell cause fixed | 716a66be (theme.blur off in the Shoji base) |
| `--radio` | 1.25 | shell cause fixed | b4b9818f |
| `--reactive-rims` | 1.25 | shell cause fixed | 716a66be (theme.blur off in the Shoji base) |
| `--record` | 1.25 | shell cause fixed | b4b9818f |
| `--render` | 1.25 | shell cause fixed | 58a3f1a3 (settled before the first wallpaper picture) |
| `--settings-ui --light` | 1.25 | shell cause fixed | b26f732e (header check past the card border, failed on Hyprland too) |
| `--spaces` | 1.25 | shell cause fixed | 9a5b626d (shoji-ctl moves the window its own spawn mapped) |
| `--wallpaper` | 1.25 | shell cause fixed | f40742f0 |
| `--wheel` | 1.25 | shell cause fixed | 716a66be (sourceless wheel axis, core traced wlrctl's finger source) |
| `--zen` | 1.25 | shell cause fixed | 88f4c2f7 |
| `--dock` | 1.25 | engine cause | the running binary (0bdedfe7) sends no window tag, so no window is a Mac one, the hand-over binary sends it |
| `--dock-density` | 1.25 | engine cause | the running binary (0bdedfe7) sends no window tag, so no window is a Mac one, the hand-over binary sends it |
| `--headset-card` | 1.25 | engine cause | the Escape bind consumes the key the card's terminal should also read, wm/formalwm/src/binds.rs:150 |
| `--mac-keys` | 1.25 | engine cause | the running binary (0bdedfe7) sends no window tag, so no window is a Mac one, the hand-over binary sends it |
| `--mac-menu` | 1.25 | engine cause | the running binary (0bdedfe7) sends no window tag, so no window is a Mac one, the hand-over binary sends it |
| `--mac-menu --sidebar` | 1.25 | engine cause | the running binary (0bdedfe7) sends no window tag, so no window is a Mac one, the hand-over binary sends it |
| `--multi-output` | 1.25 | engine cause | no output focus from the pointer, focus_output's one caller is wm/formalwm/src/gestures.rs:86, so the live bar stays on the first output |
| `--sidebar` | 1.25 | engine cause | ShojiWM refuses formalwm's focus of an older window (shojiwm_lib/src/input.rs:3817 at 85f78ef), the keyboard stays on the newer probe and its click focuses nothing |
| `--sidebar --light` | 1.25 | engine cause | ShojiWM refuses formalwm's focus of an older window (shojiwm_lib/src/input.rs:3817 at 85f78ef), the keyboard stays on the newer probe and its click focuses nothing |
| `--switcher-keys` | 1.25 | engine cause | every transparent release bind of the released modifier fires whatever mods it names (shojiwm_lib/src/input.rs:222 at 85f78ef), Hyprland still counts Alt held |
| `--hypr-motion` | 1.25 | Hyprland only | Hyprland's animation config |
| `--mac-faults` | 1.25 | Hyprland only | the include's mac submap under reloads, a shell gone and late asks, formalwm's hand over ends with the shell's connection instead |
| `--overview` | 1.25 | Hyprland only | the shell's overview, formalwm draws its own (spec section 4) |
| `--device-routes` | 1.25 | needs g815 | a budget or the VM rig |
| `--firefox-install` | 1.25 | needs g815 | a budget or the VM rig |
| `--idle` | 1.25 | needs g815 | a budget or the VM rig |
| `--idle --bluez-rssi` | 1.25 | needs g815 | a budget or the VM rig |
| `--launcher-send` | 1.25 | needs g815 | a budget or the VM rig |
| `--localsend` | 1.25 | needs g815 | a budget or the VM rig |
| `--logind-lock` | 1.25 | needs g815 | a budget or the VM rig |
| `--menu-budget` | 1.25 | needs g815 | a budget or the VM rig |
| `--mirror` | 1.25 | needs g815 | a budget or the VM rig |
| `--panel-morph` | 1.25 | needs g815 | a budget or the VM rig |
| `--polkit` | 1.25 | needs g815 | a budget or the VM rig |
| `--power` | 1.25 | needs g815 | a budget or the VM rig |
| `--profiles-pill` | 1.25 | needs g815 | a budget or the VM rig |
| `--quick-settings-devices` | 1.25 | needs g815 | a budget or the VM rig |
| `--quick-settings-devices --light` | 1.25 | needs g815 | a budget or the VM rig |
| `--quick-settings-power` | 1.25 | needs g815 | a budget or the VM rig |
| `--sleep` | 1.25 | needs g815 | a budget or the VM rig |
| `--wifi` | 1.25 | needs g815 | a budget or the VM rig |
| `--activity` | 1.25 | pass | passed on rerun after a first-pass failure |
| `--airplay` | 1.25 | pass |  |
| `--app-grid` | 1.25 | pass |  |
| `--app-install` | 1.25 | pass |  |
| `--appmenu` | 1.25 | pass |  |
| `--backend gpu --dump` | 1.25 | pass |  |
| `--bar-adaptive` | 1.25 | pass |  |
| `--bar-ink` | 1.25 | pass |  |
| `--bar-layout` | 1.25 | pass |  |
| `--bar-position bottom` | 1.25 | pass |  |
| `--bar-position left` | 1.25 | pass |  |
| `--bar-room` | 1.25 | pass |  |
| `--bar-style corners` | 1.25 | pass | passed on rerun after a first-pass failure |
| `--bar-style corners --frame` | 1.25 | pass |  |
| `--bar-style solid --shoulders` | 1.25 | pass |  |
| `--bar-title` | 1.25 | pass |  |
| `--base` | 1.25 | pass |  |
| `--bluez-player` | 1.25 | pass | passed on rerun after a first-pass failure |
| `--brightness` | 1.25 | pass | passed on rerun after a first-pass failure |
| `--caffeinate` | 1.25 | pass |  |
| `--capture` | 1.25 | pass |  |
| `--capture-edit` | 1.25 | pass |  |
| `--center` | 1.25 | pass |  |
| `--chevron` | 1.25 | pass | passed on rerun after a first-pass failure |
| `--chevron-quiet` | 1.25 | pass |  |
| `--clipboard` | 1.25 | pass |  |
| `--clipssh` | 1.25 | pass |  |
| `--clipssh-image` | 1.25 | pass |  |
| `--config-reload` | 1.25 | pass |  |
| `--console` | 1.25 | pass |  |
| `--deform` | 1.25 | pass |  |
| `--desktop-clock` | 1.25 | pass |  |
| `--dump` | 1.25 | pass |  |
| `--earbuds` | 1.25 | pass |  |
| `--emoji` | 1.25 | pass |  |
| `--flexoki` | 1.25 | pass |  |
| `--frame` | 1.25 | pass |  |
| `--fullscreen` | 1.25 | pass | passed on rerun after a first-pass failure |
| `--gallery` | 1.25 | pass |  |
| `--gpu` | 1.25 | pass |  |
| `--grid-relaunch` | 1.25 | pass |  |
| `--host-binds` | 1.25 | pass |  |
| `--hotcorner` | 1.25 | pass | passed on rerun after a first-pass failure |
| `--hotcorner-relock` | 1.25 | pass |  |
| `--instance` | 1.25 | pass |  |
| `--ipad` | 1.25 | pass | passed on rerun after a first-pass failure |
| `--ipad --light` | 1.25 | pass | passed on rerun after a first-pass failure |
| `--iphone` | 1.25 | pass |  |
| `--join` | 1.25 | pass |  |
| `--keybinds` | 1.25 | pass |  |
| `--kopuz` | 1.25 | pass |  |
| `--launch` | 1.25 | pass |  |
| `--launcher-abbrev` | 1.25 | pass |  |
| `--launcher-actions` | 1.25 | pass |  |
| `--launcher-apps` | 1.25 | pass |  |
| `--launcher-install` | 1.25 | pass |  |
| `--launcher-install --light` | 1.25 | pass |  |
| `--launcher-modes` | 1.25 | pass |  |
| `--launcher-mouse` | 1.25 | pass |  |
| `--launcher-preview` | 1.25 | pass |  |
| `--light --gallery` | 1.25 | pass |  |
| `--lights` | 1.25 | pass |  |
| `--lock` | 1.25 | pass |  |
| `--lock --depth` | 1.25 | pass |  |
| `--lock-restore` | 1.25 | pass |  |
| `--login-lock` | 1.25 | pass | passed on rerun after a first-pass failure |
| `--lyrics` | 1.25 | pass |  |
| `--lyrics-blur` | 1.25 | pass |  |
| `--mac-launcher` | 1.25 | pass |  |
| `--media` | 1.25 | pass |  |
| `--menu` | 1.25 | pass |  |
| `--menu-actions` | 1.25 | pass |  |
| `--menu-hover` | 1.25 | pass |  |
| `--menu-morph` | 1.25 | pass |  |
| `--menu-resize` | 1.25 | pass |  |
| `--menu-rows` | 1.25 | pass |  |
| `--mic` | 1.25 | pass |  |
| `--monitor` | 1.25 | pass |  |
| `--motion-art` | 1.25 | pass |  |
| `--nightlight` | 1.25 | pass |  |
| `--nix-run` | 1.25 | pass |  |
| `--no-morph --panel-emerge` | 1.25 | pass |  |
| `--notify` | 1.25 | pass |  |
| `--notify-close` | 1.25 | pass |  |
| `--notify-emerge` | 1.25 | pass |  |
| `--ocr` | 1.25 | pass |  |
| `--osd` | 1.25 | pass |  |
| `--overnight` | 1.25 | pass |  |
| `--panel audio` | 1.25 | pass |  |
| `--panel calendar` | 1.25 | pass |  |
| `--panel display` | 1.25 | pass |  |
| `--panel media` | 1.25 | pass |  |
| `--panel monitor` | 1.25 | pass |  |
| `--panel network` | 1.25 | pass |  |
| `--panel network --tooltip` | 1.25 | pass |  |
| `--panel network --tooltip-travel` | 1.25 | pass |  |
| `--panel power` | 1.25 | pass |  |
| `--panel systemupdate` | 1.25 | pass |  |
| `--panel tailscale` | 1.25 | pass | passed on rerun after a first-pass failure |
| `--panel trayoverflow` | 1.25 | pass |  |
| `--panel weather` | 1.25 | pass |  |
| `--panel-anchor` | 1.25 | pass |  |
| `--panel-at 1` | 1.25 | pass |  |
| `--panel-handoff` | 1.25 | pass |  |
| `--panel-keys` | 1.25 | pass | passed on rerun after a first-pass failure |
| `--picker` | 1.25 | pass |  |
| `--picker --light` | 1.25 | pass |  |
| `--plugins` | 1.25 | pass | passed on rerun after a first-pass failure |
| `--processes` | 1.25 | pass |  |
| `--quick-settings` | 1.25 | pass | passed on rerun after a first-pass failure |
| `--quick-settings --light` | 1.25 | pass | passed on rerun after a first-pass failure |
| `--radio-atlas` | 1.25 | pass | passed on rerun after a first-pass failure |
| `--radio-atlas-cold` | 1.25 | pass |  |
| `--reminder` | 1.25 | pass |  |
| `--screensaver` | 1.25 | pass |  |
| `--screensaver-inhibit` | 1.25 | pass |  |
| `--screenshot` | 1.25 | pass |  |
| `--settings-ui` | 1.25 | pass |  |
| `--share` | 1.25 | pass |  |
| `--showcase` | 1.25 | pass |  |
| `--sidebar-activity` | 1.25 | pass |  |
| `--sidebar-join` | 1.25 | pass |  |
| `--sidebar-join --light` | 1.25 | pass |  |
| `--sidebar-software` | 1.25 | pass |  |
| `--sidebar-software --light` | 1.25 | pass |  |
| `--sky` | 1.25 | pass |  |
| `--spectrum` | 1.25 | pass | passed on rerun after a first-pass failure |
| `--speedtest` | 1.25 | pass | passed on rerun after a first-pass failure |
| `--switcher` | 1.25 | pass |  |
| `--switcher-off` | 1.25 | pass | passed on rerun after a first-pass failure |
| `--systemupdate` | 1.25 | pass |  |
| `--theme-auto` | 1.25 | pass |  |
| `--theme-toggle` | 1.25 | pass |  |
| `--toast-swipe` | 1.25 | pass |  |
| `--toggles` | 1.25 | pass |  |
| `--tray` | 1.25 | pass |  |
| `--tray --sidebar` | 1.25 | pass |  |
| `--tray-overflow` | 1.25 | pass |  |
| `--tray-overflow --sidebar` | 1.25 | pass |  |
| `--unlock` | 1.25 | pass |  |
| `--use` | 1.25 | pass | passed on rerun after a first-pass failure |
| `--visualizer` | 1.25 | pass |  |
| `--visualizer-styles` | 1.25 | pass |  |
| `--wallpaper-pair` | 1.25 | pass |  |
| `--weather-locate` | 1.25 | pass |  |
| `--workspaces` | 1.25 | pass |  |
