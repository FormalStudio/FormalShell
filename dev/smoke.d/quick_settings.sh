# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --quick-settings: the quick settings card
# (docs/superpowers/specs/2026-11-04-quick-settings.md) opened over `panel
# open quicksettings`, read off `quicksettings status`. Its rows in order:
# the system row's buttons, then the toggles in the spec's order, each
# kind right. Its geometry off the card's own rect: 400 wide, every toggle
# 178 by 48, 12 between columns and rows, 16 from every edge. The rig's
# honest states: its private system bus carries no NetworkManager, BlueZ
# or power-profiles-daemon, so Wi-Fi, Bluetooth and Power mode are
# disabled with their reasons, and `quicksettings activate` refuses them
# and an unknown id. Dark style, Do not disturb and Caffeinate are walked
# to on real arrows and flipped by a real Return each way, read back off
# `theme status`, `notifications dndState` and `caffeinate status`; under
# `--light`, which pins the mode in settings.json, Dark style is disabled
# saying so instead. Escape closes the card, and settings.json's sha256 is
# the same at the end. The card photographed at rest and with the toggles
# on; ride `--showcase` for the `on` fill in a wallpaper's colour.
#
# Then the sliders and the in-place menu, over two null sinks of the leg's
# own: Volume a slider with its arrow, Microphone only while pw-record
# reads a virtual source, Brightness only where the host has a backlight
# or a DDC display. Right twice on the Volume track, read off pactl's volume as
# 0.10 on the shell's cube-root scale. The Volume arrow reached from the
# Power button by a real Down and opened by Return at a tenth speed, its
# frames and statuses read as the rows under it moving down and the menu's
# rows fading in only once its height has landed, the dim up from the first
# frame, the last toggle reading 0.6 of its luminance once it settles and
# the card's fill beside it its own. Down to the second sink inside the
# stale audit and Return making it the default (`pactl get-default-sink`);
# Escape closing the menu, then the card. The Power button's menu on this
# rig's private system bus, where logind answers nothing, is Log out alone:
# one Return arms it ("Confirm log out?") and Escape closes the menu with
# nothing run. The menu photographed open dark and with `--light`.
leg_quick_settings_flag="--quick-settings"
leg_quick_settings_order=262
leg_quick_settings_needs="jq wtype grim convert pactl pw-loopback pw-record"

quick_settings_dir="$shot_dir/quick-settings"
quick_settings_settings="$iso_home/.config/formalshell/settings.json"

need_pw_loopback() { need_tool pw_loopback_bin pw-loopback pipewire-tools; }
need_pw_record() { need_tool pw_record_bin pw-record pipewire-tools; }

leg_quick_settings_timing() {
  # Two opens of the card past the first, one of them at a tenth speed.
  leg_timing 60 200 0 170
}

leg_quick_settings_drive() {
  local script="$shot_dir/quick-settings-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
d="$quick_settings_dir"
mkdir -p "\$d"
status() {
  $ipc call quicksettings status > "\$d/status-\$1.json" 2>&1
}
shot() {
  "$grim_bin" "\$d/quick-settings-\$1.png" > /dev/null 2>&1
  status "shot-\$1"
}
# Arrows toward the stop \$1 off the boxes \`status\` reports (a row, its
# icon and arrow, a menu row), the cursor on each step written to \$2. A
# hidden cursor takes a first key to show. Left and Right only along one
# row, so a slider's track is never stepped on the way.
walk_to() {
  local n=0 at key
  : > "\$d/\$2"
  if [ "\$($ipc call quicksettings status 2>/dev/null | "$jq_bin" -r '.cursor // ""')" = "" ]; then
    "$wtype_bin" -k Down
    sleep 0.3
  fi
  while [ \$n -lt 14 ]; do
    $ipc call quicksettings status > "\$d/walk.json" 2>/dev/null
    at=\$("$jq_bin" -r '.cursor // ""' "\$d/walk.json")
    echo "\$at" >> "\$d/\$2"
    [ "\$at" = "\$1" ] && return 0
    key=\$("$jq_bin" -r --arg at "\$at" --arg to "\$1" '
      ([.rows[] | {key: .id, value: .rect}, {key: (.id + ":icon"), value: .icon}, {key: (.id + ":menu"), value: .arrow}]
        + [(.menu.rows // [])[] | {key: .id, value: .rect}] | map(select(.value != null)) | from_entries) as \$box
      | \$box[\$at] as \$a | \$box[\$to] as \$b
      | if \$a == null or \$b == null then "Down"
        elif \$b.y >= \$a.y + \$a.height then "Down"
        elif \$b.y + \$b.height <= \$a.y then "Up"
        elif \$b.x > \$a.x then "Right" else "Left" end' "\$d/walk.json")
    "$wtype_bin" -k "\${key:-Down}"
    sleep 0.3
    n=\$((n + 1))
  done
}
press() {
  "$wtype_bin" -k Return
  sleep "\${1:-0.8}"
}
sleep 3
sha256sum "$quick_settings_settings" | cut -d' ' -f1 > "\$d/sha-before"
$ipc call panel open quicksettings > "\$d/open.txt" 2>&1
sleep 1.5
$ipc call panel state > "\$d/panel-open.txt" 2>&1
$ipc call debug dump > "\$d/dump-open.json" 2>&1
shot open
$ipc call quicksettings activate wifi > "\$d/activate-wifi.txt" 2>&1
$ipc call quicksettings activate nothing > "\$d/activate-unknown.txt" 2>&1
$ipc call theme status > "\$d/theme-before.json" 2>&1
if "$jq_bin" -e '.rows[] | select(.id == "dark-style") | .available' "\$d/status-shot-open.json" > /dev/null 2>&1; then
  walk_to dark-style walk-dark.txt
  press 2
  $ipc call theme status > "\$d/theme-flipped.json" 2>&1
  status dark-flipped
  shot dark-flipped
  press 2
  $ipc call theme status > "\$d/theme-back.json" 2>&1
fi
$ipc call notifications dndState > "\$d/dnd-before.txt" 2>&1
walk_to dnd walk-dnd.txt
press
$ipc call notifications dndState > "\$d/dnd-flipped.txt" 2>&1
$ipc call caffeinate status > "\$d/caffeinate-before.json" 2>&1
walk_to caffeinate walk-caffeinate.txt
press
$ipc call caffeinate status > "\$d/caffeinate-flipped.json" 2>&1
status on
shot on
press
$ipc call caffeinate status > "\$d/caffeinate-back.json" 2>&1
walk_to dnd walk-dnd-back.txt
press
$ipc call notifications dndState > "\$d/dnd-back.txt" 2>&1
"$wtype_bin" -k Escape
sleep 1
$ipc call panel state > "\$d/panel-closed.txt" 2>&1
status closed

pactl() { "$pactl_bin" "\$@"; }
pactl get-default-sink > "\$d/default-sink-before.txt" 2>&1
pactl get-default-source > "\$d/default-source-before.txt" 2>&1
pactl load-module module-null-sink sink_name=qs_one sink_properties=device.description=QuickOne > "\$d/module-one.txt" 2>&1
pactl load-module module-null-sink sink_name=qs_two sink_properties=device.description=QuickTwo > "\$d/module-two.txt" 2>&1
pactl set-default-sink qs_one
pactl set-sink-volume qs_one 50%
setsid "$pw_loopback_bin" -n qs-mic -i "{ media.class=Audio/Sink node.name=qs_mic_in }" \
  -o "{ media.class=Audio/Source node.name=qs_mic node.description=QuickMic }" > /dev/null 2>&1 < /dev/null &
echo \$! > "\$d/loopback.pid"
sleep 1.5
pactl set-default-source qs_mic
$ipc call panel open quicksettings > /dev/null 2>&1
sleep 1.5
# The sinks reach the store off PipeWire's own events, a beat behind pactl.
for i in \$(seq 1 20); do
  $ipc call quicksettings status 2>/dev/null | "$jq_bin" -e '.rows[] | select(.id == "volume") | .arrow != null' > /dev/null 2>&1 && break
  sleep 0.5
done
status sliders
"$pw_record_bin" --target qs_mic "\$d/mic.wav" > /dev/null 2>&1 &
rec=\$!
sleep 1.5
status mic-live
kill \$rec 2>/dev/null
wait \$rec 2>/dev/null
rm -f "\$d/mic.wav"
sleep 1.5
status mic-done
kill "\$(cat "\$d/loopback.pid")" 2>/dev/null

pactl get-sink-volume qs_one > "\$d/volume-before.txt" 2>&1
walk_to volume walk-volume.txt
"$wtype_bin" -k Right
sleep 0.5
"$wtype_bin" -k Right
sleep 1.2
pactl get-sink-volume qs_one > "\$d/volume-after.txt" 2>&1
status volume-stepped

# A card takes the motion scale it opens under, so the menu's tenth speed
# needs the card opened again.
"$wtype_bin" -k Escape
sleep 1.2
$ipc call debug motionScale 1000 > /dev/null 2>&1
$ipc call panel open quicksettings > /dev/null 2>&1
sleep 5
walk_to power walk-power.txt
"$wtype_bin" -k Down
sleep 0.8
status at-arrow
"$grim_bin" "\$d/quick-settings-at-arrow.png" > /dev/null 2>&1
"$wtype_bin" -k Return
for i in \$(seq 1 18); do
  status "burst-\$i"
  "$grim_bin" "\$d/quick-settings-burst-\$i.png" > /dev/null 2>&1
  sleep 0.1
done
sleep 3
shot menu-open
$(stale_audit_start)
n=0
while [ \$n -lt 8 ]; do
  [ "\$($ipc call quicksettings status | "$jq_bin" -r '.cursor // ""')" = "m:volume:qs_two" ] && break
  "$wtype_bin" -k Down
  sleep 0.4
  n=\$((n + 1))
done
sleep 0.6
$(stale_audit_stop quick-settings-menu)
status menu-at-two
press 1.2
pactl get-default-sink > "\$d/default-sink-after.txt" 2>&1
status menu-picked
"$wtype_bin" -k Escape
sleep 6
status menu-closed
"$wtype_bin" -k Escape
sleep 5
$ipc call panel state > "\$d/panel-closed-menu.txt" 2>&1
$ipc call debug motionScale 100 > /dev/null 2>&1

$ipc call panel open quicksettings > /dev/null 2>&1
sleep 1.5
walk_to power walk-power-menu.txt
press 1.5
status power-open
press 1
status power-armed
shot power-armed
$ipc call panel state > "\$d/panel-armed.txt" 2>&1
"$wtype_bin" -k Escape
sleep 1.2
status power-closed
"$wtype_bin" -k Escape
sleep 1

pactl set-default-sink "\$(cat "\$d/default-sink-before.txt")" 2>/dev/null
pactl set-default-source "\$(cat "\$d/default-source-before.txt")" 2>/dev/null
pactl unload-module "\$(cat "\$d/module-one.txt")" 2>/dev/null
pactl unload-module "\$(cat "\$d/module-two.txt")" 2>/dev/null
kill "\$(cat "\$d/loopback.pid")" 2>/dev/null
sha256sum "$quick_settings_settings" | cut -d' ' -f1 > "\$d/sha-after"
EOF
  add_cleanup "[ -s '$quick_settings_dir/module-one.txt' ] && '$pactl_bin' unload-module \"\$(cat '$quick_settings_dir/module-one.txt')\" 2>/dev/null || true"
  add_cleanup "[ -s '$quick_settings_dir/module-two.txt' ] && '$pactl_bin' unload-module \"\$(cat '$quick_settings_dir/module-two.txt')\" 2>/dev/null || true"
  add_cleanup "[ -s '$quick_settings_dir/loopback.pid' ] && kill \"\$(cat '$quick_settings_dir/loopback.pid')\" 2>/dev/null || true"
  hypr_exec_once "bash $script"
}

quick_settings_jq() {
  local what="$1" file="$quick_settings_dir/$2" expr="$3"
  [ -s "$file" ] || fail "quick-settings: no $2 for $what"
  "$jq_bin" -e "$expr" "$file" > /dev/null 2>&1 \
    || fail "quick-settings: $what: $("$jq_bin" -c '{card, cursor, dim, menu, rows: [.rows[]? | {id, kind, on, available, rect, icon, arrow}]}' "$file" 2>/dev/null || head -c 1200 "$file")"
  echo "SMOKE_QUICK_SETTINGS $what"
}

quick_settings_text() {
  local what="$1" file="$quick_settings_dir/$2" want="$3"
  [ "$(cat "$file" 2>/dev/null)" = "$want" ] || fail "quick-settings: $what: expected '$want', got '$(cat "$file" 2>/dev/null)'"
  echo "SMOKE_QUICK_SETTINGS $what"
}

quick_settings_walked() {
  local what="$1" file="$quick_settings_dir/$2" want="$3"
  [ "$(tail -n1 "$file" 2>/dev/null)" = "$want" ] || fail "quick-settings: $what: the cursor walked $(tr '\n' ' ' < "$file" 2>/dev/null)"
  echo "SMOKE_QUICK_SETTINGS $what: $(tr '\n' ' ' < "$file")"
}

leg_quick_settings_assert() {
  local open="status-shot-open.json"
  quick_settings_text "panel open quicksettings answered ok" open.txt "ok"
  quick_settings_text "panel state names it" panel-open.txt "quicksettings"
  quick_settings_jq "status reads the card open" "$open" '.open == true and .from == "ipc" and .dim == false and .menu == null'
  # GNOME's order, less what this machine has no service for.
  quick_settings_jq "the rows in the spec's order" "$open" '
    ["battery","screenshot","settings","lock","power","volume","microphone","brightness","wired","wifi","bluetooth","power-mode","night-light","dark-style","dnd","keyboard","caffeinate","overnight","hdr"] as $order
    | [.rows[].id] as $ids
    | ($ids | map(. as $i | $order | index($i))) as $at
    | ($at | all(. != null)) and $at == ($at | sort)
      and (["screenshot","settings","lock","power","wifi","bluetooth","power-mode","night-light","dark-style","dnd","caffeinate","overnight"] - $ids) == []'
  quick_settings_jq "the system row, the sliders, then the toggles" "$open" '
    [.rows[].kind] as $k | ($k | all(. == "system" or . == "slider" or . == "toggle" or . == "split"))
      and $k == ($k | sort_by(if . == "system" then 0 elif . == "slider" then 1 else 2 end))'
  # Every edge off the card's own rect, the toggles off each other.
  # A split toggle's rect is its toggle part, its arrow the rest of the pill.
  local pill='def pill: if .arrow then {x: .rect.x, y: .rect.y, width: (.arrow.x + .arrow.width - .rect.x), height: .rect.height} else .rect end;'
  quick_settings_jq "the card is 400 wide, the toggles 178 by 48" "$open" "$pill"'
    .card.width == 400 and ([.rows[] | select(.kind == "toggle" or .kind == "split") | pill | .width == 178 and .height == 48] | all)'
  quick_settings_jq "12 between columns and rows, 16 from every edge" "$open" "$pill"'
    def near($a; $b): ($a - $b) | fabs <= 1;
    .card as $c
    | [.rows[] | select(.kind == "toggle" or .kind == "split") | pill] as $t
    | [.rows[] | select(.kind == "system") | .rect] as $s
    | ([.rows[] | select(.kind == "slider")] | length) as $n
    | ($t | map(.x) | min) as $left
    | ($t | map(.x + .width) | max) as $right
    | near($left - $c.x; 16) and near($c.x + $c.width - $right; 16)
      and near(($s | map(.y) | min) - $c.y; 16)
      and near($c.y + $c.height - ($t | map(.y + .height) | max); 16)
      and near(($s | map(.x + .width) | max); $right)
      and near(($t[0].y) - ($s | map(.y + .height) | max); 12 + $n * 52)
      and near($t[1].x - ($t[0].x + $t[0].width); 12)
      and near($t[2].y - ($t[0].y + $t[0].height); 12)'
  quick_settings_jq "Wi-Fi disabled with no NetworkManager" "$open" '.rows[] | select(.id == "wifi") | .available == false and .reason == "No NetworkManager" and .on == false'
  quick_settings_jq "Bluetooth disabled with no adapter" "$open" '.rows[] | select(.id == "bluetooth") | .available == false and .reason == "No adapter"'
  quick_settings_jq "Power mode disabled with no daemon" "$open" '.rows[] | select(.id == "power-mode") | .available == false and .reason == "power-profiles-daemon is not running"'
  quick_settings_text "activate refuses an unavailable control" activate-wifi.txt "error: wifi is unavailable: No NetworkManager"
  quick_settings_text "activate refuses an unknown id" activate-unknown.txt "error: unknown control 'nothing'"
  local first walk=walk-dark.txt
  leg_on light && walk=walk-dnd.txt
  first=$("$jq_bin" -r '[.rows[] | select(.kind == "toggle" and .available)][0].id' "$quick_settings_dir/$open")
  [ "$(head -n1 "$quick_settings_dir/$walk" 2>/dev/null)" = "$first" ] \
    || fail "quick-settings: the cursor started on '$(head -n1 "$quick_settings_dir/$walk" 2>/dev/null)', not the first usable toggle '$first'"
  echo "SMOKE_QUICK_SETTINGS the cursor starts on the first usable toggle, $first"
  if leg_on light; then
    quick_settings_jq "under a pinned mode Dark style says so" "$open" '.rows[] | select(.id == "dark-style") | .available == false and .reason == "Set in settings.json" and .on == false'
  else
    quick_settings_walked "arrows reached Dark style" walk-dark.txt dark-style
    quick_settings_jq "Return on Dark style made the mode light" theme-flipped.json '.mode == "light"'
    quick_settings_jq "the tile reads off" status-dark-flipped.json '.rows[] | select(.id == "dark-style") | .on == false'
    quick_settings_jq "Return again made it dark" theme-back.json '.mode == "dark"'
  fi
  quick_settings_walked "arrows reached Do not disturb" walk-dnd.txt dnd
  quick_settings_text "do not disturb was off" dnd-before.txt off
  quick_settings_text "Return turned do not disturb on" dnd-flipped.txt on
  quick_settings_walked "arrows reached Caffeinate" walk-caffeinate.txt caffeinate
  quick_settings_jq "caffeinate was off" caffeinate-before.json '.active == false'
  quick_settings_jq "Return caffeinated the session" caffeinate-flipped.json '.active == true'
  quick_settings_jq "the tiles read on" status-on.json '[.rows[] | select(.id == "dnd" or .id == "caffeinate") | .on] == [true, true]'
  quick_settings_jq "Return again let it go" caffeinate-back.json '.active == false'
  quick_settings_walked "arrows went back to Do not disturb" walk-dnd-back.txt dnd
  quick_settings_text "Return turned do not disturb off" dnd-back.txt off
  quick_settings_text "Escape closed the card" panel-closed.txt ""
  quick_settings_jq "status reads it shut" status-closed.json '.open == false and .card == null'
  local before after shot
  before=$(cat "$quick_settings_dir/sha-before" 2>/dev/null)
  after=$(cat "$quick_settings_dir/sha-after" 2>/dev/null)
  [ -n "$before" ] && [ "$before" = "$after" ] || fail "quick-settings: settings.json changed ($before -> $after)"
  echo "SMOKE_QUICK_SETTINGS settings.json unchanged $after"
  quick_settings_menu_assert
  for shot in open on menu-open power-armed; do
    [ -s "$quick_settings_dir/quick-settings-$shot.png" ] || fail "quick-settings: no quick-settings-$shot.png"
    echo "SMOKE_QUICK_SETTINGS_SHOT $quick_settings_dir/quick-settings-$shot.png"
  done
}

# The dim behind the open menu off the frames: the last toggle's mean
# luminance with the menu open over the same toggle's before it opened,
# GNOME's brightness -0.4 reading 0.6 of it, and the card's own fill in the
# gap before that toggle unchanged, since only the items dim.
quick_settings_dim() {
  local d="$quick_settings_dir" id before after ratio gap
  id=$("$jq_bin" -r '[.rows[] | select(.kind == "toggle" or .kind == "split")][-1].id' "$d/status-at-arrow.json")
  luma() {
    "$convert_bin" "$d/$1" -crop "$2" +repage -colorspace Gray -format '%[fx:mean]' info: 2>/dev/null
  }
  ratio_of() {
    awk -v a="$1" -v b="$2" 'BEGIN { if (a > 0) printf "%.3f", b / a }'
  }
  box() {
    scaled_jq -r --arg id "$id" "$geometry_jq"'.rows[] | select(.id == $id) | .rect | geometry' "$d/$1"
  }
  before=$(luma quick-settings-at-arrow.png "$(box status-at-arrow.json)")
  after=$(luma quick-settings-menu-open.png "$(box status-shot-menu-open.json)")
  ratio=$(ratio_of "$before" "$after")
  awk -v r="$ratio" 'BEGIN { exit !(r != "" && r >= 0.5 && r <= 0.7) }' \
    || fail "quick-settings: $id under the open menu reads $after against $before before it opened ($ratio), not dimmed to 0.6"
  echo "SMOKE_QUICK_SETTINGS the dim: $id reads $ratio of its luminance under the open menu"
  # The column gap left of the last toggle, inset 2px off each toggle's
  # antialiased edge; a last row of one toggle has none.
  gap() {
    scaled_jq -r --arg id "$id" "$geometry_jq"'[.rows[] | select(.kind == "toggle" or .kind == "split")] as $t
      | ($t[-1].rect) as $b | [$t[] | select(.rect.y == $b.y and .id != $id)][-1].rect as $a
      | if $a == null then empty else {x: ($a.x + $a.width + 2), y: ($b.y + 8), width: ($b.x - $a.x - $a.width - 4), height: ($b.height - 16)} | geometry end' "$d/$1"
  }
  gap=$(gap status-at-arrow.json)
  if [ -z "$gap" ]; then
    echo "SMOKE_QUICK_SETTINGS the card fill: no toggle beside $id, skipped"
    return
  fi
  before=$(luma quick-settings-at-arrow.png "$gap")
  after=$(luma quick-settings-menu-open.png "$(gap status-shot-menu-open.json)")
  ratio=$(ratio_of "$before" "$after")
  awk -v r="$ratio" 'BEGIN { exit !(r != "" && r >= 0.97 && r <= 1.03) }' \
    || fail "quick-settings: the card fill beside $id reads $after under the open menu against $before before ($ratio), dimmed with the items"
  echo "SMOKE_QUICK_SETTINGS the card fill: $ratio of its luminance beside $id under the open menu"
}

# The jq over one burst status: the menu's height and content shares.
quick_settings_burst() {
  "$jq_bin" -r '[(.menu.height // 0), (.menu.content // 0), ([.rows[] | select(.kind == "toggle" or .kind == "split")][0].rect.y), .dim, .cursor] | @tsv' "$quick_settings_dir/status-burst-$1.json" 2>/dev/null
}

quick_settings_menu_assert() {
  local d="$quick_settings_dir"
  quick_settings_jq "Volume is a slider with its icon and arrow over two sinks" status-sliders.json '
    .rows[] | select(.id == "volume") | .kind == "slider" and .icon != null and .arrow != null and .rect != null and (.value | type) == "number"'
  quick_settings_jq "Brightness, where this host has a screen to set, a slider with its icon" status-sliders.json '
    [.rows[] | select(.id == "brightness")] | all(.kind == "slider" and .icon != null)'
  quick_settings_jq "no Microphone while nothing records" status-sliders.json '[.rows[].id] | index("microphone") == null'
  quick_settings_jq "Microphone shows while pw-record reads a source" status-mic-live.json '.rows[] | select(.id == "microphone") | .kind == "slider" and .icon != null'
  quick_settings_jq "and goes once it stops" status-mic-done.json '[.rows[].id] | index("microphone") == null'
  quick_settings_walked "arrows reached the Volume track" walk-volume.txt volume
  local before after
  # pipewire-pulse reports a channel's volume as the cube root of its linear
  # one, the shell's own scale, over 65536.
  before=$(awk '/^Volume:/ { printf "%.3f", $3 / 65536 }' "$d/volume-before.txt" 2>/dev/null)
  after=$(awk '/^Volume:/ { printf "%.3f", $3 / 65536 }' "$d/volume-after.txt" 2>/dev/null)
  awk -v a="$before" -v b="$after" 'BEGIN { exit !(a != "" && b != "" && b - a > 0.09 && b - a < 0.11) }' \
    || fail "quick-settings: Right twice on Volume moved qs_one $before -> $after, not 0.10: $(cat "$d/volume-before.txt" "$d/volume-after.txt")"
  echo "SMOKE_QUICK_SETTINGS Right twice on Volume: qs_one $before -> $after"
  quick_settings_walked "Down from Power lands on the Volume arrow" walk-power.txt power
  quick_settings_jq "the cursor on the Volume arrow" status-at-arrow.json '.cursor == "volume:menu" and .menu == null and .dim == false'
  local i line h c y dim rest open grew=0 faded=0 early=0 moved=0 seen=""
  rest=$("$jq_bin" -r '[.rows[] | select(.kind == "toggle" or .kind == "split")][0].rect.y' "$d/status-at-arrow.json")
  open=$("$jq_bin" -r '[.rows[] | select(.kind == "toggle" or .kind == "split")][0].rect.y' "$d/status-shot-menu-open.json")
  for i in $(seq 1 18); do
    line=$(quick_settings_burst "$i")
    [ -n "$line" ] || fail "quick-settings: burst $i: no status"
    read -r h c y dim at <<< "$line"
    seen="$seen $h/$c@$y:$at"
    [ "$dim" = true ] || fail "quick-settings: burst $i: the dim is not up ($line)"
    awk -v h="$h" 'BEGIN { exit !(h > 0 && h < 1) }' && grew=$((grew + 1))
    awk -v h="$h" -v c="$c" 'BEGIN { exit !(h >= 0.98 && c > 0 && c < 1) }' && faded=$((faded + 1))
    awk -v h="$h" -v c="$c" 'BEGIN { exit !(h < 0.98 && c > 0) }' && early=$((early + 1))
    awk -v y="$y" -v a="$rest" -v b="$open" 'BEGIN { exit !(y > a + 1 && y < b - 1) }' && moved=$((moved + 1))
  done
  echo "quick-settings burst height/content@first-toggle-y:$seen (rest $rest, open $open)"
  [ "$grew" -ge 1 ] || fail "quick-settings: no frame caught the menu's height part way"
  [ "$faded" -ge 1 ] || fail "quick-settings: no frame caught the rows fading in after the height landed"
  [ "$early" -eq 0 ] || fail "quick-settings: $early frames showed rows before the menu's height landed"
  [ "$moved" -ge 1 ] || fail "quick-settings: no frame caught the rows under the menu part way down"
  echo "SMOKE_QUICK_SETTINGS the menu grew in $grew frames, its rows faded in over $faded after, the rows under it moving in $moved"
  quick_settings_jq "the open menu lists the sinks with the default checked, Sound settings last" status-shot-menu-open.json '
    .menu.id == "volume" and .dim == true
    and ([.menu.rows[] | select(.id == "m:volume:qs_one")][0].checked == true)
    and ([.menu.rows[] | select(.id == "m:volume:qs_two")][0].checked == false)
    and (.menu.rows[-1] | .id == "m:settings" and .panel == "audio")
    and .cursor == .menu.rows[0].id'
  quick_settings_jq "the menu spans the grid 12 under the Volume row" status-shot-menu-open.json '
    def near($a; $b): ($a - $b) | fabs <= 1;
    ([.rows[] | select(.kind == "toggle" or .kind == "split")][0].rect) as $t
    | (.rows[] | select(.id == "volume") | .icon) as $v
    | .menu.rect as $m
    | near($m.x; $t.x) and near($m.width; 368) and near($m.y; $v.y + $v.height + 2 + 12)'
  local menu_h
  menu_h=$("$jq_bin" -r '.menu.rect.height' "$d/status-shot-menu-open.json")
  awk -v a="$rest" -v b="$open" -v h="$menu_h" 'BEGIN { d = b - a - h - 12; exit !(d >= -1 && d <= 1) }' \
    || fail "quick-settings: the toggles moved $rest -> $open under a menu $menu_h tall, not by its height and a gap"
  echo "SMOKE_QUICK_SETTINGS the rows after the menu moved down by its height and a gap: $rest -> $open, menu $menu_h"
  quick_settings_dim
  quick_settings_jq "Down walked inside the menu to the second sink" status-menu-at-two.json '.cursor == "m:volume:qs_two"'
  stale_audit_assert quick-settings-menu
  quick_settings_text "Return made the second sink the default" default-sink-after.txt qs_two
  quick_settings_jq "the menu checks it" status-menu-picked.json '[.menu.rows[] | select(.id == "m:volume:qs_two")][0].checked == true'
  quick_settings_jq "Escape closed the menu, the cursor back on the arrow" status-menu-closed.json '.open == true and .menu == null and .dim == false and .cursor == "volume:menu"'
  quick_settings_text "a second Escape closed the card" panel-closed-menu.txt ""
  quick_settings_walked "arrows reached the Power button" walk-power-menu.txt power
  quick_settings_jq "with no logind the Power menu is Log out alone" status-power-open.json '
    .menu.id == "power" and ([.menu.rows[].id] == ["m:power:logout"]) and .menu.rows[0].title == "Log out…" and .cursor == "m:power:logout"'
  quick_settings_jq "one Return armed it and ran nothing" status-power-armed.json '.open == true and .menu.rows[0].armed == true and .menu.rows[0].title == "Confirm log out?"'
  quick_settings_text "the card stayed up armed" panel-armed.txt "quicksettings"
  quick_settings_jq "Escape closed the menu disarmed" status-power-closed.json '.open == true and .menu == null and .cursor == "power"'
  echo "SMOKE_QUICK_SETTINGS_SHOT $d/quick-settings-burst-6.png"
}
