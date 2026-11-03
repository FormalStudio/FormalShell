#!/usr/bin/env python3
"""BlueZ with remotes playing over AVRCP, for FormalShell's --bluez-player
smoke leg.

Owns org.bluez on the bus it is pointed at and serves what g815's BlueZ
showed with the owner's iPad streaming to it: one powered adapter and a
connected device whose object path carries a random resolvable address
while its Address property is the identity one, with an
org.bluez.MediaPlayer1 at <device>/avrcp/player0 (Name, Status, Position,
Track). --extra adds that many more connected devices with a paused player
each, so the media panel's source menu has more rows than an output holds.

Play, Pause, Next and Previous are served on every player: each call is
appended to --log as a JSON line, Play and Pause set Status, Next and
Previous step through --tracks, and every change goes out as a
PropertiesChanged the way BlueZ sends it. Anything else is logged as
unhandled.
"""
import argparse
import json
import sys

from gi.repository import Gio, GLib

ADAPTER = "/org/bluez/hci0"
PLAYER_IFACE = "org.bluez.MediaPlayer1"

PLAYER_XML = """<node><interface name="org.bluez.MediaPlayer1">
<method name="Play"/><method name="Pause"/><method name="Stop"/>
<method name="Next"/><method name="Previous"/>
<property name="Name" type="s" access="read"/>
<property name="Status" type="s" access="read"/>
<property name="Position" type="u" access="read"/>
<property name="Track" type="a{sv}" access="read"/>
<property name="Device" type="o" access="read"/>
<property name="Type" type="s" access="read"/>
</interface></node>"""

MANAGER_XML = """<node><interface name="org.freedesktop.DBus.ObjectManager">
<method name="GetManagedObjects"><arg type="a{oa{sa{sv}}}" direction="out"/></method>
<signal name="InterfacesAdded"><arg type="o"/><arg type="a{sa{sv}}"/></signal>
<signal name="InterfacesRemoved"><arg type="o"/><arg type="as"/></signal>
</interface></node>"""


def track(t):
    out = {"Title": GLib.Variant("s", t["title"]), "Artist": GLib.Variant("s", t.get("artist", ""))}
    if t.get("album"):
        out["Album"] = GLib.Variant("s", t["album"])
    if t.get("duration_ms"):
        out["Duration"] = GLib.Variant("u", t["duration_ms"])
    return out


def device(address, alias):
    return {
        "Address": GLib.Variant("s", address),
        "Name": GLib.Variant("s", alias),
        "Alias": GLib.Variant("s", alias),
        "Icon": GLib.Variant("s", "multimedia-player"),
        "Paired": GLib.Variant("b", True),
        "Bonded": GLib.Variant("b", True),
        "Trusted": GLib.Variant("b", True),
        "Connected": GLib.Variant("b", True),
        "Adapter": GLib.Variant("o", ADAPTER),
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--address", required=True, help="the bus to own org.bluez on")
    ap.add_argument("--identity", required=True, help="the remote's identity address (Device1.Address)")
    ap.add_argument("--random", required=True, help="the random address its object path carries")
    ap.add_argument("--alias", required=True)
    ap.add_argument("--app", default="YouTube", help="MediaPlayer1.Name")
    ap.add_argument("--tracks", required=True, help="JSON list of {title, artist, album, duration_ms}")
    ap.add_argument("--position-ms", type=int, default=0)
    ap.add_argument("--extra", type=int, default=0, help="more connected remotes, each with a paused player")
    ap.add_argument("--log", required=True, help="JSON lines, one per method call")
    ap.add_argument("--ready", required=True, help="file written once org.bluez is owned")
    args = ap.parse_args()

    with open(args.tracks) as f:
        tracks = json.load(f)

    bus = Gio.DBusConnection.new_for_address_sync(
        args.address,
        Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,
        None,
        None,
    )

    objects = {
        ADAPTER: {
            "org.bluez.Adapter1": {
                "Address": GLib.Variant("s", "00:11:22:33:44:55"),
                "Name": GLib.Variant("s", "rig"),
                "Alias": GLib.Variant("s", "rig"),
                "Powered": GLib.Variant("b", True),
                "PowerState": GLib.Variant("s", "on"),
                "Discovering": GLib.Variant("b", False),
                "Discoverable": GLib.Variant("b", False),
                "Pairable": GLib.Variant("b", True),
            }
        }
    }
    # path -> {"turn": index into tracks, "props": MediaPlayer1 properties}
    players = {}

    def add_remote(random, identity, alias, status, turn, position_ms):
        dev = ADAPTER + "/dev_" + random.replace(":", "_")
        objects[dev] = {"org.bluez.Device1": device(identity, alias)}
        path = dev + "/avrcp/player0"
        props = {
            "Name": GLib.Variant("s", args.app),
            "Status": GLib.Variant("s", status),
            "Position": GLib.Variant("u", position_ms),
            "Track": GLib.Variant("a{sv}", track(tracks[turn])),
            "Device": GLib.Variant("o", dev),
            "Type": GLib.Variant("s", "Audio"),
        }
        objects[path] = {PLAYER_IFACE: props}
        players[path] = {"turn": turn, "props": props}

    add_remote(args.random, args.identity, args.alias, "playing", 0, args.position_ms)
    for i in range(args.extra):
        add_remote("5A:00:00:00:00:%02X" % i, "C0:FF:EE:00:00:%02X" % i, "Remote %d" % (i + 1), "paused", 0, 0)

    def log(entry):
        with open(args.log, "a") as f:
            f.write(json.dumps(entry) + "\n")

    def changed(path, values):
        players[path]["props"].update(values)
        bus.emit_signal(
            None,
            path,
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            GLib.Variant("(sa{sv}as)", (PLAYER_IFACE, values, [])),
        )

    def managed():
        return GLib.Variant(
            "(a{oa{sa{sv}}})",
            ({p: {i: dict(props) for i, props in ifaces.items()} for p, ifaces in objects.items()},),
        )

    def on_manager(conn, sender, path, iface, method, params, invocation):
        if method == "GetManagedObjects":
            invocation.return_value(managed())
        else:
            print(f"bluez-player-stub: unhandled {iface}.{method} on {path}", file=sys.stderr, flush=True)
            invocation.return_dbus_error("org.bluez.Error.NotSupported", "stub")

    def on_player(conn, sender, path, iface, method, params, invocation):
        p = players[path]
        log({"call": method, "path": path})
        if method in ("Play", "Pause"):
            changed(path, {"Status": GLib.Variant("s", "playing" if method == "Play" else "paused")})
        elif method in ("Next", "Previous"):
            p["turn"] = (p["turn"] + (1 if method == "Next" else -1)) % len(tracks)
            changed(path, {"Track": GLib.Variant("a{sv}", track(tracks[p["turn"]])), "Position": GLib.Variant("u", 0)})
        else:
            print(f"bluez-player-stub: unhandled {iface}.{method} on {path}", file=sys.stderr, flush=True)
            invocation.return_dbus_error("org.bluez.Error.NotSupported", "stub")
            return
        invocation.return_value(None)

    def get_property(conn, sender, path, iface, name):
        return players[path]["props"].get(name)

    bus.register_object("/", Gio.DBusNodeInfo.new_for_xml(MANAGER_XML).interfaces[0], on_manager, None, None)
    player_info = Gio.DBusNodeInfo.new_for_xml(PLAYER_XML).interfaces[0]
    for path in players:
        bus.register_object(path, player_info, on_player, get_property, None)

    def owned(*_):
        with open(args.ready, "w") as f:
            f.write("ready\n")

    Gio.bus_own_name_on_connection(bus, "org.bluez", Gio.BusNameOwnerFlags.NONE, owned, None)
    GLib.MainLoop().run()


if __name__ == "__main__":
    main()
