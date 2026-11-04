# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --settings-ui: the settings pages (docs/superpowers/specs/2026-10-31-settings-ui.md)
# on real keys. The fixture's settings.json pins `motion.morph` false. The
# Settings panel's root walked on Downs reads four tiles (Appearance,
# Bluetooth, System, Information) then the four session buttons. Down and
# Return open Appearance; Down, Down and Return flip Reactive rims off,
# read back as `settings.theme.reactiveRims` false in state.json, false in
# `debug dump`'s `configLoaded`, and an override in `settings get`. Three
# Downs reach the pinned morph row: Return and Right leave it false with
# no override written, and `settings set` and `settings reset` on it
# answer the pinned error. Three Ups and `x` reset Reactive rims: the
# override gone from state.json and from the resolved document. Downs to
# Appearance's Bar row and Return open Bar, three Downs and Right set its
# style to solid, read off `configLoaded` and `bar paint`'s style, then
# reset over IPC; Backspace goes back to Appearance. Downs to its last
# row, Wallpaper, and Return open the picker in the card (`menu status`
# level `wallpaper`, view `picker`), and Backspace there lands back on
# Appearance; Backspace again goes to the tiles. Desktop runs past the card, which rests on a whole row: fourteen
# Downs and a Left reach Bottom right below it and set it to screensaver,
# and a real click on its token climbs to Appearance. settings.json's sha256 is the same at the end. Every page photographed.
# The panels live in the launcher card, centred (Pond's settings window):
# `menu status` reads open, `view` panel and the page's name at each step,
# `panel open info` lands there too, Escape on a page goes back to the
# tiles and Escape on the tiles leaves the route for the root, where
# typing "settings" and Return opens the tiles again. Last, Restart takes
# one Return without closing the card (armed) and the lock button locks
# the session, which the real password unlocks.
leg_settings_ui_flag="--settings-ui"
leg_settings_ui_order=260
leg_settings_ui_needs="jq wtype wlrctl python3"

settings_ui_settings="$iso_home/.config/formalshell/settings.json"
settings_ui_state="$iso_home/.local/state/formalshell/state.json"

leg_settings_ui_fixture() {
  settings_fragment ', "motion": {"morph": false}'
}

leg_settings_ui_timing() {
  leg_timing 80 140
}

leg_settings_ui_drive() {
  local script="$shot_dir/settings-ui-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
d="$shot_dir/settings-ui"
mkdir -p "\$d"
key() {
  local k
  for k in "\$@"; do
    "$wtype_bin" -k "\$k"
    sleep 0.25
  done
  sleep 0.8
}
read_back() {
  cp "$settings_ui_state" "\$d/state-\$1.json" 2>/dev/null
  $ipc call debug dump > "\$d/dump-\$1.json" 2>&1
  $ipc call panel state > "\$d/panel-\$1.txt" 2>&1
  $ipc call menu status > "\$d/menu-\$1.json" 2>&1
}
# Downs (or \$3, Up) until the panel cursor is on stop \$1, each stop
# passed written to \$2 (at most 16 keys).
walk_to() {
  local n=0 at
  : > "\$d/\$2"
  while [ \$n -lt 16 ]; do
    at=\$($ipc call menu status 2>/dev/null | "$jq_bin" -r '.panelCursor // ""')
    echo "\$at" >> "\$d/\$2"
    [ "\$at" = "\$1" ] && return 0
    "$wtype_bin" -k "\${3:-Down}"
    sleep 0.3
    n=\$((n + 1))
  done
}
shot() {
  "$grim_bin" "\$d/settings-ui-\$1.png" > /dev/null 2>&1
  $ipc call menu status > "\$d/box-\$1.json" 2>&1
}
sleep 3
sha256sum "$settings_ui_settings" | cut -d' ' -f1 > "\$d/sha-before"
$ipc call panel open settings > /dev/null 2>&1
sleep 1.5
shot grid
key Down Return
read_back appearance
shot appearance
key Down Down Return
read_back flipped
$ipc call settings get theme.reactiveRims > "\$d/get-flipped.json" 2>&1
shot appearance-flipped
key Down Down Down Return Right
read_back pinned
$ipc call settings get motion.morph > "\$d/get-pinned.json" 2>&1
$ipc call settings set motion.morph true > "\$d/set-pinned.txt" 2>&1
$ipc call settings reset motion.morph > "\$d/reset-pinned.txt" 2>&1
shot appearance-pinned
key Up Up Up x
read_back reset
$ipc call settings get theme.reactiveRims > "\$d/get-reset.json" 2>&1
walk_to page:settings:bar walk-appearance.txt
key Return
read_back bar
key Down Down Down Right
read_back solid
$ipc call bar paint > "\$d/paint-solid.json" 2>&1
shot bar
$ipc call settings reset bar.style > "\$d/reset-style.txt" 2>&1
sleep 1
read_back style-reset
key BackSpace
read_back back-sub
walk_to route:wallpaper walk-wallpaper.txt
key Return
sleep 1
read_back wallpaper
"$grim_bin" "\$d/settings-ui-wallpaper.png" > /dev/null 2>&1
key BackSpace
read_back wallpaper-back
key BackSpace
read_back back
walk_to session:lock walk-tiles.txt
for page in desktop system; do
  $ipc call panel open "settings:\$page" > /dev/null 2>&1
  sleep 1.5
  shot \$page
  if [ "\$page" = desktop ]; then
    key Down Down Down Down Down Down Down Down Down Down Down Down Down Down Left
    $ipc call settings get hotCorners.bottomRight > "\$d/get-below.json" 2>&1
    $ipc call settings reset hotCorners.bottomRight > /dev/null 2>&1
    # A real click on the token climbs one page, as Backspace does.
    $ipc call menu status > "\$d/menu-token-before.json" 2>&1
    read -r tx ty tw th < <("$jq_bin" -r '[.tokenBox.x, .tokenBox.y, .tokenBox.width, .tokenBox.height] | @tsv' "\$d/menu-token-before.json")
    "$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1
    "$wlrctl_bin" pointer move \$((tx + tw / 2)) \$((ty + th / 2)) > /dev/null 2>&1
    sleep 0.3
    "$wlrctl_bin" pointer click left > /dev/null 2>&1
    sleep 0.8
    read_back token
    "$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1
  fi
done
key Escape
read_back escape-page
key Escape
read_back escape-tiles
for c in s e t t i n g s; do "$wtype_bin" "\$c"; sleep 0.05; done
sleep 0.6
key Return
read_back root
$ipc call panel open info > /dev/null 2>&1
sleep 1.5
read_back info
shot info
$ipc call panel close > /dev/null 2>&1
sleep 0.5
read_back closed
sleep 0.5
sha256sum "$settings_ui_settings" | cut -d' ' -f1 > "\$d/sha-after"
$ipc call panel open settings > /dev/null 2>&1
sleep 1.5
walk_to session:reboot walk-reboot.txt
key Return
read_back armed
walk_to session:lock walk-lock.txt Up
read_back on-lock
key Return
sleep 2
$ipc call lock status > "\$d/lock-locked.json" 2>&1
"$wtype_bin" -s 300 "formalshell-test"
sleep 0.5
"$wtype_bin" -s 300 -k Return
sleep 4
$ipc call lock status > "\$d/lock-unlocked.json" 2>&1
EOF
  hypr_exec_once "bash $script"
}

settings_ui_jq() {
  local what="$1" file="$shot_dir/settings-ui/$2" expr="$3"
  [ -s "$file" ] || fail "settings-ui: no $2 for $what"
  "$jq_bin" -e "$expr" "$file" > /dev/null 2>&1 || fail "settings-ui: $what: $(head -c 600 "$file")"
  echo "SMOKE_SETTINGS_UI $what"
}

settings_ui_text() {
  local what="$1" file="$shot_dir/settings-ui/$2" want="$3"
  [ "$(cat "$file" 2>/dev/null)" = "$want" ] || fail "settings-ui: $what: expected '$want', got '$(cat "$file" 2>/dev/null)'"
  echo "SMOKE_SETTINGS_UI $what"
}

leg_settings_ui_assert() {
  settings_ui_jq "panel open settings opened the launcher on the tiles" menu-appearance.json '.isOpen == true and .level == "settings" and .view == "panel"'
  settings_ui_text "Down, Return on the tiles opened Appearance" panel-appearance.txt "settings:appearance"
  settings_ui_jq "the launcher holds Appearance" menu-appearance.json '.panel == "settings:appearance"' 
  settings_ui_jq "Return on Reactive rims wrote the override" state-flipped.json '.settings.theme.reactiveRims == false'
  settings_ui_jq "the resolved config carries it" dump-flipped.json '.configLoaded.theme.reactiveRims == false'
  settings_ui_jq "settings get reads it back" get-flipped.json '.override == false and .value == false and .pinned == false'
  settings_ui_jq "the pinned morph row took no edit" state-pinned.json '(.settings.motion // {}) | has("morph") | not'
  settings_ui_jq "morph still settings.json's" dump-pinned.json '.configLoaded.motion.morph == false and .theme.morph == false'
  settings_ui_jq "settings get calls morph pinned" get-pinned.json '.pinned == true and .file == false and .override == null'
  settings_ui_text "settings set refuses the pinned key" set-pinned.txt "error: motion.morph is set in settings.json"
  settings_ui_text "settings reset refuses the pinned key" reset-pinned.txt "error: motion.morph is set in settings.json"
  settings_ui_jq "x reset the override" state-reset.json '(.settings.theme // {}) | has("reactiveRims") | not'
  settings_ui_jq "the resolved config dropped it" dump-reset.json '(.configLoaded.theme // {}) | has("reactiveRims") | not'
  settings_ui_jq "settings get after the reset" get-reset.json '.override == null and .shown == true'
  tail -n1 "$shot_dir/settings-ui/walk-appearance.txt" > "$shot_dir/settings-ui/walk-appearance-end.txt" 2>/dev/null
  settings_ui_text "Downs reached Appearance's Bar row" walk-appearance-end.txt "page:settings:bar"
  settings_ui_text "Return on it opened Bar" panel-bar.txt "settings:bar"
  settings_ui_jq "Right on Bar style wrote solid" state-solid.json '.settings.bar.style == "solid"'
  settings_ui_jq "the resolved config is solid" dump-solid.json '.configLoaded.bar.style == "solid"'
  settings_ui_jq "the bar repainted solid" paint-solid.json '.[0].paint.style == "solid"'
  settings_ui_text "settings reset bar.style" reset-style.txt "ok"
  settings_ui_text "Backspace on Bar went back to Appearance" panel-back-sub.txt "settings:appearance"
  tail -n1 "$shot_dir/settings-ui/walk-wallpaper.txt" > "$shot_dir/settings-ui/walk-wallpaper-end.txt" 2>/dev/null
  settings_ui_text "Downs reached Appearance's Wallpaper row" walk-wallpaper-end.txt "route:wallpaper"
  settings_ui_jq "Return on it opened the picker in the card" menu-wallpaper.json '.isOpen == true and .level == "wallpaper" and .view == "picker"'
  settings_ui_jq "Backspace in the picker went back to Appearance" menu-wallpaper-back.json '.isOpen == true and .panel == "settings:appearance"'
  [ -s "$shot_dir/settings-ui/settings-ui-wallpaper.png" ] || fail "settings-ui: no settings-ui-wallpaper.png"
  echo "SMOKE_SETTINGS_UI_SHOT $shot_dir/settings-ui/settings-ui-wallpaper.png"
  settings_ui_text "Backspace on Appearance went back to the tiles" panel-back.txt "settings"
  local tiles
  tiles=$(grep -E '^(tile|session):' "$shot_dir/settings-ui/walk-tiles.txt" 2>/dev/null | uniq | tr '\n' ' ')
  [ "$tiles" = "tile:appearance tile:bluetooth tile:system tile:info session:lock " ] \
    || fail "settings-ui: the root walked '$tiles', not four tiles then the session column"
  echo "SMOKE_SETTINGS_UI the root holds four tiles then the session column"
  # The Desktop page runs past the card, which rests on a whole row: the
  # keys still walk onto its last control, below the viewport.
  settings_ui_jq "Downs reach Desktop's last control below the card" get-below.json '.override == "screensaver"'
  settings_ui_jq "a click on Desktop's token went back to Appearance" menu-token.json '.isOpen == true and .panel == "settings:appearance"'
  settings_ui_jq "Escape on a page went back to the tiles" menu-escape-page.json '.isOpen == true and .panel == "settings"'
  settings_ui_jq "Escape on the tiles left the route" menu-escape-tiles.json '.isOpen == true and .level == null and .view != "panel"'
  settings_ui_jq "the root's Settings row opened the tiles" menu-root.json '.level == "settings" and .panel == "settings"'
  settings_ui_jq "panel open info landed in the launcher" menu-info.json '.isOpen == true and .level == "info" and .panel == "info"'
  settings_ui_text "panel state names info" panel-info.txt "info"
  settings_ui_jq "panel close closed the launcher" menu-closed.json '.isOpen == false' 
  settings_ui_jq "the style override is gone" state-style-reset.json '.settings == {}'
  settings_ui_jq "one Return on Restart armed it and kept the card" menu-armed.json '.isOpen == true and .panelCursor == "session:reboot"'
  settings_ui_jq "the cursor reached the lock button" menu-on-lock.json '.panelCursor == "session:lock"'
  settings_ui_jq "the lock button locked the session" lock-locked.json '.locked == true'
  settings_ui_jq "the real password unlocked it" lock-unlocked.json '.locked == false'
  local before after
  before=$(cat "$shot_dir/settings-ui/sha-before" 2>/dev/null)
  after=$(cat "$shot_dir/settings-ui/sha-after" 2>/dev/null)
  [ -n "$before" ] && [ "$before" = "$after" ] || fail "settings-ui: settings.json changed ($before -> $after)"
  echo "SMOKE_SETTINGS_UI settings.json unchanged $after"
  local shot
  for shot in grid appearance appearance-flipped appearance-pinned bar desktop system info; do
    [ -s "$shot_dir/settings-ui/settings-ui-$shot.png" ] || fail "settings-ui: no settings-ui-$shot.png"
    echo "SMOKE_SETTINGS_UI_SHOT $shot_dir/settings-ui/settings-ui-$shot.png"
    panel_language_assert "settings-ui $shot" "$shot_dir/settings-ui/settings-ui-$shot.png" \
      "$("$jq_bin" -c '.cardBox' "$shot_dir/settings-ui/box-$shot.json" 2>/dev/null)" yes \
      "$shot_dir/settings-ui/dump-appearance.json" inset
  done
  panel_language_verdict
}
