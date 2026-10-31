# The settings UI

Date: 2026-10-31. Owner's call (HANDOFF.md): build it. Pages of settings
the user changes from the shell itself, without touching settings.json,
drawn in the panel language (`2026-10-30-panel-language.md`), which wins
over this spec on anything visual.

## 1. Layering

The shell reads one resolved document, built in this order, each layer
over the one before:

1. Defaults: what each consumer brings to its own `config.get`. Nothing
   new: a key nobody sets reads as it always did.
2. Overrides: the `settings` object in `$XDG_STATE_HOME/formalshell/state.json`,
   written by this UI and the `settings` IPC target alone.
3. settings.json, read and never written (CLAUDE.md's hard rule).

Objects merge key by key; a leaf of a later layer replaces whatever the
earlier one held at that path. The config service does the merge
(`services/config.rs`) and publishes the result as the same
`Diff::Config` a settings.json change publishes, so every consumer that
follows a settings.json edit live follows an override live too, and
`debug dump`'s `configLoaded` is the resolved document. The config service
holds its first publish until the state service has handed it the
overrides it read at start, so a start never draws the defaults first and
then jumps.

## 2. Pinned keys

A key settings.json sets (the path exists in the file, or an ancestor is a
non-object value there) is pinned: its row still shows, with the value
settings.json gives it, a lock glyph and the caption "Set in
settings.json", and refuses every edit. `settings set` and `settings reset`
on it answer `error: <key> is set in settings.json`. An override left
under a key settings.json later pins stays in state.json and is ignored by
the merge; it comes back if the file drops the key.

## 3. Pages

The Settings panel (`panel:settings`, Pond's tile grid) is the entry. Its
tiles are Appearance, Bar, Desktop, System, Bluetooth and Information, two
by three; the first four open `settings:<page>`, the other two their own
panels, as before. Each page is a panel of its own, so `panel open
settings:bar` opens it straight, and opening one from a tile is a panel
handoff.

| page | key | control | values, the default in bold |
| --- | --- | --- | --- |
| appearance | `theme.mode` | segmented | Light, **Dark**, Auto (see §4) |
| appearance | `motion.morph` | switch | **true** |
| appearance | `motion.enabled` | switch | **true** |
| appearance | `theme.reactiveRims` | switch | **true** |
| appearance | `wallpaper.animate` | switch | **false** |
| bar | `layout` | segmented | **`bar`**, `sidebar` |
| bar | `bar.position` | segmented | **`top`**, `bottom`, `left`, `right` |
| bar | `bar.style` | segmented | **`transparent`**, `solid`, `corners` |
| bar | `notifications.position` | segmented | **`bottom-right`**, `top-right`, `bottom-left`, `top-left` |
| bar | `fullscreen.hideChrome` | switch | **true** |
| desktop | `desktopClock.enabled` | switch | **false** |
| desktop | `desktopClock.face` | segmented | **`pixel`**, `gooey`, `serif` |
| desktop | `desktopClock.width` | segmented | **`wide`**, `narrow` |
| desktop | `desktopClock.hour12` | switch | **false** |
| desktop | `desktopClock.depth` | switch | **true** |
| desktop | `hotCorners.enabled` | switch | **true** |
| desktop | `hotCorners.topLeft` | segmented | `none`, **`overview`**, `screensaver`, `lock` |
| desktop | `hotCorners.topRight` | segmented | **`none`**, `overview`, `screensaver`, `lock` |
| desktop | `hotCorners.bottomLeft` | segmented | `none`, `overview`, **`screensaver`**, `lock` |
| desktop | `hotCorners.bottomRight` | segmented | `none`, `overview`, `screensaver`, **`lock`** |
| system | `switcher.enabled` | switch | **true** |
| system | `switcher.currentWorkspace` | switch | **false** |
| system | `workspaces.preview` | switch | **true** |
| system | `menu.appGrid` | switch | **false** |
| system | `media.lyrics` | switch | **true** |
| system | `media.lyricsBlur` | switch | **true** |
| system | `media.animatedBarCover` | switch | **true** |
| system | `screensaver.guardMediaPlayback` | switch | **true** |
| system | `lock.beforeSleep` | switch | **true** |

Left out on purpose: keys read once at start (`render.backend`,
`compositor.backend`, `caffeinate.onStartup`), lists and objects
(`bar.layout`, `bar.modules`, `menu.*` trees), paths and commands, and
numbers with no sensible handful of steps. Those stay settings.json's.

## 4. Theme mode

Light and Dark are the mode the toggle flips (state.json's `mode`, through
the same rules `theme mode` follows) with any `theme.mode` override
cleared; Auto writes the override `theme.mode: "auto"`. A settings.json
`theme.mode` pins the row like any other key.

## 5. A page

A panel with the display title (the page's name), a back button in the
header ahead of the close button (`chevron-left`, to the tile grid), and
the rows in sections. A switch key is one row: its name, the switch
trailing. A choice key is a label row (the name at the left, the current
choice muted at the right) over a segmented control filling the row. A
pinned key's name carries "Set in settings.json" under it and a lock
glyph. A key with an
override carries a reset button (`rotate-ccw`) in its row, which drops the
override. Rows are the panel's stops in reading order: Up and Down walk
them, Enter or Space flips a switch, Left and Right move a choice and
apply it at once (Enter steps a choice round), `x` resets the key under
the cursor, Backspace or the back button returns to the tiles, Escape
closes.

## 6. IPC

- `settings get <key>`: `{"key","value","override","file","pinned","page"}`,
  `value` the resolved one (null when nothing sets it, the consumer's
  default then applies), `override` state.json's, `file` settings.json's.
- `settings set <key> <value>`: `value` is JSON (`true`, `"sidebar"`); a
  bare word that is not JSON reads as a string. Answers `ok`, or an error
  for a key not in §3, a value outside its control's values, or a pinned
  key.
- `settings reset <key>`: drops the override; `ok`, or the pinned error.
- `settings list`: every key of §3 with its page and the `get` fields.
- `panel open settings:<page>`: one of `appearance`, `bar`, `desktop`,
  `system`.

## 7. Proof

`--settings-ui`: a page opened from the tile grid on real keys, a key
changed on real keys and read back in state.json's `settings` and in
`debug dump`'s `configLoaded`, a key pinned by settings.json refusing the
edit (keys and IPC), reset clearing the override, settings.json's sha256
unchanged across the run. Photographed dark and `--light`.
