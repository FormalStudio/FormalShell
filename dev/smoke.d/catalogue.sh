# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --catalogue rides --cartridge and photographs its motion catalogue (spec
# C5) at a fifth of its speed (`debug motionScale 500`), one burst of frames
# back to back per effect, each followed by two rest frames a second apart:
#
#   menu-open      the launcher on the toggle hub: rows rising 40px in
#                  order, the grain over the fresh card fading
#   menu-focus     one real Down: the label leaving the cursor narrows, the
#                  one reaching it widens to 125, 700 and 0.04em
#   menu-use       `menu activate` on Do Not Disturb, the hub staying open:
#                  the row's ghost growing to (1.1, 1.3) as it fades
#   menu-swap      a real BackSpace on the empty query climbing to the root:
#                  the hub's rows jumping back and out, the root's coming in
#                  from ahead
#   panel-open     the audio panel: the status strip and the hero's title
#                  typing in, the rows rising, the grain fading
#   panel-zoom     the app menu over the fixture window: its picture landing
#                  from 20 times its size in two steps
#   panel-swap     a handoff from the app menu to audio: the body coming in
#                  from 20px ahead
#   osd            the volume pill as `█████░░░░░ 50%` blocks
#   lock-shake     a wrong password: the error label's axes jittering
#
# The frames are for reading; what is asserted is the half a reader cannot
# see in a still: that every burst moved (some frame differs from its rest),
# and that nothing moves at rest (the two rest frames are byte-identical
# below the bar, whose clock may tick between them).
leg_catalogue_flag="--catalogue"
leg_catalogue_order=600
leg_catalogue_needs="wtype convert jq"
leg_catalogue_fixture_window=keep

catalogue_bursts="menu-open menu-focus menu-use menu-swap panel-open panel-zoom panel-swap lock-shake"
catalogue_gap=0.05

leg_catalogue_validate() {
  if ! leg_on cartridge; then
    echo "usage: --catalogue reads the cartridge table's effects and needs --cartridge" >&2
    exit 1
  fi
}

leg_catalogue_timing() {
  leg_timing 150 220
}

leg_catalogue_drive() {
  local script="$shot_dir/catalogue-drive.sh" p="$shot_dir/catalogue"
  write_script "$script" <<EOF
#!/usr/bin/env bash
ipc() { $ipc call "\$@"; }
row_index() {
  local i=0 idx=""
  while [ "\$i" -lt 20 ]; do
    ipc menu status > "\$1" 2>&1
    idx=\$("$jq_bin" -r --arg id "\$2" '.ids | index(\$id) // empty' "\$1" 2>/dev/null)
    [ -n "\$idx" ] && break
    sleep 0.5
    i=\$((i + 1))
  done
  echo "\$idx"
}
# A burst of \$2 frames named after \$1, then its two rest frames once \$3
# seconds have let everything land.
burst() {
  local i
  for i in \$(seq -w 1 "\$2"); do
    "$grim_bin" "$p-\$1-\$i.png" > /dev/null 2>&1
    sleep $catalogue_gap
  done
  sleep "\$3"
  "$grim_bin" "$p-\$1-rest.png" > /dev/null 2>&1
  sleep 1
  "$grim_bin" "$p-\$1-rest2.png" > /dev/null 2>&1
}
sleep 5
ipc debug motionScale 500 > "$p-scale.txt" 2>&1

ipc menu summon toggles > /dev/null 2>&1
burst menu-open 24 8
"$wtype_bin" -k Down
burst menu-focus 10 2
idx=\$(row_index "$p-hub.json" toggles.dnd)
ipc menu activate "\$idx" > "$p-activate.txt" 2>&1
burst menu-use 10 2
"$wtype_bin" -k BackSpace
burst menu-swap 16 4
ipc menu status > "$p-root.json" 2>&1
ipc menu close > /dev/null 2>&1
sleep 5

ipc panel open audio > "$p-audio.txt" 2>&1
burst panel-open 24 9
ipc panel close > /dev/null 2>&1
sleep 5
ipc panel open appmenu > "$p-appmenu.txt" 2>&1
burst panel-zoom 14 5
ipc panel toggle audio > "$p-handoff.txt" 2>&1
burst panel-swap 12 4
ipc panel close > /dev/null 2>&1
sleep 5

ipc osd volume > /dev/null 2>&1
sleep 2.5
"$grim_bin" "$p-osd.png" > /dev/null 2>&1
sleep 4

ipc lock lock > /dev/null 2>&1
sleep 6
"$wtype_bin" "wrong-password"
"$wtype_bin" -k Return
burst lock-shake 36 4
"$wtype_bin" "formalshell-test"
"$wtype_bin" -k Return
sleep 3
ipc lock isLocked > "$p-locked-after.txt" 2>&1
ipc debug motionScale 100 > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_catalogue_assert() {
  local p="$shot_dir/catalogue" b f moved n
  grep -q '^ok$' "$p-scale.txt" 2>/dev/null || fail "debug motionScale did not answer ok: $(cat "$p-scale.txt" 2>/dev/null)"
  for b in $catalogue_bursts; do
    [ -f "$p-$b-rest.png" ] && [ -f "$p-$b-rest2.png" ] || fail "the $b burst left no rest frames"
    moved=0
    n=0
    for f in "$p-$b"-[0-9]*.png; do
      n=$((n + 1))
      cmp -s "$f" "$p-$b-rest.png" || moved=$((moved + 1))
      echo "SMOKE_CATALOGUE_$(echo "$b" | tr 'a-z-' 'A-Z_')_$(basename "$f" .png | sed 's/.*-//') $f"
    done
    echo "SMOKE_CATALOGUE_$(echo "$b" | tr 'a-z-' 'A-Z_')_REST $p-$b-rest.png"
    echo "catalogue $b: $moved of $n frames differ from rest"
    [ "$moved" -gt 0 ] || fail "nothing moved through the $b burst"
    local still
    still=$($convert_bin "$p-$b-rest.png" "$p-$b-rest2.png" -compose difference -composite -crop "${out_w}x$((out_h - 60))+0+60" +repage \
      -colorspace Gray -threshold 0 -format '%[fx:mean]' info: 2>/dev/null)
    echo "catalogue $b: rest frames differ over ${still:-?} of the output below the bar"
    awk -v v="${still:-1}" 'BEGIN { exit !(v == 0) }' || fail "the $b surface is still moving at rest"
  done
  [ -f "$p-osd.png" ] || fail "no OSD frame"
  echo "SMOKE_CATALOGUE_OSD $p-osd.png"
  grep -q '^false$' "$p-locked-after.txt" 2>/dev/null || fail "the real password did not unlock: $(cat "$p-locked-after.txt" 2>/dev/null)"
}
