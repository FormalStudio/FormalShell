#!/usr/bin/env python3
"""Minimal StatusNotifierItem producer for FormalShell's --tray smoke fixture.

Registers a real org.kde.StatusNotifierItem object on the session bus and
announces it to whatever org.kde.StatusNotifierWatcher is running (the shell
owns that name itself), so the tray widget has genuine items to render,
never anything faked inside the shell.
The icon is a flat color square built at runtime from --color (no icon-theme
lookup: the VM ships no icon theme), in the wire format IconPixmap
requires: network/big-endian ARGB32 bytes per pixel.

--menu (M32) additionally exports a real com.canonical.dbusmenu tree at
/MenuBar and points the item's own "Menu" property (an object path) at it,
which is what marks the item as having a menu. GetLayout/AboutToShow/Event
are the only methods a client needs to show and activate it: it calls
AboutToShow then always GetLayout regardless of the reply, and Event only
to activate an entry. GetLayout always returns the full tree from whichever
id was asked, ignoring recursionDepth: a client reads the whole tree once
and never re-asks a submenu id over the wire, so a depth-aware trim here
would just be unexercised code.
"""
import argparse
import sys
import time

import gi

gi.require_version("GLib", "2.0")
gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib

ITEM_INTERFACE_XML_BASE = """<node>
  <interface name="org.kde.StatusNotifierItem">
    <property name="Category" type="s" access="read"/>
    <property name="Id" type="s" access="read"/>
    <property name="Title" type="s" access="read"/>
    <property name="Status" type="s" access="read"/>
    <property name="WindowId" type="u" access="read"/>
    <property name="IconThemePath" type="s" access="read"/>
    <property name="IconName" type="s" access="read"/>
    <property name="IconPixmap" type="a(iiay)" access="read"/>
    <property name="OverlayIconName" type="s" access="read"/>
    <property name="OverlayIconPixmap" type="a(iiay)" access="read"/>
    <property name="AttentionIconName" type="s" access="read"/>
    <property name="AttentionIconPixmap" type="a(iiay)" access="read"/>
    <property name="AttentionMovieName" type="s" access="read"/>
    <property name="ToolTip" type="(sa(iiay)ss)" access="read"/>
    {menu_property}
    <method name="ContextMenu">
      <arg type="i" direction="in" name="x"/>
      <arg type="i" direction="in" name="y"/>
    </method>
    <method name="Activate">
      <arg type="i" direction="in" name="x"/>
      <arg type="i" direction="in" name="y"/>
    </method>
    <method name="SecondaryActivate">
      <arg type="i" direction="in" name="x"/>
      <arg type="i" direction="in" name="y"/>
    </method>
    <method name="Scroll">
      <arg type="i" direction="in" name="delta"/>
      <arg type="s" direction="in" name="orientation"/>
    </method>
    <signal name="NewTitle"/>
    <signal name="NewIcon"/>
    <signal name="NewAttentionIcon"/>
    <signal name="NewOverlayIcon"/>
    <signal name="NewToolTip"/>
    <signal name="NewStatus">
      <arg type="s" name="status"/>
    </signal>
  </interface>
</node>"""

# com.canonical.dbusmenu, the wire interface a tray client speaks,
# reproduced here verbatim from the freedesktop dbusmenu wire spec. Only GetLayout/AboutToShow/Event are ever called by our client
# (see module docstring); the rest is declared so introspection and
# Properties.GetAll behave like a real implementation.
MENU_INTERFACE_XML = """<node>
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
    <method name="GetGroupProperties">
      <arg type="ai" direction="in" name="ids"/>
      <arg type="as" direction="in" name="propertyNames"/>
      <arg type="a(ia{sv})" direction="out" name="properties"/>
    </method>
    <method name="GetProperty">
      <arg type="i" direction="in" name="id"/>
      <arg type="s" direction="in" name="name"/>
      <arg type="v" direction="out" name="value"/>
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
    <method name="AboutToShowGroup">
      <arg type="ai" direction="in" name="ids"/>
      <arg type="ai" direction="out" name="updatesNeeded"/>
      <arg type="ai" direction="out" name="idErrors"/>
    </method>
    <signal name="ItemsPropertiesUpdated">
      <arg type="a(ia{sv})" name="updatedProps"/>
      <arg type="a(ias)" name="removedProps"/>
    </signal>
    <signal name="LayoutUpdated">
      <arg type="u" name="revision"/>
      <arg type="i" name="parent"/>
    </signal>
    <signal name="ItemActivationRequested">
      <arg type="i" name="id"/>
      <arg type="u" name="timestamp"/>
    </signal>
  </interface>
</node>"""

# A fixed fixture tree: a plain entry, a
# disabled one, a checked one, a separator, and a submenu with one child.
# Ids are the dbusmenu item ids GetLayout/Event address; 0 is the root and
# is never itself a visible row.
MENU_TREE = {
    0: {"props": {}, "children": [1, 2, 3, 4, 5]},
    1: {"props": {"label": "Plain Item"}, "children": []},
    2: {"props": {"label": "Disabled Item", "enabled": False}, "children": []},
    3: {
        "props": {"label": "Checked Item", "toggle-type": "checkmark", "toggle-state": 1},
        "children": [],
    },
    4: {"props": {"type": "separator"}, "children": []},
    5: {"props": {"label": "Submenu", "children-display": "submenu"}, "children": [6]},
    6: {"props": {"label": "Sub Item"}, "children": []},
}


def make_icon_pixmap(color_hex, size=22):
    r = int(color_hex[0:2], 16)
    g = int(color_hex[2:4], 16)
    b = int(color_hex[4:6], 16)
    pixel = bytes([0xFF, r, g, b])  # A,R,G,B big-endian per pixel
    return GLib.Variant("a(iiay)", [(size, size, pixel * (size * size))])


def _menu_props_variant(props):
    out = {}
    for key, value in props.items():
        if isinstance(value, bool):
            out[key] = GLib.Variant("b", value)
        elif isinstance(value, int):
            out[key] = GLib.Variant("i", value)
        else:
            out[key] = GLib.Variant("s", value)
    return out


def build_menu_layout(item_id, depth):
    node = MENU_TREE[item_id]
    children = []
    if depth != 0:
        next_depth = depth - 1 if depth > 0 else -1
        for child_id in node["children"]:
            # Raw child Variant, not pre-wrapped in GLib.Variant("v", ...):
            # the outer "(ia{sv}av)" constructor's own "av" field already
            # boxes each list element into a variant itself, so wrapping
            # here too double-boxes every child, and a client's deserialization
            # unwraps exactly once, so a double-boxed child comes out as a
            # default-constructed layout (id 0, empty everything) for every entry.
            children.append(build_menu_layout(child_id, next_depth))
    return GLib.Variant("(ia{sv}av)", (item_id, _menu_props_variant(node["props"]), children))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--id", required=True)
    parser.add_argument("--title", required=True)
    parser.add_argument("--color", required=True, help="icon fill color, RRGGBB")
    parser.add_argument(
        "--activate-file",
        help="append '<id>: <method>(x, y)' here on Activate/SecondaryActivate, and "
        "'<id> menu: Event(<item-id>, <eventId>)' on a DBusMenu Event call, so the "
        "smoke rig can assert the shell's activate/menu paths reached this item",
    )
    parser.add_argument(
        "--menu",
        action="store_true",
        help="export the fixture com.canonical.dbusmenu tree at /MenuBar and point "
        "the item's Menu property at it, marking the item as having a menu",
    )
    args = parser.parse_args()

    menu_property_xml = '<property name="Menu" type="o" access="read"/>' if args.menu else ""
    item_interface_xml = ITEM_INTERFACE_XML_BASE.format(menu_property=menu_property_xml)

    props = {
        "Category": GLib.Variant("s", "ApplicationStatus"),
        "Id": GLib.Variant("s", args.id),
        "Title": GLib.Variant("s", args.title),
        "Status": GLib.Variant("s", "Active"),
        "WindowId": GLib.Variant("u", 0),
        "IconThemePath": GLib.Variant("s", ""),
        "IconName": GLib.Variant("s", ""),
        "IconPixmap": make_icon_pixmap(args.color),
        "OverlayIconName": GLib.Variant("s", ""),
        "OverlayIconPixmap": GLib.Variant("a(iiay)", []),
        "AttentionIconName": GLib.Variant("s", ""),
        "AttentionIconPixmap": GLib.Variant("a(iiay)", []),
        "AttentionMovieName": GLib.Variant("s", ""),
        "ToolTip": GLib.Variant("(sa(iiay)ss)", ("", [], args.title, "")),
    }
    if args.menu:
        props["Menu"] = GLib.Variant("o", "/MenuBar")

    menu_props = {
        "Version": GLib.Variant("u", 3),
        "TextDirection": GLib.Variant("s", "ltr"),
        "Status": GLib.Variant("s", "normal"),
        "IconThemePath": GLib.Variant("as", []),
    }

    def handle_item_method_call(connection, sender, object_path, interface_name, method_name, parameters, invocation):
        if method_name in ("Activate", "SecondaryActivate", "ContextMenu"):
            x, y = parameters.unpack()
            print(f"{args.id}: {method_name}({x}, {y})", flush=True)
            if args.activate_file and method_name in ("Activate", "SecondaryActivate"):
                with open(args.activate_file, "a") as f:
                    f.write(f"{args.id}: {method_name}({x}, {y})\n")
            invocation.return_value(None)
        elif method_name == "Scroll":
            delta, orientation = parameters.unpack()
            print(f"{args.id}: Scroll({delta}, {orientation})", flush=True)
            invocation.return_value(None)
        else:
            invocation.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", method_name)

    def handle_item_get_property(connection, sender, object_path, interface_name, property_name):
        return props.get(property_name)

    def handle_menu_method_call(connection, sender, object_path, interface_name, method_name, parameters, invocation):
        if method_name == "GetLayout":
            parent_id, recursion_depth, _property_names = parameters.unpack()
            if parent_id not in MENU_TREE:
                invocation.return_dbus_error(
                    "org.freedesktop.DBus.Error.InvalidArgs", f"no menu item {parent_id}"
                )
                return
            layout = build_menu_layout(parent_id, recursion_depth)
            invocation.return_value(GLib.Variant.new_tuple(GLib.Variant("u", 1), layout))
        elif method_name == "AboutToShow":
            (item_id,) = parameters.unpack()
            invocation.return_value(GLib.Variant("(b)", (False,)))
        elif method_name == "Event":
            item_id, event_id, _data, _timestamp = parameters.unpack()
            print(f"{args.id} menu: Event({item_id}, {event_id})", flush=True)
            if args.activate_file:
                with open(args.activate_file, "a") as f:
                    f.write(f"{args.id} menu: Event({item_id}, {event_id})\n")
            invocation.return_value(None)
        else:
            invocation.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", method_name)

    def handle_menu_get_property(connection, sender, object_path, interface_name, property_name):
        return menu_props.get(property_name)

    node_info = Gio.DBusNodeInfo.new_for_xml(item_interface_xml)
    iface_info = node_info.lookup_interface("org.kde.StatusNotifierItem")

    connection = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    connection.register_object(
        "/StatusNotifierItem",
        iface_info,
        handle_item_method_call,
        handle_item_get_property,
        None,
    )

    if args.menu:
        menu_node_info = Gio.DBusNodeInfo.new_for_xml(MENU_INTERFACE_XML)
        menu_iface_info = menu_node_info.lookup_interface("com.canonical.dbusmenu")
        connection.register_object(
            "/MenuBar",
            menu_iface_info,
            handle_menu_method_call,
            handle_menu_get_property,
            None,
        )

    # The watcher (org.kde.StatusNotifierWatcher) only exists once the shell
    # has registered it, so retry briefly rather than
    # assume a fixed startup delay always wins that race.
    unique_name = connection.get_unique_name()
    last_error = None
    for _ in range(20):
        try:
            connection.call_sync(
                "org.kde.StatusNotifierWatcher",
                "/StatusNotifierWatcher",
                "org.kde.StatusNotifierWatcher",
                "RegisterStatusNotifierItem",
                GLib.Variant("(s)", (unique_name,)),
                None,
                Gio.DBusCallFlags.NONE,
                2000,
                None,
            )
            last_error = None
            break
        except GLib.Error as error:
            last_error = error
            time.sleep(0.5)

    if last_error is not None:
        print(f"{args.id}: failed to register with StatusNotifierWatcher: {last_error}", file=sys.stderr, flush=True)
        sys.exit(1)

    print(f"{args.id}: registered as {unique_name}", flush=True)
    GLib.MainLoop().run()


if __name__ == "__main__":
    main()
