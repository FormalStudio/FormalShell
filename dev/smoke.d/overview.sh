# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --overview: HyprExpo, the workspace overview the owner's spec builds as
# the fspec package `hyprexpo` (FS_HYPREXPO points at another build), loaded
# the way their hyprland.lua loads it: only while hl.version() is the
# Hyprland its hyprland-version file names, since a plugin off other
# headers takes the compositor down. Only this leg loads it: every
# other run keeps the minimap's cross-row drags its own. Under `layout:
# sidebar`, foot windows on workspaces 1 and 2: `hyprctl plugin list`
# naming hyprexpo, its `columns` reaching it from hl.config, and the
# `debug dump`'s `overview` true. Then the
# overview opened four ways, each read back off `hyprctl submap` (the
# plugin enters its `hyprexpo` submap while it is up) and closed over IPC:
# Super+O typed by wtype, `workspaces overview open`, a 900ms press on a
# minimap icon, and an icon dragged off its row onto the next. The bind's
# and the IPC's frames are pulled to read the grid by eye, each differing
# from the desktop frame before it; the drag's release lands on the
# overview, which takes the pointer while it is up and closes on it.
leg_overview_flag="--overview"
leg_overview_order=239
# Not a budget: Hyprland's function hooks decode x86_64 alone (udis86), so
# on the mac rig's aarch64 HyprExpo throws "Failed initializing hooks" in
# init and Hyprland unloads it. This is the field that keeps a leg off it.
leg_overview_budget="HyprExpo's hooks need x86_64"
leg_overview_needs="jq foot wtype wlrctl python3 convert"
overview_dir="${FS_HYPREXPO:-/opt/fspec/current/pkgs/hyprexpo}"
overview_vpointer="$PWD/dev/vpointer.py"
overview_plugins="$shot_dir/overview-plugins.json"
overview_columns="$shot_dir/overview-columns.json"
overview_dump="$shot_dir/overview-dump.json"
overview_desktop_png="$shot_dir/overview-desktop.png"
overview_bind_png="$shot_dir/overview-bind.png"
overview_ipc_png="$shot_dir/overview-ipc.png"
overview_submaps="$shot_dir/overview-submaps.txt"
overview_guard="$shot_dir/overview-guard.txt"
overview_hyprlog="$shot_dir/overview-hyprland-plugin.log"

leg_overview_validate() {
  if leg_on bar_position || leg_on frame || leg_on minimap; then
    echo "usage: --overview runs under layout sidebar and cannot combine with --bar-position, --frame or --minimap" >&2
    exit 1
  fi
}

leg_overview_fixture() {
  [ -s "$overview_dir/hyprexpo.so" ] && [ -s "$overview_dir/hyprland-version" ] \
    || fail "overview: no hyprexpo build at $overview_dir: apply the spec's hyprexpo package, or set FS_HYPREXPO"
  leg_on sidebar || settings_fragment ', "layout": "sidebar"'
}

leg_overview_timing() {
  leg_timing 30 70
}

leg_overview_drive() {
  local script="$shot_dir/overview-drive.sh"
  cat <<EOF
do
  local f = io.open([==[$overview_dir/hyprland-version]==])
  local built = f and f:read("l")
  if f then f:close() end
  local seen = io.open([==[$overview_guard]==], "w")
  if seen then seen:write(tostring(built), " ", hl.version(), "\\n") seen:close() end
  if built == hl.version() then hl.plugin.load([==[$overview_dir/hyprexpo.so]==]) end
end
if hl.plugin.hyprexpo then
  hl.config({ plugin = { hyprexpo = { columns = 3, gaps_in = 8, gaps_out = 24, keynav_enable = 1, skip_empty = 0 } } })
end
hl.bind("SUPER + O", function() if hl.plugin.hyprexpo then hl.plugin.hyprexpo.expo("toggle") end end)
EOF
  write_script "$script" <<EOS
#!/usr/bin/env bash
for _ in \$(seq 1 40); do
  $ipc call sidebar status 2>/dev/null | grep -q '"cards"' && break
  sleep 0.5
done
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-ov-a sh -c 'sleep 300']==])" > /dev/null 2>&1
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-ov-b sh -c 'sleep 300']==], { workspace = '2 silent' })" > /dev/null 2>&1
sleep 3
"$hyprctl_bin" -j plugin list > "$overview_plugins" 2>&1
grep -i -E 'plugin|hyprexpo|\\[he\\]' "$hypr_log_path" > "$overview_hyprlog" 2>&1
"$hyprctl_bin" -j getoption plugin:hyprexpo:columns > "$overview_columns" 2>&1
$ipc call debug dump > "$overview_dump" 2>&1
submap() { printf '%s %s\n' "\$1" "\$("$hyprctl_bin" submap 2>&1 | tr -d '\n')" >> "$overview_submaps"; }
"$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1
"$wlrctl_bin" pointer move 900 500 > /dev/null 2>&1
sleep 0.5
"$grim_bin" "$overview_desktop_png" > /dev/null 2>&1
submap rest
"$wtype_bin" -M logo -k o -m logo
sleep 1.5
submap bind-open
"$grim_bin" "$overview_bind_png" > /dev/null 2>&1
$ipc call workspaces overview close > /dev/null 2>&1
sleep 1
submap bind-closed
$ipc call workspaces overview open > /dev/null 2>&1
sleep 1.5
submap ipc-open
"$grim_bin" "$overview_ipc_png" > /dev/null 2>&1
$ipc call workspaces overview close > /dev/null 2>&1
sleep 1
submap ipc-closed
centre() {
  $ipc call sidebar status 2>/dev/null | "$jq_bin" -r --arg on "\$1" '.hits[] | select(.on == \$on) | "\(.x + (.w / 2 | floor)) \(.y + (.h / 2 | floor))"' | head -1
}
first=\$($ipc call sidebar status | "$jq_bin" -r '.rows[0].windows[0]')
read -r ax ay <<< "\$(centre "window:\$first")"
"$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1
"$wlrctl_bin" pointer move "\$ax" "\$ay" > /dev/null 2>&1
sleep 0.3
"$python3_bin" "$overview_vpointer" down wait 1500 up > /dev/null 2>&1 &
sleep 1.1
submap press-open
wait
sleep 1
submap press-released
$ipc call workspaces overview close > /dev/null 2>&1
"$hyprctl_bin" dispatch "hl.dsp.focus({ workspace = 1 })" > /dev/null 2>&1
sleep 1
first=\$($ipc call sidebar status | "$jq_bin" -r '.rows[0].windows[0]')
read -r ax ay <<< "\$(centre "window:\$first")"
read -r bx by <<< "\$(centre row:1)"
"$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1
"$wlrctl_bin" pointer move "\$ax" "\$ay" > /dev/null 2>&1
sleep 0.3
dy=\$(( (by - ay) / 8 ))
steps=()
for _ in 1 2 3 4 5 6 7 8; do steps+=(move 0 "\$dy" wait 30); done
"$python3_bin" "$overview_vpointer" down wait 50 "\${steps[@]}" wait 1500 up > /dev/null 2>&1 &
sleep 1.4
submap drag-open
wait
sleep 1
submap drag-released
$ipc call workspaces overview close > /dev/null 2>&1
EOS
  hypr_exec_once "bash $script"
}

overview_differs() {
  "$convert_bin" "$1" "$2" -compose difference -composite -colorspace Gray -format '%[fx:mean]' info: 2>/dev/null
}

leg_overview_assert() {
  local f
  for f in "$overview_plugins" "$overview_columns" "$overview_dump" "$overview_submaps" "$overview_desktop_png" "$overview_bind_png" "$overview_ipc_png"; do
    [ -s "$f" ] || fail "overview: $f missing or empty"
  done
  echo "SMOKE_OVERVIEW_BIND $overview_bind_png"
  echo "SMOKE_OVERVIEW_IPC $overview_ipc_png"
  cat "$overview_submaps"
  cat "$overview_hyprlog" 2>/dev/null
  "$jq_bin" -e 'any(.[]; .name == "hyprexpo")' "$overview_plugins" > /dev/null \
    || fail "overview: hyprexpo is not loaded (plugin list: $(cat "$overview_plugins"); the guard read built and running as: $(cat "$overview_guard" 2>/dev/null))"
  "$jq_bin" -e '.int == 3' "$overview_columns" > /dev/null \
    || fail "overview: plugin:hyprexpo:columns did not take hl.config's 3: $(cat "$overview_columns")"
  "$jq_bin" -e '.overview == true' "$overview_dump" > /dev/null \
    || fail "overview: debug dump's overview is not true: $("$jq_bin" -c '.overview' "$overview_dump")"
  local step
  for step in rest bind-closed ipc-closed press-released drag-released; do
    grep -q "^$step " "$overview_submaps" && ! grep -q "^$step .*hyprexpo" "$overview_submaps" \
      || fail "overview: the overview was up at $step"
  done
  for step in bind-open ipc-open press-open drag-open; do
    grep -q "^$step .*hyprexpo" "$overview_submaps" || fail "overview: the overview was not up at $step"
  done
  local d
  for f in "$overview_bind_png" "$overview_ipc_png"; do
    d=$(overview_differs "$overview_desktop_png" "$f")
    echo "overview: $(basename "$f") differs from the desktop by $d"
    awk -v d="$d" 'BEGIN { exit !(d > 0.02) }' || fail "overview: $(basename "$f") is the desktop, no grid drawn (mean difference $d)"
  done
}
