# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --capture-edit drives the SAVED notification's EDIT action, the most
# owner-visible part of the capture flow (the misbinding that shipped for
# weeks was found by the owner, not by this suite). `screenshot full` is the
# plain grim/wl-copy pipeline, the same one `pick`'s default processing hands
# off to once a region is chosen: the notify/edit half under test does not
# depend on which route picked the geometry, and --capture already covers the
# picker's own keyboard model. Once `status` settles the SAVED toast has
# already fired from inside the same handler, so the frame taken next
# carries its thumbnail and its EDIT action. `screenshot edit ""` is the
# exact call that action invokes, reachable without synthetic click delivery
# into an Overlay-layer surface.
#
# The editor is a shim that logs the argv it was handed. What needs proving
# is the shell's own path (capture, save, notify, action, spawn, argv), not
# whether a real GTK4 app renders in a VM. It is pointed at through
# settings.json's `screenshot.editor` rather than shadowing `tensaku-edit`
# on PATH: the scaffold owns the shell's launch script, so a leg has no seam
# to prefix its PATH with, and the config key is the shell's own documented
# handoff (`FS_EDITOR`) reaching the same spawn.
#
# A real foot window filling the screen with a known solid colour is the
# fixture, so the SAVED thumbnail shows scaled colour rather than an empty
# desktop that would look identical to the card's own background.
leg_capture_edit_flag="--capture-edit"
leg_capture_edit_order=225
leg_capture_edit_needs="foot file jq"
leg_capture_edit_claims="toasts=shared"

capture_edit_reply_path="$shot_dir/capture-edit-reply.txt"
capture_edit_status_path="$shot_dir/capture-edit-status.json"
capture_edit_toast_path="$shot_dir/capture-edit-toast.png"
capture_edit_edit_reply_path="$shot_dir/capture-edit-edit-reply.txt"
capture_edit_argv_path="$shot_dir/capture-edit-argv.txt"
capture_edit_fixture_pid_path="$shot_dir/capture-edit-foot.pid"
capture_edit_shim_dir="$shot_dir/capture-edit-shim"

leg_capture_edit_fixture() {
  mkdir -p "$capture_edit_shim_dir"
  cat > "$capture_edit_shim_dir/tensaku-edit" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$@" > "$capture_edit_argv_path"
EOF
  chmod +x "$capture_edit_shim_dir/tensaku-edit"
  settings_pin ".screenshot.editor = \"$capture_edit_shim_dir/tensaku-edit\""
}

leg_capture_edit_timing() {
  # 3s startup, the full()/status poll (fast: real grim and wl-copy against a
  # small screen), a 3s settle for the toast's image slot to finish its
  # asynchronous decode, then the edit() call.
  leg_timing 50
}

leg_capture_edit_drive() {
  local fixture_script="$shot_dir/capture-edit-fixture.sh" script="$shot_dir/capture-edit-drive.sh"
  write_script "$fixture_script" <<EOF
#!/usr/bin/env bash
echo \$\$ > "$capture_edit_fixture_pid_path"
exec "$foot_bin" --app-id=formalshell-smoke-capture-edit \\
  --override=colors.background=e6a23c --override=colors.foreground=101010 \\
  sh -c 'sleep 300'
EOF

  write_script "$script" <<EOF
#!/usr/bin/env bash
until_window formalshell-smoke-capture-edit
sleep 1
$ipc call screenshot full "" > "$capture_edit_reply_path" 2>&1
for _ in \$(seq 1 20); do
  $ipc call screenshot status > "$capture_edit_status_path" 2>&1
  if "$jq_bin" -e '.capturing == false' "$capture_edit_status_path" > /dev/null 2>&1; then
    break
  fi
  sleep 0.5
done
# The toast's image slot decodes asynchronously (Image.asynchronous in
# the notification card), scaling the just-saved screenshot down to a 40x40
# thumbnail off the render thread. A 1s buffer once left the shot racing that
# decode, landing a blank slot even though the layout had reserved its width.
sleep 3
"$grim_bin" "$capture_edit_toast_path" > /dev/null 2>&1
$ipc call screenshot edit "" > "$capture_edit_edit_reply_path" 2>&1
sleep 1
EOF
  wm_exec_once "bash $fixture_script"
  wm_exec_once "bash $script"
  # The fixture terminal has no auto-close of its own.
  add_cleanup "kill \"\$(cat '$capture_edit_fixture_pid_path' 2>/dev/null)\" 2>/dev/null || true"
}

leg_capture_edit_assert() {
  local capture_edit_file capture_edit_argv
  if [ ! -s "$capture_edit_reply_path" ]; then
    fail "no screenshot full IPC reply produced"
  fi
  cat "$capture_edit_reply_path"
  capture_edit_file=$(head -n1 "$capture_edit_reply_path" | tr -d '\r')
  case "$capture_edit_file" in
    error*|"") fail "screenshot full replied with an error: $(cat "$capture_edit_reply_path")" ;;
  esac
  if [ ! -f "$capture_edit_file" ]; then
    fail "screenshot reply path does not exist: $capture_edit_file"
  fi
  if ! "$file_bin" "$capture_edit_file" | grep -q "PNG image data"; then
    fail "saved screenshot is not a valid PNG, file(1) says: $("$file_bin" -b "$capture_edit_file")"
  fi
  if [ ! -s "$capture_edit_status_path" ]; then
    fail "no screenshot status produced"
  fi
  cat "$capture_edit_status_path"
  expect_jq "$capture_edit_status_path" '.capturing == false and .lastError == ""' "screenshot status did not settle clean before the edit handoff"
  expect_jq "$capture_edit_status_path" ".lastPath == \"$capture_edit_file\"" "screenshot status lastPath does not match the reply path ($capture_edit_file)" .lastPath
  # The toast's thumbnail and EDIT action are read by eye off this frame:
  # NotificationsIpc's status() counts popups, it does not dump their content.
  if [ ! -f "$capture_edit_toast_path" ]; then
    fail "no capture-edit toast screenshot produced at $capture_edit_toast_path"
  fi
  if ! grep -q '^ok$' "$capture_edit_edit_reply_path" 2>/dev/null; then
    fail "screenshot edit did not return ok. Got: $(cat "$capture_edit_edit_reply_path" 2>/dev/null)"
  fi
  if [ ! -s "$capture_edit_argv_path" ]; then
    fail "the shimmed editor never ran, no argv recorded at $capture_edit_argv_path"
  fi
  capture_edit_argv=$(cat "$capture_edit_argv_path")
  echo "SMOKE_CAPTURE_EDIT_ARGV $capture_edit_argv"
  if [ "$capture_edit_argv" != "$capture_edit_file" ]; then
    fail "editor argv did not match the saved path, got '$capture_edit_argv', wanted '$capture_edit_file'"
  fi
  echo "SMOKE_CAPTURE_EDIT_TOAST $capture_edit_toast_path"
}
