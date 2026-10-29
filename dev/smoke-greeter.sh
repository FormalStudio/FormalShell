#!/usr/bin/env bash
# greetd smoke (M8 Task 2): drives the real greetd instance's default_session
# (the formalshell-greeter compositor the VM rig runs) through a full
# login, typed via wtype into the greeter's own Wayland session rather than
# an IPC shortcut (the same "verify the action, not the input method" idiom
# dev/smoke.sh's --lock leg already uses). Prints
# the pre-auth (clock + input cell) and post-auth screenshots plus the
# session log, proving the real create_session/auth_message/start_session
# exchange happened rather than just "it renders".
#
# Unlike dev/smoke.sh's own ephemeral nested compositor (composed fresh
# every run by that script), greetd's default_session is a persistent system
# service declared once in the VM — driving it means finding the
# ALREADY-RUNNING greeter session rather than spawning a new one, and
# reaching from this script's own account into the separate `greeter`
# system account's Wayland session. Root (passwordless sudo, already wired
# for --lock's own wtype use) bypasses the greeter runtime dir's 0700 mode
# outright, the same way it bypasses any other user's file permissions, so
# grim/wtype run via `sudo env ...` here rather than as the `greeter` user
# itself.
#
# `sudo systemctl restart greetd` up front makes every run idempotent: once
# a login succeeds, greetd moves on to the authenticated user's own session
# and the default_session compositor this script talks to is gone — the
# restart forces a fresh greeter back up regardless of what a previous run
# (or a fresh boot) left running.
set -euo pipefail
cd "$(dirname "$0")/.."

runtime_dir=/run/formalshell-greeter
session_log=/tmp/formalshell-greeter-session.log
post_auth_src=/tmp/formalshell-greeter-post-auth.png

out_dir="artifacts/greeter"
mkdir -p "$out_dir"
pre_auth_png="$out_dir/greeter-pre-auth.png"
wrong_pw_png="$out_dir/greeter-wrong-pw.png"
post_auth_png="$out_dir/greeter-post-auth.png"
session_log_out="$out_dir/greeter-session.log"
journal_out="$out_dir/greetd-journal.txt"

# wtype connects to the compositor fresh on every invocation; the virtual
# keyboard's keymap upload needs one round trip to land before the first
# key event is safe to send, or that first character is silently dropped
# (reproduced directly: `wtype "12345"` into an empty field landed "2345").
# `-s <ms>` sleeps before interpreting the text/key that follows, giving
# that round trip time to complete — confirmed reliable up to a 16-char
# string across this same cross-user path.
wtype_settle_ms=300

# The login is this account's own: the rig's password is the same on every
# user it makes.
user=$(id -un)
desktop_png="$out_dir/greeter-desktop.png"

# A successful login leaves the authenticated user's greetd session on seat0,
# and whatever it runs keeps the display card until something ends it. The
# trap covers the failure exits too. greetd then brings the greeter back as
# its default_session, which is the one thing that should hold the card.
end_login_sessions() {
  local id
  for id in $(loginctl list-sessions --no-legend | awk -v u="$(id -un)" '$3 == u && $4 == "seat0" {print $1}'); do
    sudo loginctl terminate-session "$id" || true
  done
}
trap end_login_sessions EXIT

sudo rm -f "$post_auth_src" "$session_log"
sudo systemctl restart greetd

# The session script pins no fixed socket name: the compositor picks its
# own, so discover it the same way that script does.
wayland_display=""
for _ in $(seq 1 60); do
  wayland_display=$(sudo find "$runtime_dir" -maxdepth 1 -name 'wayland-*' ! -name '*.lock' -printf '%f' -quit 2>/dev/null)
  [ -n "$wayland_display" ] && break
  sleep 2
done
if [ -z "$wayland_display" ]; then
  echo "SMOKE_FAIL: greeter compositor socket never appeared under $runtime_dir" >&2
  sudo cat "$session_log" >&2 || true
  exit 1
fi
greeter_env=(sudo env "XDG_RUNTIME_DIR=$runtime_dir" "WAYLAND_DISPLAY=$wayland_display")

# formalshell-greeter needs a moment after connecting to map its surfaces
# and read greetd's first state — matches the fixed post-connect
# settle windows dev/smoke.sh's own legs use before their first shot.
sleep 3
"${greeter_env[@]}" grim "$pre_auth_png"

"${greeter_env[@]}" wtype -s "$wtype_settle_ms" "$user"
"${greeter_env[@]}" wtype -s "$wtype_settle_ms" -k Return
# The create_session -> auth_message round trip (real PAM conversation)
# measured comparably slow to --lock's own PAM round trip in
# dev/smoke.sh; 3s margin before typing the password avoids racing the
# prompt switching from "USER" to the password step.
sleep 3

# Wrong-password leg first (regression guard for the auth error race:
# greetd unconditionally follows
# every auth_error with a cancel_session it can no longer deliver, and the
# resulting "Connection refused" error must never clobber the real PAM
# failure text already showing). 3s covers the same PAM round trip plus the
# cancel_session cleanup that follows it before the input cell settles.
"${greeter_env[@]}" wtype -s "$wtype_settle_ms" "wrong-password"
"${greeter_env[@]}" wtype -s "$wtype_settle_ms" -k Return
sleep 3
"${greeter_env[@]}" grim "$wrong_pw_png"
sudo cp "$session_log" "$session_log_out" 2>/dev/null || true
sudo chmod 644 "$session_log_out" 2>/dev/null || true
# greetd's own wire reply, carrying PAM's verdict: never a greeter-side guess.
if ! grep -q '"error_type":"auth_error","description":"pam_authenticate: AUTH_ERR"' "$session_log_out"; then
  echo "SMOKE_FAIL: wrong-password leg never produced a real pam_authenticate AUTH_ERR, got:" >&2
  cat "$session_log_out" >&2
  exit 1
fi

# greetd resets to Inactive after auth_error; re-submit the username to
# start a fresh conversation for the real login below.
"${greeter_env[@]}" wtype -s "$wtype_settle_ms" "$user"
"${greeter_env[@]}" wtype -s "$wtype_settle_ms" -k Return
sleep 3
"${greeter_env[@]}" wtype -s "$wtype_settle_ms" "formalshell-test"
"${greeter_env[@]}" wtype -s "$wtype_settle_ms" -k Return

for _ in $(seq 1 30); do
  sudo test -f "$post_auth_src" && break
  sleep 2
done
if ! sudo test -f "$post_auth_src"; then
  echo "SMOKE_FAIL: no post-auth screenshot, the greeter never started the session; session log:" >&2
  sudo cat "$session_log" >&2 || true
  exit 1
fi
sudo cp "$post_auth_src" "$post_auth_png"
sudo chmod 644 "$post_auth_png"

sudo cp "$session_log" "$session_log_out" 2>/dev/null || true
sudo chmod 644 "$session_log_out" 2>/dev/null || true
sudo journalctl -u greetd --no-pager -n 300 > "$journal_out" 2>&1 || true

if ! grep -qF "greetd: authentication complete" "$session_log_out"; then
  echo "SMOKE_FAIL: session log has no authentication-complete line, got:" >&2
  cat "$session_log_out" >&2
  exit 1
fi
if ! grep -qF "greeter: session started" "$session_log_out"; then
  echo "SMOKE_FAIL: session log has no line saying the greeter exited after start_session, got:" >&2
  cat "$session_log_out" >&2
  exit 1
fi

# The session the login started: Hyprland under uwsm in this account's
# manager, its shell's bar mapped. That bar is what reaching the desktop
# means; the greeter's exit alone is not.
instance="" wl_socket=""
for _ in $(seq 1 45); do
  read -r instance wl_socket < <(hyprctl instances -j 2>/dev/null | jq -r '.[0] | "\(.instance) \(.wl_socket)"' 2>/dev/null) || true
  if [ -n "$instance" ] && [ "$instance" != null ] \
    && HYPRLAND_INSTANCE_SIGNATURE=$instance hyprctl -j layers 2>/dev/null | grep -q '"namespace": *"formalshell:bar"'; then
    break
  fi
  instance=""
  sleep 2
done
if [ -z "$instance" ]; then
  echo "SMOKE_FAIL: the login's desktop never mapped a formalshell:bar layer; Hyprland instances: $(hyprctl instances 2>&1)" >&2
  journalctl --user -b --no-pager -n 80 >&2 || true
  exit 1
fi
sleep 2
WAYLAND_DISPLAY=$wl_socket grim "$desktop_png"
echo "greeter: desktop up, Hyprland $instance on $wl_socket with the bar mapped"

echo "SMOKE_GREETER_PRE $pre_auth_png"
echo "SMOKE_GREETER_WRONGPW $wrong_pw_png"
echo "SMOKE_GREETER_POST $post_auth_png"
echo "SMOKE_GREETER_DESKTOP $desktop_png"
echo "SMOKE_GREETER_OK"
