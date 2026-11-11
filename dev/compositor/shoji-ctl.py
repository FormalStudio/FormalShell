#!/usr/bin/env python3
"""shoji-ctl.py [-j] <verb> [args]: $wmctl_bin under FS_COMPOSITOR=shoji.

Answers the part of hyprctl's command line the smoke legs use from
formalwm's socket (the 2026-11-11 shoji spec, section 3), in hyprctl's own
shapes, so a leg reads and dispatches the same way under either compositor:
`clients`, `activewindow`, `workspaces`, `activeworkspace`, `monitors`,
`layers`, `binds` and `cursorpos` (each with -j), `dispatch` of the
`hl.dsp.*` calls below, and `output create headless`, `output remove` and
the `hl.monitor` eval that sizes a made output. Anything else exits 3
naming what it was asked, so a leg leaning on Hyprland alone fails saying
so.
"""
import json
import os
import re
import socket
import sys
import time

HYPR_SPECIAL_ID = -99
# Hyprland's own size for a headless output before a monitor rule sizes it.
HEADLESS_DEFAULT = (1920, 1080)
LAYER_LEVELS = {"background": "0", "bottom": "1", "top": "2", "overlay": "3"}
MODMASK = {"SHIFT": 1, "CAPS": 2, "CTRL": 4, "ALT": 8, "MOD2": 16, "MOD3": 32, "SUPER": 64, "MOD5": 128}


class Unsupported(Exception):
    pass


def socket_path():
    runtime = os.environ.get("XDG_RUNTIME_DIR") or "/tmp"
    return f"{runtime}/shojiwm-{os.environ.get('WAYLAND_DISPLAY') or 'wayland-0'}.sock"


def call(method, params=None, answer=True):
    req = {"method": method}
    if answer:
        req["id"] = 1
    if params is not None:
        req["params"] = params
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
        s.settimeout(5)
        s.connect(socket_path())
        s.sendall((json.dumps(req) + "\n").encode())
        if not answer:
            return None
        for line in s.makefile():
            msg = json.loads(line)
            if msg.get("id") == 1:
                if "error" in msg:
                    sys.exit(f"shoji-ctl: {method}: {msg['error']}")
                return msg.get("result")
    sys.exit(f"shoji-ctl: {method}: the socket closed without an answer")


def outputs_by_name(state):
    return {o["name"]: n for n, o in enumerate(state["outputs"])}


def pid(n):
    """The engine reports 0 for a client whose pid it does not know, and a
    leg's `kill $(... .pid)` would be `kill 0`, the session's whole process
    group. null makes that kill fail on its own."""
    return n or None


def client(state, w):
    ws = next((x for x in state["workspaces"] if x["id"] == w["workspaceId"]), None)
    rect = w["rect"] or {"x": 0, "y": 0, "width": 0, "height": 0}
    if w["overlay"]:
        workspace = {"id": HYPR_SPECIAL_ID, "name": "special:" + w["overlay"]}
    else:
        workspace = {"id": ws["idx"] if ws else -1, "name": ws["name"] if ws else ""}
    order = [x["id"] for x in state["windows"] if x["focused"]] + [x["id"] for x in state["windows"] if not x["focused"]]
    return {
        "address": w["id"],
        "mapped": True,
        "hidden": bool(w["overlay"]) and not w["shown"],
        "at": [rect["x"], rect["y"]],
        "size": [rect["width"], rect["height"]],
        "workspace": workspace,
        "floating": w["floating"],
        "monitor": outputs_by_name(state).get(ws["output"] if ws else state["focusedOutput"], -1),
        "class": w["appId"],
        "title": w["title"],
        "initialClass": w["initialAppId"],
        "initialTitle": w["initialTitle"],
        "pid": pid(w["pid"]),
        "xwayland": False,
        "pinned": False,
        "fullscreen": 2 if w["fullscreen"] else 0,
        "fullscreenClient": 2 if w["fullscreen"] else 0,
        "grouped": [],
        "focusHistoryID": order.index(w["id"]),
    }


def workspace(state, ws):
    windows = [w for w in state["windows"] if w["workspaceId"] == ws["id"]]
    last = next((w for w in windows if w["focused"]), windows[0] if windows else None)
    return {
        "id": ws["idx"],
        "name": ws["name"],
        "monitor": ws["output"],
        "monitorID": outputs_by_name(state).get(ws["output"], -1),
        "windows": len(windows),
        "hasfullscreen": any(w["fullscreen"] for w in windows),
        "lastwindow": last["id"] if last else "0x0",
        "lastwindowtitle": last["title"] if last else "",
    }


def monitors(state):
    rows = {o["name"]: o for o in call("outputs.get")}
    out = []
    for n, o in enumerate(o for o in state["outputs"] if o["enabled"]):
        row = rows.get(o["name"], {})
        active = next((ws for ws in state["workspaces"] if ws["output"] == o["name"] and ws["active"]), None)
        out.append({
            "id": n,
            "name": o["name"],
            "make": row.get("make", ""),
            "model": row.get("model", ""),
            "width": row.get("width", round(o["width"] * o["scale"])),
            "height": row.get("height", round(o["height"] * o["scale"])),
            "refreshRate": row.get("refresh", 0),
            "x": o["x"],
            "y": o["y"],
            "scale": o["scale"],
            "transform": row.get("transform", 0),
            "focused": o["name"] == state["focusedOutput"],
            "activeWorkspace": {"id": active["idx"], "name": active["name"]} if active else {"id": -1, "name": ""},
            "disabled": False,
        })
    return out


def layers(state):
    out = {o["name"]: {"levels": {lvl: [] for lvl in LAYER_LEVELS.values()}} for o in state["outputs"] if o["enabled"]}
    for n, layer in enumerate(call("layers.list")):
        out.setdefault(layer["output"], {"levels": {lvl: [] for lvl in LAYER_LEVELS.values()}})
        out[layer["output"]]["levels"][LAYER_LEVELS[layer["layer"]]].append({
            "address": f"layer-{n}",
            "x": layer["x"],
            "y": layer["y"],
            "w": layer["width"],
            "h": layer["height"],
            "namespace": layer["namespace"],
            "pid": pid(layer["pid"]),
        })
    return out


def binds():
    return [{
        "locked": False,
        "mouse": b["key"].startswith("mouse"),
        "modmask": sum(MODMASK.get(m, 0) for m in b["mods"]),
        "submap": "",
        "key": b["key"],
        "keycode": 0,
        "catch_all": False,
        "description": b["title"],
        "dispatcher": b["action"],
        "arg": " ".join(b["args"]),
    } for b in call("binds.list")]


def read(verb, args, as_json):
    if verb == "getoption":
        if args != ["general:col.active_border"]:
            raise Unsupported(f"getoption {' '.join(args)}")
        return f"str: {call('state.get')['chrome']['activeBorder']}"
    if verb == "binds":
        rows = binds()
        if as_json:
            return rows
        return "".join(f"bind\n\tmodmask: {r['modmask']}\n\tkey: {r['key']}\n\tdescription: {r['description']}\n"
                       f"\tdispatcher: {r['dispatcher']}\n\targ: {r['arg']}\n\n" for r in rows)
    if verb == "cursorpos":
        p = call("cursor.pos")
        return p if as_json else f"{round(p['x'])}, {round(p['y'])}"
    state = call("state.get")
    if verb == "clients":
        return [client(state, w) for w in state["windows"]]
    if verb == "activewindow":
        w = next((w for w in state["windows"] if w["id"] == state["focusedWindowId"]), None)
        return client(state, w) if w else {}
    if verb == "workspaces":
        return [workspace(state, ws) for ws in state["workspaces"]]
    if verb == "activeworkspace":
        ws = next(ws for ws in state["workspaces"] if ws["id"] == state["focusedWorkspaceId"])
        return workspace(state, ws)
    if verb == "monitors":
        return monitors(state)
    if verb == "layers":
        return layers(state)
    raise Unsupported(verb)


# Dispatch: hl.dsp.<name>(<lua table or string>).

def lua_table(text):
    """The flat Lua tables the legs pass: `key = 'str' | "str" | number | bool`."""
    out = {}
    for key, value in re.findall(r"(\w+)\s*=\s*('[^']*'|\"[^\"]*\"|[-\d.]+|true|false)", text or ""):
        if value[0] in "'\"":
            out[key] = value[1:-1]
        elif value in ("true", "false"):
            out[key] = value == "true"
        else:
            out[key] = float(value) if "." in value else int(value)
    return out


def select(state, selector):
    """A window from hyprctl's `class:<regex>` or `address:<id>`, the focused
    one without a selector."""
    if not selector:
        w = next((w for w in state["windows"] if w["id"] == state["focusedWindowId"]), None)
    elif selector.startswith("address:"):
        w = next((w for w in state["windows"] if w["id"] == selector[len("address:"):]), None)
    elif selector.startswith("class:"):
        pattern = re.compile(selector[len("class:"):])
        w = next((w for w in state["windows"] if pattern.search(w["appId"])), None)
    else:
        raise Unsupported(f"window selector {selector}")
    if w is None:
        sys.exit(f"shoji-ctl: no window matches {selector or 'the focus'}")
    return w


def spawn(command, rules):
    before = {w["id"] for w in call("state.get")["windows"]}
    call("spawn", {"argv": ["sh", "-c", command]}, answer=False)
    target = rules.get("workspace")
    if target is None:
        return
    # hyprctl's `{ workspace = 'N silent' }`: the window that maps next goes
    # to slot N of the focused output, focus staying where it is.
    m = re.fullmatch(r"(\d+)( silent)?", str(target))
    if not m or not m.group(2):
        raise Unsupported(f"exec_cmd workspace rule {target}")
    end = time.monotonic() + 10
    while time.monotonic() < end:
        state = call("state.get")
        new = [w for w in state["windows"] if w["id"] not in before and not w["overlay"]]
        if new:
            call("window.moveTo", {"id": new[0]["id"], "idx": int(m.group(1)), "output": state["focusedOutput"]}, answer=False)
            return
        time.sleep(0.05)
    sys.exit(f"shoji-ctl: nothing mapped for {command!r} within 10s")


def dispatch(text):
    m = re.fullmatch(r"\s*hl\.dsp\.([\w.]+)\((.*)\)\s*", text, re.S)
    if not m:
        raise Unsupported(f"dispatch {text}")
    name, arg = m.group(1), m.group(2)
    if name == "exec_cmd":
        cm = re.fullmatch(r"\s*\[==\[(.*?)\]==\]\s*(?:,\s*(\{.*\}))?\s*", arg, re.S)
        if not cm:
            raise Unsupported(f"exec_cmd {arg}")
        return spawn(cm.group(1), lua_table(cm.group(2)))
    if name == "exit":
        return call("session.exit", answer=False)
    t = lua_table(arg)
    state = call("state.get")
    if name == "focus":
        if "workspace" in t:
            return call("workspace.focusAt", {"idx": int(t["workspace"])}, answer=False)
        return call("window.focus", {"id": select(state, t.get("window"))["id"]}, answer=False)
    w = select(state, t.get("window"))
    if name == "window.close":
        return call("window.close", {"id": w["id"]}, answer=False)
    if name == "window.fullscreen":
        action = t.get("action", "toggle")
        if action == "toggle" or (action == "set") != w["fullscreen"]:
            call("window.fullscreen", {"id": w["id"]}, answer=False)
        return None
    if name == "window.float":
        floating = {"toggle": None, "enable": True, "disable": False}[t.get("action", "toggle")]
        params = {"id": w["id"]} if floating is None else {"id": w["id"], "floating": floating}
        return call("window.float", params, answer=False)
    if name == "window.move" and "workspace" in t:
        if t.get("follow", True):
            raise Unsupported("window.move with follow")
        ws = next((x for x in state["workspaces"] if x["id"] == w["workspaceId"]), None)
        output = ws["output"] if ws else state["focusedOutput"]
        return call("window.moveTo", {"id": w["id"], "idx": int(t["workspace"]), "output": output}, answer=False)
    r = w["rect"]
    if name == "window.move":
        return call("window.place", {"id": w["id"], "x": t["x"], "y": t["y"], "width": r["width"], "height": r["height"]}, answer=False)
    if name == "window.resize":
        return call("window.place", {"id": w["id"], "x": r["x"], "y": r["y"], "width": t["x"], "height": t["y"]}, answer=False)
    raise Unsupported(f"hl.dsp.{name}")


def key(chord):
    """A chord pressed through the core's own input path, off the control
    socket shoji-run.sh names in SHOJIWM_SOCKET (~/RIG.md): binds, then the
    focused window, as a real keyboard."""
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
        s.connect(os.environ["SHOJIWM_SOCKET"])
        s.sendall(f"key {chord}\n".encode())
        answer = s.makefile().readline().strip()
    if answer != "ok":
        sys.exit(f"shoji-ctl: key {chord}: {answer or 'no answer'}")
    return answer


def made_path():
    return os.path.join(os.environ.get("XDG_RUNTIME_DIR") or "/tmp", "shoji-ctl-made-outputs")


def made():
    try:
        return open(made_path()).read().split()
    except FileNotFoundError:
        return []


def output(args):
    if args[:2] == ["create", "headless"] and len(args) <= 3:
        name = call("outputs.create", {"width": HEADLESS_DEFAULT[0], "height": HEADLESS_DEFAULT[1], **({"name": args[2]} if len(args) == 3 else {})})["name"]
        with open(made_path(), "a") as f:
            f.write(name + "\n")
        return name
    if args[:1] == ["remove"] and len(args) == 2:
        call("outputs.remove", {"name": args[1]})
        with open(made_path(), "w") as f:
            f.write("".join(n + "\n" for n in made() if n != args[1]))
        return "ok"
    raise Unsupported(f"output {' '.join(args)}")


def evaluate(text):
    """`hl.monitor({ output, mode, position, scale })` on an output this
    made: hyprctl sizes a headless output after making it, and
    `outputs.create` takes the size up front, so it is made again at that
    size. The engine places a made output right of the rest, which is the
    one position the legs ask for. `hl.config` asking for the scrolling
    layout, formalwm's only one, at the column width wm/example/settings.json
    opens columns at, changes nothing."""
    c = re.fullmatch(r"\s*hl\.config\((.*)\)\s*", text, re.S)
    if c and lua_table(c.group(1)) in ({"layout": "scrolling"}, {"layout": "scrolling", "column_width": 0.5}):
        return "ok"
    m = re.fullmatch(r"\s*hl\.monitor\((.*)\)\s*", text, re.S)
    t = lua_table(m.group(1)) if m else {}
    if not m or t.get("output") not in made():
        raise Unsupported(f"eval {text}")
    mode = re.fullmatch(r"(\d+)x(\d+)(@[\d.]+)?", t.get("mode", ""))
    if not mode:
        raise Unsupported(f"hl.monitor mode {t.get('mode')}")
    state = call("state.get")
    right = max((o["x"] + o["width"] for o in state["outputs"] if o["enabled"] and o["name"] != t["output"]), default=0)
    if t.get("position") not in (None, f"{right}x0"):
        raise Unsupported(f"hl.monitor position {t['position']} (a made output sits at {right}x0)")
    call("outputs.remove", {"name": t["output"]})
    call("outputs.create", {"name": t["output"], "width": int(mode.group(1)), "height": int(mode.group(2)), "scale": float(t.get("scale", 1))})
    return "ok"


def main(argv):
    as_json = "-j" in argv
    args = [a for a in argv if a != "-j"]
    if not args:
        raise Unsupported("no verb")
    if args[0] == "dispatch":
        dispatch(" ".join(args[1:]))
        print("ok")
        return
    if args[0] == "key" and len(args) == 2:
        print(key(args[1]))
        return
    if args[0] == "output":
        print(output(args[1:]))
        return
    if args[0] == "eval":
        print(evaluate(" ".join(args[1:])))
        return
    out = read(args[0], args[1:], as_json)
    print(json.dumps(out, indent=2) if not isinstance(out, str) else out)


if __name__ == "__main__":
    try:
        main(sys.argv[1:])
    except Unsupported as what:
        print(f"shoji-ctl: hyprctl {' '.join(sys.argv[1:])}: {what} has no formalwm equivalent", file=sys.stderr)
        sys.exit(3)
