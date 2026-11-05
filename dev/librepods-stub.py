#!/usr/bin/env python3
"""The omarchy-pods librepods daemon's IPC, for FormalShell's --earbuds leg.

Reconstructed from omarchy-pods v1.0.2 (daemon/main.cpp, the QLocalServer
newConnection handler, and writeStateFile), the build ~/.config/nix packages:
one connection carries one verb, read in one go with no terminator, matched
exactly, and the daemon hangs up after it. `status` is answered with the
status line; an unknown verb is logged as the daemon logs it. A noise verb
for the mode already held is dropped, as setNoiseControlMode does. The state
file is rewritten through a rename, as QSaveFile does.

`heartrate:on` and `heartrate:off` start and stop the heart rate stream
(FormalSnake/omarchy-pods branch formalshell): while on, `heart_rate` is
rewritten once a second, first WARMUP's unsettled values (what real buds
read at the start of a session), then SETTLED's in a loop. `available` is
the fixture's own; an `on` with it false streams nothing, the daemon
dropping the link instead, and stays logged for the leg to fail on.

`motion:watch` keeps its connection and streams one JSON line per sample,
`{"q":[w,x,y,z],"g":[x,y,z],"gyro":[x,y,z],"t":<ms>}`, at 25 Hz until the
client goes (FormalSnake/omarchy-pods branch formalshell). The script runs
from the watch's own start: still for 1 s, a yaw turn of +40 degrees over
1 s, a 6 s hold, back over 1 s, 1 s still, a 20 degree nod over 1 s, then
round again after 12 s. The body starts START_YAW degrees round, so a
reference taken at the first sample matters. `--motion-error` answers the
watch with `error: ...` and hangs up instead. Watch starts and ends go to
--motion-log (default --log) as `motion:watch` and `motion:end`.
`--motion-from <s>` starts each watch that far into the script and
`--motion-pace <n>` plays it n times slower, so a photograph lands mid-move;
`--motion-silent` holds the watch open and sends nothing.

--hang plays the daemon e1504g ran on 2026-10-09: its main thread blocked in
pa_threaded_mainloop_wait, the socket still listening and never accepting.

Every verb received is appended to --log, one per line, exactly as read.
"""
import argparse
import json
import math
import os
import socket
import sys
import time

NOISE = {"noise:off": 0, "noise:anc": 1, "noise:transparency": 2, "noise:adaptive": 3}
EAR = {"ear:one": 0, "ear:both": 1, "ear:off": 2}
WARMUP = [169, 99, 66]
SETTLED = [72, 74, 73, 76, 75, 73, 71]
START_YAW = 25.0
CYCLE = 12.0
PERIOD = 0.04


def ramp(t, start, length):
    """0 to 1 over [start, start + length], eased."""
    x = min(max((t - start) / length, 0.0), 1.0)
    return x * x * (3 - 2 * x)


def pose(t):
    """(yaw, pitch) in degrees, relative to the watch's start."""
    t %= CYCLE
    yaw = 40.0 * (ramp(t, 1, 1) - ramp(t, 8, 1))
    pitch = 20.0 * (ramp(t, 10, 0.5) - ramp(t, 10.5, 0.5))
    return yaw, pitch


def quat(yaw, pitch):
    """Body to world, z up: about z by yaw, then about y by pitch."""
    hy, hp = math.radians(yaw) / 2, math.radians(pitch) / 2
    wz, zz, wy, yy = math.cos(hy), math.sin(hy), math.cos(hp), math.sin(hp)
    return (wz * wy, -zz * yy, wz * yy, zz * wy)


def motion_line(t):
    yaw, pitch = pose(t)
    w, x, y, z = quat(START_YAW + yaw, pitch)
    # Gravity in the body frame: world (0, 0, -1) rotated by the conjugate.
    g = [-2 * (x * z - w * y), -2 * (y * z + w * x), -(1 - 2 * (x * x + y * y))]
    ahead = pose(t + PERIOD)
    rate = (ahead[0] - yaw) / PERIOD
    return json.dumps({
        "q": [round(c, 6) for c in (w, x, y, z)],
        "g": [round(c * 1010) for c in g],
        "gyro": [0, 0, round(rate * 4)],
        "t": int(time.time() * 1000),
    }, separators=(",", ":")) + "\n"


class Watcher:
    def __init__(self, conn, args):
        conn.setblocking(False)
        self.conn = conn
        self.start = time.monotonic()
        self.next = self.start
        self.args = args

    def tick(self, now):
        """False once the client has gone."""
        if now < self.next:
            return True
        self.next += PERIOD
        if self.args.motion_silent:
            try:
                return self.conn.recv(64) != b""
            except BlockingIOError:
                return True
            except OSError:
                return False
        try:
            t = self.args.motion_from + (now - self.start) / self.args.motion_pace
            self.conn.send(motion_line(t).encode())
        except BlockingIOError:
            pass
        except OSError:
            return False
        return True


IDLE_HEART = {"wanted": False, "available": False, "streaming": False, "bpm": 0, "settled": False, "sampled_at_ms": 0}


class Heart:
    def __init__(self):
        self.on = False
        self.n = 0
        self.next = 0.0

    def start(self, status):
        hr = status["heart_rate"]
        hr["wanted"] = True
        if hr["available"]:
            self.on, self.n, self.next = True, 0, time.monotonic()
            hr["streaming"] = True

    def stop(self, status):
        self.on = False
        status["heart_rate"].update(wanted=False, streaming=False)

    def tick(self, status):
        """True when a sample was written into status."""
        if not self.on or time.monotonic() < self.next:
            return False
        self.next += 1.0
        warm = self.n < len(WARMUP)
        bpm = WARMUP[self.n] if warm else SETTLED[(self.n - len(WARMUP)) % len(SETTLED)]
        self.n += 1
        status["heart_rate"].update(bpm=bpm, settled=not warm, sampled_at_ms=int(time.time() * 1000))
        return True


def write_status(path, status):
    tmp = path + ".tmp"
    with open(tmp, "w") as f:
        f.write(json.dumps(status, sort_keys=True, separators=(",", ":")) + "\n")
    os.replace(tmp, path)


def apply(status, verb, heart):
    if verb == "heartrate:on":
        heart.start(status)
        return True
    if verb == "heartrate:off":
        heart.stop(status)
        return True
    if verb in NOISE:
        if status.get("noise_mode") != NOISE[verb]:
            status["noise_mode"] = NOISE[verb]
            status["noise_control_changes_total"] = status.get("noise_control_changes_total", 0) + 1
        return True
    if verb in EAR:
        status["ear_detection_behavior"] = EAR[verb]
        return True
    if verb in ("ca:on", "ca:off"):
        status["conversational_awareness"] = verb == "ca:on"
        return True
    if verb in ("onebud:on", "onebud:off"):
        status["one_bud_anc_mode"] = verb == "onebud:on"
        return True
    if verb.startswith("adaptive:"):
        tail = verb[len("adaptive:"):]
        if tail.isdigit() and 0 <= int(tail) <= 100 and status.get("noise_mode") == 3:
            status["adaptive_noise_level"] = int(tail)
        return True
    return verb in ("noise:cycle", "forget", "connect", "disconnect", "reopen", "status")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--socket", required=True)
    ap.add_argument("--status", required=True, help="status.json to publish, seeded from --fixture")
    ap.add_argument("--fixture", required=True)
    ap.add_argument("--log", required=True)
    ap.add_argument("--motion-log", help="where motion:watch and motion:end go, default --log")
    ap.add_argument("--motion-error", action="store_true", help="answer motion:watch with an error line and hang up")
    ap.add_argument("--motion-from", type=float, default=0.0, help="seconds into the script each watch starts at")
    ap.add_argument("--motion-pace", type=float, default=1.0, help="play the script this many times slower")
    ap.add_argument("--motion-silent", action="store_true", help="hold the watch open and send nothing")
    ap.add_argument("--hang", action="store_true", help="listen and never accept, as a daemon with its event loop stuck")
    args = ap.parse_args()

    with open(args.fixture) as f:
        status = json.load(f)
    status["heart_rate"] = {**IDLE_HEART, **status.get("heart_rate", {})}
    heart = Heart()
    os.makedirs(os.path.dirname(args.status), exist_ok=True)
    write_status(args.status, status)

    try:
        os.unlink(args.socket)
    except FileNotFoundError:
        pass
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    server.bind(args.socket)
    server.listen(8)
    if args.hang:
        # The daemon with its event loop stuck: the kernel still queues
        # connections on the listening socket, nothing ever accepts them.
        while True:
            time.sleep(60)
    server.settimeout(PERIOD / 2)
    watchers = []
    motion_log = args.motion_log or args.log

    def note(path, text):
        with open(path, "a") as log:
            log.write(text + "\n")

    while True:
        if heart.tick(status):
            write_status(args.status, status)
        now = time.monotonic()
        for w in watchers[:]:
            if not w.tick(now):
                watchers.remove(w)
                w.conn.close()
                note(motion_log, "motion:end")
                if not watchers and "motion" in status:
                    status["motion"]["streaming"] = False
                    write_status(args.status, status)
        try:
            conn, _ = server.accept()
        except socket.timeout:
            continue
        try:
            conn.settimeout(2)
            try:
                data = conn.recv(4096)
            except socket.timeout:
                data = b""
            if not data:
                continue
            verb = data.decode("utf-8", "replace")
            if verb == "motion:watch":
                note(motion_log, verb)
                if args.motion_error:
                    conn.sendall(b"error: motion unavailable on this link\n")
                else:
                    watchers.append(Watcher(conn, args))
                    conn = None
                    if "motion" in status:
                        status["motion"]["streaming"] = True
                        write_status(args.status, status)
                continue
            with open(args.log, "a") as log:
                log.write(verb + "\n")
            if not apply(status, verb, heart):
                print(f"Unknown message received: {verb!r}", file=sys.stderr, flush=True)
            elif verb == "status":
                conn.sendall(json.dumps(status, sort_keys=True, separators=(",", ":")).encode() + b"\n")
            write_status(args.status, status)
        finally:
            if conn is not None:
                conn.close()


if __name__ == "__main__":
    main()
