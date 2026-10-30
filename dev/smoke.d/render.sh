# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --render: `render.backend` resolved at start, three shells in one session
# (each new one replaces the last through the instance lock). The first
# asks for gpu: in a nested session (a real render node behind the parent)
# `debug dump`'s render block reads gpu and names the device, and on a vkms
# rig (no render node) it reads cpu with the reason. The second asks for
# cpu. The third asks for gpu with VK_DRIVER_FILES naming no driver, a
# machine with no Vulkan driver at all: cpu, saying no Vulkan device was
# found. Each opens the launcher, and the card is cropped off `menu
# status`'s cardBox: the GPU's crop within 3% RMSE of the CPU's, and the
# fallback's too, none of them flat.
leg_render_flag="--render"
leg_render_order=113
leg_render_needs="jq convert"

render_dump() { echo "$shot_dir/render-$1.json"; }
render_frame() { echo "$shot_dir/render-$1.png"; }
render_status() { echo "$shot_dir/render-$1-menu.json"; }

leg_render_validate() {
  # It replaces the session's shell twice, every other leg's shell too.
  local v
  for v in $(compgen -v | grep -E '^leg_[a-z0-9_]+_on$'); do
    if [ "$v" != leg_render_on ] && [ "${!v}" = true ]; then
      echo "usage: --render replaces the session's shell and cannot combine with --$(echo "${v#leg_}" | sed 's/_on$//; s/_/-/g')" >&2
      exit 1
    fi
  done
}

leg_render_fixture() {
  settings_pin '.render.backend = "gpu"'
}

leg_render_timing() {
  leg_timing 45 80
}

leg_render_drive() {
  local script="$shot_dir/render-drive.sh" settings="$iso_home/.config/formalshell/settings.json" launch="$PWD/result/bin/formalshell"
  write_script "$script" <<EOF
#!/usr/bin/env bash
# One shell's turn: wait for it to answer with the asked backend, dump,
# open the launcher, photograph it, close it.
turn() {
  local name=\$1 asked=\$2
  for _ in \$(seq 1 80); do
    $ipc call debug dump 2>/dev/null | "$jq_bin" -e --arg a "\$asked" '.render.asked == \$a' > /dev/null 2>&1 && break
    sleep 0.25
  done
  sleep 2
  $ipc call debug dump > "$shot_dir/render-\$name.json" 2>&1
  $ipc call menu toggle > /dev/null 2>&1
  sleep 2.5
  $ipc call menu status > "$shot_dir/render-\$name-menu.json" 2>&1
  "$grim_bin" "$shot_dir/render-\$name.png" > /dev/null 2>&1
  $ipc call menu toggle > /dev/null 2>&1
  sleep 1.5
}
sleep 4
turn gpu gpu
"$jq_bin" '.render.backend = "cpu"' "$settings" > "$shot_dir/render-settings.json" && cat "$shot_dir/render-settings.json" > "$settings"
"$launch" > "$shot_dir/render-cpu.log" 2>&1 &
turn cpu cpu
"$jq_bin" '.render.backend = "gpu"' "$settings" > "$shot_dir/render-settings.json" && cat "$shot_dir/render-settings.json" > "$settings"
echo '{"file_format_version": "1.0.0"}' > "$shot_dir/render-no-icd.json"
VK_DRIVER_FILES="$shot_dir/render-no-icd.json" "$launch" > "$shot_dir/render-nodriver.log" 2>&1 &
turn nodriver gpu
EOF
  hypr_exec_once "bash $script"
}

# The card's crop off one turn's frame, by its menu status.
render_crop() {
  local box
  box=$("$jq_bin" -r '.cardBox | "\(.width)x\(.height)+\(.x)+\(.y)"' "$(render_status "$1")" 2>/dev/null)
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
  else
    render_expect gpu '.render.asked == "gpu" and .render.backend == "cpu" and (.render.reason | test("render node|Vulkan device"))' "a $session_mode session has no render node to draw on, and the reason does not say so"
  fi
  render_expect cpu '.render.asked == "cpu" and .render.backend == "cpu" and .render.reason == "render.backend is cpu"' "asked for cpu"
  render_expect nodriver '.render.asked == "gpu" and .render.backend == "cpu" and (.render.reason | test("Vulkan device"))' "with no Vulkan driver the GPU ask did not fall back saying so"
  for t in gpu cpu nodriver; do
    card=$(render_crop "$t")
    echo "SMOKE_RENDER_$(echo "$t" | tr a-z A-Z)_CARD $card"
    spread=$("$convert_bin" "$card" -colorspace Gray -format '%[fx:standard_deviation]' info: 2>/dev/null)
    echo "render $t card spread: $spread"
    awk -v s="${spread:-0}" 'BEGIN { exit !(s > 0.02) }' || fail "--render $t: the launcher card is flat (spread ${spread:-none}), nothing was drawn"
  done
  for t in gpu nodriver; do
    rmse=$("$convert_bin" "$shot_dir/render-$t-card.png" "$shot_dir/render-cpu-card.png" -metric RMSE -compare -format '%[distortion]' info: 2>/dev/null)
    echo "render $t card against cpu: RMSE ${rmse:-none}"
    awk -v r="${rmse:-1}" 'BEGIN { exit !(r < 0.03) }' || fail "--render $t: the launcher card differs from the CPU's (RMSE ${rmse:-none}, over 0.03)"
  done
}
