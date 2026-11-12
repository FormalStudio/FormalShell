#!/usr/bin/env python3
"""A held-button pointer drag over zwlr_virtual_pointer_v1, which wlrctl
cannot do (its `pointer click` presses and releases in one go), and a
two-finger touchpad scroll, which wlrctl cannot do either (its `pointer
scroll` carries no axis_source, so Hyprland reads it as a wheel).

    vpointer.py down | up | move DX DY | wheel DY | finger DY | hfinger DX | lift | hlift | wait MS ...

`wheel DY` is one vertical axis frame of DY with no axis_source, which
ShojiWM hands on as a wheel (wlrctl's `pointer scroll` names finger, which
only Hyprland drops). `finger DY` is one vertical axis frame of DY pixels
with axis_source finger, `hfinger DX` the same on the horizontal axis; `lift` is the
axis_stop a touchpad sends when the fingers leave it, `hlift` the
horizontal axis's.

Steps run in order on one virtual pointer. Every event carries the
monotonic clock in milliseconds as its time, the way a real device's
events do, and is followed by a frame. Talks the Wayland wire protocol
directly, so it needs nothing but python3 and WAYLAND_DISPLAY.
"""

import struct
import sys
import time

from wl import Wire, string

BTN_LEFT = 0x110
AXIS_VERTICAL = 0
AXIS_HORIZONTAL = 1
SOURCE_FINGER = 1


def fixed(v):
    return struct.pack("<i", int(round(v * 256)))


def main(steps):
    w = Wire("vpointer")
    registry = w.new_id()
    w.send(1, 1, struct.pack("<I", registry))
    found = {}

    def global_(obj, op, body):
        if obj == registry and op == 0:
            name, n = struct.unpack_from("<II", body)
            iface = body[8:8 + n - 1].decode()
            version = struct.unpack_from("<I", body, 8 + n + (-n % 4))[0]
            found[iface] = (name, version)

    w.roundtrip(global_)
    if "zwlr_virtual_pointer_manager_v1" not in found:
        sys.exit("vpointer: no zwlr_virtual_pointer_manager_v1")
    name, _ = found["zwlr_virtual_pointer_manager_v1"]
    manager = w.new_id()
    w.send(registry, 0, struct.pack("<I", name) + string("zwlr_virtual_pointer_manager_v1") + struct.pack("<II", 1, manager))
    pointer = w.new_id()
    w.send(manager, 0, struct.pack("<II", 0, pointer))
    w.roundtrip()

    def stamp():
        return int(time.monotonic() * 1000) & 0xFFFFFFFF

    i = 0
    while i < len(steps):
        step = steps[i]
        if step in ("down", "up"):
            w.send(pointer, 2, struct.pack("<III", stamp(), BTN_LEFT, 1 if step == "down" else 0))
            w.send(pointer, 4)
            i += 1
        elif step == "move":
            dx, dy = float(steps[i + 1]), float(steps[i + 2])
            w.send(pointer, 0, struct.pack("<I", stamp()) + fixed(dx) + fixed(dy))
            w.send(pointer, 4)
            i += 3
        elif step == "wheel":
            w.send(pointer, 3, struct.pack("<II", stamp(), AXIS_VERTICAL) + fixed(float(steps[i + 1])))
            w.send(pointer, 4)
            i += 2
        elif step in ("finger", "hfinger"):
            # Hyprland's axis request resets the pending event's source, so
            # axis_source has to follow it inside the frame.
            axis = AXIS_VERTICAL if step == "finger" else AXIS_HORIZONTAL
            w.send(pointer, 3, struct.pack("<II", stamp(), axis) + fixed(float(steps[i + 1])))
            w.send(pointer, 5, struct.pack("<I", SOURCE_FINGER))
            w.send(pointer, 4)
            i += 2
        elif step in ("lift", "hlift"):
            axis = AXIS_VERTICAL if step == "lift" else AXIS_HORIZONTAL
            w.send(pointer, 6, struct.pack("<II", stamp(), axis))
            w.send(pointer, 5, struct.pack("<I", SOURCE_FINGER))
            w.send(pointer, 4)
            i += 1
        elif step == "wait":
            w.roundtrip()
            time.sleep(float(steps[i + 1]) / 1000)
            i += 2
        else:
            sys.exit(f"vpointer: unknown step {step}")
    w.send(pointer, 8)
    w.roundtrip()


if __name__ == "__main__":
    main(sys.argv[1:])
