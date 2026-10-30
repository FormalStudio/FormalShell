# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --menu-morph: the launcher's card takes the size of the level kind it
# shows and travels into it rather than jumping. The card opens at the root's
# `popupWidthMenu`, then `menu summon clipboard` re-levels it in place onto
# `popupWidthMenuSplit`, the width a selected row with a preview takes (one
# wl-copy seeds that row: an empty clipboard has no preview and stays 560),
# all at a tenth of its speed (set before the open: a window takes the scale
# it opened with) and photographed frame by frame.
#
# Read off one row through the card's header band: the card's own fill sits
# well above the scrim around it, so the first column bright enough is the
# card's left edge. The root and the rest frames have to land on the two
# tokens' edges (a card centred on the output), and at least one frame in
# between has to sit strictly between them: a card that jumped never does.
#
# Then the actions column: back at the root, a real Tab on the first row
# opens it and the card travels from 560 to 840 the same way, and a real
# Shift+Tab closes it and the card travels back, each read off the same
# row with a frame caught between the two widths.
leg_menu_morph_flag="--menu-morph"
leg_menu_morph_order=126
leg_menu_morph_needs="convert wtype wl-copy"

menu_morph_frames=24
menu_morph_gap=0.12
# The header band's row: the card's top sits at 30% of the 1080 output.
menu_morph_row=345
# A card centred on the 1920 output, 560 and 840 wide.
menu_morph_root_left=680
menu_morph_split_left=540

leg_menu_morph_validate() {
  local other
  for other in clipboard menu menu_emerge picker emoji monitor mirror; do
    if leg_on "$other"; then
      echo "usage: --menu-morph measures the launcher's own frames and cannot combine with --${other//_/-}" >&2
      exit 1
    fi
  done
}

leg_menu_morph_timing() {
  leg_timing 60 100
}

leg_menu_morph_drive() {
  local script="$shot_dir/menu-morph-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
call() { $ipc call "\$@"; }
sleep 4
"$wl_copy_bin" "menu morph preview text"
sleep 1
call debug motionScale 1000 > /dev/null 2>&1
call menu summon "" > /dev/null 2>&1
sleep 8
"$grim_bin" "$shot_dir/menu-morph-root.png" > /dev/null 2>&1
call menu summon clipboard > "$shot_dir/menu-morph-summon.txt" 2>&1
for i in \$(seq 1 $menu_morph_frames); do
  "$grim_bin" "$shot_dir/menu-morph-\$i.png" > /dev/null 2>&1
  sleep $menu_morph_gap
done
sleep 6
"$grim_bin" "$shot_dir/menu-morph-rest.png" > /dev/null 2>&1
call menu summon "" > /dev/null 2>&1
sleep 6
for phase in tab back; do
  if [ "\$phase" = tab ]; then "$wtype_bin" -k Tab; else "$wtype_bin" -M shift -k Tab -m shift; fi
  for i in \$(seq 1 $menu_morph_frames); do
    "$grim_bin" "$shot_dir/menu-morph-\$phase-\$i.png" > /dev/null 2>&1
    sleep $menu_morph_gap
  done
  sleep 6
  "$grim_bin" "$shot_dir/menu-morph-\$phase-rest.png" > /dev/null 2>&1
done
call debug motionScale 100 > /dev/null 2>&1
call menu close > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

# The first column left of the output's centre, scanning out from it, past
# which the row turns to scrim: the card's left edge.
menu_morph_left() {
  $convert_bin "$1" -crop "960x1+0+$menu_morph_row" +repage -colorspace Gray -depth 8 gray:- 2>/dev/null \
    | od -An -tu1 -v | tr -s ' ' '\n' | grep -v '^$' \
    | awk '{ v[NR - 1] = $1 } END { for (x = NR - 1; x >= 0; x--) if (v[x] <= 15) { print x + 1; exit } print 0 }'
}

leg_menu_morph_assert() {
  local root rest i left between=0 seen=""
  grep -q '^ok$' "$shot_dir/menu-morph-summon.txt" 2>/dev/null \
    || fail "menu summon clipboard did not answer ok: $(cat "$shot_dir/menu-morph-summon.txt" 2>/dev/null)"
  root=$(menu_morph_left "$shot_dir/menu-morph-root.png")
  rest=$(menu_morph_left "$shot_dir/menu-morph-rest.png")
  echo "card left edge: root $root, split rest $rest"
  [ "$(( root - menu_morph_root_left ))" -ge -2 ] && [ "$(( root - menu_morph_root_left ))" -le 2 ] \
    || fail "the root card's left edge is $root, not the popupWidthMenu card's $menu_morph_root_left"
  [ "$(( rest - menu_morph_split_left ))" -ge -2 ] && [ "$(( rest - menu_morph_split_left ))" -le 2 ] \
    || fail "the split card's left edge is $rest, not the popupWidthMenuSplit card's $menu_morph_split_left"
  for i in $(seq 1 $menu_morph_frames); do
    left=$(menu_morph_left "$shot_dir/menu-morph-$i.png")
    seen="$seen $left"
    if [ "$left" -gt $((menu_morph_split_left + 4)) ] && [ "$left" -lt $((menu_morph_root_left - 4)) ]; then
      between=$((between + 1))
    fi
  done
  echo "left edge per frame:$seen"
  [ "$between" -ge 1 ] || fail "no frame caught the card between its two widths: it jumped"
  echo "SMOKE_MENU_MORPH $shot_dir/menu-morph-rest.png ($between frames mid-morph)"
  local phase want
  for phase in tab back; do
    want=$menu_morph_split_left
    [ "$phase" = back ] && want=$menu_morph_root_left
    rest=$(menu_morph_left "$shot_dir/menu-morph-$phase-rest.png")
    [ "$(( rest - want ))" -ge -2 ] && [ "$(( rest - want ))" -le 2 ] \
      || fail "after the $phase key the card's left edge is $rest, not $want"
    between=0
    seen=""
    for i in $(seq 1 $menu_morph_frames); do
      left=$(menu_morph_left "$shot_dir/menu-morph-$phase-$i.png")
      seen="$seen $left"
      if [ "$left" -gt $((menu_morph_split_left + 4)) ] && [ "$left" -lt $((menu_morph_root_left - 4)) ]; then
        between=$((between + 1))
      fi
    done
    echo "$phase: left edge per frame:$seen, rest $rest"
    [ "$between" -ge 1 ] || fail "no frame caught the card between its widths after the $phase key: it jumped"
    echo "SMOKE_MENU_MORPH $shot_dir/menu-morph-$phase-rest.png ($between frames mid-morph)"
  done
}
