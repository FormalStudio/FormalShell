# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --scroll-pacing: frame pacing while a scroll moves, read off the shell's
# own commit log. Two scroll areas: the wallpaper picker's grid (the
# launcher's content layer, `menu-content`) over the --wheel leg's kind of
# fixture, and the notification centre over thirty rows (the panel host,
# `panel`). On each, one wheel notch, a burst of five notches 40ms apart,
# thirty finger frames 8ms apart (a touchpad reporting at 125Hz) lifted
# after a pause, and a flick that coasts. Each scroll sits between a `menu
# ping` and the status read after it, so its window in the log is the
# commits between the two.
#
# Claims, against the output's refresh interval off `monitors -j`:
#   - a notch glides: at least four commits, not one jump;
#   - over a scroll's first 700ms, no gap between commits past two refresh
#     intervals (a dropped frame or a stalled one); a coast's tail after
#     that moves under three pixels a frame;
#   - the fingers' frames come out one commit a refresh, never one per
#     input frame (a few more for the compositor's first callbacks, which
#     come back at once after an idle).
# The median render_us of each window is printed beside it, the per-frame
# cost a scroll pays. Budget numbers: read on g815, and under FS_CPU_QUOTA
# for e1504g's power saver by hand.
leg_scroll_pacing_flag="--scroll-pacing"
leg_scroll_pacing_order=126
leg_scroll_pacing_claims="session"
leg_scroll_pacing_budget="frame gaps against the output's refresh are g815's numbers"
leg_scroll_pacing_needs="wlrctl convert python3 jq notify-send"

scroll_pacing_dir="$iso_home/.local/share/formalshell/pacing-pictures"
scroll_pacing_dispatch_path="$shot_dir/scroll-pacing-dispatch.txt"
scroll_pacing_monitors_path="$shot_dir/scroll-pacing-monitors.json"
scroll_pacing_menu_path="$shot_dir/scroll-pacing-menu.json"
scroll_pacing_center_path="$shot_dir/scroll-pacing-center.json"
scroll_pacing_png="$shot_dir/scroll-pacing.png"
scroll_pacing_center_png="$shot_dir/scroll-pacing-center.png"
scroll_pacing_vpointer="$PWD/dev/vpointer.py"

leg_scroll_pacing_fixture() {
  local i
  mkdir -p "$scroll_pacing_dir"
  for i in $(seq 0 119); do
    $convert_bin -size 320x180 "xc:hsl($((i * 9)),70%,45%)" "$scroll_pacing_dir/img-$(printf '%03d' "$i").png"
  done
  settings_pin ".picker.directory = \"$scroll_pacing_dir\""
  shell_env="${shell_env:+$shell_env }FS_WALLPAPERS=$scroll_pacing_dir-none"
}

leg_scroll_pacing_timing() {
  leg_timing 70
}

leg_scroll_pacing_drive() {
  local script="$shot_dir/scroll-pacing-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
"$wmctl_bin" monitors -j > "$scroll_pacing_monitors_path" 2>&1
# Thirty distinct rows, in the centre's pending tier once their popups
# time out while the picker half runs.
for i in \$(seq 1 30); do
  "$notify_send_bin" -u normal "Pacing row \$i" 'Filling the centre' &
done
wait
drag="\$(for _ in \$(seq 30); do printf 'finger 6 wait 8 '; done)wait 100 lift"
flick="\$(for _ in 1 2 3 4 5 6; do printf 'finger 40 wait 8 '; done)finger 40 lift"
# One notch, a burst, fingers, a flick, each between a ping and a status.
scrolls() {
  $ipc call menu ping > /dev/null 2>&1
  $(wm_wheel 15) >> "$scroll_pacing_dispatch_path" 2>&1
  sleep 1
  \$1 > /dev/null 2>&1
  $ipc call menu ping > /dev/null 2>&1
  for _ in 1 2 3 4 5; do
    $(wm_wheel 15) >> "$scroll_pacing_dispatch_path" 2>&1
    sleep 0.04
  done
  sleep 1
  \$1 > /dev/null 2>&1
  $ipc call menu ping > /dev/null 2>&1
  "$python3_bin" "$scroll_pacing_vpointer" \$drag >> "$scroll_pacing_dispatch_path" 2>&1
  sleep 1
  \$1 > /dev/null 2>&1
  $ipc call menu ping > /dev/null 2>&1
  "$python3_bin" "$scroll_pacing_vpointer" \$flick >> "$scroll_pacing_dispatch_path" 2>&1
  sleep 1.6
  \$1 > "\$2" 2>&1
}
$ipc call picker summon > /dev/null 2>&1
sleep 2
park
read -r gx gy <<< "\$($ipc call menu status 2>/dev/null | "$jq_bin" -r '.rowBoxes | (map(.y) | unique | .[1]) as \$y | (map(.x) | unique | .[1]) as \$x | map(select(.x == \$x and .y == \$y))[0] | "\(.x + .width / 2 | floor) \(.y + .height / 2 | floor)"')"
echo "grid cell at \$gx \$gy" >> "$scroll_pacing_dispatch_path"
"$wlrctl_bin" pointer move "\$gx" "\$gy" >> "$scroll_pacing_dispatch_path" 2>&1
sleep 1
scrolls "$ipc call menu status" "$scroll_pacing_menu_path"
"$grim_bin" "$scroll_pacing_png" > /dev/null 2>&1
$ipc call menu close > /dev/null 2>&1
sleep 8
$ipc call notifications showHistory > /dev/null 2>&1
sleep 2
read -r cx cy <<< "\$($ipc call debug dump 2>/dev/null | "$jq_bin" -r '.panel.viewport | "\(.x + .width / 2 | floor) \(.y + .height / 2 | floor)"')"
echo "centre card at \$cx \$cy" >> "$scroll_pacing_dispatch_path"
park "\${cx:-1700}" "\${cy:-600}"
sleep 1
scrolls "$ipc call notifications status" "$scroll_pacing_center_path"
"$grim_bin" "$scroll_pacing_center_png" > /dev/null 2>&1
EOF
  wm_exec_once "bash $script"
}

leg_scroll_pacing_assert() {
  local refresh interval report
  cat "$scroll_pacing_dispatch_path" 2>/dev/null || true
  refresh=$("$jq_bin" -r '[.[] | select(.focused)][0].refreshRate // .[0].refreshRate // empty' "$scroll_pacing_monitors_path" 2>/dev/null)
  if [ -z "$refresh" ]; then
    fail "no refresh rate off monitors -j: $(cat "$scroll_pacing_monitors_path")"
  fi
  interval=$(awk -v r="$refresh" 'BEGIN { printf "%.2f", 1000 / r }')
  echo "SMOKE_SCROLL_PACING_REFRESH ${refresh}Hz, ${interval}ms a frame"
  "$jq_bin" -e '.scrollTop > 0' "$scroll_pacing_menu_path" > /dev/null 2>&1 \
    || fail "the picker never scrolled: $(cat "$scroll_pacing_menu_path")"
  "$jq_bin" -e '.centerCapped == true' "$scroll_pacing_center_path" > /dev/null 2>&1 \
    || fail "the centre was not long enough to scroll: $(cat "$scroll_pacing_center_path")"
  # Window n: the commits of its surface between the n-th `menu ping` and
  # the status read after it. Windows 1-4 are the picker's, 5-8 the
  # centre's, each a notch, a burst, fingers and a flick.
  if ! report=$(awk -v iv="$interval" '
    function us(line, key,   m) { if (match(line, key "=[0-9]+")) { m = substr(line, RSTART, RLENGTH); sub(/^[^=]*=/, "", m); return m + 0 } return 0 }
    function t(line,   m) { match(line, /t=[0-9]+ms/); m = substr(line, RSTART + 2, RLENGTH - 4); return m + 0 }
    /^ipc t=[0-9]+ms call menu ping/ { w++; open = 1; n[w] = 0; next }
    /^ipc t=[0-9]+ms call (menu|notifications) status/ { open = 0; next }
    open && /^commit surface=/ {
      want = (w <= 4) ? "menu-content" : "panel"
      if ($2 != "surface=" want) next
      k = ++n[w]; at[w, k] = t($0); r[w, k] = us($0, "render_us")
    }
    END {
      split("notch burst fingers flick", kind, " ")
      bad = 0
      for (i = 1; i <= 8; i++) {
        name = ((i <= 4) ? "picker " : "centre ") kind[(i - 1) % 4 + 1]
        c = n[i]; gap = 0
        # The last 350ms of a coast creeps under three pixels a frame, where
        # a frame that rounds to the last one commits nothing.
        for (k = 2; k <= c && at[i, k] - at[i, 1] <= 700; k++) { d = at[i, k] - at[i, k - 1]; if (d > gap) gap = d }
        span = c ? at[i, c] - at[i, 1] : 0
        # Median render_us.
        m = 0
        if (c > 0) { for (k = 1; k <= c; k++) v[k] = r[i, k]; for (a = 1; a <= c; a++) for (b = a + 1; b <= c; b++) if (v[b] < v[a]) { x = v[a]; v[a] = v[b]; v[b] = x }; m = v[int((c + 1) / 2)] }
        printf "%s: %d commits over %dms, longest gap %dms, median render_us %d\n", name, c, span, gap, m
        if (c < 4) { printf "FAIL %s: %d commits, a jump rather than a glide\n", name, c; bad = 1 }
        if (gap > 2 * iv + 2) { printf "FAIL %s: a %dms gap between commits, past two frames of %.1fms\n", name, gap, iv; bad = 1 }
        if (kind[(i - 1) % 4 + 1] == "fingers" && c > int(span / iv) + 4) { printf "FAIL %s: %d commits in %dms, more than one a refresh\n", name, c, span; bad = 1 }
      }
      if (w < 8) { printf "FAIL only %d scroll windows in the log, not 8\n", w; bad = 1 }
      exit bad
    }' "$shell_log_path"); then
    echo "$report"
    fail "scroll pacing: $(echo "$report" | grep '^FAIL' | head -3 | tr '\n' ' ')"
  fi
  echo "$report" | sed 's/^/SMOKE_SCROLL_PACING /'
  echo "SMOKE_SCROLL_PACING_PICKER $scroll_pacing_png"
  echo "SMOKE_SCROLL_PACING_CENTRE $scroll_pacing_center_png"
}
