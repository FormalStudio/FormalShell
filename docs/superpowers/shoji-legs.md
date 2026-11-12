# dev/smoke-all.list under ShojiWM

Every line of `dev/smoke-all.list` run with `FS_COMPOSITOR=shoji` on the mac rig's slot 3 at scale 1.25 (the rig's default), on the shell at a6697e69 and the engine binary `formalwm` sha256 a5a825cc (`shoji` e7c0b375 on `formal-rs` e6493f8, the build `wm/VERIFIED.md` names). The Binary column is the `formalwm` each row ran. The first 112 rows of the run ran the slot's `~/fwm-target` build; a promote from another worktree replaced it at 13:51:26 UTC on 2026-11-12, so the rest ran a copy of the same a5a825cc build through `FS_SHOJI_BIN`. A line that failed was run once more on its own; the row holds that second result. A leg whose lines all pass carries `leg_<n>_compositors="hyprland shoji"`. Logs are under `artifacts/shoji-sweep/` in the worktree that ran them.

Counts: fails twice: 4, Hyprland only: 3, engine cause fixed: 7, shell cause fixed: 32, needs g815: 18, pass: 145.

| Row | Scale | Binary | Sort | Detail |
|---|---|---|---|---|
| `--dock` | 1.25 | a5a825cc | fails twice | SMOKE_FAIL: --dock: the drive stopped at a step that never came true: until_jq 10 (dock.ship.log, dock.again.log) |
| `--hotcorner-relock` | 1.25 | a5a825cc | fails twice | SMOKE_FAIL: a pointer move dropped the screensaver to the card (hotcorner-relock.full.log, hotcorner-relock.again.log) |
| `--mac-keys` | 1.25 | a5a825cc | fails twice | SMOKE_FAIL: --mac-keys: SUPER + H did not move focus back onto the stub (mac-keys.ship.log, mac-keys.again.log) |
| `--switcher-keys` | 1.25 | a5a825cc | fails twice | SMOKE_FAIL: under the scrolling layout the cells with no frame (switcher-keys.full.log, switcher-keys.again.log) |
| `--hypr-motion` | 1.25 | a5a825cc | Hyprland only | Hyprland's animation config |
| `--mac-faults` | 1.25 | a5a825cc | Hyprland only | the include's mac submap under reloads, a shell gone and late asks, formalwm's hand over ends with the shell's connection instead |
| `--overview` | 1.25 | a5a825cc | Hyprland only | the shell's overview, formalwm draws its own (spec section 4) |
| `--dock-density` | 1.25 | a5a825cc | engine cause fixed | passes on a5a825cc |
| `--headset-card` | 1.25 | a5a825cc | engine cause fixed | passes on a5a825cc |
| `--mac-menu` | 1.25 | a5a825cc | engine cause fixed | passes on a5a825cc |
| `--mac-menu --sidebar` | 1.25 | a5a825cc | engine cause fixed | passes on a5a825cc |
| `--multi-output` | 1.25 | a5a825cc | engine cause fixed | passes on a5a825cc |
| `--sidebar` | 1.25 | a5a825cc | engine cause fixed | passes on a5a825cc |
| `--sidebar --light` | 1.25 | a5a825cc | engine cause fixed | passes on a5a825cc |
| `--backdrop` | 1.25 | a5a825cc | shell cause fixed | 988e4bde |
| `--bar-style solid --join --menu-emerge` | 1.25 | a5a825cc | shell cause fixed | 716a66be (theme.blur off in the Shoji base) |
| `--card-corner` | 1.25 | a5a825cc | shell cause fixed | 988e4bde |
| `--display` | 1.25 | a5a825cc | shell cause fixed | f40742f0, 988e4bde |
| `--glass` | 1.25 | a5a825cc | shell cause fixed | 716a66be |
| `--glass-behind` | 1.25 | a5a825cc | shell cause fixed | 61b538ed (the cover float read as a game over the output, so the shell held the OSD back) |
| `--glass-behind --no-morph` | 1.25 | a5a825cc | shell cause fixed | 61b538ed (the cover float read as a game over the output, so the shell held the OSD back) |
| `--hdr` | 1.25 | a5a825cc | shell cause fixed | f40742f0 |
| `--launcher-density` | 1.25 | a5a825cc | shell cause fixed | 988e4bde |
| `--launcher-overflow` | 1.25 | a5a825cc | shell cause fixed | f40742f0 |
| `--lock-media` | 1.25 | a5a825cc | shell cause fixed | f40742f0 |
| `--media-progress` | 1.25 | a5a825cc | shell cause fixed | 716a66be |
| `--menu-emerge` | 1.25 | a5a825cc | shell cause fixed | 716a66be (theme.blur off in the Shoji base) |
| `--minimap` | 1.25 | a5a825cc | shell cause fixed | f40742f0, 988e4bde |
| `--panel activity` | 1.25 | a5a825cc | shell cause fixed | 716a66be (theme.blur off in the Shoji base) |
| `--panel appmenu` | 1.25 | a5a825cc | shell cause fixed | 716a66be (theme.blur off in the Shoji base) |
| `--panel bluetooth` | 1.25 | a5a825cc | shell cause fixed | 716a66be (theme.blur off in the Shoji base) |
| `--panel dualsense` | 1.25 | a5a825cc | shell cause fixed | 716a66be (theme.blur off in the Shoji base) |
| `--panel earbuds` | 1.25 | a5a825cc | shell cause fixed | 716a66be (theme.blur off in the Shoji base) |
| `--panel github` | 1.25 | a5a825cc | shell cause fixed | 716a66be (theme.blur off in the Shoji base) |
| `--panel iphone` | 1.25 | a5a825cc | shell cause fixed | 716a66be (theme.blur off in the Shoji base) |
| `--panel usage` | 1.25 | a5a825cc | shell cause fixed | 716a66be (theme.blur off in the Shoji base) |
| `--panel-emerge` | 1.25 | a5a825cc | shell cause fixed | 716a66be (theme.blur off in the Shoji base) |
| `--radio` | 1.25 | a5a825cc | shell cause fixed | b4b9818f |
| `--reactive-rims` | 1.25 | a5a825cc | shell cause fixed | 716a66be (theme.blur off in the Shoji base) |
| `--record` | 1.25 | a5a825cc | shell cause fixed | b4b9818f |
| `--render` | 1.25 | a5a825cc | shell cause fixed | 58a3f1a3 (settled before the first wallpaper picture) |
| `--settings-ui --light` | 1.25 | a5a825cc | shell cause fixed | b26f732e (header check past the card border, failed on Hyprland too) |
| `--spaces` | 1.25 | a5a825cc | shell cause fixed | 9a5b626d (shoji-ctl moves the window its own spawn mapped) |
| `--wallpaper` | 1.25 | a5a825cc | shell cause fixed | f40742f0 |
| `--wheel` | 1.25 | a5a825cc | shell cause fixed | 716a66be (sourceless wheel axis, core traced wlrctl's finger source) |
| `--zen` | 1.25 | a5a825cc | shell cause fixed | 88f4c2f7 |
| `--device-routes` | 1.25 | not run | needs g815 | a budget or the VM rig |
| `--firefox-install` | 1.25 | not run | needs g815 | a budget or the VM rig |
| `--idle` | 1.25 | not run | needs g815 | a budget or the VM rig |
| `--idle --bluez-rssi` | 1.25 | not run | needs g815 | a budget or the VM rig |
| `--launcher-send` | 1.25 | not run | needs g815 | a budget or the VM rig |
| `--localsend` | 1.25 | not run | needs g815 | a budget or the VM rig |
| `--logind-lock` | 1.25 | not run | needs g815 | a budget or the VM rig |
| `--menu-budget` | 1.25 | not run | needs g815 | a budget or the VM rig |
| `--mirror` | 1.25 | not run | needs g815 | a budget or the VM rig |
| `--panel-morph` | 1.25 | not run | needs g815 | a budget or the VM rig |
| `--polkit` | 1.25 | not run | needs g815 | a budget or the VM rig |
| `--power` | 1.25 | not run | needs g815 | a budget or the VM rig |
| `--profiles-pill` | 1.25 | not run | needs g815 | a budget or the VM rig |
| `--quick-settings-devices` | 1.25 | not run | needs g815 | a budget or the VM rig |
| `--quick-settings-devices --light` | 1.25 | not run | needs g815 | a budget or the VM rig |
| `--quick-settings-power` | 1.25 | not run | needs g815 | a budget or the VM rig |
| `--sleep` | 1.25 | not run | needs g815 | a budget or the VM rig |
| `--wifi` | 1.25 | not run | needs g815 | a budget or the VM rig |
| `--activity` | 1.25 | a5a825cc | pass |  |
| `--airplay` | 1.25 | a5a825cc | pass |  |
| `--app-grid` | 1.25 | a5a825cc | pass |  |
| `--app-install` | 1.25 | a5a825cc | pass |  |
| `--appmenu` | 1.25 | a5a825cc | pass |  |
| `--backend gpu --dump` | 1.25 | a5a825cc | pass |  |
| `--bar-adaptive` | 1.25 | a5a825cc | pass |  |
| `--bar-ink` | 1.25 | a5a825cc | pass |  |
| `--bar-layout` | 1.25 | a5a825cc | pass |  |
| `--bar-position bottom` | 1.25 | a5a825cc | pass |  |
| `--bar-position left` | 1.25 | a5a825cc | pass |  |
| `--bar-room` | 1.25 | a5a825cc | pass |  |
| `--bar-style corners` | 1.25 | a5a825cc | pass |  |
| `--bar-style corners --frame` | 1.25 | a5a825cc | pass |  |
| `--bar-style solid --shoulders` | 1.25 | a5a825cc | pass |  |
| `--bar-title` | 1.25 | a5a825cc | pass |  |
| `--base` | 1.25 | a5a825cc | pass |  |
| `--bluez-player` | 1.25 | a5a825cc | pass |  |
| `--brightness` | 1.25 | a5a825cc | pass |  |
| `--caffeinate` | 1.25 | a5a825cc | pass |  |
| `--capture` | 1.25 | a5a825cc | pass |  |
| `--capture-edit` | 1.25 | a5a825cc | pass |  |
| `--center` | 1.25 | a5a825cc | pass |  |
| `--chevron` | 1.25 | a5a825cc | pass |  |
| `--chevron-quiet` | 1.25 | a5a825cc | pass |  |
| `--clipboard` | 1.25 | a5a825cc | pass |  |
| `--clipssh` | 1.25 | a5a825cc | pass |  |
| `--clipssh-image` | 1.25 | a5a825cc | pass |  |
| `--config-reload` | 1.25 | a5a825cc | pass |  |
| `--console` | 1.25 | a5a825cc | pass |  |
| `--deform` | 1.25 | a5a825cc | pass |  |
| `--desktop-clock` | 1.25 | a5a825cc | pass |  |
| `--dump` | 1.25 | a5a825cc | pass |  |
| `--earbuds` | 1.25 | a5a825cc | pass |  |
| `--emoji` | 1.25 | a5a825cc | pass |  |
| `--flexoki` | 1.25 | a5a825cc | pass |  |
| `--frame` | 1.25 | a5a825cc | pass |  |
| `--fullscreen` | 1.25 | a5a825cc | pass |  |
| `--gallery` | 1.25 | a5a825cc | pass |  |
| `--gpu` | 1.25 | a5a825cc | pass |  |
| `--grid-relaunch` | 1.25 | a5a825cc | pass |  |
| `--host-binds` | 1.25 | a5a825cc | pass |  |
| `--hotcorner` | 1.25 | a5a825cc | pass |  |
| `--instance` | 1.25 | a5a825cc | pass |  |
| `--ipad` | 1.25 | a5a825cc | pass |  |
| `--ipad --light` | 1.25 | a5a825cc | pass |  |
| `--iphone` | 1.25 | a5a825cc | pass |  |
| `--join` | 1.25 | a5a825cc | pass |  |
| `--keybinds` | 1.25 | a5a825cc | pass |  |
| `--kopuz` | 1.25 | a5a825cc | pass |  |
| `--launch` | 1.25 | a5a825cc | pass |  |
| `--launcher-abbrev` | 1.25 | a5a825cc | pass |  |
| `--launcher-actions` | 1.25 | a5a825cc | pass |  |
| `--launcher-apps` | 1.25 | a5a825cc | pass |  |
| `--launcher-install` | 1.25 | a5a825cc | pass |  |
| `--launcher-install --light` | 1.25 | a5a825cc | pass |  |
| `--launcher-modes` | 1.25 | a5a825cc | pass |  |
| `--launcher-mouse` | 1.25 | a5a825cc | pass |  |
| `--launcher-preview` | 1.25 | a5a825cc | pass |  |
| `--light --gallery` | 1.25 | a5a825cc | pass |  |
| `--lights` | 1.25 | a5a825cc | pass |  |
| `--lock` | 1.25 | a5a825cc | pass |  |
| `--lock --depth` | 1.25 | a5a825cc | pass |  |
| `--lock-restore` | 1.25 | a5a825cc | pass |  |
| `--login-lock` | 1.25 | a5a825cc | pass |  |
| `--lyrics` | 1.25 | a5a825cc | pass |  |
| `--lyrics-blur` | 1.25 | a5a825cc | pass |  |
| `--mac-launcher` | 1.25 | a5a825cc | pass |  |
| `--media` | 1.25 | a5a825cc | pass |  |
| `--menu` | 1.25 | a5a825cc | pass |  |
| `--menu-actions` | 1.25 | a5a825cc | pass |  |
| `--menu-hover` | 1.25 | a5a825cc | pass |  |
| `--menu-morph` | 1.25 | a5a825cc | pass |  |
| `--menu-resize` | 1.25 | a5a825cc | pass |  |
| `--menu-rows` | 1.25 | a5a825cc | pass |  |
| `--mic` | 1.25 | a5a825cc | pass |  |
| `--monitor` | 1.25 | a5a825cc | pass |  |
| `--motion-art` | 1.25 | a5a825cc | pass |  |
| `--nightlight` | 1.25 | a5a825cc | pass |  |
| `--nix-run` | 1.25 | a5a825cc | pass |  |
| `--no-morph --panel-emerge` | 1.25 | a5a825cc | pass |  |
| `--notify` | 1.25 | a5a825cc | pass |  |
| `--notify-close` | 1.25 | a5a825cc | pass |  |
| `--notify-emerge` | 1.25 | a5a825cc | pass |  |
| `--ocr` | 1.25 | a5a825cc | pass |  |
| `--osd` | 1.25 | a5a825cc | pass |  |
| `--overnight` | 1.25 | a5a825cc | pass |  |
| `--panel audio` | 1.25 | a5a825cc | pass |  |
| `--panel calendar` | 1.25 | a5a825cc | pass |  |
| `--panel display` | 1.25 | a5a825cc | pass |  |
| `--panel media` | 1.25 | a5a825cc | pass |  |
| `--panel monitor` | 1.25 | a5a825cc | pass |  |
| `--panel network` | 1.25 | a5a825cc | pass |  |
| `--panel network --tooltip` | 1.25 | a5a825cc | pass |  |
| `--panel network --tooltip-travel` | 1.25 | a5a825cc | pass |  |
| `--panel power` | 1.25 | a5a825cc | pass |  |
| `--panel systemupdate` | 1.25 | a5a825cc | pass |  |
| `--panel tailscale` | 1.25 | a5a825cc | pass |  |
| `--panel trayoverflow` | 1.25 | a5a825cc | pass |  |
| `--panel weather` | 1.25 | a5a825cc | pass |  |
| `--panel-anchor` | 1.25 | a5a825cc | pass |  |
| `--panel-at 1` | 1.25 | a5a825cc | pass |  |
| `--panel-handoff` | 1.25 | a5a825cc | pass |  |
| `--panel-keys` | 1.25 | a5a825cc | pass |  |
| `--picker` | 1.25 | a5a825cc | pass |  |
| `--picker --light` | 1.25 | a5a825cc | pass |  |
| `--plugins` | 1.25 | a5a825cc | pass |  |
| `--processes` | 1.25 | a5a825cc | pass |  |
| `--quick-settings` | 1.25 | a5a825cc | pass |  |
| `--quick-settings --light` | 1.25 | a5a825cc | pass |  |
| `--radio-atlas` | 1.25 | a5a825cc | pass |  |
| `--radio-atlas-cold` | 1.25 | a5a825cc | pass |  |
| `--reminder` | 1.25 | a5a825cc | pass |  |
| `--screensaver` | 1.25 | a5a825cc | pass |  |
| `--screensaver-inhibit` | 1.25 | a5a825cc | pass |  |
| `--screenshot` | 1.25 | a5a825cc | pass |  |
| `--settings-ui` | 1.25 | a5a825cc | pass |  |
| `--share` | 1.25 | a5a825cc | pass |  |
| `--showcase` | 1.25 | a5a825cc | pass |  |
| `--sidebar-activity` | 1.25 | a5a825cc | pass |  |
| `--sidebar-join` | 1.25 | a5a825cc | pass |  |
| `--sidebar-join --light` | 1.25 | a5a825cc | pass |  |
| `--sidebar-software` | 1.25 | a5a825cc | pass |  |
| `--sidebar-software --light` | 1.25 | a5a825cc | pass |  |
| `--sky` | 1.25 | a5a825cc | pass |  |
| `--spectrum` | 1.25 | a5a825cc | pass |  |
| `--speedtest` | 1.25 | a5a825cc | pass |  |
| `--switcher` | 1.25 | a5a825cc | pass |  |
| `--switcher-off` | 1.25 | a5a825cc | pass |  |
| `--systemupdate` | 1.25 | a5a825cc | pass |  |
| `--theme-auto` | 1.25 | a5a825cc | pass |  |
| `--theme-toggle` | 1.25 | a5a825cc | pass |  |
| `--toast-swipe` | 1.25 | a5a825cc | pass |  |
| `--toggles` | 1.25 | a5a825cc | pass |  |
| `--tray` | 1.25 | a5a825cc | pass |  |
| `--tray --sidebar` | 1.25 | a5a825cc | pass |  |
| `--tray-overflow` | 1.25 | a5a825cc | pass |  |
| `--tray-overflow --sidebar` | 1.25 | a5a825cc | pass |  |
| `--unlock` | 1.25 | a5a825cc | pass |  |
| `--use` | 1.25 | a5a825cc | pass |  |
| `--visualizer` | 1.25 | a5a825cc | pass |  |
| `--visualizer-styles` | 1.25 | a5a825cc | pass |  |
| `--wallpaper-pair` | 1.25 | a5a825cc | pass |  |
| `--weather-locate` | 1.25 | a5a825cc | pass |  |
| `--workspaces` | 1.25 | a5a825cc | pass |  |

## Hyprland

The Hyprland legs the Shoji work touches, on the mac rig's slot 0 at scale 1.25 with the shell at a6697e69.

| Row | Scale | Result |
|---|---|---|
| `--dock` | 1.25 | pass |
| `--dock-density` | 1.25 | pass |
| `--mac-keys` | 1.25 | pass |
| `--mac-menu` | 1.25 | pass |
| `--mac-menu --sidebar` | 1.25 | pass |
| `--mac-launcher` | 1.25 | pass |
| `--mac-faults` | 1.25 | pass |
| `--glass-behind` | 1.25 | pass |
| `--glass-behind --no-morph` | 1.25 | pass |
