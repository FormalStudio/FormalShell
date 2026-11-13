# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --gpu-lost pins `render.backend` gpu and, once the device is up, loses it
# at the shell's next picture upload (`debug gpuLose`), in the eviction's
# submit ahead of it: where NVIDIA's loss under the launcher was learnt
# (spec 2026-10-31-gpu-rendering.md, section 1). It rides a leg that draws
# pictures after that (`--launcher-modes`), whose own frames are then the
# CPU's. Its own claim: the shell never panics, answers after the loss, and
# `debug dump`'s render block draws on the CPU with the loss as its reason.
# The shell takes `debug gpuLose` only with FORMALSHELL_SMOKE_GPU_LOSE=1 on
# its launch line, which this fixture sets; `--backend` checks a shell
# without it refuses.
leg_gpu_lost_flag="--gpu-lost"
leg_gpu_lost_order=8
leg_gpu_lost_compositors="hyprland shoji"
leg_gpu_lost_needs="jq"
leg_gpu_lost_fixture_window=keep
leg_gpu_lost_budget="it needs a Vulkan device, which only g815 has"
leg_gpu_lost_rider="it pins render.backend and changes the device under the ridden leg, not what it draws"
leg_gpu_lost_claims="render.backend"

gpu_lost_up_path="$shot_dir/gpu-lost-up.json"
gpu_lost_reply_path="$shot_dir/gpu-lost-reply.txt"
gpu_lost_dump_path="$shot_dir/gpu-lost-dump.json"

leg_gpu_lost_fixture() {
  settings_pin '.render.backend = "gpu"'
  shell_env="${shell_env:+$shell_env }FORMALSHELL_SMOKE_GPU_LOSE=1"
}

leg_gpu_lost_drive() {
  local script="$shot_dir/gpu-lost-drive.sh"
  write_script "$script" <<EOS
#!/usr/bin/env bash
until_jq 20 "$gpu_lost_up_path" '.render.backend == "gpu"' call debug dump || exit 0
call debug gpuLose > "$gpu_lost_reply_path" 2>&1
until_jq 120 "$gpu_lost_dump_path" '.render.backend == "cpu"' call debug dump
EOS
  wm_exec_once "bash $script"
}

leg_gpu_lost_assert() {
  [ -s "$gpu_lost_up_path" ] || fail "no debug dump produced for the GPU loss check"
  "$jq_bin" -e '.render.backend == "gpu"' "$gpu_lost_up_path" > /dev/null 2>&1 \
    || fail "the shell never drew on the GPU: $("$jq_bin" -c .render "$gpu_lost_up_path" 2>/dev/null)"
  [ "$(cat "$gpu_lost_reply_path" 2>/dev/null)" = ok ] || fail "debug gpuLose answered: $(cat "$gpu_lost_reply_path" 2>/dev/null)"
  "$jq_bin" -e '.render.backend == "cpu" and (.render.reason // "" | startswith("the GPU device was lost"))' "$gpu_lost_dump_path" > /dev/null 2>&1 \
    || fail "no answer drawing on the CPU after the loss: $("$jq_bin" -c .render "$gpu_lost_dump_path" 2>/dev/null || cat "$gpu_lost_dump_path")"
  local panic
  panic=$(grep -m1 'panicked' "$shell_log_path" || true)
  [ -z "$panic" ] || fail "the shell panicked after the loss: $panic"
  grep -q '^formalshell: render: .*; drawing on the CPU$' "$shell_log_path" || fail "the shell never logged its move to the CPU"
  echo "SMOKE_GPU_LOST $("$jq_bin" -c .render "$gpu_lost_dump_path")"
}
