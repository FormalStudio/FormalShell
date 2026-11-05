# Quick settings

Date: 2026-11-04. Owner's call: the chevron's second bar (the chip grid,
`surfaces/chips.rs` laid out by `wayland::Popout`) becomes a control centre
modelled on GNOME's latest quick settings menu. The source read is
gnome-shell 51.1, the newest tag on gitlab.gnome.org on this date (51.0 and
51.1 the only 51 releases). Every GNOME number below is cited as
`path:line` in that tag; `scss/` is `data/theme/gnome-shell-sass/`.

This spec wins over the panel-language spec and the sidebar spec for this
card and for what the chevron opens. The theme-boundary spec still decides
what a theme owns: GNOME's structure, order, behaviour and copy are taken
as they are; since 2026-11-05 its sizes and material are not, and the card
draws the panel language's grid and boxes (section 2), every fill, ring,
radius and clock from our table (`crates/fs-theme/formalshell.json`), so a
GNOME colour or number never lands in code.

## 1. GNOME 51.1, read off the source

| Thing | GNOME | Source |
| --- | --- | --- |
| Columns | 2 | `js/ui/panel.js:42` (`N_QUICK_SETTINGS_COLUMNS`) |
| Card padding, radius | 18, 36 (`$base_padding * 3`, `$modal_radius * 2.25`) | `scss/widgets/_quick-settings.scss:1-3` |
| Grid gaps | 12 rows and columns | `_quick-settings.scss:10-13` |
| Toggle | 12em wide (176px at 14.667px/em), 48 tall (`$scalable_icon_size * 3`), radius 999 | `_quick-settings.scss:15-20` |
| Toggle inner | 15 left, 12 right, 9 between icon and text; icon 16 | `_quick-settings.scss:26-36, 47` |
| Toggle text | title `%heading` (700), subtitle `%caption` at normal weight, both one line, title ellipsised | `_quick-settings.scss:38-45`, `js/ui/quickSettings.js:63-129` |
| Card width | none set: two 176 toggles, a 12 gap and the 18 padding, about 400 | `quickSettings.js:672-676` |
| Off and on | off is the plain button fill (fg mixed 9% dark, 12% light into bg); on (`:checked`) is the accent fill with accent-fg ink | `_quick-settings.scss:24`, `scss/_common.scss:101-128` |
| Split toggle | `[toggle | 1px separator | menu button]`; the button is `go-next-symbolic`, 6 by 10.5 padding, its own fill; clicking the toggle part toggles, the arrow opens the menu; `menuEnabled` hides the arrow and separator | `quickSettings.js:143-238`, `_quick-settings.scss:51-135` |
| In-place menu | a card in an overlay bound to the grid's width, placed under the toggle's row; the rows after it move down by its height | `quickSettings.js:367-383, 733-735, 784-819` |
| Menu motion | height grows over 200ms, then the content fades in over 200ms (ours run together); closing reverses; the rest of the menu dims to brightness -0.4 over 400ms ease-out-quad | `quickSettings.js:16-17, 463-525, 860-875` |
| Menu box | `%card`, radius 24, padding 12, 12 under its row | `_quick-settings.scss:150-160` |
| Menu header | icon 24 in a round chip 9 padding (`fg` at 0.2, accent when the toggle is on), title `%title_3`, 12 under it; `setHeader` takes a subtitle (`%caption_heading`) but no menu in 51.0 passes one: "Sound Output", "Sound Input", "Wired Connections", "Wi-Fi", "Bluetooth", "Power Mode", "Power Off" | `quickSettings.js:390-454`, `_quick-settings.scss:167-199` |
| Menu rows | `popup-menu-item` 9 by 12 padding, radius 12; a separator, then a settings row last ("All Networks", "Bluetooth Settings", "Power Settings", "Sound Settings") | `scss/widgets/_popovers.scss:36-40`, `js/ui/popupMenu.js:710-728` |
| One menu at a time | opening one closes the other; closing the card closes it | `quickSettings.js:535-538, 852-858` |
| Slider row | `[icon button | slider | menu button]`, 6 apart; the icon mutes on volume; the arrow shows only with more than one device | `quickSettings.js:240-361`, `_quick-settings.scss:138-148`, `js/ui/status/volume.js:39-73, 173` |
| System row | battery toggle (percent and level icon, opens Power settings, hidden with no battery), a spacer shown with the battery, screenshot, settings, a spacer shown without it, lock, power menu; 12 apart, round icon buttons | `js/ui/status/system.js:53-105, 262-298` (51.0: `_laptopSpacer` 275-279, `_desktopSpacer` 287-292), `_quick-settings.scss:205-216` |
| Power menu | Suspend, Restart…, Power Off…, separator, Log Out…, Switch User…, each shown only when allowed | `system.js:177-207` |
| Order | system row, output volume, input volume (only while something records), brightness, then wired, Wi-Fi, mobile, tether, VPN, Bluetooth, Power Mode, Night Light, Dark Style, Do Not Disturb, Keyboard, Airplane Mode, Auto Rotate; extensions' toggles; Background Apps last, full width | `panel.js:379-437` |
| Background Apps | a flat 40 tall full-width toggle, "%d Background Apps", shown while any exist or its menu is open; its menu lists each app with a close button | `js/ui/status/backgroundApps.js:136-242`, `_quick-settings.scss:244-272` |
| Unavailable | network toggles hide with no device; Wi-Fi stays shown and goes insensitive on a hardware kill switch; Bluetooth shows off with "Turn on Bluetooth…" in its menu; Power Mode hides with no daemon | `js/ui/status/network.js:1535, 1816-1823`, `js/ui/status/bluetooth.js`, `js/ui/status/powerProfiles.js:111` |
| Keyboard | St focus groups; the menu key or Shift+F10 opens a toggle's menu; Escape closes the open menu first; `<Super>s` toggles the card | `quickSettings.js:233-236, 386-388`, `js/ui/popupMenu.js:1636-1655`, `data/org.gnome.shell.gschema.xml.in:179-180` |

Recent NEWS for quick settings: 49.alpha.1 moved Do Not Disturb into the
grid, 49.beta added per-monitor brightness sliders, 50.beta fixed Tab focus
in the menu (`NEWS:247, 443, 466-467`).

## 2. Our card

A panel module, `quicksettings` (`surfaces/panel/quick/`), in the panel host
like every other panel: it hangs off the chevron that opened it (or the
sidebar's chevron button), takes the keyboard, scrolls past the output's
room, and hands over to another panel through the host's own handoff. It
has no header and no close button: Escape and a click outside close it, as
the Settings tiles do.

Top to bottom, every row `panelGap` apart:

1. The system row (section 5).
2. Sliders, each the full width: output volume, input volume while a
   capture stream is live, brightness.
3. The toggle grid, two columns, GNOME's order (section 3), then the
   extras (section 7).
4. Background, full width, while anything runs in the background
   (section 6).

### Geometry

2026-11-05 (owner: "its style must be updated to match the rest of the
shell"): GNOME's function and order stay, its numbers and material go.
The card is a headerless panel on the panel language's grid
(`docs/superpowers/specs/2026-10-30-panel-language.md` §2), with no
tokens of its own:

| Piece | px | Token |
| --- | --- | --- |
| card width | 400 | `panelWidthWide` |
| body inset | 8, all four sides, the host's own frame | `panelRowInset` |
| rows and grid columns | 8 apart | `panelGap` |
| toggle | 44 tall; (400 - 16 - 8) / 2 = 188 wide | `panelRow` |
| split toggle | the toggle part and a 44 square arrow tile, 4 apart | `panelRow`, `rowGap` |
| system button | 44 square | `panelRow` |
| slider row | 40 tall | `panelRowCompact` |
| Background | 44 tall, the full width | `panelRow` |

### Material

Every control is a Settings tile: an interactive `panel.group` cell (its
outline steps up to `panel.group.hover` and the kit's wash eases in under
the pointer), the group's `open` fill while on. A toggle holds a
`panel.chip` with its glyph (lit `primary` while on) `sm` in from its
edge, then the title in `body` 500 over the subtitle in `caption`
`mutedForeground`, each one line, eliding at the end, as `panel_row` lays
out a row; the title sits centred when there is no subtitle. A toggle
with no adapter or daemon behind it is the kit's 0.5 fade and takes no
stop. The chip column holds down the card: the Background row's chip and
the sliders' buttons sit on it too, and the sliders' arrows on the split
toggles' arrow tiles.

The dim behind an open menu takes 0.4 of the brightness off every item
but the menu group (GNOME's brightness -0.4): a filled box gets the `scrim`
role at 0.4 over its own shape, ink, marks and a slider's fill darken to
0.6, and the card's own fill between them stays as it is (`El::shade`).
Focus is the table's one `ring`, drawn by the keyboard only. The card is
the panel host's card, so the shared-kit polish pass (border opacity,
shadows, menu radius, eased hover) reaches it with no change here.

### Motion

The in-place menu's height and the rows under it travel on `spatialFast`;
the menu's content fades in on `effectsFast` from the same instant, and out
on it while the height shrinks, so a row shows and takes a key as the menu
opens; the dim rises and falls on `effects`. A tile turning on eases its
fill on `Cx::color`, as every group does. The card itself arrives as
every panel does (join or popover, `motion.morph`). GNOME's clock lengths
differ from ours by design: the clocks are the theme's.

## 3. Toggles and sliders

Every control drives a service the shell already has, through the shell's
own IPC verbs run in process (`Effect::ipc`, dispatched by
`ipc::dispatch`), so the card and the legs act on one path. "Shown"
follows GNOME except where CLAUDE.md's honest-state rule asks for a dim
toggle: a missing adapter or daemon reads as a disabled toggle saying so,
never a hidden one and never a made-up value.

| GNOME | Ours | Icon (Lucide) | Service and call | Subtitle | Unavailable | Menu |
| --- | --- | --- | --- | --- | --- | --- |
| Output volume slider | Volume | `volume-2`, `volume-x` muted | `devices::audio` (`set_volume`, `toggle_mute`, the default sink `Command`) | none | hidden with no default sink | sinks with a check on the default, then "Sound settings" (the `audio` panel); arrow only with two or more sinks |
| Input volume slider | Microphone | `mic`, `mic-off` | `devices::audio` source volume and `toggle_source_mute` | none | shown only while a capture stream is live (`Audio::capturing`, new) | sources, "Sound settings" |
| Brightness slider | Brightness | `sun` | `services::brightness`, `display::set_percent` | none | hidden with no backlight and no DDC display | one slider per display (GNOME 49), "Display settings" (the `display` panel); arrow only with two or more |
| Wired | Wired | `ethernet-port` | `network::wired_toggle` | the connection's name | shown only with an Ethernet device (GNOME hides it too) | wired connections, "Network settings" (`network`) |
| Wi-Fi | Wi-Fi | `wifi`, `wifi-off` | `network wifi <bool>` | the SSID | dim, "No Wi-Fi adapter"; dim with NetworkManager absent, "No NetworkManager" | up to eight networks, a check on the connected one, a lock on secured ones, Enter connects (a secured unknown one hands over to the `network` panel's password step), then "All networks" (`network`) |
| Bluetooth | Bluetooth | `bluetooth`, `bluetooth-off` | `bluetooth toggle` | "n connected" or the one device's name | dim, "No adapter" | paired devices with Connect or Disconnect under each name; off: "Turn on Bluetooth to connect to devices"; then "Bluetooth settings" (`bluetooth`) |
| Power Mode | Power mode | `gauge` | `devices::power::set_profile` | the profile's name | dim, "power-profiles-daemon is not running" | Performance, Balanced, Power saver with a check, then "Power settings" (`power`); on while not Balanced, the click flipping Balanced and the last other profile, as GNOME does (`powerProfiles.js:127`) |
| Night Light | Night light | `lightbulb` | `nightlight toggle` | none | dim, wlsunset's reason, when it cannot gamma-control | none |
| Dark Style | Dark style | `moon` | `theme mode toggle` (state.json, never settings.json) | none | never | none |
| Do Not Disturb | Do not disturb | `bell-off` | `notifications toggleDnd` | none | never | none |
| Keyboard | Keyboard | `keyboard` | `lights toggle` (asusd's Aura) | the effect's name | shown only with asusctl (the backlight GNOME means is the same hardware) | the effects with a check, then "Keyboard lights" (the launcher's `lights.effect` route) |
| extensions' toggles | Caffeinate | `coffee` | `caffeinate toggle` | none | never | none |
| | Overnight | `moon-star` | `overnight toggle` | none | never | none |
| | HDR | `sun-medium` | `display toggle` | the output | shown only while an output supports it (`hdr_supported`) | none |

Not carried over, because the shell has no service for them and inventing
one is out of this spec: Mobile broadband, Bluetooth tether, VPN, Airplane
Mode, Auto Rotate, Switch User. Camera, microphone-in-use and location are
privacy indicators in GNOME's top bar, not toggles; they stay out.

## 4. The in-place menu

- A split toggle's arrow (or Right from its toggle part, then Enter, or the
  menu key) opens its menu. One menu at a time; opening another swaps them.
- The menu is a `panel.group` box spanning the grid's width, painted on the
  card fill so the dim never reaches it, `panelGap` under its toggle's row.
  The rows after that row move down by the menu's height. One inset level
  holds: card, group, rows.
- Its header: the toggle's glyph in a `panel.chip` (lit while the toggle
  is on) on the rows' chip column, the title in `body` at 600 and no
  subtitle (GNOME's menus pass none; the toggle already carries it),
  `panelRow` tall and `panelGap` over the rows. The wired menu is "Wired
  connections", GNOME's title.
- Its rows are the kit's `panel_row`s (44, `lg` radius, the current one with
  its check, never a fill), a hairline, then the settings row last, which
  hands the card over to the full panel.
- Escape, Left from the menu's first column, or the arrow again closes it;
  a second Escape closes the card. Closing the card closes the menu.
- A menu taller than the room left scrolls with the card's body.

## 5. The system row

Left to right, `panelGap` apart: the battery tile, a spacer, Screenshot
(`camera`, `screenshot region`, the card closing first), Settings
(`settings`, the launcher's `settings` route), Lock (`lock`, `lock lock`)
and Power (`power`, an in-place menu). With no battery the spacer moves
between Settings and Lock, so Screenshot and Settings hold the left edge
and Lock and Power the right, as GNOME's `_desktopSpacer` does. Each
button is a `panelRow` square tile holding its glyph, with its name as the
tooltip.

- Battery: a tile as wide as its content, a `panel.chip` holding the
  battery level glyph (`battery`, `battery-low`, `battery-medium`, `battery-full`,
  `battery-charging`) and the percent in `mono`; Enter or a click hands
  over to the `power` panel. Absent with no battery (UPower's `IsPresent`
  false, `devices.power.battery` none), which is the bar's own honest
  absent cell.
- Power's menu: Suspend, Restart…, Power off… (`services::power::request`
  through logind with the shell's polkit agent), a hairline, Log out…
  (`power logout`). Each row shows only when logind answers that it can.

## 6. Background

GNOME's Background Apps, holding what the shell's indicators rail carries:
Activity's running groups, a recording, the clipboard image going to a
host, AirPlay and pending reminders. Caffeinate, night light and
overnight are toggles in the grid instead.

- A full-width `panel_row`, `panelRow` tall with no fill at rest, its
  `activity` chip on the tiles' chip column, the count as its name ("1
  background task", "n background tasks") and a `chevron-right` mark,
  shown while the count is above 0 or its menu is open.
- Its menu, headed "Background", lists each one as a `panel_row` (its own
  icon, an Activity group's ring, its title, its state under it) with a
  trailing ghost icon button where the thing can be stopped: Stop for a
  recording (`record stop`), Clear for reminders (`reminder clear`, one row
  for all of them), Dismiss for a finished Activity group (`activity
  dismiss`). The row is one stop: Enter or a click on it does what its
  button does, and with no button hands the card over to where the thing
  lives (Activity for a running group, `media` for AirPlay). Then
  "Activity" (the `activity` panel).
- Open with nothing left, the row reads "No background tasks" and the menu
  "Nothing is running in the background"; the row goes once the menu
  closes, as GNOME's does.

## 7. Extras: what the chevron governs

`bar.layout` keeps deciding what leaves the strip: the cells a chevron
governs still collapse off it. What they become changes:

- A governed cell the card already draws is absorbed: `audio` (Volume),
  `network` (Wi-Fi and Wired), `bluetooth`, `battery` (the system row),
  `display` (Brightness), `indicators` (the toggles and Background),
  `microphone` (Microphone).
- Every other governed entry (`weather`, `github`, `usage`, `tailscale`,
  `iphone`, `dualsense`, `earbuds`, `monitor`, `systemUpdate`, `bell`,
  `tray`, `keyboardLayout`, `custom:` and `plugin:` entries) becomes an
  info tile: a toggle's tile never `on`, its icon, its name as
  the title and the cell's own value as the subtitle ("12 / 15" for GitHub's
  pull requests and issues, "9%" for the usage gauge, the temperature,
  "Connected" for the iPhone). A click hands over to the cell's panel
  (`widget_panel`), or runs the cell's own click where it has no panel. A
  cell whose honest state is hidden (`usage` before a provider answers)
  has no tile. Order is `bar.layout`'s, after the fixed toggles and before
  Background, where GNOME puts extensions' toggles.
- Each tile is backed by the cell itself: the card builds the entry's cell
  as the second bar did (`Slot::rail`), holds it visible while open so its
  wants run, and reads a new `Cell::summary()` (icon, name, value, tone).
- Under the sidebar the extras are `bar.layout.right`'s entries the column
  draws no card for (`sidebar_overflow_entries` today, less the absorbed
  ones), governed or not, since the column has no strip.
- With no chevron (`panel toggle quicksettings`, `Super+S`), the extras are
  the right region's governed entries, or none.
- No settings key is added. The card writes nothing to settings.json; the
  dark style, DND, caffeinate and overnight toggles go through the verbs
  that already write state.json.

## 8. Where it opens

- Top or bottom bar: the chevron cell opens it, hanging off the strip under
  the chevron like any panel, the chevron marked open. On a left or right
  bar it buds off the inner edge at the chevron the same way.
- Sidebar: the status card's chevron button (tooltip "Quick settings")
  opens it budding off the column's inner edge beside the button, as the
  network button's panel does, and no longer rises off the status card at
  the column's width.
- `panel open|toggle quicksettings`, and `Super+S` in the shipped
  `docs/examples/hyprland/formalshell.lua` (GNOME's default).
- A click on an info tile, a settings row or the battery pill hands the card
  over to that panel; the panel does not hang off the card (the nested
  second-bar placement goes away).

## 9. Keyboard

The panel host's cursor over stops. The grid is spatial: Up and Down move
by row, Left and Right by column, and a split toggle is two stops (`<id>`
and `<id>:menu`) side by side. Enter or Space activates; on a slider stop
Left and Right step 5% (`Panel::step`), and Enter on its icon mutes. Tab
walks sections (system row, sliders, grid, Background). Escape closes the
open menu, then the card. The cursor starts on the first toggle.

## 10. IPC

A new target, `quicksettings`, its lines added to `ipc-golden.jsonl`:

- `status`: open, what opened it (`chevron:<region>`, `sidebar`, `ipc`),
  the card's rect, and for every row its id, kind (`system`, `slider`,
  `toggle`, `split`, `info`, `background`), title, subtitle, `on`,
  `available`, its reason when not, its rect and its arrow's rect; the open
  menu's id, rect and rows; and `dim`.
- `activate <id>`, `menu <id>` (open that menu), `back` (close the menu),
  each an error string when the card is shut or the id is unknown.

`bar chevron toggle|expand|collapse|status` keeps its contract: expand
opens this card from that chevron, `status` keeps `collapses` and `open`.

## 11. What goes, what stays

- Goes: the chevron's second bar. In `wayland.rs`, `Popout`'s slot mode
  (`slots`, `hover`, `cells`, `fit`, `plates`, `rise`), `Rise`,
  `chips_grid`, `chips_size`, `App::overflow`, `set_overflow`,
  `overflow_rise`, `close_overflow` and the nested panel placement
  (`Place::target`, the card's `far_gap`). `Popout` keeps the tray item's
  menu alone.
- Goes: `surfaces/chips.rs`. Its one remaining caller, the tray's panel,
  keeps `COLUMNS` and `columns()` in `surfaces/panel/tray.rs`.
- Stays: `trayoverflow`, the tray's own second bar, opened by the tray
  cell's dots and the sidebar's tray button, unchanged. Tray items are
  apps, not settings; GNOME carries none in this menu. A governed `tray`
  cell is an info tile that hands over to it.

## 12. Proof

- `--quick-settings`: the card on the rig, read off `quicksettings status`
  and frames, dark and `--light`, at both scales; geometry against section 2.
- `--quick-settings-devices` (vm_only): Wi-Fi, Bluetooth with no
  controller and Power mode against the guest's daemons.
- The legs that read the second bar (`--chevron`, `--chevron-quiet`,
  `--sidebar`, `--tray-overflow`, `--join`, `--use`, `--bar-position`)
  read this card instead.
- A side-by-side sheet against GNOME 51 in a VM, dark and light, closed and
  with a menu open, `artifacts/quick-settings-vs-gnome-{dark,light}.png`,
  with every difference either fixed or listed here as a material the
  table owns.

## 13. Against GNOME 51.0

Read 2026-11-04 in a throwaway Fedora 45 Workstation live VM on g815
(gnome-shell 51.0, gnome-control-center 51.0), two null sinks, at scale 1
and 1.25, dark and light, beside `--quick-settings` and `--light` at the
same scales: `artifacts/quick-settings-vs-gnome-{dark,light}.png`, each
row one scale so a logical pixel is one size across it.

The same: the card's width (400), two columns of toggles, the order
(system row, sliders, the toggle grid), a split toggle's arrow beside its
toggle, the menu opening in place under its own row with the rows after it
moved down, the dim over the rest, the menu's header and its settings row
last, the Power menu under the system row.

Fixed here: the system row's spacer (Screenshot and Settings at the left
with no battery, not every button at the right), and the menu header's
subtitle, which no GNOME menu carries.

Ours by design since 2026-11-05 (section 2), the panel language and not a
defect:

- Geometry: GNOME's 18 padding, 12 gaps and 48 tall pills; ours the panel
  grid, `panelRowInset` round the body, `panelGap` between rows and
  columns, `panelRow` tall tiles.
- Material: GNOME's filled pills, accent blue when on, its round system
  buttons and its separator before a split's arrow; ours `panel.group`
  tiles with a `panel.chip` lit `primary` when on, the arrow its own tile.
- Radii: GNOME's 36 card and 24 menu box; ours the panel host's card and
  `panel.group`.
- Type: GNOME's 700 toggle titles and its `%title_3` menu header; ours
  `body` 500 over a muted `caption`, the menu title `body` 600, sentence
  case.
- Menu rows: GNOME's 9 by 12 rows with a trailing tick; ours the kit's 44
  `panel_row` with its device chip and check mark.
- The slider: GNOME's round knob over an accent track; ours the kit's
  slider.
- Clocks: the menu grows on our `spatialFast`, its fade on `effectsFast`,
  its dim on `effects`.
- Light: under Settings' Default style (`color-scheme` `default`) GNOME
  51.0 keeps its shell dark, so its light captures show a dark card; ours
  follows `theme.mode` like every surface and goes light.

What differs on the rig alone, not in the card: no Wired toggle (no
Ethernet device), Wi-Fi, Bluetooth and Power mode disabled with their
reasons where GNOME hides a missing device, a Brightness slider for the
display the rig reports, Caffeinate, Overnight and the extras after the GNOME
set, and a Power menu holding Log out alone where logind answers nothing.
