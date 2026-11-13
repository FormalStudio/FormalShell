# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154,SC2016  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --mac-electron: a Mac Electron app in atoll's Mac mode, used through the
# shell's tray the way any app's menu bar extra is. The app is atoll's
# fixtures/electron-panel (FS_ELECTRON_APP), put in the session's home by
# `atoll install` (FS_ATOLL), so it runs on the Linux Electron with
# process.platform darwin. Its tray item's click shows a frameless `panel`
# window that hides on blur, shown inactive and then made key, the usual
# quick-entry panel; every focus, blur and keystroke is a line in the app's
# log, which is the evidence, with WAYLAND_DEBUG on for the activation and
# keyboard traffic. The session honours xdg-activation as the owner's does
# (misc.focus_on_activate). A real click on the item in the second bar has
# to leave the panel in front with the keyboard: typed text has to reach
# its field, with no blur in between. The item has to export the menu the
# app pops up from its right-click handler. Not in dev/smoke-all.list: it
# needs an atoll build and the fixture in the slot, as --mac-app does.
leg_mac_electron_flag="--mac-electron"
leg_mac_electron_order=175
leg_mac_electron_needs="jq wlrctl wtype python3"
leg_mac_electron_claims="focus screen=shared"
leg_mac_electron_env="FS_ATOLL FS_ELECTRON_APP"

mac_electron_prefix="$shot_dir/mac-electron"
mac_electron_atoll="${FS_ATOLL:-$HOME/atoll/target/debug/atoll}"
mac_electron_bundle="${FS_ELECTRON_APP:-$HOME/atoll/fixtures/out/electron-panel/ElectronPanel.app}"
mac_electron_id="com.atoll.electron-panel"
mac_electron_data="$iso_home/.local/share"
mac_electron_log="$shot_dir/mac-electron-app.log"
mac_electron_binary="$shot_dir/mac-electron-binary.txt"

leg_mac_electron_fixture() {
  local stop='echo "SMOKE_FAIL: --mac-electron: $why" >&2; exit 1' why entry
  why="no atoll at $mac_electron_atoll (FS_ATOLL)"; [ -x "$mac_electron_atoll" ] || eval "$stop"
  why="no bundle at $mac_electron_bundle (FS_ELECTRON_APP)"; [ -d "$mac_electron_bundle" ] || eval "$stop"
  mkdir -p "$mac_electron_data" "$iso_home/.config"
  # The Linux Electron download stays in the slot's own cache, shared by every run.
  HOME="$iso_home" XDG_DATA_HOME="$mac_electron_data" XDG_CONFIG_HOME="$iso_home/.config" XDG_CACHE_HOME="$HOME/.cache" \
    "$mac_electron_atoll" install "$mac_electron_bundle" > "$shot_dir/mac-electron-install.log" 2>&1
  entry="$mac_electron_data/applications/$mac_electron_id.desktop"
  why="atoll install wrote no $entry: $(tail -n 3 "$shot_dir/mac-electron-install.log")"; [ -f "$entry" ] || eval "$stop"
  sed -n 's/^Exec="\([^"]*\)".*/\1/p' "$entry" | head -n 1 > "$mac_electron_binary"
  why="the desktop entry names no binary"; [ -s "$mac_electron_binary" ] || eval "$stop"
}

leg_mac_electron_timing() {
  leg_timing 150
}

leg_mac_electron_drive() {
  echo 'hl.config({ misc = { focus_on_activate = true } })'
  local script="$shot_dir/mac-electron-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
m="$mac_electron_prefix"
log="$mac_electron_log"
bin=\$(cat "$mac_electron_binary")
# logged <fixed text>: how many of the app's lines carry it.
logged() { grep -cF "\$1" "\$log" 2>/dev/null || true; }
# square <frame> <rrggbb>: the centre of the pixels of that colour, in the
# pointer's logical pixels.
square() { "$python3_bin" -c 'import sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGB")
want = tuple(int(sys.argv[2][i:i + 2], 16) for i in (0, 2, 4))
s = float(sys.argv[3])
px = im.load()
hit = [(x, y) for y in range(im.height) for x in range(im.width) if all(abs(a - b) < 6 for a, b in zip(px[x, y], want))]
print(round(sum(p[0] for p in hit) / len(hit) / s), round(sum(p[1] for p in hit) / len(hit) / s)) if hit else print("0 0")' "\$@" "${FS_SMOKE_SCALE:-1}"; }

"$wmctl_bin" dispatch "hl.dsp.exec_cmd([==[sh -c 'WAYLAND_DEBUG=1 exec \$bin --ozone-platform=wayland > \$log 2>&1']==])" > /dev/null 2>&1
for _ in \$(seq 1 120); do [ "\$(logged ' ready')" -ge 1 ] && break; sleep 0.5; done
until_jq 30 "\$m-tray.json" '.items | length > 0' call tray status
# A keyboard for the whole walk, as a seat with a real one has: focus
# reaches a window when it is activated, not when the next wtype starts.
"$wtype_bin" -s 140000 -k Shift_L &
echo \$! > "\$m-keyboard.pid"
sleep 0.5
call bar room > "\$m-room.json" 2>&1
strip() {
  park \$("$jq_bin" -r '.[0].cells[] | select(.name == "tray") | "\(.x + .width / 2 | floor) \(.y + .height / 2 | floor)"' "\$m-room.json" | head -n 1)
  click
  sleep 1.5
  until_settled
}

# The baseline, in the order the shell used: the overflow strip open, the
# item's Activate over the bus with no token, then the strip closed.
strip
item=\$(busctl --user --json=short get-property org.kde.StatusNotifierWatcher /StatusNotifierWatcher org.kde.StatusNotifierWatcher RegisteredStatusNotifierItems | "$jq_bin" -r '.data[0]')
echo "\$item" > "\$m-sni.txt"
busctl --user call "\${item%%/*}" "/\${item#*/}" org.kde.StatusNotifierItem Activate ii 0 0 >> "\$m-sni.txt" 2>&1
call panel close > /dev/null 2>&1
sleep 3
"$wmctl_bin" -j activewindow > "\$m-raw-active.json" 2>&1
shoot "\$m-raw.png"
"$wtype_bin" 'raw'
sleep 1.5
wc -l < "\$log" > "\$m-mark.txt"

strip
shoot "\$m-strip.png"
read -r ix iy <<< "\$(square "\$m-strip.png" e84393)"
echo "\$ix \$iy" > "\$m-item.txt"
park "\$ix" "\$iy"
click
sleep 3
"$wmctl_bin" -j activewindow > "\$m-active.json" 2>&1
"$wmctl_bin" -j clients > "\$m-clients.json" 2>&1
call tray status > "\$m-tray-after.json" 2>&1
shoot "\$m-panel.png"
"$wtype_bin" 'hello panel'
sleep 1.5
shoot "\$m-typed.png"

# The item's own menu: a right click, as on the Mac.
"$wtype_bin" -k Escape
sleep 1
strip
park "\$ix" "\$iy"
click right
sleep 2
until_settled
shoot "\$m-right.png"
menu=\$(busctl --user get-property "\${item%%/*}" "/\${item#*/}" org.kde.StatusNotifierItem Menu | cut -d '"' -f 2)
busctl --user call "\${item%%/*}" "\$menu" com.canonical.dbusmenu GetLayout -- iias 0 -1 0 > "\$m-layout.txt" 2>&1
call debug dump > "\$m-right-dump.json" 2>&1
call panel close > /dev/null 2>&1
kill "\$(cat "\$m-keyboard.pid")" 2>/dev/null
EOF
  write_script "$shot_dir/mac-electron-kill.sh" <<EOF
#!/usr/bin/env bash
pkill -f "\$(cat "$mac_electron_binary")" 2>/dev/null || true
kill "\$(cat "$mac_electron_prefix-keyboard.pid" 2>/dev/null)" 2>/dev/null || true
EOF
  add_cleanup "bash $shot_dir/mac-electron-kill.sh"
  wm_exec_once "bash $script"
}

leg_mac_electron_assert() {
  local m="$mac_electron_prefix"
  local shot
  local mark
  mark=$(cat "$m-mark.txt" 2>/dev/null || echo 0)
  for shot in raw strip panel typed right; do
    [ ! -f "$m-$shot.png" ] || echo "SMOKE_MAC_ELECTRON_$(echo "$shot" | tr 'a-z-' 'A-Z_') $m-$shot.png"
  done
  echo "SMOKE_MAC_ELECTRON_LOG $mac_electron_log"
  echo "== bare Activate, no token ($(tr '\n' ' ' < "$m-sni.txt" 2>/dev/null))"
  head -n "$mark" "$mac_electron_log" 2>/dev/null | grep -E '^[0-9.]+ |xdg_activation|wl_keyboard#[0-9]+\.(enter|leave)' | cut -c1-200 | tail -n 30
  "$jq_bin" -c '{class, title}' "$m-raw-active.json" 2>/dev/null
  echo "== the shell's click"
  tail -n +"$((mark + 1))" "$mac_electron_log" 2>/dev/null | grep -E '^[0-9.]+ |xdg_activation|wl_keyboard#[0-9]+\.(enter|leave)' | cut -c1-200 | head -n 40
  "$jq_bin" -c '{class, title, floating}' "$m-active.json" 2>/dev/null
  "$jq_bin" -c '.[] | select(.class == "electronpanel") | {title, tags, at, size, floating}' "$m-clients.json" 2>/dev/null
  echo "== the item's dbusmenu: $(cut -c1-400 "$m-layout.txt" 2>/dev/null)"
  { grep -E '^tray: ' "$shell_log_path" 2>/dev/null || true; } | tail -n 10
  [ "$(cat "$m-item.txt" 2>/dev/null)" != "0 0" ] || fail "--mac-electron: no tray item of the fixture's colour in the second bar"
  grep -qF 'tray click' "$mac_electron_log" || fail "--mac-electron: a click on the tray item did not reach the app as its Tray click"
  grep -qF '"Quit ElectronPanel"' "$m-layout.txt" 2>/dev/null \
    || fail "--mac-electron: the item exports no menu where the app pops one up on right-click ($(cut -c1-200 "$m-layout.txt" 2>/dev/null))"
  # wtype's own keyboard going away at its exit is a blur after the text,
  # so only one before it counts.
  local walk
  # From the click on: opening the strip first blurs the baseline's panel.
  walk=$(tail -n +"$((mark + 1))" "$mac_electron_log" | grep -E '^[0-9.]+ ' | sed -n '/ tray click$/,/hello panel"/p')
  echo "$walk" | grep -q 'hello panel"' \
    || fail "--mac-electron: text typed after the click did not reach the panel ($(echo "$walk" | tr '\n' ';'))"
  ! echo "$walk" | grep -qF 'panel blur' || fail "--mac-electron: the panel lost focus before the typing was done ($(echo "$walk" | tr '\n' ';'))"
}
