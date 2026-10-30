# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, iso_home, the *_bin paths and fail()
# --launcher-mouse: the launcher under a real pointer (launcher spec §6),
# read off `menu status` (`cursorId`, `hover`, `actionHover`,
# `actionsOpen`, `actionsScroll`, `footHints`, `actionsBox`) and off the
# services a row drives. wlrctl's pointer is relative only, so every move
# slams it into the corner first and goes from there; dev/vpointer.py
# sends the touchpad frames wlrctl cannot.
#
# Where rows are comes off the open status: row 0's `viewCursor` and the
# pitch a real Down moves it by, the levels read here being one section
# each. The pointer crossing five root rows leaves `cursorId` on row 0 and
# moves `hover` row by row. In the toggle hub a click on Caffeinate runs
# it (caffeinated, ticked, the hub still open on it); a right-click on Do
# Not Disturb opens its actions with it selected and DND as it was; a
# click on Overnight with the column open closes the column and selects
# it, overnight still off; the foot's Enter button then runs it. At the
# root filtered to Toggles the Actions, Back and Into buttons do Tab,
# Shift+Tab and →.
#
# The staged Mouse Probe app carries 16 menu.jsonc actions, more than the
# column shows, beside 20 Mouse Probe Extra apps that overflow the list. A
# wheel notch and a touchpad flick over the column scroll the column and
# leave the list, its cursor and the column's cursor where they were; a
# notch over the list scrolls the list and leaves the column. A click
# outside the card closes it. The hover beside the selection and the
# right-click column are photographed in dark and light.
leg_launcher_mouse_flag="--launcher-mouse"
leg_launcher_mouse_order=31
leg_launcher_mouse_needs="jq wtype wlrctl python3"

launcher_mouse_app="apps.formalshell-mouse-probe"
launcher_mouse_vpointer="$PWD/dev/vpointer.py"

leg_launcher_mouse_validate() {
  local other
  for other in launcher_actions launcher_modes launcher_abbrev menu_actions menu_hover wheel overnight caffeinate; do
    if leg_on "$other"; then
      echo "usage: --launcher-mouse drives the launcher with a pointer of its own and cannot combine with --${other//_/-}" >&2
      exit 1
    fi
  done
}

leg_launcher_mouse_fixture() {
  local apps="$iso_home/.local/share/applications" i actions=""
  mkdir -p "$apps" "$iso_home/.config/formalshell"
  printf '[Desktop Entry]\nType=Application\nName=Mouse Probe\nExec=true\n' > "$apps/formalshell-mouse-probe.desktop"
  for i in $(seq -w 1 20); do
    printf '[Desktop Entry]\nType=Application\nName=Mouse Probe Extra %s\nExec=true\n' "$i" > "$apps/formalshell-mouse-extra-$i.desktop"
  done
  for i in $(seq -w 1 16); do
    actions="$actions${actions:+, }{ \"id\": \"mouse-$i\", \"label\": \"Mouse action $i\", \"action\": \"true\" }"
  done
  printf '{\n  "%s": { "actions": [ %s ] }\n}\n' "$launcher_mouse_app" "$actions" > "$iso_home/.config/formalshell/menu.jsonc"
}

leg_launcher_mouse_timing() {
  leg_timing 60 110
}

leg_launcher_mouse_drive() {
  local script="$shot_dir/launcher-mouse-drive.sh" p="$shot_dir/launcher-mouse"
  write_script "$script" <<EOF
#!/usr/bin/env bash
call() { $ipc call "\$@"; }
status() { call menu status > "$p-\$1.json" 2>&1; }
shoot() { "$grim_bin" "$p-\$1.png" > /dev/null 2>&1; }
at() { "$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1; "$wlrctl_bin" pointer move "\$1" "\$2" > /dev/null 2>&1; sleep 0.4; }
click() { "$wlrctl_bin" pointer click "\${1:-left}" > /dev/null 2>&1; sleep 0.8; }
# The middle of foot button \$2 in status \$1.
hint() { "$jq_bin" -r --arg on "\$2" '.footHints[] | select(.on == \$on) | "\(.x + .width / 2) \(.y + .height / 2)"' "$p-\$1.json" | head -n 1; }
# Row 0's box off status \$1, taken with the cursor on it; then row \$1's
# middle line off that box and the pitch.
rows_from() { read -r top bottom < <("$jq_bin" -r '[.viewCursor.top, .viewCursor.bottom] | @tsv' "$p-\$1.json"); }
row_y() { echo \$((by + top + \$1 * pitch + (bottom - top) / 2)); }
index() { "$jq_bin" -r --arg id "\$2" '.ids | index(\$id) // -1' "$p-\$1.json"; }
sleep 4
"$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1
call menu summon "" > /dev/null 2>&1
sleep 1.5
status root
"$wtype_bin" -k Down
sleep 0.5
status root-down
"$wtype_bin" -k Up
sleep 0.5
read -r bx by top bottom left < <("$jq_bin" -r '[.body.x, .body.y, .viewCursor.top, .viewCursor.bottom, .viewCursor.left] | @tsv' "$p-root.json")
pitch=\$(( \$("$jq_bin" -r '.viewCursor.top' "$p-root-down.json") - top ))
x=\$((bx + left + 40))
echo "\$bx \$by \$top \$bottom \$left \$pitch" > "$p-geometry.txt"

at "\$x" "\$(row_y 0)"
"$wlrctl_bin" pointer move 0 1 > /dev/null 2>&1
for i in 0 1 2 3 4; do
  sleep 0.5
  status "hover-\$i"
  [ "\$i" = 2 ] && shoot hover-dark
  "$wlrctl_bin" pointer move 0 "\$pitch" > /dev/null 2>&1
done

call menu summon toggles > /dev/null 2>&1
sleep 1.5
status hub
rows_from hub
caffeinate=\$(index hub toggles.caffeinate)
dnd=\$(index hub toggles.dnd)
overnight=\$(index hub toggles.overnight)
at "\$x" "\$(row_y "\$caffeinate")"
click
status clicked
call caffeinate status > "$p-caffeinate.json" 2>&1
call notifications dndState > "$p-dnd-before.txt" 2>&1
at "\$x" "\$(row_y "\$dnd")"
click right
status right
call notifications dndState > "$p-dnd-after.txt" 2>&1
shoot column-dark
at "\$x" "\$(row_y "\$overnight")"
click
status closed
call overnight status > "$p-overnight-click.json" 2>&1
read -r hx hy < <(hint closed primary)
at "\${hx%.*}" "\${hy%.*}"
click
status enter
call overnight status > "$p-overnight-enter.json" 2>&1
call overnight disable > /dev/null 2>&1
call caffeinate disable > /dev/null 2>&1

call menu summon "" > /dev/null 2>&1
call menu filter Toggles > /dev/null 2>&1
sleep 1
status hints
from=hints
for on in actions column-back into; do
  read -r hx hy < <(hint "\$from" "\$on")
  at "\${hx%.*}" "\${hy%.*}"
  click
  status "hint-\$on"
  from="hint-\$on"
done

call menu summon "" > /dev/null 2>&1
call menu filter "Mouse Probe" > /dev/null 2>&1
sleep 1
status probe
rows_from probe
at "\$x" "\$(row_y 0)"
click right
status column
read -r cx cy < <("$jq_bin" -r '"\(.actionsBox.x + .actionsBox.width / 2) \(.actionsBox.y + .actionsBox.height / 2)"' "$p-column.json")
at "\${cx%.*}" "\${cy%.*}"
status column-hover
"$wlrctl_bin" pointer scroll 15 0 > /dev/null 2>&1
sleep 1
status column-wheel
"$python3_bin" "$launcher_mouse_vpointer" finger 30 wait 16 finger 30 wait 100 lift > /dev/null 2>&1
sleep 1.5
status column-finger
shoot column-scrolled
at "\$x" "\$(row_y 1)"
"$wlrctl_bin" pointer scroll 15 0 > /dev/null 2>&1
sleep 1
status list-wheel

# Left of the 840 card the column widened it to.
ox=\$("$jq_bin" -r '.body.x' "$p-list-wheel.json")
at \$((ox - 80)) \$((by + 40))
click
status outside

call theme mode light > /dev/null 2>&1
sleep 2
call theme status > "$p-theme.json" 2>&1
call menu summon "" > /dev/null 2>&1
sleep 1
rows_from root
at "\$x" "\$(row_y 2)"
shoot hover-light
call menu summon toggles > /dev/null 2>&1
sleep 1
rows_from hub
at "\$x" "\$(row_y "\$dnd")"
click right
shoot column-light
call menu close > /dev/null 2>&1
"$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

# `<step> <jq claim> <what it means>`, against that step's status.
launcher_mouse_expect() {
  local f="$shot_dir/launcher-mouse-$1.json"
  [ -s "$f" ] || fail "launcher-mouse: no status for $1"
  "$jq_bin" -e "$2" "$f" > /dev/null \
    || fail "launcher-mouse $1: $3, got $("$jq_bin" -c '{isOpen, level, cursor, cursorId, hover, actionHover, actionsOpen, actionCursor, actionsScroll, actionsScrollMax, scrollTop, scrollMax, ids: .ids[:4]}' "$f")"
}

leg_launcher_mouse_assert() {
  local p="$shot_dir/launcher-mouse" i first
  [ -s "$p-geometry.txt" ] || fail "launcher-mouse: the drive never read the rows' geometry"
  echo "launcher-mouse geometry (body x y, row 0 top bottom left, pitch): $(cat "$p-geometry.txt")"
  launcher_mouse_expect root '.sections | length <= 1' "the root is more than one section, so the pitch does not hold down it"
  launcher_mouse_expect root-down '.cursor == 1' "a real Down did not move the cursor to row 1"
  first=$("$jq_bin" -r '.cursorId' "$p-root.json")
  for i in 0 1 2 3 4; do
    launcher_mouse_expect "hover-$i" ".cursorId == \"$first\" and .hover == $i" "the pointer on row $i did not hover it alone"
  done
  echo "SMOKE_LAUNCHER_MOUSE the pointer crossed five rows, hover 0..4, the selection held on $first"

  launcher_mouse_expect hub '.sections | length <= 1' "the toggle hub is more than one section"
  launcher_mouse_expect clicked '.isOpen and .level == "toggles" and .cursorId == "toggles.caffeinate" and (.checked | index("toggles.caffeinate")) != null' "a click on Caffeinate did not select and run it with the hub open"
  "$jq_bin" -e '.active == true' "$p-caffeinate.json" > /dev/null || fail "launcher-mouse: the click did not caffeinate: $(cat "$p-caffeinate.json")"
  echo "SMOKE_LAUNCHER_MOUSE a click ran the keepOpen Caffeinate row"

  launcher_mouse_expect right '.actionsOpen and .cursorId == "toggles.dnd" and (.actions | length) > 0' "a right-click on Do Not Disturb did not open its actions with it selected"
  [ "$(cat "$p-dnd-before.txt")" = "$(cat "$p-dnd-after.txt")" ] \
    || fail "launcher-mouse: the right-click ran Do Not Disturb ($(cat "$p-dnd-before.txt") -> $(cat "$p-dnd-after.txt"))"
  echo "SMOKE_LAUNCHER_MOUSE a right-click opened Do Not Disturb's actions and ran nothing"

  launcher_mouse_expect closed '(.actionsOpen | not) and .cursorId == "toggles.overnight"' "a click on Overnight with the column open did not close it onto Overnight"
  "$jq_bin" -e '.active == false' "$p-overnight-click.json" > /dev/null || fail "launcher-mouse: the closing click ran Overnight: $(cat "$p-overnight-click.json")"
  launcher_mouse_expect enter '.isOpen and .cursorId == "toggles.overnight"' "the foot's Enter button left the hub"
  "$jq_bin" -e '.active == true' "$p-overnight-enter.json" > /dev/null || fail "launcher-mouse: the foot's Enter button did not run Overnight: $(cat "$p-overnight-enter.json")"
  echo "SMOKE_LAUNCHER_MOUSE a click on the list closed the column onto Overnight, the foot's Enter button ran it"

  launcher_mouse_expect hints '.ids[0] == "toggles" and ([.footHints[].on] | index("actions") and index("into") and index("primary"))' "the Toggles row's foot has no Actions, Into and Enter buttons"
  launcher_mouse_expect hint-actions '.actionsOpen and .cursorId == "toggles"' "the Actions button was not Tab"
  launcher_mouse_expect hint-column-back '(.actionsOpen | not) and .level == null' "the Back button was not Shift+Tab"
  launcher_mouse_expect hint-into '.level == "toggles"' "the Into button was not →"
  echo "SMOKE_LAUNCHER_MOUSE the foot's Actions, Back and Into buttons did Tab, Shift+Tab and →"

  launcher_mouse_expect probe ".ids[0] == \"$launcher_mouse_app\" and .scrollMax > 0" "the Mouse Probe filter did not head a list that scrolls"
  launcher_mouse_expect column ".actionsOpen and .cursorId == \"$launcher_mouse_app\" and .actionsScrollMax > 0 and .actionsBox.width > 0" "the probe's column does not overflow"
  launcher_mouse_expect column-hover '.actionHover >= 0 and .actionCursor == 0' "the pointer over the column did not hover an action alone"
  launcher_mouse_expect column-wheel ".actionsScroll > 0 and .scrollTop == 0 and .actionCursor == 0 and .cursorId == \"$launcher_mouse_app\"" "a wheel notch over the column did not scroll the column alone"
  local wheel finger
  wheel=$("$jq_bin" -r '.actionsScroll' "$p-column-wheel.json")
  finger=$("$jq_bin" -r '.actionsScroll' "$p-column-finger.json")
  launcher_mouse_expect column-finger ".actionsScroll > $wheel and .scrollTop == 0 and .actionCursor == 0" "a touchpad flick over the column did not scroll it on from $wheel alone"
  launcher_mouse_expect list-wheel ".scrollTop > 0 and .actionsScroll == $finger" "a wheel notch over the list did not scroll the list alone"
  echo "SMOKE_LAUNCHER_MOUSE the column scrolled on its own: a notch to $wheel, a flick to $finger, the list's notch left it"

  launcher_mouse_expect outside '.isOpen == false' "a click outside the card did not close it"
  echo "SMOKE_LAUNCHER_MOUSE a click outside closed the card"

  "$jq_bin" -e '.mode == "light"' "$p-theme.json" > /dev/null || fail "launcher-mouse: the light frames were not taken in light mode"
  local shot mode
  for shot in hover column; do
    for mode in dark light; do
      [ -s "$p-$shot-$mode.png" ] || fail "launcher-mouse: no $shot $mode frame"
      echo "SMOKE_LAUNCHER_MOUSE_SHOT $p-$shot-$mode.png"
    done
  done
  echo "SMOKE_LAUNCHER_MOUSE_SHOT $p-column-scrolled.png"
}
