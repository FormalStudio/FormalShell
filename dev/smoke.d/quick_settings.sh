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
leg_quick_settings_flag="--quick-settings"
leg_quick_settings_order=262
leg_quick_settings_needs="jq wtype grim"

quick_settings_dir="$shot_dir/quick-settings"
quick_settings_settings="$iso_home/.config/formalshell/settings.json"

leg_quick_settings_timing() {
  leg_timing 60 120
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
# Arrows toward the control \$1 off the boxes \`status\` reports, the cursor
# on each step written to \$2. A hidden cursor takes a first key to show.
walk_to() {
  local n=0 at key
  : > "\$d/\$2"
  if [ "\$($ipc call quicksettings status 2>/dev/null | "$jq_bin" -r '.cursor // ""')" = "" ]; then
    "$wtype_bin" -k Down
    sleep 0.3
  fi
  while [ \$n -lt 12 ]; do
    $ipc call quicksettings status > "\$d/walk.json" 2>/dev/null
    at=\$("$jq_bin" -r '.cursor // ""' "\$d/walk.json")
    echo "\$at" >> "\$d/\$2"
    [ "\$at" = "\$1" ] && return 0
    key=\$("$jq_bin" -r --arg at "\$at" --arg to "\$1" '
      (.rows[] | select(.id == \$at) | .rect) as \$a
      | (.rows[] | select(.id == \$to) | .rect) as \$b
      | if \$b.y >= \$a.y + \$a.height then "Down"
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
sha256sum "$quick_settings_settings" | cut -d' ' -f1 > "\$d/sha-after"
EOF
  hypr_exec_once "bash $script"
}

quick_settings_jq() {
  local what="$1" file="$quick_settings_dir/$2" expr="$3"
  [ -s "$file" ] || fail "quick-settings: no $2 for $what"
  "$jq_bin" -e "$expr" "$file" > /dev/null 2>&1 || fail "quick-settings: $what: $(head -c 1200 "$file")"
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
    ["battery","screenshot","settings","lock","wifi","bluetooth","power-mode","night-light","dark-style","dnd","keyboard","caffeinate","overnight","hdr"] as $order
    | [.rows[].id] as $ids
    | ($ids | map(. as $i | $order | index($i))) as $at
    | ($at | all(. != null)) and $at == ($at | sort)
      and (["screenshot","settings","lock","wifi","bluetooth","power-mode","night-light","dark-style","dnd","caffeinate","overnight"] - $ids) == []'
  quick_settings_jq "the system row then the toggles" "$open" '
    [.rows[].kind] as $k | ($k | all(. == "system" or . == "toggle")) and $k == ($k | sort_by(if . == "system" then 0 else 1 end))'
  # Every edge off the card's own rect, the toggles off each other.
  quick_settings_jq "the card is 400 wide, the toggles 178 by 48" "$open" '
    .card.width == 400 and ([.rows[] | select(.kind == "toggle") | .rect | .width == 178 and .height == 48] | all)'
  quick_settings_jq "12 between columns and rows, 16 from every edge" "$open" '
    def near($a; $b): ($a - $b) | fabs <= 1;
    .card as $c
    | [.rows[] | select(.kind == "toggle") | .rect] as $t
    | [.rows[] | select(.kind == "system") | .rect] as $s
    | ($t | map(.x) | min) as $left
    | ($t | map(.x + .width) | max) as $right
    | near($left - $c.x; 16) and near($c.x + $c.width - $right; 16)
      and near(($s | map(.y) | min) - $c.y; 16)
      and near($c.y + $c.height - ($t | map(.y + .height) | max); 16)
      and near(($s | map(.x + .width) | max); $right)
      and near(($t[0].y) - ($s | map(.y + .height) | max); 12)
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
  for shot in open on; do
    [ -s "$quick_settings_dir/quick-settings-$shot.png" ] || fail "quick-settings: no quick-settings-$shot.png"
    echo "SMOKE_QUICK_SETTINGS_SHOT $quick_settings_dir/quick-settings-$shot.png"
  done
}
