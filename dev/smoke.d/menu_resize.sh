# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --menu-resize: the launcher's card and its content (a subsurface of the
# card) shown as one layout on every frame of a view change that resizes
# the card. The launcher opened on its root, the monitor view reached the
# way a user does (typing `sysmon` and a real Return, so the card's look is
# first drawn at the root's size and has to grow), a real Escape back to
# the root, then `menu summon monitor` again, each photographed in a burst
# at a fifth of the motion's speed (set before the open: a window takes the
# scale it opened with). Run it under FS_CPU_QUOTA as well: a starved shell
# draws the card in slices across loop turns, which is where the two came
# apart.
#
# Every frame is read for two things:
# - the scrim: a patch of desktop left of both cards and one right of them
#   within 3 levels of the same patches in the settled monitor frame;
# - the card enclosing its content: along three rows (header, body, the
#   monitor view's footer) and two columns (inside the monitor card, outside
#   the root one) the pixels brighter than the scrim form one run at most.
#   The card's own fill sits above the scrim, so a card that encloses what
#   is drawn on it is one run; content drawn past the card's border, or
#   over a card that is not there yet, is a second run beside it.
# The two settled frames have to read as one run along every probe too,
# and on the two cards' own widths, so a probe that reads nothing is no pass.
#
# Card fill under all the content: on every settled frame and the last
# frames of each burst, every pixel row and column of the content's box
# (`debug dump`'s `content` rect, inset past the card's rounded corners)
# shows the scrim's level neither for more than 3px at the box's edges
# nor for more than 80px inside it, wider than any icon or glyph the
# launcher draws dark. A look whose
# subsurfaces kept a stale size once showed a cross of bare desktop through
# the card, and a card stopping short of its content, on g815's GPU.
#
# Off the shell's own numbers as well: each settled card centred on the
# output within a pixel (`debug dump`'s card rect against the frame's own
# width).
leg_menu_resize_flag="--menu-resize"
leg_menu_resize_order=127
leg_menu_resize_needs="convert wtype jq"

menu_resize_frames=24
menu_resize_gap=0.04
menu_resize_rows="352 450 1016"
menu_resize_cols="560 1300"
# The probes skip the bar's band.
menu_resize_col_from=60
# Desktop patches clear of both cards: x y w h.
menu_resize_patches="80,150,100,100 1740,700,100,100"
# Cards centred on the 1920 output: the root's 640 card and the
# monitor view's popupWidthMenuApp (900), read along the middle row.
menu_resize_root_run="640 1280"
menu_resize_monitor_run="510 1410"

leg_menu_resize_validate() {
  local other
  for other in clipboard menu menu_emerge menu_morph picker emoji monitor mirror processes app_grid; do
    if leg_on "$other"; then
      echo "usage: --menu-resize measures the launcher's own frames and cannot combine with --${other//_/-}" >&2
      exit 1
    fi
  done
}

leg_menu_resize_timing() {
  leg_timing 36 80
}

leg_menu_resize_drive() {
  local script="$shot_dir/menu-resize-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
call() { $ipc call "\$@"; }
burst() {
  for i in \$(seq 1 $menu_resize_frames); do
    "$grim_bin" "$shot_dir/menu-resize-\$1-\$i.png" > /dev/null 2>&1
    sleep $menu_resize_gap
  done
}
sleep 4
call debug motionScale 500 > /dev/null 2>&1
call menu toggle > /dev/null 2>&1
sleep 6
"$grim_bin" "$shot_dir/menu-resize-root0.png" > /dev/null 2>&1
call debug dump > "$shot_dir/menu-resize-dump-root0.json" 2>&1
"$wtype_bin" sysmon
sleep 3
"$wtype_bin" -k Return
burst into
sleep 6
call menu status > "$shot_dir/menu-resize-monitor-status.json" 2>&1
"$grim_bin" "$shot_dir/menu-resize-monitor.png" > /dev/null 2>&1
call debug dump > "$shot_dir/menu-resize-dump-monitor.json" 2>&1
"$wtype_bin" -k Escape
burst back
sleep 6
call menu status > "$shot_dir/menu-resize-root-status.json" 2>&1
"$grim_bin" "$shot_dir/menu-resize-root.png" > /dev/null 2>&1
call debug dump > "$shot_dir/menu-resize-dump-root.json" 2>&1
call menu summon monitor > "$shot_dir/menu-resize-summon2.txt" 2>&1
burst forth
sleep 6
"$grim_bin" "$shot_dir/menu-resize-monitor2.png" > /dev/null 2>&1
call debug dump > "$shot_dir/menu-resize-dump-monitor2.json" 2>&1
call debug motionScale 100 > /dev/null 2>&1
call menu close > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

# The gray levels of one probe line: "row <y>" or "col <x>".
menu_resize_line() {
  local png=$1 kind=$2 at=$3 geom
  if [ "$kind" = row ]; then
    geom="1920x1+0+$at"
  else
    geom="1x$((1080 - menu_resize_col_from))+$at+$menu_resize_col_from"
  fi
  $convert_bin "$png" -crop "$geom" +repage -colorspace Gray -depth 8 gray:- 2>/dev/null \
    | od -An -tu1 -v | tr -s ' ' '\n' | grep -v '^$'
}

# The runs of pixels brighter than <thr> along a line, as "start-end"
# words, two runs closer than 3px read as one (antialiased edges).
menu_resize_runs() {
  awk -v thr="$1" '
    { v = $1; i = NR - 1
      if (v > thr) { if (open && i - last <= 3) { last = i } else { if (open) printf "%d-%d ", s, last; s = i; last = i; open = 1 } } }
    END { if (open) printf "%d-%d", s, last; print "" }'
}

# The mean gray of a patch "x,y,w,h".
menu_resize_patch() {
  local x y w h
  IFS=, read -r x y w h <<< "$2"
  $convert_bin "$1" -crop "${w}x${h}+${x}+${y}" +repage -colorspace Gray -format '%[fx:round(mean*255)]' info: 2>/dev/null
}

# The scrim's brightest level across the patches, plus a margin: the
# brightness past which a pixel is card or content.
menu_resize_threshold() {
  local png=$1 p v max=0
  for p in $menu_resize_patches; do
    v=$(menu_resize_patch "$png" "$p")
    [ "${v:-0}" -gt "$max" ] && max=$v
  done
  echo $((max + 5))
}

# Checks one frame against the scrim reference; prints its probe runs and
# a `BAD` line for a second run along any probe or a moved scrim.
menu_resize_frame() {
  local png=$1 label=$2 thr=$3 p v ref kind at runs n
  for p in $menu_resize_patches; do
    v=$(menu_resize_patch "$png" "$p")
    ref=$(menu_resize_patch "$shot_dir/menu-resize-monitor.png" "$p")
    if [ -z "$v" ] || [ "$(( v - ref ))" -gt 3 ] || [ "$(( ref - v ))" -gt 3 ]; then
      echo "BAD $label: the scrim patch $p reads ${v:-nothing}, the settled frame $ref: the scrim did not hold"
    fi
  done
  local out=""
  for kind in row col; do
    local list=$menu_resize_rows
    [ "$kind" = col ] && list=$menu_resize_cols
    for at in $list; do
      runs=$(menu_resize_line "$png" "$kind" "$at" | menu_resize_runs "$thr")
      n=$(wc -w <<< "$runs")
      out="$out $kind$at[$runs]"
      [ "$n" -le 1 ] || echo "BAD $label: $kind $at has $n runs past the scrim ($runs): content outside the card's border"
    done
  done
  echo "$label:$out"
}

# The rows and columns of the content's box in <png> showing the scrim
# rather than card fill: a `BAD` line when any does.
menu_resize_filled() {
  local png=$1 label=$2 thr=$3 dump=$4 box x y w h inset=24
  box=$("$jq_bin" -r '[.modals[] | select(.namespace == "formalshell:menu") | .content] | first | "\(.x) \(.y) \(.width) \(.height)"' "$dump" 2>/dev/null)
  read -r x y w h <<< "$box"
  if ! [[ "$x $y $w $h" =~ ^[0-9]+\ [0-9]+\ [0-9]+\ [0-9]+$ ]]; then
    echo "BAD $label: no content rect in $(basename "$dump") ($box)"
    return
  fi
  x=$((x + inset)) y=$((y + inset)) w=$((w - 2 * inset)) h=$((h - 2 * inset))
  $convert_bin "$png" -crop "${w}x${h}+${x}+${y}" +repage -colorspace Gray -depth 8 gray:- 2>/dev/null \
    | od -An -tu1 -v | tr -s ' ' '\n' | grep -v '^$' \
    | awk -v w="$w" -v h="$h" -v thr="$thr" -v label="$label" -v x0="$x" -v y0="$y" '
      { v[NR - 1] = $1 }
      # The longest run of scrim along n pixels from i, step s, when it
      # touches either end past 3px or runs past 80px inside; else 0.
      function scrim(i, s, n,   k, run, lead, worst) {
        run = 0; worst = 0; lead = -1
        for (k = 0; k < n; k++) {
          if (v[i + k * s] <= thr) { if (++run > worst) worst = run } else { if (lead < 0) lead = run; run = 0 }
        }
        if (lead < 0) lead = run
        return (lead > 3 || run > 3 || worst > 80) ? worst : 0
      }
      END {
        if (NR != w * h) { printf "BAD %s: read %d pixels of the %dx%d content box\n", label, NR, w, h; exit }
        rows = 0; cols = 0; first = ""
        for (r = 0; r < h; r++) {
          worst = scrim(r * w, 1, w)
          if (worst) { rows++; if (first == "") first = sprintf("row %d (%dpx of scrim)", y0 + r, worst) }
        }
        for (c = 0; c < w; c++) {
          worst = scrim(c, w, h)
          if (worst) { cols++; if (first == "") first = sprintf("col %d (%dpx of scrim)", x0 + c, worst) }
        }
        if (rows + cols > 0) printf "BAD %s: %d rows and %d columns of the content box %dx%d+%d+%d show the scrim, first %s: content with no card fill under it\n", label, rows, cols, w, h, x0, y0, first
        else printf "%s: card fill under every row and column of the content box %dx%d+%d+%d\n", label, w, h, x0, y0
      }'
}

leg_menu_resize_assert() {
  local thr f i mid report bad="" between=0
  "$jq_bin" -e '.level == "monitor"' "$shot_dir/menu-resize-monitor-status.json" > /dev/null 2>&1 \
    || fail "typing sysmon and Return did not open the monitor view: $(head -c 300 "$shot_dir/menu-resize-monitor-status.json" 2>/dev/null)"
  grep -q '^ok$' "$shot_dir/menu-resize-summon2.txt" 2>/dev/null \
    || fail "menu summon monitor did not answer ok: $(cat "$shot_dir/menu-resize-summon2.txt" 2>/dev/null)"
  thr=$(menu_resize_threshold "$shot_dir/menu-resize-monitor.png")
  echo "monitor status: $("$jq_bin" -c '{level, view, query, cardWidth, cardBox}' "$shot_dir/menu-resize-monitor-status.json" 2>&1)"
  for f in root0 monitor root monitor2; do
    echo "$f card: $("$jq_bin" -c '[.modals[] | select(.namespace == "formalshell:menu") | {card, content}] | first' "$shot_dir/menu-resize-dump-$f.json" 2>&1)"
  done
  echo "card threshold: gray > $thr"
  # Card fill under the content, settled and at the end of each resize.
  local filled
  for f in root0 monitor root monitor2; do
    filled=$(menu_resize_filled "$shot_dir/menu-resize-$f.png" "$f" "$thr" "$shot_dir/menu-resize-dump-$f.json")
    echo "$filled"
    grep -q "^BAD" <<< "$filled" && bad="$bad $f-fill"
  done
  for f in into:monitor back:root forth:monitor2; do
    for i in $((menu_resize_frames - 1)) $menu_resize_frames; do
      filled=$(menu_resize_filled "$shot_dir/menu-resize-${f%%:*}-$i.png" "${f%%:*}-$i" "$thr" "$shot_dir/menu-resize-dump-${f#*:}.json")
      echo "$filled"
      if grep -q "^BAD" <<< "$filled"; then
        bad="$bad ${f%%:*}-$i-fill"
        echo "SMOKE_MENU_RESIZE_UNFILLED_${f%%:*}_$i $shot_dir/menu-resize-${f%%:*}-$i.png"
      fi
    done
  done
  [ -z "$bad" ] || fail "content with no card fill under it:$bad"
  # The settled frames: one run each along the middle row, on the card's
  # own width.
  for f in root0:root monitor:monitor root:root monitor2:monitor; do
    local png="$shot_dir/menu-resize-${f%%:*}.png" want
    [ "${f#*:}" = root ] && want=$menu_resize_root_run || want=$menu_resize_monitor_run
    report=$(menu_resize_frame "$png" "${f%%:*}" "$thr")
    echo "$report"
    grep -q '^BAD' <<< "$report" && bad="$bad ${f%%:*}"
    mid=$(menu_resize_line "$png" row 450 | menu_resize_runs "$thr")
    [ "$(wc -w <<< "$mid")" -eq 1 ] || fail "${f%%:*}: the card spans '$mid' on row 450, not one run on the ${f#*:} card's $want"
    local s=${mid%-*} e=${mid#*-} ws=${want% *} we=${want#* }
    if [ "$(( s - ws ))" -lt -3 ] || [ "$(( s - ws ))" -gt 3 ] || [ "$(( e + 1 - we ))" -lt -3 ] || [ "$(( e + 1 - we ))" -gt 3 ]; then
      fail "${f%%:*}: the card spans '$mid' on row 450, not the ${f#*:} card's $want"
    fi
  done
  for f in into back forth; do
    for i in $(seq 1 $menu_resize_frames); do
      report=$(menu_resize_frame "$shot_dir/menu-resize-$f-$i.png" "$f-$i" "$thr")
      echo "$report"
      local start=${report#*row352\[}
      start=${start%%-*}
      if [ -n "$start" ] && [ "$start" -gt $((${menu_resize_monitor_run% *} + 4)) ] && [ "$start" -lt $((${menu_resize_root_run% *} - 4)) ]; then
        between=$((between + 1))
      fi
      if grep -q '^BAD' <<< "$report"; then
        bad="$bad $f-$i"
        echo "SMOKE_MENU_RESIZE_BAD_${f^^}_$i $shot_dir/menu-resize-$f-$i.png"
      fi
    done
  done
  # Centred, off the shell's own card rect.
  local out_w card
  for f in root0 monitor root monitor2; do
    out_w=$($convert_bin "$shot_dir/menu-resize-$f.png" -format '%w' info: 2>/dev/null)
    card=$("$jq_bin" -r '[.modals[] | select(.namespace == "formalshell:menu") | .card] | first | "\(.x) \(.width)"' \
      "$shot_dir/menu-resize-dump-$f.json" 2>/dev/null)
    local cx=${card% *} cw=${card#* }
    [ -n "$out_w" ] && [ -n "$cx" ] && [ "$cx" != null ] || fail "$f: no card rect in the dump or no frame width ($card, $out_w)"
    local off=$(( 2 * cx + cw - out_w ))
    [ "$off" -ge -2 ] && [ "$off" -le 2 ] || fail \
      "$f: the card rests at x $cx, ${cw} wide, on a ${out_w} wide output: $((off / 2))px off centre"
    echo "SMOKE_MENU_RESIZE_CENTRED $f x $cx width $cw output $out_w"
  done

  echo "frames caught between the two widths: $between"
  [ "$between" -ge 1 ] || fail "no frame caught the card between its two widths, so no frame of the resize was read"
  [ -z "$bad" ] || fail "frames with content outside the card or a moved scrim:$bad"
  echo "SMOKE_MENU_RESIZE $shot_dir/menu-resize-forth-4.png ($((menu_resize_frames * 3)) frames, $between mid-resize, one run along every probe)"
}
