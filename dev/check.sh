#!/usr/bin/env bash
# One service crate's tests against its real daemon, on a Linux host
# (dev/remote.sh check <name> runs it on g815, or on the VM rig).
#
#   dev/check.sh fs-tray|fs-notifd|fs-mpris
#   dev/check.sh build <crate> <dir>     on the rig: a VM check's binaries into <dir>
#   FS_CHECK_BIN=<dir> dev/check.sh fs-auth|fs-audio|fs-bluez|fs-network|fs-upower
#
# The first three start their own dbus-daemon (and fs-notifd the real
# notify-send) on a private session bus; dbus-run-session keeps anything
# they miss off the host's. fs-auth, fs-audio, fs-bluez, fs-network and
# fs-upower drive real system daemons (PAM and polkit, PipeWire, BlueZ,
# NetworkManager against two hostapd radios, UPower and
# power-profiles-daemon), so they run in a VM rig guest (dev/vm.sh check),
# which has no toolchain: `build` compiles every test binary, ignored tests
# included, and the crate's probes on the rig, and the guest runs them.
set -euo pipefail
cd "$(dirname "$0")/../crates"

fail() { echo "$1" >&2; exit 1; }

vm_checks="fs-auth fs-audio fs-bluez fs-network fs-upower"

if [ "${1:-}" = build ]; then
  [ $# -eq 3 ] || fail "usage: $0 build <crate> <dir>"
  crate=$2 dest=$3
  case " $vm_checks " in *" $crate "*) ;; *) fail "$crate: not a VM check ($vm_checks)" ;; esac
  case "$dest" in /tmp/formalshell-rig-result-*) ;; *) fail "build: $dest is not /tmp/formalshell-rig-result-*" ;; esac
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

case "$1" in
  fs-tray|fs-notifd|fs-mpris)
    for tool in dbus-daemon dbus-run-session; do
      command -v "$tool" >/dev/null 2>&1 || fail "$1: $tool is not on PATH; install the openSUSE package dbus-1-daemon"
    done
    if [ "$1" = fs-notifd ] && ! command -v notify-send >/dev/null 2>&1; then
      fail "fs-notifd: notify-send is not on PATH; install the openSUSE package libnotify-tools"
    fi
    exec dbus-run-session -- fspec dev -- cargo test --locked --release --package "$1"
    ;;
  fs-auth|fs-audio|fs-bluez|fs-network|fs-upower)
    bin="${FS_CHECK_BIN:-}"
    [ -n "$bin" ] || fail "$1: drives real system daemons and runs on the VM rig: dev/vm.sh check $1 (dev/remote.sh check $1 hands it there)"
    skip=()
    ;;
  *)
    fail "$1: no such check (fs-tray, fs-notifd, fs-mpris; on the VM rig: $vm_checks)"
    ;;
esac

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
play.kill()
assert s["type"] == "AudioOutStream" and s["is_stream"], s
assert s["target"] == "fs_b", s
assert len(s["volumes"]) > 0, s
print("fs-audio: every step passed")
PY
    ;;
esac
