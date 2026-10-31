# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --overview: the shell's workspace overview
# (docs/superpowers/specs/2026-10-31-overview.md), niri's, under `layout:
# sidebar` with foot windows a and b on workspace 1 and c on 2, each step
# read off `debug dump`'s `overview` and `captures.overview`.
#
# Opened four ways: Super+O (a bind on `workspaces overview toggle`, as the
# shipped Lua has it), `workspaces overview open`, the top-left hot corner
# (parked in, it stays open; a leave and a new approach close it), and the
# minimap (a 900ms press on an icon, and an icon dragged out past the
# column). Closed by Escape, Return, the corner and IPC, and after every
# close no capture session is left. Inside it real keys and a wheel move
# the selection: Down onto workspace 2 and its window c, Up back, Right
# along workspace 1, a wheel notch down and up; Return on c focuses c
# (`hyprctl activewindow`). A real drag (`dev/vpointer.py`) carries a onto
# workspace 2's card and lets go there: a lands on workspace 2 in `hyprctl
# clients` with the overview still open.
#
# Budgets off the shell's own log: the surface maps inside 50ms of the
# request (g815 only), the zoom commits frames all the way through, and at
# rest it commits nothing in 3s. Frames pulled for the sheet: the desktop,
# open, mid-zoom and mid-glide at a tenth speed, and the drop.
leg_overview_flag="--overview"
leg_overview_order=239
leg_overview_needs="jq foot wtype wlrctl python3 grim"
overview_vpointer="$PWD/dev/vpointer.py"
overview_log="$shot_dir/overview-steps.log"
overview_notes="$shot_dir/overview-notes.log"
overview_desktop_png="$shot_dir/overview-desktop.png"
overview_open_png="$shot_dir/overview-open.png"
overview_zoom_png="$shot_dir/overview-zoom.png"
overview_glide_png="$shot_dir/overview-glide.png"
overview_drop_png="$shot_dir/overview-drop.png"

leg_overview_validate() {
  if leg_on bar_position || leg_on frame || leg_on minimap; then
    echo "usage: --overview runs under layout sidebar and cannot combine with --bar-position, --frame or --minimap" >&2
    exit 1
  fi
}

leg_overview_fixture() {
  leg_on sidebar || settings_fragment ', "layout": "sidebar"'
  settings_fragment ', "hotCorners": {"topLeft": "overview", "bottomLeft": "none", "bottomRight": "none"}'
}

leg_overview_timing() {
  leg_timing 55 140
}

leg_overview_drive() {
  local script="$shot_dir/overview-drive.sh"
  cat <<EOF
-- wtype's keymap and Hyprland's keycodes disagree; switcher_keys.sh says why.
hl.config({ input = { resolve_binds_by_sym = true } })
hl.bind("SUPER + O", hl.dsp.exec_cmd([==[$ipc call workspaces overview toggle]==]))
EOF
  write_script "$script" <<EOS
#!/usr/bin/env bash
log() { printf '%s\n' "\$*" >> "$overview_notes"; }
dump() { $ipc call debug dump 2>/dev/null; }
# One line per step: the overview's phase, its selection and the sessions it holds.
step() {
  dump | "$jq_bin" -c --arg s "\$1" '{step: \$s, phase: .overview.phase, open: .overview.open, zoom: .overview.zoom, ws: .overview.selected.workspace, win: .overview.selected.window, opens: .overview.opens, capturing: .overview.capturing, sessions: (.captures.overview | length), rows: [.overview.rows[] | {workspace, windows: [.windows[].id]}]}' >> "$overview_log" 2>&1
}
wait_phase() {
  for _ in \$(seq 1 40); do
    [ "\$(dump | "$jq_bin" -r '.overview.phase')" = "\$1" ] && return 0
    sleep 0.1
  done
  return 1
}
for _ in \$(seq 1 40); do
  $ipc call sidebar status 2>/dev/null | grep -q '"cards"' && break
  sleep 0.5
done
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin -o colors-dark.background=1e3a5f --app-id=formalshell-ov-a sh -c 'printf \"\\\\033[1mwindow a\\\\033[0m\\\\n\\\\n\"; ls -1 /usr/bin | head -60; sleep 300']==])" > /dev/null 2>&1
sleep 1
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin -o colors-dark.background=4a2f5c --app-id=formalshell-ov-b sh -c 'printf \"\\\\033[1mwindow b\\\\033[0m\\\\n\\\\n\"; ls -1 /usr/bin | head -60; sleep 300']==])" > /dev/null 2>&1
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin -o colors-dark.background=2f4f33 --app-id=formalshell-ov-c sh -c 'printf \"\\\\033[1mwindow c\\\\033[0m\\\\n\\\\n\"; ls -1 /usr/bin | head -60; sleep 300']==], { workspace = '2 silent' })" > /dev/null 2>&1
sleep 3
# The dump's ids are Hyprland's addresses without their 0x.
id_of() { "$hyprctl_bin" -j clients | "$jq_bin" -r --arg c "\$1" '.[] | select(.class == \$c) | .address | ltrimstr("0x")' | head -1; }
# wlrctl moves are relative: from where the cursor is to an absolute point,
# without passing the top-left corner on the way.
goto() {
  read -r cx cy <<< "\$("$hyprctl_bin" -j cursorpos | "$jq_bin" -r '"\(.x | floor) \(.y | floor)"')"
  "$wlrctl_bin" pointer move "\$(( \$1 - cx ))" "\$(( \$2 - cy ))" > /dev/null 2>&1
}
log "ids a=\$(id_of formalshell-ov-a) b=\$(id_of formalshell-ov-b) c=\$(id_of formalshell-ov-c)"
"$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1
"$wlrctl_bin" pointer move 900 500 > /dev/null 2>&1
sleep 0.5
"$grim_bin" "$overview_desktop_png" > /dev/null 2>&1
step rest

# Super+O, then the keys inside it.
"$wtype_bin" -M logo -P Super_L -k o -p Super_L -m logo
wait_phase open; sleep 0.8
step bind-open
"$grim_bin" "$overview_open_png" > /dev/null 2>&1
sleep 3
"$wtype_bin" -k Down; sleep 0.6; step key-down
"$wtype_bin" -k Up; sleep 0.6; step key-up
"$wtype_bin" -k Right; sleep 0.6; step key-right
"$wtype_bin" -k Left; sleep 0.6; step key-left
"$wlrctl_bin" pointer scroll 15 0 > /dev/null 2>&1; sleep 0.6; step wheel-down
"$wlrctl_bin" pointer scroll -15 0 > /dev/null 2>&1; sleep 0.6; step wheel-up
"$wtype_bin" -k Escape
wait_phase closed; sleep 0.3
step escape-closed

# IPC, then Return on c.
$ipc call workspaces overview open > /dev/null 2>&1
wait_phase open; sleep 0.8
step ipc-open
"$wtype_bin" -k Down; sleep 0.6; step ipc-down
"$wtype_bin" -k Return
wait_phase closed; sleep 0.5
step return-closed
"$hyprctl_bin" -j activewindow | "$jq_bin" -c '{class, workspace: .workspace.id}' > "$shot_dir/overview-active.json" 2>&1
log "active \$(cat "$shot_dir/overview-active.json")"
"$hyprctl_bin" dispatch "hl.dsp.focus({ workspace = 1 })" > /dev/null 2>&1
sleep 1

# A tenth speed: one frame part way through the zoom, one through a glide.
$ipc call debug motionScale 1000 > /dev/null 2>&1
$ipc call workspaces overview open > /dev/null 2>&1
sleep 1.6
"$grim_bin" "$overview_zoom_png" > /dev/null 2>&1
step mid-zoom
wait_phase open
sleep 4.5
"$wtype_bin" -k Down; sleep 1.6
"$grim_bin" "$overview_glide_png" > /dev/null 2>&1
step mid-glide
sleep 4
$ipc call debug motionScale 100 > /dev/null 2>&1
"$wtype_bin" -k Up; sleep 0.6
$ipc call workspaces overview close > /dev/null 2>&1
wait_phase closed; sleep 0.3
step ipc-closed

# The drag: a onto workspace 2's card.
$ipc call workspaces overview open > /dev/null 2>&1
wait_phase open; sleep 0.8
a=\$(id_of formalshell-ov-a)
read -r ax ay bx by <<< "\$(dump | "$jq_bin" -r --arg a "\$a" '.overview as \$o | (\$o.rows[] | .windows[] | select(.id == \$a) | .rect) as \$w | (\$o.rows[1].card) as \$c | "\(\$o.origin[0] + \$w.x + (\$w.width / 2) | floor) \(\$o.origin[1] + \$w.y + (\$w.height / 2) | floor) \(\$o.origin[0] + \$c.x + (\$c.width / 2) | floor) \(\$o.origin[1] + \$c.y + (\$c.height / 2) | floor)"')"
log "drag a from \$ax,\$ay to \$bx,\$by"
goto "\$ax" "\$ay"
sleep 0.4
dx=\$(( (bx - ax) / 10 )); dy=\$(( (by - ay) / 10 ))
steps=()
for _ in 1 2 3 4 5 6 7 8 9 10; do steps+=(move "\$dx" "\$dy" wait 30); done
"$python3_bin" "$overview_vpointer" down wait 60 "\${steps[@]}" wait 300 up > /dev/null 2>&1
sleep 1.2
step dropped
"$grim_bin" "$overview_drop_png" > /dev/null 2>&1
"$hyprctl_bin" -j clients | "$jq_bin" -c --arg a "\$a" '[.[] | select(.address == ("0x" + \$a)) | .workspace.id]' > "$shot_dir/overview-dropped.json" 2>&1
log "a on \$(cat "$shot_dir/overview-dropped.json")"
$ipc call workspaces overview close > /dev/null 2>&1
wait_phase closed; sleep 0.3
step drag-closed

# The corner: parked opens it and it stays; a leave and a return close it.
"$wlrctl_bin" pointer move 900 500 > /dev/null 2>&1
sleep 0.6
"$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1
sleep 1.2
step corner-open
sleep 2
step corner-parked
"$wlrctl_bin" pointer move 900 500 > /dev/null 2>&1
sleep 0.8
"$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1
sleep 1.5
step corner-closed
"$wlrctl_bin" pointer move 900 500 > /dev/null 2>&1
sleep 0.6

# The minimap: a long press on an icon, then an icon dragged out past the column.
centre() {
  $ipc call sidebar status 2>/dev/null | "$jq_bin" -r --arg on "\$1" '.hits[] | select(.on == \$on) | "\(.x + (.w / 2 | floor)) \(.y + (.h / 2 | floor))"' | head -1
}
first=\$($ipc call sidebar status | "$jq_bin" -r '.rows[0].windows[0]')
read -r mx my <<< "\$(centre "window:\$first")"
goto "\$mx" "\$my"
sleep 0.3
"$python3_bin" "$overview_vpointer" down wait 900 up > /dev/null 2>&1
sleep 1.2
step press-open
$ipc call workspaces overview close > /dev/null 2>&1
wait_phase closed; sleep 0.3
step press-closed
first=\$($ipc call sidebar status | "$jq_bin" -r '.rows[0].windows[0]')
read -r mx my <<< "\$(centre "window:\$first")"
goto "\$mx" "\$my"
sleep 0.3
"$python3_bin" "$overview_vpointer" down wait 50 move 40 0 wait 30 move 80 0 wait 30 move 120 0 wait 30 move 160 0 wait 300 up > /dev/null 2>&1
sleep 1.2
step out-open
$ipc call workspaces overview close > /dev/null 2>&1
wait_phase closed; sleep 0.3
step out-closed
EOS
  hypr_exec_once "bash $script"
}

overview_field() {
  "$jq_bin" -r --arg s "$1" "select(.step == \$s) | $2" "$overview_log" 2>/dev/null | head -1
}

leg_overview_assert() {
  echo "SMOKE_OVERVIEW_STEPS $overview_log"
  echo "SMOKE_OVERVIEW_NOTES $overview_notes"
  local png
  for png in "$overview_desktop_png" "$overview_open_png" "$overview_zoom_png" "$overview_glide_png" "$overview_drop_png"; do
    [ -s "$png" ] && echo "SMOKE_OVERVIEW_$(basename "$png" .png | tr 'a-z-' 'A-Z_') $png"
  done
  [ -s "$overview_log" ] || fail "overview: no step log at $overview_log"
  cat "$overview_notes" "$overview_log"
  grep -m4 -E 'overview (opening|mapped)|^commit surface=overview n=1 |configure surface=overview' "$shell_log_path"
  grep -E 'hot corner|overview' "$shell_log_path" | tail -60
  local a b c
  read -r a b c <<< "$(sed -n 's/^ids a=\([^ ]*\) b=\([^ ]*\) c=\([^ ]*\)$/\1 \2 \3/p' "$overview_notes")"
  [ -n "$a" ] && [ -n "$c" ] || fail "overview: the fixture windows never mapped"
  [ "$(overview_field rest .phase)" = closed ] || fail "overview: open before anything asked"

  local step
  for step in bind-open ipc-open corner-open corner-parked press-open out-open; do
    [ "$(overview_field "$step" .phase)" = open ] || fail "overview: $step: not open ($(overview_field "$step" .phase))"
  done
  for step in escape-closed return-closed ipc-closed drag-closed corner-closed press-closed out-closed; do
    [ "$(overview_field "$step" .phase)" = closed ] || fail "overview: $step: not closed ($(overview_field "$step" .phase))"
    [ "$(overview_field "$step" .sessions)" = 0 ] && [ "$(overview_field "$step" .capturing)" = 0 ] \
      || fail "overview: $step: captures left after the close ($(overview_field "$step" .sessions) sessions)"
  done
  [ "$(overview_field bind-open .sessions)" -ge 2 ] || fail "overview: open with $(overview_field bind-open .sessions) capture sessions, wanted one per window"
  [ "$(overview_field bind-open ".zoom == 1")" = true ] || fail "overview: at rest the zoom reads $(overview_field bind-open .zoom)"

  local ws1 ws2
  ws1=$(overview_field bind-open '.rows[0].workspace')
  ws2=$(overview_field bind-open '.rows[1].workspace')
  [ "$(overview_field bind-open '.rows | length')" = 3 ] || fail "overview: wanted workspaces 1 and 2 and the empty slot: $(overview_field bind-open .rows)"
  [ "$(overview_field bind-open .ws)" = "$ws1" ] || fail "overview: opened off the focused workspace"
  [ "$(overview_field key-down .ws)" = "$ws2" ] && [ "$(overview_field key-down .win)" = "$c" ] || fail "overview: Down did not land on workspace 2's window"
  [ "$(overview_field key-up .ws)" = "$ws1" ] || fail "overview: Up did not come back to workspace 1"
  [ "$(overview_field key-right .win)" != "$(overview_field key-left .win)" ] || fail "overview: Right and Left did not move along workspace 1"
  [ "$(overview_field wheel-down .ws)" = "$ws2" ] || fail "overview: a wheel notch down did not move to workspace 2"
  [ "$(overview_field wheel-up .ws)" = "$ws1" ] || fail "overview: a wheel notch up did not come back"
  [ "$(overview_field ipc-down .win)" = "$c" ] || fail "overview: Down after the IPC open did not select c"
  "$jq_bin" -e '.class == "formalshell-ov-c"' "$shot_dir/overview-active.json" > /dev/null 2>&1 \
    || fail "overview: Return on c focused $(cat "$shot_dir/overview-active.json" 2>/dev/null)"
  local zoom
  zoom=$(overview_field mid-zoom .zoom)
  awk -v z="$zoom" 'BEGIN { exit !(z > 0.02 && z < 0.98) }' || fail "overview: the mid-zoom frame was taken at zoom $zoom"

  [ "$(overview_field dropped .phase)" = open ] || fail "overview: the drop closed it"
  "$jq_bin" -e '.[0] == 2' "$shot_dir/overview-dropped.json" > /dev/null 2>&1 \
    || fail "overview: a dragged onto workspace 2 is on $(cat "$shot_dir/overview-dropped.json" 2>/dev/null)"
  [ "$(overview_field dropped "[.rows[1].windows[] | select(. == \"$a\")] | length")" = 1 ] || fail "overview: workspace 2's card does not hold a after the drop"

  # The log's own clocks: request to the first frame, the zoom's frames, rest.
  local opened mapped
  opened=$(grep -m1 'ipc t=[0-9]*ms overview opening' "$shell_log_path" | sed 's/.*t=\([0-9]*\)ms.*/\1/')
  mapped=$(grep -m1 'ipc t=[0-9]*ms overview mapped' "$shell_log_path" | sed 's/.*t=\([0-9]*\)ms.*/\1/')
  [ -n "$opened" ] && [ -n "$mapped" ] || fail "overview: no opening or mapped line in the shell log"
  echo "SMOKE_OVERVIEW_FIRST_FRAME $((mapped - opened))ms"
  if [ "$(cat /etc/formalshell-rig/kind 2>/dev/null)" != mac ] && [ $((mapped - opened)) -ge 50 ]; then
    fail "overview: $((mapped - opened))ms from the request to the first frame, over 50"
  fi
  local zooming zoom_commits
  zooming=$(grep -m1 'ipc t=[0-9]*ms overview zooming' "$shell_log_path" | sed 's/.*t=\([0-9]*\)ms.*/\1/')
  zoom_commits=$(awk -v t0="$zooming" '/^commit surface=overview / { sub(/^commit surface=overview n=[0-9]* t=/, ""); sub(/ms.*/, ""); if ($0 + 0 >= t0 && $0 + 0 <= t0 + 400) n++ } END { print n + 0 }' "$shell_log_path")
  echo "SMOKE_OVERVIEW_ZOOM_COMMITS $zoom_commits in the first 400ms of the zoom"
  [ "$zoom_commits" -ge 12 ] || fail "overview: only $zoom_commits commits through a 400ms zoom"
  # Super+O's zoom starts at t0, the keys come 3.8s later.
  local rest
  rest=$(awk -v t0="$zooming" '/^commit surface=overview / { sub(/^commit surface=overview n=[0-9]* t=/, ""); sub(/ms.*/, ""); if ($0 + 0 >= t0 + 1000 && $0 + 0 <= t0 + 3600) n++ } END { print n + 0 }' "$shell_log_path")
  echo "SMOKE_OVERVIEW_REST_COMMITS $rest between 1s and 3.6s after the zoom started"
  [ "$rest" = 0 ] || fail "overview: $rest commits at rest"

  for png in "$overview_desktop_png" "$overview_open_png" "$overview_zoom_png" "$overview_glide_png" "$overview_drop_png"; do
    [ -s "$png" ] || fail "overview: no frame at $png"
  done
}
