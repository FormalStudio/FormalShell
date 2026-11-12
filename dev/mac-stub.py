#!/usr/bin/env python3
"""A stand-in for a Mac app under atoll, for the --mac-menu, --mac-keys and --dock legs.

One process, because the shell pairs a menu with a window by process id: it
maps an xdg_toplevel tagged `atoll` (xdg_toplevel_tag_v1, which is how the
shell knows a Mac window) and serves a com.canonical.dbusmenu at
/org/atoll/MainMenu, which it names to the shell's registrar
(com.canonical.AppMenu.Registrar on the bus name org.atoll.AppMenu.Registrar)
whenever that name gets an owner, as atoll does. Its dock menu is a second
dbusmenu at /org/atoll/DockMenu on the same connection, and its dock tile a
com.canonical.Unity.LauncherEntry `Update` for application://<app id>.desktop,
sent from that connection too, since a tile's marks go when its sender
leaves the bus. The tile changes when the leg says so: the stub owns
org.atoll.Stub and takes `Tile(a{sv})` at /org/atoll/Stub, the properties
going out as they came in, and --tile is the client of that, sending the
running stub its properties and leaving (a whole number an int64, true or
false a boolean, any other number a double, the rest a string):

    dev/mac-stub.py --tile count=3 count-visible=true progress=0.4 progress-visible=true

--log takes one line per thing that reached the stub:

    registered
    focus in | focus out
    key <keysym> <down|up> mods=<depressed xkb mask>
    menu: AboutToShow(<id>)
    menu: Event(<id>, <eventId>)
    dock: AboutToShow(<id>)
    dock: Event(<id>, <eventId>)
    tile: <the properties' names>
    quit

The menu is fixed (MENU): an application menu, File with a submenu and a
disabled item, Edit with a section heading and a checkmark that flips when
it is clicked (ItemsPropertiesUpdated), and Window, which is empty until its
first AboutToShow fills it (LayoutUpdated). Super+Q and a click on Quit both
end the process, as a Mac app's key equivalent and menu item do.

A key is named by the first keysym its keycode carries in the keymap the
compositor sent, read off the keymap's text: wtype types through a keymap
of its own, where a letter sits on whatever keycode it was given.

The window is drawn off wl_shm in one colour; the Wayland side is the wire
protocol by hand (wl.py), so this needs python3 with PyGObject and nothing
else.
"""
import argparse
import mmap
import os
import re
import socket
import struct
import sys

import gi

gi.require_version("GLib", "2.0")
gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib

from wl import Wire, string

REGISTRAR = "org.atoll.AppMenu.Registrar"
REGISTRAR_IFACE = "com.canonical.AppMenu.Registrar"
REGISTRAR_PATH = "/com/canonical/AppMenu/Registrar"
MENU_PATH = "/org/atoll/MainMenu"
DOCK_PATH = "/org/atoll/DockMenu"
CONTROL = "org.atoll.Stub"
CONTROL_PATH = "/org/atoll/Stub"
LAUNCHER_ENTRY = "com.canonical.Unity.LauncherEntry"

CONTROL_XML = """<node>
  <interface name="org.atoll.Stub">
    <method name="Tile">
      <arg type="a{sv}" direction="in" name="properties"/>
    </method>
  </interface>
</node>"""

MENU_XML = """<node>
  <interface name="com.canonical.dbusmenu">
    <property name="Version" type="u" access="read"/>
    <property name="TextDirection" type="s" access="read"/>
    <property name="Status" type="s" access="read"/>
    <property name="IconThemePath" type="as" access="read"/>
    <method name="GetLayout">
      <arg type="i" direction="in" name="parentId"/>
      <arg type="i" direction="in" name="recursionDepth"/>
      <arg type="as" direction="in" name="propertyNames"/>
      <arg type="u" direction="out" name="revision"/>
      <arg type="(ia{sv}av)" direction="out" name="layout"/>
    </method>
    <method name="Event">
      <arg type="i" direction="in" name="id"/>
      <arg type="s" direction="in" name="eventId"/>
      <arg type="v" direction="in" name="data"/>
      <arg type="u" direction="in" name="timestamp"/>
    </method>
    <method name="AboutToShow">
      <arg type="i" direction="in" name="id"/>
      <arg type="b" direction="out" name="needUpdate"/>
    </method>
    <signal name="ItemsPropertiesUpdated">
      <arg type="a(ia{sv})" name="updatedProps"/>
      <arg type="a(ias)" name="removedProps"/>
    </signal>
    <signal name="LayoutUpdated">
      <arg type="u" name="revision"/>
      <arg type="i" name="parent"/>
    </signal>
  </interface>
</node>"""

QUIT, RULER, WINDOW_MENU, WINDOW_ITEM = 15, 34, 4, 41
MOD_SUPER = 0x40


def keysyms(keymap):
    """Each evdev code's first keysym, off an xkb keymap's text."""
    codes = {name: int(code) for name, code in re.findall(r"<(\w+)>\s*=\s*(\d+)\s*;", keymap)}
    out = {}
    for name, sym in re.findall(r"key\s*<(\w+)>\s*\{(?:[^}\[]*symbols\s*\[[^\]]*\]\s*=)?[^}\[]*\[\s*([^,\]\s]+)", keymap):
        if name in codes:
            # wtype writes a keysym as its number.
            if sym.startswith("0x") and 0x20 < int(sym, 16) < 0x7F:
                sym = chr(int(sym, 16))
            out[codes[name] - 8] = sym
    return out


def item(label, **props):
    return {"props": {"label": label, **props}, "children": []}


def menu(label, children):
    return {"props": {"label": label, "children-display": "submenu"}, "children": children}


SEPARATOR = {"props": {"type": "separator"}, "children": []}


def tree(name):
    return {
        0: {"props": {"children-display": "submenu"}, "children": [1, 2, 3, WINDOW_MENU]},
        1: menu(name, [11, 12, 13, 14, QUIT]),
        11: item(f"About {name}"),
        12: SEPARATOR,
        13: item("Settings…", shortcut=[["Super", ","]]),
        14: SEPARATOR,
        QUIT: item(f"Quit {name}", shortcut=[["Super", "Q"]]),
        2: menu("File", [21, 22, 23, 24, 25]),
        21: item("New", shortcut=[["Super", "N"]]),
        22: item("Open…", shortcut=[["Super", "O"]]),
        23: SEPARATOR,
        24: menu("Export", [241, 242]),
        241: item("PDF"),
        242: item("HTML"),
        25: item("Save", enabled=False, shortcut=[["Super", "S"]]),
        3: menu("Edit", [31, 32, 33, RULER]),
        31: item("Copy", shortcut=[["Super", "C"]]),
        32: item("Paste", shortcut=[["Shift", "Super", "V"]]),
        33: {"props": {"label": "Format", "enabled": False, "x-atoll-section": True}, "children": []},
        RULER: item("Show Ruler", **{"toggle-type": "checkmark", "toggle-state": 0}),
        WINDOW_MENU: menu("Window", []),
    }


def dock_tree():
    return {
        0: {"props": {"children-display": "submenu"}, "children": [1, 2]},
        1: item("New Document"),
        2: item("Show All Windows"),
    }


def variant(value):
    if isinstance(value, bool):
        return GLib.Variant("b", value)
    if isinstance(value, int):
        return GLib.Variant("i", value)
    if isinstance(value, list):
        return GLib.Variant("aas", value)
    return GLib.Variant("s", value)


class Stub:
    def __init__(self, args):
        self.args = args
        self.menu = tree(args.name)
        self.menus = {MENU_PATH: self.menu, DOCK_PATH: dock_tree()}
        self.revision = 1
        self.loop = GLib.MainLoop()
        self.bus = None
        self.mods = 0
        self.size = (0, 0)
        self.drawn = None
        self.keyboard = None
        self.buffer = None
        self.fds = []
        self.syms = {}

    def log(self, line):
        print(f"{self.args.name}: {line}", flush=True)
        if self.args.log:
            with open(self.args.log, "a") as f:
                f.write(line + "\n")

    def quit(self):
        self.log("quit")
        self.loop.quit()

    # The menu.

    def layout(self, menu, item_id):
        node = menu[item_id]
        # Each child goes in raw: the "av" field boxes it itself.
        children = [self.layout(menu, c) for c in node["children"]]
        return GLib.Variant("(ia{sv}av)", (item_id, {k: variant(v) for k, v in node["props"].items()}, children))

    def emit(self, signal, params):
        self.bus.emit_signal(None, MENU_PATH, "com.canonical.dbusmenu", signal, params)

    def on_call(self, _conn, _sender, path, _iface, method, params, invocation):
        menu, main = self.menus[path], path == MENU_PATH
        said = "menu" if main else "dock"
        if method == "GetLayout":
            parent = params.unpack()[0]
            if parent not in menu:
                invocation.return_dbus_error("org.freedesktop.DBus.Error.InvalidArgs", f"no menu item {parent}")
                return
            invocation.return_value(GLib.Variant.new_tuple(GLib.Variant("u", self.revision), self.layout(menu, parent)))
        elif method == "AboutToShow":
            (item_id,) = params.unpack()
            self.log(f"{said}: AboutToShow({item_id})")
            fill = main and item_id == WINDOW_MENU and not self.menu[WINDOW_MENU]["children"]
            if fill:
                self.menu[WINDOW_ITEM] = item(f"{self.args.name} Document", **{"toggle-type": "checkmark", "toggle-state": 1})
                self.menu[WINDOW_MENU]["children"] = [WINDOW_ITEM]
                self.revision += 1
                self.emit("LayoutUpdated", GLib.Variant("(ui)", (self.revision, WINDOW_MENU)))
            invocation.return_value(GLib.Variant("(b)", (fill,)))
        elif method == "Event":
            item_id, event, _data, _time = params.unpack()
            self.log(f"{said}: Event({item_id}, {event})")
            invocation.return_value(None)
            if event != "clicked" or not main:
                return
            if item_id == RULER:
                props = self.menu[RULER]["props"]
                props["toggle-state"] = 1 - props["toggle-state"]
                update = [(RULER, {"toggle-state": variant(props["toggle-state"])})]
                self.emit("ItemsPropertiesUpdated", GLib.Variant("(a(ia{sv})a(ias))", (update, [])))
            elif item_id == QUIT:
                self.quit()
        else:
            invocation.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", method)

    def on_get(self, _conn, _sender, _path, _iface, name):
        return {
            "Version": GLib.Variant("u", 3),
            "TextDirection": GLib.Variant("s", "ltr"),
            "Status": GLib.Variant("s", "normal"),
            "IconThemePath": GLib.Variant("as", []),
        }.get(name)

    def on_control(self, _conn, _sender, _path, _iface, _method, params, invocation):
        (props,) = params.unpack()
        self.log(f"tile: {' '.join(sorted(props))}")
        uri = f"application://{self.args.app_id}.desktop"
        self.bus.emit_signal(
            None, "/com/canonical/unity/launcherentry/1", LAUNCHER_ENTRY, "Update",
            GLib.Variant.new_tuple(GLib.Variant("s", uri), params.get_child_value(0)),
        )
        invocation.return_value(None)

    def register(self, *_):
        self.bus.call(
            REGISTRAR, REGISTRAR_PATH, REGISTRAR_IFACE, "RegisterWindow",
            GLib.Variant("(uo)", (0, MENU_PATH)), None, Gio.DBusCallFlags.NONE, 2000, None, self.registered,
        )

    def registered(self, bus, result):
        try:
            bus.call_finish(result)
            self.log("registered")
        except GLib.Error as error:
            print(f"{self.args.name}: RegisterWindow failed: {error.message}", file=sys.stderr, flush=True)

    def serve(self):
        self.bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
        info = Gio.DBusNodeInfo.new_for_xml(MENU_XML).lookup_interface("com.canonical.dbusmenu")
        for path in self.menus:
            self.bus.register_object(path, info, self.on_call, self.on_get, None)
        control = Gio.DBusNodeInfo.new_for_xml(CONTROL_XML).lookup_interface(CONTROL)
        self.bus.register_object(CONTROL_PATH, control, self.on_control, None, None)
        Gio.bus_own_name_on_connection(self.bus, CONTROL, Gio.BusNameOwnerFlags.NONE, None, None)
        Gio.bus_watch_name_on_connection(self.bus, REGISTRAR, Gio.BusNameWatcherFlags.NONE, self.register, None)

    # The window.

    def bind(self, iface, version):
        name, offered = self.globals[iface]
        obj = self.w.new_id()
        self.w.send(self.registry, 0, struct.pack("<I", name) + string(iface) + struct.pack("<II", min(version, offered), obj))
        return obj

    def map(self):
        w = self.w = Wire("mac-stub")
        self.registry = w.new_id()
        w.send(1, 1, struct.pack("<I", self.registry))
        self.globals = {}

        def global_(obj, op, body):
            if obj == self.registry and op == 0:
                name, n = struct.unpack_from("<II", body)
                self.globals[body[8:8 + n - 1].decode()] = (name, struct.unpack_from("<I", body, 8 + n + (-n % 4))[0])

        w.roundtrip(global_)
        for need in ("wl_compositor", "wl_shm", "xdg_wm_base", "wl_seat", "xdg_toplevel_tag_manager_v1"):
            if need not in self.globals:
                sys.exit(f"mac-stub: the compositor has no {need}")
        compositor = self.bind("wl_compositor", 4)
        self.shm = self.bind("wl_shm", 1)
        self.wm_base = self.bind("xdg_wm_base", 1)
        self.seat = self.bind("wl_seat", 5)
        tags = self.bind("xdg_toplevel_tag_manager_v1", 1)
        self.surface = w.new_id()
        w.send(compositor, 0, struct.pack("<I", self.surface))
        self.xdg_surface = w.new_id()
        w.send(self.wm_base, 2, struct.pack("<II", self.xdg_surface, self.surface))
        self.toplevel = w.new_id()
        w.send(self.xdg_surface, 1, struct.pack("<I", self.toplevel))
        w.send(self.toplevel, 2, string(self.args.name))
        w.send(self.toplevel, 3, string(self.args.app_id))
        # Before the first commit, so the window is a Mac window from the
        # moment the compositor lists it.
        w.send(tags, 1, struct.pack("<I", self.toplevel) + string("atoll"))
        w.send(self.surface, 6)
        GLib.io_add_watch(w.sock.fileno(), GLib.IO_IN | GLib.IO_HUP, self.readable)

    def draw(self, serial):
        w = self.w
        w.send(self.xdg_surface, 4, struct.pack("<I", serial))
        width, height = (self.size[0] or 640, self.size[1] or 480)
        if self.drawn == (width, height):
            w.send(self.surface, 6)
            return
        stride, size = width * 4, width * height * 4
        fd = os.memfd_create("mac-stub")
        os.ftruncate(fd, size)
        with mmap.mmap(fd, size) as pixels:
            # XRGB8888, little-endian: blue first.
            pixels[:] = (bytes.fromhex(self.args.color)[::-1] + b"\xff") * (width * height)
        pool = w.new_id()
        w.send(self.shm, 0, struct.pack("<I", pool) + struct.pack("<i", size), fd=fd)
        os.close(fd)
        buffer = w.new_id()
        w.send(pool, 0, struct.pack("<IiiiiI", buffer, 0, width, height, stride, 1))
        w.send(pool, 1)
        w.send(self.surface, 1, struct.pack("<Iii", buffer, 0, 0))
        w.send(self.surface, 2, struct.pack("<iiii", 0, 0, width, height))
        w.send(self.surface, 6)
        if self.buffer is not None:
            w.send(self.buffer, 0)
        self.buffer, self.drawn = buffer, (width, height)

    def event(self, obj, op, body):
        w = self.w
        if obj == self.wm_base and op == 0:
            w.send(self.wm_base, 3, body[:4])
        elif obj == self.toplevel and op == 0:
            self.size = struct.unpack_from("<ii", body)
        elif obj == self.toplevel and op == 1:
            self.quit()
        elif obj == self.xdg_surface and op == 0:
            self.draw(struct.unpack_from("<I", body)[0])
        elif obj == self.seat and op == 0:
            # A keyboard may only be asked of a seat that has one.
            if struct.unpack_from("<I", body)[0] & 2 and self.keyboard is None:
                self.keyboard = w.new_id()
                w.send(self.seat, 1, struct.pack("<I", self.keyboard))
        elif obj == self.keyboard and op == 0:
            # The one event here that carries a descriptor.
            size = struct.unpack_from("<II", body)[1]
            fd = self.fds.pop(0)
            with mmap.mmap(fd, size, mmap.MAP_PRIVATE, mmap.PROT_READ) as text:
                self.syms = keysyms(text[:].rstrip(b"\0").decode(errors="replace"))
            os.close(fd)
        elif obj == self.keyboard and op == 1:
            self.log("focus in")
        elif obj == self.keyboard and op == 2:
            self.log("focus out")
        elif obj == self.keyboard and op == 3:
            _serial, _time, key, state = struct.unpack_from("<IIII", body)
            sym = self.syms.get(key, f"code:{key}")
            self.log(f"key {sym} {'down' if state else 'up'} mods={self.mods}")
            if state and sym.lower() == "q" and self.mods == MOD_SUPER:
                self.quit()
        elif obj == self.keyboard and op == 4:
            self.mods = struct.unpack_from("<II", body)[1]

    def readable(self, _fd, condition):
        w = self.w
        chunk, fds = b"", []
        if condition & GLib.IO_IN:
            chunk, fds, _flags, _addr = socket.recv_fds(w.sock, 65536, 8)
        self.fds += fds
        if not chunk:
            self.loop.quit()
            return False
        w.buf += chunk
        while len(w.buf) >= 8 and len(w.buf) >= struct.unpack_from("<I", w.buf, 4)[0] >> 16:
            self.event(*w.read())
        return True


def tile_value(text):
    if text in ("true", "false"):
        return GLib.Variant("b", text == "true")
    try:
        return GLib.Variant("x", int(text))
    except ValueError:
        pass
    try:
        return GLib.Variant("d", float(text))
    except ValueError:
        return GLib.Variant("s", text)


def send_tile(pairs):
    props = {name: tile_value(value) for name, _, value in (pair.partition("=") for pair in pairs)}
    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    bus.call_sync(CONTROL, CONTROL_PATH, CONTROL, "Tile", GLib.Variant("(a{sv})", (props,)), None, Gio.DBusCallFlags.NONE, 2000, None)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--name", default="Stub", help="the app's name: its window title and its application menu")
    parser.add_argument("--app-id", default="org.atoll.stub")
    parser.add_argument("--color", default="3a6ea5", help="the window's fill, RRGGBB")
    parser.add_argument("--log", help="append what reached the stub here, a line each")
    parser.add_argument("--tile", nargs="+", metavar="NAME=VALUE", help="send the running stub's dock tile these properties and leave")
    args = parser.parse_args()
    if args.tile:
        send_tile(args.tile)
        return
    stub = Stub(args)
    stub.serve()
    stub.map()
    stub.loop.run()


if __name__ == "__main__":
    main()
