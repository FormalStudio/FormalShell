#!/usr/bin/env python3
"""A BlueZ on a private bus for FormalShell's smoke legs, in one of two shapes.

--count (--bluez-rssi): a BlueZ that is only scanning. Serves the same
objects crates/fs-bluez/tests/fake_bluez.rs does: one powered adapter,
discovering on a scan the shell did not start, one paired device and a
handful of strangers. Then it does what g815's BlueZ did at idle: a Device1
PropertiesChanged carrying only RSSI at --rate per minute, round robin over
every device, and a new stranger every 10 seconds.

--devices FILE (--earbuds, --headset-card): the rig has no Bluetooth
controller, so the stub plays one. A powered adapter that is not
discovering, and one paired device per entry of FILE, a JSON array of
`{address, name, connected, icon, battery}`: `icon` is Device1's Icon
(`audio-headset` when absent), `battery` 0 to 100 is Battery1's Percentage
(no Battery1 when absent), and `name` is both Name and Alias. The file is
read again every 100ms, so a leg connects and disconnects a device by
rewriting it: a changed entry sends Device1's or Battery1's
PropertiesChanged, an entry added or gone sends InterfacesAdded or
InterfacesRemoved. A file that does not parse (caught mid-write) is skipped.

Methods are not served; the shell must not call any of them while its panel
is shut, and a call shows up in the stub's own log as unhandled.
"""
import argparse
import json
import random
import sys

from gi.repository import Gio, GLib

ADAPTER = "/org/bluez/hci0"
DEVICE = "org.bluez.Device1"
BATTERY = "org.bluez.Battery1"


def device(address, alias, paired):
    return {
        "Address": GLib.Variant("s", address),
        "Name": GLib.Variant("s", alias),
        "Alias": GLib.Variant("s", alias),
        "Icon": GLib.Variant("s", "audio-headset" if paired else ""),
        "Paired": GLib.Variant("b", paired),
        "Bonded": GLib.Variant("b", paired),
        "Trusted": GLib.Variant("b", paired),
        "Connected": GLib.Variant("b", False),
        "Adapter": GLib.Variant("o", ADAPTER),
        "RSSI": GLib.Variant("n", -60),
    }


def path_of(address):
    return ADAPTER + "/dev_" + address.replace(":", "_")


def listed(entry):
    """One entry of --devices as the interfaces its object carries."""
    address = entry["address"].upper()
    name = entry.get("name", "")
    ifaces = {
        DEVICE: {
            "Address": GLib.Variant("s", address),
            "Name": GLib.Variant("s", name),
            "Alias": GLib.Variant("s", name),
            "Icon": GLib.Variant("s", entry.get("icon", "audio-headset")),
            "Paired": GLib.Variant("b", True),
            "Bonded": GLib.Variant("b", True),
            "Trusted": GLib.Variant("b", True),
            "Connected": GLib.Variant("b", entry.get("connected") is True),
            "Adapter": GLib.Variant("o", ADAPTER),
        }
    }
    if entry.get("battery") is not None:
        ifaces[BATTERY] = {"Percentage": GLib.Variant("y", max(0, min(100, int(entry["battery"]))))}
    return path_of(address), ifaces


def read_devices(path):
    try:
        with open(path) as f:
            raw = json.load(f)
        return dict(listed(e) for e in raw if isinstance(e, dict) and isinstance(e.get("address"), str))
    except (OSError, ValueError, TypeError):
        return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--address", required=True, help="the bus to own org.bluez on")
    mode = ap.add_mutually_exclusive_group(required=True)
    mode.add_argument("--count", help="scan: file the RSSI signal count is written to")
    mode.add_argument("--devices", help="file holding the JSON device list, read again on every change")
    ap.add_argument("--rate", type=int, default=700, help="scan: RSSI signals per minute")
    ap.add_argument("--ready", help="file written once org.bluez is owned")
    args = ap.parse_args()

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
                "Discovering": GLib.Variant("b", args.count is not None),
                "Discoverable": GLib.Variant("b", False),
                "Pairable": GLib.Variant("b", True),
            }
        },
    }
    if args.count:
        objects[path_of("AA:BB:CC:DD:EE:FF")] = {DEVICE: device("AA:BB:CC:DD:EE:FF", "Headphones", True)}
        for i in range(6):
            a = "11:22:33:44:55:%02X" % i
            objects[path_of(a)] = {DEVICE: device(a, "", False)}
    else:
        first = read_devices(args.devices)
        if first is None:
            sys.exit(f"bluez-stub: {args.devices} holds no device list")
        objects.update(first)

    def managed():
        return GLib.Variant(
            "(a{oa{sa{sv}}})",
            ({p: {i: dict(props) for i, props in ifaces.items()} for p, ifaces in objects.items()},),
        )

    node = Gio.DBusNodeInfo.new_for_xml(
        """<node><interface name="org.freedesktop.DBus.ObjectManager">
        <method name="GetManagedObjects"><arg type="a{oa{sa{sv}}}" direction="out"/></method>
        <signal name="InterfacesAdded"><arg type="o"/><arg type="a{sa{sv}}"/></signal>
        <signal name="InterfacesRemoved"><arg type="o"/><arg type="as"/></signal>
        </interface></node>"""
    )

    def on_call(conn, sender, path, iface, method, params, invocation):
        if method == "GetManagedObjects":
            invocation.return_value(managed())
        else:
            print(f"bluez-stub: unhandled {iface}.{method} on {path}", file=sys.stderr, flush=True)
            invocation.return_dbus_error("org.bluez.Error.NotSupported", "stub")

    bus.register_object("/", node.interfaces[0], on_call, None, None)

    def owned(*_):
        if args.ready:
            with open(args.ready, "w") as f:
                f.write("ready\n")

    Gio.bus_own_name_on_connection(bus, "org.bluez", Gio.BusNameOwnerFlags.NONE, owned, None)

    def added(path, ifaces):
        bus.emit_signal(
            None, "/", "org.freedesktop.DBus.ObjectManager", "InterfacesAdded", GLib.Variant("(oa{sa{sv}})", (path, ifaces))
        )

    def removed(path, names):
        bus.emit_signal(
            None, "/", "org.freedesktop.DBus.ObjectManager", "InterfacesRemoved", GLib.Variant("(oas)", (path, names))
        )

    def changed(path, iface, props):
        bus.emit_signal(
            None, path, "org.freedesktop.DBus.Properties", "PropertiesChanged", GLib.Variant("(sa{sv}as)", (iface, props, []))
        )

    if args.devices:

        def reread():
            fresh = read_devices(args.devices)
            if fresh is None:
                return True
            for path in [p for p in objects if p != ADAPTER and p not in fresh]:
                removed(path, list(objects.pop(path)))
            for path, ifaces in fresh.items():
                old = objects.get(path)
                objects[path] = ifaces
                if old is None:
                    added(path, ifaces)
                    continue
                gone = [i for i in old if i not in ifaces]
                if gone:
                    removed(path, gone)
                for iface, props in ifaces.items():
                    if iface not in old:
                        added(path, {iface: props})
                        continue
                    delta = {k: v for k, v in props.items() if k not in old[iface] or old[iface][k].unpack() != v.unpack()}
                    if delta:
                        changed(path, iface, delta)
            return True

        GLib.timeout_add(100, reread)
        GLib.MainLoop().run()
        return

    sent = [0]
    turn = [0]

    def rssi():
        paths = [p for p in objects if p != ADAPTER]
        p = paths[turn[0] % len(paths)]
        turn[0] += 1
        value = GLib.Variant("n", random.randint(-90, -40))
        objects[p][DEVICE]["RSSI"] = value
        changed(p, DEVICE, {"RSSI": value})
        sent[0] += 1
        with open(args.count, "w") as f:
            f.write(f"{sent[0]}\n")
        return True

    strangers = [6]

    def stranger():
        a = "11:22:33:44:55:%02X" % strangers[0]
        strangers[0] += 1
        props = device(a, "", False)
        objects[path_of(a)] = {DEVICE: props}
        added(path_of(a), {DEVICE: props})
        return True

    GLib.timeout_add(max(1, 60000 // args.rate), rssi)
    GLib.timeout_add_seconds(10, stranger)
    GLib.MainLoop().run()


if __name__ == "__main__":
    main()
