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
  die   a rollback: planning with no progress, then a final line cut short
        and the socket gone, the way a killed fspec leaves a reader
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
    if steps[-1] is None:
        with open(f"{marks}.{len(steps)}", "w") as f:
            f.write("cut\n")


if __name__ == "__main__":
    main()
