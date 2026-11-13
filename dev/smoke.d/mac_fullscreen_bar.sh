# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154,SC2016  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --mac-fullscreen-bar: the bar over a fullscreen Mac window
# (`wayland/reveal.rs`), as the menu bar slides down over a fullscreen app
# on the Mac. The app is atoll's fixtures/electron-panel (FS_ELECTRON_APP),
# installed by `atoll install` (FS_ATOLL) as --mac-electron does; its
# process registers its main menu, so its panel window is a Mac window. The
# panel opens at launch (ELECTRON_PANEL_OPEN) and goes fullscreen through
# `hl.dsp.window.fullscreen`, the test's setup only.
#
# First a foot window fullscreen, standing in for a game:
# no line along the bar's edge, and the real pointer at the top edge brings
# nothing up. Then the Mac window fullscreen: the bar off the layers and the
# line (`formalshell:reveal`) on the overlay level, the real pointer at the
# top edge mapping the bar on the overlay level with the window's size
# unchanged, a real click on a menu title opening its card over the window,
# a second click shutting it, and the pointer moved away taking the bar
# down. Again with a menu left open: the bar stays past the hold with the
# pointer off it, and goes once `appmenu close` shuts the card.
# `org.atoll.MenuBar`'s `Revealed` is read over the bus at each step, true
# exactly while the bar is up, each change announced as PropertiesChanged.
# Last, out of fullscreen, its `Height` with the bar on the top edge, on the
# bottom (0) and back, each change announced too. Not in dev/smoke-all.list: it needs an atoll build and
# the fixture in the slot, as --mac-electron does.
leg_mac_fullscreen_bar_flag="--mac-fullscreen-bar"
leg_mac_fullscreen_bar_order=176
leg_mac_fullscreen_bar_needs="jq foot wlrctl wtype"
leg_mac_fullscreen_bar_claims="focus screen=shared bar.position"
leg_mac_fullscreen_bar_env="FS_ATOLL FS_ELECTRON_APP"

mac_fsbar_prefix="$shot_dir/mac-fsbar"
mac_fsbar_atoll="${FS_ATOLL:-$HOME/atoll/target/debug/atoll}"
mac_fsbar_bundle="${FS_ELECTRON_APP:-$HOME/atoll/fixtures/out/electron-panel/ElectronPanel.app}"
mac_fsbar_id="com.atoll.electron-panel"
mac_fsbar_data="$iso_home/.local/share"
mac_fsbar_log="$shot_dir/mac-fsbar-app.log"
mac_fsbar_binary="$shot_dir/mac-fsbar-binary.txt"

leg_mac_fullscreen_bar_fixture() {
  local stop='echo "SMOKE_FAIL: --mac-fullscreen-bar: $why" >&2; exit 1' why entry
  why="no atoll at $mac_fsbar_atoll (FS_ATOLL)"; [ -x "$mac_fsbar_atoll" ] || eval "$stop"
  why="no bundle at $mac_fsbar_bundle (FS_ELECTRON_APP)"; [ -d "$mac_fsbar_bundle" ] || eval "$stop"
  mkdir -p "$mac_fsbar_data" "$iso_home/.config"
  HOME="$iso_home" XDG_DATA_HOME="$mac_fsbar_data" XDG_CONFIG_HOME="$iso_home/.config" XDG_CACHE_HOME="$HOME/.cache" \
    "$mac_fsbar_atoll" install "$mac_fsbar_bundle" > "$shot_dir/mac-fsbar-install.log" 2>&1
  entry="$mac_fsbar_data/applications/$mac_fsbar_id.desktop"
  why="atoll install wrote no $entry: $(tail -n 3 "$shot_dir/mac-fsbar-install.log")"; [ -f "$entry" ] || eval "$stop"
  sed -n 's/^Exec="\([^"]*\)".*/\1/p' "$entry" | head -n 1 > "$mac_fsbar_binary"
  why="the desktop entry names no binary"; [ -s "$mac_fsbar_binary" ] || eval "$stop"
}

leg_mac_fullscreen_bar_timing() {
  leg_timing 240
}

leg_mac_fullscreen_bar_drive() {
  echo 'hl.config({ misc = { focus_on_activate = true } })'
  local script="$shot_dir/mac-fsbar-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
m="$mac_fsbar_prefix"
log="$mac_fsbar_log"
bin=\$(cat "$mac_fsbar_binary")
cx=$((logical_w / 2))
cy=$((logical_h / 2))
fs() { "$wmctl_bin" dispatch "hl.dsp.window.fullscreen({ mode = 'fullscreen', action = '\$2', window = 'class:^(\$1)\\\$' })" > /dev/null 2>&1; }
bar_prop() { busctl --user get-property org.atoll.AppMenu.Registrar /com/canonical/AppMenu/Registrar org.atoll.MenuBar "\$1" 2>&1; }
height() { bar_prop Height; }
dump() {
  call debug dump > "\$m-\$1-dump.json" 2>&1
  "$wmctl_bin" -j layers > "\$m-\$1-layers.json" 2>&1
  bar_prop Revealed > "\$m-\$1-revealed.txt"
}
# The middle of the menu title named by jq index <i>, in the output's
# logical pixels: its box along the strip, the bar layer's own origin and
# depth.
title_at() {
  call appmenu status > "\$m-titles.json" 2>&1
  "$wmctl_bin" -j layers > "\$m-bar-layers.json" 2>&1
  local bx by bh
  read -r bx by bh < <("$jq_bin" -r "\$layers_jq"'layer("formalshell:bar") | first | "\(.x) \(.y) \(.h)"' "\$m-bar-layers.json")
  "$jq_bin" -r --argjson i "\$1" --argjson bx "\$bx" --argjson by "\$by" --argjson bh "\$bh" \
    '[.titles[] | select(.x != null)] | (.[\$i] // .[0]) | "\(\$bx + .x + .width / 2 | floor) \(\$by + \$bh / 2 | floor)"' "\$m-titles.json"
}

# A game: a window of its own fullscreen gets no line and no bar.
spawn_foot formalshell-mac-fsbar-game
until_window formalshell-mac-fsbar-game
"$wmctl_bin" dispatch "hl.dsp.focus({ window = 'class:^(formalshell-mac-fsbar-game)\\\$' })" > /dev/null 2>&1
fs formalshell-mac-fsbar-game set
until_jq 10 "\$m-game-dump.json" '.fullscreenOutputs | length > 0' call debug dump
park "\$cx" 0
sleep 1  # the pointer resting on the edge, where a line would have brought the bar up
dump game
fs formalshell-mac-fsbar-game unset
"$wmctl_bin" dispatch 'hl.dsp.window.close({ window = "class:formalshell-mac-fsbar-game" })' > /dev/null 2>&1
until_jq 10 "\$m-ungame-dump.json" '.fullscreenOutputs == []' call debug dump

# A Mac window floating over a borderless game, focused: the game covers
# the output, so the top edge brings nothing. Borderless, as Hyprland takes
# a fullscreen window out of full screen when another one takes focus; the
# fixture's plain window, in an instance of its own, as its panel hides once
# it loses focus.
spawn_foot formalshell-mac-fsbar-game
until_window formalshell-mac-fsbar-game
game="class:formalshell-mac-fsbar-game"
"$wmctl_bin" dispatch "hl.dsp.window.float({ window = '\$game', action = 'set' })" > /dev/null 2>&1
"$wmctl_bin" dispatch "hl.dsp.window.resize({ x = $logical_w, y = $logical_h, window = '\$game' })" > /dev/null 2>&1
"$wmctl_bin" dispatch "hl.dsp.window.move({ x = 0, y = 0, window = '\$game' })" > /dev/null 2>&1
until_jq 10 "\$m-over-cover.json" '.fullscreenOutputs | length > 0' call debug dump
"$wmctl_bin" dispatch "hl.dsp.exec_cmd([==[sh -c 'ELECTRON_PANEL_WINDOW=1 exec \$bin --ozone-platform=wayland > \$m-over.log 2>&1']==])" > /dev/null 2>&1
until_jq 60 "\$m-over-clients.json" 'any(.[]; .title == "ElectronPanel - Window")' "$wmctl_bin" -j clients
over="title:ElectronPanel - Window"
"$wmctl_bin" dispatch "hl.dsp.window.float({ window = '\$over', action = 'set' })" > /dev/null 2>&1
"$wmctl_bin" dispatch "hl.dsp.focus({ window = '\$over' })" > /dev/null 2>&1
until_jq 10 "\$m-over-active.json" '.title == "ElectronPanel - Window"' "$wmctl_bin" -j activewindow
park "\$cx" 0
sleep 1  # the pointer resting on the edge, where a line would have brought the bar up
dump over
park "\$cx" "\$cy"
kill "\$("$jq_bin" -r .pid "\$m-over-active.json")" 2>/dev/null
"$wmctl_bin" dispatch 'hl.dsp.window.close({ window = "class:formalshell-mac-fsbar-game" })' > /dev/null 2>&1
until_jq 10 "\$m-unover-dump.json" '.fullscreenOutputs == [] and .windows == []' call debug dump

"$wmctl_bin" dispatch "hl.dsp.exec_cmd([==[sh -c 'ELECTRON_PANEL_OPEN=1 exec \$bin --ozone-platform=wayland > \$log 2>&1']==])" > /dev/null 2>&1
"$wtype_bin" -s 170000 -k Shift_L &
echo \$! > "\$m-keyboard.pid"
for _ in \$(seq 1 120); do grep -qF ' ready' "\$log" 2>/dev/null && break; sleep 0.5; done
until_jq 30 "\$m-clients.json" 'any(.[]; .class == "electronpanel")' "$wmctl_bin" -j clients
"$wmctl_bin" dispatch "hl.dsp.focus({ window = 'class:^(electronpanel)\\\$' })" > /dev/null 2>&1
until_jq 30 "\$m-menu.json" '.pid != null and (.titles | length) > 0' call appmenu status
height > "\$m-height.txt"
timeout 200 busctl --user monitor --match "type='signal',interface='org.freedesktop.DBus.Properties',path='/com/canonical/AppMenu/Registrar'" > "\$m-signals.txt" 2>&1 &
mon=\$!
sleep 1  # the monitor has to be on the bus before the first change
fs electronpanel set
until_jq 10 "\$m-hidden-dump.json" '.reveal.sensor and (.fullscreenOutputs | length > 0)' call debug dump
dump hidden
"$wmctl_bin" -j clients > "\$m-hidden-clients.json" 2>&1
shoot "\$m-hidden.png"

# The real pointer to the top edge brings the bar up over the window.
park "\$cx" 0
until_jq 10 "\$m-shown-dump.json" '.reveal.shown' call debug dump
until_settled bar
dump revealed
"$wmctl_bin" -j clients > "\$m-revealed-clients.json" 2>&1
shoot "\$m-revealed.png"

read -r tx ty <<< "\$(title_at 1)"
echo "\$tx \$ty" > "\$m-title.txt"
park "\$tx" "\$ty"
click
until_jq 10 "\$m-open.json" '.open != null' call appmenu status
until_settled
dump menu
shoot "\$m-menu.png"
click
until_jq 10 "\$m-shut.json" '.open == null' call appmenu status
park "\$cx" "\$cy"
until_jq 10 "\$m-gone-dump.json" '.reveal.shown == false' call debug dump
dump gone
shoot "\$m-gone.png"

# A menu left open holds the bar with the pointer off it.
park "\$cx" 0
until_jq 10 "\$m-again-dump.json" '.reveal.shown' call debug dump
until_settled bar
read -r tx ty <<< "\$(title_at 1)"
park "\$tx" "\$ty"
click
until_jq 10 "\$m-open2.json" '.open != null' call appmenu status
park "\$cx" "\$cy"
sleep 1.5  # three times the hold, which is what the bar has to outlast
dump held
shoot "\$m-held.png"
call appmenu close > /dev/null 2>&1
until_jq 10 "\$m-closed-dump.json" '.reveal.shown == false' call debug dump
dump closed

fs electronpanel unset
until_jq 10 "\$m-after-dump.json" '.fullscreenOutputs == [] and .reveal.sensor == false' call debug dump
until_settled bar
dump after

# Height over the bus: on the top edge, on the bottom, and back.
height > "\$m-height-top.txt"
settings_retarget '.bar.position = "bottom"'
for _ in \$(seq 1 20); do [ "\$(height)" = "u 0" ] && break; sleep 0.5; done
height > "\$m-height-bottom.txt"
settings_retarget '.bar.position = "top"'
for _ in \$(seq 1 20); do [ "\$(height)" = "\$(cat "\$m-height-top.txt")" ] && break; sleep 0.5; done
height > "\$m-height-back.txt"
sleep 1  # the last PropertiesChanged reaching the monitor
kill "\$mon" 2>/dev/null
kill "\$(cat "\$m-keyboard.pid")" 2>/dev/null
EOF
  write_script "$shot_dir/mac-fsbar-kill.sh" <<EOF
#!/usr/bin/env bash
pkill -f "\$(cat "$mac_fsbar_binary")" 2>/dev/null || true
kill "\$(cat "$mac_fsbar_prefix-keyboard.pid" 2>/dev/null)" 2>/dev/null || true
EOF
  add_cleanup "bash $shot_dir/mac-fsbar-kill.sh"
  wm_exec_once "bash $script"
}

# _mac_fsbar_level <layers> <namespace> <level>: that namespace's surfaces on
# one level, 2 the top layer and 3 the overlay.
_mac_fsbar_level() {
  "$jq_bin" -r --arg ns "$2" --arg l "$3" '[.[] | .levels[$l][]? | select(.namespace == $ns)] | length' "$1" 2>/dev/null
}

_mac_fsbar_size() {
  "$jq_bin" -c '[.[] | select(.class == "electronpanel" and .fullscreen != 0 and .fullscreen != false) | .size] | first' "$1" 2>/dev/null
}

leg_mac_fullscreen_bar_assert() {
  local m="$mac_fsbar_prefix" shot phase
  for shot in hidden revealed menu gone held; do
    [ ! -f "$m-$shot.png" ] || echo "SMOKE_MAC_FSBAR_$(echo "$shot" | tr 'a-z' 'A-Z') $m-$shot.png"
  done
  for phase in game over hidden revealed menu gone held closed after; do
    echo "$phase: reveal=$("$jq_bin" -c .reveal "$m-$phase-dump.json" 2>/dev/null) Revealed='$(cat "$m-$phase-revealed.txt" 2>/dev/null)'" \
      "bar top=$(_mac_fsbar_level "$m-$phase-layers.json" formalshell:bar 2) overlay=$(_mac_fsbar_level "$m-$phase-layers.json" formalshell:bar 3)" \
      "line=$(_mac_fsbar_level "$m-$phase-layers.json" formalshell:reveal 3)"
  done
  { grep -E 'bar revealed|bar reveal ended' "$shell_log_path" 2>/dev/null || true; } | tail -n 8

  expect_jq "$m-game-dump.json" '.reveal.sensor == false and .reveal.shown == false' "a fullscreen window that is not a Mac app's got the line or the bar"
  [ "$(_mac_fsbar_level "$m-game-layers.json" formalshell:bar 2)$(_mac_fsbar_level "$m-game-layers.json" formalshell:bar 3)" = 00 ] \
    || fail "--mac-fullscreen-bar: the bar was mapped over the fullscreen game"
  expect_jq "$m-menu.json" '.pid != null' "the fixture's process registered no menu"
  expect_jq "$m-over-dump.json" '.fullscreenOutputs | length > 0' "the game did not cover the output under the focused Mac window"
  expect_jq "$m-over-dump.json" '.reveal.sensor == false and .reveal.shown == false' "a Mac window focused over a game covering the output got the line or the bar"
  [ "$(_mac_fsbar_level "$m-over-layers.json" formalshell:bar 2)$(_mac_fsbar_level "$m-over-layers.json" formalshell:bar 3)" = 00 ] \
    || fail "--mac-fullscreen-bar: the bar was mapped over a game covering the output under a focused Mac window"

  [ "$(_mac_fsbar_level "$m-hidden-layers.json" formalshell:bar 2)$(_mac_fsbar_level "$m-hidden-layers.json" formalshell:bar 3)" = 00 ] \
    || fail "--mac-fullscreen-bar: the bar stayed mapped over the fullscreen Mac window before the pointer reached the edge"
  [ "$(_mac_fsbar_level "$m-hidden-layers.json" formalshell:reveal 3)" = 1 ] \
    || fail "--mac-fullscreen-bar: no line on the overlay level over the fullscreen Mac window"

  expect_jq "$m-revealed-dump.json" '.reveal.shown' "the pointer at the top edge did not bring the bar up"
  [ "$(_mac_fsbar_level "$m-revealed-layers.json" formalshell:bar 3)" = 1 ] \
    || fail "--mac-fullscreen-bar: the revealed bar is not on the overlay level"
  local before after
  before=$(_mac_fsbar_size "$m-hidden-clients.json")
  after=$(_mac_fsbar_size "$m-revealed-clients.json")
  echo "fullscreen window size: hidden=$before revealed=$after"
  [ -n "$before" ] && [ "$before" != null ] || fail "--mac-fullscreen-bar: the Mac window was not fullscreen"
  [ "$before" = "$after" ] || fail "--mac-fullscreen-bar: the revealed bar resized the fullscreen window ($before to $after)"

  expect_jq "$m-open.json" '.open != null' "a click on a menu title opened no menu"
  [ "$(_mac_fsbar_level "$m-menu-layers.json" formalshell:panel 3)" -ge 1 ] \
    || fail "--mac-fullscreen-bar: the menu card is not on the overlay level"
  expect_jq "$m-gone-dump.json" '.reveal.shown == false and .reveal.sensor' "the bar stayed with the pointer moved off it and no menu open"
  [ "$(_mac_fsbar_level "$m-gone-layers.json" formalshell:bar 3)" = 0 ] \
    || fail "--mac-fullscreen-bar: the bar layer stayed on the overlay level after the reveal ended"

  expect_jq "$m-held-dump.json" '.reveal.shown' "the bar went with a menu open"
  expect_jq "$m-closed-dump.json" '.reveal.shown == false' "the bar stayed once the open menu shut with the pointer off it"

  [ "$(_mac_fsbar_level "$m-after-layers.json" formalshell:bar 2)" = 1 ] \
    || fail "--mac-fullscreen-bar: the bar is not back on the top level out of fullscreen"
  [ "$(_mac_fsbar_level "$m-after-layers.json" formalshell:reveal 3)" = 0 ] \
    || fail "--mac-fullscreen-bar: the line stayed out of fullscreen"

  local phase want
  for phase in game:false hidden:false revealed:true menu:true gone:false held:true closed:false after:false; do
    want="b ${phase#*:}"
    [ "$(cat "$m-${phase%%:*}-revealed.txt" 2>/dev/null)" = "$want" ] \
      || fail "--mac-fullscreen-bar: Revealed reads '$(cat "$m-${phase%%:*}-revealed.txt" 2>/dev/null)' at ${phase%%:*}, not '$want'"
  done
  local reveals
  reveals=$(grep -c '"Revealed"' "$m-signals.txt" 2>/dev/null || true)
  echo "org.atoll.MenuBar Revealed PropertiesChanged=$reveals"
  [ "${reveals:-0}" -ge 4 ] || fail "--mac-fullscreen-bar: $reveals PropertiesChanged for Revealed over two reveals and their ends"

  local top bottom back changes
  top=$(cat "$m-height-top.txt" 2>/dev/null)
  bottom=$(cat "$m-height-bottom.txt" 2>/dev/null)
  back=$(cat "$m-height-back.txt" 2>/dev/null)
  changes=$(grep -c '"Height"' "$m-signals.txt" 2>/dev/null || true)
  echo "org.atoll.MenuBar Height: top='$top' bottom='$bottom' back='$back' PropertiesChanged=$changes"
  echo "SMOKE_MAC_FSBAR_HEIGHT $top"
  [[ "$top" =~ ^u\ [1-9][0-9]*$ ]] || fail "--mac-fullscreen-bar: Height on the top edge reads '$top'"
  [ "$bottom" = "u 0" ] || fail "--mac-fullscreen-bar: Height with the bar on the bottom reads '$bottom'"
  [ "$back" = "$top" ] || fail "--mac-fullscreen-bar: Height back on the top edge reads '$back', not '$top'"
  [ "${changes:-0}" -ge 2 ] || fail "--mac-fullscreen-bar: $changes PropertiesChanged for Height over two changes"
}
