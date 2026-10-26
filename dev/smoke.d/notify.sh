# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --notify: four notify-sends into the private bus, then `notifications
# expand on/off`, the rig's stand-in for hovering the stack (there is no
# synthetic pointer here). The expanded frame lands a beat after the toggle
# so the reflow has settled, and `expand off` restores the collapsed pile the
# run's own smoke.png reads.
#
# Three of the four are the icon resolution order (M48 D4), which only the
# expanded frame can show: a collapsed stack draws the front card's content
# and empty chrome for the peeks behind it. One carries a fixture PNG by
# absolute path, one a themed name this session's own icon theme really
# carries, and one no icon at all, which has to fall back to the card's bell.
# The theme is staged into the isolated HOME rather than pulled off the
# machine: the rig hands the session `XDG_DATA_DIRS=$iso_home/.local/share`
# and nothing else, so an icon theme installed system-wide (there is none in
# nix/testvm.nix either) would be invisible to it. hicolor is the one theme
# every other theme falls back to, which is why a name dropped in there
# resolves without the session having to agree on a theme name.
#
# The two layer dumps are the sonner stack's own claim (M48): the toast
# surface is a column the output's height down the stack's own side, and
# stays that size while it is mapped, so the collapse/expand reflow moves cards inside a static window instead of
# resizing a layer surface the compositor would then animate against the
# shell's own motion.
leg_notify_flag="--notify"
leg_notify_order=30
leg_notify_needs="notify-send convert jq"

toasts_expanded_path="$shot_dir/toasts-expanded.png"
toasts_expand_status_path="$shot_dir/toasts-expand-status.txt"
notify_layers_collapsed_path="$shot_dir/toasts-layers-collapsed.json"
notify_layers_expanded_path="$shot_dir/toasts-layers-expanded.json"
# What the server says it is, off the session's own private bus, held byte
# for byte: senders key on the name and version.
notify_server_info_path="$shot_dir/notify-server-info.txt"
notify_icon_dir="$iso_home/.local/share/icons/hicolor/48x48/apps"
notify_icon_file="$notify_icon_dir/formalshell-notify-fixture.png"
# A real freedesktop icon name, so what the card resolves is a themed lookup
# rather than a path that happens to be spelled like one.
notify_icon_theme_name="dialog-information"

leg_notify_timing() {
  # notify-drive.sh's `expand off` lands at 12; this run's generic smoke.png
  # is taken after it, showing the collapsed pile again.
  leg_timing 14 45
}

leg_notify_fixture() {
  mkdir -p "$notify_icon_dir"
  # Distinct flat colours, so which of the three cards resolved which source
  # is readable off the frame rather than inferred.
  $convert_bin -size 48x48 xc:'#3B82F6' "$notify_icon_file"
  $convert_bin -size 48x48 xc:'#22C55E' "$notify_icon_dir/$notify_icon_theme_name.png"
  # QIconLoader enumerates no directories at all without an index.theme, so
  # a bare PNG in the right place still resolves to no icon. Same file the
  # base run's fixture-window icon writes, and deliberately the same
  # contents: only one of the two is ever staged in a given run (this leg
  # takes the fixture window out of the frame), and a future combination
  # that staged both must not disagree about the theme.
  cat > "$iso_home/.local/share/icons/hicolor/index.theme" <<'EOF'
[Icon Theme]
Name=Hicolor
Comment=Fallback icon theme
Directories=48x48/apps

[48x48/apps]
Size=48
Context=Applications
Type=Threshold
EOF
}

leg_notify_drive() {
  local script="$shot_dir/notify-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 3
"$notify_send_bin" -u normal -i "$notify_icon_file" 'Fixture PNG' 'Icon from a path'
sleep 1
"$notify_send_bin" -u normal -i "$notify_icon_theme_name" 'Themed icon' 'Icon from the theme'
sleep 1
"$notify_send_bin" -u normal 'No icon' 'Falls back to the bell'
sleep 1
"$notify_send_bin" -u critical 'Crit' 'Now'
sleep 2
{ busctl --user call org.freedesktop.Notifications /org/freedesktop/Notifications org.freedesktop.Notifications GetServerInformation
  busctl --user call org.freedesktop.Notifications /org/freedesktop/Notifications org.freedesktop.Notifications GetCapabilities
} > "$notify_server_info_path" 2>&1
"$hyprctl_bin" -j layers > "$notify_layers_collapsed_path" 2>&1
$ipc call notifications expand on > "$toasts_expand_status_path" 2>&1
sleep 2
"$grim_bin" "$toasts_expanded_path" > /dev/null 2>&1
"$hyprctl_bin" -j layers > "$notify_layers_expanded_path" 2>&1
sleep 1
$ipc call notifications expand off > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_notify_assert() {
  local f collapsed_size expanded_size
  echo "host org.freedesktop.Notifications owner PID unchanged: $host_notifications_owner_after"
  if [ ! -s "$toasts_expand_status_path" ]; then
    fail "no notifications expand status produced"
  fi
  cat "$toasts_expand_status_path"
  if ! grep -q "^on$" "$toasts_expand_status_path"; then
    fail "notifications expand on did not report on, got: $(cat "$toasts_expand_status_path")"
  fi

  for f in "$notify_layers_collapsed_path" "$notify_layers_expanded_path"; do
    if [ ! -s "$f" ]; then
      fail "no layer dump produced at $f"
    fi
  done
  # Read as one string so a missing key cannot pass by comparing null with
  # null, and checked against the monitor dev/smoke.sh pins rather than
  # against itself.
  collapsed_size=$("$jq_bin" -r '[.[].levels[]?[]? | select(.namespace == "formalshell:notifications")][0] | "\(.w)x\(.h)"' "$notify_layers_collapsed_path")
  expanded_size=$("$jq_bin" -r '[.[].levels[]?[]? | select(.namespace == "formalshell:notifications")][0] | "\(.w)x\(.h)"' "$notify_layers_expanded_path")
  echo "toast layer size collapsed: $collapsed_size, expanded: $expanded_size"
  if [ "${collapsed_size%x*}" -ge 1920 ] 2>/dev/null || [ "${collapsed_size#*x}" != 1080 ]; then
    fail "the toast layer surface is not a column the output's height, got $collapsed_size"
  fi
  if [ "$collapsed_size" != "$expanded_size" ]; then
    fail "the toast layer surface resized on expand ($collapsed_size -> $expanded_size), the compositor animates that geometry"
  fi

  if [ ! -f "$toasts_expanded_path" ]; then
    fail "no toasts-expanded screenshot produced"
  fi
  cat "$notify_server_info_path" 2>/dev/null || true
  # GetServerInformation at whatever release this is, and GetCapabilities
  # held byte for byte.
  grep -qxE 'ssss "formalshell" "FormalShell" "[0-9]+\.[0-9]+\.[0-9]+" "1\.2"' "$notify_server_info_path" \
    || fail "GetServerInformation is not the expected answer: $(head -n 1 "$notify_server_info_path")"
  grep -qxF 'as 4 "persistence" "body" "actions" "icon-static"' "$notify_server_info_path" \
    || fail "GetCapabilities is not the expected answer: $(tail -n 1 "$notify_server_info_path")"
  echo "SMOKE_NOTIFY_SERVER_INFO $notify_server_info_path"
  echo "SMOKE_TOASTS_EXPANDED $toasts_expanded_path"
  echo "SMOKE_TOASTS_LAYERS_COLLAPSED $notify_layers_collapsed_path"
  echo "SMOKE_TOASTS_LAYERS_EXPANDED $notify_layers_expanded_path"
}
