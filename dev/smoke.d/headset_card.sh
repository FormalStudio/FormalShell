# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --headset-card drives the headset connect card (new in the Rust shell, spec
# 2026-10-06-rust-rewrite.md under R7). The rig has no Bluetooth controller,
# so the shell's system bus is a private dbus-daemon of the leg's own, where
# dev/bluez-stub.py plays BlueZ off a JSON device list (`address`, `name`,
# `connected`, `icon`, `battery`) it reads again whenever the file changes:
# the leg connects and disconnects a device by rewriting that file, and the
# shell sees Device1 and Battery1 change as it would on a real adapter.
#
# What it proves, off the shell's own `headset card mapped` and `unmapped`
# log lines and the frames:
#  1. A device connected at startup raises no card.
#  2. A headphone going from disconnected to connected raises one carrying
#     its icon, its name, "Connected" and one ring from BlueZ's battery.
#  3. A real pointer parked on the card holds it past its four seconds, and
#     the pointer leaving then dismisses it.
#  4. The same device disconnecting and reconnecting a second later raises
#     none.
#  5. A pair of AirPods (the librepods status file staged first) raises a
#     card wearing the backend's picture set where the headphone's wore
#     none, both a capsule (`debug dump`'s `headset`: one height, the
#     radius half of it) as wide as its name, the AirPods' the wider, and
#     the pointer leaving the card dismisses it.
#  6. Studio Cans, which no backend names, still has the earbuds service
#     held to learn its picture set once Escape has taken its card down
#     (`headset.learning`).
#     Escape, typed by wtype (a real virtual keyboard) into a foot window
#     that has the focus and never loses it, dismisses a card held up by the
#     pointer parked on it, through the
#     non-consuming bind the card adds while it is up: one Escape bind in
#     `hyprctl binds` with the card up and none after, the active window
#     the same foot throughout, and the letters typed around the Escape
#     reaching foot with the Escape between them.
#  7. With do-not-disturb on in state.json a connect raises none.
#  8. settings.json rewritten to `layout: sidebar`, the AirPods connecting
#     again with the librepods status file gone raise the card off the top
#     edge's middle all the same, not off the column's line: wider than
#     tall, centred on the output within 2px and near its top, and wearing
#     the picture set and the name state.json's `earbudsFaces` kept from
#     step 5, since no backend names the pair this time and BlueZ's name
#     for them is cut short (off `debug dump`'s `headset`).
#
# The pointer is wlrctl, a real virtual-pointer client, for the reason
# tooltip.sh documents: only one sends the surface a pointer enter.
leg_headset_card_flag="--headset-card"
leg_headset_card_order=177
leg_headset_card_needs="wlrctl jq wtype foot python3"
leg_headset_card_claims="bus.system pointer layout bar.position=top frame=none"

headset_dir="$shot_dir/headset-card"
headset_list="$headset_dir/bluetooth.json"
headset_bus="$headset_dir/bluez-bus"
headset_log_marks="$headset_dir/marks.txt"
headset_bluez_png="$shot_dir/headset-card-bluez.png"
headset_held_png="$shot_dir/headset-card-held.png"
headset_airpods_png="$shot_dir/headset-card-airpods.png"
headset_dnd_png="$shot_dir/headset-card-dnd.png"
headset_escape_png="$shot_dir/headset-card-escape.png"
headset_typed="$headset_dir/typed.txt"
headset_escape_marks="$headset_dir/escape.txt"
headset_sidebar_png="$shot_dir/headset-card-sidebar.png"
headset_sidebar_dump="$headset_dir/sidebar-dump.json"
headset_bluez_dump="$headset_dir/bluez-dump.json"
headset_airpods_dump="$headset_dir/airpods-dump.json"

headset_buds_address="AC:12:2F:11:22:33"
headset_cans_address="11:22:33:44:55:66"
headset_pods_address="AA:BB:CC:DD:EE:01"

# The band the card hangs in, below the bar's strip and centred, in the
# logical pixels grim -g and the pointer take: the output's centre is x 768
# at 1.25.
headset_park_y=85

headset_devices() {
  # $1 buds connected, $2 cans connected, $3 pods connected (true or false)
  printf '[{"address":"%s","name":"Liberty 4 NC","connected":%s,"battery":80},{"address":"%s","name":"Studio Cans","connected":%s,"icon":"audio-headphones","battery":55},{"address":"%s","name":"Kyan'"'"'s AirPods Pro","connected":%s}]\n' \
    "$headset_buds_address" "$1" "$headset_cans_address" "$2" "$headset_pods_address" "$3"
}

leg_headset_card_fixture() {
  mkdir -p "$headset_dir"
  headset_devices true false false > "$headset_list"
  cp dev/smoke.d/fixtures/airpods-pro3.json "$headset_dir/airpods-status.json"
  [ -s "$headset_dir/airpods-status.json" ] || { echo "headset-card: dev/smoke.d/fixtures/airpods-pro3.json is missing" >&2; exit 1; }
  local daemon pid
  daemon=$(command -v dbus-daemon) || fail "--headset-card: no dbus-daemon"
  pid=$("$daemon" --session --address="unix:path=$headset_bus" --fork --print-pid --nopidfile)
  add_cleanup "kill $pid 2>/dev/null"
  "$python3_bin" dev/bluez-stub.py --address "unix:path=$headset_bus" --devices "$headset_list" \
    --ready "$headset_dir/bluez-ready" > "$headset_dir/bluez-stub.log" 2>&1 &
  add_cleanup "kill $! 2>/dev/null"
  for _ in $(seq 50); do
    [ -s "$headset_dir/bluez-ready" ] && break
    sleep 0.1
  done
  [ -s "$headset_dir/bluez-ready" ] || fail "--headset-card: the stub never owned org.bluez ($(cat "$headset_dir/bluez-stub.log"))"
  shell_env="${shell_env:+$shell_env }DBUS_SYSTEM_BUS_ADDRESS=unix:path=$headset_bus"
}

leg_headset_card_timing() {
  leg_timing 86 120
}

leg_headset_card_drive() {
  local script="$shot_dir/headset-card-drive.sh"
  local state_dir="$iso_home/.local/state/librepods"
  local fs_state="$iso_home/.local/state/formalshell/state.json"
  local headset_park_x headset_region
  headset_park_x=$((logical_w / 2))
  headset_region="$((headset_park_x - 300)),30 600x150"
  write_script "$script" <<EOF
#!/usr/bin/env bash
list() { local pods="Kyan's AirPods Pro"; [ -z "\${4:-}" ] || pods=\$4; printf '[{"address":"$headset_buds_address","name":"Liberty 4 NC","connected":%s,"battery":80},{"address":"$headset_cans_address","name":"Studio Cans","connected":%s,"icon":"audio-headphones","battery":55},{"address":"$headset_pods_address","name":"%s","connected":%s}]\n' "\$1" "\$2" "\$pods" "\$3" > "$headset_list"; }
mapped() { grep -c 'headset card mapped' "$shell_log_path"; }
unmapped() { grep -c 'headset card unmapped' "$shell_log_path"; }
mark() { echo "\$1 mapped=\$(mapped) unmapped=\$(unmapped)" >> "$headset_log_marks"; }
hover() {
  park
  sleep 0.5
  park "\$1" "\$2"
}
leave() { park 400 600; }
: > "$headset_log_marks"
: > "$headset_escape_marks"
sleep 6
mark startup
list true true false
sleep 2
$ipc call debug dump 2>/dev/null | "$jq_bin" -c '.headset' > "$headset_bluez_dump"
"$grim_bin" -g "$headset_region" "$headset_bluez_png" > /dev/null 2>&1
mark connected
hover $headset_park_x $headset_park_y
sleep 6
"$grim_bin" -g "$headset_region" "$headset_held_png" > /dev/null 2>&1
mark held
leave
sleep 2
mark released
list true false false
sleep 1
list true true false
sleep 3
mark reconnected
mkdir -p "$state_dir"
cp "$headset_dir/airpods-status.json" "$state_dir/status.json"
sleep 4
list true true true
sleep 2.5
$ipc call debug dump 2>/dev/null | "$jq_bin" -c '.headset' > "$headset_airpods_dump"
"$grim_bin" -g "$headset_region" "$headset_airpods_png" > /dev/null 2>&1
mark airpods
hover $headset_park_x $headset_park_y
sleep 1
leave
sleep 2.5
mark left
list true true false
sleep 4
escapes() { "$hyprctl_bin" binds -j | "$jq_bin" '[.[] | select(.key == "Escape" and .modmask == 0)] | length'; }
active() { "$hyprctl_bin" activewindow -j | "$jq_bin" -r '.class'; }
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=headset-typing sh -c 'stty raw -echo; exec dd bs=1 status=none of=$headset_typed']==])" > /dev/null
sleep 2
echo "before active=\$(active) escapes=\$(escapes)" >> "$headset_escape_marks"
# Typed before the card: each wtype run uploads a keymap whose first key
# sits on keycode 9, which a bind table reads as Escape.
"$wtype_bin" ab
list true false false
sleep 4
list true true false
sleep 1.5
# The pointer parked on the card holds it, so only Escape can take it down.
hover $headset_park_x $headset_park_y
sleep 1
echo "up active=\$(active) escapes=\$(escapes)" >> "$headset_escape_marks"
"$grim_bin" -g "$headset_region" "$headset_escape_png" > /dev/null 2>&1
"$wtype_bin" -k Escape
for i in \$(seq 1 30); do [ "\$(unmapped)" -ge 3 ] && break; sleep 0.2; done
sleep 0.5
echo "after active=\$(active) escapes=\$(escapes)" >> "$headset_escape_marks"
mark escaped
$ipc call debug dump 2>/dev/null | "$jq_bin" -c '.headset' > "$headset_dir/escaped-dump.json"
leave
"$wtype_bin" cd
sleep 0.5
if [ -f "$fs_state" ]; then
  "$jq_bin" '.dnd = true' "$fs_state" > "$fs_state.tmp" && mv "$fs_state.tmp" "$fs_state"
else
  echo '{"dnd": true}' > "$fs_state"
fi
sleep 2
list true true true
sleep 2.5
"$grim_bin" -g "$headset_region" "$headset_dnd_png" > /dev/null 2>&1
mark dnd
rm -f "$state_dir/status.json"
"$jq_bin" '.dnd = false' "$fs_state" > "$fs_state.tmp" && mv "$fs_state.tmp" "$fs_state"
list true false false
"$jq_bin" -c '.earbudsFaces' "$fs_state" > "$headset_dir/earbuds-art.json"
settings_retarget '.layout = "sidebar"'
for _ in \$(seq 1 20); do
  $ipc call sidebar status 2>/dev/null | grep -q '"cards"' && break
  sleep 0.5
done
sleep 4
list true false true "Kyan's AirP"
sleep 2.5
$ipc call debug dump 2>/dev/null | "$jq_bin" -c '.headset' > "$headset_sidebar_dump"
"$grim_bin" "$headset_sidebar_png" > /dev/null 2>&1
mark sidebar
$ipc call headset dismiss > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

headset_mark() {
  # headset_mark <name> <field> prints one counter off a checkpoint line.
  awk -v n="$1" -v f="$2=" '$1 == n { for (i = 2; i <= NF; i++) if (index($i, f) == 1) print substr($i, length(f) + 1) }' "$headset_log_marks"
}

headset_expect() {
  # headset_expect <name> <mapped> <unmapped>
  local got_m got_u
  got_m=$(headset_mark "$1" mapped)
  got_u=$(headset_mark "$1" unmapped)
  if [ "$got_m" != "$2" ] || [ "$got_u" != "$3" ]; then
    fail "headset card at '$1': expected $2 mapped and $3 unmapped, the shell log counted ${got_m:-none} and ${got_u:-none}"
  fi
}

headset_escape_expect() {
  local before up after typed
  cat "$headset_escape_marks"
  before=$(awk '$1 == "before"' "$headset_escape_marks")
  up=$(awk '$1 == "up"' "$headset_escape_marks")
  after=$(awk '$1 == "after"' "$headset_escape_marks")
  [ "$before" = "before active=headset-typing escapes=0" ] || fail "headset escape: expected the typing foot focused and no Escape bind before the card, got '$before'"
  [ "$up" = "up active=headset-typing escapes=1" ] || fail "headset escape: expected one Escape bind with the card up and focus left on foot, got '$up'"
  [ "$after" = "after active=headset-typing escapes=0" ] || fail "headset escape: expected the Escape bind gone and focus still on foot, got '$after'"
  typed=$(cat -v "$headset_typed" 2>/dev/null)
  echo "SMOKE_HEADSET_TYPED $typed"
  [ "$typed" = "ab^[cd" ] || fail "headset escape: foot read '$typed', not every key typed around the card ('ab^[cd')"
}

leg_headset_card_assert() {
  local png
  [ -s "$headset_log_marks" ] || fail "no headset card checkpoints were written"
  cat "$headset_log_marks"
  for png in "$headset_bluez_png" "$headset_held_png" "$headset_airpods_png" "$headset_escape_png" "$headset_dnd_png" "$headset_sidebar_png"; do
    [ -s "$png" ] || fail "no screenshot at $png"
  done
  echo "SMOKE_HEADSET_BLUEZ $headset_bluez_png"
  echo "SMOKE_HEADSET_HELD $headset_held_png"
  echo "SMOKE_HEADSET_AIRPODS $headset_airpods_png"
  echo "SMOKE_HEADSET_ESCAPE $headset_escape_png"
  echo "SMOKE_HEADSET_DND $headset_dnd_png"
  echo "SMOKE_HEADSET_SIDEBAR $headset_sidebar_png"

  headset_expect startup 0 0
  headset_expect connected 1 0
  # Parked on, the card outlives its four seconds; the pointer leaving lets it go.
  headset_expect held 1 0
  headset_expect released 1 1
  # Off and on inside a second: no second card.
  headset_expect reconnected 1 1
  headset_expect airpods 2 1
  local glyph pods
  glyph=$("$jq_bin" -r '"\(.rest.w)x\(.rest.h) r\(.radius) art=\(.art)"' "$headset_bluez_dump" 2>/dev/null)
  pods=$("$jq_bin" -r '"\(.rest.w)x\(.rest.h) r\(.radius) art=\(.art)"' "$headset_airpods_dump" 2>/dev/null)
  echo "headset cards: glyph $glyph, airpods $pods"
  "$jq_bin" -e --slurpfile p "$headset_airpods_dump" '.rest.h == $p[0].rest.h and .radius == $p[0].radius' "$headset_bluez_dump" > /dev/null \
    || fail "headset: the two cards differ in height or radius ($glyph, $pods)"
  "$jq_bin" -e --slurpfile p "$headset_airpods_dump" '.rest.w < $p[0].rest.w' "$headset_bluez_dump" > /dev/null \
    || fail "headset: the card for \"Kyan's AirPods Pro\" is no wider than the one for \"Studio Cans\" ($glyph, $pods)"
  "$jq_bin" -e '.radius * 2 - .rest.h | fabs <= 1' "$headset_bluez_dump" > /dev/null || fail "headset: the card is not a capsule ($glyph)"
  [ "${glyph#*art=}" = "" ] && [ -n "${pods#*art=}" ] || fail "headset: the headphone wore '${glyph#*art=}', the AirPods '${pods#*art=}'"
  # The pointer leaving is what dismissed the pair.
  headset_expect left 2 2
  # Escape into the focused foot: the card goes, foot keeps the focus and the key.
  headset_expect escaped 3 3
  headset_escape_expect
  local learning
  learning=$("$jq_bin" -r '.learning' "$headset_dir/escaped-dump.json" 2>/dev/null)
  echo "headset learning after escape: $learning"
  [ "$learning" = "$headset_cans_address" ] || fail "headset: the earbuds service was not held to learn Studio Cans' picture past its card ($learning)"
  headset_expect dnd 3 3
  headset_expect sidebar 4 3
  local x y w h off
  read -r x y w h <<< "$("$jq_bin" -r '.rest | "\(.x) \(.y) \(.w) \(.h)"' "$headset_sidebar_dump" 2>/dev/null)"
  echo "headset sidebar card: x=$x y=$y w=$w h=$h on a $logical_w wide output"
  [ -n "$w" ] && [ "$w" != null ] || fail "headset sidebar: debug dump carried no card ($(cat "$headset_sidebar_dump"))"
  [ "${w%.*}" -gt "${h%.*}" ] || fail "headset sidebar: the card is ${w}x${h}, on its side"
  off=$(( ${x%.*} + ${w%.*} / 2 - logical_w / 2 ))
  [ "${off#-}" -le 2 ] || fail "headset sidebar: the card's middle is ${off}px off the output's"
  [ "${y%.*}" -le 40 ] || fail "headset sidebar: the card rests at y=$y, not off the top edge"
  local kept art kept_name name
  kept=$("$jq_bin" -r --arg a "$headset_pods_address" '.[$a].art // empty' "$headset_dir/earbuds-art.json" 2>/dev/null)
  kept_name=$("$jq_bin" -r --arg a "$headset_pods_address" '.[$a].name // empty' "$headset_dir/earbuds-art.json" 2>/dev/null)
  art=$("$jq_bin" -r '.art' "$headset_sidebar_dump")
  name=$("$jq_bin" -r '.name' "$headset_sidebar_dump")
  echo "headset face: state.json kept '$kept' '$kept_name' for the AirPods, the reconnect wore '$art' '$name'"
  [ -n "$kept" ] && [ -n "$kept_name" ] || fail "headset: state.json's earbudsFaces kept no face for the AirPods ($(cat "$headset_dir/earbuds-art.json"))"
  [ "$art" = "$kept" ] || fail "headset: the AirPods reconnecting with no backend wore '$art', not the kept '$kept'"
  [ "$name" = "$kept_name" ] || fail "headset: the AirPods reconnecting under BlueZ's cut name read '$name', not the kept '$kept_name'"
  echo "SMOKE_HEADSET_CARD ok startup 0, connect 1, held, leave, reconnect none, airpods 1, leave, escape, dnd none, sidebar ${w}x${h} at ${x},${y}"
}
