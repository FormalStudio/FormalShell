# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --anim-pacing: frame pacing while the shell's cards come and go, read off
# its own commit log. Thirty toasts arriving at once and dismissed at once,
# quick settings opening, its Power menu and the card closing, the launcher
# opening and closing, the volume OSD's round trip, and the switcher over
# two windows opening and cancelled. Each sits between a `menu ping` and
# the status read after it, so its window in the log is the commits between
# the two, read per surface the window names.
#
# Claims, against the output's refresh interval off `monitors -j`:
#   - every window's surfaces commit at least twice: the thing animated;
#   - a commit that asked for the next frame (`request=1`) is followed by
#     the next one within two refresh intervals for every frame callback
#     between them, unless the compositor itself sent that callback late
#     (`last_wait_us` past an interval): no frame of the shell's dropped
#     while moving. A callback that found nothing changed commits no buffer
#     and logs nothing, so a still stretch inside an animation is two
#     callbacks over two refreshes, not a gap. Late callbacks are counted
#     and printed apart: the switcher's thumbnails are Hyprland copying the
#     windows, which on this rig costs it a frame;
#   - no frame past a surface's first renders for longer than a refresh
#     interval, which no rate holds (thirty toasts arriving at once take 9
#     to 15ms on a busy rig; working their casts out at the canvas's pixels
#     took 25ms). Not read under FS_CPU_QUOTA, whose throttling stretches
#     render_us tenfold.
# Each surface's commit count, median and worst render_us and its longest
# gap, split into the callback wait, the reaction and the render, are
# printed beside it. Budget numbers: read on g815, and under FS_CPU_QUOTA
# for e1504g's power saver by hand.
leg_anim_pacing_flag="--anim-pacing"
leg_anim_pacing_order=127
leg_anim_pacing_claims="session"
leg_anim_pacing_budget="frame gaps against the output's refresh are g815's numbers"
leg_anim_pacing_needs="foot jq notify-send"

anim_pacing_windows_path="$shot_dir/anim-pacing-windows.txt"
anim_pacing_monitors_path="$shot_dir/anim-pacing-monitors.json"
anim_pacing_toasts_png="$shot_dir/anim-pacing-toasts.png"

leg_anim_pacing_timing() {
  leg_timing 50
}

leg_anim_pacing_drive() {
  local script="$shot_dir/anim-pacing-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
"$wmctl_bin" monitors -j > "$anim_pacing_monitors_path" 2>&1
for i in 1 2; do
  "$wmctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-anim-pacing --title='anim pacing \$i' sh -c 'sleep 300']==])" > /dev/null 2>&1
done
park
sleep 2
# One window: its label and the surfaces read in it, then what it drives.
win() {
  echo "\$1 \$2" >> "$anim_pacing_windows_path"
  $ipc call menu ping > /dev/null 2>&1
}
win toasts-arrive toasts
for i in \$(seq 1 30); do
  "$notify_send_bin" -u normal -t 60000 "Pacing toast \$i" 'Thirty at once' &
done
wait
sleep 1.5
$ipc call notifications status > /dev/null 2>&1
# A screencopy costs the compositor a frame, so it stays out of a window.
"$grim_bin" "$anim_pacing_toasts_png" > /dev/null 2>&1
win toasts-exit toasts
$ipc call notifications dismissAll > /dev/null 2>&1
sleep 1.5
$ipc call notifications status > /dev/null 2>&1
win quicksettings-open panel,scrim
$ipc call panel open quicksettings > /dev/null 2>&1
sleep 1.2
$ipc call quicksettings status > /dev/null 2>&1
win quicksettings-menu panel
$ipc call quicksettings menu power > /dev/null 2>&1
sleep 1.2
$ipc call quicksettings status > /dev/null 2>&1
$ipc call quicksettings back > /dev/null 2>&1
sleep 1
win panel-close panel,scrim
$ipc call panel close > /dev/null 2>&1
sleep 1.2
$ipc call panel state > /dev/null 2>&1
win launcher-open menu,menu-content,scrim
$ipc call menu toggle > /dev/null 2>&1
sleep 1.2
$ipc call menu status > /dev/null 2>&1
win launcher-close menu,menu-content,scrim
$ipc call menu close > /dev/null 2>&1
sleep 1.2
$ipc call menu status > /dev/null 2>&1
win osd osd
$ipc call osd volume > /dev/null 2>&1
sleep 4
$ipc call osd state > /dev/null 2>&1
win switcher-open switcher
$ipc call switcher next > /dev/null 2>&1
sleep 1.2
$ipc call switcher state > /dev/null 2>&1
win switcher-cancel switcher
$ipc call switcher cancel > /dev/null 2>&1
sleep 1.2
$ipc call switcher state > /dev/null 2>&1
EOF
  wm_exec_once "bash $script"
}

leg_anim_pacing_assert() {
  local refresh interval report
  refresh=$("$jq_bin" -r '[.[] | select(.focused)][0].refreshRate // .[0].refreshRate // empty' "$anim_pacing_monitors_path" 2>/dev/null)
  [ -n "$refresh" ] || fail "no refresh rate off monitors -j: $(cat "$anim_pacing_monitors_path")"
  [ -s "$anim_pacing_windows_path" ] || fail "the drive wrote no windows"
  interval=$(awk -v r="$refresh" 'BEGIN { printf "%.2f", 1000 / r }')
  echo "SMOKE_ANIM_PACING_REFRESH ${refresh}Hz, ${interval}ms a frame"
  if ! report=$(awk -v iv="$interval" -v quota="${FS_CPU_QUOTA:-}" '
    function num(line, key,   m) { if (match(line, " " key "=[0-9]+")) { m = substr(line, RSTART, RLENGTH); sub(/^[^=]*=/, "", m); return m + 0 } return 0 }
    function t(line,   m) { match(line, /t=[0-9]+ms/); m = substr(line, RSTART + 2, RLENGTH - 4); return m + 0 }
    FNR == NR { label[++nw] = $1; want[nw] = "," $2 ","; next }
    /^ipc t=[0-9]+ms call menu ping/ { w++; open = 1; next }
    /^ipc t=[0-9]+ms call [a-z]+ (status|state)$/ { open = 0; next }
    open && /^commit surface=/ {
      s = $2; sub(/^surface=/, "", s)
      if (index(want[w], "," s ",") == 0) next
      k = ++n[w, s]; at[w, s, k] = t($0); r[w, s, k] = num($0, "render_us")
      q[w, s, k] = num($0, "request"); cb[w, s, k] = num($0, "frame_callbacks"); lw[w, s, k] = num($0, "last_wait_us"); re[w, s, k] = num($0, "react_us")
    }
    END {
      bad = 0
      if (w < nw) { printf "FAIL only %d windows in the log, not %d\n", w, nw; bad = 1 }
      for (i = 1; i <= nw; i++) {
        split(substr(want[i], 2, length(want[i]) - 2), surf, ",")
        total = 0
        for (j in surf) {
          s = surf[j]; c = n[i, s] + 0; total += c
          if (c == 0) continue
          gap = 0; g = 0; late = 0; lateworst = 0
          for (k = 1; k < c; k++) if (q[i, s, k] == 1) {
            f = cb[i, s, k + 1] - cb[i, s, k]; d = (at[i, s, k + 1] - at[i, s, k]) / (f > 1 ? f : 1)
            if (lw[i, s, k + 1] > iv * 1000 + 2000) { late++; if (lw[i, s, k + 1] > lateworst) lateworst = lw[i, s, k + 1]; continue }
            if (d > gap) { gap = d; g = k + 1 }
          }
          worst = 0; for (k = 1; k <= c; k++) { v[k] = r[i, s, k]; if (v[k] > worst && (k > 1 || cb[i, s, k] > 0)) worst = v[k] }
          for (a = 1; a <= c; a++) for (b = a + 1; b <= c; b++) if (v[b] < v[a]) { x = v[a]; v[a] = v[b]; v[b] = x }
          med = v[int((c + 1) / 2)]
          printf "%s %s: %d commits over %dms, median render_us %d, worst %d, longest gap a callback %dms", label[i], s, c, at[i, s, c] - at[i, s, 1], med, worst, gap
          if (g) printf " (wait_us %d react_us %d render_us %d)", lw[i, s, g], re[i, s, g], r[i, s, g]
          if (late) printf ", %d late callbacks from the compositor, worst %dus", late, lateworst
          printf "\n"
          if (quota == "" && worst > iv * 1000) { printf "FAIL %s %s: a frame rendered for %dus, past a frame of %.1fms\n", label[i], s, worst, iv; bad = 1 }
          if (gap > 2 * iv + 2) { printf "FAIL %s %s: a %dms gap a callback after a frame that asked for the next, past two frames of %.1fms\n", label[i], s, gap, iv; bad = 1 }
        }
        if (total < 2) { printf "FAIL %s: %d commits, nothing animated\n", label[i], total; bad = 1 }
      }
      exit bad
    }' "$anim_pacing_windows_path" "$shell_log_path"); then
    echo "$report"
    fail "anim pacing: $(echo "$report" | grep '^FAIL' | head -3 | tr '\n' ' ')"
  fi
  echo "$report" | sed 's/^/SMOKE_ANIM_PACING /'
  echo "SMOKE_ANIM_PACING_TOASTS $anim_pacing_toasts_png"
}
