#!/usr/bin/env python3
"""A window holding a persistent zwp_locked_pointer_v1, the way a game
holds the cursor, logging every lock state change to a file.

    lockpointer.py APP_ID LOG

Each line of LOG is `<monotonic ms> <event>`: `configure WxH`, `enter`,
`leave`, `motion`, `locked`, `unlocked`, `rearm`, `release`. The lock is
asked for once the first configure lands; the compositor activates it
while the pointer is over the focused window. SIGUSR1 destroys the lock and
asks for a new one, which Hyprland activates at once while the window holds
keyboard focus, so a lock some earlier step released is back without focus
moving. SIGUSR2 destroys the lock and asks for none, which leaves the
window a fullscreen video's: any `motion` it logs is one that would wake
the video's hidden cursor. Talks the Wayland wire protocol through dev/wl.py,
so it needs nothing but python3 and WAYLAND_DISPLAY.
"""

import os
import signal
import struct
import sys
import time

from wl import Wire, string

XRGB8888 = 1
PERSISTENT = 2
POOL_W, POOL_H = 3840, 2160


def main(app_id, log_path):
    log = open(log_path, "a", buffering=1)

    def note(what):
        log.write(f"{int(time.monotonic() * 1000)} {what}\n")

    w = Wire("lockpointer")
    registry = w.new_id()
    w.send(1, 1, struct.pack("<I", registry))
    callback = w.new_id()
    w.send(1, 0, struct.pack("<I", callback))
    found = {}
    while True:
        obj, op, body = w.read()
        if obj == callback:
            break
        if obj == registry and op == 0:
            name, n = struct.unpack_from("<II", body)
            found[body[8:8 + n - 1].decode()] = name

    def bind(iface, version):
        if iface not in found:
            sys.exit(f"lockpointer: no {iface}")
        i = w.new_id()
        w.send(registry, 0, struct.pack("<I", found[iface]) + string(iface) + struct.pack("<II", version, i))
        return i

    compositor = bind("wl_compositor", 4)
    shm = bind("wl_shm", 1)
    wm = bind("xdg_wm_base", 1)
    seat = bind("wl_seat", 1)
    constraints = bind("zwp_pointer_constraints_v1", 1)

    fd = os.memfd_create("lockpointer")
    os.ftruncate(fd, POOL_W * POOL_H * 4)
    pool = w.new_id()
    w.send(shm, 0, struct.pack("<Ii", pool, POOL_W * POOL_H * 4), fd=fd)
    pointer = w.new_id()
    w.send(seat, 0, struct.pack("<I", pointer))

    surface = w.new_id()
    w.send(compositor, 0, struct.pack("<I", surface))
    xdg_surface = w.new_id()
    w.send(wm, 2, struct.pack("<II", xdg_surface, surface))
    toplevel = w.new_id()
    w.send(xdg_surface, 1, struct.pack("<I", toplevel))
    w.send(toplevel, 2, string(app_id))
    w.send(toplevel, 3, string(app_id))
    w.send(surface, 6)

    size = (640, 400)
    pending = size
    locked_pointer = None
    want_lock = True

    def rearm(*_):
        nonlocal locked_pointer, want_lock
        want_lock = True
        if locked_pointer is not None:
            w.send(locked_pointer, 0)
        locked_pointer = w.new_id()
        note("rearm")
        w.send(constraints, 1, struct.pack("<IIIII", locked_pointer, surface, pointer, 0, PERSISTENT))

    def release(*_):
        nonlocal locked_pointer, want_lock
        want_lock = False
        if locked_pointer is not None:
            w.send(locked_pointer, 0)
            locked_pointer = None
        note("release")

    signal.signal(signal.SIGUSR1, rearm)
    signal.signal(signal.SIGUSR2, release)
    while True:
        obj, op, body = w.read()
        if obj == wm and op == 0:
            w.send(wm, 3, body[:4])
        elif obj == toplevel and op == 0:
            width, height = struct.unpack_from("<ii", body)
            if width > 0 and height > 0:
                pending = (min(width, POOL_W), min(height, POOL_H))
        elif obj == xdg_surface and op == 0:
            w.send(xdg_surface, 4, body[:4])
            size = pending
            note(f"configure {size[0]}x{size[1]}")
            buffer = w.new_id()
            w.send(pool, 0, struct.pack("<IiiiiI", buffer, 0, size[0], size[1], size[0] * 4, XRGB8888))
            w.send(surface, 1, struct.pack("<Iii", buffer, 0, 0))
            w.send(surface, 2, struct.pack("<iiii", 0, 0, size[0], size[1]))
            w.send(surface, 6)
            if locked_pointer is None and want_lock:
                locked_pointer = w.new_id()
                w.send(constraints, 1, struct.pack("<IIIII", locked_pointer, surface, pointer, 0, PERSISTENT))
        elif obj == pointer and op in (0, 1):
            note("enter" if op == 0 else "leave")
        elif obj == pointer and op == 2:
            note("motion")
        elif obj == locked_pointer and op in (0, 1):
            note("locked" if op == 0 else "unlocked")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
