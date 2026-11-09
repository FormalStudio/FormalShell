# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --clipssh: the menu's clipssh route driven the way a user drives it, over
# `menu activate` (the rig's Enter stand-in), against a PATH-shimmed clipssh
# speaking the real one's output contract: `box` takes six seconds and
# succeeds, `nohost` fails at once.
#
# What is under test is the shell's own path (row -> `@ipc:clipssh.send:` ->
# ClipsshService -> Process -> exit code -> toast plus the bar indicator that
# is up for exactly as long as the process runs), not whether the rig can
# reach an ssh host, so the shim stands in for the binary the same way the
# gpu leg's nvidia-smi does. It records every invocation, which is the one
# claim no frame can make: the alias the row carried reached the command
# line.
#
# Four frames off one timeline: the route's own rows, the transfer in flight
# (indicator cell up, SENDING toast beside it), the same transfer landed
# (COPIED, carrying the remote path the shim printed), and the failure card
# carrying clipssh's own `Error:` line. The two transfers are sequential on
# purpose: the service refuses a second while one is in flight, so an overlap
# would test the refusal rather than the failure path.
#
# The shim ends the way the real one does, wl-copying the remote path as
# text, and wl-copy leaves a daemon behind serving it with clipssh's stderr
# still open. So the leg also reads, off `bar room` and `debug dump`, that
# the indicator is up mid-flight and gone with the COPIED toast as soon as
# clipssh exits, not at the next copy, and off wl-paste that the clipboard
# then holds the path.
leg_clipssh_flag="--clipssh"
leg_clipssh_order=135
leg_clipssh_needs="jq wl-copy wl-paste"
leg_clipssh_claims="launcher=shared"

clipssh_shim_dir="$shot_dir/clipssh-shim"
clipssh_calls_path="$shot_dir/clipssh-calls.txt"
clipssh_summon_reply_path="$shot_dir/clipssh-summon-reply.txt"
clipssh_send_reply_path="$shot_dir/clipssh-send-reply.txt"
clipssh_fail_summon_reply_path="$shot_dir/clipssh-fail-summon-reply.txt"
clipssh_fail_reply_path="$shot_dir/clipssh-fail-reply.txt"
clipssh_notify_status_path="$shot_dir/clipssh-notify-status.json"
clipssh_room_flight_path="$shot_dir/clipssh-room-flight.json"
clipssh_room_landed_path="$shot_dir/clipssh-room-landed.json"
clipssh_paste_path="$shot_dir/clipssh-paste.txt"
clipssh_dump_landed_path="$shot_dir/clipssh-dump-landed.json"
clipssh_dump_path="$shot_dir/clipssh-dump.json"
clipssh_remote_path="/tmp/clipboard-1755180000.png"
clipssh_route_png="$shot_dir/clipssh-route.png"
clipssh_sending_png="$shot_dir/clipssh-sending.png"
clipssh_copied_png="$shot_dir/clipssh-copied.png"
clipssh_failed_png="$shot_dir/clipssh-failed.png"

leg_clipssh_fixture() {
  # clipssh's own alias store, in the isolated HOME so the route's rows are
  # this run's two and not whatever the host has saved. `nohost` is the row
  # whose transfer fails; the shim keys its failure off that name.
  mkdir -p "$iso_home/.clipssh"
  printf '%s\n' 'box=test@10.255.255.7' 'nohost=test@10.255.255.1' > "$iso_home/.clipssh/aliases"

  mkdir -p "$clipssh_shim_dir"
  # clipssh's own output contract, read off its script (v1.0.0): "Uploaded:
  # <path>" on stdout at exit 0, "Error: <reason>" on stderr otherwise, both
  # ANSI-coloured. Six seconds over the success case so the in-flight state
  # is a thing a frame can catch.
  cat > "$clipssh_shim_dir/clipssh" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\${1:-}" >> "$clipssh_calls_path"
if [ "\${1:-}" = "nohost" ]; then
  printf '\033[0;31mError:\033[0m Failed to upload to test@10.255.255.1\n' >&2
  exit 1
fi
sleep 6
printf '%s' "$clipssh_remote_path" | wl-copy
printf '\033[0;32mUploaded: $clipssh_remote_path\033[0m\n'
printf 'Path copied to clipboard - paste it directly\n'
EOF
  chmod +x "$clipssh_shim_dir/clipssh"
  # The scaffold owns the shell's launch line, so a shim reaches the shell by
  # riding the rig's own environment into the session: PATH is not in
  # session_env, so this is what Hyprland and everything it spawns inherit.
  # Safe to widen that far because the shim answers to one name and every
  # binary the scaffold itself resolved is already an absolute path. The
  # route is gated on `command -v clipssh` (default-menu.jsonc), so this is
  # also what makes it exist at all.
  export PATH="$clipssh_shim_dir:$(dirname "$wl_copy_bin"):$PATH"
}

leg_clipssh_timing() {
  # The frame script's last grim lands at 26s; this run's own smoke.png comes
  # after all four, on a session with nothing in flight and the indicator
  # gone again.
  leg_timing 34 75
}

leg_clipssh_drive() {
  local script="$shot_dir/clipssh-drive.sh" frames="$shot_dir/clipssh-frames.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 3
$ipc call menu summon clipssh > "$clipssh_summon_reply_path" 2>&1
sleep 3
$ipc call menu activate 0 > "$clipssh_send_reply_path" 2>&1
sleep 3
$ipc call bar room > "$clipssh_room_flight_path" 2>&1
sleep 6
$ipc call bar room > "$clipssh_room_landed_path" 2>&1
"$wl_paste_bin" --no-newline > "$clipssh_paste_path" 2>&1
$ipc call debug dump > "$clipssh_dump_landed_path" 2>&1
sleep 2
$ipc call menu summon clipssh > "$clipssh_fail_summon_reply_path" 2>&1
sleep 2
$ipc call menu activate 1 > "$clipssh_fail_reply_path" 2>&1
sleep 4
$ipc call notifications status > "$clipssh_notify_status_path" 2>&1
$ipc call debug dump > "$clipssh_dump_path" 2>&1
EOF
  # Frames on their own clock rather than interleaved with the calls above:
  # each `ipc call` spawn costs about a second on llvmpipe, and both toast
  # windows the middle two frames aim at are only six seconds wide.
  write_script "$frames" <<EOF
#!/usr/bin/env bash
sleep 6
"$grim_bin" "$clipssh_route_png" > /dev/null 2>&1
sleep 5
"$grim_bin" "$clipssh_sending_png" > /dev/null 2>&1
sleep 5
"$grim_bin" "$clipssh_copied_png" > /dev/null 2>&1
sleep 10
"$grim_bin" "$clipssh_failed_png" > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
  hypr_exec_once "bash $frames"
}

leg_clipssh_assert() {
  local f
  if ! grep -q '^ok$' "$clipssh_summon_reply_path" 2>/dev/null \
    || ! grep -q '^ok$' "$clipssh_fail_summon_reply_path" 2>/dev/null; then
    fail "menu summon clipssh did not answer ok, got: $(cat "$clipssh_summon_reply_path" "$clipssh_fail_summon_reply_path" 2>/dev/null)"
  fi
  if ! grep -q '^ok$' "$clipssh_send_reply_path" 2>/dev/null; then
    fail "menu activate on the box row did not answer ok, got: $(cat "$clipssh_send_reply_path" 2>/dev/null)"
  fi
  if ! grep -q '^ok$' "$clipssh_fail_reply_path" 2>/dev/null; then
    fail "menu activate on the nohost row did not answer ok, got: $(cat "$clipssh_fail_reply_path" 2>/dev/null)"
  fi
  # The claim no frame can make: both rows ran clipssh, with the alias the
  # row carried, in the order they were activated. A row that never reached
  # ClipsshService, or one that handed it the wrong alias, leaves this file
  # short or wrong while the toasts still look plausible.
  if [ ! -s "$clipssh_calls_path" ]; then
    fail "clipssh was never invoked: no row reached ClipsshService"
  fi
  cat "$clipssh_calls_path"
  if [ "$(cat "$clipssh_calls_path")" != "box
nohost" ]; then
    fail "clipssh was invoked with $(tr '\n' ' ' < "$clipssh_calls_path"), want box then nohost"
  fi
  local rail_flight rail_landed toasts
  rail_flight=$("$jq_bin" '[.[0].cells[] | select(.name == "indicators")] | length' "$clipssh_room_flight_path" 2>/dev/null)
  rail_landed=$("$jq_bin" '[.[0].cells[] | select(.name == "indicators")] | length' "$clipssh_room_landed_path" 2>/dev/null)
  echo "indicator cells: in flight ${rail_flight:-?}, landed ${rail_landed:-?}"
  if [ "${rail_flight:-0}" -lt 1 ]; then
    fail "no indicator cell on the strip while clipssh ran: $(cat "$clipssh_room_flight_path")"
  fi
  if [ "${rail_landed:-1}" -ne 0 ]; then
    fail "the indicator is still up after clipssh exited: $(cat "$clipssh_room_landed_path")"
  fi
  # What clipssh's own wl-copy left, read in the nested session: the remote
  # path as text, so a paste in a terminal on the host is the path.
  if [ "$(cat "$clipssh_paste_path" 2>/dev/null)" != "$clipssh_remote_path" ]; then
    fail "the clipboard after the send holds '$(cat "$clipssh_paste_path" 2>/dev/null)', want $clipssh_remote_path"
  fi
  # Word for word, urgency and no actions, the way the QML service raised
  # them. The COPIED one is read before anything else touches the clipboard.
  toasts='[.toasts[] | "\(.summary)|\(.body)|\(.urgency)|\(.actions | join(","))"]'
  "$jq_bin" -r "$toasts | .[]" "$clipssh_dump_landed_path" 2>/dev/null
  if ! "$jq_bin" -e --arg p "$clipssh_remote_path" "$toasts"' | any(. == "CLIPSSH SENDING|Clipboard image to box|1|") and any(. == "CLIPSSH COPIED|\($p) is on the clipboard|1|")' \
    "$clipssh_dump_landed_path" > /dev/null 2>&1; then
    fail "want the SENDING and COPIED toasts before the next copy: $(cat "$clipssh_dump_landed_path")"
  fi
  if ! "$jq_bin" -e "$toasts"' | any(. == "CLIPSSH FAILED|nohost: Failed to upload to test@10.255.255.1|2|")' \
    "$clipssh_dump_path" > /dev/null 2>&1; then
    fail "want the critical CLIPSSH FAILED toast carrying clipssh's own Error: line: $(cat "$clipssh_dump_path")"
  fi
  # The failure toast is urgency 2, so it is sticky: a popup still up four
  # seconds after the failing row is the FAILED card itself, while the two
  # normal-urgency ones have long expired into pending.
  if [ ! -s "$clipssh_notify_status_path" ]; then
    fail "no notifications status produced"
  fi
  cat "$clipssh_notify_status_path"; echo
  if grep -q '"popups":0' "$clipssh_notify_status_path"; then
    fail "no popup is up after the failed transfer, the urgent CLIPSSH FAILED toast never landed: $(cat "$clipssh_notify_status_path")"
  fi
  for f in "$clipssh_route_png" "$clipssh_sending_png" "$clipssh_copied_png" "$clipssh_failed_png"; do
    if [ ! -f "$f" ]; then
      fail "no clipssh screenshot produced at $f"
    fi
  done
  echo "SMOKE_CLIPSSH_ROUTE $clipssh_route_png"
  echo "SMOKE_CLIPSSH_SENDING $clipssh_sending_png"
  echo "SMOKE_CLIPSSH_COPIED $clipssh_copied_png"
  echo "SMOKE_CLIPSSH_FAILED $clipssh_failed_png"
}
