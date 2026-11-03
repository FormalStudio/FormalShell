# A Bluetooth device's player as a media source

The owner's iPad streams A2DP to g815 and exposes an AVRCP target. BlueZ
publishes its now playing as `org.bluez.MediaPlayer1` under the device
(`<device>/avrcp/player0`: `Name`, `Status`, `Position` in ms, `Track` with
`Title`, `Artist`, `Album`, `Duration` in ms). No MPRIS name exists for it, so
until now the media panel only listed the A2DP stream as a bare `stream:` row
named after the device.

## Source

- fs-bluez keeps every `MediaPlayer1` beside the devices (`state::Player`,
  `Event::PlayerChanged`/`PlayerRemoved`), position and duration as
  `Duration`, and its narrow subscription takes `PropertiesChanged` for that
  interface. `Bluez::player_command` sends Play, Pause, Next, Previous.
- `devices::bluetooth` publishes them to the media store as `media::Remote`,
  one per player, id `bluez:<Device1.Address>` (the identity address: the
  object path can carry a random resolvable one), labelled with the device's
  alias. A player whose position and status did not move keeps its read time,
  so its clock runs on.
- The media service lists a remote as kind `bluetooth`, auto-pickable, playing
  while `Status` is `playing`. Title, artist, album, position and duration
  come off the player; next and previous are always offered; play/pause sends
  Pause or Play (AVRCP has no toggle); there is no seek.
- The stream BlueZ's PipeWire plugin makes for the device
  (`node.name` `bluez_input.<address with underscores>.<profile>`) belongs to
  the remote, the way an MPRIS player owns its streams: no row of its own, and
  the remote's volume and output are that stream's.
- Generic for any device with a `MediaPlayer1`; nothing names the iPad.

## Verification

`--bluez-player`, against a stub BlueZ on a private system bus (the rig's
nested runs have no BlueZ of their own and the VM rig's has no controller).
