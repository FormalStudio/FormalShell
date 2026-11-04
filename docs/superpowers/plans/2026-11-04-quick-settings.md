# Quick settings

**Date:** 2026-11-04
**Status:** planned, on `main`
**Spec:** `docs/superpowers/specs/2026-11-04-quick-settings.md` (wins on conflict)

Each task is one subagent, run in order, ending in its own commit pushed to
`origin/main` with the verification output read and the PNGs looked at.
Every leg runs at both scales (`just smoke` does). Another agent's Pond
polish pass edits the shared kit and `crates/fs-theme/formalshell.json` at
the same time: add keys and constructors, never rewrite theirs, and pull
before each task.

## Task 1: tokens, roles and the kit pieces

- `crates/fs-theme/src/tokens.rs:185-194` and `:239-248`: `quick_inset` 16,
  `quick_gap` 12, `quick_toggle` 48, `quick_arrow` 36, `quick_button` 36,
  `quick_slider` 40, `quick_background` 40 in `SEMANTIC_SPACING_BASE`, named
  `quickInset` and so on.
- `crates/fs-theme/formalshell.json` `roles`: `quick.toggle` (`rest`, `on`,
  `disabled`, radius `pill`) and `quick.arrow` (`rest`, `on`, separator
  colours), per spec section 2. The fs-theme style test that every role is
  declared passes.
- `crates/formalshell-rs/src/ui/w.rs` beside the panel kit (`:431-623`):
  `quick_toggle`, `quick_split` (toggle part, separator, arrow; two stops),
  `quick_info`, `quick_button` (a 36 circle), `quick_slider` (icon button,
  the kit's `slider`, optional arrow) and `quick_menu` (header chip, title,
  subtitle, rows, settings row, on a `panel.group` over the card fill).
- `crates/formalshell-rs/src/surfaces/panel/quick/grid.rs`: the pure layout,
  rows of spans (1 or 2 columns) to rects at a width, and a menu inserted
  under row n moving the rows after it. Unit tests: 178 wide tiles at 400,
  a span-2 row, a menu of height h moving later rows by h + `quickGap`.
- `surfaces/panel/gallery.rs`: each piece off, on, disabled and split.
- `dev/smoke-map.toml`: `crates/formalshell-rs/src/surfaces/panel/quick/**`
  runs `--gallery` until task 2 adds its leg.
- Verify: `just lint`, `just smoke --gallery`, read the sheet.

## Task 2: the card, its system row and plain toggles

- `crates/fs-chrome/src/bar/panels.rs:17-46`: `quicksettings` in `PANELS`
  (27); `crates/fs-menu/tests/menu_reachability.rs:39-44` passes.
- `crates/formalshell-rs/src/surfaces/panel/mod.rs:82-97`: `Effect::ipc`, a
  list of argv the host runs through `ipc::dispatch`
  (`src/ipc/mod.rs:133`) with the card left open; `:279` builds the module.
- `surfaces/panel/quick/mod.rs`: `Panel` with `header` and `closable` false,
  width `panel_width_wide`; the system row (battery pill handing over to
  `power`, Screenshot, Settings, Lock; Power's menu waits for task 3);
  Wi-Fi, Bluetooth, Power mode, Night light, Dark style, Do not disturb,
  Keyboard, Caffeinate, Overnight, HDR as plain toggles in spec order, each
  through its verb, with the honest disabled states and reasons of spec
  section 3. Keyboard stops per spec section 9.
- `src/ipc/quicksettings.rs`: `status`, `activate`; registered; lines added
  to `crates/formalshell-rs/tests/ipc-golden.jsonl`.
- New leg `dev/smoke.d/quick_settings.sh` `--quick-settings`: `panel open
  quicksettings`; `status` order and kinds; geometry (card 400, tiles 178 by
  48, gaps 12, inset 16 off the card rect); the rig's honest states (no
  NetworkManager, no BlueZ, no power-profiles-daemon on the private system
  bus) as disabled with their reasons; Dark style, Do not disturb and
  Caffeinate flipped by real arrows and Enter, read back off `theme status`,
  `notifications dndState` and `caffeinate status`; settings.json's sha256
  unchanged; frames dark and with `--light`.
- `dev/smoke-map.toml`: the quick paths and `src/ipc/quicksettings.rs` run
  `--quick-settings` and `--quick-settings --light`; both lines in
  `dev/smoke-all.list`; the leg's line in CLAUDE.md.
- Verify: `just lint`, `just smoke --quick-settings`, `just smoke
  --quick-settings --light`.

## Task 3: sliders and the in-place menu

- `crates/fs-audio/src/lib.rs` (`Stream/Input/Audio`, `:69`) and
  `src/services/devices/audio.rs:17-35`: `Audio::capturing`, true while a
  capture stream is live.
- Volume (icon mutes, arrow with two or more sinks), Microphone (shown while
  `capturing`), Brightness (one slider, per-display sliders in its menu,
  `display::set_percent`, `src/services/display.rs:400`); Left and Right
  step 5%.
- The menu machinery in `quick/mod.rs` and `grid.rs`: one menu at a time,
  rows below moving on `spatialFast`, content fading on `effects` after the
  height lands, the 0.4 `scrim` dim on `effectsSlow` around the menu group,
  Escape closing the menu before the card; Power's menu in the system row
  (`services::power::request`, `power logout`).
- `quicksettings menu <id>` and `back`, golden lines.
- `--quick-settings`: two null sinks (as `--device-routes` makes one); the
  Volume arrow opening the sinks menu, a burst at a tenth speed showing the
  rows under it moving down and the menu's content arriving after its
  height, `dim` true; Down and Enter making the second sink the default
  (`pactl get-default-sink`); Escape twice closing menu then card; the moves
  wrapped in `stale_audit_*`; Right twice on Volume read back off `wpctl`.
  The menu open photographed dark and light.
- Verify: `just lint`, `just smoke --quick-settings`, `just smoke
  --quick-settings --light`, `just smoke --osd` (shares the audio service).

## Task 4: the split toggles' menus, on real daemons

- Wi-Fi (eight networks, connect, a secured unknown network handing over to
  the `network` panel's password step), Wired, Bluetooth, Power mode,
  Keyboard menus per spec section 3, each ending in its settings row.
- New leg `dev/smoke.d/quick_settings_devices.sh` `--quick-settings-devices`,
  `leg_..._vm_only`: on the guest's two hostapd radios the Wi-Fi tile on,
  its menu listing both SSIDs, Enter on the known one connecting with the
  SSID as the subtitle, the tile off reaching NetworkManager (`nmcli radio
  wifi` disabled) and on again; the guest's wired NIC as the Wired tile
  with its connection's name; Bluetooth disabled reading "No adapter"
  (BlueZ with no controller); Power mode's menu picking Power saver, read off
  busctl's `ActiveProfile`, and the tile's click flipping back to balanced.
  Frames of the Wi-Fi menu open, dark and light.
- `dev/smoke-map.toml`: the quick paths also run `--quick-settings-devices`;
  `dev/smoke-all.list` gains it among the vm_only lines; CLAUDE.md line.
- Verify: `just smoke --quick-settings-devices`, `just smoke --wifi`
  (shares the network service).

## Task 5: Background

- The indicators rail's items (`src/surfaces/bar/cells/indicators.rs:51-95`,
  less caffeinate, night light and overnight) and Activity's running groups
  as Background's count and menu rows, with Stop (`record stop`), Clear
  (reminders) and Dismiss (a finished group), then the `activity` row.
- `--quick-settings`: `record start` raising "1 background task", its menu's
  Stop ending it (`record status`), the row gone after; a reminder set over
  IPC listed and cleared; photographed.
- Verify: `just smoke --quick-settings`, `just smoke --record`, `just smoke
  --activity`.

## Task 6: extras as info tiles

- `src/surfaces/bar/cell.rs:195`: `Cell::summary() -> Option<Summary>`
  (icon, name, value, tone), implemented in every `cells/*.rs` that can land
  behind a chevron, `None` where the cell is hidden.
- `crates/fs-chrome/src/bar/layout.rs:647-677`: `quick_extras`, the
  governed entries less the absorbed ones (spec section 7), and its sidebar
  form from `sidebar_overflow_entries`; unit tests beside `:1238`.
- The card builds each extra's cell through `Slot::rail`
  (`src/surfaces/bar/slot.rs:62`), holds it visible while open, draws it
  with `quick_info`, and a click hands over to `widget_panel` or runs the
  cell's own action.
- `--quick-settings`: the `--chevron` fixture's layout pinned, its governed
  cells as tiles in `bar.layout` order with their values matching `bar room`
  and the cell's own status, a click on the weather tile leaving `panel
  state` on `weather`.
- Verify: `just lint`, `just smoke --quick-settings`, `just smoke
  --bar-layout`.

## Task 7: the chevron and the sidebar open the card; the second bar goes

- `src/wayland.rs:1618-1622`: `Action::Overflow` opens `quicksettings` at
  the chevron's anchor, recording its region; `src/ipc/bar.rs:36-70` maps
  `expand`, `collapse`, `toggle` and `status` onto it.
- `src/surfaces/sidebar/mod.rs:553, 806, 846`: the chevron button opens the
  card budding off the column; tooltip "Quick settings".
- Delete from `src/wayland.rs`: `Popout`'s slot mode and `Rise` (`:94-237`),
  `chips_grid` and `chips_size` (`:240-255`), `overflow` (`:319`),
  `overflow_open*` (`:927-933`), the overflow half of `sync_join` (`:994`),
  `set_overflow` and `overflow_rise` (`:1070-1165`), the nested placement in
  `panel_place` (`:1167`) and `set_panel_from`'s `nested` (`:1201`),
  `close_overflow` and its catcher (`:1285-1315`); the callers in
  `src/wayland/heads.rs:139, 188, 202, 216, 277`, `rims.rs:130, 162`,
  `toasts.rs:195`; `far_gap` in `src/surfaces/card.rs:146` and `Target` if
  `rg` finds no other user. `src/surfaces/bar/mod.rs:444` keeps feeding the
  collapse.
- Delete `src/surfaces/chips.rs` and `src/surfaces/mod.rs:10`; `COLUMNS`
  and `columns()` move into `src/surfaces/panel/tray.rs:102`.
- Legs: `dev/smoke.d/chevron.sh` (the five governed cells off the strip as
  before, `bar chevron expand` opening this card with five info tiles, a
  tile's handoff in place of the nested bud, the band's ink not reaching the
  card); `chevron_quiet.sh` (the card never narrower than it settles,
  nothing moving after its entrance, cropped off `quicksettings status`);
  `sidebar.sh:201-216` (the chevron button opening the card off the column,
  the owner's layout giving Volume, Brightness and the GitHub, usage,
  weather and iPhone tiles, the weather tile handing over);
  `tray_overflow.sh:73-123, 266-277` (a governed tray as a tile handing over
  to `trayoverflow`); `join.sh` case b (this card budding off the line)
  with case c dropped; `use.sh:42-64, 257` and `bar_position.sh` read this
  card where they read the second bar.
- `dev/smoke-map.toml`: drop the `chips.rs` rule (`:237-238`); the
  `chevron.rs` rule (`:233-234`) and the sidebar rule (`:732-733`) gain
  `--quick-settings`.
- Verify: `just lint`, `just smoke-affected`, then each leg above at both
  scales, PNGs read.

## Task 8: keybind and docs

- `docs/examples/hyprland/formalshell.lua:365-375`:
  `fs_bind("SUPER + S", "panel toggle quicksettings")`.
- `docs/DESIGN.md:351-352, 413, 681-695, 752-753`: the chevron opens quick
  settings; the second bar is the tray's alone. Sidebar spec section 2's
  status card paragraph points here.
- CLAUDE.md: the `--chevron`, `--chevron-quiet`, `--join`, `--sidebar` and
  `--tray-overflow` lines say what they read now; `surfaces/chips.rs` gone
  from them.
- Verify: `rg -n "chips\.rs|chevron's second bar" CLAUDE.md docs/DESIGN.md` returns
  nothing; `just smoke-affected` runs nothing.

## Task 9: GNOME side by side

- A throwaway GNOME VM on g815 (the `host-ops` skill's distro VM), with
  `gnome-shell --version` reading 51; two null sinks for the Sound menu.
- Capture GNOME at scale 1 and 1.25, dark and light (`gsettings set
  org.gnome.desktop.interface color-scheme`): the menu closed, the Sound
  output menu open, the Power off menu open. Ours: the same three off
  `--quick-settings` frames.
- Compose each pair at equal physical size with ImageMagick into
  `artifacts/quick-settings-vs-gnome-{dark,light}.png` and read them:
  card width, tile size, gaps, radii, row order, menu placement and dim.
  A structural difference is fixed in this commit; a material one (fill,
  radius, clock) is listed in spec section 12 as the table's.
- Remove the VM.
- Verify: the two sheets read, `just smoke --quick-settings` green after any
  fix.
