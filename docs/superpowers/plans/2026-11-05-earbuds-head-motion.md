# Earbuds head motion

The earbuds panel shows the wearer's head orientation live for earbuds that
stream device motion, which today is AirPods Pro 3 through the librepods
daemon, beside the heart rate block.

## What the buds send

Measured on g815 against an A3064 (firmware 90.3431), 2026-11-05. The buds
publish a `devmotion6` sensor service among the descriptors they list once
per ACL (the daemon's handshake query already gets them listed). Sensor
service 16 (0x10) started with the heart rate's packet shape at a 40000 us
interval (`HeartRate::streamPacket(0x10, seq, true)` with the interval a
parameter) streams 25 frames a second on opcode 0x17, each carrying `1a 3a`
then a 58-byte report `r` (`r[0] == 0x01`):

- `r[1..9]` u64 timestamp, `r[12..20]` a second one.
- `r[20..26]` three i16 LE: the vector part (x, y, z) of a unit quaternion,
  each / 32768, `w = sqrt(1 - x^2 - y^2 - z^2)`. All zero in the first few
  frames of a stream (warm-up): drop those. Checked against `r[44..50]`:
  rotating world (0, 0, -1) by the conjugate of q gives the measured gravity
  direction, mean cosine 0.99993 over 497 samples with natural head motion.
- `r[26..32]` three i16: angular rate (gyro), raw units.
- `r[44..50]` three i16: gravity in milli-g (|g| 1010 to 1018).
- `r[54..58]` u32 counter, +8 a frame.

Stopping is the same packet with interval 0. No `0x30` sensor-on control
command is needed for it as far as measured (the probe sent one; Task 1
confirms without). Yaw is relative to wherever the stream started: there is
no magnetometer reference.

Services 14 (`activity`, 5 Hz, 46-byte frames) and 18 (nothing) are out of
scope.

## Task 1: daemon (`~/Developer/omarchy-pods`, FormalSnake fork)

Branch `feat/head-motion` off `feat/heart-rate`, merged into the deploy
branch `formalshell` too.

- `daemon/motion.hpp` (header-only beside `heartrate.hpp`): service id,
  descriptor marker `devmotion`, `parseMotion(const QByteArray&)` returning
  quaternion (w, x, y, z as float), gravity (3 ints, milli-g), gyro (3
  ints) and the counter, nullopt for a zero quaternion or a short frame.
  Unit tests in `tests/tst_motion.cpp` off frames captured on g815 (in this
  plan's appendix).
- Streaming follows its watchers, not a flag: a control-socket client
  sending `motion:watch` keeps its connection open and gets one compact JSON
  line per sample, `{"q":[w,x,y,z],"g":[x,y,z],"gyro":[x,y,z],"t":<ms>}`.
  The first watcher starts service 16, the last one disconnecting (or the
  control link going) stops it. A shell that dies stops the stream with it.
  `motion:watch` while unavailable answers `error: ...` and closes.
- Status gains `"motion": {"available", "streaming"}` (additive);
  `available` once the `devmotion` descriptor was seen on this link.
- Verified on g815 (podman Tumbleweed build of the tree, the fspec daemon
  stopped meanwhile and restarted after, audio left on the AirPods): ctest,
  then `motion:watch` printing 25 lines a second with a unit quaternion, and
  the stream stopping on disconnect (no more 0x17 frames in the debug log).
- Commit, push the branch and `formalshell`, open a PR against
  thisisgm/omarchy-pods stacked on #80.

## Task 2: model, service, rig (FormalShell)

- `fs-devices` earbuds: a `Motion` on `Device` (`Option`, AirPods only, when
  the daemon reports `motion.available`), carrying the latest orientation as
  a quaternion type of its own (unit quaternion, named methods for the
  relative rotation against a reference and for yaw, pitch, roll in
  degrees), gravity, and whether it is live. Parser tests for the line
  format and the status key.
- Service (`services/devices/earbuds.rs`): while the panel is open and the
  active AirPods report `motion.available`, hold one `motion:watch`
  connection on the service thread and fold its lines into the device; drop
  it on close. The reference orientation is the first sample of a watch;
  `earbuds recenter` (IPC) takes the current one. Nothing held while closed.
- `dev/librepods-stub.py`: `motion:watch` streams a scripted head motion
  (still, a yaw turn, a nod) at 25 Hz until the client goes.
- `--earbuds` leg: AirPods with `motion.available`, the panel open reaching
  the stub as `motion:watch`, `earbuds status` reading the orientation the
  stub is at, closing the panel ending the stub's watch, `available: false`
  sending none. Update the leg's paragraph in CLAUDE.md.

## Task 3: the visualizer

A "Head" block in the earbuds panel under the heart rate block, for a
device with a `Motion`, following `docs/DESIGN.md` and the panel language
check:

- A wireframe head turned by the relative orientation: a sphere of a few
  latitude and longitude lines, a nose and two ears marking front and
  sides, projected flat, back lines dimmer than front ones, drawn in the
  theme's foreground with `primary` on the nose. Drawn off frame callbacks
  while samples arrive, eased between samples so 25 Hz input reads smooth.
- Yaw, pitch and roll in the values face (`tnum`) with a dim caption each.
- A Recenter button (`ui::w` button, Lucide icon) doing what `earbuds
  recenter` does.
- Honest states: no sample yet reads "Waiting for motion", no bud in reads
  "Put an AirPod in".
- Photographed dark and `--light`.

## Appendix: frames captured on g815

Two service-16 frames (whole AACP packets) verbatim, the first in the
warm-up with a zero quaternion:

```
04000400170000001000450008d10310013a3e08101a3a016068b1ad661700000300821087510d00000000000000000000000000000000621003e93404a1026102c005a2026802c201827fde38be0b0000
04000400170000001000450008f70310013a3e08101a3a0118e1df07671700000300433f9d680d0000000086953d177026d4ff91ffc0ff610003e93404fffff3ff030099026302d30160d26638ee0c0000
```

The second decodes to q vector (-0.8318, 0.1815, 0.3003), gravity (665, 611, 467).

## Task 4: shake to dismiss, nod to act

Only while a toast is up and the active AirPods have a bud in and report
`motion.available`, the service holds a `motion:watch` (the panel's watch
shares it) and reads the gyro's yaw and pitch rates:

- A shake: at least two sign reversals of the yaw rate, each swing past a
  rate threshold, inside 1.2 s. Dismisses the front toast.
- A nod: the same on pitch. Runs the front toast's default action, if it
  has one; otherwise nothing.
- Thresholds measured first off a capture of deliberate shakes and nods and
  of looking around, so ordinary glances never fire. Behind
  `notifications.headGestures` (default true), and off under DND.
- The watch drops a second after the last toast goes. Rig: the stub plays a
  shake and a glance; the glance dismisses nothing, the shake the front toast.
