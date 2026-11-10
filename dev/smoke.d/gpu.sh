# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --gpu: the monitor surfaces against a hybrid Intel + NVIDIA laptop this rig
# is not. A PATH shim and a rows file draw it, the same hermetic-producer line
# the clipssh shim draws, and neither invents a /sys entry:
#
#   nvidia-smi  the monitor service already resolves it off PATH, so a shim
#               printing the owner's real g815 row makes the whole
#               collector -> parseNvidia -> mergeGpu -> render path real.
#               `[N/A]` is a value nvidia-smi genuinely emits for fan speed
#               on a laptop GPU and has to come out unavailable, never 0.
#   drm rows    the monitor service reads /sys/class/drm itself. With
#               FORMALSHELL_SMOKE_DRM_ROWS set it reads the @drm rows of
#               crates/fs-system/tests/fixtures/gpu-hybrid.txt instead, so
#               every other section stays this machine's own honest /proc.
#               GpuService's own nvidia-offload/prime-run probe still answers
#               "neither", since the env-var offload path is what this leg
#               proves.
#
# The rows replace the machine's own cards rather than join them: this
# session renders on a real vkms card that enumerates as card0, the same id
# the fixture's dGPU carries. Two cards under one id is a machine that cannot
# exist, and it renders like one: the launcher's route keys its rows on the
# card id. So the fixture pair is the whole card list, asserted as exactly
# two.
#
# Then the claim no screenshot can make: `monitor launch` against a fixture
# .desktop whose Exec is a probe writing its own argv and environment to a
# file, read back to confirm all four offload variables reached the child and
# that the Exec's %U field code did not.
leg_gpu_flag="--gpu"
leg_gpu_order=145
leg_gpu_needs="jq"
leg_gpu_claims="launcher=shared"

gpu_shim_dir="$shot_dir/gpu-shim"
gpu_drm_rows_path="$shot_dir/gpu-drm-rows.txt"
gpu_probe_script="$shot_dir/gpu-offload-probe.sh"
gpu_offload_env_path="$shot_dir/gpu-offload-env.txt"
gpu_cards_path="$shot_dir/gpu-cards.json"
gpu_route_reply_path="$shot_dir/gpu-route-reply.txt"
gpu_menu_status_path="$shot_dir/gpu-menu-status.json"
gpu_launch_reply_path="$shot_dir/gpu-launch-reply.txt"
gpu_route_png="$shot_dir/gpu-route.png"
gpu_monitor_png="$shot_dir/gpu-monitor.png"

leg_gpu_fixture() {
  mkdir -p "$gpu_shim_dir"

  cat > "$gpu_shim_dir/nvidia-smi" <<'EOF'
#!/usr/bin/env bash
case "${1:-}" in
  --query-gpu=*)
    printf '%s\n' '0, NVIDIA GeForce RTX 5070 Laptop GPU, 16, 50, 78, 8151, 12.17, [N/A]'
    exit 0
    ;;
esac
exit 1
EOF
  chmod +x "$gpu_shim_dir/nvidia-smi"

  # The fixture's @drm section only: @nvidia comes from the shim above (the
  # real collector line really invoking it), @gfx stays this machine's own
  # empty answer. The rows are the fixture file itself rather than a copy
  # pasted in here, so they are the same bytes the parser tests assert
  # against, captured off real hardware.
  awk '/^@drm$/ { in_drm = 1; next } /^@/ { in_drm = 0 } in_drm' crates/fs-system/tests/fixtures/gpu-hybrid.txt > "$gpu_drm_rows_path"
  if [ ! -s "$gpu_drm_rows_path" ]; then
    echo "SMOKE_FAIL: no @drm rows in crates/fs-system/tests/fixtures/gpu-hybrid.txt, the collector shim would hand back an empty card list" >&2
    exit 1
  fi

  shell_env="${shell_env:+$shell_env }FORMALSHELL_SMOKE_DRM_ROWS=$gpu_drm_rows_path"

  # The entry `monitor launch` offloads. Its Exec is a probe that writes its
  # own argv and environment and exits, the only way an env var reaching a
  # child is provable at all. The trailing %U is deliberate: a field code the
  # desktop spec says a launcher strips, so the recorded argv is also the
  # proof stripFieldCodes ran.
  cat > "$iso_home/.local/share/applications/formalshell-gpu-probe.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=GPU Offload Probe
Exec=$gpu_probe_script %U
EOF
  cat > "$gpu_probe_script" <<EOF
#!/usr/bin/env bash
{
  printf 'ARGV:%s\n' "\$*"
  env | sort
} > "$gpu_offload_env_path"
EOF
  chmod +x "$gpu_probe_script"

  # The scaffold owns the shell's launch line, so the nvidia-smi shim
  # reaches the shell by riding the rig's own environment into the session
  # (PATH is not in session_env).
  export PATH="$gpu_shim_dir:$PATH"
}

leg_gpu_timing() {
  # gpu-drive.sh's last step lands well inside this, with the offload
  # launch 3s and a settle before it so the probe has long since written
  # its file.
  leg_timing 60
}

leg_gpu_drive() {
  local script="$shot_dir/gpu-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
$ipc call monitor gpu > "$gpu_cards_path" 2>&1
$ipc call menu summon gpu > "$gpu_route_reply_path" 2>&1
until_settled
$ipc call menu status > "$gpu_menu_status_path" 2>&1
"$grim_bin" "$gpu_route_png" > /dev/null 2>&1
sleep 1
$ipc call monitor launch formalshell-gpu-probe card0 > "$gpu_launch_reply_path" 2>&1
sleep 3
$ipc call menu summon monitor > /dev/null 2>&1
until_settled
"$grim_bin" "$gpu_monitor_png" > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_gpu_assert() {
  local var gpu_card_count
  if [ ! -s "$gpu_cards_path" ]; then
    fail "no monitor gpu produced"
  fi
  cat "$gpu_cards_path"; echo
  expect_jq "$gpu_cards_path" 'any(.cards[]; .card == "card0" and .driver == "nvidia")' "the shimmed collector's dGPU never reached the card list"
  expect_jq "$gpu_cards_path" 'any(.cards[]; .card == "card1" and .driver == "i915")' "the shimmed collector's iGPU never reached the card list"
  # Three claims on card0: fan.speed's `[N/A]` became null rather than 0,
  # nvidia-smi's marketing name is what named the card, and boot_vga (0 on
  # this card) is what made it the discrete one, not its card NUMBER, which
  # is the lower of the two.
  if ! jq -e '.cards[] | select(.card == "card0") | .metrics.fanPercent == null and .name == "NVIDIA GeForce RTX 5070 Laptop GPU" and .discrete == true' "$gpu_cards_path" > /dev/null; then
    fail "the nvidia-smi row did not merge into card0 as an unavailable fan on a discrete card. Got: $(cat "$gpu_cards_path")"
  fi
  expect_jq "$gpu_cards_path" 'any(.cards[]; .metrics.busy == 0.16)' "nvidia-smi's 16% utilization did not land as a 0..1 fraction"
  # i915 has no unprivileged utilisation counter, so an empty metrics record
  # beside a named card is the honest answer, never a zero.
  if ! jq -e '.cards[] | select(.card == "card1") | .metrics.available == false and ([.metrics[] | select(. != null and . != false)] | length) == 0 and .name == "Onboard - Video" and .discrete == false' "$gpu_cards_path" > /dev/null; then
    fail "the iGPU did not render as ACPI-labelled, integrated and metric-less. Got: $(cat "$gpu_cards_path")"
  fi
  # The external display hangs off the dGPU on this machine and the internal
  # panel off the iGPU, which is the whole reason connectors are collected.
  expect_jq "$gpu_cards_path" 'any(.cards[].outputs[]; . == {"name": "HDMI-A-1", "connected": true})' "card0's connected HDMI connector never reached the record"
  # The fixture pair is the whole list: this machine's own cards are out of
  # the collector's view for the one command that reads /sys/class/drm (see
  # the header), so a third card here would mean the replacement leaked.
  gpu_card_count=$("$jq_bin" '[.cards[] | select(.card | startswith("card"))] | length' "$gpu_cards_path" 2>/dev/null)
  if [ "$gpu_card_count" != "2" ]; then
    fail "monitor gpu reported $gpu_card_count cards, want the fixture's 2: $(cat "$gpu_cards_path")"
  fi
  if ! grep -q '^ok$' "$gpu_route_reply_path" 2>/dev/null; then
    fail "menu summon gpu did not answer ok, got: $(cat "$gpu_route_reply_path" 2>/dev/null)"
  fi
  if [ -s "$gpu_menu_status_path" ]; then
    cat "$gpu_menu_status_path"; echo
  fi
  if [ ! -f "$gpu_route_png" ]; then
    fail "no gpu-route screenshot produced"
  fi
  echo "SMOKE_GPU_ROUTE $gpu_route_png"
  if ! grep -q '^ok: launched' "$gpu_launch_reply_path" 2>/dev/null; then
    fail "monitor launch was refused, got: $(cat "$gpu_launch_reply_path" 2>/dev/null)"
  fi
  cat "$gpu_launch_reply_path"
  # The assertion no screenshot can make. The probe wrote its own
  # environment, so these four lines are the offload argv actually reaching a
  # child process rather than a plausible-looking reply string.
  if [ ! -s "$gpu_offload_env_path" ]; then
    fail "the offload probe never ran, no environment dump at $gpu_offload_env_path"
  fi
  for var in '__NV_PRIME_RENDER_OFFLOAD=1' '__NV_PRIME_RENDER_OFFLOAD_PROVIDER=NVIDIA-G0' '__GLX_VENDOR_LIBRARY_NAME=nvidia' '__VK_LAYER_NV_optimus=NVIDIA_only'; do
    if ! grep -qxF "$var" "$gpu_offload_env_path"; then
      fail "the launched child's environment is missing $var. Got: $(grep -E '^(__NV|__GLX|__VK)' "$gpu_offload_env_path" | tr '\n' ' ')"
    fi
  done
  # The entry's Exec carries a %U the launcher is supposed to strip: a child
  # handed a literal field code means stripFieldCodes never ran.
  if ! grep -q '^ARGV:$' "$gpu_offload_env_path"; then
    fail "the launched child was handed arguments, the Exec's %U field code survived. Got: $(grep '^ARGV:' "$gpu_offload_env_path")"
  fi
  if [ ! -f "$gpu_monitor_png" ]; then
    fail "no gpu-monitor screenshot produced"
  fi
  echo "SMOKE_GPU_MONITOR $gpu_monitor_png"
}
