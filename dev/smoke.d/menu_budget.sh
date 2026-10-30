# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --menu-budget: the rust rewrite spec's launcher budget, read off the
# shell's own commit log. The bar's badge spins throughout (`debug
# r0Spinner`, the agent badge's spinner) while the launcher opens and
# closes five times over `menu toggle`. Two numbers per open: the time from
# the toggle reaching the shell to the launcher surface's first commit
# (budget 50 ms), and the longest gap between two bar commits from the
# toggle to a second after it (budget 33 ms, two frames at 60 Hz). Each
# gap is split into the compositor's callback wait and the shell's own
# hold on the frame, which has to stay under one frame (16 ms) wherever
# the run is. Then five seconds with the launcher open at rest, which must
# commit nothing.
leg_menu_budget_flag="--menu-budget"
leg_menu_budget_order=22
leg_menu_budget_budget="the launcher and bar frame budgets are g815's numbers"
leg_menu_budget_needs=""

leg_menu_budget_timing() {
  leg_timing 30 60
}

leg_menu_budget_drive() {
  local script="$shot_dir/menu-budget-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 4
$ipc call debug r0Spinner true > /dev/null 2>&1
sleep 1
for i in 1 2 3 4 5; do
  $ipc call menu toggle > /dev/null 2>&1
  sleep 1.5
  $ipc call menu toggle > /dev/null 2>&1
  sleep 1
done
$ipc call debug r0Spinner false > /dev/null 2>&1
sleep 1
$ipc call menu toggle > /dev/null 2>&1
sleep 2
echo rest-start > "$shot_dir/menu-budget-rest"
sleep 5
echo rest-end >> "$shot_dir/menu-budget-rest"
$ipc call menu toggle > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_menu_budget_assert() {
  local out
  out=$(awk '
    function t(line) { match(line, /t=[0-9]+ms/); return substr(line, RSTART + 2, RLENGTH - 4) + 0 }
    /^ipc t=.*function: "toggle"/ && /target: "menu"/ { n++; if (n % 2 == 1 && n <= 10) { open[++o] = t($0) } ; if (n == 11) rest = t($0) }
    /^commit surface=menu / { mc[++m] = t($0) }
    /^ipc t=.* menu configured$/ { cf[++c] = t($0) }
    function us(line, key) { if (!match(line, key "=[0-9]+")) return 0; return substr(line, RSTART + length(key) + 1, RLENGTH - length(key) - 1) + 0 }
    /^commit surface=bar / { bc[++b] = t($0); bw[b] = us($0, "last_wait_us"); br[b] = us($0, "react_us") }
    /^event loop: slow present / { pt[++q] = t($0); pu[q] = us($0, "present_us") }
    END {
      worst_first = 0; worst_gap = 0; worst_shell = 0; worst_wait = 0
      for (i = 1; i <= o; i++) {
        first = -1
        for (j = 1; j <= m; j++) if (mc[j] >= open[i]) { first = mc[j] - open[i]; break }
        # The compositor share of that: the toggle to the first configure
        # of the card, which a new layer surface has to wait for.
        conf = 0
        for (j = 1; j <= c; j++) if (cf[j] >= open[i]) { conf = cf[j] - open[i]; break }
        if (conf > first) conf = 0
        # Each bar gap split in two: what the shell held it for (the
        # callback to the commit, plus every present of 8 ms or more that
        # ran inside the gap) and what the compositor did (its callback
        # wait).
        gap = 0; shell = 0; wait = 0; last = -1
        for (j = 1; j <= b; j++) {
          if (bc[j] < open[i]) { last = bc[j]; continue }
          if (bc[j] > open[i] + 1000) break
          if (last >= 0) {
            if (bc[j] - last > gap) gap = bc[j] - last
            own = br[j] / 1000
            for (k = 1; k <= q; k++) if (pt[k] > last && pt[k] <= bc[j]) own += pu[k] / 1000
            if (own > shell) shell = own
            if (bw[j] / 1000 > wait) wait = bw[j] / 1000
          }
          last = bc[j]
        }
        printf "open %d first_commit_ms=%d configure_ms=%d bar_gap_ms=%d shell_ms=%d compositor_wait_ms=%d\n", i, first, conf, gap, shell, wait
        if (first < 0 || first > worst_first) worst_first = (first < 0 ? 99999 : first)
        if (first >= 0 && first - conf > worst_own) worst_own = first - conf
        if (conf > worst_conf) worst_conf = conf
        if (gap > worst_gap) worst_gap = gap
        if (shell > worst_shell) worst_shell = shell
        if (wait > worst_wait) worst_wait = wait
      }
      # The frame interval of the session itself while the badge spins and nothing
      # else moves: the mean bar gap in the second before the first open.
      k = 0; sum = 0
      for (j = 2; j <= b; j++) if (bc[j] < open[1] && bc[j-1] > open[1] - 1000) { k++; sum += bc[j] - bc[j-1] }
      base = k ? sum / k : 0
      quiet = 0
      for (j = 1; j <= m; j++) if (mc[j] > rest + 2000 && mc[j] < rest + 7000) quiet++
      printf "worst first_commit_ms=%d first_own_ms=%d configure_ms=%d bar_gap_ms=%d shell_ms=%d compositor_wait_ms=%d rest_commits=%d opens=%d frame_ms=%d\n", worst_first, worst_own, worst_conf, worst_gap, worst_shell, worst_wait, quiet, o, base
    }' "$shell_log_path")
  echo "$out" | sed 's/^/SMOKE_MENU_BUDGET /'
  # Every commit, callback wait and slow loop turn of the run, for reading
  # where a gap came from (the compositor's wait or the shell's own turn).
  grep -E '^(commit|ipc|event loop:) ' "$shell_log_path" > "$shot_dir/menu-budget.log" || true
  echo "SMOKE_MENU_BUDGET_LOG $shot_dir/menu-budget.log"
  echo "SMOKE_MENU_BUDGET_SHELL $shell_log_path"
  local worst
  worst=$(echo "$out" | tail -1)
  local first gap quiet opens
  first=$(echo "$worst" | sed -n 's/.*first_commit_ms=\([0-9]*\).*/\1/p')
  gap=$(echo "$worst" | sed -n 's/.*bar_gap_ms=\([0-9]*\).*/\1/p')
  quiet=$(echo "$worst" | sed -n 's/.*rest_commits=\([0-9]*\).*/\1/p')
  opens=$(echo "$worst" | sed -n 's/.*opens=\([0-9]*\).*/\1/p')
  [ "${opens:-0}" -eq 5 ] || fail "menu budget: saw ${opens:-0} launcher opens in the shell log, wanted 5"
  local own conf
  own=$(echo "$worst" | sed -n 's/.*first_own_ms=\([0-9]*\).*/\1/p')
  conf=$(echo "$worst" | sed -n 's/.*configure_ms=\([0-9]*\).*/\1/p')
  # The VM's llvmpipe Hyprland takes 100 to 400 ms to configure a new layer
  # surface, so there the budget is read on the shell's own share: the
  # configure to the commit.
  if [ "${conf:-0}" -gt 20 ]; then
    echo "SMOKE_MENU_BUDGET first commit compositor-bound here: configures waited up to ${conf}ms, the shell's own share ${own}ms"
    [ "$own" -le 50 ] || fail "menu budget: a launcher open took ${own}ms from its configure to its first commit (budget 50)"
  else
    [ "$first" -le 50 ] || fail "menu budget: a launcher open took ${first}ms to its first commit (budget 50)"
  fi
  local shell wait
  shell=$(echo "$worst" | sed -n 's/.*shell_ms=\([0-9]*\).*/\1/p')
  wait=$(echo "$worst" | sed -n 's/.*compositor_wait_ms=\([0-9]*\).*/\1/p')
  # The sidebar carries no Spaces cell, so no badge spins and the bar
  # commits nothing to time the frames against.
  if leg_on sidebar; then
    echo "SMOKE_MENU_BUDGET bar frames skipped: the sidebar has no spinning badge"
    [ "$quiet" -eq 0 ] || fail "menu budget: the launcher committed $quiet times while open at rest"
    return 0
  fi
  [ "${shell:-99}" -le 16 ] || fail "menu budget: the shell held a bar frame ${shell}ms across an open (budget 16, one frame)"
  # The VM's llvmpipe Hyprland drops to 20-30 fps compositing the launcher
  # and its scrim, so every gap there is its own callback wait: the 33 ms
  # gap budget is only readable where the compositor kept pace.
  if [ "${wait:-0}" -gt 20 ]; then
    echo "SMOKE_MENU_BUDGET bar gap compositor-bound here: callbacks waited up to ${wait}ms, the shell's own share ${shell}ms"
  else
    [ "$gap" -le 33 ] || fail "menu budget: the bar went ${gap}ms without a frame across an open (budget 33)"
  fi
  [ "$quiet" -eq 0 ] || fail "menu budget: the launcher committed $quiet times while open at rest"
}
