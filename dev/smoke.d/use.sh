# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --use drives the shell the way a hand does rather than the way a leg
# usually does: real keys through real compositor binds (wtype), a real
# virtual pointer for moves, clicks and the wheel (wlrctl), fast and
# overlapping. No IPC call drives anything; `bar room`, `menu status` and
# `panel state` are only read, for where the cells are and what the shell
# believes is open at each checkpoint. Every step also runs a burst of
# frames (grim in a loop, ~50 a second here) into `use/<step>/`, named by
# the millisecond each grab started, for reading by eye.
#
# What it asserts, each one a glitch real use hit while every scripted leg
# passed (2026-10-09):
#   - A click on the bar's launcher cell opens the launcher: in a fresh
#     session, after Escape closed it, and over an open panel, each read off
#     `menu status` and off the card's fill in the frame. The cell's click
#     answered nothing at all.
#   - The launcher key opens the launcher after Escape closed it. Escape at
#     the root closed the card but left the launcher's model open, so the
#     next press "closed" an invisible launcher and the typing that followed
#     went to the focused window.
#   - Escape walks back one level and the launcher stays open on the root.
#   - A bar cell clicked while another cell's panel is open opens its own
#     panel. The panel's full-output surface sits over the bar and took the
#     click as a dismiss, so every second cell clicked in a row did nothing.
#   - Three fast clicks on one cell leave its panel open. The third lands
#     while the card is still leaving, through the strip.
#   - A theme mode flip under the open launcher and under an open panel
#     moves each card's fill with it. Both kept the old fill under the new
#     ink, so every word on them vanished.
#   - A wallpaper change or a theme mode flip never blanks the bar: every
#     frame of the strip through the recolour carries cells. The bar used to
#     rebuild every cell and grow each one out of nothing.
#
# Nothing backed by the system bus or the owner's audio is touched: on a
# real host the nested shell shares them, so the network, bluetooth, audio
# and battery cells, their panels and launcher levels are never opened, let
# alone driven. The clock and now playing cells carry the panel clicks, the
# wheel goes over the workspaces cell, and the launcher's levels are
# Keybinds and Emoji.
#
# The layout carries a chevron (chevron.sh's) so its second bar is part of
# the clicking. The fixture window stays: it is what a stray key lands in.
leg_use_flag="--use"
leg_use_order=400
leg_use_needs="wlrctl wtype jq notify-send convert"
leg_use_fixture_window=keep

use_dir="$shot_dir/use"
use_wp_a="$shot_dir/use-wp-a.png"
use_wp_b="$shot_dir/use-wp-b.png"

leg_use_validate() {
  local other
  for other in chevron bar_layout bar_room bar_title; do
    if leg_on "$other"; then
      echo "usage: --use pins its own bar.layout and cannot combine with --${other//_/-}" >&2
      exit 1
    fi
  done
}

leg_use_fixture() {
  settings_fragment ', "bar": {"layout": {"right": ["bluetooth", "weather", "tray", "bell", "indicators", "chevron", "battery", "audio", "network"]}}'
  $convert_bin -size 1920x1080 gradient:'#20304a-#6a3a5a' "$use_wp_a"
  $convert_bin -size 1920x1080 gradient:'#d8c890-#3a6a40' "$use_wp_b"
}

leg_use_timing() {
  leg_timing 155 200
}

leg_use_drive() {
  local script="$shot_dir/use-drive.sh"
  # resolve_binds_by_sym for the reason switcher_keys.sh gives: wtype ships
  # a keymap of its own.
  echo "hl.config({ input = { resolve_binds_by_sym = true } })"
  echo "hl.bind(\"SUPER + space\", hl.dsp.exec_cmd([==[$ipc call menu toggle]==]))"
  echo "hl.bind(\"SUPER + T\", hl.dsp.exec_cmd([==[$ipc call theme mode toggle]==]))"
  echo "hl.bind(\"SUPER + A\", hl.dsp.exec_cmd([==[$ipc call wallpaper set $use_wp_a]==]))"
  echo "hl.bind(\"SUPER + B\", hl.dsp.exec_cmd([==[$ipc call wallpaper set $use_wp_b]==]))"
  write_script "$script" <<EOF
#!/usr/bin/env bash
d="$use_dir"
mkdir -p "\$d"
now() { date +%s%3N; }
log() { echo "\$(now) \$*" >> "\$d/steps.txt"; }
burst() {
  local name="\$1" secs="\$2" end
  mkdir -p "\$d/\$name"
  end=\$(( \$(now) + secs * 1000 ))
  (while [ "\$(now)" -lt "\$end" ]; do
     "$grim_bin" -t jpeg -q 85 "\$d/\$name/\$(now).jpg" > /dev/null 2>&1
   done) &
}
key() { "$wtype_bin" "\$@"; }
super() { "$wtype_bin" -M logo -k "\$1" -m logo; }
home() { "$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1; }
to() { home; "$wlrctl_bin" pointer move "\$1" "\$2" > /dev/null 2>&1; }
click() { "$wlrctl_bin" pointer click left > /dev/null 2>&1; }
cell() { "$jq_bin" -r --arg n "\$1" '.[0].cells[] | select(.name == \$n) | "\(.x + .width / 2 | floor) \(.y + .height / 2 | floor)"' "\$d/room.json" | head -n 1; }
# One checkpoint: what the launcher and the panels hold, one line each.
check() {
  local m
  m=\$($ipc call menu status 2>/dev/null | "$jq_bin" -c '{isOpen, level}' 2>/dev/null)
  echo "\$1 menu=\$m panel=\$($ipc call panel state 2>/dev/null)" >> "\$d/checks.txt"
}
desktop() { to 900 700; click; }
patch_mean() { $convert_bin "\$1" -crop "\$2" +repage -colorspace gray -format '%[fx:int(mean*255)]' info: 2>/dev/null; }
# A mode flip under an open card: frames until the card's fill has moved,
# up to 12s, and how long that took. The palette comes off matugen, a child
# of the shell, so under FS_CPU_QUOTA it is slow by design.
flip() {
  local name="\$1" patch="\$2" t0 before mean
  t0=\$(now)
  before=\$(patch_mean "\$d/\$name-before.png" "\$patch")
  while [ \$(( \$(now) - t0 )) -lt 12000 ]; do
    "$grim_bin" "\$d/\$name-after.png"
    mean=\$(patch_mean "\$d/\$name-after.png" "\$patch")
    if [ -n "\$before" ] && [ -n "\$mean" ] && [ \$(( mean > before ? mean - before : before - mean )) -gt 60 ]; then break; fi
    sleep 0.2
  done
  echo "\$name \$(( \$(now) - t0 ))" >> "\$d/flip-ms.txt"
  sleep 0.5
}
sleep 6
$ipc call bar room > "\$d/room.json" 2>&1
# The pointer starts off every surface.
to 900 700
launcher=\$(cell launcher)

log launcher-click
burst launcher-click 3
"$grim_bin" "\$d/launcher-click-desktop.png"
to \$launcher; click; sleep 1.2; check launcher-click-fresh
"$grim_bin" "\$d/launcher-click-fresh.png"
key -k Escape; sleep 1.2; to 900 700

log launcher-spam
burst launcher-spam 3
for i in 1 2 3 4 5 6 7 8 9 10; do super space; sleep 0.08; done
sleep 2; check spam-even
super space; sleep 0.6; check spam-then-one
key -k Escape; sleep 1.2

log launcher-type
burst launcher-type 4
super space; sleep 0.15
key -d 15 "firefox settings"
sleep 0.3; key -k Escape
sleep 0.3; check type-cleared
key -k Escape
sleep 1; check type-closed
super space; sleep 0.6; check reopen-after-escape

log launcher-levels
burst launcher-levels 8
key -d 20 "keybinds"; sleep 0.3; key -k Return; sleep 0.6; check level-keybinds
key -k Escape; sleep 0.4; check level-back
key -d 20 "emoji"; sleep 0.3; key -k Return; sleep 0.6; check level-emoji
key -k BackSpace; sleep 0.4; check level-backspace
key -k Escape; sleep 1; check levels-closed
super space; sleep 0.6; check levels-reopen
key -k Escape; sleep 1.2

log launcher-click-again
burst launcher-click-again 6
to \$launcher; click; sleep 1.2; check launcher-click-escaped
"$grim_bin" "\$d/launcher-click-escaped.png"
key -k Escape; sleep 1.2
to \$(cell clock); click; sleep 0.8
to \$launcher; click; sleep 1.2; check launcher-click-panel
"$grim_bin" "\$d/launcher-click-panel.png"
key -k Escape; sleep 1; desktop; sleep 1.2

log cells-quick
burst cells-quick 7
n=0
for name in clock nowPlaying clock nowPlaying; do
  n=\$((n + 1))
  xy=\$(cell "\$name"); [ -n "\$xy" ] || continue
  log "click \$name \$xy"
  to \$xy; click; sleep 0.35
  check "click-\$n"
done
sleep 1; key -k Escape; sleep 1.2; check cells-closed

log cell-triple
clock=\$(cell clock)
burst cell-triple 10
# Three rounds: the third click of a round lands as the card leaves, and
# one round in five lost it while the panel's surface still covered the
# strip.
for r in 1 2 3; do
  to \$clock; click; sleep 0.1; click; sleep 0.1; click
  sleep 1.5; check "triple-\$r"
  key -k Escape; sleep 1.2
done

log hover-sweep
burst hover-sweep 5
home; "$wlrctl_bin" pointer move 0 18 > /dev/null 2>&1
for i in \$(seq 1 64); do "$wlrctl_bin" pointer move 30 0 > /dev/null 2>&1; sleep 0.04; done
sleep 1.5

log wheel-workspaces
burst wheel-workspaces 4
to \$(cell workspaces)
for i in 1 2; do "$wlrctl_bin" pointer scroll 15 0 > /dev/null 2>&1; sleep 0.15; done
sleep 0.8
for i in 1 2; do "$wlrctl_bin" pointer scroll -15 0 > /dev/null 2>&1; sleep 0.15; done
sleep 1.5
$ipc call workspaces status > "\$d/workspaces.json" 2>&1

log toasts-over-panel
burst toasts-over-panel 6
to \$clock; click; sleep 0.6
"$notify_send_bin" 'First' 'one'; sleep 0.3
"$notify_send_bin" 'Second' 'two'; sleep 0.3
"$notify_send_bin" -u critical 'Third' 'three'
sleep 2.5; check toasts
desktop; sleep 1.5; check toasts-dismissed

log panel-then-launcher
burst panel-then-launcher 4
to \$clock; click; sleep 0.1; super space
sleep 1.2; check panel-then-launcher
key -k Escape; sleep 1.2; check panel-then-launcher-escape
desktop; sleep 1.2

log theme-under-launcher
burst theme-under-launcher 10
super space; sleep 0.8
"$grim_bin" "\$d/launcher-before.png"
super t
flip launcher "$use_launcher_patch"
check theme-launcher
super b; sleep 4
check wallpaper-launcher
key -k Escape; sleep 1.2

log wallpaper-under-panel
burst wallpaper-under-panel 8
to \$clock; click; sleep 0.8
super a; sleep 5
check wallpaper-panel
"$grim_bin" "\$d/panel-before.png"
log theme-under-panel
super t
flip panel "$use_panel_patch"
check theme-panel
desktop; sleep 1.2

log chevron
chev=\$(cell chevron)
burst chevron 7
to \$chev; click; sleep 0.15; click; sleep 0.15; click
sleep 1.5
$ipc call bar chevron status > "\$d/chevron.json" 2>&1
"$hyprctl_bin" -j layers > "\$d/chevron-layers.json" 2>&1
to \$chev; click; sleep 1.2
$ipc call bar chevron status > "\$d/chevron-closed.json" 2>&1
log end
EOF
  hypr_exec_once "bash $script"
}

# The checkpoint line named $1, or nothing.
use_check() {
  grep "^$1 " "$use_dir/checks.txt" 2>/dev/null | head -n 1 || true
}

use_expect() {
  local name="$1" want="$2" what="$3" line
  line=$(use_check "$name")
  case "$line" in
    *"$want"*) echo "SMOKE_USE $what" ;;
    *) fail "$what: checkpoint $name wanted '$want', got '${line:-nothing}'" ;;
  esac
}

# The bar's strip in every frame of a burst: one carrying cells has ink
# across it, a blank band is one flat fill. Read off the spread of the
# strip's middle rows, which JPEG noise and the band's own gradient keep
# under 6 and a row of cells puts well past it.
use_blank_bar_frames() {
  local dir="$1" f sd out=""
  for f in "$dir"/*.jpg; do
    [ -f "$f" ] || continue
    sd=$($convert_bin "$f" -crop 1920x20+0+10 +repage -colorspace gray \
      -format '%[fx:int(standard_deviation*255)]' info: 2>/dev/null)
    if [ -n "$sd" ] && [ "$sd" -lt 6 ]; then out="$out $(basename "$f")"; fi
  done
  printf '%s' "$out"
}

# Bare fill on each card, clear of every word on it: right of the
# launcher's placeholder in its search row, and between the calendar's
# title and its header buttons. Both cards sit where the rig's one
# 1920x1080 output and the clock cell put them.
use_launcher_patch="160x14+990+346"
use_panel_patch="200x14+830+68"

# A mode flip under an open card has to move its fill from one end of the
# scale to the other. The card kept its old fill under the new ink, which
# reads as words vanishing into it.
use_flip() {
  local name="$1" patch="$2" what="$3" before after
  before=$($convert_bin "$use_dir/$name-before.png" -crop "$patch" +repage -colorspace gray -format '%[fx:int(mean*255)]' info: 2>/dev/null)
  after=$($convert_bin "$use_dir/$name-after.png" -crop "$patch" +repage -colorspace gray -format '%[fx:int(mean*255)]' info: 2>/dev/null)
  echo "$what fill across the mode flip: $before -> $after, settled in $(awk -v n="$name" '$1 == n { print $2 }' "$use_dir/flip-ms.txt" 2>/dev/null)ms"
  [ -n "$before" ] && [ -n "$after" ] || fail "could not read $what off $name-before/after.png"
  local delta=$((after - before))
  [ "${delta#-}" -gt 60 ] || fail "$what kept its fill across a theme mode flip ($before -> $after)"
  echo "SMOKE_USE $what took the new mode's fill"
}

# Each frame grabbed after a click on the launcher cell differs from the
# desktop grabbed before the first click over the box the card rests in on
# the rig's 1920x1080 output: its icons and words move the mean difference
# well past the few hundredths a cursor or a clock tick does.
use_launcher_box="560x520+680+325"
use_launcher_frames() {
  local f rmse
  for f in launcher-click-fresh launcher-click-escaped launcher-click-panel; do
    [ -f "$use_dir/$f.png" ] || fail "no $f.png to read the launcher card off"
    rmse=$($convert_bin "$use_dir/$f.png" "$use_dir/launcher-click-desktop.png" -crop "$use_launcher_box" +repage \
      -compose difference -composite -colorspace gray -format '%[fx:int(mean*1000)]' info: 2>/dev/null)
    echo "launcher box against the desktop: $f=$rmse/1000"
    [ -n "$rmse" ] && [ "$rmse" -gt 30 ] || fail "$f.png does not show the launcher card over the desktop (difference $rmse/1000)"
    echo "SMOKE_USE $f.png shows the card"
  done
}

leg_use_assert() {
  # Packed first, so a failing run still hands every frame back
  # (dev/remote.sh pulls and unpacks a SMOKE_ *.tar).
  cp "$shell_log_path" "$use_dir/shell.log" 2>/dev/null || true
  tar -cf "$shot_dir/use.tar" -C "$use_dir" . 2>/dev/null || true
  echo "SMOKE_USE_FRAMES $shot_dir/use.tar"
  echo "SMOKE_USE_DIR $use_dir"
  [ -s "$use_dir/checks.txt" ] || fail "--use wrote no checkpoints"
  cat "$use_dir/checks.txt"
  use_expect launcher-click-fresh '"isOpen":true' "a click on the launcher cell opens the launcher in a fresh session"
  use_expect launcher-click-escaped '"isOpen":true' "and after Escape closed it"
  use_expect launcher-click-panel '"isOpen":true' "and over an open panel"
  use_launcher_frames
  use_expect spam-even '"isOpen":false' "ten fast launcher presses leave it shut"
  use_expect spam-then-one '"isOpen":true' "one more press opens it"
  use_expect type-cleared '"isOpen":true' "Escape over a query clears it and keeps the launcher"
  use_expect type-closed '"isOpen":false' "a second Escape closes it"
  use_expect reopen-after-escape '"isOpen":true' "the launcher key opens it again after Escape closed it"
  use_expect level-keybinds '"level":"keybinds"' "Enter on the Keybinds row enters its level"
  use_expect level-back '"isOpen":true,"level":null' "Escape climbs back to the root, launcher open"
  use_expect level-emoji '"level":"emoji"' "Enter on the Emoji row enters its level"
  use_expect level-backspace '"isOpen":true,"level":null' "Backspace on an empty query climbs back"
  use_expect levels-closed '"isOpen":false' "Escape at the root closes it"
  use_expect levels-reopen '"isOpen":true' "and the key opens it again"
  use_expect click-1 'panel=calendar' "the clock opens the calendar"
  use_expect click-2 'panel=media' "the now playing cell, clicked over it, opens the media panel"
  use_expect click-3 'panel=calendar' "the clock, clicked over that, opens the calendar again"
  use_expect click-4 'panel=media' "and the media panel once more"
  use_expect triple-1 'panel=calendar' "three fast clicks on the clock leave the calendar open"
  use_expect triple-2 'panel=calendar' "a second round does too"
  use_expect triple-3 'panel=calendar' "and a third"
  use_expect toasts 'panel=calendar' "toasts arriving leave the open panel alone"
  use_expect panel-then-launcher '"isOpen":true' "the launcher opens over a panel mid-entrance"
  use_expect theme-launcher '"isOpen":true' "a theme flip keeps the launcher open"
  use_expect wallpaper-launcher '"isOpen":true' "a wallpaper change keeps the launcher open"
  use_expect wallpaper-panel 'panel=calendar' "a wallpaper change keeps the panel open"
  "$jq_bin" -e '.regions.right.open == true' "$use_dir/chevron.json" > /dev/null 2>&1 \
    || fail "three fast clicks on the chevron left its second bar shut: $(cat "$use_dir/chevron.json" 2>/dev/null)"
  echo "SMOKE_USE three fast clicks on the chevron leave its second bar open"
  if grep -q 'formalshell:tooltip' "$use_dir/chevron-layers.json" 2>/dev/null; then
    fail "the chevron's tooltip hangs over the second bar it opened"
  fi
  echo "SMOKE_USE no tooltip over the open second bar"
  "$jq_bin" -e '.regions.right.open == false' "$use_dir/chevron-closed.json" > /dev/null 2>&1 \
    || fail "a fourth click left the second bar open: $(cat "$use_dir/chevron-closed.json" 2>/dev/null)"
  echo "SMOKE_USE a fourth click shuts it"
  local blank b
  for b in theme-under-launcher wallpaper-under-panel; do
    blank=$(use_blank_bar_frames "$use_dir/$b")
    [ -z "$blank" ] || fail "the bar went blank through the $b recolour on:$blank"
    echo "SMOKE_USE the bar kept its cells through $b"
  done
  use_expect theme-panel 'panel=calendar' "a theme flip keeps the panel open"
  use_flip launcher "$use_launcher_patch" "the launcher's card"
  use_flip panel "$use_panel_patch" "the calendar's card"
  echo "slow presents: $(grep -c '^event loop: slow present' "$shell_log_path" 2>/dev/null || true)"
}
