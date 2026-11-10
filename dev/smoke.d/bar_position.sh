# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --bar-position <top|bottom|left|right> pins `bar.position` in the settings
# fixture and proves the strip really moved: the formalshell:bar layer's box
# is read off `hyprctl -j layers` and held against the output's own size,
# since a screenshot of a bar drawn on the wrong edge would look like a bar.
# On a left or right bar that box is the strip's thickness wide and the
# whole output tall; on a bottom bar it ends on the output's last row.
#
# It carries a right region long enough to be worth a chevron (thirteen
# governed cells ahead of it, battery/audio/network outboard). Collapsed
# frame, `bar chevron expand`, expanded frame, the two asserted to differ:
# the chevron opens quick settings, read off `quicksettings status` as open
# from that chevron and lying wholly off the strip on its inner side, its
# governed cells as info tiles. Then `panel open audio` hangs a panel
# off the bar's inner edge for the run's own frame, with `panel state`
# agreeing it is the open one. It claims the right region, so it does not
# combine with another leg writing it (--chevron, --bar-layout).
#
# It also RIDES a leg that photographs its own frames on the moved strip
# (--join, --bar-title, --menu-emerge, --panel-morph, --osd), which is the
# only way those legs reach an edge other than the top without a copy of
# each of them per position. Riding, this leg pins the edge and nothing else: the layout is
# the rider's own or the default, the chevron and the panel open (which would
# land in the middle of somebody else's sampling) are left undriven, and the
# one claim left is the one the frames cannot make, the strip's own box on
# the edge it was asked for.
leg_bar_position_flag="--bar-position <edge>"
leg_bar_position_order=185
leg_bar_position_needs="jq"

bar_position_layers_path="$shot_dir/bar-position-layers.json"
bar_position_monitors_path="$shot_dir/bar-position-monitors.json"
bar_position_status_collapsed_path="$shot_dir/bar-position-chevron-collapsed.json"
bar_position_status_expanded_path="$shot_dir/bar-position-chevron-expanded.json"
bar_position_quick_path="$shot_dir/bar-position-quick.json"
bar_position_expand_reply_path="$shot_dir/bar-position-expand-reply.txt"
bar_position_collapsed_path="$shot_dir/bar-position-collapsed.png"
bar_position_expanded_path="$shot_dir/bar-position-expanded.png"
bar_position_panel_open_path="$shot_dir/bar-position-panel-open.txt"
bar_position_panel_state_path="$shot_dir/bar-position-panel-state.txt"
bar_position_dump_path="$shot_dir/bar-position-dump.json"

bar_position_rider() {
  leg_on join || leg_on bar_title || leg_on menu_emerge || leg_on panel_morph || leg_on osd
}

leg_bar_position_validate() {
  case "$(leg_arg bar_position)" in
    top|bottom|left|right) ;;
    *)
      echo "usage: $0 --bar-position <top|bottom|left|right>" >&2
      exit 1
      ;;
  esac
  leg_bar_position_claims="bar.position=$(leg_arg bar_position) layout=bar"
  bar_position_rider || leg_bar_position_claims+=" bar.layout.right"
}

leg_bar_position_fixture() {
  settings_pin ".bar.position = \"$(leg_arg bar_position)\""
  bar_position_rider && return 0
  settings_pin '.bar.layout.right = ["bluetooth", "weather", "tray", "bell", "indicators", "monitor", "keyboardLayout", "display", "github", "usage", "tailscale", "systemUpdate", "clock", "chevron", "battery", "audio", "network"]'
}

leg_bar_position_timing() {
  # The drive's last step (the panel open) lands ~12s in; the run's own
  # frame is taken past that, so it shows the panel hanging off the bar.
  leg_timing 40
}

leg_bar_position_drive() {
  local script="$shot_dir/bar-position-drive.sh" own=""
  if ! bar_position_rider; then
    own=$(cat <<EOS
"$grim_bin" "$bar_position_collapsed_path" > /dev/null 2>&1
$ipc call bar chevron status > "$bar_position_status_collapsed_path" 2>&1
$ipc call bar chevron expand > "$bar_position_expand_reply_path" 2>&1
sleep 2
$ipc call bar chevron status > "$bar_position_status_expanded_path" 2>&1
$ipc call quicksettings status > "$bar_position_quick_path" 2>&1
"$grim_bin" "$bar_position_expanded_path" > /dev/null 2>&1
$ipc call panel open audio > "$bar_position_panel_open_path" 2>&1
until_settled
$ipc call panel state > "$bar_position_panel_state_path" 2>&1
EOS
)
  fi
  write_script "$script" <<EOS
#!/usr/bin/env bash
# Waits for the shell to have read settings.json rather than sleeping a
# fixed count at it. The bar maps as a default top strip and only moves to
# bar.position once Config reaches Theme.barPosition, and the shell's own
# first map lands somewhere either side of five seconds on this rig, so a
# bare sleep read either an empty layer list or a top bar depending on how
# loaded the host was, and the assert below called both a bar on the wrong
# edge. The position in a debug dump is that same edit having landed,
# whatever layout the run carries; the beat after it is the one frame the
# move takes.
#
# No backticks in here: this script is written through an unquoted heredoc,
# so a quoted identifier in a comment runs as a command.
until_jq 20 "$bar_position_dump_path" '.theme.barPosition == "$(leg_arg bar_position)"' call debug dump
sleep 1
"$hyprctl_bin" -j layers > "$bar_position_layers_path" 2>&1
"$hyprctl_bin" -j monitors > "$bar_position_monitors_path" 2>&1
$own
EOS
  hypr_exec_once "bash $script"
}

leg_bar_position_assert() {
  local edge box bx by bw bh mw mh
  edge=$(leg_arg bar_position)
  if [ ! -s "$bar_position_layers_path" ] || [ ! -s "$bar_position_monitors_path" ]; then
    fail "no layer or monitor dump produced for the bar position check"
  fi
  # hyprctl -j layers nests every surface under its monitor and level; the
  # bar is the one surface in the formalshell:bar namespace.
  box=$("$jq_bin" -r "$layers_jq"'layer("formalshell:bar") | first | "\(.x) \(.y) \(.w) \(.h)"' "$bar_position_layers_path" 2>/dev/null)
  if [ -z "$box" ] || [ "$box" = "null" ]; then
    fail "no formalshell:bar layer surface in $bar_position_layers_path"
  fi
  read -r bx by bw bh <<< "$box"
  # With --frame the bar's window is the whole output and paints the ring;
  # frame.sh asserts that box, and the edge is proven by the
  # zones instead.
  if leg_on frame; then
    echo "bar window spans the output under --frame: x=$bx y=$by w=$bw h=$bh"
    box="framed"
  fi
  # The output's logical size, the space layer geometry is reported in.
  read -r mw mh <<< "$("$jq_bin" -r 'first | "\(.width / .scale | floor) \(.height / .scale | floor)"' "$bar_position_monitors_path")"
  echo "bar layer: x=$bx y=$by w=$bw h=$bh on ${mw}x${mh} ($edge)"
  [ "$box" = "framed" ] && edge=framed
  case "$edge" in
    top)
      [ "$by" -eq 0 ] && [ "$bw" -eq "$mw" ] && [ "$bh" -lt "$bw" ] || fail "a top bar should span the top edge, got x=$bx y=$by w=$bw h=$bh"
      ;;
    bottom)
      [ $((by + bh)) -eq "$mh" ] && [ "$bw" -eq "$mw" ] && [ "$bh" -lt "$bw" ] || fail "a bottom bar should span the bottom edge, got x=$bx y=$by w=$bw h=$bh"
      ;;
    left)
      [ "$bx" -eq 0 ] && [ "$bh" -eq "$mh" ] && [ "$bw" -lt "$bh" ] || fail "a left bar should span the left edge, got x=$bx y=$by w=$bw h=$bh"
      ;;
    right)
      [ $((bx + bw)) -eq "$mw" ] && [ "$bh" -eq "$mh" ] && [ "$bw" -lt "$bh" ] || fail "a right bar should span the right edge, got x=$bx y=$by w=$bw h=$bh"
      ;;
  esac
  # The strip is one cell row plus its margin band, never a stretched
  # window: 68 is well past what the default scale resolves (40 on a
  # horizontal bar, 56 on a vertical one, which stacks each cell's icon
  # over its label instead of running the two along the strip) and well
  # short of anything a wrongly sized surface would report.
  local thickness=$bh
  case "$edge" in left|right) thickness=$bw ;; framed) thickness=0 ;; esac
  if [ "$thickness" -gt 68 ]; then
    fail "bar strip is $thickness thick, which is not a cell row plus its margin band"
  fi

  if bar_position_rider; then
    echo "SMOKE_BAR_POSITION_RIDER the edge alone: the layout, the chevron and the panel belong to the leg being ridden"
    return 0
  fi

  # What the chevron governs, and whether the card it opens is up: the
  # governed names are off the strip for the whole session and
  # `collapses` is the only list there is to report, so `open` is what the
  # expand changes.
  local hidden_names='"bluetooth","weather","tray","bell","indicators","monitor","keyboardLayout","display","github","usage","tailscale","systemUpdate","clock"'
  expect_jq "$bar_position_status_collapsed_path" "any(.regions[]; .chevron == true and .collapses == [$hidden_names] and .open == false)" \
    "bar chevron status did not report the governed names off the strip while collapsed" '.regions'
  if ! grep -q '^ok$' "$bar_position_expand_reply_path" 2>/dev/null; then
    fail "bar chevron expand was refused. Got: $(cat "$bar_position_expand_reply_path" 2>/dev/null)"
  fi
  expect_jq "$bar_position_status_expanded_path" "any(.regions[]; .chevron == true and .collapses == [$hidden_names] and .open == true)" \
    "bar chevron expand did not open quick settings" '.regions'
  # The card off the strip's inner side, on whichever edge the strip is.
  "$jq_bin" -e '.open == true and .from == "chevron:right" and ([.rows[] | select(.kind == "info")] | length > 0)' "$bar_position_quick_path" > /dev/null 2>&1 \
    || fail "the chevron's card is not quick settings carrying its governed cells as tiles: $(cat "$bar_position_quick_path" 2>/dev/null)"
  local card
  card=$("$jq_bin" -r '.card | "\(.x) \(.y) \(.width) \(.height)"' "$bar_position_quick_path")
  echo "quick settings card on the $edge bar: $card"
  # shellcheck disable=SC2086
  set -- $card
  case "$edge" in
    top) [ "$2" -ge $((by + bh)) ] ;;
    bottom) [ $(($2 + $4)) -le "$by" ] ;;
    left) [ "$1" -ge $((bx + bw)) ] ;;
    right) [ $(($1 + $3)) -le "$bx" ] ;;
    *) true ;;
  esac || fail "the card at $card overlaps the $edge strip at x=$bx y=$by w=$bw h=$bh"
  for f in "$bar_position_collapsed_path" "$bar_position_expanded_path"; do
    if [ ! -f "$f" ]; then
      fail "no screenshot produced at $f"
    fi
  done
  echo "SMOKE_BAR_POSITION_COLLAPSED $bar_position_collapsed_path"
  echo "SMOKE_BAR_POSITION_EXPANDED $bar_position_expanded_path"
  if cmp -s "$bar_position_collapsed_path" "$bar_position_expanded_path"; then
    fail "collapsed and expanded screenshots are byte-identical: the chevron changed the state but rendered nothing along the $edge bar"
  fi
  if ! grep -q '^ok$' "$bar_position_panel_open_path" 2>/dev/null; then
    fail "panel open audio did not answer ok. Got: $(cat "$bar_position_panel_open_path" 2>/dev/null)"
  fi
  if ! grep -q '^audio$' "$bar_position_panel_state_path" 2>/dev/null; then
    fail "panel state is not audio. Got: $(cat "$bar_position_panel_state_path" 2>/dev/null)"
  fi
}
