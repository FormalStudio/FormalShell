# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --settings-ui: the settings pages (docs/superpowers/specs/2026-10-31-settings-ui.md)
# on real keys. The fixture's settings.json pins `motion.morph` false. From
# the Settings panel's tiles, Down and Return open Appearance; Down, Down
# and Return flip Reactive rims off, read back as `settings.theme.reactiveRims`
# false in state.json, false in `debug dump`'s `configLoaded`, and an
# override in `settings get`. Three Downs reach the pinned morph row:
# Return and Right leave it false with no override written, and `settings
# set` and `settings reset` on it answer the pinned error. Three Ups and
# `x` reset Reactive rims: the override gone from state.json and from the
# resolved document. Backspace goes back to the tiles, and Down, Down,
# Return, three Downs and Right set the Bar page's style to solid, read
# off `configLoaded` and `bar paint`'s style, then reset over IPC.
# settings.json's sha256 is the same at the end. Every page photographed.
leg_settings_ui_flag="--settings-ui"
leg_settings_ui_order=260
leg_settings_ui_needs="jq wtype"

settings_ui_settings="$iso_home/.config/formalshell/settings.json"
settings_ui_state="$iso_home/.local/state/formalshell/state.json"

leg_settings_ui_fixture() {
  settings_fragment ', "motion": {"morph": false}'
}

leg_settings_ui_timing() {
  leg_timing 26 80
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
}
sleep 3
sha256sum "$settings_ui_settings" | cut -d' ' -f1 > "\$d/sha-before"
$ipc call panel open settings > /dev/null 2>&1
sleep 1.5
"$grim_bin" "\$d/settings-ui-grid.png" > /dev/null 2>&1
key Down Return
read_back appearance
"$grim_bin" "\$d/settings-ui-appearance.png" > /dev/null 2>&1
key Down Down Return
read_back flipped
$ipc call settings get theme.reactiveRims > "\$d/get-flipped.json" 2>&1
"$grim_bin" "\$d/settings-ui-appearance-flipped.png" > /dev/null 2>&1
key Down Down Down Return Right
read_back pinned
$ipc call settings get motion.morph > "\$d/get-pinned.json" 2>&1
$ipc call settings set motion.morph true > "\$d/set-pinned.txt" 2>&1
$ipc call settings reset motion.morph > "\$d/reset-pinned.txt" 2>&1
"$grim_bin" "\$d/settings-ui-appearance-pinned.png" > /dev/null 2>&1
key Up Up Up x
read_back reset
$ipc call settings get theme.reactiveRims > "\$d/get-reset.json" 2>&1
key BackSpace
read_back back
key Down Down Return
read_back bar
key Down Down Down Right
read_back solid
$ipc call bar paint > "\$d/paint-solid.json" 2>&1
"$grim_bin" "\$d/settings-ui-bar.png" > /dev/null 2>&1
$ipc call settings reset bar.style > "\$d/reset-style.txt" 2>&1
sleep 1
read_back style-reset
for page in desktop system; do
  $ipc call panel open "settings:\$page" > /dev/null 2>&1
  sleep 1.5
  "$grim_bin" "\$d/settings-ui-\$page.png" > /dev/null 2>&1
done
$ipc call panel close > /dev/null 2>&1
sleep 0.5
sha256sum "$settings_ui_settings" | cut -d' ' -f1 > "\$d/sha-after"
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
  settings_ui_text "Down, Return on the tiles opened Appearance" panel-appearance.txt "settings:appearance"
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
  settings_ui_text "Backspace went back to the tiles" panel-back.txt "settings"
  settings_ui_text "Down, Down, Return opened Bar" panel-bar.txt "settings:bar"
  settings_ui_jq "Right on Bar style wrote solid" state-solid.json '.settings.bar.style == "solid"'
  settings_ui_jq "the resolved config is solid" dump-solid.json '.configLoaded.bar.style == "solid"'
  settings_ui_jq "the bar repainted solid" paint-solid.json '.[0].paint.style == "solid"'
  settings_ui_text "settings reset bar.style" reset-style.txt "ok"
  settings_ui_jq "the style override is gone" state-style-reset.json '.settings == {}'
  local before after
  before=$(cat "$shot_dir/settings-ui/sha-before" 2>/dev/null)
  after=$(cat "$shot_dir/settings-ui/sha-after" 2>/dev/null)
  [ -n "$before" ] && [ "$before" = "$after" ] || fail "settings-ui: settings.json changed ($before -> $after)"
  echo "SMOKE_SETTINGS_UI settings.json unchanged $after"
  local shot
  for shot in grid appearance appearance-flipped appearance-pinned bar desktop system; do
    [ -s "$shot_dir/settings-ui/settings-ui-$shot.png" ] || fail "settings-ui: no settings-ui-$shot.png"
    echo "SMOKE_SETTINGS_UI_SHOT $shot_dir/settings-ui/settings-ui-$shot.png"
  done
}
