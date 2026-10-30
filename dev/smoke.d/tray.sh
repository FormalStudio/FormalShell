# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154,SC2016  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --tray launches six real StatusNotifierItem producers (dev/sni-stub.py,
# which registers on the session bus for real, never faked inside the shell)
# and reads the whole strip back. It pins `tray.maxVisible: -1`, the strip
# carrying whatever fits, since the shipped default is the second bar
# (--tray-overflow reads that one back). Six items with no visible limit is
# the claim: every one is its own cell, room alone decides, and bounding a
# long strip is the bar chevron's job. `tray status` has to report all six and no drawer, bucket or
# expand key at all; `tray activate` has to reach that one item over D-Bus
# (the stub's own --activate-file is the evidence, not the IPC reply); the
# shell-owned menu has to open, walk two rows and fire the third; and the
# launcher's tray route has to be searchable from inside while staying
# invisible from the root.
# Ridden by --sidebar, which has no strip, the same then runs once more off
# the sidebar's status card: its tray button in `sidebar status`, a real
# click on it opening the second bar, a real click on fixture 1's square
# (found by its colour in the frame) reaching the stub as Activate, and a
# real right click on fixture 2's opening its menu.
leg_tray_flag="--tray"
leg_tray_order=170
leg_tray_needs="python3"

# The stub items are PyGObject.
need_python3() {
  need_tool python3_bin python3 python3
  "$python3_bin" -c 'import gi' >/dev/null 2>&1 \
    || { echo "SMOKE_FAIL: $python3_bin cannot import gi; install the openSUSE package python3-gobject" >&2; exit 1; }
}

tray_status_path="$shot_dir/tray-status.json"
tray_strip_path="$shot_dir/tray-strip.png"
tray_pids_path="$shot_dir/tray-pids.txt"
tray_activate_path="$shot_dir/tray-activate.txt"
tray_activate_reply_path="$shot_dir/tray-activate-reply.txt"
tray_menu_reply_path="$shot_dir/tray-menu-reply.txt"
tray_menu_path="$shot_dir/tray-menu.png"
tray_menuactivate_reply_path="$shot_dir/tray-menuactivate-reply.txt"
tray_query_root_path="$shot_dir/tray-query-root.json"
tray_query_route_path="$shot_dir/tray-query-route.json"
tray_sidebar_status_path="$shot_dir/tray-sidebar-status.json"
tray_sidebar_open_path="$shot_dir/tray-sidebar-open.json"
tray_sidebar_png="$shot_dir/tray-sidebar-open.png"
tray_sidebar_menu_png="$shot_dir/tray-sidebar-menu.png"
tray_sidebar_menu_reply_path="$shot_dir/tray-sidebar-menu-reply.txt"
tray_activate_before_path="$shot_dir/tray-activate-before-sidebar.txt"

leg_tray_fixture() {
  settings_fragment ', "tray": {"maxVisible": -1}'
}

leg_tray_timing() {
  # tray-drive.sh's own last step lands around its internal sleep sum (~19s)
  # plus a second apiece for eleven `ipc call` spawns on llvmpipe. The run's own
  # frame is taken past that, so it shows the ordinary bar with all six items
  # as their own cells and the route closed again. Getting this wrong is not a
  # slow run but a false failure: the kill script takes the stubs down right
  # after that frame, so a drive step landing later reads an empty item list.
  if leg_on sidebar; then leg_timing 40 64; else leg_timing 26 50; fi
}

leg_tray_drive() {
  local script="$shot_dir/tray-drive.sh"
  local kill_script="$shot_dir/tray-kill.sh"
  local stub="$PWD/dev/sni-stub.py"
  local route_steps
  route_steps=$(cat <<STEPS
$ipc call debug query "Tray Fixture 2" > "$tray_query_root_path" 2>&1
sleep 1
$ipc call menu summon tray > /dev/null 2>&1
sleep 2
$ipc call debug query "Tray Fixture 2" > "$tray_query_route_path" 2>&1
sleep 1
$ipc call menu close > /dev/null 2>&1
STEPS
)
  local sidebar_steps=""
  if leg_on sidebar; then
    sidebar_steps=$(cat <<STEPS
cp "$tray_activate_path" "$tray_activate_before_path"
$ipc call sidebar status > "$tray_sidebar_status_path" 2>&1
origin=\$("$hyprctl_bin" -j layers | "$jq_bin" -r '[.[] | .levels[] | .[] | select(.namespace == "formalshell:bar")] | first | "\(.x) \(.y)"')
read -r ox oy <<< "\$origin"
read -r tx ty <<< "\$("$jq_bin" -r --argjson ox "\$ox" --argjson oy "\$oy" '.hits[] | select(.on == "panel:trayoverflow") | "\(\$ox + .x + (.w / 2 | floor)) \(\$oy + .y + (.h / 2 | floor))"' "$tray_sidebar_status_path" | head -1)"
to() { "$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1; "$wlrctl_bin" pointer move "\$1" "\$2" > /dev/null 2>&1; sleep 0.4; }
to "\$tx" "\$ty"
"$wlrctl_bin" pointer click left > /dev/null 2>&1
sleep 2
$ipc call tray status > "$tray_sidebar_open_path" 2>&1
"$grim_bin" "$tray_sidebar_png" > /dev/null 2>&1
# A fixture's square: the centre of the pixels carrying its colour.
square() { "$python3_bin" -c 'import sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGB")
want = tuple(int(sys.argv[2][i:i + 2], 16) for i in (0, 2, 4))
px = im.load()
hit = [(x, y) for y in range(0, im.height, 2) for x in range(0, im.width, 2) if all(abs(a - b) < 8 for a, b in zip(px[x, y], want))]
print(sum(p[0] for p in hit) // len(hit), sum(p[1] for p in hit) // len(hit)) if hit else print("0 0")' "\$@"; }
read -r ax ay <<< "\$(square "$tray_sidebar_png" c0392b)"
to "\$ax" "\$ay"
"$wlrctl_bin" pointer click left > /dev/null 2>&1
sleep 2
to "\$tx" "\$ty"
"$wlrctl_bin" pointer click left > /dev/null 2>&1
sleep 2
"$grim_bin" "$tray_sidebar_png.again.png" > /dev/null 2>&1
read -r mx my <<< "\$(square "$tray_sidebar_png.again.png" 27ae60)"
to "\$mx" "\$my"
"$wlrctl_bin" pointer click right > /dev/null 2>&1
sleep 2
"$grim_bin" "$tray_sidebar_menu_png" > /dev/null 2>&1
$ipc call tray menucursor 1 > "$tray_sidebar_menu_reply_path" 2>&1
$ipc call panel close > /dev/null 2>&1
STEPS
)
  fi
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 1
"$python3_bin" "$stub" --id tray-fixture-1 --title "Tray Fixture 1" --color c0392b --activate-file "$tray_activate_path" & echo \$! >> "$tray_pids_path"
"$python3_bin" "$stub" --id tray-fixture-2 --title "Tray Fixture 2" --color 27ae60 --activate-file "$tray_activate_path" --menu & echo \$! >> "$tray_pids_path"
"$python3_bin" "$stub" --id tray-fixture-3 --title "Tray Fixture 3" --color 2980b9 --activate-file "$tray_activate_path" & echo \$! >> "$tray_pids_path"
"$python3_bin" "$stub" --id tray-fixture-4 --title "Tray Fixture 4" --color f1c40f --activate-file "$tray_activate_path" & echo \$! >> "$tray_pids_path"
"$python3_bin" "$stub" --id tray-fixture-5 --title "Tray Fixture 5" --color 8e44ad --activate-file "$tray_activate_path" & echo \$! >> "$tray_pids_path"
"$python3_bin" "$stub" --id tray-fixture-6 --title "Tray Fixture 6" --color 16a085 --activate-file "$tray_activate_path" & echo \$! >> "$tray_pids_path"
sleep 6
$ipc call tray status > "$tray_status_path" 2>&1
sleep 1
"$grim_bin" "$tray_strip_path" > /dev/null 2>&1
sleep 1
$ipc call tray activate tray-fixture-2 > "$tray_activate_reply_path" 2>&1
sleep 1
$ipc call tray menu tray-fixture-2 > "$tray_menu_reply_path" 2>&1
sleep 3
"$grim_bin" "$tray_menu_path" > /dev/null 2>&1
sleep 1
$ipc call tray menucursor 1 > /dev/null 2>&1
$ipc call tray menucursor 1 > /dev/null 2>&1
$ipc call tray menuactivate > "$tray_menuactivate_reply_path" 2>&1
sleep 1
$route_steps
$sidebar_steps
EOF

  # The stubs sit in GLib.MainLoop().run() forever, so they are killed by PID
  # right after the run's own frame rather than left to outlive the session.
  write_script "$kill_script" <<EOF
#!/usr/bin/env bash
if [ -f "$tray_pids_path" ]; then
  while read -r pid; do
    kill "\$pid" 2>/dev/null || true
  done < "$tray_pids_path"
fi
EOF
  add_cleanup "bash $kill_script"
  hypr_exec_once "bash $script"
}

leg_tray_assert() {
  local tray_count tray_activate_lines
  if [ ! -s "$tray_status_path" ]; then
    fail "no tray status produced"
  fi
  cat "$tray_status_path"; echo
  tray_count=$(grep -o '"id":' "$tray_status_path" | wc -l | tr -d ' ')
  if [ "$tray_count" != "6" ]; then
    fail "tray status did not report all 6 fixture items (got $tray_count), stub registration likely failed"
  fi
  # No drawer since M24, so nothing here may report one. A drawer/expanded
  # key coming back would mean a stale build, not a passing run.
  if grep -qE '"(drawer|expanded|pinned|bucket)":' "$tray_status_path"; then
    fail "tray status still reports drawer/bucket state, got: $(cat "$tray_status_path")"
  fi
  if [ ! -f "$tray_strip_path" ]; then
    fail "no tray-strip screenshot produced"
  fi
  echo "SMOKE_TRAY_STRIP $tray_strip_path"
  if ! grep -q '^ok$' "$tray_activate_reply_path" 2>/dev/null; then
    fail "tray activate IPC call did not return ok, got: $(cat "$tray_activate_reply_path" 2>/dev/null)"
  fi
  if [ ! -s "$tray_activate_path" ]; then
    fail "no tray-activate.txt produced, the shell's activate() never reached the stub over D-Bus"
  fi
  cat "$tray_activate_path"
  if ! grep -q '^tray-fixture-2: Activate(' "$tray_activate_path"; then
    fail "tray-activate.txt does not record Activate on tray-fixture-2, got: $(cat "$tray_activate_path")"
  fi
  # `tray menu` opens the shell-owned tray menu surface, so there is no
  # platform QMenu grab to wedge a headless run.
  if ! grep -q '^ok$' "$tray_menu_reply_path" 2>/dev/null; then
    fail "tray menu IPC call did not return ok, got: $(cat "$tray_menu_reply_path" 2>/dev/null)"
  fi
  if [ ! -f "$tray_menu_path" ]; then
    fail "no tray-menu screenshot produced"
  fi
  echo "SMOKE_TRAY_MENU $tray_menu_path"
  if ! grep -q '^ok$' "$tray_menuactivate_reply_path" 2>/dev/null; then
    fail "tray menuactivate IPC call did not return ok, got: $(cat "$tray_menuactivate_reply_path" 2>/dev/null)"
  fi
  # `menucursor 1` twice walks Plain Item -> Disabled Item -> Checked Item
  # (rows 0/1/2 of sni-stub.py's own tree) and `menuactivate` fires entry id
  # 3, which the stub appends to the same --activate-file. Refing the root
  # around open/close sends "opened" and "closed" either side of it, so the
  # file ends up with exactly four lines: asserting the clicked line plus the
  # count catches both a missing round trip and an over-firing one.
  if ! grep -q '^tray-fixture-2 menu: Event(3, clicked)$' "$tray_activate_path"; then
    fail "tray-activate.txt does not record the menu Event round trip on entry 3, got: $(cat "$tray_activate_path")"
  fi
  local counted="$tray_activate_path"
  leg_on sidebar && counted="$tray_activate_before_path"
  tray_activate_lines=$(wc -l < "$counted" | tr -d ' ')
  if [ "$tray_activate_lines" != "4" ]; then
    fail "expected exactly Activate + opened + clicked + closed (got $tray_activate_lines lines), got: $(cat "$tray_activate_path")"
  fi
  # routeOnly: the tray names its rows after the applications they belong to,
  # so the same app used to come back two and three times in one root search.
  # The query runs twice against the same shell, once from the root and once
  # standing inside the route, because "invisible" on its own is equally
  # consistent with a row that stopped existing.
  if [ ! -s "$tray_query_root_path" ]; then
    fail "no root debug query produced"
  fi
  cat "$tray_query_root_path"; echo
  if grep -q '"tray\.' "$tray_query_root_path"; then
    fail "a root query still reaches tray rows, got: $(cat "$tray_query_root_path")"
  fi
  if [ ! -s "$tray_query_route_path" ]; then
    fail "no in-route debug query produced"
  fi
  cat "$tray_query_route_path"; echo
  if ! grep -q '"id":"tray\.tray-fixture-2"' "$tray_query_route_path"; then
    fail "the tray route cannot search its own rows, got: $(cat "$tray_query_route_path")"
  fi
  leg_on sidebar || return 0
  "$jq_bin" -e '[.hits[] | select(.on == "panel:trayoverflow")] | length == 1' "$tray_sidebar_status_path" > /dev/null 2>&1 \
    || fail "the sidebar's status card carries no tray button with items registered: $(cat "$tray_sidebar_status_path" 2>/dev/null)"
  [ "$("$jq_bin" -r '.overflow.open' "$tray_sidebar_open_path" 2>/dev/null)" = "true" ] \
    || fail "a real click on the sidebar's tray button did not open the second bar: $(cat "$tray_sidebar_open_path" 2>/dev/null)"
  echo "SMOKE_TRAY_SIDEBAR $tray_sidebar_png"
  echo "SMOKE_TRAY_SIDEBAR_MENU $tray_sidebar_menu_png"
  grep -q '^tray-fixture-1: Activate(' "$tray_activate_path" \
    || fail "a real click on fixture 1 in the sidebar's second bar did not reach it as Activate: $(cat "$tray_activate_path")"
  grep -q '^ok$' "$tray_sidebar_menu_reply_path" 2>/dev/null \
    || fail "a real right click on fixture 2 in the sidebar's second bar opened no menu: $(cat "$tray_sidebar_menu_reply_path" 2>/dev/null)"
}
