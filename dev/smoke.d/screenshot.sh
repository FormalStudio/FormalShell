# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --screenshot drives the `screenshot` IPC target's two non-picker routes.
# First the region/cancel round trip: `screenshot region` starts a real
# themed slurp, which blocks on a drag no headless run can supply (the exact
# stuck state the cancel path exists for), proven by pgrep plus
# capturing:true; `screenshot cancel` then has to kill it and settle to
# capturing:false / lastCancelled:true / empty lastError with no file
# written. Then the full capture in the same session: `screenshot full`
# replies synchronously with the destination path while the grim/wl-copy
# pipeline it starts runs on, and wl-paste's own type list is what proves
# the copy half landed.
#
# No screenshot.directory in the settings fixture on purpose: the capture
# landing under the isolated HOME's Pictures/Screenshots is what proves the
# documented default resolves. Slurp's overlay look and a real drag stay
# host-trial: this rig has no synthetic pointer.
leg_screenshot_flag="--screenshot"
leg_screenshot_order=205
leg_screenshot_needs="wl-paste file"

screenshot_reply_path="$shot_dir/screenshot-reply.txt"
screenshot_status_path="$shot_dir/screenshot-status.json"
screenshot_types_path="$shot_dir/screenshot-types.txt"
screenshot_region_reply_path="$shot_dir/screenshot-region-reply.txt"
screenshot_region_status_path="$shot_dir/screenshot-region-status.json"
screenshot_cancel_reply_path="$shot_dir/screenshot-cancel-reply.txt"
screenshot_cancelled_status_path="$shot_dir/screenshot-cancelled-status.json"
screenshot_slurp_before_path="$shot_dir/screenshot-slurp-before.txt"
screenshot_slurp_after_path="$shot_dir/screenshot-slurp-after.txt"

leg_screenshot_timing() {
  leg_timing 45
}

leg_screenshot_drive() {
  local script="$shot_dir/screenshot-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
$ipc call screenshot region "" > "$screenshot_region_reply_path" 2>&1
sleep 1
pgrep -x slurp > "$screenshot_slurp_before_path" 2>&1
$ipc call screenshot status > "$screenshot_region_status_path" 2>&1
$ipc call screenshot cancel > "$screenshot_cancel_reply_path" 2>&1
sleep 1
pgrep -x slurp > "$screenshot_slurp_after_path" 2>&1 || true
$ipc call screenshot status > "$screenshot_cancelled_status_path" 2>&1
$ipc call screenshot full "" > "$screenshot_reply_path" 2>&1
for _ in \$(seq 1 20); do
  $ipc call screenshot status > "$screenshot_status_path" 2>&1
  if grep -q '"capturing":false' "$screenshot_status_path"; then
    break
  fi
  sleep 0.5
done
"$wl_paste_bin" --list-types > "$screenshot_types_path" 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_screenshot_assert() {
  local screenshot_region_file screenshot_file
  if [ ! -s "$screenshot_region_reply_path" ]; then
    fail "no screenshot region IPC reply produced"
  fi
  cat "$screenshot_region_reply_path"
  screenshot_region_file=$(head -n1 "$screenshot_region_reply_path" | tr -d '\r')
  case "$screenshot_region_file" in
    error*|"") fail "screenshot region replied with an error: $(cat "$screenshot_region_reply_path")" ;;
  esac
  if [ ! -s "$screenshot_slurp_before_path" ]; then
    fail "no slurp process found while the region capture was pending, slurp never launched"
  fi
  if ! grep -q '"capturing":true' "$screenshot_region_status_path"; then
    fail "status did not report capturing:true while slurp was pending: $(cat "$screenshot_region_status_path" 2>/dev/null)"
  fi
  if ! grep -q '^ok$' "$screenshot_cancel_reply_path"; then
    fail "screenshot cancel did not return ok. Got: $(cat "$screenshot_cancel_reply_path" 2>/dev/null)"
  fi
  if [ -s "$screenshot_slurp_after_path" ]; then
    fail "slurp still running after cancel (pids: $(cat "$screenshot_slurp_after_path"))"
  fi
  if [ ! -s "$screenshot_cancelled_status_path" ]; then
    fail "no post-cancel screenshot status produced"
  fi
  cat "$screenshot_cancelled_status_path"
  if ! grep -q '"capturing":false' "$screenshot_cancelled_status_path" \
    || ! grep -q '"lastCancelled":true' "$screenshot_cancelled_status_path" \
    || ! grep -q '"lastError":""' "$screenshot_cancelled_status_path"; then
    fail "post-cancel status did not settle to a clean cancel: $(cat "$screenshot_cancelled_status_path")"
  fi
  if [ -e "$screenshot_region_file" ]; then
    fail "cancelled region capture still wrote a file: $screenshot_region_file"
  fi
  echo "SMOKE_SCREENSHOT_CANCEL region pending to cancelled clean, slurp killed, no file at $screenshot_region_file"

  if [ ! -s "$screenshot_reply_path" ]; then
    fail "no screenshot IPC reply produced"
  fi
  cat "$screenshot_reply_path"
  screenshot_file=$(head -n1 "$screenshot_reply_path" | tr -d '\r')
  case "$screenshot_file" in
    error*|"") fail "screenshot full replied with an error: $(cat "$screenshot_reply_path")" ;;
  esac
  if [ ! -f "$screenshot_file" ]; then
    fail "screenshot reply path does not exist: $screenshot_file"
  fi
  if ! "$file_bin" "$screenshot_file" | grep -q "PNG image data"; then
    fail "screenshot file is not a valid PNG, file(1) says: $("$file_bin" -b "$screenshot_file")"
  fi
  echo "SMOKE_SCREENSHOT $screenshot_file ($("$file_bin" -b "$screenshot_file"))"
  if [ ! -s "$screenshot_status_path" ]; then
    fail "no screenshot status produced"
  fi
  cat "$screenshot_status_path"
  if ! grep -q '"capturing":false' "$screenshot_status_path" || ! grep -q '"lastError":""' "$screenshot_status_path"; then
    fail "screenshot status did not settle clean: $(cat "$screenshot_status_path")"
  fi
  if ! grep -qF "\"lastPath\":\"$screenshot_file\"" "$screenshot_status_path"; then
    fail "screenshot status lastPath does not match the reply path ($screenshot_file): $(cat "$screenshot_status_path")"
  fi
  if [ -s "$screenshot_types_path" ] && grep -q "image/png" "$screenshot_types_path"; then
    cat "$screenshot_types_path"
  else
    fail "wl-paste --list-types did not offer image/png after the capture: $(cat "$screenshot_types_path" 2>/dev/null)"
  fi
}
