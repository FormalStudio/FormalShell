"""The Wayland wire protocol for the rig's client helpers (vpointer.py,
lockpointer.py): one connection to $WAYLAND_DISPLAY, objects numbered from 2
(1 is wl_display), messages packed by hand. Needs nothing but python3."""

import array
import os
import socket
import struct
import sys


class Wire:
    def __init__(self, prog):
        self.prog = prog
        name = os.environ.get("WAYLAND_DISPLAY", "wayland-0")
        path = name if name.startswith("/") else os.path.join(os.environ["XDG_RUNTIME_DIR"], name)
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.connect(path)
        self.next_id = 2
        self.buf = b""

    def new_id(self):
        i = self.next_id
        self.next_id += 1
        return i

    def send(self, obj, opcode, payload=b"", fd=None):
        msg = struct.pack("<II", obj, ((8 + len(payload)) << 16) | opcode) + payload
        if fd is None:
            self.sock.sendall(msg)
        else:
            self.sock.sendmsg([msg], [(socket.SOL_SOCKET, socket.SCM_RIGHTS, array.array("i", [fd]))])

    def read(self):
        """The next event as (object, opcode, body); a wl_display error ends the process."""
        while len(self.buf) < 8 or len(self.buf) < (struct.unpack_from("<I", self.buf, 4)[0] >> 16):
            chunk = self.sock.recv(65536)
            if not chunk:
                sys.exit(f"{self.prog}: compositor closed the connection")
            self.buf += chunk
        obj, word = struct.unpack_from("<II", self.buf)
        size = word >> 16
        body = self.buf[8:size]
        self.buf = self.buf[size:]
        if obj == 1 and word & 0xFFFF == 0:
            _, code, n = struct.unpack_from("<III", body)
            sys.exit(f"{self.prog}: protocol error {code}: {body[12:12 + n - 1].decode()}")
        return obj, word & 0xFFFF, body

    def roundtrip(self, on_event=None):
        callback = self.new_id()
        self.send(1, 0, struct.pack("<I", callback))
        while True:
            obj, op, body = self.read()
            if obj == callback:
                return
            if on_event:
                on_event(obj, op, body)


def string(s):
    b = s.encode() + b"\0"
    return struct.pack("<I", len(b)) + b + b"\0" * (-len(b) % 4)
