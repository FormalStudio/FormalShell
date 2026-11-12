# dev/smoke-all.list under ShojiWM

Every line of `dev/smoke-all.list` run with `FS_COMPOSITOR=shoji` on the mac rig's slot 3 at scale 1.25 (the rig's default), each line's last result across the first pass and the reruns. A leg whose lines all pass carries `leg_<n>_compositors="hyprland shoji"`. Engine and core causes are with the engine agent; their rows rerun once its binary lands.

Counts: shell cause fixed: 28, engine cause: 11, core cause: 12, Hyprland only: 2, needs g815: 18, pass: 131.

| Row | Scale | Sort | Detail |
|---|---|---|---|
| `--backdrop` | 1.25 | shell cause fixed | 988e4bde |
| `--bar-style solid --join --menu-emerge` | 1.25 | shell cause fixed | 716a66be (theme.blur off in the Shoji base) |
| `--card-corner` | 1.25 | shell cause fixed | 988e4bde |
| `--display` | 1.25 | shell cause fixed | f40742f0, 988e4bde |
| `--glass` | 1.25 | shell cause fixed | 716a66be |
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
| `--wallpaper` | 1.25 | shell cause fixed | f40742f0 |
| `--wheel` | 1.25 | shell cause fixed | 716a66be (sourceless wheel axis, core traced wlrctl's finger source) |
| `--zen` | 1.25 | shell cause fixed | 88f4c2f7 |
| `--bar-style corners` | 1.25 | engine cause | state.get reserved edges, 92dbe8db, shoji-ctl mapping in 716a66be |
| `--fullscreen` | 1.25 | engine cause | pointer lock outlives focus loss |
| `--glass-behind` | 1.25 | engine cause | keyboard stays on a moved window |
| `--glass-behind --no-morph` | 1.25 | engine cause | keyboard stays on a moved window |
| `--headset-card` | 1.25 | engine cause | binds.list Escape row |
| `--hotcorner` | 1.25 | engine cause | a removed output's layers stay listed |
| `--multi-output` | 1.25 | engine cause | a removed output's layers stay listed |
| `--sidebar` | 1.25 | engine cause | border measured inside the gap, fix in next binary |
| `--sidebar --light` | 1.25 | engine cause | border measured inside the gap, fix in next binary |
| `--spaces` | 1.25 | engine cause | keyboard stays on a moved window |
| `--switcher-keys` | 1.25 | engine cause | release binds on one modifier share passthrough |
| `--activity` | 1.25 | core cause | keys into a freshly mapped panel, core 57588f0 (in engine core 50100de), awaiting binary |
| `--bluez-player` | 1.25 | core cause | keys into a freshly mapped panel, core 57588f0 (in engine core 50100de), awaiting binary |
| `--brightness` | 1.25 | core cause | keys into a freshly mapped panel, core 57588f0 (in engine core 50100de), awaiting binary |
| `--chevron` | 1.25 | core cause | keys into a freshly mapped panel, core 57588f0 (in engine core 50100de), awaiting binary |
| `--ipad` | 1.25 | core cause | keys into a freshly mapped panel, core 57588f0 (in engine core 50100de), awaiting binary |
| `--ipad --light` | 1.25 | core cause | keys into a freshly mapped panel, core 57588f0 (in engine core 50100de), awaiting binary |
| `--panel-keys` | 1.25 | core cause | keys into a freshly mapped panel, core 57588f0 (in engine core 50100de), awaiting binary |
| `--plugins` | 1.25 | core cause | keys into a freshly mapped panel, core 57588f0 (in engine core 50100de), awaiting binary |
| `--quick-settings` | 1.25 | core cause | keys into a freshly mapped panel, core 57588f0 (in engine core 50100de), awaiting binary |
| `--quick-settings --light` | 1.25 | core cause | keys into a freshly mapped panel, core 57588f0 (in engine core 50100de), awaiting binary |
| `--settings-ui --light` | 1.25 | core cause | keys into a freshly mapped panel, core 57588f0 (in engine core 50100de), awaiting binary |
| `--use` | 1.25 | core cause | keys into a freshly mapped panel, core 57588f0 (in engine core 50100de), awaiting binary |
| `--hypr-motion` | 1.25 | Hyprland only | Hyprland's animation config |
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
| `--bar-style corners --frame` | 1.25 | pass |  |
| `--bar-style solid --shoulders` | 1.25 | pass |  |
| `--bar-title` | 1.25 | pass |  |
| `--base` | 1.25 | pass |  |
| `--caffeinate` | 1.25 | pass |  |
| `--capture` | 1.25 | pass |  |
| `--capture-edit` | 1.25 | pass |  |
| `--center` | 1.25 | pass |  |
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
| `--gallery` | 1.25 | pass |  |
| `--gpu` | 1.25 | pass |  |
| `--grid-relaunch` | 1.25 | pass |  |
| `--host-binds` | 1.25 | pass |  |
| `--hotcorner-relock` | 1.25 | pass |  |
| `--instance` | 1.25 | pass |  |
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
| `--picker` | 1.25 | pass |  |
| `--picker --light` | 1.25 | pass |  |
| `--processes` | 1.25 | pass |  |
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
| `--visualizer` | 1.25 | pass |  |
| `--visualizer-styles` | 1.25 | pass |  |
| `--wallpaper-pair` | 1.25 | pass |  |
| `--weather-locate` | 1.25 | pass |  |
| `--workspaces` | 1.25 | pass |  |
