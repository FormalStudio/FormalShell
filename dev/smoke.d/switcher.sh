# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --switcher: Gala's Alt+Tab as a keyboard-only surface (M60 T6, the
# 2026-09-17 spec's Part 2), with live window thumbnails (owner, 2026-09-29).
# Three windows in one session, the cursor walked over IPC the way the
# compositor's bind walks it, and both halves of the claim at every step:
# what `switcher state` says the cursor is on, and the card as drawn.
#
# The three windows are the base run's own fixture window plus two more of
# the same app id, so all three captions resolve the same icon out of the
# isolated home's icon theme (#CE5D97, which nothing else in a frame is) and a
# caption carrying an icon is telling apart from an empty one by colour alone.
#
# A fourth window sits on workspace 2 and must not be offered (M64): Gala
# lists the active workspace's windows alone, and offering the rest is what
# made a quick Alt+Tab commit to a window elsewhere and take the compositor
# to it. The compositor's own client list is read beside `switcher state`, so
# a run where that fourth window never spawned cannot pass by having nothing
# to exclude.
#
# The thumbnails are ScreencopyViews and live only while the card is open:
# `switcher state` reports how many hold a frame (`captured`) and how many
# hold a capture source (`capturing`), which has to be all three while open
# and zero once the commit has closed the card and its fade has run out.
#
# Cell positions come from `switcher state` (`cells`, in output pixels as the
# card draws them) rather than from constants, since a thumbnail's width
# follows its window. The frames are read for the caption icon's pink at the
# centre of each cell's icon rect, and for the strip of cell fill left of each
# thumbnail. The selected cell takes the table's `switcher.cell` selected
# fill, so its strip differs from the other two while theirs match each
# other, and after one `switcher prev` that difference has moved one cell
# left. A patch read per cell rather than a colour compared against a
# constant: the fill is the wallpaper's own accent under matugen, and a leg
# naming a hex would be asserting the palette rather than the cursor.
#
# The switcher is on by default (`switcher.enabled`).
leg_switcher_flag="--switcher"
leg_switcher_order=102
leg_switcher_needs="foot jq convert"
# The base run's fixture window is the third of the three, and the one the
# commit lands on.
leg_switcher_fixture_window=keep

switcher_second_json="$shot_dir/switcher-second.json"
switcher_first_json="$shot_dir/switcher-first.json"
switcher_closed_json="$shot_dir/switcher-closed.json"
switcher_second_png="$shot_dir/switcher-second.png"
switcher_first_png="$shot_dir/switcher-first.png"
switcher_layers_open="$shot_dir/switcher-layers-open.json"
switcher_layers_closed="$shot_dir/switcher-layers-closed.json"
switcher_active_json="$shot_dir/switcher-active.json"
switcher_clients_json="$shot_dir/switcher-clients.json"

leg_switcher_fixture() {
  # The fourth window on workspace 2 is only held out of the card under
  # Gala's list, which is opt-in.
  settings_fragment ', "switcher": {"currentWorkspace": true}'
}

leg_switcher_validate() {
  if leg_on switcher_off; then
    echo "usage: --switcher-off turns the switcher off in settings.json, so it cannot share a session with --switcher" >&2
    exit 1
  fi
}

leg_switcher_timing() {
  leg_timing 40 100
}

leg_switcher_drive() {
  local script="$shot_dir/switcher-drive.sh"
  write_script "$script" <<EOS
#!/usr/bin/env bash
sleep 4
# Two more windows of the same app id as the base fixture's, spawned through
# the compositor so they are tracked from the moment they map.
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-smoke-iconic --title='formalshell smoke two' sh -c 'sleep 300']==])"
sleep 2
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-smoke-iconic --title='formalshell smoke three' sh -c 'sleep 300']==])"
sleep 3
# And one the card must not hold, moved off silently so the monitor stays on
# the workspace the other three are on.
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-smoke-elsewhere --title='formalshell smoke elsewhere' sh -c 'sleep 300']==])"
sleep 3
"$hyprctl_bin" dispatch "hl.dsp.window.move({ workspace = 2, follow = false, window = 'class:formalshell-smoke-elsewhere' })"
sleep 3
"$hyprctl_bin" -j clients > "$switcher_clients_json" 2>&1

# The first press opens the card on the window before the focused one; the
# second walks on to the third entry.
$ipc call switcher next > /dev/null 2>&1
sleep 1
$ipc call switcher next > /dev/null 2>&1
sleep 2
$ipc call switcher state > "$switcher_second_json" 2>&1
"$hyprctl_bin" -j layers > "$switcher_layers_open" 2>&1
"$grim_bin" "$switcher_second_png" > /dev/null 2>&1

$ipc call switcher prev > /dev/null 2>&1
sleep 2
$ipc call switcher state > "$switcher_first_json" 2>&1
"$grim_bin" "$switcher_first_png" > /dev/null 2>&1

$ipc call switcher commit > /dev/null 2>&1
sleep 3
$ipc call switcher state > "$switcher_closed_json" 2>&1
"$hyprctl_bin" -j layers > "$switcher_layers_closed" 2>&1
"$hyprctl_bin" -j activewindow > "$switcher_active_json" 2>&1
EOS
  hypr_exec_once "bash $script"
}

# One patch of a frame as its own mean colour, as a plain string: two crops of
# a flat region answer the same string, and two different fills never do.
_switcher_patch() {
  $convert_bin "$1" -crop "$2" +repage \
    -format '%[fx:int(mean.r*255+0.5)].%[fx:int(mean.g*255+0.5)].%[fx:int(mean.b*255+0.5)]' \
    info: 2>/dev/null
}

# Whether two `r.g.b` patches are the same colour within the rig's own
# rounding. Only the icon read needs it: the same-or-different reads below
# are two crops of one flat region and answer identically or not at all.
_switcher_near() {
  local a b i tolerance=8
  IFS='.' read -r -a a <<< "$1"
  IFS='.' read -r -a b <<< "$2"
  for i in 0 1 2; do
    local delta=$(( ${a[$i]:-0} - ${b[$i]:-0} ))
    [ "$delta" -lt 0 ] && delta=$(( -delta ))
    [ "$delta" -le "$tolerance" ] || return 1
  done
  return 0
}

# A crop geometry for the middle of cell $2's caption icon, and for the strip
# of cell fill four pixels in from its left edge, read off the state in $1.
_switcher_icon_patch() {
  "$jq_bin" -r --argjson i "$2" \
    '.cells[$i].icon | "6x6+\(.x + .width / 2 - 3 | floor)+\(.y + .height / 2 - 3 | floor)"' "$1"
}

_switcher_margin_patch() {
  "$jq_bin" -r --argjson i "$2" \
    '.cells[$i].cell | "4x20+\(.x + 4)+\(.y + .height / 2 - 10 | floor)"' "$1"
}

# Hyprland's own address for a window, and the shell's id for it, differ by
# the prefix alone (HyprlandBackend: ids are the hex address verbatim, the
# dispatcher's selector adds the 0x).
_switcher_bare_address() {
  echo "${1#0x}"
}

_switcher_count_layers() {
  "$jq_bin" -r '[.[] | .levels[] | .[] | select(.namespace == "formalshell:switcher")] | length' \
    "$1" 2>/dev/null
}

leg_switcher_assert() {
  local f
  for f in "$switcher_second_json" "$switcher_first_json" "$switcher_closed_json" \
    "$switcher_layers_open" "$switcher_layers_closed" "$switcher_active_json" \
    "$switcher_clients_json"; do
    [ -s "$f" ] || fail "no switcher reply produced at $f"
  done
  for f in "$switcher_second_png" "$switcher_first_png"; do
    [ -f "$f" ] || fail "no frame produced at $f"
  done

  # Two presses over three windows: open on the second entry, then on the
  # third.
  local open count index selected_id selected_title
  open=$("$jq_bin" -r '.open' "$switcher_second_json")
  count=$("$jq_bin" -r '.count' "$switcher_second_json")
  index=$("$jq_bin" -r '.index' "$switcher_second_json")
  selected_id=$("$jq_bin" -r '.id' "$switcher_second_json")
  selected_title=$("$jq_bin" -r '.title' "$switcher_second_json")
  echo "two presses: open=$open index=$index count=$count title='$selected_title'"
  [ "$open" = "true" ] || fail "the switcher is not open after two next calls: $(cat "$switcher_second_json")"
  [ "$index" = "2" ] || fail "the cursor is on entry $index, not the third"
  [ -n "$selected_id" ] || fail "the switcher reports no window under the cursor"

  # The session holds four windows and the card holds three: the fourth is on
  # workspace 2, and the card is the workspace being looked at.
  local mapped_windows elsewhere elsewhere_workspace
  mapped_windows=$("$jq_bin" -r 'length' "$switcher_clients_json")
  elsewhere=$("$jq_bin" -r '[.[] | select(.class == "formalshell-smoke-elsewhere")] | length' \
    "$switcher_clients_json")
  elsewhere_workspace=$("$jq_bin" -r \
    '[.[] | select(.class == "formalshell-smoke-elsewhere") | .workspace.id] | first' \
    "$switcher_clients_json")
  echo "session: $mapped_windows windows mapped, one on workspace $elsewhere_workspace"
  [ "${mapped_windows:-0}" -ge 4 ] \
    || fail "the session holds $mapped_windows windows, so the one for another workspace never spawned and there was nothing to exclude"
  [ "${elsewhere:-0}" -eq 1 ] \
    || fail "$elsewhere windows carry the other workspace's app id, expected exactly one"
  [ "$elsewhere_workspace" = "2" ] \
    || fail "the fourth window is on workspace $elsewhere_workspace, not the 2 it was moved to"
  [ "$count" = "3" ] \
    || fail "the switcher offers $count windows, not the three on this workspace: a window from somewhere else reached the card"

  # The surface itself, off the compositor's own layer list.
  local mapped
  mapped=$(_switcher_count_layers "$switcher_layers_open")
  echo "layers while open: formalshell:switcher=$mapped"
  [ "${mapped:-0}" -ge 1 ] || fail "no formalshell:switcher layer surface while the card is open"

  # Thumbnails: every cell holds a live frame and a capture source while the
  # card is open.
  local captured capturing
  captured=$("$jq_bin" -r '.captured' "$switcher_second_json")
  capturing=$("$jq_bin" -r '.capturing' "$switcher_second_json")
  echo "thumbnails while open: captured=$captured capturing=$capturing"
  [ "$capturing" = "$count" ] \
    || fail "$capturing of $count thumbnails hold a capture source while the card is open"
  [ "$captured" = "$count" ] \
    || fail "$captured of $count thumbnails hold a captured frame while the card is open"
  "$jq_bin" -e '.cells | length == 3' "$switcher_second_json" > /dev/null \
    || fail "switcher state reports $("$jq_bin" -r '.cells | length' "$switcher_second_json") cells, not three"

  # Three captions carrying the fixture's own icon, which is the only pink in
  # the session.
  local i icon_colours=""
  for i in 0 1 2; do
    icon_colours="$icon_colours $(_switcher_patch "$switcher_second_png" "$(_switcher_icon_patch "$switcher_second_json" "$i")")"
  done
  echo "caption icons: $icon_colours"
  local expected_icon="206.93.151"
  local colour
  for colour in $icon_colours; do
    _switcher_near "$colour" "$expected_icon" \
      || fail "a caption icon reads $colour, not the fixture icon's $expected_icon: three icon captions are not what was drawn"
  done

  # The strip of cell fill left of each thumbnail: the selected one fills
  # with the table's selected fill, the other two show the card under it.
  local second_margins=() first_margins=()
  for i in 0 1 2; do
    second_margins+=("$(_switcher_patch "$switcher_second_png" "$(_switcher_margin_patch "$switcher_second_json" "$i")")")
    first_margins+=("$(_switcher_patch "$switcher_first_png" "$(_switcher_margin_patch "$switcher_first_json" "$i")")")
  done
  echo "cell margins on the third: ${second_margins[*]}"
  echo "cell margins on the second: ${first_margins[*]}"

  [ "${second_margins[0]}" = "${second_margins[1]}" ] \
    || fail "the first two cells differ (${second_margins[0]} vs ${second_margins[1]}) with the cursor on the third"
  [ "${second_margins[2]}" != "${second_margins[1]}" ] \
    || fail "the third cell draws what the unselected ones do (${second_margins[2]}), so no selection fill reached the frame"

  # One `prev`, and the fill has moved one cell left.
  local first_index
  first_index=$("$jq_bin" -r '.index' "$switcher_first_json")
  echo "after prev: index=$first_index"
  [ "$first_index" = "1" ] || fail "prev left the cursor on entry $first_index, not the second"
  [ "${first_margins[0]}" = "${first_margins[2]}" ] \
    || fail "the unselected cells differ (${first_margins[0]} vs ${first_margins[2]}) with the cursor on the second"
  [ "${first_margins[1]}" != "${first_margins[0]}" ] \
    || fail "the second cell draws what the unselected ones do (${first_margins[1]}), so the fill did not travel"
  [ "${first_margins[1]}" = "${second_margins[2]}" ] \
    || fail "the fill on the second cell (${first_margins[1]}) is not the one the third carried (${second_margins[2]})"

  # Commit: the card closes and focus lands on the window it named.
  local committed_id closed_open closed_layers active_address
  committed_id=$("$jq_bin" -r '.id' "$switcher_first_json")
  closed_open=$("$jq_bin" -r '.open' "$switcher_closed_json")
  closed_layers=$(_switcher_count_layers "$switcher_layers_closed")
  active_address=$("$jq_bin" -r '.address' "$switcher_active_json")
  echo "commit: open=$closed_open layers=$closed_layers active=$active_address want=$committed_id"
  [ "$closed_open" = "false" ] || fail "the switcher is still open after a commit: $(cat "$switcher_closed_json")"
  "$jq_bin" -e '.captured == 0 and .capturing == 0' "$switcher_closed_json" > /dev/null \
    || fail "thumbnails are still captured after the commit: $("$jq_bin" -c '{captured, capturing}' "$switcher_closed_json")"
  [ "${closed_layers:-0}" -eq 0 ] || fail "the switcher's layer surface stayed mapped after a commit ($closed_layers)"
  [ -n "$active_address" ] || fail "hyprctl reports no active window after the commit"
  [ "$(_switcher_bare_address "$active_address")" = "$(_switcher_bare_address "$committed_id")" ] \
    || fail "focus landed on $active_address, not on the committed $committed_id"

  echo "SMOKE_SWITCHER ok index=$index->$first_index count=$count captured=$captured committed=$committed_id"
  echo "SMOKE_SWITCHER_SECOND $switcher_second_png"
  echo "SMOKE_SWITCHER_FIRST $switcher_first_png"
}
