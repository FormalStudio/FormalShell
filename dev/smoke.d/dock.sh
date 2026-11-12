# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154,SC2016  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --dock: the dock, against dev/mac-stub.py as the Mac app
# (docs/superpowers/specs/2026-11-11-mac-apps.md, section 4).
# With a foot window alone the dock has no surface: `dock status` says it
# does not exist, no layer is in the formalshell:dock namespace, and the
# pointer resting on the bottom edge brings nothing out. The stub's window
# gives it its strip and nothing else. Everything after is a real pointer,
# with `dock status` only read for where things are:
# the pointer parked on the bottom edge brings the card out with both apps
# in it; the stub's tile, sent as atoll sends it, lands as a count, a
# progress and then a badge that is not a number; a click on foot's icon
# moves focus to foot and one on the stub's icon back; a right click on the
# stub's icon opens its menu with the stub's own dock menu, its window, its
# desktop entry's action, Keep in Dock and Quit, and a click on New
# Document reaches the stub's dock menu; the dock goes once the menu shut with the pointer off it; Keep
# in Dock pins the stub ahead of a rule; with foot in front the stub
# turning urgent brings the dock out by itself, its icon hops (read off
# where the icon's own colour starts in a run of frames) and the dock goes
# again;
# `dock open` holds it out and `dock activate` moves focus with no pointer;
# Quit in the menu reaches the stub as its own Quit item, and with its
# window gone the dock has no surface again though the stub is still
# pinned.
leg_dock_flag="--dock"
leg_dock_order=174
leg_dock_needs="python3 jq foot wlrctl convert"
leg_dock_claims="screen=shared"

dock_prefix="$shot_dir/dock"
dock_log="$shot_dir/dock-stub.log"

# The stub has a desktop entry, so its icon, its name and its entry's
# action are the entry's; foot has none and keeps the glyph and its app id.
leg_dock_fixture() {
  local icons="$iso_home/.local/share/icons/hicolor"
  mkdir -p "$iso_home/.local/share/applications" "$icons/128x128/apps"
  $convert_bin -size 128x128 xc:none -fill '#E0A021' -draw 'roundrectangle 6,6,121,121,28,28' "$icons/128x128/apps/org.atoll.stub.png"
  [ -f "$icons/index.theme" ] || cat > "$icons/index.theme" <<'EOF'
[Icon Theme]
Name=Hicolor
Comment=Fallback icon theme
Directories=128x128/apps

[128x128/apps]
Size=128
Context=Applications
Type=Threshold
EOF
  cat > "$iso_home/.local/share/applications/org.atoll.stub.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Stub
Exec=true
Icon=org.atoll.stub
Actions=blank;

[Desktop Action blank]
Name=New Blank Stub
Exec=true
EOF
}

leg_dock_timing() {
  leg_timing 120
}

leg_dock_drive() {
  local script="$shot_dir/dock-drive.sh"
  local stub="$PWD/dev/mac-stub.py"
  write_script "$script" <<EOF
#!/usr/bin/env bash
m="$dock_prefix"
log="$dock_log"
jq="$jq_bin"
stub=org.atoll.stub
foot=formalshell-dock
read -r W H <<< "\$("$hyprctl_bin" -j monitors | "\$jq" -r '.[0] | "\(.width / .scale | round) \(.height / .scale | round)"')"
active() { "$hyprctl_bin" -j activewindow | "\$jq" -r .class; }
# front <class>: until that window is the active one and the dock has
# heard, 10s at most. The dock goes when focus moves with the pointer off
# it, so a pointer moved before the shell's own view of the windows caught
# up would find it going.
front() {
  for _ in \$(seq 1 40); do [ "\$(active)" = "\$1" ] && break; sleep 0.25; done
  [ "\$(active)" = "\$1" ] && until_jq 10 "\$m-front.json" "any(.apps[]; .app == \\"\$1\\" and .focused)" call dock status
}
layers() { "$hyprctl_bin" -j layers > "\$1" 2>&1; }
# need <step...>: the step, or the drive over at the first one that failed,
# named in \$m-gave-up.txt beside the dock's status then. Every later step
# stands on this one, and a run of waits each at its limit outlasts the
# session.
need() {
  "\$@" && return
  echo "\$*" > "\$m-gave-up.txt"
  call dock status > "\$m-gave-up.json" 2>&1
  exit 0
}
tile() { "$python3_bin" "$stub" --tile "\$@" >> "\$m-tile.log" 2>&1; }
# The middle of an app's cell, and of a row of the open menu, off the
# status in \$2.
cell() { "\$jq" -r --arg app "\$1" '.apps[] | select(.app == \$app) | .rect | "\(.x + (.width / 2 | floor)) \(.y + (.height / 2 | floor))"' "\$2"; }
row() { "\$jq" -r --arg label "\$1" '.menu.rows[] | select(.label == \$label) | .rect | "\(.x + (.width / 2 | floor)) \(.y + (.height / 2 | floor))"' "\$2"; }
# edge <path>: the pointer onto the bottom edge, and the status once the
# card is out and at rest.
edge() {
  park \$((W / 2)) \$((H - 1))
  until_jq 10 "\$1" '.shown and all(.apps[]; .rect != null)' call dock status || return 1
  until_settled
  call dock status > "\$1" 2>&1
}
# menu <path>: a right click on the stub's icon, and the status once its
# own dock menu has landed in the card.
menu() {
  edge "\$1" || return 1
  park \$(cell \$stub "\$1"); click right
  until_jq 10 "\$1" '.menu.app == "org.atoll.stub" and any(.menu.rows[]; .label == "New Document")' call dock status || return 1
  until_settled
  call dock status > "\$1" 2>&1
}
hidden() { until_jq 10 "\$1" '.shown | not' call dock status; }

spawn_foot \$foot
need until_window \$foot
need front \$foot
need until_jq 10 "\$m-native.json" 'any(.apps[]; .app == "formalshell-dock")' call dock status
layers "\$m-native-layers.json"
park \$((W / 2)) \$((H - 1))
sleep 1
call dock status > "\$m-native-edge.json" 2>&1
park

"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$python3_bin $stub --name Stub --log \$log]==])" > /dev/null 2>&1
need until_window \$stub 15
need front \$stub
need until_jq 10 "\$m-strip.json" '.exists and .strip and any(.apps[]; .app == "org.atoll.stub" and .icon)' call dock status
layers "\$m-strip-layers.json"
until_settled
shoot "\$m-hidden.png"

need edge "\$m-out.json"
shoot "\$m-out.png"

tile count=3 count-visible=true progress=0.4 progress-visible=true
need until_jq 10 "\$m-count.json" 'any(.apps[]; .app == "org.atoll.stub" and .badge == "3" and .progress == 0.4)' call dock status
until_settled
shoot "\$m-count.png"
tile count=0 count-visible=true x-atoll-badge=New progress-visible=false
need until_jq 10 "\$m-badge.json" 'any(.apps[]; .app == "org.atoll.stub" and .badge == "New" and .progress == null)' call dock status
until_settled
shoot "\$m-badge.png"

park \$(cell \$foot "\$m-out.json"); click
front \$foot; echo "\$?" > "\$m-click-foot.txt"
call dock status > "\$m-on-foot.json" 2>&1
park \$(cell \$stub "\$m-out.json"); click
front \$stub; echo "\$?" > "\$m-click-stub.txt"
call dock status > "\$m-on-stub.json" 2>&1

need menu "\$m-menu.json"
shoot "\$m-menu.png"
park \$(row "New Document" "\$m-menu.json"); click
for _ in \$(seq 1 20); do grep -q '^dock: Event(1, clicked)\$' "\$log" 2>/dev/null && break; sleep 0.25; done
need until_jq 10 "\$m-menu-shut.json" '.menu == null' call dock status
need hidden "\$m-gone.json"

need menu "\$m-menu-keep.json"
park \$(row "Keep in Dock" "\$m-menu-keep.json"); click
need until_jq 10 "\$m-pinned.json" '.apps[0].app == "org.atoll.stub" and .apps[0].pinned and .rule == 1' call dock status
need edge "\$m-ruled.json"
shoot "\$m-ruled.png"
park
need hidden "\$m-left.json"

"$hyprctl_bin" dispatch "hl.dsp.focus({ window = 'class:\$foot' })" > /dev/null 2>&1
need front \$foot
need hidden "\$m-before-urgent.json"
tile urgent=true
need until_jq 10 "\$m-urgent.json" '.shown and any(.apps[]; .app == "org.atoll.stub" and .urgent)' call dock status
burst "\$m-hop" 12 0 -g "\$("\$jq" -r '.rect | "\(.x - 20),\(.y - 40) \(.width + 40)x\(.height + 44)"' "\$m-ruled.json")"
need hidden "\$m-urgent-gone.json"
tile urgent=false

call dock open > "\$m-open.txt" 2>&1
need until_jq 10 "\$m-held.json" '.shown and .held' call dock status
call dock activate \$stub > /dev/null 2>&1
front \$stub; echo "\$?" > "\$m-activate.txt"
sleep 1
call dock status > "\$m-held-refocused.json" 2>&1
call dock close > /dev/null 2>&1
need hidden "\$m-dismissed.json"

need menu "\$m-menu-quit.json"
park \$(row "Quit" "\$m-menu-quit.json"); click
for _ in \$(seq 1 20); do grep -qx quit "\$log" 2>/dev/null && break; sleep 0.25; done
need until_jq 10 "\$m-quit.json" '(.exists | not) and any(.apps[]; .app == "org.atoll.stub" and .pinned and (.running | not))' call dock status
layers "\$m-quit-layers.json"
call dock unpin \$stub > /dev/null 2>&1
EOF
  write_script "$shot_dir/dock-kill.sh" <<EOF
#!/usr/bin/env bash
pkill -f "$stub --name Stub --log $dock_log" 2>/dev/null || true
EOF
  add_cleanup "bash $shot_dir/dock-kill.sh"
  hypr_exec_once "bash $script"
}

leg_dock_assert() {
  local m="$dock_prefix"
  local in_dock="$layers_jq"'layer("formalshell:dock") | length'
  local stub='.apps[] | select(.app == "org.atoll.stub")'
  [ -s "$m-native.json" ] || fail "--dock: no dock status"
  local shot
  for shot in hidden out count badge menu ruled $(seq -f 'hop-%g' 1 12); do
    [ ! -f "$m-$shot.png" ] || echo "SMOKE_DOCK_$(echo "$shot" | tr 'a-z-' 'A-Z_') $m-$shot.png"
  done
  echo "SMOKE_DOCK_SHELL $shell_log_path"
  [ ! -f "$m-gave-up.txt" ] || fail "--dock: the drive stopped at a step that never came true: $(cat "$m-gave-up.txt"): $(cat "$m-gave-up.json" 2>/dev/null)"
  expect_jq "$m-native.json" '(.exists | not) and (.strip | not) and (.shown | not)' "--dock: a foot window alone gave the dock a surface"
  expect_jq "$m-native-layers.json" "$in_dock == 0" "--dock: a layer is in the dock's namespace with no Mac window" "$layers_jq"'[layers[].namespace]'
  expect_jq "$m-native-edge.json" '.shown | not' "--dock: the pointer on the bottom edge brought the dock out with no Mac window"
  cat "$m-out.json"; echo
  expect_jq "$m-strip.json" '.exists and .strip and (.shown | not)' "--dock: the stub's window did not give the dock its strip alone"
  expect_jq "$m-strip-layers.json" "$in_dock == 1" "--dock: the strip is not the one layer in the dock's namespace" "$layers_jq"'[layers[].namespace]'
  expect_jq "$m-out.json" '.shown and .edge == "bottom" and (.held | not) and ([.apps[].app] == ["formalshell-dock", "org.atoll.stub"]) and .rule == null' \
    "--dock: the pointer on the bottom edge did not bring out foot and the stub"
  expect_jq "$m-out.json" "$stub"' | .mac and .running and .focused and .windows == 1 and (.pinned | not)' "--dock: the stub is not a running, focused Mac app in the dock"
  expect_jq "$m-out.json" "$stub"' | .name == "Stub" and .icon' "--dock: the stub does not carry its desktop entry's name and icon"
  expect_jq "$m-out.json" '.apps[0] | (.mac | not) and .running and (.focused | not)' "--dock: foot is not a running app behind the stub"
  expect_jq "$m-out.json" '.rect as $r | all(.apps[]; .rect.x >= $r.x and .rect.x + .rect.width <= $r.x + $r.width and .rect.y >= $r.y and .rect.y + .rect.height <= $r.y + $r.height)' \
    "--dock: a cell lies outside the card"
  [ -s "$dock_log" ] || fail "--dock: the stub logged nothing"
  cat "$dock_log"
  expect_jq "$m-count.json" "$stub"' | .badge == "3" and .progress == 0.4' "--dock: the tile's count and progress did not land on the stub's icon"
  expect_jq "$m-badge.json" "$stub"' | .badge == "New" and .progress == null' "--dock: the tile's text badge did not replace its count, or its progress stayed"
  [ "$(cat "$m-click-foot.txt" 2>/dev/null)" = 0 ] || fail "--dock: a click on foot's icon did not move focus to foot"
  expect_jq "$m-on-foot.json" '.shown' "--dock: focus moving by a click in the dock put the dock away under the pointer"
  [ "$(cat "$m-click-stub.txt" 2>/dev/null)" = 0 ] || fail "--dock: a click on the stub's icon did not move focus back to it: $(cat "$m-on-stub.json" 2>/dev/null)"
  expect_jq "$m-menu.json" '[.menu.rows[].label] | index("New Document") != null and index("Show All Windows") != null and index("Stub") != null and index("New Window") != null and index("New Blank Stub") != null and index("Keep in Dock") != null and index("Quit") != null' \
    "--dock: the stub's menu does not hold its own dock menu, its window, its entry's action, Keep in Dock and Quit" '.menu'
  expect_jq "$m-menu.json" '.rect as $d | all(.menu.rows[]; .rect.y + .rect.height <= $d.y)' "--dock: the menu does not stand above the dock" '{rect, menu}'
  grep -qxF 'dock: Event(1, clicked)' "$dock_log" || fail "--dock: a click on New Document never reached the stub's dock menu"
  expect_jq "$m-menu-shut.json" '.menu == null' "--dock: the menu stayed up after its row was clicked"
  expect_jq "$m-gone.json" '.shown | not' "--dock: the dock stayed out after its menu shut with the pointer off it"
  expect_jq "$m-pinned.json" '.apps[0].app == "org.atoll.stub" and .apps[0].pinned and .rule == 1' "--dock: Keep in Dock did not pin the stub ahead of a rule"
  expect_jq "$m-left.json" '.shown | not' "--dock: the dock stayed out with the pointer gone from it"
  expect_jq "$m-urgent.json" '.shown and (.held | not)' "--dock: the stub turning urgent did not bring the dock out"
  # The row the stub's icon starts on, in the frame's own pixels: the top
  # of its own colour's box. A hop is that row above where the icon rests,
  # which the frame of the pinned dock at rest gives.
  icon_top() {
    local w top
    IFS='x+' read -r w _ _ top <<< "$("$convert_bin" "$1" -fuzz 8% -fill black +opaque '#E0A021' -fill white -opaque '#E0A021' -format '%@' info: 2>/dev/null)"
    [ "${w:-0}" -lt "$(logical_px 30)" ] || echo "$top"
  }
  local rest tops lift
  rest=$(icon_top "$m-ruled.png")
  lift=$(logical_px "$("$jq_bin" '.rect.y - 40' "$m-ruled.json")")
  tops=$(for shot in $(seq 1 12); do icon_top "$m-hop-$shot.png"; done | sort -n | head -n 1)
  echo "hop: the icon rests on row ${rest:-none} and stood highest on row $((${tops:-9999} + lift))"
  [ -n "$rest" ] && [ "$((rest - ${tops:-9999} - lift))" -ge "$(logical_px 4)" ] \
    || fail "--dock: the urgent stub's icon never stood above where it rests (row ${rest:-none}, highest $((${tops:-9999} + lift)))"
  expect_jq "$m-urgent-gone.json" '.shown | not' "--dock: the dock stayed out after the stub's hops"
  expect_jq "$m-held.json" '.shown and .held' "--dock: dock open did not hold the dock out: $(cat "$m-open.txt" 2>/dev/null)"
  [ "$(cat "$m-activate.txt" 2>/dev/null)" = 0 ] || fail "--dock: dock activate did not move focus to the stub"
  expect_jq "$m-held-refocused.json" '.shown and .held' "--dock: focus moving put a held dock away"
  expect_jq "$m-dismissed.json" '.shown | not' "--dock: dock close left the dock out"
  grep -qxF 'menu: Event(15, clicked)' "$dock_log" || fail "--dock: Quit did not reach the stub as its own Quit item"
  grep -qxF quit "$dock_log" || fail "--dock: the stub did not quit"
  expect_jq "$m-quit.json" '(.exists | not) and (.strip | not)' "--dock: the dock kept a surface after the last Mac window went"
  expect_jq "$m-quit-layers.json" "$in_dock == 0" "--dock: a layer stayed in the dock's namespace after the stub quit" "$layers_jq"'[layers[].namespace]'
  for shot in hidden out count badge menu ruled hop-1; do
    [ -f "$m-$shot.png" ] || fail "--dock: no $shot frame"
  done
}
