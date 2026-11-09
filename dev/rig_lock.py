#!/usr/bin/env python3
"""The rig's mac-side locks, written once.

    rig_lock.py slot <rig> <slots> <me> -- <cmd...>   one rig slot (g815 or mac)
    rig_lock.py vm <slots> -- <cmd...>                one VM rig guest
    rig_lock.py all <rig> <holder> -- <cmd...>        the one smoke-all per rig
    rig_lock.py file <path> -- <cmd...>               an exclusive flock of <path>
    rig_lock.py marker                                 dev/perf-gate.py's marker path

Each runs <cmd> holding the lock, with the slot in its environment
(FS_RIG_SLOT, FS_VM_SLOT), and exits with its status. When FS_SLOT_FILE
names a file, the slot taken goes into it as `<slot> <seconds waited>`, so a
caller reads the slot as a value; the variable is not passed on, so a nested
lock writes nowhere unless its caller asks.

The lock files are /tmp/formalshell-<rig>-<n>.lock (with a .last naming the
checkout that ran there last), /tmp/formalshell-fstestvm-<n>.lock and
/tmp/formalshell-smoke-all.lock, flocked exclusively. A run started by an
older copy of the scripts takes the same files the same way, so old and new
exclude each other.
"""
import fcntl
import os
import subprocess
import sys
import time

# dev/perf-hold.sh sources the same path from here.
PERF_MARKER = "/tmp/formalshell-perf-gate.running"


def lock_path(kind, n, rig=None):
    if kind == "vm":
        return "/tmp/formalshell-fstestvm-%d.lock" % n
    return "/tmp/formalshell-%s-%d.lock" % (rig, n)


def last_path(rig, n):
    return "/tmp/formalshell-%s-%d.last" % (rig, n)


def all_path(rig):
    return "/tmp/formalshell-smoke-all.lock" if rig == "g815" else "/tmp/formalshell-smoke-all-%s.lock" % rig


def try_lock(path):
    lock = open(path, "w")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError:
        lock.close()
        return None
    return lock


def record(slot, began):
    waited = int(time.time() - began)
    path = os.environ.pop("FS_SLOT_FILE", "")
    if path:
        with open(path, "w") as f:
            f.write("%s %d\n" % (slot, waited))
    return waited


def last(rig, n):
    try:
        return open(last_path(rig, n)).read().strip()
    except OSError:
        return ""


def take_rig_slot(rig, slots, me):
    """Free slots this checkout left warm first, then unused ones, then any."""
    began = time.time()
    while True:
        for n in sorted(range(slots), key=lambda n: (last(rig, n) != me, last(rig, n) != "", n)):
            lock = try_lock(lock_path("rig", n, rig))
            if lock is None:
                continue
            with open(last_path(rig, n), "w") as f:
                f.write(me + "\n")
            os.environ["FS_RIG_SLOT"] = str(n)
            waited = record(str(n), began)
            if waited:
                print("%s: waited %ds for a slot" % (rig, waited), file=sys.stderr)
            print("%s: slot %d" % (rig, n), file=sys.stderr)
            return lock
        time.sleep(3)


def take_vm_slot(slots):
    began = time.time()
    while True:
        for n in range(slots):
            lock = try_lock(lock_path("vm", n))
            if lock is None:
                continue
            os.environ["FS_VM_SLOT"] = str(n)
            waited = record("vm%d" % n, began)
            if waited:
                print("g815: waited %ds for a slot" % waited, file=sys.stderr)
            print("g815: slot vm%d" % n, file=sys.stderr)
            return lock
        time.sleep(3)


def take_all(rig, holder):
    lock = open(all_path(rig), "a+")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError:
        lock.seek(0)
        print("smoke-all: queued behind the run started by %s" % (lock.read().strip() or "another agent"), file=sys.stderr)
        fcntl.flock(lock, fcntl.LOCK_EX)
    lock.seek(0)
    lock.truncate()
    lock.write("%s (pid %d)\n" % (holder, os.getpid()))
    lock.flush()
    os.environ["FS_SMOKE_ALL_HELD"] = "1"
    return lock


def take_file(path):
    lock = open(path, "a+")
    fcntl.flock(lock, fcntl.LOCK_EX)
    return lock


def main(argv):
    if argv[:1] == ["marker"]:
        print(PERF_MARKER)
        return 0
    if "--" not in argv:
        print(__doc__, file=sys.stderr)
        return 2
    split = argv.index("--")
    head, cmd = argv[:split], argv[split + 1:]
    kind = head[0] if head else ""
    if kind == "slot" and len(head) == 4 and cmd:
        lock = take_rig_slot(head[1], int(head[2]), head[3])
    elif kind == "vm" and len(head) == 2 and cmd:
        lock = take_vm_slot(int(head[1]))
    elif kind == "all" and len(head) == 3 and cmd:
        lock = take_all(head[1], head[2])
    elif kind == "file" and len(head) == 2 and cmd:
        lock = take_file(head[1])
    else:
        print(__doc__, file=sys.stderr)
        return 2
    # The parent holds the lock for the child's life; the child gets no copy.
    status = subprocess.call(cmd)
    lock.close()
    return status


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
