# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --tray-overflow reads back where the tray lives with nothing configured at
# all, which since 2026-08-28 is the second bar: the strip carries the dots
# toggle and no icons, whatever room it has (`maxVisible` 0
# default). What has to be true is that the tray moved WHOLE, nothing of it
# left on the strip but the toggle and every registered item reachable in the
# second bar, and that the bar itself opens as a real panel, the
# `trayoverflow` panel in the panel host like any other.
# `tray.maxVisible: -1` is the other half of that key, the strip carrying
# what fits; --tray pins it and reads the six-cell strip back.
# The unit tests pin the arithmetic behind both.
#
# It also puts the tray behind a bar chevron, alone on its governed side.
# The chevron opens quick settings, where a governed tray is an info tile
# (docs/superpowers/specs/2026-11-04-quick-settings.md §7, §11): the tile
# reads the tray's item count, and a real click on it hands the card over
# to this second bar. The governed tray drawing nothing until it has
# measured shipped once (the chevron opened with no tray in it), with
# `tray status` reporting the collapse correctly the
# whole time, which is why the claim here is the tile and a picture rather
# than a reply.
#
# Last, settings.json rewritten to put the dots alone on the strip beside
# the clock, and the second bar under a real pointer behaving like a panel:
# a click on the desktop shuts it, and a click on the
# clock shuts it and opens the calendar in its place.
#
# Ridden by --sidebar there is no strip and no chevron: the tray button on
# the status card stands in for the dots, so the chevron round trip is
# skipped and the four real clicks start on that button, the handoff
# landing on the time card's calendar. Then the second bar opened once more
# by a real click and a sticky toast sent under it: the toast column is the
# bottom-left one beside the column, the same stretch the card hangs in
# (an Activity toast must not draw over the bar), so the card's
# rest rect and every toast card's rect in `debug dump` share x and must
# never intersect.
#
# Fixture 6 registers twice, two processes sharing one `Id` the way two
# copies of one app do (two Beeper windows, both cells keyed by that `Id`,
# must leave no empty cell in the second bar): the second bar has to
# draw both pictures.
#
# It owns bar.layout, so it does not combine with --bar-layout, --bar-position
# or --chevron, and it registers stubs of its own, so --tray's own count
# assert does not survive that pair either.
leg_tray_overflow_flag="--tray-overflow"
leg_tray_overflow_order=172
leg_tray_overflow_needs="python3 jq wlrctl notify-send"
leg_tray_overflow_claims="bar.layout panel=shared"

tray_overflow_pids_path="$shot_dir/tray-overflow-pids.txt"
tray_overflow_status_path="$shot_dir/tray-overflow-status.json"
tray_overflow_open_path="$shot_dir/tray-overflow-open.json"
tray_overflow_layers_path="$shot_dir/tray-overflow-layers.json"
tray_overflow_strip_path="$shot_dir/tray-overflow-strip.png"
tray_overflow_bar_path="$shot_dir/tray-overflow-bar.png"
tray_overflow_bar_dump_path="$shot_dir/tray-overflow-bar-dump.json"
tray_overflow_collapsed_path="$shot_dir/tray-overflow-collapsed.png"
tray_overflow_expanded_path="$shot_dir/tray-overflow-expanded.png"
tray_overflow_menu_reply_path="$shot_dir/tray-overflow-menu-reply.txt"
tray_overflow_menu_path="$shot_dir/tray-overflow-menu.json"
tray_overflow_menu_shot_path="$shot_dir/tray-overflow-menu.png"
tray_overflow_room_path="$shot_dir/tray-overflow-room.json"
tray_overflow_clicks_path="$shot_dir/tray-overflow-clicks.txt"
tray_overflow_outside_path="$shot_dir/tray-overflow-outside.png"
tray_overflow_handoff_path="$shot_dir/tray-overflow-handoff.png"
tray_overflow_toast_path="$shot_dir/tray-overflow-toast.png"
tray_overflow_toast_dump_path="$shot_dir/tray-overflow-toast-dump.json"
tray_overflow_quick_path="$shot_dir/tray-overflow-quick.json"
tray_overflow_tile_panel_path="$shot_dir/tray-overflow-tile-panel.txt"
tray_overflow_tile_path="$shot_dir/tray-overflow-tile.png"

leg_tray_overflow_fixture() {
  # The chevron governs what precedes it in a right region, so the tray is the
  # whole of its governed side and `clock`, outboard of it, is what keeps the
  # two frames from differing by anything else.
  settings_pin '. * {"bar": {"layout": {"left": [], "center": [], "right": ["tray", "chevron", "clock"]}}}'
}

leg_tray_overflow_timing() {
  # The drive's last step lands ~26s in (~30s under --sidebar, with the
  # toast case); the run's own frame is taken past it, with the second bar
  # closed.
  leg_timing 80
}

leg_tray_overflow_drive() {
  local script="$shot_dir/tray-overflow-drive.sh"
  local kill_script="$shot_dir/tray-overflow-kill.sh"
  local stub="$PWD/dev/sni-stub.py"
  local tail_steps
  if leg_on sidebar; then
    tail_steps=$(cat <<STEPS
sidebar_at() { $ipc call sidebar status 2>/dev/null | "$jq_bin" -r --arg on "\$1" --argjson ox "\$ox" --argjson oy "\$oy" '.hits[] | select(.on == \$on) | "\(\$ox + .x + (.w / 2 | floor)) \(\$oy + .y + (.h / 2 | floor))"' | head -n 1; }
origin=\$("$wmctl_bin" -j layers | "$jq_bin" -r "\$layers_jq"'bar_origin')
read -r ox oy <<< "\$origin"
check() { echo "\$1 open=\$($ipc call tray status 2>/dev/null | "$jq_bin" -c '.overflow.open') panel=\$($ipc call panel state 2>/dev/null)" >> "$tray_overflow_clicks_path"; }
park \$(sidebar_at panel:trayoverflow); click; sleep 1.5; check clicked-open
park 900 700; click; sleep 1.5; check outside
"$grim_bin" "$tray_overflow_outside_path" > /dev/null 2>&1
park \$(sidebar_at panel:trayoverflow); click; sleep 1.5; check reopened
park \$(sidebar_at panel:calendar); click; sleep 1.5; check handoff
"$grim_bin" "$tray_overflow_handoff_path" > /dev/null 2>&1
park \$(sidebar_at panel:trayoverflow); click; sleep 1.5
"$notify_send_bin" -u critical 'Applying g815' 'system scope apply failed'
park 900 400
sleep 3
$ipc call debug dump > "$tray_overflow_toast_dump_path" 2>&1
"$grim_bin" "$tray_overflow_toast_path" > /dev/null 2>&1
$ipc call panel close > /dev/null 2>&1
STEPS
)
  else
    tail_steps=$(cat <<STEPS
"$grim_bin" "$tray_overflow_collapsed_path" > /dev/null 2>&1
$ipc call bar chevron expand > /dev/null 2>&1
sleep 2
$ipc call quicksettings status > "$tray_overflow_quick_path" 2>&1
"$grim_bin" "$tray_overflow_expanded_path" > /dev/null 2>&1
park \$("$jq_bin" -r '.rows[] | select(.id == "tray") | .rect | "\(.x + (.width / 2 | floor)) \(.y + (.height / 2 | floor))"' "$tray_overflow_quick_path" | head -n 1)
click
sleep 2
$ipc call panel state > "$tray_overflow_tile_panel_path" 2>&1
"$grim_bin" "$tray_overflow_tile_path" > /dev/null 2>&1
$ipc call panel close > /dev/null 2>&1
settings_retarget '.bar.layout.right = ["tray", "clock"]'
sleep 3
$ipc call bar room > "$tray_overflow_room_path" 2>&1
cell() { "$jq_bin" -r --arg n "\$1" '.[0].cells[] | select(.name == \$n) | "\(.x + .width / 2 | floor) \(.y + .height / 2 | floor)"' "$tray_overflow_room_path" | head -n 1; }
check() { echo "\$1 open=\$($ipc call tray status 2>/dev/null | "$jq_bin" -c '.overflow.open') panel=\$($ipc call panel state 2>/dev/null)" >> "$tray_overflow_clicks_path"; }
park \$(cell tray); click; sleep 1.5; check clicked-open
park 900 700; click; sleep 1.5; check outside
"$grim_bin" "$tray_overflow_outside_path" > /dev/null 2>&1
park \$(cell tray); click; sleep 1.5; check reopened
park \$(cell clock); click; sleep 1.5; check handoff
"$grim_bin" "$tray_overflow_handoff_path" > /dev/null 2>&1
$ipc call panel close > /dev/null 2>&1
STEPS
)
  fi
  write_script "$script" <<EOF
#!/usr/bin/env bash
"$python3_bin" "$stub" --id overflow-fixture-1 --title "Overflow Fixture 1" --color c0392b & echo \$! >> "$tray_overflow_pids_path"
"$python3_bin" "$stub" --id overflow-fixture-2 --title "Overflow Fixture 2" --color 27ae60 --menu & echo \$! >> "$tray_overflow_pids_path"
"$python3_bin" "$stub" --id overflow-fixture-3 --title "Overflow Fixture 3" --color 2980b9 & echo \$! >> "$tray_overflow_pids_path"
"$python3_bin" "$stub" --id overflow-fixture-4 --title "Overflow Fixture 4" --color f1c40f & echo \$! >> "$tray_overflow_pids_path"
"$python3_bin" "$stub" --id overflow-fixture-5 --title "Overflow Fixture 5" --color 8e44ad & echo \$! >> "$tray_overflow_pids_path"
"$python3_bin" "$stub" --id overflow-fixture-6 --title "Overflow Fixture 6" --color 16a085 & echo \$! >> "$tray_overflow_pids_path"
"$python3_bin" "$stub" --id overflow-fixture-6 --title "Overflow Fixture 6" --color e67e22 & echo \$! >> "$tray_overflow_pids_path"
sleep 7
$ipc call tray status > "$tray_overflow_status_path" 2>&1
"$grim_bin" "$tray_overflow_strip_path" > /dev/null 2>&1
sleep 1
$ipc call panel toggle trayoverflow > /dev/null 2>&1
until_settled
$ipc call tray status > "$tray_overflow_open_path" 2>&1
$ipc call debug dump > "$tray_overflow_bar_dump_path" 2>&1
"$wmctl_bin" -j layers > "$tray_overflow_layers_path" 2>&1
"$grim_bin" "$tray_overflow_bar_path" > /dev/null 2>&1
sleep 1
$ipc call tray menu overflow-fixture-2 > "$tray_overflow_menu_reply_path" 2>&1
sleep 2
$ipc call tray status > "$tray_overflow_menu_path" 2>&1
"$grim_bin" "$tray_overflow_menu_shot_path" > /dev/null 2>&1
sleep 1
$ipc call panel close > /dev/null 2>&1
sleep 1
$tail_steps
EOF

  # The stubs sit in GLib.MainLoop().run() forever, same as --tray's.
  write_script "$kill_script" <<EOF
#!/usr/bin/env bash
if [ -f "$tray_overflow_pids_path" ]; then
  while read -r pid; do
    kill "\$pid" 2>/dev/null || true
  done < "$tray_overflow_pids_path"
fi
EOF
  add_cleanup "bash $kill_script"
  wm_exec_once "bash $script"
}

# How many pixels of the frame carry one fixture's colour.
tray_overflow_pixels() {
  "$python3_bin" -c 'import sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGB")
want = tuple(int(sys.argv[2][i:i + 2], 16) for i in (0, 2, 4))
print(sum(n for n, p in im.getcolors(1 << 24) if all(abs(a - b) < 8 for a, b in zip(p, want))))' "$1" "$2"
}

leg_tray_overflow_assert() {
  local total inline hidden open panels
  if [ ! -s "$tray_overflow_status_path" ]; then
    fail "no tray status produced for the overflow check"
  fi
  cat "$tray_overflow_status_path"; echo
  total=$("$jq_bin" -r '.items | length' "$tray_overflow_status_path")
  inline=$("$jq_bin" -r '.overflow.inline' "$tray_overflow_status_path")
  hidden=$("$jq_bin" -r '.overflow.hidden | length' "$tray_overflow_status_path")
  echo "tray: $inline of $total left on the strip, $hidden in the second bar"
  if [ "$total" -lt 7 ]; then
    fail "tray status did not report the 7 fixture items (got $total), stub registration likely failed"
  fi
  if [ "$inline" -ne 0 ]; then
    fail "the tray kept $inline items on a strip with no room for them; it moves whole or not at all"
  fi
  if [ "$hidden" -ne "$total" ]; then
    fail "the second bar holds $hidden of $total items; it holds the whole tray or the strip keeps it"
  fi
  if [ ! -f "$tray_overflow_strip_path" ]; then
    fail "no squeezed-strip screenshot produced"
  fi
  echo "SMOKE_TRAY_OVERFLOW_STRIP $tray_overflow_strip_path"
  # The same split, with the second bar up: the surface is a popout like any
  # other, so it answers PanelIpc and shows up as a formalshell:panel layer.
  if [ ! -s "$tray_overflow_open_path" ]; then
    fail "no tray status produced with the second bar open"
  fi
  open=$("$jq_bin" -r '.overflow.open' "$tray_overflow_open_path")
  if [ "$open" != "true" ]; then
    fail "panel toggle trayoverflow did not open the second bar, got: $(cat "$tray_overflow_open_path")"
  fi
  panels=$("$jq_bin" -r "$layers_jq"'layer("formalshell:panel") | length' "$tray_overflow_layers_path" 2>/dev/null)
  echo "panel layers while the second bar is open: $panels"
  if [ "${panels:-0}" -lt 1 ]; then
    fail "the second bar reported itself open with no formalshell:panel layer mapped"
  fi
  if [ ! -f "$tray_overflow_bar_path" ]; then
    fail "no second-bar screenshot produced"
  fi
  echo "SMOKE_TRAY_OVERFLOW_BAR $tray_overflow_bar_path"
  local colour
  for colour in 16a085 e67e22; do
    if [ "$(tray_overflow_pixels "$tray_overflow_bar_path" "$colour")" -lt 20 ]; then
      fail "the second bar draws no #$colour square: two items sharing overflow-fixture-6's Id did not both get a cell"
    fi
  done
  # The second bar is a headerless panel: the shared panel-language check,
  # its chips' room even on every side.
  panel_language_assert trayoverflow "$tray_overflow_bar_path" "$("$jq_bin" -c '.panel.rest' "$tray_overflow_bar_dump_path")" no "$tray_overflow_bar_dump_path" rowInset
  panel_language_verdict
  # An item's own menu belongs to the bar its cell is in, so opening one
  # leaves that bar where it was. Closing the bar with it
  # is the gesture cancelling itself.
  if ! grep -q '^ok$' "$tray_overflow_menu_reply_path" 2>/dev/null; then
    fail "tray menu was refused inside the second bar, got: $(cat "$tray_overflow_menu_reply_path" 2>/dev/null)"
  fi
  if [ "$("$jq_bin" -r '.overflow.open' "$tray_overflow_menu_path")" != "true" ]; then
    fail "opening a tray item's menu closed the second bar out from under it, got: $(cat "$tray_overflow_menu_path")"
  fi
  # Where the two sit is the frame's to say: both hang off the same bar edge
  # by the same rule, so a menu that did not stand clear of its owner drew
  # straight over it. No layer box can show that, they are
  # both full-output surfaces.
  if [ ! -f "$tray_overflow_menu_shot_path" ]; then
    fail "no menu-over-the-second-bar screenshot produced"
  fi
  echo "SMOKE_TRAY_OVERFLOW_MENU $tray_overflow_menu_shot_path"
  # The governed tray as a quick settings tile: the chevron opens the card
  # with a Tray tile reading every item, and a real click on it hands the
  # card over to this second bar.
  if leg_on sidebar; then
    echo "SMOKE_TRAY_OVERFLOW chevron tile skipped: the sidebar has no chevron"
  else
    [ -f "$tray_overflow_collapsed_path" ] && [ -f "$tray_overflow_expanded_path" ] \
      || fail "no chevron screenshots produced"
    echo "SMOKE_TRAY_OVERFLOW_COLLAPSED $tray_overflow_collapsed_path"
    echo "SMOKE_TRAY_OVERFLOW_EXPANDED $tray_overflow_expanded_path"
    echo "SMOKE_TRAY_OVERFLOW_TILE $tray_overflow_tile_path"
    if cmp -s "$tray_overflow_collapsed_path" "$tray_overflow_expanded_path"; then
      fail "the chevron opened nothing: the two frames are byte-identical"
    fi
    echo "tray tile: $("$jq_bin" -c '.rows[] | select(.id == "tray")' "$tray_overflow_quick_path" 2>/dev/null)"
    "$jq_bin" -e --argjson n "$total" '.from == "chevron:right" and ([.rows[] | select(.kind == "info") | .id] == ["tray"])
        and (.rows[] | select(.id == "tray") | (.subtitle | tostring | contains($n | tostring)))' "$tray_overflow_quick_path" > /dev/null 2>&1 \
      || fail "the chevron's card does not carry the governed tray as one tile reading its $total items: $(cat "$tray_overflow_quick_path" 2>/dev/null)"
    grep -q '^trayoverflow$' "$tray_overflow_tile_panel_path" 2>/dev/null \
      || fail "a real click on the Tray tile did not hand over to the tray's second bar: $(cat "$tray_overflow_tile_panel_path" 2>/dev/null)"
  fi
  echo "SMOKE_TRAY_OVERFLOW_OUTSIDE $tray_overflow_outside_path"
  echo "SMOKE_TRAY_OVERFLOW_HANDOFF $tray_overflow_handoff_path"
  cat "$tray_overflow_clicks_path" 2>/dev/null
  _tray_overflow_click_expect clicked-open 'open=true panel=' "a real click on the dots opens the second bar"
  _tray_overflow_click_expect outside 'open=false panel=' "a click on the desktop shuts it"
  _tray_overflow_click_expect reopened 'open=true panel=' "the dots open it again"
  _tray_overflow_click_expect handoff 'open=false panel=calendar' "a click on the clock shuts it and opens the calendar"
  if leg_on sidebar; then
    _tray_overflow_toast_clear
  fi
}

# The second bar's rest rect against every toast card's rect, both off the
# same `debug dump`: sharing x, apart in y.
_tray_overflow_toast_clear() {
  local panel cards overlap
  [ -f "$tray_overflow_toast_path" ] || fail "no frame of the second bar with a toast under it"
  echo "SMOKE_TRAY_OVERFLOW_TOAST $tray_overflow_toast_path"
  panel=$("$jq_bin" -c '.panel' "$tray_overflow_toast_dump_path" 2>/dev/null)
  cards=$("$jq_bin" -c '.toastCards' "$tray_overflow_toast_dump_path" 2>/dev/null)
  echo "second bar: $panel"
  echo "toast cards: $cards"
  "$jq_bin" -e '.panel.id == "trayoverflow" and .panel.open' "$tray_overflow_toast_dump_path" > /dev/null 2>&1 \
    || fail "a real click on the tray button did not leave the trayoverflow panel open for the toast case: $panel"
  "$jq_bin" -e '.toastCards | length >= 1' "$tray_overflow_toast_dump_path" > /dev/null 2>&1 \
    || fail "the sticky toast never reached the stack: $cards"
  "$jq_bin" -e '.panel.rest as $p | all(.toastCards[].rect; .x < $p.x + $p.width and $p.x < .x + .width)' "$tray_overflow_toast_dump_path" > /dev/null 2>&1 \
    || fail "the toast column and the second bar do not share x, so this case proves nothing: $panel $cards"
  overlap=$("$jq_bin" -r '[.panel.rest, .panel.live] as $ps | [.toastCards[].rect as $t | $ps[] | select($t.x < .x + .width and .x < $t.x + $t.width and $t.y < .y + .height and .y < $t.y + $t.height)] | length' "$tray_overflow_toast_dump_path" 2>/dev/null)
  [ "${overlap:-1}" = "0" ] || fail "a toast card intersects the open second bar: $panel $cards"
  echo "SMOKE_TRAY_OVERFLOW the toast keeps clear of the open second bar"
}

_tray_overflow_click_expect() {
  local name="$1" want="$2" what="$3" line
  line=$(grep "^$name " "$tray_overflow_clicks_path" 2>/dev/null | head -n 1)
  case "$line" in
    *"$want"*) echo "SMOKE_TRAY_OVERFLOW $what" ;;
    *) fail "$what: $name wanted '$want', got '${line:-nothing}'" ;;
  esac
}
