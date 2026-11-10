# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --render: `render.backend` resolved at start, three shells in one session
# (each new one replaces the last through the instance lock). The first
# asks for gpu: in a nested session (a real render node behind the parent)
# `debug dump`'s render block reads gpu and names the device, and on a vkms
# rig (no render node) it reads cpu with the reason. The second asks for
# cpu. The third asks for gpu with VK_DRIVER_FILES naming no driver, a
# machine with no Vulkan driver at all: cpu, saying no Vulkan device was
# found. Each is photographed on the bare desktop (the bar and the desktop
# clock over a gradient wallpaper it is given, which a GPU shell blends on
# the GPU), with the audio panel open, with the launcher open and with a
# sticky toast over the volume OSD, and locked (the real password typed
# after); the launcher card is cropped off `menu status`'s cardBox. The GPU's frames within 3% RMSE of the CPU's, and the
# fallback's too, the card not flat.
leg_render_flag="--render"
leg_render_order=113
leg_render_needs="jq convert notify-send wtype"
leg_render_claims="session render.backend"

render_dump() { echo "$shot_dir/render-$1.json"; }
render_frame() { echo "$shot_dir/render-$1.png"; }
render_status() { echo "$shot_dir/render-$1-menu.json"; }

leg_render_fixture() {
  settings_pin '.render.backend = "gpu"'
  settings_pin '.desktopClock.enabled = true'
}

# The frames every turn takes beside the launcher's.
render_scenes="desktop panel toast lock"

leg_render_timing() {
  leg_timing 100
}

leg_render_drive() {
  local script="$shot_dir/render-drive.sh" launch="$PWD/result/bin/formalshell"
  "$convert_bin" -size 1080x1920 gradient:'#3fb07a-#0b2d20' -rotate 90 "$shot_dir/render-wallpaper.png"
  write_script "$script" <<EOF
#!/usr/bin/env bash
# One shell's turn: wait for it to answer with the asked backend, dump,
# open the launcher, photograph it, close it.
turn() {
  local name=\$1 asked=\$2
  for _ in \$(seq 1 80); do
    $ipc call debug dump 2>/dev/null | "$jq_bin" -e --arg a "\$asked" '.render.asked == \$a and .render.reason != "the GPU is starting"' > /dev/null 2>&1 && break
    sleep 0.25
  done
  $ipc call wallpaper set "$shot_dir/render-wallpaper.png" > /dev/null 2>&1
  # A later shell starts on the theme.json the first one made for this
  # picture and runs no matugen of its own, so it names no palette source.
  until_jq 60 "$shot_dir/render-\$name-theme.json" '.desktop == "$shot_dir/render-wallpaper.png" and (.palette.wallpaper == .desktop or (.palette == null and .themeJsonPresent))' call theme status
  until_settled
  $ipc call debug dump > "$shot_dir/render-\$name.json" 2>&1
  "$grim_bin" "$shot_dir/render-\$name-desktop.png" > /dev/null 2>&1
  date +%H%M > "$shot_dir/render-\$name-desktop.minute"
  $ipc call panel open audio > /dev/null 2>&1
  until_settled
  "$grim_bin" "$shot_dir/render-\$name-panel.png" > /dev/null 2>&1
  date +%H%M > "$shot_dir/render-\$name-panel.minute"
  $ipc call panel close > /dev/null 2>&1
  sleep 1
  $ipc call menu toggle > /dev/null 2>&1
  until_settled
  $ipc call menu status > "$shot_dir/render-\$name-menu.json" 2>&1
  "$grim_bin" "$shot_dir/render-\$name.png" > /dev/null 2>&1
  $ipc call menu toggle > /dev/null 2>&1
  until_settled
  notify-send -u critical -t 0 "Render check" "One sticky toast for each shell" > /dev/null 2>&1
  $ipc call osd volume > /dev/null 2>&1
  sleep 1.2
  "$grim_bin" "$shot_dir/render-\$name-toast.png" > /dev/null 2>&1
  date +%H%M > "$shot_dir/render-\$name-toast.minute"
  $ipc call osd close > /dev/null 2>&1
  sleep 1
  $ipc call lock lock > /dev/null 2>&1
  until_jq 10 "$shot_dir/render-\$name-locked.json" '.locked and .secure' call lock status
  until_settled
  $ipc call lock status > "$shot_dir/render-\$name-lock.json" 2>&1
  "$grim_bin" "$shot_dir/render-\$name-lock.png" > /dev/null 2>&1
  date +%H%M > "$shot_dir/render-\$name-lock.minute"
  "$wtype_bin" -s 300 "$rig_password"
  "$wtype_bin" -s 300 -k Return
  for _ in \$(seq 1 80); do
    [ "\$($ipc call lock isLocked 2>/dev/null)" = false ] && break
    sleep 0.25
  done
}
turn gpu gpu
settings_retarget '.render.backend = "cpu"'
"$launch" > "$shot_dir/render-cpu.log" 2>&1 &
turn cpu cpu
settings_retarget '.render.backend = "gpu"'
echo '{"file_format_version": "1.0.0"}' > "$shot_dir/render-no-icd.json"
VK_DRIVER_FILES="$shot_dir/render-no-icd.json" "$launch" > "$shot_dir/render-nodriver.log" 2>&1 &
turn nodriver gpu
EOF
  hypr_exec_once "bash $script"
}

# The card's crop off one turn's frame, by its menu status.
render_crop() {
  local box
  box=$(scaled_jq -r "$geometry_jq"'.cardBox | geometry' "$(render_status "$1")" 2>/dev/null)
  [ -n "$box" ] && [ "$box" != "nullxnull+null+null" ] || fail "--render $1: menu status carries no cardBox: $(cat "$(render_status "$1")" 2>/dev/null)"
  "$convert_bin" "$(render_frame "$1")" -crop "$box" +repage "$shot_dir/render-$1-card.png" || fail "--render $1: could not crop $box"
  echo "$shot_dir/render-$1-card.png"
}

render_expect() {
  "$jq_bin" -e "$2" "$(render_dump "$1")" > /dev/null \
    || fail "--render $1: $3, got $("$jq_bin" -c .render "$(render_dump "$1")" 2>/dev/null || cat "$(render_dump "$1")")"
}

leg_render_assert() {
  local t card spread rmse
  for t in gpu cpu nodriver; do
    [ -s "$(render_dump "$t")" ] || fail "--render $t: no debug dump"
    [ -f "$(render_frame "$t")" ] || fail "--render $t: no frame"
    echo "render $t: $("$jq_bin" -c .render "$(render_dump "$t")")"
    echo "SMOKE_RENDER_$(echo "$t" | tr a-z A-Z) $(render_frame "$t")"
  done
  if [ "$session_mode" = nested ]; then
    render_expect gpu '.render.asked == "gpu" and .render.backend == "gpu" and (.render.device | test("renderD"))' "a nested session has a render node, and the GPU path did not take it"
    grep -q 'commit surface=wallpaper .* picture=true gpu=true' "$shell_log_path" || fail "--render gpu: the wallpaper was never blended on the GPU"
  else
    render_expect gpu '.render.asked == "gpu" and .render.backend == "cpu" and (.render.reason | test("render node|Vulkan device"))' "a $session_mode session has no render node to draw on, and the reason does not say so"
  fi
  render_expect cpu '.render.asked == "cpu" and .render.backend == "cpu" and .render.reason == "render.backend is cpu"' "asked for cpu"
  # Off a nested session there is no render node, and that is the reason
  # given before any driver is looked for.
  nodriver_reason="Vulkan device"
  [ "$session_mode" = nested ] || nodriver_reason="render node|Vulkan device"
  render_expect nodriver '.render.asked == "gpu" and .render.backend == "cpu" and (.render.reason | test("'"$nodriver_reason"'"))' "with no Vulkan driver the GPU ask did not fall back saying so"
  for t in gpu cpu nodriver; do
    card=$(render_crop "$t")
    echo "SMOKE_RENDER_$(echo "$t" | tr a-z A-Z)_CARD $card"
    spread=$("$convert_bin" "$card" -colorspace Gray -format '%[fx:standard_deviation]' info: 2>/dev/null)
    echo "render $t card spread: $spread"
    awk -v s="${spread:-0}" 'BEGIN { exit !(s > 0.02) }' || fail "--render $t: the launcher card is flat (spread ${spread:-none}), nothing was drawn"
  done
  local scene box mask
  for scene in $render_scenes; do
    echo "SMOKE_RENDER_CPU_$(echo "$scene" | tr a-z A-Z) $shot_dir/render-cpu-$scene.png"
  done
  for t in gpu nodriver; do
    for scene in card $render_scenes; do
      [ -f "$shot_dir/render-$t-$scene.png" ] || fail "--render $t: no $scene frame"
      [ "$scene" = card ] || echo "SMOKE_RENDER_$(echo "$t" | tr a-z A-Z)_$(echo "$scene" | tr a-z A-Z) $shot_dir/render-$t-$scene.png"
      # The desktop clock's minute can roll between two turns: its rect
      # is then left out of that one comparison.
      mask=()
      if [ "$scene" != card ] && ! cmp -s "$shot_dir/render-$t-$scene.minute" "$shot_dir/render-cpu-$scene.minute"; then
        if [ "$scene" = lock ]; then
          box=$("$jq_bin" -r '.outputs | to_entries | .[0].value.clockRect | "\(.x | floor),\(.y | floor) \(.x + .width | ceil),\(.y + .height | ceil)"' "$shot_dir/render-cpu-lock.json" 2>/dev/null)
        else
          box=$("$jq_bin" -r '.desktopClock[0].rect | "\(.[0]),\(.[1]) \(.[0] + .[2]),\(.[1] + .[3])"' "$(render_dump cpu)" 2>/dev/null)
        fi
        [ -n "$box" ] && mask=(-fill black -draw "rectangle $box")
        echo "render $t $scene: the minute rolled since the CPU's frame, the clock left out"
      fi
      rmse=$("$convert_bin" \( "$shot_dir/render-$t-$scene.png" "${mask[@]}" \) \( "$shot_dir/render-cpu-$scene.png" "${mask[@]}" \) -metric RMSE -compare -format '%[distortion]' info: 2>/dev/null)
      echo "render $t $scene against cpu: RMSE ${rmse:-none}"
      awk -v r="${rmse:-1}" 'BEGIN { exit !(r < 0.03) }' || fail "--render $t: the $scene frame differs from the CPU's (RMSE ${rmse:-none}, over 0.03)"
    done
  done
}
