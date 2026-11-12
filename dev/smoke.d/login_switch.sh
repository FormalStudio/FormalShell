# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --login-switch: another session picked on the boot's lock
# (docs/superpowers/specs/2026-10-30-locked-login.md), through the login
# script greetd runs (crates/formalshell-rs/src/formalshell-login.sh, run
# as it is). The script starts the session as `uwsm`, a stand-in that runs
# the rig's own; it writes the login marker, so the lock fades in from
# black over the same three fixture sessions as --login-lock. Its session,
# fixture and drive serve --login-stuck too.
#
# The drive Tabs onto the session chip, opens its menu, moves the pick
# off the running compositor's entry (to Hyprland under formalwm, to
# formalwm under Hyprland), photographs the open menu and closes it. A
# wrong password keeps the lock up and writes no session marker. The real
# one leaves the picked session's environment and command for the script,
# the card says "Starting <name>…" (`lock status`'s busy label, shown on
# every output) and the compositor ends after it. The script then starts
# that entry, whose Exec here is a stand-in recording what it was handed
# (the environment, the marker spent, the first compositor gone, the card's
# CRTC between the two) before running the rig's session again, in which the
# drive finds the shell unlocked and photographs it.
#
# A stand-in for gnome-keyring-daemon's control socket
# (`$XDG_RUNTIME_DIR/keyring/control`, fs_auth::keyring) answers the
# shell's unlock 2s late: the compositor is only asked to end once it has,
# so the first session's shell logs the keyring unlocked before it goes.
# --login-stuck's never takes the connection, its backlog full, so the
# shell's connect blocks for good.
#
# Under formalwm the shell runs with the slot's own runtime dir (the
# script's) in place of shoji-run.sh's, which reaches it through a link the
# lock refuses to write through. The compositor's Wayland and JSON sockets
# are linked into the slot's dir for it: the run's dir is named by
# shoji-run.sh's pid, the process the stand-ins exec, and a fresh run
# listens on wayland-1.
leg_login_switch_flag="--login-switch"
leg_login_switch_order=114
leg_login_switch_compositors="hyprland shoji"
leg_login_switch_needs="wtype python3"
leg_login_switch_claims="session"

login_switch_host_runtime="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
login_switch_runtime="$login_switch_host_runtime/formalshell"
login_switch_marker="$login_switch_runtime/login-session"
login_switch_bin="$shot_dir/login-bin"
login_switch_dest_sh="$shot_dir/login-switch-destination.sh"
login_switch_dest_txt="$shot_dir/login-switch-destination.txt"
login_switch_dest_json="$shot_dir/login-switch-destination.json"
login_switch_dest_png="$shot_dir/login-switch-destination.png"
login_switch_origin_pid="$shot_dir/login-switch-origin.pid"
login_switch_origin_log="$shot_dir/login-switch-origin-shell.log"
login_switch_card_png="$shot_dir/login-switch-card.png"
login_switch_menu_png="$shot_dir/login-switch-menu.png"
login_switch_wrong_png="$shot_dir/login-switch-wrong.png"
login_switch_leaving_png="$shot_dir/login-switch-leaving.png"
login_switch_ready_json="$shot_dir/login-switch-ready.json"
login_switch_menu_json="$shot_dir/login-switch-menu.json"
login_switch_wrong_json="$shot_dir/login-switch-wrong.json"
login_switch_wrong_marker="$shot_dir/login-switch-wrong-marker.txt"
login_switch_stuck_json="$shot_dir/login-switch-stuck.json"
login_switch_starting_json="$shot_dir/login-switch-starting.json"
login_switch_stuck_txt="$shot_dir/login-switch-stuck.txt"
login_switch_keyring_dir="$login_switch_host_runtime/keyring"
login_switch_keyring_txt="$shot_dir/login-switch-keyring.txt"
login_switch_keyring_pid=""
login_switch_links="wayland-1 shojiwm-wayland-1.sock"

# The pick the drive moves to, and the key that gets there in the menu's
# name order (formalwm, Hyprland, Plasma).
login_switch_target() {
  case $FS_COMPOSITOR in shoji) echo hyprland-uwsm ;; *) echo formalwm-uwsm ;; esac
}
# The picked entry's name, as the fixture writes it.
login_switch_target_name() {
  case $FS_COMPOSITOR in shoji) echo "Hyprland (uwsm-managed)" ;; *) echo "formalwm (uwsm-managed)" ;; esac
}
login_switch_key() {
  case $FS_COMPOSITOR in shoji) echo Down ;; *) echo Up ;; esac
}
# The compositor process itself, for `pgrep -f`: not the timeout or
# shoji-run.sh around it.
login_switch_wm_pattern() {
  case $FS_COMPOSITOR in
    shoji) echo "^$(realpath "$shoji_bin") --tty" ;;
    *) echo "^[^ ]*yprland --config $wm_config_path" ;;
  esac
}

# [hang]: the keyring stand-in's backlog full rather than answering.
login_switch_fixture() {
  mkdir -p "$login_switch_runtime"
  rm -f "$login_switch_marker" "$login_switch_dest_txt"
  login_lock_sessions
  sed -i "s|^Exec=.*|Exec=$login_switch_dest_sh|" "$shot_dir/wayland-sessions/$(login_switch_target).desktop"
  if [ "$FS_COMPOSITOR" = shoji ]; then
    shell_env="${shell_env:+$shell_env }$(login_switch_runtime_env)"
    for f in $login_switch_links; do
      if [ -e "$login_switch_host_runtime/$f" ] && [ ! -L "$login_switch_host_runtime/$f" ]; then
        echo "SMOKE_FAIL: $login_switch_host_runtime/$f is taken, another session runs on this slot" >&2
        exit 1
      fi
    done
  fi
  if [ -e "$login_switch_keyring_dir" ]; then
    echo "SMOKE_FAIL: $login_switch_keyring_dir is taken, a keyring daemon runs for this slot" >&2
    exit 1
  fi
  mkdir -m 700 "$login_switch_keyring_dir"
  python3 -I - "$login_switch_keyring_dir/control" "$rig_password" "$login_switch_keyring_txt" "${1:-answer}" > "$shot_dir/login-switch-keyring.log" 2>&1 <<'PY' &
import socket, struct, sys, time
path, password, out, mode = sys.argv[1], sys.argv[2].encode(), sys.argv[3], sys.argv[4]
server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
server.bind(path)
if mode == "hang":
    server.listen(0)
    fillers = []
    for _ in range(4):
        c = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        c.setblocking(False)
        try:
            c.connect(path)
        except OSError:
            pass
        fillers.append(c)
    while True:
        time.sleep(3600)
server.listen(4)
while True:
    conn, _ = server.accept()
    data = b""
    while len(data) < 5 or len(data) < 1 + struct.unpack(">I", data[1:5])[0]:
        chunk = conn.recv(4096)
        if not chunk:
            break
        data += chunk
    op, size = struct.unpack(">II", data[5:13])
    got = data[13:13 + size]
    time.sleep(2)
    with open(out, "a") as f:
        f.write(f"op={op} password={'match' if got == password else 'mismatch'}\n")
    conn.sendall(struct.pack(">II", 8, 0))
    conn.close()
PY
  login_switch_keyring_pid=$!
}
login_switch_runtime_env() {
  echo "XDG_RUNTIME_DIR=$login_switch_host_runtime"
}
leg_login_switch_fixture() { login_switch_fixture; }

login_switch_cleanup() {
  rm -f "$login_switch_runtime/login-lock" "$login_switch_marker"
  local f
  for f in $login_switch_links; do
    [ ! -L "$login_switch_host_runtime/$f" ] || rm -f "$login_switch_host_runtime/$f"
  done
  if [ -n "$login_switch_keyring_pid" ]; then
    kill "$login_switch_keyring_pid" 2> /dev/null || true
    rm -f "$login_switch_keyring_dir/control"
    rmdir "$login_switch_keyring_dir" 2> /dev/null || true
  fi
}
leg_login_switch_cleanup() { login_switch_cleanup; }

leg_login_switch_timing() { leg_timing 90; }

# The login script in place of the session: `uwsm` runs the rig's session,
# and the picked entry's stand-in runs it again once it has written down
# what it found.
login_switch_session() {
  local origin links="" f
  origin=$(printf '%q ' "${wm_cmd[@]}")
  if [ "$FS_COMPOSITOR" = shoji ]; then
    for f in $login_switch_links; do
      links+="ln -sfn $(printf %q "$login_switch_host_runtime")/shoji-run-\$\$/$f $(printf %q "$login_switch_host_runtime/$f")"$'\n'
    done
  fi
  mkdir -p "$login_switch_bin"
  write_script "$login_switch_bin/uwsm" <<EOF
#!/bin/sh
$links
exec $origin
EOF
  write_script "$login_switch_dest_sh" <<EOF
#!/usr/bin/env bash
card=""
for sysfs in /sys/class/drm/card[0-9]*; do
  case "\$(readlink -f "\$sysfs/device")" in */vkms-"\$(id -un)") card=/dev/dri/\${sysfs##*/} ;; esac
done
{
  echo "XDG_SESSION_TYPE=\${XDG_SESSION_TYPE:-}"
  echo "XDG_SESSION_DESKTOP=\${XDG_SESSION_DESKTOP:-}"
  echo "XDG_CURRENT_DESKTOP=\${XDG_CURRENT_DESKTOP:-}"
  if [ -e "$login_switch_marker" ]; then echo marker=left; else echo marker=spent; fi
  if kill -0 "\$(cat "$login_switch_origin_pid" 2>/dev/null)" 2>/dev/null; then echo origin=alive; else echo origin=gone; fi
  # What the card scans out with neither compositor up: each CRTC's
  # framebuffer, and the fbdev emulation the kernel restores on last close.
  if [ -n "\$card" ]; then
    echo "card=\$card fbdev=\$(cat /sys/class/graphics/fb0/name 2>/dev/null || echo none)"
    python3 -I - "\$card" <<'PY' 2>&1
import ctypes, os, sys
drm = ctypes.CDLL("libdrm.so.2")
u32, i32 = ctypes.c_uint32, ctypes.c_int
class Res(ctypes.Structure):
    _fields_ = [("count_fbs", i32), ("fbs", ctypes.POINTER(u32)), ("count_crtcs", i32), ("crtcs", ctypes.POINTER(u32))]
class Crtc(ctypes.Structure):
    _fields_ = [("crtc_id", u32), ("buffer_id", u32), ("x", u32), ("y", u32), ("width", u32), ("height", u32), ("mode_valid", i32)]
class Fb(ctypes.Structure):
    _fields_ = [("fb_id", u32), ("width", u32), ("height", u32), ("pitch", u32), ("bpp", u32), ("depth", u32), ("handle", u32)]
drm.drmModeGetResources.restype = ctypes.POINTER(Res)
drm.drmModeGetCrtc.restype = ctypes.POINTER(Crtc)
drm.drmModeGetFB.restype = ctypes.POINTER(Fb)
fd = os.open(sys.argv[1], os.O_RDWR | os.O_CLOEXEC)
res = drm.drmModeGetResources(fd).contents
for i in range(res.count_crtcs):
    c = drm.drmModeGetCrtc(fd, res.crtcs[i]).contents
    fb = drm.drmModeGetFB(fd, c.buffer_id) if c.buffer_id else None
    size = f" {fb.contents.width}x{fb.contents.height} depth {fb.contents.depth}" if fb else ""
    print(f"crtc={c.crtc_id} fb={c.buffer_id}{size} mode={'set' if c.mode_valid else 'off'}")
os.close(fd)
PY
  fi
} > "$login_switch_dest_txt.part"
# Under Hyprland the first session's frame script outlives it, finishing
# its teardown; the second one's would run beside it.
for _ in \$(seq 100); do
  pgrep -f -- "bash $shot_script" > /dev/null || break
  sleep 0.2
done
cp "$shell_log_path" "$login_switch_origin_log"
rm -f "$shot_dir/session-over"
mv "$login_switch_dest_txt.part" "$login_switch_dest_txt"
$links
exec $origin
EOF
  wm_cmd=(sh -c 'PATH=$0:$PATH exec sh "$1"' "$login_switch_bin" "$script_dir/../crates/formalshell-rs/src/formalshell-login.sh")
}
leg_login_switch_session() { login_switch_session; }

# <script> <stuck yes|no>
login_switch_drive() {
  local wrong="" enter
  if [ "$2" = yes ]; then
    enter=$(cat <<EOF
"$wtype_bin" -s 300 "$rig_password" -k Return
kill -STOP "\$origin"
until_jq 30 "$login_switch_stuck_json" '.authError == "Could not end the session"' call lock status
{
  if [ -e "$login_switch_marker" ]; then echo marker=left; else echo marker=gone; fi
  echo "origin=\$(ps -o stat= -p "\$origin")"
} > "$login_switch_stuck_txt"
kill -CONT "\$origin"
EOF
)
  else
    wrong=$(cat <<EOF
"$wtype_bin" -s 300 "wrong-$rig_password" -k Return
until_jq 15 "$login_switch_wrong_json" '.authError == "Wrong password"' call lock status
if [ -e "$login_switch_marker" ]; then echo left; else echo none; fi > "$login_switch_wrong_marker"
shoot "$login_switch_wrong_png"
EOF
)
    enter=$(cat <<EOF
"$wtype_bin" -s 300 "$rig_password" -k Return
until_jq 2 "$login_switch_starting_json" '.leavingFor != null and .startingShown == true' call lock status
shoot "$login_switch_leaving_png"
EOF
)
  fi
  write_script "$1" <<EOF
#!/usr/bin/env bash
if [ -e "$login_switch_dest_txt" ]; then
  until_jq 30 "$login_switch_dest_json" '.locked == false' call lock status
  shoot "$login_switch_dest_png"
  exit 0
fi
origin=\$(pgrep -u "\$(id -u)" -f -- "$(login_switch_wm_pattern)" | head -1)
echo "\$origin" > "$login_switch_origin_pid"
until_jq 20 "$login_switch_ready_json" '.secure == true and .sessions != null' call lock status
# The fade from black and the card's entrance.
sleep 3
shoot "$login_switch_card_png"
"$wtype_bin" -s 300 -k Tab -k Return
"$wtype_bin" -s 300 -k $(login_switch_key)
sleep 1
call lock status > "$login_switch_menu_json" 2>&1
shoot "$login_switch_menu_png"
"$wtype_bin" -s 300 -k Return
sleep 0.5
shoot "$shot_path"
$wrong
$enter
# The compositor ends on the Exit; under formalwm this script outlives it.
for _ in \$(seq 75); do
  kill -0 "\$origin" 2> /dev/null || break
  sleep 0.2
done
EOF
  wm_exec_once "bash $1"
}
leg_login_switch_drive() { login_switch_drive "$shot_dir/login-switch-drive.sh" no; }

# The origin shell's log: the destination's shell writes a log of its own
# over the run's.
login_switch_origin_shell_log() {
  if [ -s "$login_switch_origin_log" ]; then echo "$login_switch_origin_log"; else echo "$shell_log_path"; fi
}

login_switch_menu_assert() {
  local f
  for f in "$login_switch_card_png" "$login_switch_menu_png"; do
    [ -f "$f" ] || fail "no screenshot at $f"
  done
  echo "SMOKE_LOGIN_SWITCH_CARD $login_switch_card_png"
  echo "SMOKE_LOGIN_SWITCH_MENU $login_switch_menu_png"
  cat "$login_switch_menu_json"; echo
  expect_jq "$login_switch_menu_json" '.boot == true and .sessions.menu == true' "the session menu did not open on the boot lock" '{boot, sessions}'
  expect_jq "$login_switch_menu_json" ".sessions.running == \"$(login_lock_running)\" and .sessions.picked == \"$(login_switch_target)\"" "the pick did not move to $(login_switch_target)" '{sessions}'
}

# The keyring stand-in had the lock's password once, and the shell its
# answer before the compositor took it down.
login_switch_keyring_assert() {
  cat "$login_switch_keyring_txt" 2> /dev/null
  [ "$(cat "$login_switch_keyring_txt" 2> /dev/null)" = "op=1 password=match" ] || fail "the login keyring was not asked to unlock once with the password"
  grep -q '^lock: login keyring unlocked$' "$1" || fail "the shell went before the login keyring answered"
}

leg_login_switch_assert() {
  local target log want
  target=$(login_switch_target)
  login_switch_menu_assert

  expect_jq "$login_switch_wrong_json" ".locked == true and .leavingFor == null and .sessions.picked == \"$target\"" "a wrong password did more than refuse" '{locked, authError, leavingFor, sessions}'
  [ "$(cat "$login_switch_wrong_marker")" = none ] || fail "a wrong password left a session marker"
  [ -f "$login_switch_wrong_png" ] && echo "SMOKE_LOGIN_SWITCH_WRONG $login_switch_wrong_png"
  [ -f "$login_switch_leaving_png" ] && echo "SMOKE_LOGIN_SWITCH_LEAVING $login_switch_leaving_png"
  expect_jq "$login_switch_starting_json" ".leavingFor == \"$target\" and .busy == \"Starting $(login_switch_target_name)\u2026\" and .startingShown == true" "the card did not show Starting $(login_switch_target_name) while the handover waited" '{leavingFor, busy, startingShown}'

  log=$(login_switch_origin_shell_log)
  login_switch_keyring_assert "$log"
  grep -E '^lock: (leaving for|the compositor did not end)' "$log" || true
  grep -q "^lock: leaving for $target\$" "$log" || fail "the password did not hand over to $target"
  if grep -q '^lock: the compositor did not end' "$log"; then fail "the compositor outlived the handover"; fi

  [ -f "$login_switch_dest_txt" ] || fail "the login script did not start $target"
  echo "SMOKE_LOGIN_SWITCH_DESTINATION $login_switch_dest_txt"
  cat "$login_switch_dest_txt"
  case $target in
    formalwm-uwsm) want=$'XDG_SESSION_TYPE=wayland\nXDG_SESSION_DESKTOP=formalwm-uwsm\nXDG_CURRENT_DESKTOP=ShojiWM' ;;
    *) want=$'XDG_SESSION_TYPE=wayland\nXDG_SESSION_DESKTOP=hyprland-uwsm\nXDG_CURRENT_DESKTOP=Hyprland' ;;
  esac
  [ "$(head -3 "$login_switch_dest_txt")" = "$want" ] || fail "$target did not start with its own environment"
  grep -qx marker=spent "$login_switch_dest_txt" || fail "the session marker outlived the handover"
  grep -qx origin=gone "$login_switch_dest_txt" || fail "the first compositor was still up when $target started"
  [ ! -e "$login_switch_marker" ] || fail "a session marker is left after the run"

  expect_jq "$login_switch_dest_json" '.locked == false and .boot == false and .leavingFor == null' "$target did not come up unlocked" '{locked, boot, leavingFor}'
  [ -f "$login_switch_dest_png" ] && echo "SMOKE_LOGIN_SWITCH_DESTINATION_FRAME $login_switch_dest_png"
}
