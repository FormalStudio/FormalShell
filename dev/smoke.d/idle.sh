# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --idle: the rust rewrite spec's idle and cold start budgets, read under
# whatever settings the scaffold writes (the default bar, every cell it
# ships). Cold start is the launch stamp to the bar's first commit, with
# the shell's own `phase` lines between. Idle is 60 s with nothing driven,
# 20 s after the launch: CPU ticks and context switches of every thread
# (a voluntary switch is a sleep, so each one is a wakeup), the frame
# callbacks the shell asked for, and the commits it made (the clock's
# minute tick counted apart). Over the same window, GPU time off
# /proc/<pid>/fdinfo: the shell's drm-engine-* time and the DRM fds it
# holds, and the nested Hyprland's engine time, the compositing the shell's
# surfaces cost. Under FS_CPU_QUOTA this is the e1504g power saver stand-in.
leg_idle_flag="--idle"
leg_idle_order=901
leg_idle_needs=""

idle_results_path="$shot_dir/idle-results.txt"
idle_start_path="$shot_dir/shell-start.ns"
idle_done_path="$shot_dir/idle-done"

# perf for a profile of the spin, where the host has it; the run goes on
# without one.
leg_idle_fixture() {
  idle_perf=$(command -v perf 2>/dev/null || true)
}

leg_idle_shell() {
  local software=""
  if [ "$session_mode" = vkms ]; then software="export LIBGL_ALWAYS_SOFTWARE=1"; fi
  write_script "$1" <<EOF
#!/usr/bin/env bash
$software
date '+%s%N' > "$idle_start_path"
$shell_prefix "$shell_bin" > "$shell_log_path" 2>&1 &
echo \$! > "$shot_dir/shell.pid"
wait
EOF
}

leg_idle_timing() {
  leg_timing 180 215
}

leg_idle_drive() {
  local script="$shot_dir/idle-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
# Per thread: name, utime+stime ticks, voluntary and involuntary switches.
sample() {
  for t in /proc/\$pid/task/*; do
    name=\$(cat "\$t/comm" 2>/dev/null) || continue
    ticks=\$(sed 's/.*) //' "\$t/stat" | awk '{print \$12 + \$13}')
    vol=\$(awk '/^voluntary_ctxt_switches/{print \$2}' "\$t/status")
    inv=\$(awk '/^nonvoluntary_ctxt_switches/{print \$2}' "\$t/status")
    echo "\${t##*/} \$name \$ticks \$vol \$inv"
  done
}
# drm-engine-* ns summed over every engine of every DRM client the process
# holds, then the count of its fds open on a /dev/dri node. Dup'd fds share
# a drm-client-id and report the same counters, so each client counts once.
gpu() {
  local f
  for f in /proc/\$1/fdinfo/*; do
    case "\$(readlink "/proc/\$1/fd/\${f##*/}" 2>/dev/null)" in /dev/dri/*) echo fd ;; *) continue ;; esac
    awk '/^drm-client-id:/ { id = \$2 } /^drm-engine-/ && !/^drm-engine-capacity-/ { print "engine", id, \$1, \$2 }' "\$f" 2>/dev/null
  done | awk '\$1 == "fd" { n++; next } { e[\$2 " " \$3] = \$4 } END { for (k in e) t += e[k]; printf "%d %d\\n", t, n }'
}
for _ in \$(seq 100); do
  pid=\$(cat "$shot_dir/shell.pid" 2>/dev/null) && [ -r "/proc/\$pid/stat" ] && break
  sleep 0.1
done
hypr=""
for p in \$(pgrep -f -- "--config $cfg"); do
  [ "\$(cat "/proc/\$p/comm" 2>/dev/null)" = Hyprland ] && hypr=\$p
done
sleep 20
sample > "$shot_dir/idle-before.txt"
read -r g0 _ < <(gpu "\$pid")
read -r h0 _ < <(gpu "\${hypr:-0}")
c0=\$(grep -c '^commit ' "$shell_log_path")
echo "idle-start \$(date +%s%N)" > "$shot_dir/idle-marks.txt"
sleep 60
sample > "$shot_dir/idle-after.txt"
read -r g1 drm_fds < <(gpu "\$pid")
read -r h1 _ < <(gpu "\${hypr:-0}")
c1=\$(grep -c '^commit ' "$shell_log_path")
echo "idle-end \$(date +%s%N)" >> "$shot_dir/idle-marks.txt"
awk 'NR == FNR { t[\$1] = \$3; v[\$1] = \$4; i[\$1] = \$5; next }
  { dt = \$3 - t[\$1]; dv = \$4 - v[\$1]; di = \$5 - i[\$1]; tt += dt; tv += dv
    printf "thread %s %s ticks=%d wakeups=%d preempted=%d\n", \$1, \$2, dt, dv, di }
  END { printf "total ticks=%d wakeups=%d clk_tck=%d\n", tt, tv, '"\$(getconf CLK_TCK)"' }' \
  "$shot_dir/idle-before.txt" "$shot_dir/idle-after.txt" > "$idle_results_path"
# The clock's cell redraws on the wall clock's minute (services/clock.rs),
# so a bar commit within a second of a minute is its tick (one has logged at
# 59.999s, the timer and the log's whole milliseconds rounding apart); every
# other commit is the shell spending a frame on nothing.
zero_ms=\$(awk '/^start epoch_us=/{ split(\$2, a, "="); print int(a[2] / 1000); exit }' "$shell_log_path")
grep '^commit ' "$shell_log_path" \\
  | awk -v zero="\$zero_ms" -v c0="\$c0" -v c1="\$c1" 'NR <= c0 || NR > c1 { next } { match(\$0, /t=[0-9]+ms/); w = zero + substr(\$0, RSTART + 2, RLENGTH - 4)
      x = w % 60000; if (\$2 == "surface=bar" && (x < 1000 || x > 59000)) clock++; else other++ }
    END { printf "commits %d clock=%d\\n", other, clock }' >> "$idle_results_path"
if [ -n "\$hypr" ]; then
  echo "gpu shell_engine_ns=\$((g1 - g0)) shell_drm_fds=\$drm_fds compositor_engine_ns=\$((h1 - h0)) compositor_pid=\$hypr" >> "$idle_results_path"
else
  echo "gpu shell_engine_ns=\$((g1 - g0)) shell_drm_fds=\$drm_fds compositor_engine_ns=none" >> "$idle_results_path"
fi
# Then 20 s under strace, after the sample so its cost is not counted: the
# syscall each wakeup came back from names its waker.
if command -v strace > /dev/null; then
  st=strace; [ "\$(cat /proc/sys/kernel/yama/ptrace_scope 2>/dev/null)" = 0 ] || st="sudo -n strace"
  timeout -s INT 20 \$st -f -tt -T -p "\$pid" -o "$shot_dir/idle-strace.log" > /dev/null 2>&1 || true
fi
# The agent badge spinning for 30 s: the cost of one 11x11 cell per frame.
$ipc call debug r0Spinner true > /dev/null 2>&1
sleep 2
sample > "$shot_dir/spin-before.txt"
s0=\$(grep -c '^commit surface=bar ' "$shell_log_path")
sleep 30
sample > "$shot_dir/spin-after.txt"
s1=\$(grep -c '^commit surface=bar ' "$shell_log_path")
if [ -n "${idle_perf:-}" ]; then
  sudo -n "$idle_perf" record -F 1999 -g -p "\$pid" -o "$shot_dir/spin.perf" -- sleep 10 > /dev/null 2>&1 || true
  sudo -n "$idle_perf" report -i "$shot_dir/spin.perf" --stdio --no-children -g none --percent-limit 0.5 2> /dev/null \
    | head -80 > "$shot_dir/spin-perf.log" || true
fi
$ipc call debug r0Spinner false > /dev/null 2>&1
awk 'NR == FNR { t[\$1] = \$3; next } { d = \$3 - t[\$1]; tt += d; if (d > 0) printf "spin thread %s %s ticks=%d\n", \$1, \$2, d }
  END { printf "spin total ticks=%d over 30s\n", tt }' "$shot_dir/spin-before.txt" "$shot_dir/spin-after.txt" >> "$idle_results_path"
echo "spin bar commits \$((s1 - s0))" >> "$idle_results_path"
# One screensaver cycle: what it takes while it runs has to come back.
rss() { awk '/^VmRSS/{print \$2}' "/proc/\$pid/status"; }
r0=\$(rss)
$ipc call screensaver start > /dev/null 2>&1
sleep 8
r1=\$(rss)
$ipc call screensaver stop > /dev/null 2>&1
sleep 4
echo "rss_kb before_saver=\$r0 saver=\$r1 after_saver=\$(rss)" >> "$idle_results_path"
touch "$idle_done_path"
EOF
  hypr_exec_once "bash $script"
}

leg_idle_assert() {
  [ -f "$idle_done_path" ] || fail "--idle: the drive never finished its sample"
  local start first
  start=$(cat "$idle_start_path")
  first=$(awk '/^commit surface=bar n=1 /{match($0, /t=[0-9]+ms/); print substr($0, RSTART + 2, RLENGTH - 4); exit}' "$shell_log_path")
  awk -v start="$start" '/^start epoch_us=/{ split($2, a, "="); printf "SMOKE_IDLE launch_to_zero_ms=%d\n", (a[2] * 1000 - start) / 1000000 }' "$shell_log_path"
  grep -E '^phase ' "$shell_log_path" | sed 's/^/SMOKE_IDLE /'
  echo "SMOKE_IDLE first_bar_commit_ms=${first:-none}"
  sed 's/^/SMOKE_IDLE /' "$idle_results_path"
  grep -q '^gpu .*compositor_engine_ns=[0-9]' "$idle_results_path" \
    || fail "--idle: found no Hyprland carrying this run's config to read the compositor's engine time from"
  echo "SMOKE_IDLE_LOG $shell_log_path"
  if [ -s "$shot_dir/idle-strace.log" ]; then echo "SMOKE_IDLE_STRACE $shot_dir/idle-strace.log"; fi
  if [ -s "$shot_dir/spin-perf.log" ]; then echo "SMOKE_IDLE_SPIN_PERF $shot_dir/spin-perf.log"; fi
}
