# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154,SC2016  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --mac-fullscreen: a Mac window in and out of full screen, as on a Mac.
# Two apps `atoll install` puts in the session's home: an Electron app in
# atoll's Mac mode (FS_ELECTRON_APP, atoll's fixtures/electron-panel with
# ELECTRON_PANEL_WINDOW, an inset title bar window, by default; any other
# Electron bundle such as Notion works too) and a native AppKit one
# (FS_FULLSCREEN_APP, atoll's fixtures/fullscreen-hello, the template's View
# > Enter Full Screen; `none` leaves it out). The session binds SUPER + Q to
# closing the window and SUPER + SHIFT + F to full screen, as the owner's
# does. For each app, by real input: the green button takes it into full
# screen; the pointer at the top edge brings the bar and the window's
# buttons down, and the green button takes it out; SUPER + CTRL + F in and
# out; the bar's View menu in, and out after the top edge brought the bar
# back; then SUPER + Q. Hyprland's full screen state after each step is the
# evidence, with the frames, and for the fixtures each change is also a line
# in the app's log. The green button is found in a frame of the window's
# top left corner, where an app puts it.
# Not in dev/smoke-all.list: it needs an atoll build and the fixtures in the
# slot, as --mac-app does.
leg_mac_fullscreen_flag="--mac-fullscreen"
leg_mac_fullscreen_order=176
leg_mac_fullscreen_needs="jq wlrctl wtype pil"
leg_mac_fullscreen_claims="screen=shared hyprland.binds"
leg_mac_fullscreen_env="FS_ATOLL FS_ELECTRON_APP FS_FULLSCREEN_APP"

mac_fs_prefix="$shot_dir/mac-fs"
mac_fs_atoll="${FS_ATOLL:-$HOME/atoll/target/debug/atoll}"
mac_fs_fixture="$HOME/atoll/fixtures/out/electron-panel/ElectronPanel.app"
mac_fs_electron="${FS_ELECTRON_APP:-$mac_fs_fixture}"
# atoll's fixtures keep the arm64 slice in a folder of its own.
case "$(uname -m)" in aarch64) mac_fs_slice=/arm64 ;; *) mac_fs_slice= ;; esac
mac_fs_native="${FS_FULLSCREEN_APP:-$HOME/atoll/fixtures/out/fullscreen-hello$mac_fs_slice/FullScreenHello.app}"
mac_fs_native_id="dev.atoll.fixture.fullscreen-hello"
mac_fs_data="$iso_home/.local/share"
# The fixtures log each full screen change; another app is judged by
# Hyprland's state alone.
mac_fs_logged=false
[ "$mac_fs_electron" = "$mac_fs_fixture" ] && mac_fs_logged=true

leg_mac_fullscreen_fixture() {
  local stop='echo "SMOKE_FAIL: --mac-fullscreen: $why" >&2; exit 1' why b n
  why="no atoll at $mac_fs_atoll (FS_ATOLL)"; [ -x "$mac_fs_atoll" ] || eval "$stop"
  mkdir -p "$mac_fs_data" "$iso_home/.config"
  for n in electron native; do
    b=$mac_fs_electron
    if [ "$n" = native ]; then
      [ "$mac_fs_native" = none ] && continue
      b=$mac_fs_native
    fi
    why="no bundle at $b"; [ -d "$b" ] || eval "$stop"
    # The Linux Electron download stays in the slot's own cache, shared by every run.
    HOME="$iso_home" XDG_DATA_HOME="$mac_fs_data" XDG_CONFIG_HOME="$iso_home/.config" XDG_CACHE_HOME="$HOME/.cache" \
      "$mac_fs_atoll" install --json "$b" > "$mac_fs_prefix-$n-install.json" 2>> "$shot_dir/mac-fs-install.log"
    why="atoll install wrote no entry for $b: $(tail -n 3 "$shot_dir/mac-fs-install.log")"
    "$jq_bin" -r .desktop "$mac_fs_prefix-$n-install.json" > "$mac_fs_prefix-$n-desktop.txt" 2>/dev/null
    [ -f "$(cat "$mac_fs_prefix-$n-desktop.txt")" ] || eval "$stop"
  done
  sed -n 's/^Exec="\([^"]*\)".*/\1/p' "$(cat "$mac_fs_prefix-electron-desktop.txt")" | head -n 1 > "$mac_fs_prefix-electron-bin.txt"
  if [ "$mac_fs_native" != none ]; then
    sed -n 's/^Exec=\(.*\)$/\1/p' "$(cat "$mac_fs_prefix-native-desktop.txt")" | head -n 1 | sed 's/ %[fFuU]//g' > "$mac_fs_prefix-native-exec.txt"
  fi
}

leg_mac_fullscreen_timing() {
  leg_timing 300
}

leg_mac_fullscreen_drive() {
  awk '/^-- MAC APPS/ { on = 1 } /^-- Window rounding/ { on = 0 } on { print }' docs/examples/hyprland/formalshell.lua
  cat <<'EOF'
hl.config({ input = { resolve_binds_by_sym = true } })
hl.bind("SUPER + Q", hl.dsp.window.close())
hl.bind("SUPER + SHIFT + F", hl.dsp.window.fullscreen({ action = "toggle", mode = "fullscreen" }))
EOF
  write_script "$shot_dir/mac-fs-green.py" <<'EOF'
# green.py <frame> <scale> <x0> <y0> <x1> <y1>: the middle of the third of
# the window's three round buttons in that box of the output, in points.
# They are told by shape, since an inactive window draws them grey.
import sys
from collections import Counter
from PIL import Image

img = Image.open(sys.argv[1]).convert("RGB")
scale = float(sys.argv[2])
x0, y0, x1, y1 = (int(float(v) * scale) for v in sys.argv[3:7])
x0, y0, x1, y1 = max(x0, 0), max(y0, 0), min(x1, img.width), min(y1, img.height)
px = img.load()
ground = Counter(px[x, y] for y in range(y0, y1) for x in range(x0, x1)).most_common(1)[0][0]
far = lambda c: sum(abs(a - b) for a, b in zip(c, ground)) > 36
seen = set()
blobs = []
for y in range(y0, y1):
    for x in range(x0, x1):
        if (x, y) in seen or not far(px[x, y]):
            continue
        stack, pts = [(x, y)], []
        seen.add((x, y))
        while stack:
            cx, cy = stack.pop()
            pts.append((cx, cy))
            for nx, ny in ((cx + 1, cy), (cx - 1, cy), (cx, cy + 1), (cx, cy - 1)):
                if x0 <= nx < x1 and y0 <= ny < y1 and (nx, ny) not in seen and far(px[nx, ny]):
                    seen.add((nx, ny))
                    stack.append((nx, ny))
        xs = [p[0] for p in pts]
        ys = [p[1] for p in pts]
        w, h = max(xs) - min(xs) + 1, max(ys) - min(ys) + 1
        if 9 * scale <= w <= 18 * scale and abs(w - h) <= 2 * scale:
            blobs.append(((min(xs) + max(xs)) / 2, (min(ys) + max(ys)) / 2, w))
blobs.sort()
for i in range(len(blobs) - 2):
    a, b, c = blobs[i : i + 3]
    if abs(a[1] - c[1]) <= 2 * scale and abs(a[2] - c[2]) <= 2 * scale:
        print(round(c[0] / scale), round(c[1] / scale))
        sys.exit(0)
sys.exit(1)
EOF
  local script="$shot_dir/mac-fs-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
m="$mac_fs_prefix"
jq="$jq_bin"
hc="$wmctl_bin"
scale=\$("\$hc" -j monitors 2>/dev/null | "\$jq" -r '.[0].scale')
# seen <log> <fixed text> <count>: once that many of the app's lines carry
# it, or 10 s; prints the count, or - with no log.
seen() {
  if [ -z "\$1" ]; then sleep 2; echo -; return; fi
  for _ in \$(seq 1 40); do [ "\$(grep -cF "\$2" "\$1" 2>/dev/null)" -ge "\$3" ] && break; sleep 0.25; done
  sleep 0.3
  local n
  n=\$(grep -cF "\$2" "\$1" 2>/dev/null)
  echo "\${n:-0}"
}
full() { "\$hc" -j activewindow 2>/dev/null | "\$jq" -r '.fullscreen // 0'; }
geo() { "\$hc" -j activewindow 2>/dev/null | "\$jq" -r '"\(.at[0]) \(.at[1]) \(.size[0]) \(.size[1])"'; }
key() { "$wtype_bin" -M logo "\$@" -m logo; sleep 1; }
chord_f() { "$wtype_bin" -M logo -M ctrl f -m ctrl -m logo; sleep 1.5; }
menu_bar=\$(busctl --user get-property org.atoll.AppMenu.Registrar /com/canonical/AppMenu/Registrar org.atoll.MenuBar Height 2>/dev/null | awk '{print \$2}')
echo "\${menu_bar:-none}" > "\$m-menubar.txt"
menu_bar=\${menu_bar:-0}
# top: the pointer up the middle of the screen to its top edge, as a hand
# moves it, which brings the bar and the window's title bar down.
top() {
  read -r x y w h <<< "\$(geo)"
  local cx=\$((x + w / 2))
  park "\$cx" 60; sleep 0.2; park "\$cx" 1; sleep 0.2; park "\$cx" 0
  sleep 1
  until_settled
}
# green <tag>: a click on the green button, found in a frame of the
# window's top left corner, below the bar when the window is in full screen.
green() {
  read -r x y w h <<< "\$(geo)"
  local dy=0 at
  [ "\$(full)" != 0 ] && dy=\$menu_bar
  park \$((x + w / 2)) \$((y + dy + 4)); sleep 0.4
  shoot "\$m-\$1-green.png"
  at=\$("$python3_bin" "$shot_dir/mac-fs-green.py" "\$m-\$1-green.png" "\$scale" "\$x" "\$((y + dy))" "\$((x + 160))" "\$((y + dy + 70))")
  echo "\$1: green button at \${at:-none}" >> "\$m-trail.txt"
  [ -n "\$at" ] || return 0
  park \$at; sleep 0.4
  click
  sleep 1.5
}
# view <tag>: the bar's View title, then its full screen item.
view() {
  call bar room > "\$m-\$1-room.json" 2>&1
  until_jq 10 "\$m-\$1-titles.json" 'any(.titles[]; .label == "View")' call appmenu status
  local cy vx id at
  cy=\$("\$jq" -r '.[0].cells[] | select(.name == "appMenu") | (.y + .height / 2 | floor)' "\$m-\$1-room.json" | head -n 1)
  vx=\$("\$jq" -r '.titles[] | select(.label == "View") | (.x + .width / 2 | floor)' "\$m-\$1-titles.json" | head -n 1)
  park "\$vx" "\$cy"; click
  until_jq 10 "\$m-\$1-open.json" '.open != null' call appmenu status
  until_settled
  call appmenu items View > "\$m-\$1-items.json" 2>&1
  id=\$("\$jq" -r '[.[] | select(.label | test("Full Screen"))][0].id // empty' "\$m-\$1-items.json")
  # The rows move while the card opens: read them where it came to rest.
  call appmenu status > "\$m-\$1-open.json" 2>&1
  shoot "\$m-\$1-menu.png"
  at=\$("\$jq" -r --argjson id "\${id:-0}" '.rows[] | select(.id == \$id) | "\(.x + (.width / 2 | floor)) \(.y + (.height / 2 | floor))"' "\$m-\$1-open.json")
  echo "\$1: View items \$("\$jq" -c '[.[] | .label]' "\$m-\$1-items.json" 2>&1 | head -c 300), full screen item at \${at:-none}" >> "\$m-trail.txt"
  [ -n "\$at" ] || return 0
  park \$at
  click
  sleep 1.5
}

# held <tag>: who has the keyboard in this frame, for a grey title bar.
held() {
  echo "\$1: keyboards \$("\$hc" -j devices 2>/dev/null | "\$jq" -c '[.keyboards[].name]') active pid \$("\$hc" -j activewindow 2>/dev/null | "\$jq" -r '.pid // "none"')" >> "\$m-trail.txt"
}
# walk <name> <log> <in> <out>
walk() {
  local n=\$1 log=\$2 in=\$3 out=\$4
  green "\$n-in"
  echo "green in: \$(seen "\$log" "\$in" 1) full=\$(full)" >> "\$m-\$n.txt"
  until_settled; shoot "\$m-\$n-full.png"
  top
  shoot "\$m-\$n-top.png"
  green "\$n-out"
  echo "green out: \$(seen "\$log" "\$out" 1) full=\$(full)" >> "\$m-\$n.txt"
  chord_f
  echo "chord in: \$(seen "\$log" "\$in" 2) full=\$(full)" >> "\$m-\$n.txt"
  chord_f
  echo "chord out: \$(seen "\$log" "\$out" 2) full=\$(full)" >> "\$m-\$n.txt"
  view "\$n-in"
  echo "menu in: \$(seen "\$log" "\$in" 3) full=\$(full)" >> "\$m-\$n.txt"
  top
  view "\$n-out"
  echo "menu out: \$(seen "\$log" "\$out" 3) full=\$(full)" >> "\$m-\$n.txt"
  chord_f
  echo "quit from: \$(seen "\$log" "\$in" 4) full=\$(full)" >> "\$m-\$n.txt"
  until_settled; shoot "\$m-\$n-before-quit.png"
  key q
  sleep 3
}

ebin=\$(cat "\$m-electron-bin.txt")
"\$hc" dispatch "hl.dsp.exec_cmd([==[sh -c 'ELECTRON_PANEL_WINDOW=1 exec \$ebin --ozone-platform=wayland > \$m-electron.log 2>&1']==])" > /dev/null 2>&1
epid=
for _ in \$(seq 1 180); do
  epid=\$(pgrep -o -f "\$ebin")
  [ -n "\$epid" ] && "\$hc" -j clients 2>/dev/null | "\$jq" -e --argjson p "\$epid" 'any(.[]; .pid == \$p)' > /dev/null 2>&1 && break
  sleep 0.5
done
"\$hc" dispatch "hl.dsp.focus({ window = 'pid:\$epid' })" > /dev/null 2>&1
sleep 4; until_settled
shoot "\$m-electron-start.png"
held electron-start
if $mac_fs_logged; then
  walk electron "\$m-electron.log" 'window enter-full-screen' 'window leave-full-screen'
else
  walk electron "" "" ""
fi
for _ in \$(seq 1 20); do pgrep -f "\$ebin" > /dev/null || break; sleep 0.5; done
pgrep -f "\$ebin" > /dev/null && echo "quit: running" >> "\$m-electron.txt" || echo "quit: gone" >> "\$m-electron.txt"
pkill -f "\$ebin"

[ -f "\$m-native-exec.txt" ] || exit 0
nexec=\$(cat "\$m-native-exec.txt")
"\$hc" dispatch "hl.dsp.exec_cmd([==[sh -c 'exec \$nexec > \$m-native.log 2>&1']==])" > /dev/null 2>&1
until_window $mac_fs_native_id 90
"\$hc" dispatch "hl.dsp.focus({ window = 'class:$mac_fs_native_id' })" > /dev/null 2>&1
sleep 2; until_settled
shoot "\$m-native-start.png"
held native-start
walk native "\$m-native.log" 'did enter full screen' 'did exit full screen'
for _ in \$(seq 1 20); do pgrep -f "$mac_fs_native_id" > /dev/null || break; sleep 0.5; done
pgrep -f "$mac_fs_native_id" > /dev/null && echo "quit: running" >> "\$m-native.txt" || echo "quit: gone" >> "\$m-native.txt"
EOF
  write_script "$shot_dir/mac-fs-kill.sh" <<EOF
#!/usr/bin/env bash
pkill -f "\$(cat "$mac_fs_prefix-electron-bin.txt")" 2>/dev/null || true
pkill -f "$mac_fs_native_id" 2>/dev/null || true
EOF
  add_cleanup "bash $shot_dir/mac-fs-kill.sh"
  wm_exec_once "bash $script"
}

leg_mac_fullscreen_assert() {
  local m="$mac_fs_prefix" n shot line c
  for shot in "$m"-*.png; do
    [ -f "$shot" ] && echo "SMOKE_MAC_FS_$(basename "$shot" .png | tr 'a-z-' 'A-Z_') $shot"
  done
  echo "== the shell's menu bar height: $(cat "$m-menubar.txt" 2>/dev/null)"
  cat "$m-trail.txt" 2>/dev/null
  for n in electron native; do
    [ -f "$m-$n.txt" ] || continue
    echo "== $n"
    cat "$m-$n.txt"
    grep -E 'full-screen|full screen|quit|terminate|close' "$m-$n.log" 2>/dev/null | tail -n 20
  done
  # The count of the app's own lines, - for an app that does not log.
  c='-'
  $mac_fs_logged && c=1
  for line in "green in: $c full=[12]" "green out: $c full=0" "chord in: ${c/1/2} full=[12]" "chord out: ${c/1/2} full=0" \
    "menu in: ${c/1/3} full=[12]" "menu out: ${c/1/3} full=0"; do
    grep -qE "^$line\$" "$m-electron.txt" 2>/dev/null \
      || fail "--mac-fullscreen: electron: wanted \"$line\", got: $(tr '\n' ';' < "$m-electron.txt" 2>/dev/null)"
  done
  [ "$mac_fs_native" = none ] && return 0
  for line in "green in: 1 full=[12]" "green out: 1 full=0" "chord in: 2 full=[12]" "chord out: 2 full=0" "quit from: [0-9]+ full=[12]" "quit: gone"; do
    grep -qE "^$line\$" "$m-native.txt" 2>/dev/null \
      || fail "--mac-fullscreen: native: wanted \"$line\", got: $(tr '\n' ';' < "$m-native.txt" 2>/dev/null)"
  done
}
