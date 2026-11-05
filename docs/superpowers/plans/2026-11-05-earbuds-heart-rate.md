# Earbuds heart rate

The earbuds panel shows a live heart rate for earbuds that stream one,
which today is AirPods Pro 3 through the librepods daemon.

## Source

The owner's hosts run librepods from `FormalSnake/omarchy-pods` branch
`formalshell` (spec `modules/linux/store.star`), upstream PR
thisisgm/omarchy-pods#80. Its `status.json` (schema_version 1, additive)
carries:

```json
"heart_rate": {"wanted": false, "available": true, "streaming": false,
               "bpm": 0, "settled": false, "sampled_at_ms": 0}
```

- `available`: the buds published their heart rate service on this link.
  Only then is `heartrate:on` free. Sent while `available` is false and the
  buds are connected, the daemon drops and reconnects the link (about 5 s
  of silence), so the shell never sends it then.
- `streaming`: the daemon asked for samples. `bpm` and `settled` are the
  last sample (one a second); the first few of a session read
  `settled: false` and run far off the real value (169, 99, 66 measured).
  `sampled_at_ms` is wall-clock milliseconds of that sample, 0 for none.
- Samples only come while a bud is in an ear (`left.in_ear`/`right.in_ear`).
- Verbs on the control socket: `heartrate:on`, `heartrate:off`.

## Task 1: model, service, rig

- `fs-devices` earbuds model: a `HeartRate` on `Device` (`Option`, set by
  the airpods adapter only when the daemon reports `available` or
  `streaming`), carrying the latest bpm, settled, whether a bud is in, and
  a short history of settled samples (dedupe on `sampled_at_ms`, capped,
  oldest first). Units in types: bpm is not a percentage, name it so.
  Parser tests beside the existing fixtures in `earbuds/airpods.rs`.
- Service (`services/devices/earbuds.rs`): the panel's hold (`panel(open)`)
  also starts and stops the stream. While the panel is open and the active
  AirPods report `available`, send `heartrate:on` once; on close send
  `heartrate:off` if it was sent. A daemon restart (file gone) forgets
  what was sent. History lives where it survives status reloads.
- `dev/librepods-stub.py` takes the two verbs and, while on, rewrites
  `heart_rate` every second: a few unsettled warm-up values, then settled
  ones, `available` per fixture.
- `--earbuds` leg: AirPods with `available: true`, the panel open reaching
  the stub as exactly `heartrate:on`, bpm read back (a status or dump
  field), the panel frame photographed with the readout, close reaching it
  as `heartrate:off`; a fixture with `available: false` gets no verb at all.
  Update the leg's paragraph in CLAUDE.md.

## Task 2: the visualizer

A "Heart rate" block in the earbuds panel, above the controls, for a device
with a `HeartRate`, following `docs/DESIGN.md` and the panel language check:

- The bpm in the values face (`tnum`), large, with a dim `BPM` caption.
- A Lucide `heart` that beats at the measured rate: a short systole swell
  and settle each period, and an expanding ring fading out from it per
  beat. Drawn by the clock at the output's refresh rate, only while the
  panel is open with a settled reading; static under reduced motion.
- A trace of the settled history (the kit's sparkline or a paint of its
  own like `ui/spectrum.rs`), with the session's low and high.
- Honest states: no settled sample yet reads "Measuring", no bud in reads
  "Put an AirPod in", never a made-up number or trace.
- Photographed dark and `--light`.

## Task 3: spot checks in the background

The panel opening to "Measuring" every time is the cost of streaming only
while it is open; streaming always costs the buds' battery and a wakeup a
second. So the service takes a spot check on its own:

- Every 10 minutes, while the active AirPods are connected, report
  `available`, have a bud in and the panel is closed: `heartrate:on`, keep
  the status file watched, take the settled samples for 30 s past the first
  settled one (or give up 20 s in with none), then `heartrate:off`. A check
  is skipped, not queued, when any condition fails; the panel opening
  mid-check simply carries on live and its close sends the off.
- Each check leaves one point: the median of its settled samples and its
  wall-clock time. Points are kept in the service for the session (no
  state.json), capped at 4 hours' worth, and survive the panel closing.
  Live samples while the panel is open stay as today.
- The panel, before its own first settled sample, shows the latest point as
  the readout with its age ("62 BPM, 4 min ago", the heart still, not
  beating) and the trace of points; once live samples settle it switches to
  the live readout and trace as today. No point and no live sample is still
  "Measuring".
- Between checks nothing wakes: no watch held, no timer finer than the
  10-minute one.
- Rig: the interval and window come from one place a leg can shorten
  (an env var read at start, as other legs do); the `--earbuds` leg runs
  one background check with the panel closed (stub log reads exactly
  `heartrate:on` then `heartrate:off`, no verb when `available` is false
  or no bud is in), then opens the panel and photographs the point readout.
