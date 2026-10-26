# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --screensaver-inhibit drives the shell's org.freedesktop.ScreenSaver
# (`services/inhibit.rs`) the way Steam and SDL do, with a 5s screensaver
# timeout and caffeinate's own toggle left off throughout. A Gio client
# that stays on the bus takes a cookie before the first timeout: the
# inhibitor goes up, `caffeinate status` lists it as the holder, and the
# session sits three timeouts past without going idle. Killing the client
# releases its cookie off NameOwnerChanged alone, and the same timeout then
# fires the screensaver, which `GetActive` reports. A one-shot `busctl call`
# on `/ScreenSaver` loses its cookie the same way the moment it exits, and a
# second client's `UnInhibit` releases its cookie while it is still on the
# bus.
leg_screensaver_inhibit_flag="--screensaver-inhibit"
leg_screensaver_inhibit_order=233
leg_screensaver_inhibit_needs="jq python3"

si_client="$shot_dir/si-client.py"
si_hold_cookie="$shot_dir/si-hold-cookie.txt"
si_hold_pid="$shot_dir/si-hold.pid"
si_cycle_cookie="$shot_dir/si-cycle-cookie.txt"
si_cycle_pid="$shot_dir/si-cycle.pid"
si_oneshot_path="$shot_dir/si-oneshot.txt"
si_on_path="$shot_dir/si-status-on.json"
si_held_path="$shot_dir/si-screensaver-held.json"
si_held_active_path="$shot_dir/si-getactive-held.txt"
si_off_path="$shot_dir/si-status-off.json"
si_idle_path="$shot_dir/si-screensaver-idle.json"
si_idle_active_path="$shot_dir/si-getactive-idle.txt"
si_cycle_on_path="$shot_dir/si-cycle-on.json"
si_cycle_off_path="$shot_dir/si-cycle-off.json"
si_cycle_alive_path="$shot_dir/si-cycle-alive.txt"

leg_screensaver_inhibit_fixture() {
  settings_fragment ', "screensaver": {"timeoutSeconds": 5}'
}

leg_screensaver_inhibit_timing() {
  leg_timing 36 60
}

leg_screensaver_inhibit_drive() {
  local script="$shot_dir/si-drive.sh"
  # `hold` keeps its cookie until killed; `cycle` hands its cookie back with
  # UnInhibit once the drive script drops the .go file, then stays on the
  # bus so only the call can have released it. Inhibit is retried until the
  # shell owns the name.
  write_script "$si_client" <<'EOF'
import os
import sys
import time

from gi.repository import Gio, GLib

mode, app, out = sys.argv[1:4]
bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)


def call(method, args, reply):
    return bus.call_sync(
        "org.freedesktop.ScreenSaver", "/org/freedesktop/ScreenSaver", "org.freedesktop.ScreenSaver",
        method, args, GLib.VariantType(reply), Gio.DBusCallFlags.NONE, 2000, None,
    )


deadline = time.time() + 30
while True:
    try:
        cookie = call("Inhibit", GLib.Variant("(ss)", (app, f"smoke {mode}")), "(u)").unpack()[0]
        break
    except GLib.Error:
        if time.time() > deadline:
            raise
        time.sleep(0.2)
with open(out, "w") as f:
    f.write(f"{cookie}\n")
if mode == "cycle":
    while not os.path.exists(out + ".go"):
        time.sleep(0.1)
    call("UnInhibit", GLib.Variant("(u)", (cookie,)), "()")
    with open(out + ".done", "w") as f:
        f.write("ok\n")
while True:
    time.sleep(60)
EOF
  write_script "$script" <<EOF
#!/usr/bin/env bash
"$python3_bin" "$si_client" hold smoke-holder "$si_hold_cookie" 2>> "$shot_dir/si-client.log" &
echo \$! > "$si_hold_pid"
for _ in \$(seq 50); do [ -s "$si_hold_cookie" ] && break; sleep 0.1; done
busctl --user call org.freedesktop.ScreenSaver /ScreenSaver org.freedesktop.ScreenSaver \
  Inhibit ss smoke-oneshot "smoke oneshot" > "$si_oneshot_path" 2>&1
sleep 3
$ipc call caffeinate status > "$si_on_path" 2>&1
# Three timeouts' worth of no input at all.
sleep 12
$ipc call screensaver status > "$si_held_path" 2>&1
busctl --user call org.freedesktop.ScreenSaver /org/freedesktop/ScreenSaver org.freedesktop.ScreenSaver \
  GetActive > "$si_held_active_path" 2>&1
kill "\$(cat "$si_hold_pid")"
sleep 1
$ipc call caffeinate status > "$si_off_path" 2>&1
sleep 8
$ipc call screensaver status > "$si_idle_path" 2>&1
busctl --user call org.freedesktop.ScreenSaver /org/freedesktop/ScreenSaver org.freedesktop.ScreenSaver \
  GetActive > "$si_idle_active_path" 2>&1
$ipc call screensaver stop > /dev/null 2>&1
"$python3_bin" "$si_client" cycle smoke-cycle "$si_cycle_cookie" 2>> "$shot_dir/si-client.log" &
echo \$! > "$si_cycle_pid"
for _ in \$(seq 50); do [ -s "$si_cycle_cookie" ] && break; sleep 0.1; done
sleep 0.5
$ipc call caffeinate status > "$si_cycle_on_path" 2>&1
touch "$si_cycle_cookie.go"
for _ in \$(seq 50); do [ -s "$si_cycle_cookie.done" ] && break; sleep 0.1; done
sleep 0.5
$ipc call caffeinate status > "$si_cycle_off_path" 2>&1
if kill -0 "\$(cat "$si_cycle_pid")" 2>/dev/null; then echo alive; else echo gone; fi > "$si_cycle_alive_path"
kill "\$(cat "$si_cycle_pid")"
EOF
  add_cleanup "kill \$(cat '$si_hold_pid' '$si_cycle_pid' 2>/dev/null) 2>/dev/null || true"
  hypr_exec_once "bash $script"
}

leg_screensaver_inhibit_assert() {
  local f
  for f in "$si_on_path" "$si_held_path" "$si_held_active_path" "$si_off_path" "$si_idle_path" \
    "$si_idle_active_path" "$si_cycle_on_path" "$si_cycle_off_path" "$si_cycle_alive_path" "$si_oneshot_path"; do
    [ -s "$f" ] || fail "screensaver-inhibit: $f missing or empty"
    echo "$(basename "$f"): $(cat "$f")"
  done
  [ -s "$si_hold_cookie" ] || fail "screensaver-inhibit: the holding client never got a cookie"
  grep -q '^u [1-9]' "$si_oneshot_path" || fail "busctl Inhibit on /ScreenSaver returned no cookie"
  jq -e '.active == false and .inhibiting == true and ([.holders[].app] == ["smoke-holder"])
    and (.holders[0].reason == "smoke hold") and (.holders[0].sender | startswith(":"))' "$si_on_path" > /dev/null \
    || fail "a live cookie did not hold the inhibitor with the client listed alone (the one-shot busctl's gone)"
  jq -e '.isIdle == false and .active == false and .caffeinated == false' "$si_held_path" > /dev/null \
    || fail "the session went idle past the screensaver timeout under a live cookie"
  grep -qx 'b false' "$si_held_active_path" || fail "GetActive did not answer false while held"
  jq -e '.inhibiting == false and .holders == []' "$si_off_path" > /dev/null \
    || fail "the killed client's cookie outlived it on the bus"
  jq -e '.isIdle == true and .active == true' "$si_idle_path" > /dev/null \
    || fail "the screensaver timeout did not fire once the holder left the bus"
  grep -qx 'b true' "$si_idle_active_path" || fail "GetActive did not answer true under the screensaver"
  jq -e '[.holders[].app] == ["smoke-cycle"]' "$si_cycle_on_path" > /dev/null \
    || fail "the second client's Inhibit did not list it"
  jq -e '.holders == []' "$si_cycle_off_path" > /dev/null || fail "UnInhibit did not release the cookie"
  grep -qx alive "$si_cycle_alive_path" || fail "the cycle client left the bus before UnInhibit was read"
}
