#!/usr/bin/env bash
# One service crate's tests against its real daemon, on a Linux host
# (dev/remote.sh check <name> runs it on g815, or on the VM rig).
#
#   dev/check.sh <session check>         a crate in dev/rig.sh's rig_session_checks
#   dev/check.sh build <crate> <dir>     on the rig: a VM check's binaries into <dir>
#   FS_CHECK_BIN=<dir> dev/check.sh <VM check>   a crate in dev/rig.sh's rig_vm_checks
#
# The session checks start their own dbus-daemon (and fs-notifd the real
# notify-send) on a private session bus; dbus-run-session keeps anything
# they miss off the host's. The VM checks drive real system daemons (PAM and
# polkit, PipeWire, BlueZ, NetworkManager against two hostapd radios, UPower
# and power-profiles-daemon), so they run in a VM rig guest (dev/vm.sh check),
# which has no toolchain: `build` compiles every test binary, ignored tests
# included, and the crate's probes on the rig, and the guest runs them.
set -euo pipefail
. "$(dirname "$0")/rig.sh"
cd "$rig_root/crates"

fail() { echo "$1" >&2; exit 1; }

if [ "${1:-}" = build ]; then
  [ $# -eq 3 ] || fail "usage: $0 build <crate> <dir>"
  crate=$2 dest=$3
  rig_is_vm_check "$crate" || fail "$crate: not a VM check ($rig_vm_checks)"
  rig_result_ok "$dest" || fail "build: $dest is not $rig_result_prefix*"
  features=()
  [ "$crate" = fs-audio ] && features=(--features pipewire)
  rm -rf "$dest"
  mkdir -m 755 "$dest" "$dest/examples"
  fspec dev -- cargo test --locked --release --package "$crate" "${features[@]}" --no-run --message-format=json \
    | jq -r 'select(.profile.test == true and .executable != null) | .executable' \
    | while IFS= read -r exe; do
        name=$(basename "$exe")
        cp "$exe" "$dest/${name%-*}"
      done
  case "$crate" in
    fs-auth|fs-audio)
      fspec dev -- cargo build --locked --release --package "$crate" "${features[@]}" --examples
      for example in "$crate"/examples/*.rs; do
        cp "target/release/examples/$(basename "$example" .rs)" "$dest/examples/"
      done
      ;;
  esac
  chmod -R a+rX "$dest"
  ls "$dest" "$dest/examples"
  exit 0
fi

[ $# -eq 1 ] || fail "usage: $0 <crate>"

# Every test binary in $FS_CHECK_BIN, the ignored tests too, one thread:
# they share the one daemon.
run_tests() {
  local exe
  for exe in "$bin"/*; do
    [ -f "$exe" ] || continue
    echo "== $(basename "$exe")"
    "$@" "$exe" --include-ignored --test-threads=1 ${skip[@]+"${skip[@]}"}
  done
}

if rig_is_session_check "$1"; then
  for tool in dbus-daemon dbus-run-session; do
    command -v "$tool" >/dev/null 2>&1 || fail "$1: $tool is not on PATH; install the openSUSE package dbus-1-daemon"
  done
  if [ "$1" = fs-notifd ] && ! command -v notify-send >/dev/null 2>&1; then
    fail "fs-notifd: notify-send is not on PATH; install the openSUSE package libnotify-tools"
  fi
  exec dbus-run-session -- fspec dev -- cargo test --locked --release --package "$1"
fi
rig_is_vm_check "$1" || fail "$1: no such check ($rig_session_checks; on the VM rig: $rig_vm_checks)"
bin="${FS_CHECK_BIN:-}"
[ -n "$bin" ] || fail "$1: drives real system daemons and runs on the VM rig: dev/vm.sh check $1 (dev/remote.sh check $1 hands it there)"
skip=()

case "$1" in
  fs-upower)
    # One test reads a session bus; upowerd and power-profiles-daemon are
    # the system bus's, and switching a profile from an ssh login is not
    # authorized.
    run_tests sudo dbus-run-session --
    ;;
  fs-network)
    # NetworkManager lets an ssh login read but not connect.
    run_tests sudo
    ;;
  fs-bluez)
    # The two-controller test needs btvirt, which Tumbleweed's bluez does
    # not build; bluetoothd with no controller is the guest's own state.
    if ! command -v btvirt >/dev/null 2>&1; then
      echo "fs-bluez: no btvirt here, skipping two_virtual_controllers_discover_pair_and_forget"
      skip=(--skip two_virtual_controllers)
    fi
    run_tests sudo FS_BLUEZ_DBUS_DAEMON="$(command -v dbus-daemon)"
    ;;
  fs-auth)
    run_tests env
    # formalshell-lock is the PAM service the lock and greeter use; the
    # password is the rig's.
    user=$(id -un)
    echo "== pam_probe, wrong password"
    out=$(echo wrong-password | "$bin/examples/pam_probe" formalshell-lock "$user" 2>&1) && fail "pam_probe: a wrong password passed: $out"
    echo "$out"
    grep -q 'prompt echo=false' <<<"$out" || fail "pam_probe: no hidden password prompt"
    grep -q 'outcome Failed' <<<"$out" || fail "pam_probe: the wrong password did not fail"
    echo "== pam_probe, right password"
    out=$(echo formalshell-test | "$bin/examples/pam_probe" formalshell-lock "$user" 2>&1) || fail "pam_probe: the right password failed: $out"
    echo "$out"
    grep -q 'outcome Success' <<<"$out" || fail "pam_probe: no Success"
    # pkexec asks the agent registered for the caller's logind session,
    # which an ssh login is.
    echo "== polkit_probe"
    out=$(printf 'wrong-password\nformalshell-test\n' | "$bin/examples/polkit_probe" pkexec /usr/bin/true 2>&1) || fail "polkit_probe failed: $out"
    echo "$out"
    grep -q 'cancel rc=126' <<<"$out" || fail "polkit_probe: the cancelled request did not end in pkexec's 126"
    grep -q 'auth rc=0 failed=1' <<<"$out" || fail "polkit_probe: no wrong-then-right authentication"
    echo "== keyring prompter"
    ../dev/keyring-prompter.sh "$bin/examples/prompter_probe" || fail "keyring prompter failed"
    ;;
  fs-audio)
    run_tests env
    exec python3 - "$bin/examples/fs-audio-probe" <<'PY'
"""The probe against the guest's PipeWire: two null sinks, read back
through wpctl and pactl as well as the probe."""
import json, subprocess, sys, time

probe = sys.argv[1]

def sh(cmd):
    return subprocess.run(cmd, shell=True, check=True, capture_output=True, text=True).stdout

def until(cmd, timeout=15):
    end = time.time() + timeout
    while subprocess.run(cmd, shell=True, capture_output=True).returncode != 0:
        if time.time() > end:
            raise SystemExit("timed out: %s" % cmd)
        time.sleep(0.2)

def dump():
    return json.loads(sh(probe + " dump"))

def node(d, name):
    return next(n for n in d["nodes"] if n["name"] == name)

def wpctl_volume(node_id):
    out = sh("wpctl get-volume %d" % node_id).split()
    return float(out[1]), "[MUTED]" in out

def step(name):
    print("== " + name, flush=True)

step("enumerate")
sh("pactl load-module module-null-sink sink_name=fs_a sink_properties=device.description=SinkA")
sh("pactl load-module module-null-sink sink_name=fs_b sink_properties=device.description=SinkB")
until("wpctl status | grep -q SinkB")
d = dump()
a, b = node(d, "fs_a"), node(d, "fs_b")
for n, desc in ((a, "SinkA"), (b, "SinkB")):
    assert n["type"] == "AudioSink", n
    assert n["description"] == desc, n
    assert n["is_sink"] and not n["is_stream"] and n["ready"], n
    assert len(n["volumes"]) == len(n["channels"]) == 2, n
    assert abs(n["volume"] - wpctl_volume(n["id"])[0]) < 0.005, n

step("volume set and read back")
sh(probe + " set-volume fs_a 0.5")
vol, muted = wpctl_volume(a["id"])
assert abs(vol - 0.5) < 0.005 and not muted, (vol, muted)
assert abs(node(dump(), "fs_a")["volume"] - 0.5) < 0.005
sh("wpctl set-volume %d 0.3" % a["id"])
n = node(dump(), "fs_a")
assert abs(n["volume"] - 0.3) < 0.005, n
assert all(abs(v - 0.3) < 0.005 for v in n["volumes"]), n

step("mute")
sh(probe + " mute fs_a true")
assert wpctl_volume(a["id"])[1]
assert node(dump(), "fs_a")["muted"]
sh(probe + " mute fs_a false")
assert not wpctl_volume(a["id"])[1]
sh("wpctl set-mute %d 1" % a["id"])
assert node(dump(), "fs_a")["muted"]

step("default sink")
sh(probe + " set-default-sink fs_b")
assert sh("pactl get-default-sink").strip() == "fs_b"
d = dump()
assert d["defaults"]["sink"] == "fs_b" and d["defaults"]["configured_sink"] == "fs_b", d["defaults"]
assert d["default_sink"] == b["id"], d
sh("pactl set-default-sink fs_a")
d = dump()
assert d["defaults"]["sink"] == "fs_a" and d["default_sink"] == a["id"], d["defaults"]
sh(probe + " set-default-sink fs_b")
assert sh("pactl get-default-sink").strip() == "fs_b"

step("stream appears linked to the default sink")
import math, wave
with wave.open("/tmp/fs-audio-tone.wav", "wb") as w:
    w.setnchannels(2)
    w.setsampwidth(2)
    w.setframerate(48000)
    frame = lambda i: int(8000 * math.sin(2 * math.pi * 440 * i / 48000)).to_bytes(2, "little", signed=True) * 2
    w.writeframes(b"".join(frame(i) for i in range(48000 * 30)))
play = subprocess.Popen(["pw-play", "/tmp/fs-audio-tone.wav"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
s = json.loads(sh(probe + " wait-stream pw-play 15"))
assert s["type"] == "AudioOutStream" and s["is_stream"], s
assert s["target"] == "fs_b", s
assert len(s["volumes"]) > 0, s

def linked_into(stream_id):
    d = json.loads(sh("pw-dump"))
    names = {o["id"]: o["info"]["props"].get("node.name") for o in d
             if o["type"] == "PipeWire:Interface:Node" and o.get("info")}
    return {names.get(o["info"]["input-node-id"]) for o in d
            if o["type"] == "PipeWire:Interface:Link" and o.get("info")
            and o["info"]["output-node-id"] == stream_id}

step("move the stream")
m = json.loads(sh(probe + " move-stream pw-play fs_a"))
assert m["target"] == "fs_a", m
until("pw-link -l | grep -q 'fs_a:playback'")
assert linked_into(m["id"]) == {"fs_a"}, linked_into(m["id"])
meta = sh("pw-metadata -n default %d" % m["id"])
assert "key:'target.object' value:'%d' type:'Spa:Id'" % node(dump(), "fs_a")["serial"] in meta, meta

step("latency off the ports, as pw-dump reads it")
def dump_latency():
    d = json.loads(sh("pw-dump"))
    clock = {"clock.rate": 48000.0, "clock.quantum": 1024.0, "clock.force-rate": 0.0, "clock.force-quantum": 0.0}
    for o in d:
        if o["type"] == "PipeWire:Interface:Metadata" and o.get("props", {}).get("metadata.name") == "settings":
            for e in o.get("metadata") or []:
                if e.get("key") in clock and isinstance(e.get("value"), (int, float)) and e["value"] > 0:
                    clock[e["key"]] = float(e["value"])
    rate = clock["clock.force-rate"] or clock["clock.rate"]
    quantum = clock["clock.force-quantum"] or clock["clock.quantum"]
    out = {}
    for o in d:
        if o["type"] != "PipeWire:Interface:Port" or not o.get("info"):
            continue
        node_id = int(o["info"]["props"]["node.id"])
        for l in (o["info"].get("params") or {}).get("Latency") or []:
            if l.get("direction") != "Input":
                continue
            ms = (l.get("minQuantum", 0) * quantum / rate + l.get("minRate", 0) / rate) * 1000 + l.get("minNs", 0) / 1e6
            out[node_id] = max(out.get(node_id, ms), ms)
    return out
loop = subprocess.Popen(["pw-loopback", "-n", "fs_lat", "-d", "0.25", "-i",
                         "{ media.class=Audio/Sink node.name=fs_lat node.description=Latency }"],
                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
until("pw-cli ls Node | grep -q 'node.name = \"fs_lat\"'")
lat = node(dump(), "fs_lat")
sh("pw-cli set-param %d ProcessLatency '{ ns = 250000000 }'" % lat["id"])
m = json.loads(sh(probe + " move-stream pw-play fs_lat"))
assert m["target"] == "fs_lat", m
time.sleep(1)
want, d = dump_latency(), dump()
loop.kill()
play.kill()
for n in d["nodes"]:
    a, b = n["latency_ms"], want.get(n["id"])
    assert (a is None) == (b is None) and (a is None or abs(a - b) < 0.01), (n["name"], a, b)
# The loopback's 250ms on top of the stream's own quantum.
s = next(n for n in d["nodes"] if n["id"] == m["id"])
assert 250 <= s["latency_ms"] < 300, s
print("fs-audio: every step passed")
PY
    ;;
esac
