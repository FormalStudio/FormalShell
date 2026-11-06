# iPad continuity

The owner, 2026-11-06: "A thing in the quick settings and launcher to see
iPad continuity and to also adjust the position of the screen."

padbridge (FormalStudio/padbridge, a daemon of its own, already on g815)
shares this machine's pointer and keyboard with an iPad over BLE HID:
pushing the pointer past one stretch of one edge of the desktop sends
input to the iPad. Its contract, on the user session bus: name
`studio.formal.Padbridge`, path `/studio/formal/Padbridge`, interface
`studio.formal.Padbridge1`, properties `Link` (`unpaired`, `down`, `up`),
`Active`, `Audio`, `Name`, `Edge` (`left`, `right`, `top`, `bottom`),
`Start` and `End` (0 <= Start < End <= 1, from the edge's top or left end),
each emitting PropertiesChanged, and methods `SetPosition(s, d, d)`,
`Switch()` and `Reconnect()`. The shell reads and calls it; padbridge
keeps the position. No owner on the name is the honest unavailable state.

## Task 1: the service and the IPC target

Files: `services/ipad.rs`, `store.rs`, `services/mod.rs`, `ipc/ipad.rs`,
`ipc/mod.rs`, `ipc/golden.rs`, `tests/ipc-golden.jsonl`.

- A service-thread task on a private session connection (`bus::private`):
  NameOwnerChanged for padbridge's name and its PropertiesChanged,
  subscribed before the first `GetAll`, publishing `Option<Ipad>`, `None`
  while the name has no owner. The stretch is a `Stretch` type holding
  padbridge's own rule, so a bad pair never crosses into the store.
- A command channel calls `Switch`, `Reconnect` and `SetPosition`.
- `ipad status|switch|reconnect|position <edge>|stretch <start> <end>`.
  `position` keeps the stretch and `stretch` keeps the edge: the registry
  has no optional arguments, and each verb is one control's whole call.
  Every verb but `status` answers `error: padbridge is not running` with
  no padbridge, and a bad edge or stretch is refused before the bus.
- Verify: `just lint` (the golden replay and the service's unit tests).

## Task 2: the quick settings tile

Files: `surfaces/panel/quick/mod.rs`, the quick settings spec's §3 table.

- A split toggle after HDR, `tablet`, titled with the iPad's name ("iPad"
  until padbridge knows it; the tile has no room for the name and the
  state on one line), the state as its subtitle (Connected, On iPad, Not
  connected, Not paired), on while input is on the iPad. Enter or a click
  runs `ipad switch`. With no padbridge it is disabled, "padbridge is not
  running", as Power mode is with no daemon.
- Its menu: Left, Right, Above, Below with a check on the edge
  (`ipad position`), Reconnect after a hairline while the link is down,
  then "iPad settings" handing over to the launcher's route.
- Verify: `just smoke --quick-settings` still passes with the tile in its
  order list.

## Task 3: the launcher route

Files: `fs-menu` `providers/system.rs` (`ipad_entries`,
`IPAD_STRETCHES`), `toggles.rs` (`ipad.active`, `ipad.edge`,
`ipad.stretch`), `icons.rs`; the shell's `services/menu.rs` (merged into
the base like the keyboard lights' route, present only while padbridge
is), `surfaces/launcher.rs` (the snapshot), `wayland/launcher.rs`
(`@ipc:ipad.<verb>:<args>`).

- One level, "iPad": Switch to iPad (ticked while on it), Reconnect, a
  Position section with the four edges and a Stretch section with Whole
  edge, First half, Second half and Middle, the current ones ticked.

## Task 4: the leg

Files: `dev/padbridge-stub.py`, `dev/smoke.d/ipad.sh`,
`dev/smoke-map.toml`, `dev/smoke-all.list`, CLAUDE.md's leg list,
`docs/USAGE.md`.

- `--ipad`: absent first, the stub appearing, Return on the tile reaching
  it as Switch, the menu's Above as SetPosition top 0 1, the link going
  down adding Reconnect, a launcher preset as SetPosition with its
  stretch, the stub killed taking it all away. Frames dark and `--light`.
- Verify: `just smoke --ipad`, `just smoke --ipad --light`, `just
  smoke-affected`.
