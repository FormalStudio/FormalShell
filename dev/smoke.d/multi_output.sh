# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --multi-output: the wallpaper, the bar and the frame's four zones on every
# output, read off the compositor's own layer list. g815's shape: a second
# output to the right of the first at scale 1.25, here a headless one created
# after the shell started (so it reaches the shell as a new wl_output, not in
# the first roundtrip) and given its mode, position and scale with
# `hl.monitor`. Each output is grabbed on its own with `grim -o`, and the
# frame the run ends on covers both.
#
# Then a real pointer (wlrctl, the only client that sends a surface a real
# enter): a panel opened on the first output, and a click on the second
# output's desktop has to close it, through the catcher the second output
# carries while it is open. A panel opened there again, and a click on the
# second output's own audio cell (its place read off `bar room` once the
# pointer has promoted that bar) has to close it and open audio there.
# A fullscreen window there then unmaps that bar, and once it closes the
# bar's new window has to draw every cell nothing changed with the pointer
# kept off it: a new window used to draw only what changed since the old
# one, so cells stayed blank until a hover repainted them (g815,
# 2026-10-24).
#
# The screensaver next, with the pointer resting on the second output: a
# formalshell:screensaver layer on each output and each grabbed frame mostly
# the screensaver's black, then one real pointer move on the second output
# dismissing both. Started again, the second output is removed under it (the
# shell has to drop that output's surfaces and keep the first one's, still
# running) and created again, which has to get an overlay of its own. Last
# the lock over both outputs, each frame grabbed, and the real password
# lifting it.
#
# The frame's zones are counted against the shell's own `debug dump`: four
# with the ring drawn, none without.
leg_multi_output_flag="--multi-output"
leg_multi_output_order=217
leg_multi_output_needs="jq wlrctl convert wtype foot"

multi_output_second="FS-MULTI"
multi_output_create="$shot_dir/multi-output-create.txt"
multi_output_monitors="$shot_dir/multi-output-monitors.json"
multi_output_layers_one="$shot_dir/multi-output-layers-one.json"
multi_output_layers_two="$shot_dir/multi-output-layers-two.json"
multi_output_layers_gone="$shot_dir/multi-output-layers-gone.json"
multi_output_first_png="$shot_dir/multi-output-first.png"
multi_output_second_png="$shot_dir/multi-output-second.png"
multi_output_click="$shot_dir/multi-output-click.txt"
multi_output_layers_open="$shot_dir/multi-output-layers-open.json"
multi_output_layers_closed="$shot_dir/multi-output-layers-closed.json"
multi_output_layers_reopen="$shot_dir/multi-output-layers-reopen.json"
multi_output_room="$shot_dir/multi-output-room.json"
multi_output_room_after="$shot_dir/multi-output-room-after.json"
multi_output_open_png="$shot_dir/multi-output-open.png"
multi_output_closed_png="$shot_dir/multi-output-closed.png"
multi_output_cell_png="$shot_dir/multi-output-cell.png"
multi_output_dump="$shot_dir/multi-output-dump.json"
multi_output_saver="$shot_dir/multi-output-saver.txt"
multi_output_saver_layers="$shot_dir/multi-output-saver-layers.json"
multi_output_saver_dismissed="$shot_dir/multi-output-saver-dismissed.json"
multi_output_saver_readd="$shot_dir/multi-output-saver-readd.json"
multi_output_saver_first_png="$shot_dir/multi-output-saver-first.png"
multi_output_saver_second_png="$shot_dir/multi-output-saver-second.png"
multi_output_saver_readd_png="$shot_dir/multi-output-saver-readd.png"
multi_output_lock_first_png="$shot_dir/multi-output-lock-first.png"
multi_output_lock_second_png="$shot_dir/multi-output-lock-second.png"
multi_output_bare_png="$shot_dir/multi-output-bare.png"
multi_output_unhidden_png="$shot_dir/multi-output-unhidden.png"
multi_output_hidden="$shot_dir/multi-output-hidden.txt"

leg_multi_output_fixture() {
  settings_fragment ', "frame": {"thickness": 6, "radius": 20}'
}

leg_multi_output_timing() {
  leg_timing 100 140
}

leg_multi_output_drive() {
  local script="$shot_dir/multi-output-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 4
"$hyprctl_bin" -j layers > "$multi_output_layers_one" 2>&1
$ipc call debug dump > "$multi_output_dump" 2>&1
first=\$("$hyprctl_bin" -j monitors | "$jq_bin" -r '.[0].name')
width=\$("$hyprctl_bin" -j monitors | "$jq_bin" -r '.[0].width')
"$hyprctl_bin" output create headless "$multi_output_second" > "$multi_output_create" 2>&1
"$hyprctl_bin" eval "hl.monitor({ output = \\"$multi_output_second\\", mode = \\"1600x1000@60\\", position = \\"\${width}x0\\", scale = 1.25 })" >> "$multi_output_create" 2>&1
sleep 4
"$hyprctl_bin" -j monitors > "$multi_output_monitors" 2>&1
"$hyprctl_bin" -j layers > "$multi_output_layers_two" 2>&1
"$grim_bin" -o "\$first" "$multi_output_first_png" > /dev/null 2>&1
"$grim_bin" -o "$multi_output_second" "$multi_output_second_png" > /dev/null 2>&1
sleep 6
call() { $ipc call "\$@"; }
lw=\$("$hyprctl_bin" -j monitors | "$jq_bin" -r '.[0].width / .[0].scale | floor')
lh=\$("$hyprctl_bin" -j monitors | "$jq_bin" -r '.[0].height / .[0].scale | floor')
park() {
  "$wlrctl_bin" pointer move -8000 -8000 >> "$multi_output_click" 2>&1
  "$wlrctl_bin" pointer move "\$1" "\$2" >> "$multi_output_click" 2>&1
  sleep 1
  echo "park \$1 \$2: \$("$hyprctl_bin" cursorpos)" >> "$multi_output_click"
}
park \$((lw / 2)) \$((lh / 2))
echo "open: \$(call panel open network)" >> "$multi_output_click"
sleep 2
echo "state open: \$(call panel state)" >> "$multi_output_click"
"$hyprctl_bin" -j layers > "$multi_output_layers_open" 2>&1
"$grim_bin" "$multi_output_open_png" > /dev/null 2>&1
park \$((lw + 640)) 500
"$wlrctl_bin" pointer click left >> "$multi_output_click" 2>&1
sleep 2
echo "state clicked: \$(call panel state)" >> "$multi_output_click"
"$hyprctl_bin" -j layers > "$multi_output_layers_closed" 2>&1
"$grim_bin" "$multi_output_closed_png" > /dev/null 2>&1
park \$((lw + 640)) 2
call bar room > "$multi_output_room" 2>&1
park \$((lw / 2)) \$((lh / 2))
echo "reopen: \$(call panel open network)" >> "$multi_output_click"
sleep 2
echo "state reopen: \$(call panel state)" >> "$multi_output_click"
"$hyprctl_bin" -j layers > "$multi_output_layers_reopen" 2>&1
cell=\$("$jq_bin" -r '[.[0].cells[] | select(.name == "audio" and .whole)] | first | "\\(.x + .width / 2 | floor) \\(.y + .height / 2 | floor)"' "$multi_output_room" 2>/dev/null)
echo "audio cell \$cell" >> "$multi_output_click"
park \$((lw + \${cell%% *})) \${cell##* }
"$wlrctl_bin" pointer click left >> "$multi_output_click" 2>&1
sleep 2
echo "state cell: \$(call panel state)" >> "$multi_output_click"
call bar room > "$multi_output_room_after" 2>&1
"$grim_bin" "$multi_output_cell_png" > /dev/null 2>&1
call panel close > /dev/null 2>&1
park \$((lw + 640)) 500
sleep 2
"$grim_bin" -o "$multi_output_second" "$multi_output_bare_png" > /dev/null 2>&1
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-multi-full sh -c 'sleep 60']==])" > /dev/null 2>&1
sleep 3
"$hyprctl_bin" dispatch "hl.dsp.focus({ window = 'class:formalshell-multi-full' })" > /dev/null 2>&1
"$hyprctl_bin" dispatch "hl.dsp.window.fullscreen({ mode = 'fullscreen', action = 'set' })" > /dev/null 2>&1
sleep 2
"$hyprctl_bin" -j layers | "$jq_bin" -r --arg s "$multi_output_second" '"bars under fullscreen: \([.[\$s].levels[]?[]? | select(.namespace == "formalshell:bar")] | length)"' >> "$multi_output_hidden" 2>&1
"$hyprctl_bin" dispatch "hl.dsp.window.fullscreen({ mode = 'fullscreen', action = 'unset' })" > /dev/null 2>&1
"$hyprctl_bin" dispatch "hl.dsp.window.close()" > /dev/null 2>&1
sleep 3
"$grim_bin" -o "$multi_output_second" "$multi_output_unhidden_png" > /dev/null 2>&1
echo "start: \$(call screensaver start)" >> "$multi_output_saver"
sleep 3
"$hyprctl_bin" -j layers > "$multi_output_saver_layers" 2>&1
"$grim_bin" -o "\$first" "$multi_output_saver_first_png" > /dev/null 2>&1
"$grim_bin" -o "$multi_output_second" "$multi_output_saver_second_png" > /dev/null 2>&1
echo "status shown: \$(call screensaver status)" >> "$multi_output_saver"
"$wlrctl_bin" pointer move 30 30 >> "$multi_output_saver" 2>&1
sleep 3
echo "status moved: \$(call screensaver status)" >> "$multi_output_saver"
"$hyprctl_bin" -j layers > "$multi_output_saver_dismissed" 2>&1
echo "restart: \$(call screensaver start)" >> "$multi_output_saver"
sleep 2
"$hyprctl_bin" output remove "$multi_output_second" >> "$multi_output_create" 2>&1
sleep 3
"$hyprctl_bin" -j layers > "$multi_output_layers_gone" 2>&1
"$hyprctl_bin" output create headless "$multi_output_second" >> "$multi_output_create" 2>&1
"$hyprctl_bin" eval "hl.monitor({ output = \\"$multi_output_second\\", mode = \\"1600x1000@60\\", position = \\"\${width}x0\\", scale = 1.25 })" >> "$multi_output_create" 2>&1
sleep 4
"$hyprctl_bin" -j layers > "$multi_output_saver_readd" 2>&1
"$grim_bin" -o "$multi_output_second" "$multi_output_saver_readd_png" > /dev/null 2>&1
echo "status readd: \$(call screensaver status)" >> "$multi_output_saver"
call screensaver stop > /dev/null 2>&1
sleep 2
echo "lock: \$(call lock lock)" >> "$multi_output_saver"
sleep 3
"$grim_bin" -o "\$first" "$multi_output_lock_first_png" > /dev/null 2>&1
"$grim_bin" -o "$multi_output_second" "$multi_output_lock_second_png" > /dev/null 2>&1
echo "locked: \$(call lock isLocked)" >> "$multi_output_saver"
"$wtype_bin" "formalshell-test"
"$wtype_bin" -k Return
for i in \$(seq 1 40); do
  [ "\$(call lock isLocked)" = false ] && break
  sleep 0.25
done
echo "unlocked: \$(call lock isLocked)" >> "$multi_output_saver"
"$hyprctl_bin" output remove "$multi_output_second" >> "$multi_output_create" 2>&1
EOF
  hypr_exec_once "bash $script"
}

# Surfaces on output $2 under namespace $3 in layers dump $1.
_multi_output_count() {
  "$jq_bin" -r --arg o "$2" --arg ns "$3" '[.[$o].levels[]?[]? | select(.namespace == $ns)] | length' "$1" 2>/dev/null
}

# Every output in dump $1 carrying one wallpaper, one bar and the frame zones
# the shell says it drew (four, or none with no ring drawn).
_multi_output_check() {
  local dump="$1" when="$2" out wall bar zones
  for out in $("$jq_bin" -r 'keys[]' "$dump"); do
    wall=$(_multi_output_count "$dump" "$out" "formalshell:wallpaper")
    bar=$(_multi_output_count "$dump" "$out" "formalshell:bar")
    zones=$(_multi_output_count "$dump" "$out" "formalshell:frame-zone")
    echo "$when: $out wallpaper=$wall bar=$bar frame-zone=$zones"
    [ "$wall" = 1 ] || fail "$when: $out carries $wall wallpaper surfaces, want 1"
    [ "$bar" = 1 ] || fail "$when: $out carries $bar bar surfaces, want 1"
    [ "$zones" = "$multi_output_zones" ] || fail "$when: $out carries $zones frame zones, want $multi_output_zones"
  done
}

# The share of frame $1 that is the screensaver's own black.
_multi_output_black() {
  $convert_bin "$1" -alpha off -fuzz 2% -fill white +opaque black -format '%[fx:1-mean]' info: 2>/dev/null
}

leg_multi_output_assert() {
  local f n out share colors
  for f in "$multi_output_layers_one" "$multi_output_layers_two" "$multi_output_layers_gone" "$multi_output_monitors" "$multi_output_dump"; do
    [ -s "$f" ] || fail "multi-output leg produced no $(basename "$f")"
  done
  if "$jq_bin" -e '.frame.enabled == true' "$multi_output_dump" > /dev/null 2>&1; then
    multi_output_zones=4
  else
    multi_output_zones=0
  fi
  echo "frame: $("$jq_bin" -c '.frame' "$multi_output_dump"), want $multi_output_zones zones per output"
  cat "$multi_output_create"
  "$jq_bin" -e --arg s "$multi_output_second" '.[] | select(.name == $s) | (.scale - 1.25 | fabs) < 0.01' "$multi_output_monitors" > /dev/null \
    || fail "the second output is not at scale 1.25: $("$jq_bin" -c '.[] | {name,scale,x,width}' "$multi_output_monitors")"

  n=$("$jq_bin" -r 'keys | length' "$multi_output_layers_one")
  [ "$n" = 1 ] || fail "expected one output before the second was created, got $n"
  _multi_output_check "$multi_output_layers_one" "one output"

  n=$("$jq_bin" -r 'keys | length' "$multi_output_layers_two")
  [ "$n" = 2 ] || fail "expected two outputs in the layer list, got $n"
  _multi_output_check "$multi_output_layers_two" "two outputs"
  echo "SMOKE_MULTI_OUTPUT wallpaper, bar and frame on both outputs, $multi_output_second at scale 1.25"

  "$jq_bin" -e --arg s "$multi_output_second" 'has($s) | not' "$multi_output_layers_gone" > /dev/null \
    || fail "$multi_output_second still listed after its removal"
  _multi_output_check "$multi_output_layers_gone" "after removal"
  echo "SMOKE_MULTI_OUTPUT_REMOVED the first output keeps its chrome once $multi_output_second is gone"

  cat "$multi_output_click"
  _multi_output_state() { sed -n "s/^state $1: //p" "$multi_output_click"; }
  [ "$(_multi_output_state open)" = network ] || fail "panel open network on the first output left panel state at '$(_multi_output_state open)'"
  n=$(_multi_output_count "$multi_output_layers_open" "$multi_output_second" "formalshell:catcher")
  [ "$n" = 1 ] || fail "with a panel open on the first output, $multi_output_second carries $n catchers, want 1"
  [ -z "$(_multi_output_state clicked)" ] || fail "a click on $multi_output_second left the panel open: '$(_multi_output_state clicked)'"
  n=$("$jq_bin" -r '[.[] | .levels[]?[]? | select(.namespace == "formalshell:catcher")] | length' "$multi_output_layers_closed")
  [ "$n" = 0 ] || fail "$n catchers still mapped once the panel closed"
  echo "SMOKE_MULTI_OUTPUT_CLICK a click on $multi_output_second closed the panel open on the first output"
  [ "$(_multi_output_state reopen)" = network ] || fail "the panel did not open again on the first output: '$(_multi_output_state reopen)'"
  n=$(_multi_output_count "$multi_output_layers_reopen" "$multi_output_second" "formalshell:panel")
  [ "$n" = 0 ] || fail "the reopened panel is on $multi_output_second, not the first output"
  [ "$(_multi_output_state cell)" = audio ] || fail "a click on $multi_output_second's audio cell left panel state at '$(_multi_output_state cell)', want audio"
  "$jq_bin" -e --arg s "$multi_output_second" '.[0].screen == $s' "$multi_output_room_after" > /dev/null \
    || fail "the live bar is not on $multi_output_second after its cell was clicked: $("$jq_bin" -c '.[0].screen' "$multi_output_room_after" 2>/dev/null)"
  echo "SMOKE_MULTI_OUTPUT_CELL a click on $multi_output_second's audio cell closed the first output's panel and opened audio there"
  echo "SMOKE_MULTI_OUTPUT_OPEN $multi_output_open_png"
  echo "SMOKE_MULTI_OUTPUT_CLOSED $multi_output_closed_png"
  echo "SMOKE_MULTI_OUTPUT_CELL_FRAME $multi_output_cell_png"

  for f in "$multi_output_first_png" "$multi_output_second_png"; do
    [ -s "$f" ] || fail "no frame at $f"
  done
  echo "SMOKE_MULTI_OUTPUT_FIRST $multi_output_first_png"
  echo "SMOKE_MULTI_OUTPUT_SECOND $multi_output_second_png"

  # The second output's bar unmapped under a fullscreen window and given a
  # window again, with the pointer never over it: every cell but the ones
  # the fullscreen window itself moved (workspaces) or time did (clock) has
  # to come back as it was.
  cat "$multi_output_hidden"
  grep -q '^bars under fullscreen: 0$' "$multi_output_hidden" || fail "the fullscreen window on $multi_output_second left its bar mapped"
  [ -s "$multi_output_unhidden_png" ] || fail "no frame of $multi_output_second once its fullscreen window closed"
  local name box diff cells=0
  while read -r name box; do
    diff=$($convert_bin "$multi_output_bare_png" -crop "$box" +repage "(" "$multi_output_unhidden_png" -crop "$box" +repage ")" -alpha off \
      -compose difference -composite -colorspace gray -threshold 10% -format '%[fx:mean]' info: 2>&1 | tail -1)
    echo "bar after fullscreen: $name $box, share changed ${diff:-?}"
    awk -v d="${diff:-1}" 'BEGIN { exit !(d + 0 < 0.3) }' \
      || fail "$multi_output_second's bar came back from fullscreen without its $name cell (${diff:-?} of $box changed)"
    cells=$((cells + 1))
  done < <("$jq_bin" -r '.[0].cells[] | select(.whole and .name != "workspaces" and .name != "clock")
    | "\(.name) \(.width * 1.25 | ceil)x\(.height * 1.25 | ceil)+\(.x * 1.25 | floor)+\(.y * 1.25 | floor)"' "$multi_output_room" 2>/dev/null)
  [ "$cells" -gt 0 ] || fail "no cells to compare in $multi_output_second's bar room: $(head -c 300 "$multi_output_room")"
  echo "SMOKE_MULTI_OUTPUT_UNHIDDEN $multi_output_unhidden_png"

  cat "$multi_output_saver"
  grep -q '^status shown: .*"active":true' "$multi_output_saver" || fail "screensaver start did not show it: $(grep '^status shown' "$multi_output_saver")"
  [ -s "$multi_output_saver_layers" ] || fail "no layer dump while the screensaver showed"
  n=$("$jq_bin" -r 'keys | length' "$multi_output_saver_layers")
  [ "$n" = 2 ] || fail "expected two outputs while the screensaver showed, got $n"
  for out in $("$jq_bin" -r 'keys[]' "$multi_output_saver_layers"); do
    n=$(_multi_output_count "$multi_output_saver_layers" "$out" "formalshell:screensaver")
    echo "screensaver: $out carries $n"
    [ "$n" = 1 ] || fail "$out carries $n formalshell:screensaver layers while it showed, want 1"
  done
  for f in "$multi_output_saver_first_png" "$multi_output_saver_second_png"; do
    [ -s "$f" ] || fail "no screensaver frame at $f"
    share=$(_multi_output_black "$f")
    echo "screensaver black share $(basename "$f"): $share"
    awk -v s="${share:-0}" 'BEGIN { exit !(s > 0.5) }' || fail "$(basename "$f") is ${share:-?} screensaver black, so the screensaver does not cover that output"
  done
  echo "SMOKE_MULTI_OUTPUT_SAVER_FIRST $multi_output_saver_first_png"
  echo "SMOKE_MULTI_OUTPUT_SAVER_SECOND $multi_output_saver_second_png"
  grep -q '^status moved: .*"active":false' "$multi_output_saver" || fail "a pointer move on $multi_output_second did not dismiss the screensaver: $(grep '^status moved' "$multi_output_saver")"
  n=$("$jq_bin" -r '[.[] | .levels[]?[]? | select(.namespace == "formalshell:screensaver")] | length' "$multi_output_saver_dismissed")
  [ "$n" = 0 ] || fail "$n screensaver layers still mapped after the pointer move dismissed it"
  echo "SMOKE_MULTI_OUTPUT_SAVER_DISMISS a pointer move on $multi_output_second took the screensaver off both outputs"
  n=$(_multi_output_count "$multi_output_layers_gone" "$("$jq_bin" -r 'keys[0]' "$multi_output_layers_gone")" "formalshell:screensaver")
  [ "$n" = 1 ] || fail "the first output carries $n screensaver layers once $multi_output_second went under it, want 1"
  grep -q '^status readd: .*"active":true' "$multi_output_saver" || fail "the screensaver stopped across the output's removal and return: $(grep '^status readd' "$multi_output_saver")"
  for out in $("$jq_bin" -r 'keys[]' "$multi_output_saver_readd"); do
    n=$(_multi_output_count "$multi_output_saver_readd" "$out" "formalshell:screensaver")
    echo "screensaver after re-adding: $out carries $n"
    [ "$n" = 1 ] || fail "$out carries $n formalshell:screensaver layers after $multi_output_second came back under it, want 1"
  done
  share=$(_multi_output_black "$multi_output_saver_readd_png")
  awk -v s="${share:-0}" 'BEGIN { exit !(s > 0.5) }' || fail "$multi_output_second came back under the screensaver but its frame is ${share:-?} black"
  echo "SMOKE_MULTI_OUTPUT_SAVER_READD $multi_output_saver_readd_png"

  grep -q '^locked: true$' "$multi_output_saver" || fail "lock lock did not lock: $(grep '^locked' "$multi_output_saver")"
  for f in "$multi_output_lock_first_png" "$multi_output_lock_second_png"; do
    [ -s "$f" ] || fail "no lock frame at $f"
    colors=$($convert_bin "$f" -format '%k' info: 2>/dev/null)
    echo "lock frame $(basename "$f"): $colors colours"
    [ "${colors:-0}" -gt 16 ] || fail "$(basename "$f") holds ${colors:-0} colours, so no lock surface drew on that output"
  done
  echo "SMOKE_MULTI_OUTPUT_LOCK_FIRST $multi_output_lock_first_png"
  echo "SMOKE_MULTI_OUTPUT_LOCK_SECOND $multi_output_lock_second_png"
  grep -q '^unlocked: false$' "$multi_output_saver" || fail "the lock over both outputs did not lift for the real password: $(grep '^unlocked' "$multi_output_saver")"
}
