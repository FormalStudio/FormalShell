# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --launcher-apps: app rows wearing their own full-colour icons, and a long
# list resting on whole rows. Five desktop entries name icons copied out of
# the host's own hicolor theme (Steam, mpv, foot, Files, Celluloid, the
# ones a Tumbleweed rig carries), and the Apps route lists them: every one
# of their rows reads `picture` true in `menu status`'s `drawn`, photographed
# for the Pond comparison. Then the root searched for "e", a list longer
# than the card: every row box the body's viewport touches lies whole inside
# it, so the card never rests on half a row over the foot.
leg_launcher_apps_flag="--launcher-apps"
leg_launcher_apps_order=262
leg_launcher_apps_needs="jq"

launcher_apps_dir="$iso_home/.local/share/applications"
launcher_apps_icons="$iso_home/.local/share/icons/hicolor"
# desktop id, name, icon name, the icon file under /usr/share/icons/hicolor.
launcher_apps_list="steam|Steam|steam|256x256/apps/steam.png
mpv|mpv Media Player|mpv|scalable/apps/mpv.svg
foot|Foot|foot|scalable/apps/foot.svg
org.gnome.Nautilus|Files|org.gnome.Nautilus|scalable/apps/org.gnome.Nautilus.svg
celluloid|Celluloid|io.github.celluloid_player.Celluloid|scalable/apps/io.github.celluloid_player.Celluloid.svg"

leg_launcher_apps_fixture() {
  mkdir -p "$launcher_apps_dir" "$launcher_apps_icons/256x256/apps" "$launcher_apps_icons/scalable/apps"
  : > "$shot_dir/launcher-apps-ids.txt"
  local id name icon file
  while IFS='|' read -r id name icon file; do
    [ -f "/usr/share/icons/hicolor/$file" ] || continue
    cp "/usr/share/icons/hicolor/$file" "$launcher_apps_icons/$file"
    printf '[Desktop Entry]\nType=Application\nName=%s\nExec=true\nIcon=%s\n' "$name" "$icon" \
      > "$launcher_apps_dir/formalshell-apps-$id.desktop"
    echo "apps.formalshell-apps-$id" >> "$shot_dir/launcher-apps-ids.txt"
  done <<< "$launcher_apps_list"
  cat > "$launcher_apps_icons/index.theme" <<'EOF'
[Icon Theme]
Name=Hicolor
Comment=Fallback icon theme
Directories=256x256/apps,scalable/apps

[256x256/apps]
Size=256
Context=Applications
Type=Threshold

[scalable/apps]
Size=128
MinSize=8
MaxSize=512
Context=Applications
Type=Scalable
EOF
}

leg_launcher_apps_timing() {
  leg_timing 16 50
}

leg_launcher_apps_drive() {
  local script="$shot_dir/launcher-apps-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 4
$ipc call menu summon apps > /dev/null 2>&1
sleep 2.5
"$grim_bin" "$shot_dir/launcher-apps.png" > /dev/null 2>&1
$ipc call menu status > "$shot_dir/launcher-apps-status.json" 2>&1
$ipc call menu close > /dev/null 2>&1
sleep 1
$ipc call menu toggle > /dev/null 2>&1
sleep 1.5
$ipc call menu filter e > /dev/null 2>&1
sleep 2
"$grim_bin" "$shot_dir/launcher-apps-long.png" > /dev/null 2>&1
$ipc call menu status > "$shot_dir/launcher-apps-long.json" 2>&1
$ipc call menu close > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_launcher_apps_assert() {
  local status="$shot_dir/launcher-apps-status.json" long="$shot_dir/launcher-apps-long.json" n
  echo "SMOKE_LAUNCHER_APPS_SHOT $shot_dir/launcher-apps.png"
  echo "SMOKE_LAUNCHER_APPS_LONG $shot_dir/launcher-apps-long.png"
  echo "SMOKE_LAUNCHER_APPS_STATUS $status"
  echo "SMOKE_LAUNCHER_APPS_LONG_STATUS $long"
  n=$(wc -l < "$shot_dir/launcher-apps-ids.txt")
  [ "$n" -ge 3 ] || fail "launcher-apps: only $n of the five icons are on this host"
  [ -s "$status" ] || fail "launcher-apps: no menu status on the Apps route"
  local id
  while read -r id; do
    "$jq_bin" -e --arg id "$id" '[.drawn[] | select(.id == $id)] | length == 1 and .[0].picture' "$status" > /dev/null 2>&1 \
      || fail "launcher-apps: $id drew no picture: $("$jq_bin" -c '[.view, (.drawn | map([.id, .picture]))]' "$status" 2>&1 | head -c 600)"
  done < "$shot_dir/launcher-apps-ids.txt"
  echo "SMOKE_LAUNCHER_APPS $n app rows wear their own icons"
  [ -s "$long" ] || fail "launcher-apps: no menu status for the long list"
  "$jq_bin" -e '.scrollMax > 0' "$long" > /dev/null 2>&1 \
    || fail "launcher-apps: the 'e' list fits the card, nothing to cut: $("$jq_bin" -c '{body, scrollMax}' "$long")"
  # A row the viewport clips can come back as a shorter hit, so a whole row
  # is one inside the body at the full row height.
  "$jq_bin" -e '.body as $b | (.rowBoxes | map(.height) | max) as $h | [.rowBoxes[]
      | select(.y < $b.y + $b.height and .y + .height > $b.y)
      | select(.y < $b.y or .y + .height > $b.y + $b.height or .height < $h)] | length == 0' "$long" > /dev/null 2>&1 \
    || fail "launcher-apps: a row is cut by the body's edge: body $("$jq_bin" -c '.body' "$long") rows $("$jq_bin" -c '[.rowBoxes[] | [.y, .height]]' "$long")"
  echo "SMOKE_LAUNCHER_APPS the long list rests on whole rows"
}
