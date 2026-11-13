# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --backend <gpu|cpu> pins `render.backend` in the settings fixture (a
# `settings_pin`) and rides any other leg, so the budget legs read either
# path whatever `auto` picks on the machine (`dev/perf-gate.py --backend`).
# Its own claim: `debug dump`'s render block drawing on the asked backend
# once the device is up, or, asking gpu where none is usable, on the CPU
# with the reason; and a GPU that came up never given up later in the run
# (the shell's `; drawing on the CPU` line), which the ridden leg's own
# frames would not show. And `debug gpuLose` refused, as in every shell the
# rig did not launch with FORMALSHELL_SMOKE_GPU_LOSE=1 (`--gpu-lost`).
leg_backend_flag="--backend <backend>"
leg_backend_order=8
leg_backend_compositors="hyprland shoji"
leg_backend_needs="jq"
leg_backend_fixture_window=keep
# A run that owns its whole session still reads either path.
leg_backend_rider="it pins render.backend and drives nothing the ridden leg sees"
leg_backend_claims="render.backend"

backend_dump_path="$shot_dir/backend-dump.json"
backend_lose_path="$shot_dir/backend-gpu-lose.txt"

leg_backend_validate() {
  case "$(leg_arg backend)" in
    gpu|cpu) ;;
    *)
      echo "usage: $0 --backend <gpu|cpu>" >&2
      exit 1
      ;;
  esac
}

leg_backend_fixture() {
  settings_pin ".render.backend = \"$(leg_arg backend)\""
}

leg_backend_drive() {
  local script="$shot_dir/backend-drive.sh"
  write_script "$script" <<EOS
#!/usr/bin/env bash
for _ in \$(seq 1 80); do
  $ipc call debug dump > "$backend_dump_path" 2>&1
  "$jq_bin" -e '.render.reason != "the GPU is starting"' "$backend_dump_path" > /dev/null 2>&1 && break
  sleep 0.25
done
$ipc call debug gpuLose > "$backend_lose_path" 2>&1
EOS
  wm_exec_once "bash $script"
}

leg_backend_assert() {
  local asked
  asked=$(leg_arg backend)
  [ -s "$backend_dump_path" ] || fail "no debug dump produced for the backend check"
  "$jq_bin" -e --arg a "$asked" '.render.asked == $a and (.render.backend == $a or ($a == "gpu" and .render.reason != null))' "$backend_dump_path" > /dev/null \
    || fail "the dump's render block does not draw on $asked: $("$jq_bin" -c .render "$backend_dump_path")"
  echo "SMOKE_BACKEND $("$jq_bin" -c .render "$backend_dump_path")"
  grep -q 'FORMALSHELL_SMOKE_GPU_LOSE=1' "$backend_lose_path" 2> /dev/null \
    || fail "debug gpuLose was not refused without the rig's opt-in: $(cat "$backend_lose_path" 2> /dev/null)"
  echo "SMOKE_BACKEND_GPU_LOSE refused: $(head -c 200 "$backend_lose_path")"
  if "$jq_bin" -e '.render.backend == "gpu"' "$backend_dump_path" > /dev/null; then
    local lost
    lost=$(grep -m1 '^formalshell: render: .*; drawing on the CPU$' "$shell_log_path" || true)
    [ -z "$lost" ] || fail "the GPU path gave up during the run: $lost"
  fi
}
