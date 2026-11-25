# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --sidebar-pacing: what an animated frame of the sidebar costs on g815's
# tall panel, read off the shell's commit log. Under `layout: sidebar` a
# headless second output is made at DP-3's shape (2560x1600 at 1.6, 144 Hz)
# beside the session's own, so the column there is 154 by 1000 logical,
# 247 by 1600 pixels. Two foot windows on its workspace and one on another
# give the map rows. Each step sits between a `menu ping` and the status
# read after it, as in --anim-pacing, and drives one of the column's
# animations: focus leaving that output for the session's own and coming
# back (the live bar moving between the two windows), a workspace switch
# there and back (the map's plate), a window opening and one closing on
# it (a row growing and shrinking), the network panel opening off the
# column and closing, a notification arriving (the notifications card
# appearing, pushing the cards under it), and the column folding to the
# rail and opening again. Focus moving between the window beside the
# column and one past it changes the light the reactive rims take off it,
# and every card's rim blends over (`Rim::aim`). Six more windows, on
# workspaces of their own, fill the map to the column's height.
#
# Claims, per step, over the bar commits on that output (`buffer_px`
# 1600 tall) and on the session's own:
#   - every step's frames on the tall output: at least one commit;
#   - no bar frame on the tall output past the first two of a step costs
#     more than half its refresh interval (3.47ms at 144 Hz) in
#     `frame_cpu_us`, the shell thread's CPU time from the frame callback
#     landing to the commit: whatever ran ahead of the bar that turn, its
#     layout or step in place, its draw and the copy or GPU submit (the
#     wall clock's `frame_us` carries the loaded rig's preemption too), or
#     in `cpu_us` where that is more: the layout, draw and copy alone, part
#     of which runs ahead of the callback when an event laid the bar out
#     while its last frame was still out, and all of it for a frame no
#     callback started. With the shell
#     pinned to the performance cores `/sys/devices/cpu_core/cpus` lists
#     (g815's 0-7, where its desktop runs the shell; a slot's slice otherwise
#     lands it on the efficiency cores at under half the speed); the first
#     two pay for a new surface's canvas and buffers when the live bar
#     moves;
#   - both outputs' bar frames committed within one interval of each
#     other together under one interval, in the same CPU time;
#   - a commit that asked for the next frame followed within two intervals
#     for every callback between them, as --anim-pacing reads it, unless
#     that callback found nothing to draw (the next frame's `react_us` 0:
#     an event started it, not a callback).
# Not read under FS_CPU_QUOTA, whose throttling stretches every number.
# Each step's commit count, median and worst cost and its worst frame's
# parts and damage are printed beside it. Then, untimed, the rim blends
# and the fold run again under the stale audit (`stale_audit_*`), since
# a blend's frames repaint the rims in place, with no layout.
# FS_SIDEBAR_PACING_PERF=1 records the shell under perf through the timed
# steps (LBR call stacks: the release build carries no frame pointers) and
# pulls three reports: the whole shell's, the bar's present alone, and each
# hot function's own time under its callers.
leg_sidebar_pacing_flag="--sidebar-pacing"
leg_sidebar_pacing_order=128
leg_sidebar_pacing_claims="session"
leg_sidebar_pacing_budget="frame costs against a 144 Hz interval are g815's numbers"
leg_sidebar_pacing_needs="foot jq notify-send mpv ffmpeg taskset"
leg_sidebar_pacing_env="FS_SIDEBAR_PACING_PERF"

sidebar_pacing_second="HEADLESS-PACING"
sidebar_pacing_windows="$shot_dir/sidebar-pacing-windows.txt"
sidebar_pacing_monitors="$shot_dir/sidebar-pacing-monitors.json"
sidebar_pacing_log="$shot_dir/sidebar-pacing.log"
sidebar_pacing_png="$shot_dir/sidebar-pacing.png"
sidebar_pacing_perf="$shot_dir/sidebar-pacing-perf.data"
sidebar_pacing_track="$shot_dir/sidebar-pacing-track.flac"

leg_sidebar_pacing_fixture() {
  settings_pin '.layout = "sidebar"'
  "$ffmpeg_bin" -nostdin -loglevel error -f lavfi -i "anullsrc=r=48000:cl=stereo" -t 30 \
    -metadata "title=Sidebar Pacing" -metadata "artist=FormalShell" -c:a flac -y "$sidebar_pacing_track"
}

leg_sidebar_pacing_timing() {
  leg_timing 80
}

leg_sidebar_pacing_drive() {
  local script="$shot_dir/sidebar-pacing-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
for _ in \$(seq 1 40); do
  $ipc call sidebar status 2>/dev/null | grep -q '"cards"' && break
  sleep 0.5
done
own=\$("$wmctl_bin" -j monitors | "$jq_bin" -r '.[0].name')
width=\$("$wmctl_bin" -j monitors | "$jq_bin" -r '.[0].width')
"$wmctl_bin" eval "hl.monitor({ output = \\"$sidebar_pacing_second\\", mode = \\"2560x1600@144\\", position = \\"\${width}x0\\", scale = 1.6 })" >> "$sidebar_pacing_log" 2>&1
"$wmctl_bin" output create headless "$sidebar_pacing_second" >> "$sidebar_pacing_log" 2>&1
sleep 2
"$wmctl_bin" dispatch "hl.dsp.focus({ monitor = '$sidebar_pacing_second' })" >> "$sidebar_pacing_log" 2>&1
sleep 1
ws=\$("$wmctl_bin" -j monitors | "$jq_bin" -r --arg o "$sidebar_pacing_second" '.[] | select(.name == \$o) | .activeWorkspace.id')
other=\$((ws + 1))
for i in 1 2; do
  "$wmctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-sidebar-pacing-\$i sh -c 'sleep 300']==])" >> "$sidebar_pacing_log" 2>&1
done
"$wmctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-sidebar-pacing-3 sh -c 'sleep 300']==], { workspace = '\$other silent' })" >> "$sidebar_pacing_log" 2>&1
for i in 5 6 7 8 9 10; do
  "$wmctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-sidebar-pacing-\$i sh -c 'sleep 300']==], { workspace = '\$((ws + i - 3)) silent' })" >> "$sidebar_pacing_log" 2>&1
done
"$mpv_bin" --no-video --really-quiet --loop-file=inf "$sidebar_pacing_track" &
sidebar_pacing_mpv=\$!
sleep 3
"$wmctl_bin" -j monitors > "$sidebar_pacing_monitors" 2>&1
"$wmctl_bin" -j clients > "$shot_dir/sidebar-pacing-clients.log" 2>&1
# The window beside the column and the one past the rims' reach.
near=\$("$jq_bin" -r --argjson ws "\$ws" '[.[] | select(.workspace.id == \$ws)] | sort_by(.at[0]) | first | .class' "$shot_dir/sidebar-pacing-clients.log")
far=\$("$jq_bin" -r --argjson ws "\$ws" '[.[] | select(.workspace.id == \$ws)] | sort_by(.at[0]) | last | .class' "$shot_dir/sidebar-pacing-clients.log")
"$wmctl_bin" dispatch "hl.dsp.focus({ window = 'class:\$near' })" > /dev/null 2>&1
sleep 1
park
# The shell on g815's performance cores, where its own desktop runs it: a
# slot's slice lands it on the efficiency cores at under half the speed.
pcores=\$(cat /sys/devices/cpu_core/cpus 2>/dev/null)
if [ -n "\$pcores" ]; then
  "$taskset_bin" -a -p -c "\$pcores" "\$(cat "$shot_dir/shell.pid")" >> "$sidebar_pacing_log" 2>&1
  echo "\$pcores" > "$shot_dir/sidebar-pacing-cores.txt"
fi
if [ -n "\${FS_SIDEBAR_PACING_PERF:-}" ]; then
  perf record -F 4000 --call-graph lbr -e cpu_core/cycles/P -o "$sidebar_pacing_perf" -p "\$(cat "$shot_dir/shell.pid")" -- sleep 120 >> "$sidebar_pacing_log" 2>&1 &
  sidebar_pacing_perf_pid=\$!
  sleep 1
fi
win() {
  echo "\$1" >> "$sidebar_pacing_windows"
  $ipc call menu ping > /dev/null 2>&1
}
done_() {
  sleep "\$1"
  $ipc call sidebar status > /dev/null 2>&1
}
win focus-away
"$wmctl_bin" dispatch "hl.dsp.focus({ monitor = '\$own' })" > /dev/null 2>&1
done_ 1.2
win focus-back
"$wmctl_bin" dispatch "hl.dsp.focus({ monitor = '$sidebar_pacing_second' })" > /dev/null 2>&1
done_ 1.2
win focus-far
"$wmctl_bin" dispatch "hl.dsp.focus({ window = 'class:\$far' })" > /dev/null 2>&1
done_ 1.2
win focus-near
"$wmctl_bin" dispatch "hl.dsp.focus({ window = 'class:\$near' })" > /dev/null 2>&1
done_ 1.2
win focus-far-again
"$wmctl_bin" dispatch "hl.dsp.focus({ window = 'class:\$far' })" > /dev/null 2>&1
done_ 1.2
win window-away
"$wmctl_bin" dispatch "hl.dsp.window.move({ workspace = \$other, follow = false, window = 'class:\$near' })" > /dev/null 2>&1
done_ 1.2
win workspace-next
"$wmctl_bin" dispatch "hl.dsp.focus({ workspace = \$other })" > /dev/null 2>&1
done_ 1.2
win workspace-back
"$wmctl_bin" dispatch "hl.dsp.focus({ workspace = \$ws })" > /dev/null 2>&1
done_ 1.2
win window-open
"$wmctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-sidebar-pacing-4 sh -c 'sleep 300']==])" > /dev/null 2>&1
done_ 1.5
win window-close
"$wmctl_bin" dispatch "hl.dsp.window.close({ window = 'class:formalshell-sidebar-pacing-4' })" > /dev/null 2>&1
done_ 1.5
win panel-open
$ipc call panel open network > /dev/null 2>&1
done_ 1.2
win panel-close
$ipc call panel close > /dev/null 2>&1
done_ 1.2
win toast
"$notify_send_bin" -u normal -t 60000 "Sidebar pacing" 'One notification' > /dev/null 2>&1
done_ 1.5
win collapse
$ipc call sidebar collapse > /dev/null 2>&1
done_ 1.2
win expand
$ipc call sidebar expand > /dev/null 2>&1
done_ 1.2
if [ -n "\${sidebar_pacing_perf_pid:-}" ]; then kill -INT "\$sidebar_pacing_perf_pid"; wait "\$sidebar_pacing_perf_pid"; fi
$(stale_audit_start)
for target in "\$far" "\$near" "\$far"; do
  "$wmctl_bin" dispatch "hl.dsp.focus({ window = 'class:\$target' })" > /dev/null 2>&1
  sleep 0.8
done
$ipc call sidebar collapse > /dev/null 2>&1
sleep 0.8
$ipc call sidebar expand > /dev/null 2>&1
sleep 0.8
$(stale_audit_stop sidebar-pacing)
"$grim_bin" -o "$sidebar_pacing_second" "$sidebar_pacing_png" > /dev/null 2>&1
kill "\$sidebar_pacing_mpv" 2>/dev/null
wait
"$wmctl_bin" output remove "$sidebar_pacing_second" >> "$sidebar_pacing_log" 2>&1
EOF
  wm_exec_once "bash $script"
}

leg_sidebar_pacing_assert() {
  local refresh interval report
  refresh=$("$jq_bin" -r --arg o "$sidebar_pacing_second" '.[] | select(.name == $o) | .refreshRate' "$sidebar_pacing_monitors" 2>/dev/null)
  [ -n "$refresh" ] || fail "no refresh rate for $sidebar_pacing_second off monitors -j: $(cat "$sidebar_pacing_monitors" "$sidebar_pacing_log")"
  [ -s "$sidebar_pacing_windows" ] || fail "the drive wrote no steps"
  interval=$(awk -v r="$refresh" 'BEGIN { printf "%.3f", 1000 / r }')
  echo "SMOKE_SIDEBAR_PACING_REFRESH ${refresh}Hz, ${interval}ms a frame"
  echo "SMOKE_SIDEBAR_PACING_CORES $(cat "$shot_dir/sidebar-pacing-cores.txt" 2>/dev/null || echo "not pinned, no cpu_core list")"
  if [ -s "$sidebar_pacing_perf" ]; then
    perf report --stdio --children --sort sym -g none --percent-limit 0.8 -i "$sidebar_pacing_perf" > "$shot_dir/sidebar-pacing-perf.log" 2>/dev/null
    perf report --stdio --children --sort sym -g none --percent-limit 0.5 --parent "owner::Bar as" -x -i "$sidebar_pacing_perf" > "$shot_dir/sidebar-pacing-perf-bar.log" 2>/dev/null
    perf report --stdio --no-children --sort sym -g caller,0.5,callee --percent-limit 1.5 -i "$sidebar_pacing_perf" > "$shot_dir/sidebar-pacing-perf-self.log" 2>/dev/null
    echo "SMOKE_SIDEBAR_PACING_PERF $shot_dir/sidebar-pacing-perf.log"
    echo "SMOKE_SIDEBAR_PACING_PERF_SELF $shot_dir/sidebar-pacing-perf-self.log"
    echo "SMOKE_SIDEBAR_PACING_PERF_BAR $shot_dir/sidebar-pacing-perf-bar.log"
  fi
  awk 'FNR == NR { label[++nw] = $1; next } /^ipc t=[0-9]+ms call menu ping/ { w++; open = 1; next } /^ipc t=[0-9]+ms call sidebar status$/ { open = 0; next } open && /^commit surface=bar / { print label[w], $0 }' "$sidebar_pacing_windows" "$shell_log_path" > "$shot_dir/sidebar-pacing-commits.log"
  echo "SMOKE_SIDEBAR_PACING_COMMITS $shot_dir/sidebar-pacing-commits.log"
  if ! report=$(awk -v iv="$interval" -v quota="${FS_CPU_QUOTA:-}" '
    function num(line, key,   m) { if (match(line, " " key "=[0-9]+")) { m = substr(line, RSTART, RLENGTH); sub(/^[^=]*=/, "", m); return m + 0 } return 0 }
    function t(line,   m) { match(line, /t=[0-9]+ms/); m = substr(line, RSTART + 2, RLENGTH - 4); return m + 0 }
    function dmg(line,   m) { match(line, /damage=\[[^]]*\]/); return substr(line, RSTART, RLENGTH) }
    FNR == NR { label[++nw] = $1; next }
    /^ipc t=[0-9]+ms call menu ping/ { w++; open = 1; next }
    /^ipc t=[0-9]+ms call sidebar status$/ { open = 0; next }
    open && /^commit surface=bar / {
      tall = ($0 ~ /buffer_px=[0-9]+x1600/) ? "tall" : "own"
      k = ++n[w, tall]; at[w, tall, k] = t($0)
      c = num($0, "frame_cpu_us"); if (num($0, "cpu_us") > c) c = num($0, "cpu_us")
      cost[w, tall, k] = c; parts[w, tall, k] = sprintf("wall react_us %d build_us %d render_us %d copy_us %d, cpu_us %d, px %d %s", num($0, "react_us"), num($0, "build_us"), num($0, "render_us"), num($0, "copy_us"), num($0, "cpu_us"), num($0, "px"), dmg($0))
      q[w, tall, k] = num($0, "request"); cb[w, tall, k] = num($0, "frame_callbacks"); lw[w, tall, k] = num($0, "last_wait_us"); re[w, tall, k] = num($0, "react_us")
    }
    END {
      bad = 0; half = iv * 500
      if (w < nw) { printf "FAIL only %d steps in the log, not %d\n", w, nw; bad = 1 }
      for (i = 1; i <= nw; i++) {
        if (n[i, "tall"] + 0 == 0) { printf "FAIL %s: no bar frame on the tall output\n", label[i]; bad = 1 }
        for (o = 1; o <= 2; o++) {
          s = (o == 1) ? "tall" : "own"; c = n[i, s] + 0
          if (c == 0) continue
          worst = 0; wk = 0
          for (k = 1; k <= c; k++) { v[k] = cost[i, s, k]; if (k > 2 && v[k] > worst) { worst = v[k]; wk = k } }
          for (a = 1; a <= c; a++) for (b = a + 1; b <= c; b++) if (v[b] < v[a]) { x = v[a]; v[a] = v[b]; v[b] = x }
          med = v[int((c + 1) / 2)]
          gap = 0
          for (k = 1; k < c; k++) if (q[i, s, k] == 1) {
            f = cb[i, s, k + 1] - cb[i, s, k]; d = (at[i, s, k + 1] - at[i, s, k]) / (f > 1 ? f : 1)
            if (lw[i, s, k + 1] > iv * 1000 + 2000) continue
            if (re[i, s, k + 1] == 0) continue
            if (d > gap) gap = d
          }
          printf "%s %s: %d commits, median %dus, worst past the first two %dus", label[i], s, c, med, worst
          if (wk) printf " (%s)", parts[i, s, wk]
          printf ", longest gap a callback %dms\n", gap
          if (s == "tall" && quota == "" && worst > half) { printf "FAIL %s: a bar frame on the tall output cost %dus, past half a %.2fms frame\n", label[i], worst, iv; bad = 1 }
          if (s == "tall" && gap > 2 * iv + 2) { printf "FAIL %s: a %dms gap a callback, past two frames of %.2fms\n", label[i], gap, iv; bad = 1 }
        }
        both = 0
        for (k = 3; k <= n[i, "tall"]; k++) for (j = 1; j <= n[i, "own"]; j++) {
          if (at[i, "own", j] - at[i, "tall", k] > iv || at[i, "tall", k] - at[i, "own", j] > iv) continue
          if (cost[i, "tall", k] + cost[i, "own", j] > both) both = cost[i, "tall", k] + cost[i, "own", j]
        }
        if (both) printf "%s: both outputs within a frame, worst %dus together\n", label[i], both
        if (quota == "" && both > iv * 1000) { printf "FAIL %s: both bars cost %dus within one frame, past %.2fms\n", label[i], both, iv; bad = 1 }
      }
      exit bad
    }' "$sidebar_pacing_windows" "$shell_log_path"); then
    echo "$report"
    fail "sidebar pacing: $(echo "$report" | grep '^FAIL' | head -3 | tr '\n' ' ')"
  fi
  echo "$report" | sed 's/^/SMOKE_SIDEBAR_PACING /'
  echo "SMOKE_SIDEBAR_PACING_FRAME $sidebar_pacing_png"
  stale_audit_assert sidebar-pacing
}
