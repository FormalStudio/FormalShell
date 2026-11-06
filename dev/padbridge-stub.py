#!/usr/bin/env python3
"""padbridge on the session bus, for FormalShell's --ipad smoke leg.

Owns studio.formal.Padbridge and serves /studio/formal/Padbridge with
padbridge's own interface: Link, Active, Audio, Name, Edge, Start and End,
every change sent as PropertiesChanged. Switch flips Active, SetPosition
checks its arguments as padbridge does (InvalidArgs past them) and moves the
iPad, Reconnect brings Link up. Every call is appended to --log as a JSON
line. A JSON object written to --control sets those properties directly, as
padbridge's own link and audio events would.
"""
import argparse
import json
import os
import sys

from gi.repository import Gio, GLib

NAME = "studio.formal.Padbridge"
PATH = "/studio/formal/Padbridge"
IFACE = "studio.formal.Padbridge1"

XML = """<node><interface name="studio.formal.Padbridge1">
<method name="SetPosition"><arg name="edge" type="s" direction="in"/><arg name="start" type="d" direction="in"/><arg name="end" type="d" direction="in"/></method>
<method name="Switch"/>
<method name="Reconnect"/>
<property name="Link" type="s" access="read"/>
<property name="Active" type="b" access="read"/>
<property name="Audio" type="b" access="read"/>
<property name="Name" type="s" access="read"/>
<property name="Edge" type="s" access="read"/>
<property name="Start" type="d" access="read"/>
<property name="End" type="d" access="read"/>
</interface></node>"""

TYPES = {"Link": "s", "Active": "b", "Audio": "b", "Name": "s", "Edge": "s", "Start": "d", "End": "d"}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--name", default="FormalPad")
    ap.add_argument("--link", default="up")
    ap.add_argument("--edge", default="right")
    ap.add_argument("--log", required=True, help="JSON lines, one per method call")
    ap.add_argument("--control", required=True, help="a fifo taking JSON objects of properties")
    ap.add_argument("--ready", required=True, help="file written once the name is owned")
    args = ap.parse_args()

    props = {"Link": args.link, "Active": False, "Audio": False, "Name": args.name, "Edge": args.edge, "Start": 0.0, "End": 1.0}
    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)

    def log(entry):
        with open(args.log, "a") as f:
            f.write(json.dumps(entry) + "\n")

    def change(values):
        props.update(values)
        changed = {k: GLib.Variant(TYPES[k], v) for k, v in values.items()}
        bus.emit_signal(None, PATH, "org.freedesktop.DBus.Properties", "PropertiesChanged", GLib.Variant("(sa{sv}as)", (IFACE, changed, [])))

    def on_call(conn, sender, path, iface, method, params, invocation):
        if method == "SetPosition":
            edge, start, end = params.unpack()
            log({"call": method, "edge": edge, "start": start, "end": end})
            if edge not in ("left", "right", "top", "bottom") or not (0 <= start < end <= 1):
                invocation.return_dbus_error("org.freedesktop.DBus.Error.InvalidArgs", "bad position")
                return
            change({"Edge": edge, "Start": start, "End": end})
        elif method == "Switch":
            log({"call": method})
            change({"Active": not props["Active"]})
        elif method == "Reconnect":
            log({"call": method})
            change({"Link": "up"})
        else:
            print(f"padbridge-stub: unhandled {iface}.{method}", file=sys.stderr, flush=True)
            invocation.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", method)
            return
        invocation.return_value(None)

    def get_property(conn, sender, path, iface, name):
        return GLib.Variant(TYPES[name], props[name])

    bus.register_object(PATH, Gio.DBusNodeInfo.new_for_xml(XML).interfaces[0], on_call, get_property, None)

    if not os.path.exists(args.control):
        os.mkfifo(args.control)
    # Opened read and write, so the fifo never reports EOF between writers.
    fd = os.open(args.control, os.O_RDWR | os.O_NONBLOCK)
    pending = b""

    def on_control(_fd, _cond):
        nonlocal pending
        try:
            pending += os.read(fd, 65536)
        except BlockingIOError:
            return True
        *lines, pending = pending.split(b"\n")
        for line in lines:
            if line.strip():
                values = json.loads(line)
                log({"control": values})
                change(values)
        return True

    GLib.unix_fd_add_full(GLib.PRIORITY_DEFAULT, fd, GLib.IOCondition.IN, on_control)

    def owned(*_):
        with open(args.ready, "w") as f:
            f.write("ready\n")

    Gio.bus_own_name_on_connection(bus, NAME, Gio.BusNameOwnerFlags.NONE, owned, None)
    GLib.MainLoop().run()


if __name__ == "__main__":
    main()
