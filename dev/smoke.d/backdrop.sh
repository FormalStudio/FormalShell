# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --backdrop rides --cartridge: the shard sculpture over the desktop
# (`wallpaper.animate`, the preset's default) on one timeline, read off
# `debug dump`'s `backdrop` block, the shell's own `commit surface=shards`
# lines and the frames.
#
# With nothing on the desktop it runs: three frames a second apart differ
# inside the sculpture's half of the screen, and 20 s of it give the
# budget, the shell's CPU off /proc/<pid>/stat as a share of one core and
# the frame rate off the commit lines, never past 24. A foot window made
# fullscreen holds it with no commit in 5 s (`paused` "fullscreen"); the
# same window back to tiled covers the whole output, so it still holds
# ("covered", cover past 0.85); moved to workspace 2 it runs again and the
# frames move. `debug motionScale 1000` slows its own clock to a tenth while
# the beat stays at 24 a second. Under FS_CPU_QUOTA (the e1504g power
# saver stand-in) the budget is 10% of a core and the leg fails past it; the
# plain run prints its number, which stands in for nothing on real hardware.
leg_backdrop_flag="--backdrop"
leg_backdrop_order=212
leg_backdrop_needs="convert jq foot"

backdrop_results="$shot_dir/backdrop-results.txt"
backdrop_done="$shot_dir/backdrop-done"

leg_backdrop_validate() {
  if ! leg_on cartridge; then
    echo "usage: $0 --cartridge --backdrop (the sculpture is the cartridge preset's)" >&2
    exit 1
  fi
}

leg_backdrop_timing() {
  leg_timing 84 120
}

leg_backdrop_drive() {
  local script="$shot_dir/backdrop-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
out="$backdrop_results"
: > "\$out"
for _ in \$(seq 100); do
  pid=\$(cat "$shot_dir/shell.pid" 2>/dev/null) && [ -r "/proc/\$pid/stat" ] && break
  sleep 0.1
done
ticks() { sed 's/.*) //' "/proc/\$pid/stat" | awk '{print \$12 + \$13}'; }
commits() { grep -c '^commit surface=shards ' "$shell_log_path"; }
dump() { $ipc call debug dump > "$shot_dir/backdrop-\$1.json" 2>&1; }
# The commits made over \$2 seconds, as "<phase> commits=<n>".
quiet() { local c0 c1; c0=\$(commits); sleep "\$2"; c1=\$(commits); echo "\$1 commits=\$((c1 - c0)) seconds=\$2" >> "\$out"; }
sleep 8
dump running
"$grim_bin" "$shot_dir/backdrop-run-1.png"; sleep 1
"$grim_bin" "$shot_dir/backdrop-run-2.png"; sleep 1
"$grim_bin" "$shot_dir/backdrop-run-3.png"
t0=\$(ticks); c0=\$(commits); s0=\$(date +%s%N)
sleep 20
t1=\$(ticks); c1=\$(commits); s1=\$(date +%s%N)
echo "budget ticks=\$((t1 - t0)) commits=\$((c1 - c0)) ns=\$((s1 - s0)) clk_tck=\$(getconf CLK_TCK)" >> "\$out"
dump budget
# A tenth of the clock's speed, the beat unchanged.
k0=\$("$jq_bin" '.backdrop[0].clock' "$shot_dir/backdrop-budget.json")
$ipc call debug motionScale 1000 > /dev/null 2>&1
quiet slowed 4
dump slowed
$ipc call debug motionScale 100 > /dev/null 2>&1
k1=\$("$jq_bin" '.backdrop[0].clock' "$shot_dir/backdrop-slowed.json")
echo "slowclock clock_ms=\$(awk -v a="\$k0" -v b="\$k1" 'BEGIN { print b - a }')" >> "\$out"
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-backdrop --title='formalshell backdrop' sh -c 'sleep 300']==])" > /dev/null 2>&1
sleep 3
"$hyprctl_bin" dispatch "hl.dsp.window.fullscreen({ mode = 'fullscreen', action = 'set', window = 'class:formalshell-backdrop' })" > /dev/null 2>&1
sleep 2
quiet fullscreen 5
dump fullscreen
"$hyprctl_bin" -j clients > "$shot_dir/backdrop-fullscreen-clients.json" 2>&1
"$hyprctl_bin" dispatch "hl.dsp.window.fullscreen({ mode = 'fullscreen', action = 'unset', window = 'class:formalshell-backdrop' })" > /dev/null 2>&1
sleep 2
quiet covered 5
dump covered
"$grim_bin" "$shot_dir/backdrop-covered.png"
"$hyprctl_bin" dispatch "hl.dsp.window.move({ workspace = 2, follow = false, window = 'class:formalshell-backdrop' })" > /dev/null 2>&1
sleep 2
quiet resumed 5
dump resumed
"$grim_bin" "$shot_dir/backdrop-resumed-1.png"; sleep 1
"$grim_bin" "$shot_dir/backdrop-resumed-2.png"
touch "$backdrop_done"
EOF
  hypr_exec_once "bash $script"
}

# Pixels differing between two frames inside the sculpture's side of the
# screen, past the bar.
_backdrop_diff() {
  local w h
  read -r w h < <($convert_bin "$1" -format '%w %h' info:)
  $convert_bin "$1" "$2" -crop "$((w * 55 / 100))x$((h * 80 / 100))+$((w * 45 / 100))+$((h * 10 / 100))" +repage \
    -compose difference -composite -colorspace Gray -threshold 3% -format '%[fx:mean*w*h]' info: 2>/dev/null | awk '{printf "%d", $1}'
}

_backdrop_field() {
  "$jq_bin" -r ".backdrop[0].$2" "$shot_dir/backdrop-$1.json" 2>/dev/null
}

_backdrop_commits() {
  awk -v p="$1" '$1 == p { sub("commits=", "", $2); print $2 }' "$backdrop_results"
}

leg_backdrop_assert() {
  echo "SMOKE_BACKDROP_LOG $shell_log_path"
  [ -f "$backdrop_done" ] || fail "--backdrop: the drive never finished"
  sed 's/^/SMOKE_BACKDROP /' "$backdrop_results"
  local phase
  for phase in running budget slowed fullscreen covered resumed; do
    [ -s "$shot_dir/backdrop-$phase.json" ] || fail "--backdrop: no dump for $phase"
    echo "SMOKE_BACKDROP $phase $("$jq_bin" -c '.backdrop[0]' "$shot_dir/backdrop-$phase.json")"
  done
  [ "$(_backdrop_field running animate)" = true ] || fail "--backdrop: the cartridge preset made no sculpture"
  [ "$(_backdrop_field running running)" = true ] || fail "--backdrop: the sculpture is not running on a bare desktop ($(_backdrop_field running paused))"

  local d12 d23
  d12=$(_backdrop_diff "$shot_dir/backdrop-run-1.png" "$shot_dir/backdrop-run-2.png")
  d23=$(_backdrop_diff "$shot_dir/backdrop-run-2.png" "$shot_dir/backdrop-run-3.png")
  echo "SMOKE_BACKDROP frames a second apart differ by $d12 and $d23 pixels"
  [ "${d12:-0}" -gt 500 ] && [ "${d23:-0}" -gt 500 ] || fail "--backdrop: the sculpture did not move between frames ($d12, $d23)"

  local ticks commits ns clk cpu fps
  read -r ticks commits ns clk < <(awk '$1 == "budget" { for (i = 2; i <= NF; i++) { split($i, kv, "="); v[kv[1]] = kv[2] } print v["ticks"], v["commits"], v["ns"], v["clk_tck"] }' "$backdrop_results")
  cpu=$(awk -v t="$ticks" -v n="$ns" -v c="$clk" 'BEGIN { printf "%.1f", t / c / (n / 1e9) * 100 }')
  fps=$(awk -v k="$commits" -v n="$ns" 'BEGIN { printf "%.1f", k / (n / 1e9) }')
  echo "SMOKE_BACKDROP_BUDGET cpu_pct=$cpu fps=$fps render_us=$(_backdrop_field budget renderUs) quota=${FS_CPU_QUOTA:-none}"
  awk -v f="$fps" 'BEGIN { exit !(f <= 24.5) }' || fail "--backdrop: $fps frames a second, past the 24 cap"
  awk -v f="$fps" 'BEGIN { exit !(f >= 5) }' || fail "--backdrop: $fps frames a second, the sculpture is starving"
  if [ -n "${FS_CPU_QUOTA:-}" ]; then
    awk -v c="$cpu" 'BEGIN { exit !(c < 10) }' || fail "--backdrop: $cpu% of a core under FS_CPU_QUOTA, past the 10% budget"
  fi

  local slowed_ms slowed_commits
  slowed_ms=$(awk '$1 == "slowclock" { sub("clock_ms=", "", $2); print $2 }' "$backdrop_results")
  slowed_commits=$(_backdrop_commits slowed)
  awk -v m="${slowed_ms:-0}" 'BEGIN { exit !(m > 100 && m < 1200) }' \
    || fail "--backdrop: under motionScale 1000 the clock moved ${slowed_ms}ms in 4s, not about 400"
  [ "${slowed_commits:-0}" -ge 20 ] || fail "--backdrop: the beat stopped under motionScale ($slowed_commits commits in 4s)"

  local fs_windows
  fs_windows=$("$jq_bin" -r '[.[] | select(.fullscreen != 0 and .fullscreen != false)] | length' "$shot_dir/backdrop-fullscreen-clients.json" 2>/dev/null)
  [ "${fs_windows:-0}" -ge 1 ] || fail "--backdrop: no window was fullscreen, so the pause went untested"
  [ "$(_backdrop_commits fullscreen)" = 0 ] || fail "--backdrop: $(_backdrop_commits fullscreen) sculpture commits under a fullscreen window"
  [ "$(_backdrop_field fullscreen paused)" = fullscreen ] || fail "--backdrop: under fullscreen it reports '$(_backdrop_field fullscreen paused)'"

  [ "$(_backdrop_commits covered)" = 0 ] || fail "--backdrop: $(_backdrop_commits covered) sculpture commits under a covering tiled window"
  [ "$(_backdrop_field covered paused)" = covered ] || fail "--backdrop: under a tiled window it reports '$(_backdrop_field covered paused)' at cover $(_backdrop_field covered cover)"
  awk -v c="$(_backdrop_field covered cover)" 'BEGIN { exit !(c > 0.85) }' || fail "--backdrop: the tiled window covers only $(_backdrop_field covered cover)"

  [ "$(_backdrop_field resumed running)" = true ] || fail "--backdrop: it never ran again with the window gone ($(_backdrop_field resumed paused))"
  [ "$(_backdrop_commits resumed)" -ge 20 ] || fail "--backdrop: $(_backdrop_commits resumed) commits in 5s after it resumed"
  local dr
  dr=$(_backdrop_diff "$shot_dir/backdrop-resumed-1.png" "$shot_dir/backdrop-resumed-2.png")
  [ "${dr:-0}" -gt 500 ] || fail "--backdrop: the resumed frames did not move ($dr pixels)"

  echo "SMOKE_BACKDROP_RUN_1 $shot_dir/backdrop-run-1.png"
  echo "SMOKE_BACKDROP_RUN_2 $shot_dir/backdrop-run-2.png"
  echo "SMOKE_BACKDROP_RUN_3 $shot_dir/backdrop-run-3.png"
  echo "SMOKE_BACKDROP_COVERED $shot_dir/backdrop-covered.png"
  echo "SMOKE_BACKDROP_RESUMED $shot_dir/backdrop-resumed-1.png"
}
