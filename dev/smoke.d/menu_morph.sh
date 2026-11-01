# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --menu-morph: the launcher's card keeps one width and takes the height of
# what it shows, travelling into it rather than jumping. The card opens on
# the root's three `Recent` rows (three route picks staged in state.json),
# content-tall, then `menu summon clipboard` re-levels it
# in place onto `popupHeightMenuSplit`, the list and its preview side by
# side inside the same width (one wl-copy seeds that row), all at a tenth
# of its speed (set before the open: a window takes the scale it opened
# with) and photographed frame by frame.
#
# Read off two lines: one row through the header band, where the first
# column bright enough left of centre is the card's left edge, and the
# output's centre column, where the first row past the header dark enough
# to be scrim is the card's bottom. The left edge sits on the 640 card's
# on every frame. The root and rest bottoms differ, and at least one frame
# in between has a bottom strictly between them: a card that jumped never
# does.
#
# Then the actions column: back at the root, a real Tab on the first row
# opens it and the card takes the split height the same way, and a real
# Shift+Tab closes it and the card travels back, each read off the same
# column with a frame caught between the two heights.
leg_menu_morph_flag="--menu-morph"
leg_menu_morph_order=126
leg_menu_morph_needs="convert wtype wl-copy"

menu_morph_frames=24
menu_morph_gap=0.12
# The header band's row, 21 logical pixels under the card's top, and the
# card's left edge, both in frame pixels off the split's `cardBox` at rest
# (the card keeps its x and top through every level).
menu_morph_row=0
menu_morph_left_edge=0

leg_menu_morph_validate() {
  local other
  for other in clipboard menu menu_emerge picker emoji monitor mirror; do
    if leg_on "$other"; then
      echo "usage: --menu-morph measures the launcher's own frames and cannot combine with --${other//_/-}" >&2
      exit 1
    fi
  done
}

# A short root: the Modes list at rest runs about the split height, which
# leaves no travel to read.
leg_menu_morph_fixture() {
  local state="$iso_home/.local/state/formalshell/state.json" now
  now=$(($(date +%s) * 1000))
  mkdir -p "${state%/*}"
  [ -e "$state" ] || printf '{"picks":[{"id":"clipboard","count":3,"lastMs":%s},{"id":"toggles","count":2,"lastMs":%s},{"id":"emoji","count":1,"lastMs":%s}]}\n' "$now" "$now" "$now" > "$state"
}

leg_menu_morph_timing() {
  # Four bursts of frames: the drive runs past the default 52s cap where
  # grim is slow, so the session waits it out.
  leg_timing 60 140 0 110
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
call menu status > "$shot_dir/menu-morph-rest.json" 2>&1
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
  $convert_bin "$1" -crop "$((out_w / 2))x1+0+$menu_morph_row" +repage -colorspace Gray -depth 8 gray:- 2>/dev/null \
    | od -An -tu1 -v | tr -s ' ' '\n' | grep -v '^$' \
    | awk '{ v[NR - 1] = $1 } END { for (x = NR - 1; x >= 0; x--) if (v[x] <= 15) { print x + 1; exit } print 0 }'
}

# Up the output's centre column from its foot, the first row bright enough
# to be card: the card's bottom. Read from below, since the card's own
# hairlines are darker than its fill.
menu_morph_bottom() {
  $convert_bin "$1" -crop "1x$((out_h - menu_morph_row))+$((out_w / 2))+$menu_morph_row" +repage -colorspace Gray -depth 8 gray:- 2>/dev/null \
    | od -An -tu1 -v | tr -s ' ' '\n' | grep -v '^$' \
    | awk -v top="$menu_morph_row" '{ v[NR - 1] = $1 } END { for (y = NR - 1; y >= 0; y--) if (v[y] > 15) { print top + y + 1; exit } print top }'
}

# Every frame of a burst: the left edge on the 640 card's, and how many
# bottoms fall strictly between `from` and `to`.
menu_morph_burst() {
  local prefix=$1 from=$2 to=$3 i left bottom lo hi between=0 seen=""
  lo=$(( from < to ? from : to )); hi=$(( from < to ? to : from ))
  for i in $(seq 1 $menu_morph_frames); do
    left=$(menu_morph_left "$shot_dir/$prefix-$i.png")
    bottom=$(menu_morph_bottom "$shot_dir/$prefix-$i.png")
    seen="$seen $bottom"
    [ "$(( left - menu_morph_left_edge ))" -ge -2 ] && [ "$(( left - menu_morph_left_edge ))" -le 2 ] \
      || fail "$prefix-$i: the card's left edge is $left, not the card at rest's $menu_morph_left_edge: its width moved"
    if [ "$bottom" -gt $((lo + 4)) ] && [ "$bottom" -lt $((hi - 4)) ]; then
      between=$((between + 1))
    fi
  done
  echo "$prefix: bottom per frame:$seen ($from to $to)"
  [ "$between" -ge 1 ] || fail "$prefix: no frame caught the card between its two heights: it jumped"
  echo "SMOKE_MENU_MORPH $prefix ($between frames mid-morph)"
}

leg_menu_morph_assert() {
  local root rest
  grep -q '^ok$' "$shot_dir/menu-morph-summon.txt" 2>/dev/null \
    || fail "menu summon clipboard did not answer ok: $(cat "$shot_dir/menu-morph-summon.txt" 2>/dev/null)"
  read -r menu_morph_left_edge menu_morph_row < <(scaled_jq -r '.cardBox | "\(.x * $s | floor) \((.y + 21) * $s | floor)"' "$shot_dir/menu-morph-rest.json" 2>/dev/null)
  [ -n "$menu_morph_row" ] || fail "no cardBox in the split's status at rest: $(cat "$shot_dir/menu-morph-rest.json" 2>/dev/null)"
  root=$(menu_morph_bottom "$shot_dir/menu-morph-root.png")
  rest=$(menu_morph_bottom "$shot_dir/menu-morph-rest.png")
  echo "card bottom: root $root, split rest $rest; status at rest: $("$jq_bin" -c '{level, view, cardWidth, cardBox, pane: (.pane != null)}' "$shot_dir/menu-morph-rest.json" 2>&1)"
  [ "$(( rest - root ))" -gt 40 ] || [ "$(( root - rest ))" -gt 40 ] || fail "the clipboard's split card (bottom $rest) is the root's height (bottom $root)"
  menu_morph_burst menu-morph "$root" "$rest"
  local tab back
  tab=$(menu_morph_bottom "$shot_dir/menu-morph-tab-rest.png")
  back=$(menu_morph_bottom "$shot_dir/menu-morph-back-rest.png")
  [ "$(( tab - root ))" -gt 40 ] || [ "$(( root - tab ))" -gt 40 ] || fail "Tab on the first row left the card's bottom at $tab, the root's $root"
  [ "$(( back - root ))" -ge -2 ] && [ "$(( back - root ))" -le 2 ] || fail "Shift+Tab left the card's bottom at $back, not back on the root's $root"
  menu_morph_burst menu-morph-tab "$root" "$tab"
  menu_morph_burst menu-morph-back "$tab" "$root"
  echo "SMOKE_MENU_MORPH $shot_dir/menu-morph-tab-rest.png"
}
