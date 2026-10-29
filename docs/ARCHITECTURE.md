# Architecture

FormalShell is one Rust process per Wayland session, drawing every surface
itself on Hyprland. The workspace lives under `crates/`: one binary crate,
`formalshell-rs`, and a set of `fs-*` library crates holding the pure logic
and the D-Bus clients. The design rationale and the budgets the runtime is
held to are in `docs/superpowers/specs/2026-10-06-rust-rewrite.md`; the
visual language is `docs/DESIGN.md`.

## Process model

The package installs three entry points, all from the one crate:

- `formalshell` (the tarball's launcher for `formalshell-rs`): the shell.
  `formalshell install` runs it as the `formalshell.service` systemd user
  unit under `graphical-session.target`.
- `formalshell-ipc`: the IPC client (`src/bin/formalshell-ipc.rs`). Hyprland
  binds and the smoke rig drive the shell through it.
- `formalshell greeter`: the greetd greeter (`src/greeter/`), dispatched in
  `main.rs` before anything else starts. `formalshell install|update|uninstall`
  are dispatched the same way (`src/install.rs`).

`main.rs` brings the shell up in this order, logging a `phase t=` line per
step:

1. `instance::acquire()`: the single-instance lock, a Unix socket at
   `$XDG_RUNTIME_DIR/formalshell/instance-<WAYLAND_DISPLAY>.sock`. A running
   shell is asked to `takeover` and the newest one wins. This runs before
   the Wayland connection so the old shell's surfaces and IPC socket are gone
   first. A shell replaced while locked leaves the compositor holding the
   lock, which is ext-session-lock's contract.
2. The Wayland connection and the `calloop` event loop, with `wayland::App`
   as its state.
3. `runtime::Runtime::start`, which starts the service thread and runs
   `services::start` and `ipc::start` on it.
4. `ipc::listen`, which binds the IPC socket as a source on the UI loop.

### Threads

The thread set is small and fixed (`src/runtime/mod.rs`):

| Thread | Owns |
|---|---|
| UI (main) | Wayland, the scene, input, rendering, the `Store`, IPC answers. It never awaits; it applies diffs and draws. |
| `fs-service` | One `async_executor::LocalExecutor` (`runtime/service.rs`) running every service: zbus clients and servers, the Hyprland sockets, child processes, timers, and derived state such as the launcher index. |
| `fs-pool` | Blocking work (desktop entry scans, icon and image decode, file IO), at most `POOL_THREADS` (2) threads that exit after `POOL_IDLE` (5 s) with nothing queued (`runtime/pool.rs`). |
| `fs-pipewire` | PipeWire's own main loop (`fs-audio`'s `backend.rs`), posting graph events to the service thread. |
| `fs-pam` | One `pam_authenticate` conversation at a time (`fs-auth`'s `pam.rs`). |

Every thread but the UI's runs at nice 10 (`runtime::lower_priority`), so on
a starved CPU the UI loop's frame goes first. A few short-lived helpers have
their own names (`fs-instance`, `fs-fonts`, `fs-warm`, `fs-lock-auth`, the
mirror's capture thread).

### Data flow

```
service task (fs-service) ──Diff──▶ Publisher ──calloop channel──▶ App::receive
                                                                     │
pool job (fs-pool) ─────────Diff──▶ Publisher ─────────────────────▶ │
                                                                     ▼
                                                    Store::apply(diff) -> Option<Topic>
                                                                     │
                                                    surfaces::changed(app, topic)
                                                                     │
                                                    scene nodes dirtied, App::present
```

- `src/store.rs`: `Store` holds one `State` slice per service, and
  `Store::apply` returns the `Topic` that changed, or `None` when the diff
  left the slice as it was (so nothing redraws). The UI thread is the only
  writer.
- `src/services/mod.rs`: one module per service. Each owns its `State`, a
  `Diff` that applies to it, and an async task that publishes those diffs
  through `runtime::Ctx`. Services never see the store; what they need from
  settings.json is handed to them.
- `src/surfaces/mod.rs`: `changed` routes a topic to the surfaces that read
  that slice.
- `services::wants`: polls that run only while a bar cell holds a `Want` for
  them. The first holder starts the source's task, the last release stops
  it, so a bar naming none of these cells costs nothing.

## Rendering

- `src/scene.rs`: a retained scene. Nodes are in paint order, each with the
  device-pixel bounds it covers, and every change marks the old and new
  bounds dirty.
- `src/render.rs`: `Renderer` rasterises only the dirty rects with
  `vello_cpu` into one persistent premultiplied RGBA canvas.
- `src/surface.rs`: one layer or lock surface and its two `wl_shm` buffers.
  A commit copies only the dirtied rects into whichever buffer the compositor
  released, plus what that buffer missed. A frame callback is requested only
  for a commit made while something on the surface animates, so a shell at
  rest runs no clock. `PixelSurface` is the flat full-output case (scrims,
  the screensaver's fade): one `wp_single_pixel_buffer_v1` pixel stretched by
  `wp_viewporter` and faded by `wp_alpha_modifier_v1`, so a fade step is one
  multiplier and one commit.
- `src/text.rs`: shaping through `parley`, outlines through `skrifa` hinted
  at the drawn pixel size, glyph origins and baselines snapped to whole
  device pixels. `src/fontconfig.rs` reads antialias and hint style off
  fontconfig's match for `sans-serif`, so a user's fonts.conf applies.
  Fonts are the fontconfig `sans-serif` and `monospace` aliases; the Lucide
  icon font and the extra font dirs come in through `FS_RS_ICON_FONT` and
  `FS_RS_FONT_DIRS`, set by the package wrapper.
- `src/motion.rs`: the two clock families (`Kind::Spatial*` and
  `Kind::Effects*`), their curves, retargeting, and the velocity deform with
  its spring. Durations come from the live theme table and are 0 with
  `motion.enabled` false.

### The widget layer

`src/ui/` is the one way a surface puts content on a card (its `mod.rs`
header is the guide for building a new surface):

- A surface describes its content as an `El` tree (`ui/el.rs`) each time it
  redraws, built with the constructors in `ui/w.rs`, one per shadcn part
  `docs/DESIGN.md` lists.
- `Ui` keeps what persists between descriptions: each element's scene nodes
  and running tweens, keyed by its path. An unchanged frame damages nothing.
- `ui/draw.rs` measures and paints, `ui/boxes.rs` paints one theme role's
  box (casts, rings, fill and border, face, hairlines, the hover wash).
- Input comes back as data: a draw records each interactive element's rect
  (`Hit`) and each keyboard stop (`Stop`); the host maps pointer and keys
  onto them.
- Specialised elements: `ui/strip.rs` (the Spaces miniature),
  `ui/spectrum.rs` (visualizer styles), `ui/lyrics.rs`, `ui/draw/flow.rs`
  (the power flow diagram).

## Wayland side

`src/wayland.rs` holds `App`: the bar's layer surface, the frame's four
exclusion zones, the cards hanging off the bar (a panel, the chevron's
second bar, a tray menu), the tooltip, the scrim, the pointer and keyboard,
and which owner a configure, frame callback or input event belongs to.
`App::present` runs after every dispatch: due notification timers, panels
and menus whose exit has finished, then a commit for each surface with
damage. `main.rs` logs any dispatch or present that holds the loop past
8 ms.

The other windows are one module each under `src/wayland/`:

| Module | Window |
|---|---|
| `launcher.rs` | the launcher's modal card; `new_modal` builds any modal card's surfaces |
| `lock.rs` | ext-session-lock-v1, one lock surface per output |
| `polkit.rs` | the polkit dialog |
| `osd.rs`, `headset.rs` | the OSD pill and the headset connect card (`surfaces::popup`) |
| `toasts.rs` | the toast stack, and the notification and reminder clock |
| `screensaver.rs` | the screensaver overlay and the controller deciding when it shows |
| `caffeinate.rs` | the idle inhibitor surface and the ext-idle-notify listener |
| `hotcorners.rs` | the hot corner squares |
| `picker.rs` | the capture region picker, one overlay per output |
| `switcher.rs`, `preview.rs`, `capture.rs` | the Alt+Tab switcher, the Spaces preview, and their window thumbnails over ext-image-copy-capture-v1 |
| `console.rs` | the quake console's park and recall |
| `atlas.rs` | the radio atlas window |

Window capture exists only in the Spaces preview and the switcher, and only
while their card is open.

## IPC

`src/ipc/mod.rs`. The socket is `$XDG_RUNTIME_DIR/formalshell/ipc-$WAYLAND_DISPLAY.sock` (a client with no `WAYLAND_DISPLAY` takes the newest one),
served as a source on the UI loop, so `menu toggle` is answered where the
store and the surfaces live and never waits behind service work. Reads and
writes are non-blocking; a reply the socket will not take whole is finished
on the service thread.

- `ipc/wire.rs`: the request, one JSON `Request` (`Call`, `Show`, `Signal`,
  `Prop`) written before the client shuts its half down. The reply is the
  text the client prints, verbatim.
- `ipc/cli.rs`: `formalshell-ipc <call|show|wait|listen|prop> ...` argument
  parsing, including list arguments (`[a,b]`) and subcommand switching.
- `ipc/registry.rs`: `Target`s of typed `Function`s and how a request is
  answered. Errors (unknown target, wrong arity) print on stdout and the
  client still exits 0.
- One module per target, each returning a `registry::Target`, listed in
  `ipc::registry()`. The 40 targets: `airplay`, `bar`, `bluetooth`,
  `caffeinate`, `calendar`, `capture`, `clipboard`, `console`, `debug`,
  `display`, `earbuds`, `gallery`, `hdr`, `iphone`, `lights`, `localsend`,
  `lock`, `media`, `menu`, `mirror`, `monitor`, `network`, `nightlight`,
  `notifications`, `osd`, `overnight`, `panel`, `picker`, `plugins`,
  `radio`, `record`, `reminder`, `screensaver`, `screenshot`, `switcher`,
  `theme`, `tray`, `visualizer`, `wallpaper`, `workspaces`.
- `crates/formalshell-rs/tests/ipc-golden.jsonl` is the fixed wire
  contract: `ipc/golden.rs` replays every recorded argv through the client's
  parsing and the registry and checks stdout, stderr and the exit code byte
  for byte.

`debug` is the verification hook: `dump` (the whole resolved state as JSON,
which the smoke legs read), `query` (ranks a string against the live
launcher tree), `motionScale` (stretches every duration so a leg can catch a
frame mid-flight), and `join`/`joinClear`.

An unknown panel name, or any other bad argument, answers an error string,
never a silent no-op.

## Files

| Path | Who writes it |
|---|---|
| `~/.config/formalshell/settings.json` | the user (or home-manager). The shell only reads it (`services::config`). |
| `~/.config/formalshell/menu.jsonc` | the user; merged over the default launcher tree (`services::menu`) |
| `~/.config/formalshell/plugins/<id>/` | the user; command plugins |
| `~/.config/formalshell/matugen.d/`, `~/.config/matugen/config.toml` | the user; merged into the matugen config |
| `$XDG_STATE_HOME/formalshell/state.json` | the shell (`services::state`), the runtime-mutable state |
| `$XDG_STATE_HOME/formalshell/theme.json` | the theme engine (`services::theme`) |
| `$XDG_STATE_HOME/formalshell/clipboard.json` | the clipboard ledger (`services::clipboard`) |
| `$XDG_STATE_HOME/formalshell/menu-selection.txt`, `picker-selection.txt` | answers to `menu select`/`input` and `picker select` (below) |
| `~/.config/hypr/formalshell-colors.lua`, `formalshell-chrome.lua` | the theme engine, for Hyprland to read |

`services::config` keeps the parsed document and publishes the keys that
changed; every consumer asks for a dotted path and brings its own default.
`services::state` owns the working copy of state.json on the service
thread, applies setters in arrival order, and writes the file once per
burst.

Home-manager retargets `~/.config/formalshell/*` into a new store path on
every activation, which a plain inotify watch on the file never sees.
`services::watch` watches the directory of every hop from the path to the
file it lands on, so a retargeted symlink still reaches a running shell.

## Compositor backend

Hyprland is the only compositor, and Lua (`hyprland.lua`) the only config
format. `src/services/hyprland/` talks to its two raw sockets under
`$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/`:

- `.socket2.sock` streams `EVENT>>DATA` lines. An event only marks the model
  dirty, and one refresh re-reads it, so a burst of events costs one round
  of `j/` requests on `.socket.sock`.
- `hyprland/model.rs` maps those answers onto plain `Workspace`, `Window`
  and `Snapshot` types. It is pure and tested without a compositor.
- Writes go out as `hl.*` Lua through `hyprland::send(Command)`, with
  helpers for the contract's verbs: `focus_workspace`, `focus_window`,
  `close_window`, `spawn`, `power_off_monitors`, `power_on_monitors`.
  `hyprland/lua.rs` words each call; a monitor rule goes as `hyprctl eval`.
- Window and workspace ids are opaque strings everywhere. A window id is
  Hyprland's hex address kept verbatim, and `lua.rs` is the one place that
  builds the `address:0x...` selector from it.
- `State.outputs` is the exception to event-driven refresh: monitor events
  never mention disabled monitors, so outputs are re-read in full
  (`Command::RefreshOutputs`) and `outputs_state` records whether that read
  answered.

Everything that opens "on the focused screen" (panels, the launcher, the
OSD, the notification centre, the polkit dialog, the capture picker)
resolves through `Snapshot::focused_output_name`. With no Hyprland the
backend reports `available: false` and every surface renders its honest
unavailable state.

## Surfaces

`src/surfaces/` holds what each surface draws; the window plumbing is in
`src/wayland/`.

### Bar

`surfaces/bar/mod.rs`. `bar.layout` resolves into three regions through
`fs-chrome`'s resolver (`fs_chrome::bar::layout`). The room rule shares the
strip between them: free labels give ground first, then whole cells hide
from an end region's inner edge. The bar sits on whichever edge
`bar.position` names and is painted as the theme's `bar` habit, the
metamorphosis strip or the wingpanel band, whose paint is read off the
wallpaper (`services::barpaint`). With the screen frame on, the bar's window
is the whole output and paints the ring under its cells.

- `bar/cell.rs`: the `Cell` trait, the kit every cell is built from.
- `bar/slot.rs`: one placed cell (its measure, presence, fade, marquee
  clock). The strip and the chevron's second bar both hold slots.
- `bar/cells/`: one module per cell, and `cells::build`, the one place a
  layout entry becomes cells. A builtin maps to its module, a `bar.modules`
  entry of type `command` to `cells::command`, a plugin to `cells::plugin`.
  `cells::indicators` is a rail of independent cells sharing one entry.

### Cards and panels

- `surfaces/card.rs`: a card hanging off a line on any of the four edges.
  Under the `join` emerge habit it buds out of the line with shoulders cut
  at the join (`surfaces/shoulders.rs`), walls when a side runs too close to
  the line's end, and deforms on its springs; under `popover` it is a plain
  card fading out of its cell. It is worked out once for a top edge and
  mapped to the others by `Card::edge_map`.
- `surfaces/panel/mod.rs`: the `Panel` trait (id, title, icon, the topics it
  reads, a body built from `ui` widgets, header actions, what its stops do)
  and `PANELS`, the 19 names `panel open|toggle` accepts. `panel::build`
  registers a module against its name.
- `surfaces/panel/host.rs`: `Host` owns everything a panel does not: the
  full-output layer surface that turns a click outside the card into a
  close, the card and its motion, the header, scrolling, the size morph, the
  handoff when one panel opens over another, and the keyboard cursor.
  Every element marked `.stop(key)` is one stop in reading order; arrows
  walk them, Enter or Space activates, Escape closes.
- `surfaces/modal.rs`: a modal card budding off the top line on its own
  surface, holding the keyboard, over two single-pixel scrim surfaces. The
  launcher, the polkit dialog and the atlas use it.
- `surfaces/popup.rs`: a small card that buds off a line and is gone with
  its surface once it lets go (the OSD and the headset card).
- `surfaces/tooltip.rs`, `surfaces/tray_menu.rs`: the tooltip card and a
  tray item's dbusmenu.

### The rest

| Module | Surface |
|---|---|
| `surfaces/launcher.rs` | the launcher: header, body (app grid and rows), footer, every route |
| `surfaces/toasts.rs` | the toast stack, one overlay surface for as long as anything is up |
| `surfaces/panel/center.rs` | the notification centre |
| `surfaces/osd.rs`, `surfaces/headset.rs` | popup content |
| `surfaces/lock.rs` | one output's lock screen |
| `surfaces/switcher/` | the Alt+Tab card and its pure model |
| `surfaces/panel/workspace_preview.rs` | the Spaces preview card |
| `surfaces/capture/` | the screenshot, capture and record family and its region picker |
| `surfaces/atlas/` | the radio atlas and its globe |
| `surfaces/panel/gallery.rs` | the dev sheet of every widget against the live theme (`gallery` target) |
| `surfaces/battery.rs` | the low-battery watcher |

## Services

Each module under `src/services/` names its source in its header. Grouped:

- Session and files: `config`, `state`, `clock`, `theme`, `wallpaper`,
  `barpaint`, `plugins`, `commands`.
- Compositor: `hyprland`, `agents` (T3 Code agent badges on Spaces windows),
  `appicon` (which desktop entry and icon a window has), `icons`.
- Devices (`services/devices/`): `audio` over `fs-audio`, `bluetooth` over
  `fs-bluez`, `network` over `fs-network`, `power` over `fs-upower`,
  `earbuds`, `headsets`. Also `display`, `brightness`, `nightlight`
  (wlsunset), `lights` (asusd), `overnight`, `caffeinate`.
- Readings (`services/info/`): calendar, weather and geoclue, GitHub,
  Tailscale, AI usage, flake updates, the system monitor, the process
  table, the power flow, the iPhone bridge. Each runs only while a cell or
  panel wants it, and a failure is a state the cell words honestly.
- Media: `media` (one active pick across MPRIS via `fs-mpris`, the radio's
  mpv, the phone's AMS and AirPlay), `cover`, `motion_art`, `lyrics`,
  `visualizer` (one shared `cava`), `radio`, `airplay` (UxPlay), `ams`.
- Notifications: `notifications`, which runs `fs-notifd` on the service
  thread and keeps `fs-info`'s three-tier reducer on the UI thread.
- Launcher and friends: `menu`, `clipboard`, `clipssh`, `picker`,
  `localsend`, `mirror` (with `v4l2`).
- Security: `polkit`, `sleep`, `screensaver`, `capture`, `recording`.
- `proc`: one child run and read whole, a missing binary reading as exit 127.

Child processes (matugen, cava, grim, wf-recorder, ttfx, localsend-cli,
uxplay and the rest) stay child processes, spawned and read on the service
thread. Long-lived ones start under `setpriv` with `PR_SET_PDEATHSIG`.

### The library crates

The `fs-*` crates split into pure logic (plain data in, plain data out, no
IO) and D-Bus clients that take a `zbus::Connection` the caller owns and
spawn nothing, so they run on the shell's executor.

| Crate | Holds |
|---|---|
| `fs-theme` | chrome tables, tokens, presets, the `box()` resolver (`style.rs`), palettes, the pure half of matugen |
| `fs-chrome` | bar layout resolver, workspace cell model, drawer and frame geometry, panel cursor, dither, switcher list, hot corner arming, plugin manifests |
| `fs-menu` | the launcher tree, fuzzy search, key policy, frecency, calculator, providers, clipboard history, emoji |
| `fs-info` | notifications and toasts, reminders, calendar, weather, location, agents, usage, flake updates |
| `fs-media` | MPRIS source picking, Apple Music art, AirPlay, lyrics, radio, visualizer styles |
| `fs-system` | monitor parsers, power, display, lights, capture, keybinds, audio, camera, controller, lock and overnight models |
| `fs-devices` | earbuds, iPhone, Bluetooth, LocalSend, Wi-Fi and Tailscale models |
| `fs-screensaver` | built-in effects, the ttfx wire protocol, block geometry |
| `fs-js` | JavaScript's number and string formatting, so output strings match byte for byte |
| `fs-audio` | the PipeWire graph model and its client thread |
| `fs-auth` | the lock's PAM conversation and the polkit agent |
| `fs-bluez`, `fs-network`, `fs-upower`, `fs-mpris`, `fs-tray` | BlueZ, NetworkManager, UPower and power-profiles-daemon, MPRIS, StatusNotifier with dbusmenu |
| `fs-notifd` | the `org.freedesktop.Notifications` server |

## Theme engine

`services::theme` runs the files and the children; `fs-theme` holds the pure
halves.

```
state.json (wallpaper, mode) + settings.json (theme.*)
  │  theme::Inputs
  ▼
theme::run ── matugen --dry-run probe ── rank 0 source colour
  │                                         │
  │            matugen image --prefer closest-to-fallback --fallback-color <rank0>
  │             (config: fs_theme::matugen merge of the user's config, the shell's
  │              templates from FS_TEMPLATE_DIR, matugen.d drop-ins)
  ▼
theme.json  ──▶ theme::watch ──▶ Store.theme ──▶ every surface
formalshell-colors.lua, formalshell-chrome.lua ──▶ Hyprland
```

- A retheme queues instead of overlapping, and a run is never killed
  mid-write. Its wallpaper and mode are captured when it starts.
- The source colour is pinned to matugen's own rank 0 rather than left to
  `--prefer`; the module header records why.
- A wallpaper whose path names a pinned palette (flexoki, zenbones) has its
  templates rewritten to that palette's tones before matugen renders them.
- No wallpaper means the static zinc palette.
- `theme.mode: "auto"` follows sunrise and sunset (`fs_theme::sun`) off the
  configured location, and a toggle snoozes the schedule for one cycle.

`theme.preset` (`fs_theme::presets`: `metamorphosis`, `retro`, `pantheon`,
`cartridge`) picks a chrome table and a set of settings defaults. An
explicit settings key always wins over a preset's default; the table
itself is not overridable. `fs_theme::tables` embeds the JSON tables and
`fs_theme::style` resolves a role plus a state into what a box draws. What a
theme owns and what stays global is
`docs/superpowers/specs/2026-09-18-theme-boundary.md`.

## Flows

### Notifications

`fs-notifd` serves `org.freedesktop.Notifications` on the service thread.
The name is requested without replacement, so a running daemon is never
displaced and the shell shows that it could not take the name. Arrivals
reach `services::notifications` on the UI thread, where `fs-info`'s reducer
files each one into popup, pending or seen. The server closes nothing on
its own: every close the reducer decides goes back to it, so the sender
hears the right reason. `wayland/toasts.rs` sleeps until the next deadline
(a toast timing out, a reminder coming due) rather than ticking, and opening
the centre suppresses the toast stack.

Reminders live in state.json; `reminder` sets them, and one firing bypasses
DND into the popup tier.

### Launcher

`services::menu` keeps the launcher index warm on the service thread: the
base tree (the built-in default menu, the user's `menu.jsonc`,
`menu.customPowerButtons`) plus one row list per provider source, each
recomputed alone and attached as its own diff. Desktop entries and their
icons are scanned and decoded on the pool. An open only maps a surface over
rows that already exist. `fs-menu` holds the tree, ranking and key policy.

`menu select <prompt> <optionsJson> <token>` and `menu input` hand an
answer back to a script: the result lands in `menu-selection.txt` as
`{token, value}` or `{token, cancelled: true}`, and `picker select` answers
in `picker-selection.txt` the same way. The caller matches its own token.

### Lock and sleep

`wayland/lock.rs` takes ext-session-lock-v1 with one surface per output
(`surfaces/lock.rs` draws each). The password goes to `fs-auth`'s PAM
conversation against the `formalshell-lock` service, off the UI thread, and
PAM's own success is the only unlock. It fails closed: an error never
unlocks, and a shell that dies while locked leaves the compositor holding
the lock. `services::sleep` holds a logind `delay` inhibitor while awake, so
`PrepareForSleep(true)` reaches the lock first; the lock lets it go once its
surface is secure, and a fresh one is taken on wake.

### Idle and screensaver

`wayland/caffeinate.rs` listens to ext-idle-notify (respecting inhibitors)
and, while caffeinate is on, holds an idle inhibitor on a 1px surface of its
own. `wayland/screensaver.rs` crosses that idle state with the live media
guard. Frames come from a `ttfx` child parsed on the service thread
(`services::screensaver`) when ttfx is on PATH, and from `fs-screensaver`'s
built-in effects when it is not. The overlay fades through
`wp_alpha_modifier_v1` and nothing ticks while it is down.

### Capture

`surfaces/capture/` makes every decision for screenshot, capture and record
on the UI thread, next to the region picker it drives
(`wayland/picker.rs`). The children (slurp, grim, tesseract, wf-recorder,
ffmpeg, the editor) run in `services::capture` and report their pid on
start and their answer on exit. `services::recording` is the slice the bar's
recording indicator reads.

### Plugins

`services::plugins` loads each directory under
`~/.config/formalshell/plugins/<id>/` whose `manifest.json` passes
`fs-chrome`'s manifest resolver, and starts the executable named by `entry`
with the directory as its working directory. The plugin prints one JSON
object per line on stdout (`text`, `icon`, `tooltip`, `class`, `rows`), and
reads `click`, `scroll` and `activate` events as JSON lines on stdin. A
plugin that exits or cannot start is the dim PLUGIN ERROR cell and is
restarted on a doubling backoff. While it prints nothing, the shell only
keeps a read parked on its stdout.

One process manager serves every kind. A `panel` plugin is a
`surfaces::panel::plugin::PluginPanel` on the ordinary panel host: its
`plugin:<id>` name is interned (panel names are `&'static str`), the title
and width come from the manifest, and the body is built from `ui::w` widgets
over the rows in `store.plugins.runs`. An `overlay` plugin draws the same
body on a modal card (`wayland/plugin_overlay.rs`, modelled on the polkit
dialog). Opening a card sends `services::plugins::shown(id, true)`, which
starts a plugin that is not `keepLoaded`, and closing it ends the process and
clears its rows.

`bar.modules` entries of type `command` are separate: `services::commands`
runs the argv every `interval` ms and kills a run past `timeout`.

## Greeter

`formalshell greeter` (`src/greeter/mod.rs`) runs as greetd's
`default_session` inside a compositor of its own with no other client, so
there is no session lock: one overlay layer surface per output shows the
lock screen's centre column over a flat background, and only the first
output takes the keyboard. `greeter/greetd.rs` speaks greetd-ipc on
`$GREETD_SOCK`. It never reads state.json, which would belong to the
`greeter` system user; the theme is settings.json's preset over theme.json's
palette. `formalshell install` can write the greetd config.

## Packaging

- `dev/tarball.sh` builds the release tarball in a `debian:bookworm`
  container, holding every binary to glibc 2.36: the shell and
  `formalshell-ipc`, the helpers no distro carries in `lib/formalshell/bin`
  (ttfx, matugen, tensaku, clipssh, localsend-cli, the iPhone bridge, the
  earbuds CLIs, formalshell-eds), the fonts, Blue Marble tiles and earbuds
  art in `share/formalshell`, and a `bin/formalshell` launcher that puts
  those helpers first on PATH. It also builds `formalshell-watchdog` from
  `packaging/formalshell-watchdog.in`: a probe that restarts the service
  after two IPC timeouts in a row, since a hung main loop leaves a live
  process that `Restart=on-failure` never catches.
- `install.sh` installs Hyprland and the runtime tools with pacman, apt or
  dnf, unpacks the release tarball under `~/.local` (or `/usr/local` with
  `--system`), and runs `formalshell install`.
- `src/install.rs`: `formalshell install` writes the user units, the
  Hyprland include and a `hypr-user.lua` the user owns, and, behind a sudo
  prompt each, the PAM file and the greetd config. Every file it owns
  starts with a marker line; `formalshell update` rewrites only those, and
  `formalshell uninstall` removes them.

## Extending

- A bar cell: a module under `surfaces/bar/cells/` implementing `Cell`, and
  one arm in `cells::build`.
- A panel: a module under `surfaces/panel/` implementing `Panel`, its name
  in `PANELS`, and one arm in `panel::build`.
- An IPC target: a module under `src/ipc/` returning a `registry::Target`,
  listed in `ipc::registry()`.
- A service: a module under `src/services/` with its `State` and `Diff`, a
  slice and a `Topic` in `store.rs`, its task started in `services::start`,
  and the surfaces that read it in `surfaces::changed`.
- Compositor work goes in `src/services/hyprland/` only.
