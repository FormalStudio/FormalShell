# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, iso_home, the *_bin paths and fail()
# --launcher-modes: a route standing in the field as a token, read off
# `menu status`'s `level` and `token`. Every route of the launcher spec's §4
# table this rig lists is entered three ways: its name filtered at the root
# and Enter on its row (`menu activate`), `menu summon <id>`, and for the
# five with a `prefix` (`:e`, `:k`, `:r`, `:f`, and the fixture's `:m`
# from menu.jsonc) the prefix and a space typed on real keys, what follows
# staying as the query; `:e ` typed inside a folder (Files, a staged
# Documents named in user-dirs.dirs, stepped into) enters Emoji from there
# too. Share, Clipssh and Install are left out: the first
# two are gated on tools the rig lacks, the last does not exist yet.
#
# The fixture menu.jsonc nests three modes (Fixture › Outer › Inner, a
# Leaf action inside): `le` inside Fixture lists the Leaf alone where the
# root lists more, and Clipboard and Emoji searching for the staged app's
# name list none of it. Backspace on an empty field leaves one level, held
# for 1500ms still one; ← on an empty field does the same. Escape steps
# back one thing at a time: the query, then each mode, then the launcher.
# A real click on the token (its box off `menu status`'s `tokenBox`) leaves
# the mode. The token is photographed in dark then light: Inner's
# truncated path, Emoji's token over its grid and Fixture's scoped list.
leg_launcher_modes_flag="--launcher-modes"
leg_launcher_modes_order=29
leg_launcher_modes_needs="jq wtype wlrctl"

launcher_modes_app="apps.formalshell-modes-probe"

# id, then the label the token ends on.
launcher_modes_routes="apps|Apps
clipboard|Clipboard
calc|Calculator
emoji|Emoji
wallpaper|Wallpaper
monitor|System Monitor
mirror|Mirror
keybinds|Keybinds
wifi|Wi-Fi
bluetooth|Bluetooth Devices
audio|Audio Devices
radio|Radio Stations
panels|Panels
tray|Tray
gpu|GPU
system|System
reminder|Reminder
toggles|Toggles
files|Files
modesfix|Fixture"

leg_launcher_modes_fixture() {
  mkdir -p "$iso_home/.local/share/applications" "$iso_home/.config/formalshell"
  printf '[Desktop Entry]\nType=Application\nName=Modes Probe\nExec=true\n' > "$iso_home/.local/share/applications/formalshell-modes-probe.desktop"
  mkdir -p "$iso_home/Documents"
  # A leg riding along that names its own user directories keeps them.
  [ -e "$iso_home/.config/user-dirs.dirs" ] || printf 'XDG_DOCUMENTS_DIR="$HOME/Documents"\n' > "$iso_home/.config/user-dirs.dirs"
  cat > "$iso_home/.config/formalshell/menu.jsonc" <<'EOF'
{
  "modesfix": { "label": "Fixture", "prefix": ":m" },
  "modesfix.outer": { "label": "Outer" },
  "modesfix.outer.inner": { "label": "Inner" },
  "modesfix.outer.inner.leaf": { "label": "Leaf", "action": "true" }
}
EOF
}

leg_launcher_modes_timing() {
  leg_timing 84 134
}

leg_launcher_modes_drive() {
  local script="$shot_dir/launcher-modes-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
call() { $ipc call "\$@"; }
status() { call menu status > "$shot_dir/launcher-modes-\$1.json" 2>&1; }
root() { call menu summon "" > /dev/null 2>&1; sleep 0.3; }
key() { "$wtype_bin" "\$@"; sleep 0.6; }
shoot() { "$grim_bin" "$shot_dir/launcher-modes-\$1.png" > /dev/null 2>&1; }
sleep 4
while IFS='|' read -r id label; do
  root
  call menu filter "\$label" > /dev/null 2>&1
  status "find-\$id"
  i=\$("$jq_bin" --arg id "\$id" '.ids | index(\$id) // -1' "$shot_dir/launcher-modes-find-\$id.json")
  call menu activate "\$i" > /dev/null 2>&1
  sleep 0.3
  status "name-\$id"
  call menu summon "\$id" > /dev/null 2>&1
  sleep 0.3
  status "summon-\$id"
done <<'ROUTES'
$launcher_modes_routes
radio.search|Search
ROUTES

for p in "e|emoji" "k|keybinds" "r|radio.search" "f|files" "m|modesfix"; do
  root
  key -d 30 ":\${p%%|*} "
  status "prefix-\${p#*|}"
done
call menu summon ":k SUPER" > /dev/null 2>&1
sleep 0.3
status summon-prefixed
call menu summon files > /dev/null 2>&1
sleep 0.5
call menu filter Documents > /dev/null 2>&1
sleep 0.5
call menu into 0 > /dev/null 2>&1
sleep 0.6
status folder
key -d 30 ":e "
status prefix-in-folder

root
call menu filter "Modes Probe" > /dev/null 2>&1
status scope-root
call menu summon clipboard > /dev/null 2>&1
call menu filter "Modes Probe" > /dev/null 2>&1
status scope-clipboard
root
key -d 30 ":e modes"
status scope-emoji
shoot emoji-dark
root
call menu filter le > /dev/null 2>&1
status scope-root-le
root
key -d 30 ":m le"
status scope-fixture
shoot fixture-dark

call menu summon modesfix.outer.inner > /dev/null 2>&1
sleep 1
status inner
shoot inner-dark
key -k BackSpace
status backspace
call menu summon modesfix.outer.inner > /dev/null 2>&1
sleep 0.5
"$wtype_bin" -P BackSpace -s 1500 -p BackSpace
sleep 0.6
status backspace-held
call menu summon modesfix.outer.inner > /dev/null 2>&1
sleep 0.5
key -k Left
status left

call menu summon modesfix.outer.inner > /dev/null 2>&1
sleep 0.5
key le
status escape-0
for n in 1 2 3 4 5; do
  key -k Escape
  status "escape-\$n"
done

"$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1
call menu summon modesfix.outer > /dev/null 2>&1
sleep 1
status click-before
read -r tx ty tw th < <("$jq_bin" -r '[.tokenBox.x, .tokenBox.y, .tokenBox.width, .tokenBox.height] | @tsv' "$shot_dir/launcher-modes-click-before.json")
"$wlrctl_bin" pointer move \$((tx + tw / 2)) \$((ty + th / 2)) > /dev/null 2>&1
sleep 0.3
"$wlrctl_bin" pointer click left > /dev/null 2>&1
sleep 0.6
status click-after
"$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1

call theme mode light > /dev/null 2>&1
sleep 2
call theme status > "$shot_dir/launcher-modes-theme.json" 2>&1
call menu summon modesfix.outer.inner > /dev/null 2>&1
sleep 1
shoot inner-light
root
key -d 30 ":e modes"
sleep 0.5
shoot emoji-light
root
key -d 30 ":m le"
sleep 0.5
shoot fixture-light
call menu close > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

# `<step> <jq claim> <what it means>`, against that step's status.
launcher_modes_expect() {
  local f="$shot_dir/launcher-modes-$1.json"
  [ -s "$f" ] || fail "launcher-modes: no status for $1"
  "$jq_bin" -e "$2" "$f" > /dev/null || fail "launcher-modes $1: $3, got $("$jq_bin" -c '{isOpen, level, token, query, ids: .ids[:6]}' "$f")"
}

# Every status read with a token drawn: its box's vertical centre on the
# field band's within 1px, whatever stands beside it. The boxes are the last
# frame's, so a read taken before a new view's first frame has no token box
# yet and is not counted.
launcher_modes_token_centred() {
  local f off read=0
  for f in "$@"; do
    off=$("$jq_bin" -r 'select(.isOpen and .tokenBox) | if .fieldBox then (.tokenBox.y + .tokenBox.height / 2) - (.fieldBox.y + .fieldBox.height / 2) else "missing" end' "$f" 2>/dev/null)
    [ -n "$off" ] || continue
    read=$((read + 1))
    [ "$off" != missing ] || fail "$(basename "$f") drew a token with no field band"
    awk -v o="$off" 'BEGIN { exit !(o <= 1 && o >= -1) }' \
      || fail "launcher-modes: the token sits ${off}px off the field band's centre in $(basename "$f")"
  done
  [ "$read" -gt 0 ] || fail "launcher-modes: no status read carried a token and the field band"
  echo "SMOKE_LAUNCHER_MODES the token centred in the field band in $read status reads"
}

leg_launcher_modes_assert() {
  launcher_modes_token_centred "$shot_dir"/launcher-modes-*.json
  local id label n=0
  while IFS='|' read -r id label; do
    [ "$id" = radio.search ] || launcher_modes_expect "find-$id" "(.ids | index(\"$id\")) != null" "the root search for '$label' did not list $id"
    for how in name summon; do
      [ "$id" = radio.search ] && [ "$how" = name ] && continue
      launcher_modes_expect "$how-$id" ".level == \"$id\" and .token[-1] == \"$label\" and .query == \"\"" "entering by $how did not stand in $id as [$label]"
    done
    n=$((n + 1))
  done <<ROUTES
$launcher_modes_routes
radio.search|Search
ROUTES
  echo "SMOKE_LAUNCHER_MODES $n routes entered by name and Enter and by menu summon"
  launcher_modes_expect summon-radio.search '.token == ["Radio Stations", "Search"]' "radio.search's token is not its path"

  launcher_modes_expect prefix-emoji '.level == "emoji" and .token == ["Emoji"] and .query == "" and .view == "emoji"' "':e ' did not become the Emoji token"
  launcher_modes_expect prefix-keybinds '.level == "keybinds" and .token == ["Keybinds"] and .query == ""' "':k ' did not become the Keybinds token"
  launcher_modes_expect prefix-radio.search '.level == "radio.search" and .token == ["Radio Stations", "Search"]' "':r ' did not become Radio Stations › Search"
  launcher_modes_expect prefix-files '.level == "files" and .token == ["Files"] and .query == ""' "':f ' did not become the Files token"
  launcher_modes_expect prefix-modesfix '.level == "modesfix" and .token == ["Fixture"]' "menu.jsonc's ':m' prefix did not enter its route"
  launcher_modes_expect summon-prefixed '.level == "keybinds" and .query == "SUPER"' "'menu summon \":k SUPER\"' did not keep SUPER as the query in Keybinds"
  launcher_modes_expect folder '.level == "files" and .token == ["Files", "Documents"]' "Documents was not stepped into"
  launcher_modes_expect prefix-in-folder '.level == "emoji" and .token == ["Emoji"] and .query == "" and .view == "emoji"' "':e ' inside a folder did not become the Emoji token"
  echo "SMOKE_LAUNCHER_MODES prefixes :e :k :r and menu.jsonc's :m typed on real keys"

  launcher_modes_expect scope-root "(.ids | index(\"$launcher_modes_app\")) != null" "the staged app is not found from the root"
  launcher_modes_expect scope-clipboard ".level == \"clipboard\" and (.ids | index(\"$launcher_modes_app\")) == null" "Clipboard's search listed the staged app"
  launcher_modes_expect scope-emoji ".level == \"emoji\" and .query == \"modes\" and (.ids | index(\"$launcher_modes_app\")) == null" "Emoji's search listed the staged app"
  launcher_modes_expect scope-root-le '(.ids | length) > 1' "the root query 'le' found one row or none, so the scope proves nothing"
  launcher_modes_expect scope-fixture '.level == "modesfix" and .query == "le" and .ids == ["modesfix.outer.inner.leaf"]' "a query inside Fixture ranked outside it"
  echo "SMOKE_LAUNCHER_MODES a query inside a mode ranks only its own subjects"

  launcher_modes_expect inner '.token == ["Fixture", "Outer", "Inner"]' "the nested token is not the whole path"
  launcher_modes_expect backspace '.isOpen and .level == "modesfix.outer"' "Backspace on an empty field did not leave one level"
  launcher_modes_expect backspace-held '.isOpen and .level == "modesfix.outer"' "a Backspace held for 1500ms did not stop one level up"
  launcher_modes_expect left '.isOpen and .level == "modesfix.outer"' "Left on an empty field did not leave one level"
  echo "SMOKE_LAUNCHER_MODES Backspace, held Backspace and Left each left one level"

  launcher_modes_expect escape-0 '.level == "modesfix.outer.inner" and .query == "le"' "the typed query did not land"
  launcher_modes_expect escape-1 '.level == "modesfix.outer.inner" and .query == ""' "the first Escape did not clear the query alone"
  launcher_modes_expect escape-2 '.level == "modesfix.outer"' "the second Escape did not leave Inner"
  launcher_modes_expect escape-3 '.level == "modesfix"' "the third Escape did not leave Outer"
  launcher_modes_expect escape-4 '.isOpen and .level == null and .token == []' "the fourth Escape did not reach the root"
  launcher_modes_expect escape-5 '.isOpen == false' "the fifth Escape did not close the launcher"
  echo "SMOKE_LAUNCHER_MODES Escape: query, Inner, Outer, Fixture, then the launcher"

  launcher_modes_expect click-before '.level == "modesfix.outer" and .tokenBox.width > 0' "no token box to click"
  launcher_modes_expect click-after '.isOpen and .level == "modesfix"' "a click on the token did not leave the mode"
  echo "SMOKE_LAUNCHER_MODES a real click on the token left Outer"

  "$jq_bin" -e '.mode == "light"' "$shot_dir/launcher-modes-theme.json" > /dev/null \
    || fail "launcher-modes: the light frames were not taken in light mode: $(cat "$shot_dir/launcher-modes-theme.json")"
  local shot
  for shot in inner emoji fixture; do
    for mode in dark light; do
      [ -s "$shot_dir/launcher-modes-$shot-$mode.png" ] || fail "launcher-modes: no $shot $mode frame"
      echo "SMOKE_LAUNCHER_MODES_SHOT $shot_dir/launcher-modes-$shot-$mode.png"
    done
  done
}
