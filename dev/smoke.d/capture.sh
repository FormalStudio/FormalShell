# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --capture drives the shell's own region picker, a real Overlay-layer
# surface with Exclusive keyboard focus rather than slurp. `pick smart`
# opens it over a session holding one real tiled window, `key tab` cycles
# the selection, `key ctrl-return` commits the whole display, and the PNG
# that comes out is measured against what the compositor says the output is.
# Then the toolbar half: `key 4` lights REC SCREEN (the same setTool a click
# on the cell calls) and Return commits it into a real wf-recorder child.
# `pick region` plus `key escape` closes the leg on the cancel path, since a
# full-screen Overlay surface that outlives its own cancel is the worst
# failure this surface has.
#
# The window split is what the port turns on: Hyprland reports a box for
# every window it does not hide, so the fixture window has to
# arrive DRAWABLE and the named-only card stays empty (the picker's
# unboxed-windows list is "Empty on Hyprland"). Asserting a positive count either
# way is the point, since a picker that never enumerated windows at all
# would report zero of both and otherwise look identical.
#
# `key` is the rig's stand-in for real key delivery into an Exclusive-focus
# layer surface, the same split every other surface's IPC actions use.
#
# The recorder itself gets further here than it did on the old rig, where
# the dmabuf bind was rejected outright: it starts, captures and writes a
# real container. What it does not do on this software-rendered session is
# answer SIGTERM on its `-g` crop path, so the run ends in the truncated
# state RecordingService reports, and this leg says so on its own marker
# line rather than passing quietly or failing the whole port.
leg_capture_flag="--capture"
leg_capture_order=200
leg_capture_needs="file jq"
# The picker is ABOUT windows, so the base run's fixture window is this
# leg's subject rather than something in the frame's way.
leg_capture_fixture_window=keep

# file(1) backs the PNG/container assertions in four of the capture legs.
# The scaffold resolves leg_<n>_needs through need_<name> and sources every
# leg file first, so a resolver a leg needs can live in the leg itself.
need_file() { need_tool file_bin file file; }

leg_capture_fixture() {
  # recording.noDmabuf is the one recording key software rendering makes
  # non-optional: there is no GPU buffer to import here, and without it
  # wf-recorder starts, captures, then wedges on SIGTERM until
  # RecordingService's own 5s escalation SIGKILLs it mid-container. The
  # --record leg writes the same key (plus its own webcam probe), so this
  # fills it in only when that leg is off.
  if ! leg_on record; then
    settings_fragment ', "recording": {"noDmabuf": true}'
  fi
}

capture_picker_path="$shot_dir/capture-picker.png"
capture_toolbar_path="$shot_dir/capture-toolbar-record.png"
capture_open_status_path="$shot_dir/capture-open-status.json"
capture_cycled_status_path="$shot_dir/capture-cycled-status.json"
capture_pick_reply_path="$shot_dir/capture-pick-reply.txt"
capture_status_path="$shot_dir/capture-status.json"
capture_escape_status_path="$shot_dir/capture-escape-status.json"
capture_outputs_path="$shot_dir/capture-outputs.json"
capture_tool_status_path="$shot_dir/capture-tool-status.json"
capture_rec_reply_path="$shot_dir/capture-rec-reply.txt"
capture_rec_status_path="$shot_dir/capture-rec-status.json"
capture_rec_stopped_path="$shot_dir/capture-rec-stopped.json"

# This leg's own clock: the picker covers the whole output, so under
# --wallpaper it starts past that leg's last frame, the same rule menu_t0
# and lock_t0 draw.
capture_t0() {
  cover_t0 5
}

leg_capture_timing() {
  local t0
  t0=$(capture_t0)
  # Open and cycle (4s), up to 10s polling `capturing` back to false, the
  # toolbar pass, then the region-and-escape pass, plus spawn overhead
  # per call on llvmpipe.
  leg_timing $((32 + t0 - 5)) $((58 + t0 - 5))
  # Sharing wf-recorder with --record costs this leg that leg's whole
  # recording before its own toolbar half can commit one.
  if leg_on record; then leg_timing $((80 + t0 - 5)) $((110 + t0 - 5)); fi
}

leg_capture_drive() {
  local t0 script="$shot_dir/capture-drive.sh"
  t0=$(capture_t0)
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep $t0
# The compositor's own view of the output, so the captured PNG's real pixel
# dimensions can be checked against it rather than merely existing.
"$hyprctl_bin" -j monitors > "$capture_outputs_path" 2>&1
$ipc call screenshot pick smart default > /dev/null 2>&1
sleep 2
$ipc call screenshot pickerStatus > "$capture_open_status_path" 2>&1
"$grim_bin" "$capture_picker_path" > /dev/null 2>&1
sleep 1
$ipc call screenshot key tab > /dev/null 2>&1
sleep 1
$ipc call screenshot pickerStatus > "$capture_cycled_status_path" 2>&1
$ipc call screenshot key ctrl-return > "$capture_pick_reply_path" 2>&1
for _ in \$(seq 1 20); do
  $ipc call screenshot status > "$capture_status_path" 2>&1
  if grep -q '"capturing":false' "$capture_status_path"; then
    break
  fi
  sleep 0.5
done
# The wait is for a --record run in the same session: every drive script is
# spawned at startup, so the two would otherwise race for the one
# wf-recorder child and this leg would fail on "already recording".
SECONDS=0
while [ "\$SECONDS" -lt 45 ]; do
  $ipc call record status 2>&1 | grep -qF '"active":false' && break
  sleep 1
done
$ipc call screenshot pick smart default > /dev/null 2>&1
sleep 2
$ipc call screenshot key 4 > /dev/null 2>&1
sleep 1
$ipc call screenshot pickerStatus > "$capture_tool_status_path" 2>&1
"$grim_bin" "$capture_toolbar_path" > /dev/null 2>&1
$ipc call screenshot key return > "$capture_rec_reply_path" 2>&1
# Long enough that the container holds real frames rather than just a header.
sleep 3
$ipc call record status > "$capture_rec_status_path" 2>&1
$ipc call record stop > /dev/null 2>&1
SECONDS=0
while [ "\$SECONDS" -lt 20 ]; do
  $ipc call record status > "$capture_rec_stopped_path" 2>&1
  grep -qF '"active":false' "$capture_rec_stopped_path" && break
  sleep 1
done
$ipc call screenshot pick region default > /dev/null 2>&1
sleep 2
$ipc call screenshot key escape > /dev/null 2>&1
sleep 1
$ipc call screenshot pickerStatus > "$capture_escape_status_path" 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_capture_assert() {
  local f capture_drawable capture_named capture_cursor capture_file
  local capture_dims capture_out_dims capture_tool_cursor capture_rec_file capture_rec_err
  for f in "$capture_open_status_path" "$capture_cycled_status_path" \
    "$capture_status_path" "$capture_escape_status_path"; do
    if [ ! -s "$f" ]; then
      fail "no capture status produced at $f"
    fi
  done
  cat "$capture_open_status_path"; echo
  if ! grep -q '"open":true' "$capture_open_status_path"; then
    fail "picker did not report open after 'screenshot pick smart': $(cat "$capture_open_status_path")"
  fi
  if [ ! -f "$capture_picker_path" ]; then
    fail "no picker screenshot produced at $capture_picker_path"
  fi

  # The load-bearing assertion of this leg. The session holds exactly one
  # real tiled window and Hyprland reports a box for it, so it MUST arrive
  # drawable, with nothing left in the named-only card.
  capture_drawable=$(sed -n 's/.*"drawableWindows":\([0-9]*\).*/\1/p' "$capture_open_status_path")
  capture_named=$(sed -n 's/.*"namedWindows":\([0-9]*\).*/\1/p' "$capture_open_status_path")
  if [ -z "$capture_drawable" ] || [ "$capture_drawable" -lt 1 ]; then
    fail "picker reported no drawable windows under Hyprland, so the window path was never exercised: $(cat "$capture_open_status_path")"
  fi
  if [ "$capture_named" != "0" ]; then
    fail "picker reported $capture_named named-only window(s), but Hyprland boxes every window: $(cat "$capture_open_status_path")"
  fi
  echo "SMOKE_CAPTURE_WINDOWS $capture_drawable drawable, $capture_named named-only, the expected Hyprland split"

  cat "$capture_cycled_status_path"; echo
  capture_cursor=$(sed -n 's/.*"cursor":\(-\{0,1\}[0-9]*\).*/\1/p' "$capture_cycled_status_path")
  if [ -z "$capture_cursor" ] || [ "$capture_cursor" -lt 0 ]; then
    fail "TAB did not move the picker cursor off its pointer-driven default: $(cat "$capture_cycled_status_path")"
  fi
  if ! grep -q '"selectionLabel":"[^"]' "$capture_cycled_status_path"; then
    fail "cycled selection carried no label, so the selected row had nothing to render: $(cat "$capture_cycled_status_path")"
  fi

  if ! grep -q '^ok$' "$capture_pick_reply_path"; then
    fail "CTRL+RETURN did not commit a selection. Got: $(cat "$capture_pick_reply_path" 2>/dev/null)"
  fi
  if ! grep -q '"capturing":false' "$capture_status_path" || ! grep -q '"lastError":""' "$capture_status_path"; then
    fail "capture did not settle clean: $(cat "$capture_status_path")"
  fi
  capture_file=$(sed -n 's/.*"lastPath":"\([^"]*\)".*/\1/p' "$capture_status_path")
  if [ -z "$capture_file" ] || [ ! -f "$capture_file" ]; then
    fail "capture reported no existing file, lastPath was '$capture_file': $(cat "$capture_status_path")"
  fi
  if ! "$file_bin" "$capture_file" | grep -q "PNG image data"; then
    fail "capture is not a valid PNG, file(1) says: $("$file_bin" -b "$capture_file")"
  fi

  # Ctrl+Return takes the whole display, so the PNG must be exactly the
  # output's size: proof the capture cropped to the intended rectangle
  # rather than to whatever grim felt like.
  capture_dims=$("$file_bin" -b "$capture_file" | sed -n 's/.*PNG image data, \([0-9]*\) x \([0-9]*\).*/\1x\2/p')
  capture_out_dims=$("$jq_bin" -r '.[0] | "\(.width)x\(.height)"' "$capture_outputs_path" 2>/dev/null)
  if [ -n "$capture_out_dims" ] && [ "$capture_dims" != "$capture_out_dims" ]; then
    fail "CTRL+RETURN captured ${capture_dims}, but the output is ${capture_out_dims}"
  fi
  echo "SMOKE_CAPTURE $capture_file (${capture_dims}, matches the output)"

  for f in "$capture_tool_status_path" "$capture_rec_reply_path" \
    "$capture_rec_status_path" "$capture_rec_stopped_path"; do
    if [ ! -s "$f" ]; then
      fail "no capture toolbar artifact produced at $f"
    fi
  done
  cat "$capture_tool_status_path"; echo
  if ! grep -qF '"action":"record"' "$capture_tool_status_path" \
    || ! grep -qF '"tool":3' "$capture_tool_status_path" \
    || ! grep -qF '"mode":"fullscreen"' "$capture_tool_status_path"; then
    fail "the REC SCREEN toolbar cell did not take: $(cat "$capture_tool_status_path")"
  fi
  # SCREEN preselects the focused output, so the commit has something to act
  # on with no pointer anywhere near the surface.
  capture_tool_cursor=$(sed -n 's/.*"cursor":\(-\{0,1\}[0-9]*\).*/\1/p' "$capture_tool_status_path")
  if [ -z "$capture_tool_cursor" ] || [ "$capture_tool_cursor" -lt 0 ]; then
    fail "REC SCREEN left nothing selected: $(cat "$capture_tool_status_path")"
  fi
  if [ ! -f "$capture_toolbar_path" ]; then
    fail "no toolbar screenshot produced at $capture_toolbar_path"
  fi
  if ! grep -q '^ok$' "$capture_rec_reply_path"; then
    fail "RETURN did not commit the record tool. Got: $(cat "$capture_rec_reply_path" 2>/dev/null)"
  fi
  echo "SMOKE_CAPTURE_TOOLBAR $capture_toolbar_path"

  # What comes out the far side is a real wf-recorder child writing a real
  # MP4. Both outcomes are real evidence and neither is quiet: a recorder
  # that ran has to leave a real container, and one that died has to say
  # why, in the recorder's own words. An empty lastError would mean nothing
  # was ever launched, which is a failure of the handoff and is treated as
  # one.
  cat "$capture_rec_status_path"; echo
  if grep -qF '"active":true' "$capture_rec_status_path"; then
    cat "$capture_rec_stopped_path"; echo
    if ! grep -qF '"active":false' "$capture_rec_stopped_path"; then
      fail "the picker's recording never stopped: $(cat "$capture_rec_stopped_path")"
    fi
    capture_rec_file=$(sed -n 's/.*"path":"\([^"]*\)".*/\1/p' "$capture_rec_stopped_path")
    # The path is built from the answering instance's $HOME, so one outside
    # this run's isolated HOME names a different formalshell on this machine.
    case "$capture_rec_file" in
      "$iso_home"/*) ;;
      *) fail "the picker's recording was answered by an instance outside this run: $capture_rec_file is not under $iso_home" ;;
    esac
    if [ ! -s "$capture_rec_file" ]; then
      fail "the picker's recording left no file at $capture_rec_file"
    fi
    if ! "$file_bin" -b "$capture_rec_file" | grep -qi "ISO Media"; then
      fail "the picker's recording is not an MP4 container, file(1) says: $("$file_bin" -b "$capture_rec_file")"
    fi
    if grep -qF '"lastError":""' "$capture_rec_stopped_path"; then
      echo "SMOKE_CAPTURE_RECORD $capture_rec_file ($("$file_bin" -b "$capture_rec_file"))"
    else
      # Where this leg stops is where the environment stops, and it says
      # which. The picker hands RecordingService a rectangle, so wf-recorder
      # runs with `-g`, and on this software-rendered session that path never
      # answers SIGTERM: the service's own 5s escalation SIGKILLs it and says
      # so. Reproduced at 2s, 5s and 6s takes, while --record's uncropped
      # run in the same VM stops clean every time, so it is the crop path,
      # not the length. Everything the SHELL owns is still proven: the
      # toolbar switched tools, the commit was accepted, a real recorder ran
      # and its container is on disk with this run's own name on it.
      capture_rec_err=$(sed -n 's/.*"lastError":"\([^"]*\)".*/\1/p' "$capture_rec_stopped_path")
      case "$capture_rec_err" in
        *"ignored SIGTERM"*)
          echo "SMOKE_CAPTURE_RECORD_TRUNCATED $capture_rec_file ($("$file_bin" -b "$capture_rec_file"))"
          echo "SMOKE_CAPTURE_RECORD_TRUNCATED wf-recorder's -g path does not answer SIGTERM under software rendering, so the container was cut off: $capture_rec_err"
          ;;
        *)
          fail "the picker's recording did not settle clean: $(cat "$capture_rec_stopped_path")" ;;
      esac
    fi
  else
    capture_rec_err=$(sed -n 's/.*"lastError":"\([^"]*\)".*/\1/p' "$capture_rec_status_path")
    if [ -z "$capture_rec_err" ]; then
      fail "the picker's record commit launched nothing at all, no recorder ran and no error was reported: $(cat "$capture_rec_status_path")"
    fi
    case "$capture_rec_err" in
      *dmabuf*|*"ignored SIGTERM"*)
        echo "SMOKE_CAPTURE_RECORD_BLOCKED wf-recorder spawned and died on a known software-rendering limit: $capture_rec_err"
        echo "SMOKE_CAPTURE_RECORD_BLOCKED the shell's half of the path is verified; the recorder itself needs a real GPU (docs/SWITCHOVER.md)"
        ;;
      *)
        fail "the picker's recording failed for a reason that is not the known environment block: $capture_rec_err" ;;
    esac
  fi

  # Escape must leave nothing mapped.
  if ! grep -q '"open":false' "$capture_escape_status_path"; then
    fail "picker still open after ESCAPE: $(cat "$capture_escape_status_path")"
  fi
  echo "SMOKE_CAPTURE_PICKER $capture_picker_path"
  echo "SMOKE_CAPTURE_CANCEL escape closed the picker clean"
}
