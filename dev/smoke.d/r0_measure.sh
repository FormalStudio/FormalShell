# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --r0-measure <seconds>: R0's numbers (plans/2026-10-06-r0-rust-spike.md
# Task 4) on one timeline, so runs on different hosts compare. It records and asserts nothing about pass or fail: the
# budgets are read off its output by hand.
#
# The bar is pinned to workspaces on the left and the clock in the centre,
# which is all the spike draws. A foot window under the T3 Code app id sits
# on workspace 1 throughout, its one agent host an `ssh` PATH shim answering
# t3-agents with no threads; the badge spins over `debug r0Spinner`.
#
# Timeline, after the shell's pid shows up and 20s of settling:
#   idle      60s of /proc/<pid>/stat utime+stime, all threads
#   spinner   the same 60s with the badge spinning
#   panel     ten opens and closes of the clock's panel
#   scrim     ten scrim fades
#   stall     the badge spinning across ten panel opens, for the longest gap
#             between bar frames and the time to the opening surface's first
#             frame
#   rss       VmRSS and the thread count at <seconds> after the launch
# Every step is stamped in r0-marks.txt on the wall clock in ns. The shell
# logs every commit and puts its own zero on the wall clock
# (`start epoch_us=`). shell-start.ns is the launch stamp a cold start
# counts from.
leg_r0_measure_flag="--r0-measure <seconds>"
leg_r0_measure_order=900
leg_r0_measure_needs="foot"

r0_shim_dir="$shot_dir/r0-shim"
r0_marks_path="$shot_dir/r0-marks.txt"
r0_results_path="$shot_dir/r0-results.txt"
r0_done_path="$shot_dir/r0-done"
r0_start_path="$shot_dir/shell-start.ns"

leg_r0_measure_validate() {
  local other
  case "$(leg_arg r0_measure)" in
    ''|*[!0-9]*) echo "usage: --r0-measure <seconds>, the RSS sample's time after launch" >&2; exit 1 ;;
  esac
  [ "$(leg_arg r0_measure)" -ge 300 ] || { echo "usage: --r0-measure needs at least 300s for its timeline" >&2; exit 1; }
  for other in "${legs[@]}"; do
    if [ "$other" != r0_measure ] && leg_on "$other"; then
      echo "usage: --r0-measure times the shell alone and cannot combine with --${other//_/-}" >&2
      exit 1
    fi
  done
}

leg_r0_measure_fixture() {
  mkdir -p "$r0_shim_dir"
  cat > "$r0_shim_dir/ssh" <<'EOF'
#!/usr/bin/env bash
while :; do
  printf '%s\n' '{"environment": "e0", "threads": []}'
  sleep 2
done
EOF
  chmod +x "$r0_shim_dir/ssh"
  export PATH="$r0_shim_dir:$PATH"
  settings_fragment ', "bar": {"layout": {"left": ["workspaces"], "center": ["clock"], "right": []}}, "workspaces": {"agentHosts": ["fakehost"]}'
}

leg_r0_measure_shell() {
  local software=""
  # The scaffold's start script forces llvmpipe for the vkms card's sake; a
  # real host keeps its GPU, which is what the shell runs on there.
  if [ "$session_mode" = vkms ]; then software="export LIBGL_ALWAYS_SOFTWARE=1"; fi
  write_script "$1" <<EOF
#!/usr/bin/env bash
$software
date '+%s%N %z' > "$r0_start_path"
$shell_prefix "$shell_bin" > "$shell_log_path" 2>&1 &
echo \$! > "$shot_dir/shell.pid"
wait
EOF
}

leg_r0_measure_timing() {
  local rss_at
  rss_at=$(leg_arg r0_measure)
  leg_timing $((rss_at + 20)) $((rss_at + 90))
}

leg_r0_measure_drive() {
  local script="$shot_dir/r0-drive.sh" on off panel_on panel_off scrim_on scrim_off stall_on stall_off
  on="call debug r0Spinner true"
  off="call debug r0Spinner false"
  panel_on="call debug r0Panel open"
  panel_off="call debug r0Panel close"
  scrim_on="call debug r0Scrim true"
  scrim_off="call debug r0Scrim false"
  stall_on="call debug r0Panel open"
  stall_off="call debug r0Panel close"
  write_script "$script" <<EOF
#!/usr/bin/env bash
call() { $ipc call "\$@" >> "$shot_dir/r0-ipc.log" 2>&1; }
mark() { echo "\$1 \$(date +%s%N)" >> "$r0_marks_path"; }
# Fields after the comm's closing paren: utime and stime are the 12th and
# 13th, cutime and cstime the 14th and 15th.
ticks() { sed 's/.*) //' "/proc/\$pid/stat" | awk '{print \$12 + \$13, \$14 + \$15}'; }
cpu() {
  local t0 c0 t1 c1
  t0=\$(date +%s%N); c0=\$(ticks)
  sleep 60
  t1=\$(date +%s%N); c1=\$(ticks)
  echo "cpu \$1 self_ticks=\$(( \${c1% *} - \${c0% *} )) child_ticks=\$(( \${c1#* } - \${c0#* } )) wall_ns=\$(( t1 - t0 )) clk_tck=\$(getconf CLK_TCK)" >> "$r0_results_path"
}
for _ in \$(seq 100); do
  pid=\$(cat "$shot_dir/shell.pid" 2>/dev/null) && [ -r "/proc/\$pid/stat" ] && break
  sleep 0.1
done
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=com.t3tools.T3Code sh -c 'sleep infinity']==])" > "$shot_dir/r0-dispatch.txt" 2>&1
start=\$(cut -d' ' -f1 "$r0_start_path")
sleep 20
mark idle; cpu idle
$on; sleep 3
mark spinner; cpu spinner
$off; sleep 3
for i in \$(seq 10); do
  mark panel-open-\$i; $panel_on; sleep 1.5
  mark panel-close-\$i; $panel_off; sleep 1.5
done
for i in \$(seq 10); do
  mark scrim-on-\$i; $scrim_on; sleep 1.2
  mark scrim-off-\$i; $scrim_off; sleep 1.2
done
$on; sleep 3
for i in \$(seq 10); do
  mark stall-open-\$i; $stall_on; sleep 1.5
  mark stall-close-\$i; $stall_off; sleep 1.5
done
$off
mark stall-end
while [ \$(( (\$(date +%s%N) - start) / 1000000000 )) -lt $(leg_arg r0_measure) ]; do sleep 1; done
mark rss
echo "rss \$(awk '/^(VmRSS|VmHWM|Threads):/{printf "%s=%s ", \$1, \$2}' "/proc/\$pid/status")" >> "$r0_results_path"
touch "$r0_done_path"
EOF
  hypr_exec_once "bash $script"
}

leg_r0_measure_assert() {
  [ -f "$r0_done_path" ] || fail "--r0-measure: the drive never reached its RSS sample"
  # The panel and scrim steps off the shell's own log: per open, the call
  # reaching the shell to the surface's first commit, and the slowest render
  # of any commit before the next call. The first ten panel opens are the
  # panel step; the stall step's ten come after them.
  awk '
    function t(line) { match(line, /t=[0-9]+ms/); return substr(line, RSTART + 2, RLENGTH - 4) + 0 }
    function us(line) { if (!match(line, /render_us=[0-9]+/)) return 0; return substr(line, RSTART + 10, RLENGTH - 10) + 0 }
    /^ipc t=/ && /function: "r0Panel"/ { if (/"open"/ && ++np <= 10) po[np] = t($0); else if (np <= 10 && !(np in pe)) pe[np] = t($0) }
    /^ipc t=/ && /function: "r0Scrim"/ { if (/"true"/ && ++ns <= 10) so[ns] = t($0); else if (ns <= 10 && !(ns in se)) se[ns] = t($0) }
    /^commit surface=panel / { pc[++a] = t($0); pr[a] = us($0) }
    /^commit surface=scrim / { sc[++b] = t($0); sr[b] = us($0) }
    function step(name, n, opens, ends, ct, cr, m,   i, j, first, lat, k, worst, x, y) {
      k = 0; worst = 0
      for (i = 1; i <= n; i++) {
        first = -1
        for (j = 1; j <= m; j++) {
          if (ct[j] < opens[i] || (i in ends && ct[j] > ends[i])) continue
          if (first < 0) first = ct[j] - opens[i]
          if (cr[j] > worst) worst = cr[j]
        }
        if (first >= 0) lat[++k] = first
      }
      for (x = 2; x <= k; x++) for (y = x; y > 1 && lat[y - 1] > lat[y]; y--) { first = lat[y]; lat[y] = lat[y - 1]; lat[y - 1] = first }
      printf "%s opens=%d committed=%d first_commit_ms_p50=%d first_commit_ms_max=%d render_us_max=%d\n", name, n, k, (k ? lat[int((k + 1) / 2)] : -1), (k ? lat[k] : -1), worst
    }
    END { step("panel", np > 10 ? 10 : np, po, pe, pc, pr, a); step("scrim", ns > 10 ? 10 : ns, so, se, sc, sr, b) }
  ' "$shell_log_path" >> "$r0_results_path"
  cat "$r0_results_path"
  echo "SMOKE_R0_RESULTS $r0_results_path"
  echo "SMOKE_R0_MARKS $r0_marks_path"
  echo "SMOKE_R0_START $r0_start_path"
  echo "SMOKE_R0_SHELL_LOG $shell_log_path"
}
