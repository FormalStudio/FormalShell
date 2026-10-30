# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --menu-hover: the launcher's hover tint and selection fill under a real
# pointer and real keys (launcher spec §2, §6). The pointer on cell 20
# leaves `cursor` on cell 0 and sets `hover` to 20, and one cell right
# hands `hover` to 21; the hover tint differs from the rest fill and from
# the selection fill. Moving onto the cell and off it again, no frame of
# it is darker than its rest or its hover tint: a fill crossfading out of
# `transparent` through a straight lerp drags the colour through black at
# half alpha, the dark flash this leg was first written for. A real Right
# then moves the one selection fill the body draws from cell 0 to cell 1:
# on some frame its left edge sits part way between the two, rather than
# one cell fading out while the other fades in.
#
# The emoji grid is the level: its cells carry a glyph centred in a square,
# so a patch inside a cell's left edge is the cell's own fill and nothing
# else. Where the cells are comes off `menu status` (`body`, `viewCursor`),
# read with the cursor on the first cell. The pointer is slammed into the
# corner before the open (wlrctl's pointer is relative only), then moved in
# one event onto the third row's fifth cell and nudged a pixel, and later
# one cell right. Everything is sampled at a tenth speed.
leg_menu_hover_flag="--menu-hover"
leg_menu_hover_order=128
leg_menu_hover_needs="wlrctl wtype convert jq"

menu_hover_frames=24
menu_hover_target=20

leg_menu_hover_validate() {
  local other
  for other in clipboard menu menu_emerge menu_morph menu_rows picker emoji monitor mirror toggles wheel; do
    if leg_on "$other"; then
      echo "usage: --menu-hover measures the launcher's own frames and cannot combine with --${other//_/-}" >&2
      exit 1
    fi
  done
}

leg_menu_hover_timing() {
  leg_timing 40 90
}

leg_menu_hover_drive() {
  local script="$shot_dir/menu-hover-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
call() { $ipc call "\$@"; }
burst() {
  for i in \$(seq 1 $menu_hover_frames); do
    "$grim_bin" -g "\$box" "$shot_dir/menu-hover-\$1-\$i.png" > /dev/null 2>&1
    sleep 0.08
  done
}
sleep 4
"$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1
call debug motionScale 1000 > /dev/null 2>&1
call menu summon emoji > /dev/null 2>&1
sleep 8
call menu status > "$shot_dir/menu-hover-open.json" 2>&1
read -r bx by top bottom left right < <("$jq_bin" -r '[.body.x, .body.y, .viewCursor.top, .viewCursor.bottom, .viewCursor.left, .viewCursor.right] | @tsv' "$shot_dir/menu-hover-open.json")
cw=\$((right - left)); ch=\$((bottom - top))
columns=\$("$jq_bin" -r '.columns' "$shot_dir/menu-hover-open.json")
row=\$(($menu_hover_target / columns)); col=\$(($menu_hover_target % columns))
x=\$((bx + left + col * cw)); y=\$((by + top + row * ch))
box="\$x,\$y \${cw}x\${ch}"
echo "\$x \$y \$cw \$ch" > "$shot_dir/menu-hover-cell.txt"
"$grim_bin" -g "\$box" "$shot_dir/menu-hover-rest.png" > /dev/null 2>&1
"$grim_bin" -g "\$((bx + left)),\$((by + top)) \${cw}x\${ch}" "$shot_dir/menu-hover-selected.png" > /dev/null 2>&1
"$wlrctl_bin" pointer move \$((x + cw / 2)) \$((y + ch / 2)) > /dev/null 2>&1
"$wlrctl_bin" pointer move 1 0 > /dev/null 2>&1
burst in
sleep 8
"$grim_bin" -g "\$box" "$shot_dir/menu-hover-hover.png" > /dev/null 2>&1
call menu status > "$shot_dir/menu-hover-hovered.json" 2>&1
"$wlrctl_bin" pointer move \$cw 0 > /dev/null 2>&1
burst out
sleep 8
"$grim_bin" -g "\$box" "$shot_dir/menu-hover-left.png" > /dev/null 2>&1
call menu status > "$shot_dir/menu-hover-moved.json" 2>&1
"$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1
sleep 8
pair="\$((bx + left)),\$((by + top)) \$((cw * 2))x\${ch}"
"$grim_bin" -g "\$pair" "$shot_dir/menu-hover-keys-from.png" > /dev/null 2>&1
"$wtype_bin" -k Right
for i in \$(seq 1 $menu_hover_frames); do
  "$grim_bin" -g "\$pair" "$shot_dir/menu-hover-keys-\$i.png" > /dev/null 2>&1
  sleep 0.08
done
sleep 8
"$grim_bin" -g "\$pair" "$shot_dir/menu-hover-keys-to.png" > /dev/null 2>&1
call menu status > "$shot_dir/menu-hover-keys.json" 2>&1
call debug motionScale 100 > /dev/null 2>&1
"$grim_bin" "$shot_dir/menu-hover.png" > /dev/null 2>&1
call menu close > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

# Mean luminance of a cell's own fill: a 6px patch 6px inside its left
# edge, halfway down, clear of the glyph and of the corner's radius. `$2`
# is the cell's left edge inside the crop, for the pair the keys move over.
menu_hover_luma() {
  local h
  h=$(cut -d' ' -f4 "$shot_dir/menu-hover-cell.txt")
  "$convert_bin" "$1" -crop "6x6+$((${2:-0} + 6))+$((h / 2 - 3))" +repage -colorspace Gray -format '%[fx:mean]' info: 2>/dev/null
}

# Every sample of one crossing between its two ends, less 1% of slack for
# rounding. The fill travels in from the cell the cursor left rather than
# fading in place, so a 6px patch flips as its edge passes: the samples
# between the ends are counted, not required.
menu_hover_crossing() {
  local name="$1" from="$2" to="$3" i v lo eased=0 dark=""
  lo=$(awk -v a="$from" -v b="$to" 'BEGIN { print (a < b ? a : b) }')
  for i in $(seq 1 $menu_hover_frames); do
    v=$(menu_hover_luma "$shot_dir/menu-hover-$name-$i.png")
    [ -n "$v" ] || fail "no $name sample $i"
    printf '%s %s %s\n' "$name" "$i" "$v"
    if awk -v v="$v" -v lo="$lo" 'BEGIN { exit !(v < lo - 0.01) }'; then
      dark="$dark $i"
    fi
    if awk -v v="$v" -v a="$from" -v b="$to" 'BEGIN { d = 0.004; exit !((v > a + d && v < b - d) || (v < a - d && v > b + d)) }'; then
      eased=$((eased + 1))
    fi
  done
  [ -z "$dark" ] || fail "the $name crossing drew a fill darker than both ends ($from, $to) on samples$dark: $shot_dir/menu-hover-$name-*.png"
  echo "$name: $eased of $menu_hover_frames samples between $from and $to, none darker"
}

# Where the fill is along one line of the pair, 8px under its top (clear of
# the glyphs): the first and last column closer to the fill's luminance `$3`
# than to the rest's `$2`, or -1 -1 when none is.
menu_hover_span() {
  "$convert_bin" "$1" -crop "x1+0+8" +repage -colorspace Gray -depth 8 -compress none pgm:- 2>/dev/null \
    | tr -s ' \n' '\n\n' | awk -v rest="$2" -v sel="$3" '
      NF && ++n > 4 { x = n - 5; v = $1 / 255; if ((v - rest) ^ 2 > (v - sel) ^ 2) { if (l == "") l = x; r = x } }
      END { print (l == "" ? -1 : l), (r == "" ? -1 : r) }'
}

# A real Right moving the selection from cell 0 to cell 1, the pair sampled at a tenth speed: on some frame the fill's left edge stops
# part way between the two cells, the one fill travelling rather than one
# cell fading out while the other fades in.
menu_hover_keys() {
  local w rest sel b1 la lb i l r a b between=0 dark=""
  w=$(cut -d' ' -f3 "$shot_dir/menu-hover-cell.txt")
  "$jq_bin" -e ".cursor == 1" "$shot_dir/menu-hover-keys.json" > /dev/null \
    || fail "Right did not move the cursor from cell 0 onto cell 1: $(cat "$shot_dir/menu-hover-keys.json")"
  sel=$(menu_hover_luma "$shot_dir/menu-hover-keys-from.png")
  rest=$(menu_hover_luma "$shot_dir/menu-hover-keys-from.png" "$w")
  b1=$(menu_hover_luma "$shot_dir/menu-hover-keys-to.png" "$w")
  la=$(awk -v a="$rest" -v b="$sel" 'BEGIN { print (a < b ? a : b) }')
  lb=$(awk -v a="$rest" -v b="$b1" 'BEGIN { print (a < b ? a : b) }')
  read -r l r < <(menu_hover_span "$shot_dir/menu-hover-keys-from.png" "$rest" "$sel")
  echo "keys: rest $rest, fill $sel, the fill spans $l..$r before the move"
  for i in $(seq 1 $menu_hover_frames); do
    read -r l r < <(menu_hover_span "$shot_dir/menu-hover-keys-$i.png" "$rest" "$sel")
    a=$(menu_hover_luma "$shot_dir/menu-hover-keys-$i.png")
    b=$(menu_hover_luma "$shot_dir/menu-hover-keys-$i.png" "$w")
    echo "keys $i: the fill spans $l..$r, patches $a $b"
    if [ "$l" -gt 6 ] && [ "$l" -lt $((w - 6)) ]; then
      between=$((between + 1))
    fi
    if awk -v a="$a" -v b="$b" -v la="$la" -v lb="$lb" 'BEGIN { exit !(a < la - 0.01 || b < lb - 0.01) }'; then
      dark="$dark $i"
    fi
  done
  read -r l r < <(menu_hover_span "$shot_dir/menu-hover-keys-to.png" "$rest" "$sel")
  echo "keys: the fill spans $l..$r after the move"
  [ -z "$dark" ] || fail "a key move drew a fill darker than its ends on samples$dark: $shot_dir/menu-hover-keys-*.png"
  [ "$between" -ge 1 ] || fail "no frame with the fill between the two cells, the selection jumped on a real Right: $shot_dir/menu-hover-keys-*.png"
  echo "keys: $between of $menu_hover_frames samples with the fill between the two cells"
}

leg_menu_hover_assert() {
  local rest hover left selected
  for f in open hovered moved; do
    [ -s "$shot_dir/menu-hover-$f.json" ] || fail "no menu status at $f"
  done
  [ -s "$shot_dir/menu-hover-cell.txt" ] || fail "the drive never found the target cell"
  cat "$shot_dir/menu-hover-cell.txt"
  "$jq_bin" -e ".cursor == 0" "$shot_dir/menu-hover-open.json" > /dev/null \
    || fail "the emoji grid did not open on cell 0: $(cat "$shot_dir/menu-hover-open.json")"
  "$jq_bin" -e ".cursor == 0 and .hover == $menu_hover_target" "$shot_dir/menu-hover-hovered.json" > /dev/null \
    || fail "the pointer on cell $menu_hover_target did not hover it and leave the cursor on 0: $("$jq_bin" -c '{cursor, hover}' "$shot_dir/menu-hover-hovered.json")"
  "$jq_bin" -e ".cursor == 0 and .hover == $((menu_hover_target + 1))" "$shot_dir/menu-hover-moved.json" > /dev/null \
    || fail "one cell on, the hover did not follow or the cursor moved: $("$jq_bin" -c '{cursor, hover}' "$shot_dir/menu-hover-moved.json")"
  rest=$(menu_hover_luma "$shot_dir/menu-hover-rest.png")
  hover=$(menu_hover_luma "$shot_dir/menu-hover-hover.png")
  left=$(menu_hover_luma "$shot_dir/menu-hover-left.png")
  selected=$(menu_hover_luma "$shot_dir/menu-hover-selected.png")
  echo "rest $rest, hovered $hover, left $left, selected $selected"
  awk -v a="$rest" -v b="$hover" 'BEGIN { exit !(a - b > 0.01 || b - a > 0.01) }' \
    || fail "hovering left the cell's fill where it was ($rest, $hover)"
  awk -v a="$selected" -v b="$hover" 'BEGIN { exit !(a - b > 0.01 || b - a > 0.01) }' \
    || fail "the hover tint reads as the selection fill ($hover, $selected)"
  menu_hover_crossing in "$rest" "$hover"
  menu_hover_crossing out "$hover" "$left"
  menu_hover_keys
  echo "SMOKE_MENU_HOVER $shot_dir/menu-hover-hover.png (the pointer tints without moving the selection, the fill travels on keys, no frame darker than its ends)"
}
