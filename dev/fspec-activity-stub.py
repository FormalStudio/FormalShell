#!/usr/bin/env python3
"""One fspec operation on its activity socket, for the --activity smoke leg.

Speaks FormalSpec's operation stream, version 1 (FormalSpec docs/activity.md):
listens on <dir>/<id>.sock, sends every connection a snapshot of the whole
operation and then every update to everyone connected, one JSON object per
line. A scenario is a list of steps; after sending step n the stub writes
<marks>.<n> and waits for <marks>.go<n> before the next, so the drive script
reads the shell's status at a known step.

  ok    an apply: preparing 1/4, committing 3/4, succeeded
  fail  an update: resolving 0/2, failed
  build  an apply whose one resource is a package build (FormalSpec's
        `step`): fetching dependencies with no measure, compiling 120/450
        then 300/450 units, packaging stalled, succeeded; the operation's
        own count stays 0/3 throughout, as fspec's does
  die   a rollback: planning with no progress, then a final line cut short
        and the socket gone, the way a killed fspec leaves a reader
  install  an install from the launcher (FSPEC_STUB_PKG, FSPEC_STUB_TITLE):
        resolving, preparing 40%, committing 1/2, succeeded
  installfail  the same failing at preparing 10%
  uninstall  resolving, committing 0/1, succeeded
  run   a run once from the launcher (FSPEC_STUB_PKG, FSPEC_STUB_TITLE):
        resolving 0/1, preparing 40%, succeeded
  strand  a run: resolving 0/1, resolving 1/1, preparing, succeeded, with
        the first connection held open and never written to again past its
        snapshot, the way fspec treats a reader whose snapshot it could not
        send whole
"""
import json
import os
import socket
import sys
import threading
import time


def op(op_id, kind, source, title, **fields):
    now = int(time.time() * 1000)
    base = {
        "id": op_id, "kind": kind, "source": source, "title": title, "host": None, "pid": os.getpid(),
        "started": now, "updated": now, "state": "running", "phase": "evaluating", "scope": None,
        "progress": None, "item": None, "keys": [], "result": None, "error": None,
    }
    base.update(fields)
    return base


def scenario(name, op_id):
    if name == "ok":
        o = lambda **f: op(op_id, "apply", "fspec:apply:smoke", "Applying smoke", host="smoke", **f)
        return [
            o(phase="preparing", scope="user", progress={"done": 1, "total": 4, "unit": "resources"},
              item="zypper: hello", keys=["pkg:hello"]),
            o(phase="committing", scope="user", progress={"done": 3, "total": 4, "unit": "resources"},
              item="file: ~/.config/smoke", keys=["file:~/.config/smoke"]),
            o(state="succeeded", phase="committing", progress={"done": 4, "total": 4, "unit": "resources"},
              result={"changes": 4, "summary": "4 changes"}),
        ]
    if name == "fail":
        o = lambda **f: op(op_id, "update", "fspec:update:/smoke/spec", "Updating fspec.lock", **f)
        return [
            o(phase="resolving", progress={"done": 0, "total": 2, "unit": "packages"}, item="hello"),
            o(state="failed", phase="resolving", progress={"done": 1, "total": 2, "unit": "packages"},
              error="resolve hello: no release on github"),
        ]
    if name == "runfail":
        o = lambda **f: op(op_id, "run", "fspec:run:smoke-pkg", "Running smoke-pkg", **f)
        return [
            o(phase="preparing", progress={"done": 40, "total": 100, "unit": "percent"}, item="smoke-pkg"),
            o(state="failed", phase="preparing", progress={"done": 40, "total": 100, "unit": "percent"},
              error="download smoke-pkg: connection reset"),
        ]
    if name == "long":
        o = lambda **f: op(op_id, "run", f"fspec:run:{op_id}",
                           f"Running a package whose name runs well past the bar {op_id}", **f)
        return [o(phase="resolving", progress={"done": 1, "total": 3, "unit": "packages"}, item=op_id), None]
    if name == "strand":
        o = lambda **f: op(op_id, "run", "fspec:run:smoke-strand", "Running smoke-strand", **f)
        return [
            o(phase="resolving", progress={"done": 0, "total": 1, "unit": "packages"}, item="smoke-strand"),
            o(phase="resolving", progress={"done": 1, "total": 1, "unit": "packages"}, item="smoke-strand"),
            o(phase="preparing"),
            o(state="succeeded", phase="preparing", result={"changes": 0, "summary": "started"}),
        ]
    if name == "run":
        pkg = os.environ.get("FSPEC_STUB_PKG", "flatpak:org.example.smoke")
        title = os.environ.get("FSPEC_STUB_TITLE", "smoke")
        o = lambda **f: op(op_id, "run", f"fspec:run:{pkg}", f"Running {title}", **f)
        return [
            o(phase="resolving", progress={"done": 0, "total": 1, "unit": "packages"}, item=title),
            o(phase="preparing", progress={"done": 40, "total": 100, "unit": "percent"}, item=title),
            o(state="succeeded", phase="preparing", result={"changes": 0, "summary": "started"}),
        ]
    if name in ("install", "installfail", "uninstall"):
        # The package and its title come from the shim that started the stub.
        pkg = os.environ.get("FSPEC_STUB_PKG", "flatpak:org.example.smoke")
        title = os.environ.get("FSPEC_STUB_TITLE", "smoke")
        verb = "uninstall" if name == "uninstall" else "install"
        words = "Uninstalling" if verb == "uninstall" else "Installing"
        o = lambda **f: op(op_id, verb, f"fspec:{verb}:{pkg}", f"{words} {title}", host="smoke",
                           keys=[f"pkg:{pkg.split(':', 1)[-1]}"], **f)
        if name == "uninstall":
            return [
                o(phase="resolving"),
                o(phase="committing", scope="system", progress={"done": 0, "total": 1, "unit": "resources"}, item=pkg),
                o(state="succeeded", phase="pushing", result={"changes": 1, "summary": "1 change"}),
            ]
        if name == "installfail":
            return [
                o(phase="preparing", scope="system", progress={"done": 10, "total": 100, "unit": "percent"}, item=title),
                o(state="failed", phase="preparing", progress={"done": 10, "total": 100, "unit": "percent"},
                  error="flatpak: remote flathub is not reachable"),
            ]
        return [
            o(phase="resolving", item="spec"),
            o(phase="preparing", scope="system", progress={"done": 40, "total": 100, "unit": "percent"}, item=title),
            o(phase="committing", scope="system", progress={"done": 1, "total": 2, "unit": "resources"}, item=pkg),
            o(state="succeeded", phase="pushing", result={"changes": 2, "summary": "2 changes"}),
        ]
    if name == "build":
        o = lambda **f: op(op_id, "apply", "fspec:apply:smoke-build", "Applying smoke-build", host="smoke-build",
                           phase="preparing", scope="system", progress={"done": 0, "total": 3, "unit": "resources"},
                           item="store: formalshell", keys=["pkg:formalshell"], **f)
        units = lambda done: {"done": done, "total": 450, "unit": "units"}
        return [
            o(step={"name": "fetching dependencies", "progress": None, "stalled": False}),
            o(step={"name": "compiling", "progress": units(120), "stalled": False}),
            o(step={"name": "compiling", "progress": units(300), "stalled": False}),
            o(step={"name": "packaging", "progress": None, "stalled": True}),
            dict(o(), state="succeeded", phase="committing", progress={"done": 3, "total": 3, "unit": "resources"},
                 result={"changes": 3, "summary": "3 changes"}),
        ]
    if name == "die":
        o = lambda **f: op(op_id, "rollback", "fspec:rollback:smoke", "Rolling back smoke", host="smoke", **f)
        return [o(phase="planning"), None]
    raise SystemExit(f"unknown scenario {name}")


def main():
    directory, op_id, name, marks = sys.argv[1:5]
    os.makedirs(directory, mode=0o700, exist_ok=True)
    path = os.path.join(directory, f"{op_id}.sock")
    if os.path.exists(path):
        os.unlink(path)
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    server.bind(path)
    server.listen(8)
    lock = threading.Lock()
    clients = []
    stranded = []
    current = {"op": None}

    def line(kind, o):
        return (json.dumps({"v": 1, "type": kind, "op": o}) + "\n").encode()

    def accept():
        while True:
            try:
                conn, _ = server.accept()
            except OSError:
                return
            with lock:
                if current["op"] is not None:
                    try:
                        conn.sendall(line("snapshot", current["op"]))
                    except OSError:
                        continue
                if name == "strand" and not stranded:
                    stranded.append(conn)
                    continue
                clients.append(conn)

    def broadcast(data):
        with lock:
            for conn in list(clients):
                try:
                    conn.sendall(data)
                except OSError:
                    clients.remove(conn)

    steps = scenario(name, op_id)
    with lock:
        current["op"] = steps[0]
    threading.Thread(target=accept, daemon=True).start()
    for n, step in enumerate(steps, start=1):
        if n > 1:
            while not os.path.exists(f"{marks}.go{n - 1}"):
                time.sleep(0.05)
        if step is None:
            # A final line the writer never finished, then no socket to go back to.
            cut = line("update", dict(current["op"], state="succeeded"))[:40]
            broadcast(cut)
            break
        step["updated"] = int(time.time() * 1000)
        with lock:
            current["op"] = step
        broadcast(line("snapshot" if n == 1 else "update", step))
        with open(f"{marks}.{n}", "w") as f:
            f.write("sent\n")
    os.unlink(path)
    server.close()
    with lock:
        for conn in clients:
            conn.close()
    # A stranded reader is never told the operation ended.
    time.sleep(10 if stranded else 0)
    if steps[-1] is None:
        with open(f"{marks}.{len(steps)}", "w") as f:
            f.write("cut\n")


if __name__ == "__main__":
    main()
