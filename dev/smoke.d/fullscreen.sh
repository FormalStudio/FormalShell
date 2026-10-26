# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --fullscreen proves the fullscreen chrome auto-hide (fullscreen.hideChrome,
# default on): while a focused fullscreen window covers an output, the bar,
# the screen frame's zones and the hot corners all leave the overlay/top
# layers, so the window reaches Hyprland's solitary / direct-scanout path
# (nothing mapped above it), and they all come straight back when it leaves
# fullscreen. The frame is pinned on so all three chrome kinds are present to
# begin with.
#
# The base run's own focused fixture window is what gets fullscreened
# (leg_fullscreen_fixture_window=keep), so `hl.dsp.window.fullscreen`
# lands on it. Three `hyprctl -j layers` dumps off one timeline: before, while
# fullscreen, and after `fullscreen 0`. The claim is read off the compositor's
# own layer list, not the pixels: the formalshell chrome namespaces are gone
# in the middle dump and back to their starting counts in the last one, and
# `hyprctl -j clients` confirms a window really was fullscreen in the middle
# (so the disappearance is the cause under test, not a window that closed).
#
# The launcher is the chrome that has to stay reachable there: summoned over
# the fullscreen window, its surface is on the overlay level of a fourth dump
# (Hyprland draws a fullscreen window over the top level) and its card is in
# the frame, read as the frame differing from the bare fullscreen one.
#
# Nothing runs behind the covered bar: a monitor and a visualizer cell sit in
# the layout while a real MPRIS player (mpv) loops a tone. Before fullscreen
# there is a `cava` child and the monitor's `sh -c` collector is exec'd every
# tick (read off `strace -f -e execve` on the shell for 5s); while fullscreen
# there is neither for 6s, past the wants' 2s grace; after, both are back.
#
# Then a game's cursor lock under the scrolling layout, where Hyprland 0.56
# lets go of every pointer constraint on any layer map, unmap or layer
# state commit over the output (`present_toasts` has the source). The
# window is dev/lockpointer.py, floated to the output's own size so it
# covers it borderless, holding a persistent zwp_locked_pointer_v1 and
# logging each locked/unlocked event. Two notify-sends and an `osd volume`
# land over it: its log gains no `unlocked`, the notifications layer is a
# column narrower than the output whose input region the shell logged as
# empty, no OSD layer maps, and the active window and cursor stay put. A
# `debug join` over the same window last is the control: an overlay map
# the shell does not hold back, which has to log `unlocked`, so the probe
# can see a release at all. Hyprland only activates a constraint again when
# keyboard focus moves onto its surface (`CFocusState::rawSurfaceFocus`), so
# the float and resize that release the first lock are followed by focus
# leaving the window and coming back, the lock active again before the
# first toast.
#
# Then the same window in Hyprland's own fullscreen: tiled under scrolling,
# tiled under dwindle, and floating under scrolling. Tiled, the stack maps
# per toast, since an overlay layer that merely exists keeps Hyprland off
# direct scanout and tearing, and scrolling counts a tiled fullscreen window
# as covering the workspace. Floating, scrolling counts it as nothing, so the
# column stays mapped as it does over a borderless game. Each phase re-arms
# the lock (SIGUSR1 to the probe, while it holds keyboard focus), a toast
# comes and goes over it (8s, the floor for a normal one), and the log gains no
# `unlocked` across any of them.
leg_fullscreen_flag="--fullscreen"
leg_fullscreen_order=200
leg_fullscreen_needs="convert jq notify-send wlrctl python3 mpv ffmpeg strace"
leg_fullscreen_fixture_window=keep

fullscreen_before_layers="$shot_dir/fullscreen-before-layers.json"
fullscreen_on_layers="$shot_dir/fullscreen-on-layers.json"
fullscreen_after_layers="$shot_dir/fullscreen-after-layers.json"
fullscreen_on_clients="$shot_dir/fullscreen-on-clients.json"
fullscreen_menu_layers="$shot_dir/fullscreen-menu-layers.json"
fullscreen_menu_path="$shot_dir/fullscreen-menu.png"
fullscreen_before_path="$shot_dir/fullscreen-before.png"
fullscreen_on_path="$shot_dir/fullscreen-on.png"
fullscreen_after_path="$shot_dir/fullscreen-after.png"
fullscreen_lock_log="$shot_dir/fullscreen-lock.log"
fullscreen_lock_clients="$shot_dir/fullscreen-lock-clients.json"
fullscreen_lock_toast_layers="$shot_dir/fullscreen-lock-toast-layers.json"
fullscreen_lock_osd_layers="$shot_dir/fullscreen-lock-osd-layers.json"
fullscreen_lock_state="$shot_dir/fullscreen-lock-state.txt"
fullscreen_lock_toast_path="$shot_dir/fullscreen-lock-toast.png"
fullscreen_fs_phases="scrolling:tiled dwindle:tiled scrolling:floating"
fullscreen_track_path="$shot_dir/fullscreen-tone.flac"
fullscreen_mpv_pid_path="$shot_dir/fullscreen-mpv.pid"
fullscreen_work_path="$shot_dir/fullscreen-work.txt"

need_strace() {
  if [ -z "${strace_bin:-}" ]; then
    if command -v strace >/dev/null 2>&1; then
      strace_bin=$(command -v strace)
    else
      strace_bin=$(nix build 'nixpkgs#strace^out' --no-link --print-out-paths)/bin/strace
    fi
  fi
}

leg_fullscreen_fixture() {
  "$ffmpeg_bin" -nostdin -loglevel error -f lavfi -i "sine=frequency=440:sample_rate=48000:duration=20" -ac 2 \
    -metadata "title=FormalShell Fullscreen Tone" -metadata "artist=FormalShell Test Artist" -c:a flac -y "$fullscreen_track_path"
  settings_fragment ', "frame": {"thickness": 10, "radius": 20}'
  settings_fragment ', "bar": {"layout": {"center": ["clock", "monitor", "visualizer"]}}'
}

leg_fullscreen_timing() {
  leg_timing 115 160
}

leg_fullscreen_drive() {
  local script="$shot_dir/fullscreen-drive.sh"
  write_script "$script" <<EOS
#!/usr/bin/env bash
"$mpv_bin" --no-video --really-quiet --loop-file=inf "$fullscreen_track_path" &
echo \$! > "$fullscreen_mpv_pid_path"
st="$strace_bin"; [ "\$(cat /proc/sys/kernel/yama/ptrace_scope 2>/dev/null)" = 0 ] || st="sudo -n $strace_bin"
# One line: the phase, whether a cava child runs, and how many monitor
# collectors the shell exec'd over \$2 seconds.
work() {
  local cava=0 ticks
  pgrep -f -- "cava -p" > /dev/null && cava=1
  timeout -s INT "\$2" \$st -f -qq -s 64 -e trace=execve -p "\$pid" -o "$shot_dir/fullscreen-\$1-strace.log" > /dev/null 2>&1
  ticks=\$(grep -c '@stat' "$shot_dir/fullscreen-\$1-strace.log" 2>/dev/null)
  echo "\$1 cava=\$cava ticks=\${ticks:-0}" >> "$fullscreen_work_path"
}
# cava within 10s of the player starting, gone within 5s of a hide.
wait_cava() {
  for _ in \$(seq "\$2"); do
    if pgrep -f -- "cava -p" > /dev/null; then [ "\$1" = up ] && return; else [ "\$1" = down ] && return; fi
    sleep 1
  done
}
sleep 5
pid=\$(cat "$shot_dir/shell.pid")
wait_cava up 10
work before 5
"$hyprctl_bin" -j layers > "$fullscreen_before_layers" 2>&1
"$grim_bin" "$fullscreen_before_path" > /dev/null 2>&1
"$hyprctl_bin" dispatch "hl.dsp.window.fullscreen({ mode = 'fullscreen', action = 'set' })" > /dev/null 2>&1
sleep 3
"$hyprctl_bin" -j layers > "$fullscreen_on_layers" 2>&1
"$hyprctl_bin" -j clients > "$fullscreen_on_clients" 2>&1
"$grim_bin" "$fullscreen_on_path" > /dev/null 2>&1
wait_cava down 5
work on 6
$ipc call menu summon "" > /dev/null 2>&1
sleep 2
"$hyprctl_bin" -j layers > "$fullscreen_menu_layers" 2>&1
"$grim_bin" "$fullscreen_menu_path" > /dev/null 2>&1
$ipc call menu close > /dev/null 2>&1
sleep 1
"$hyprctl_bin" dispatch "hl.dsp.window.fullscreen({ mode = 'fullscreen', action = 'unset' })" > /dev/null 2>&1
sleep 3
"$hyprctl_bin" -j layers > "$fullscreen_after_layers" 2>&1
"$grim_bin" "$fullscreen_after_path" > /dev/null 2>&1
wait_cava up 10
work after 5
kill "\$(cat "$fullscreen_mpv_pid_path")" 2>/dev/null
"$hyprctl_bin" eval 'hl.config({ general = { layout = "scrolling" } })' > /dev/null 2>&1
"$python3_bin" "$PWD/dev/lockpointer.py" formalshell-lockpointer "$fullscreen_lock_log" 2>> "$fullscreen_lock_log" &
sleep 2
lp="window = 'class:^(formalshell-lockpointer)\\\$'"
"$hyprctl_bin" dispatch "hl.dsp.window.float({ \$lp, action = 'enable' })" > /dev/null 2>&1
"$hyprctl_bin" dispatch "hl.dsp.window.resize({ x = 1920, y = 1080, \$lp })" > /dev/null 2>&1
"$hyprctl_bin" dispatch "hl.dsp.window.move({ x = 0, y = 0, \$lp })" > /dev/null 2>&1
"$hyprctl_bin" dispatch "hl.dsp.focus({ \$lp })" > /dev/null 2>&1
"$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1
"$wlrctl_bin" pointer move 960 540 > /dev/null 2>&1
sleep 1
"$hyprctl_bin" dispatch "hl.dsp.focus({ window = 'class:^(formalshell-smoke-iconic)\\\$' })" > /dev/null 2>&1
sleep 0.5
"$hyprctl_bin" dispatch "hl.dsp.focus({ \$lp })" > /dev/null 2>&1
sleep 2
"$hyprctl_bin" -j clients > "$fullscreen_lock_clients" 2>&1
{ echo "before"; "$hyprctl_bin" cursorpos; "$hyprctl_bin" -j activewindow | "$jq_bin" -r .class; } > "$fullscreen_lock_state" 2>&1
echo "0 toast" >> "$fullscreen_lock_log"
"$notify_send_bin" -u normal 'Over the game' 'the cursor stays locked'
sleep 1.5
"$hyprctl_bin" -j layers > "$fullscreen_lock_toast_layers" 2>&1
"$grim_bin" "$fullscreen_lock_toast_path" > /dev/null 2>&1
"$notify_send_bin" -u critical 'Second' 'still locked'
sleep 1
$ipc call osd volume > /dev/null 2>&1
sleep 1
"$hyprctl_bin" -j layers > "$fullscreen_lock_osd_layers" 2>&1
sleep 6
{ echo "after"; "$hyprctl_bin" cursorpos; "$hyprctl_bin" -j activewindow | "$jq_bin" -r .class; } >> "$fullscreen_lock_state" 2>&1
echo "0 control" >> "$fullscreen_lock_log"
$ipc call debug join top 100 200 > /dev/null 2>&1
sleep 1
$ipc call debug joinClear > /dev/null 2>&1
$ipc call notifications dismissOne > /dev/null 2>&1
sleep 1
fs_phase() {
  local name="fs-\${1%%:*}" float=disable
  [ "\${1#*:}" = floating ] && { name="\$name-floating"; float=enable; }
  "$hyprctl_bin" eval "hl.config({ general = { layout = '\${1%%:*}' } })" > /dev/null 2>&1
  sleep 1
  "$hyprctl_bin" dispatch "hl.dsp.window.float({ \$lp, action = '\$float' })" > /dev/null 2>&1
  "$hyprctl_bin" dispatch "hl.dsp.focus({ \$lp })" > /dev/null 2>&1
  "$hyprctl_bin" dispatch "hl.dsp.window.fullscreen({ mode = 'fullscreen', action = 'set' })" > /dev/null 2>&1
  sleep 2
  echo "0 \$name-arm" >> "$fullscreen_lock_log"
  pkill -USR1 -f 'lockpointer.py formalshell-lockpointer'
  sleep 2
  "$hyprctl_bin" -j clients > "$shot_dir/fullscreen-\$name-clients.json" 2>&1
  echo "0 \$name" >> "$fullscreen_lock_log"
  "$notify_send_bin" -u normal 'Over the fullscreen game' "\$1"
  sleep 1.5
  "$hyprctl_bin" -j layers > "$shot_dir/fullscreen-\$name-toast-layers.json" 2>&1
  "$grim_bin" "$shot_dir/fullscreen-\$name.png" > /dev/null 2>&1
  sleep 8
  $ipc call osd volume > /dev/null 2>&1
  sleep 1
  "$hyprctl_bin" -j layers > "$shot_dir/fullscreen-\$name-after-layers.json" 2>&1
  echo "0 \$name-end" >> "$fullscreen_lock_log"
  "$hyprctl_bin" dispatch "hl.dsp.window.fullscreen({ mode = 'fullscreen', action = 'unset' })" > /dev/null 2>&1
  sleep 1
}
for phase in $fullscreen_fs_phases; do fs_phase "\$phase"; done
EOS
  hypr_exec_once "bash $script"
  add_cleanup "pkill -f 'lockpointer.py formalshell-lockpointer' || true"
  add_cleanup "kill \$(cat '$fullscreen_mpv_pid_path' 2>/dev/null) 2>/dev/null || true"
}

# Count layer surfaces across every output whose namespace starts with the
# given prefix (the frame-zone namespace is exactly "formalshell:frame-zone",
# the bar "formalshell:bar", the hot corners "formalshell:hotcorner").
_fullscreen_count() {
  "$jq_bin" -r --arg ns "$2" '[.[] | .levels[] | .[] | select(.namespace == $ns)] | length' "$1" 2>/dev/null
}

leg_fullscreen_assert() {
  local f
  for f in "$fullscreen_before_layers" "$fullscreen_on_layers" "$fullscreen_after_layers" "$fullscreen_on_clients"; do
    [ -s "$f" ] || fail "no dump produced at $f"
  done

  local bar_before frame_before corner_before
  bar_before=$(_fullscreen_count "$fullscreen_before_layers" "formalshell:bar")
  frame_before=$(_fullscreen_count "$fullscreen_before_layers" "formalshell:frame-zone")
  corner_before=$(_fullscreen_count "$fullscreen_before_layers" "formalshell:hotcorner")
  echo "before: bar=$bar_before frame-zone=$frame_before hotcorner=$corner_before"
  [ "$bar_before" -ge 1 ] || fail "no bar surface before fullscreen"
  [ "$frame_before" -eq 4 ] || fail "expected four frame zones before fullscreen, got $frame_before"
  [ "$corner_before" -ge 1 ] || fail "no hot corner before fullscreen; the hide would be untested"

  # A window really is fullscreen in the middle dump, so the chrome vanishing
  # is the auto-hide firing, not the fixture window having closed.
  local fs_windows
  fs_windows=$("$jq_bin" -r '[.[] | select(.fullscreen != 0 and .fullscreen != false)] | length' "$fullscreen_on_clients" 2>/dev/null)
  echo "fullscreen windows while on: $fs_windows"
  [ "${fs_windows:-0}" -ge 1 ] || fail "no window was actually fullscreen; the hide would be untested"

  local bar_on frame_on corner_on
  bar_on=$(_fullscreen_count "$fullscreen_on_layers" "formalshell:bar")
  frame_on=$(_fullscreen_count "$fullscreen_on_layers" "formalshell:frame-zone")
  corner_on=$(_fullscreen_count "$fullscreen_on_layers" "formalshell:hotcorner")
  echo "while fullscreen: bar=$bar_on frame-zone=$frame_on hotcorner=$corner_on"
  [ "$bar_on" -eq 0 ] || fail "bar stayed mapped under fullscreen ($bar_on), blocking scanout"
  [ "$frame_on" -eq 0 ] || fail "frame zones stayed mapped under fullscreen ($frame_on)"
  [ "$corner_on" -eq 0 ] || fail "hot corners stayed mapped under fullscreen ($corner_on)"

  # The launcher over the fullscreen window: on the overlay level, the one
  # Hyprland still draws above it, and actually in the frame.
  [ -s "$fullscreen_menu_layers" ] || fail "no dump produced at $fullscreen_menu_layers"
  local menu_overlay
  menu_overlay=$("$jq_bin" -r '[.[] | .levels["3"][]? | select(.namespace == "formalshell:menu")] | length' "$fullscreen_menu_layers" 2>/dev/null)
  echo "launcher over fullscreen: overlay surfaces=$menu_overlay"
  [ "${menu_overlay:-0}" -ge 1 ] || fail "the launcher is not on the overlay level over a fullscreen window"
  [ -s "$fullscreen_menu_path" ] || fail "no launcher frame at $fullscreen_menu_path"
  local menu_diff
  menu_diff=$($convert_bin "$fullscreen_on_path" "$fullscreen_menu_path" -compose difference -composite \
    -colorspace Gray -threshold 5% -format '%[fx:mean*w*h]' info: 2>/dev/null | awk '{printf "%d", $1}')
  echo "launcher over fullscreen: pixels differing from the bare frame=$menu_diff"
  [ "${menu_diff:-0}" -gt 10000 ] || fail "the launcher did not draw over the fullscreen window ($menu_diff pixels differ)"

  local bar_after frame_after corner_after
  bar_after=$(_fullscreen_count "$fullscreen_after_layers" "formalshell:bar")
  frame_after=$(_fullscreen_count "$fullscreen_after_layers" "formalshell:frame-zone")
  corner_after=$(_fullscreen_count "$fullscreen_after_layers" "formalshell:hotcorner")
  echo "after: bar=$bar_after frame-zone=$frame_after hotcorner=$corner_after"
  [ "$bar_after" -eq "$bar_before" ] || fail "bar did not return after fullscreen ($bar_after vs $bar_before)"
  [ "$frame_after" -eq "$frame_before" ] || fail "frame zones did not return after fullscreen ($frame_after vs $frame_before)"
  [ "$corner_after" -eq "$corner_before" ] || fail "hot corners did not return after fullscreen ($corner_after vs $corner_before)"

  _fullscreen_work_assert
  _fullscreen_lock_assert

  echo "SMOKE_FULLSCREEN_BEFORE $fullscreen_before_path"
  echo "SMOKE_FULLSCREEN_ON $fullscreen_on_path"
  echo "SMOKE_FULLSCREEN_MENU $fullscreen_menu_path"
  echo "SMOKE_FULLSCREEN_AFTER $fullscreen_after_path"
}

# The cava child and the monitor's collector: running before, stopped under
# the covered bar, running again after.
_fullscreen_work_assert() {
  [ -s "$fullscreen_work_path" ] || fail "no work samples at $fullscreen_work_path"
  echo "--- work behind the bar ---"; cat "$fullscreen_work_path"
  local phase cava ticks
  for phase in before on after; do
    cava=$(sed -n "s/^$phase cava=\([0-9]*\) .*/\1/p" "$fullscreen_work_path")
    ticks=$(sed -n "s/^$phase .*ticks=\([0-9]*\)$/\1/p" "$fullscreen_work_path")
    [ -n "$cava" ] && [ -n "$ticks" ] || fail "no $phase work sample"
    if [ "$phase" = on ]; then
      [ "$cava" -eq 0 ] || fail "cava ran under the covered bar"
      [ "$ticks" -eq 0 ] || fail "the monitor polled $ticks times under the covered bar"
    else
      [ "$cava" -eq 1 ] || fail "no cava child $phase fullscreen with a player playing"
      [ "$ticks" -ge 1 ] || fail "the monitor never polled $phase fullscreen"
    fi
  done
}

_fullscreen_lock_assert() {
  local f
  for f in "$fullscreen_lock_log" "$fullscreen_lock_clients" "$fullscreen_lock_toast_layers" "$fullscreen_lock_osd_layers" "$fullscreen_lock_state"; do
    [ -s "$f" ] || fail "no dump produced at $f"
  done
  echo "--- lockpointer log ---"; cat "$fullscreen_lock_log"
  echo "--- cursor and active window ---"; cat "$fullscreen_lock_state"

  local covering
  covering=$("$jq_bin" -r '[.[] | select(.class == "formalshell-lockpointer" and .floating and .size == [1920, 1080] and .at == [0, 0])] | length' "$fullscreen_lock_clients" 2>/dev/null)
  [ "${covering:-0}" -eq 1 ] || fail "the lock window is not floating over the whole output"

  local before during control
  before=$(sed -n '1,/ toast$/p' "$fullscreen_lock_log")
  during=$(sed -n '/ toast$/,/ control$/p' "$fullscreen_lock_log")
  control=$(sed -n '/ control$/,$p' "$fullscreen_lock_log")
  echo "$before" | grep -q ' locked$' || fail "the window never held its pointer lock before the toasts"
  [ "$(echo "$before" | grep ' locked$\| unlocked$' | tail -1 | awk '{print $2}')" = locked ] \
    || fail "the lock was not active when the first toast arrived"
  if echo "$during" | grep -q ' unlocked$'; then fail "a toast or the OSD released the game's pointer lock"; fi
  echo "$control" | grep -q ' unlocked$' || fail "the debug join control map released nothing, so the probe cannot see a release"

  local toasts width
  toasts=$("$jq_bin" -r '[.[] | .levels["3"][]? | select(.namespace == "formalshell:notifications")] | length' "$fullscreen_lock_toast_layers")
  width=$("$jq_bin" -r '[.[] | .levels["3"][]? | select(.namespace == "formalshell:notifications") | .w] | first // 0' "$fullscreen_lock_toast_layers")
  echo "notifications over the game: surfaces=$toasts width=$width"
  [ "$toasts" -eq 1 ] || fail "expected one notifications layer over the game, got $toasts"
  [ "$width" -gt 0 ] && [ "$width" -lt 1920 ] || fail "the notifications layer is not a column ($width wide)"
  grep -q 'toasts input empty' "$shell_log_path" || fail "the shell never set an empty input region on the covered stack"

  local osd
  osd=$(_fullscreen_count "$fullscreen_lock_osd_layers" "formalshell:osd")
  echo "osd layers over the game: $osd"
  [ "$osd" -eq 0 ] || fail "the OSD mapped over a covered output"

  [ "$(sed -n 2,3p "$fullscreen_lock_state")" = "$(sed -n 5,6p "$fullscreen_lock_state")" ] \
    || fail "the cursor or the active window moved while the toasts showed"
  echo "SMOKE_FULLSCREEN_LOCK_TOAST $fullscreen_lock_toast_path"

  local phase
  for phase in $fullscreen_fs_phases; do _fullscreen_fs_assert "$phase"; done
}

# One true-fullscreen phase: the window in mode 2 and floating or not as
# asked, the lock re-armed and active going in and never released, the stack
# mapped for the toast and, after it, gone when tiled and held when floating.
_fullscreen_fs_assert() {
  local layout="${1%%:*}" kind="${1#*:}" name="fs-${1%%:*}"
  [ "$kind" = floating ] && name="$name-floating"
  local clients="$shot_dir/fullscreen-$name-clients.json"
  local toast_layers="$shot_dir/fullscreen-$name-toast-layers.json" after_layers="$shot_dir/fullscreen-$name-after-layers.json"
  local f
  for f in "$clients" "$toast_layers" "$after_layers"; do [ -s "$f" ] || fail "no dump produced at $f"; done

  local mode floating
  mode=$("$jq_bin" -r '[.[] | select(.class == "formalshell-lockpointer") | .fullscreen] | first // 0' "$clients")
  floating=$("$jq_bin" -r '[.[] | select(.class == "formalshell-lockpointer") | .floating] | first' "$clients")
  echo "$name: lock window fullscreen mode=$mode floating=$floating"
  [ "$mode" = 2 ] || fail "$name: the lock window was not in fullscreen mode 2"
  [ "$floating" = "$([ "$kind" = floating ] && echo true || echo false)" ] || fail "$name: the lock window was not $kind"

  local arm during
  arm=$(sed -n "/ $name-arm\$/,/ $name\$/p" "$fullscreen_lock_log")
  during=$(sed -n "/ $name\$/,/ $name-end\$/p" "$fullscreen_lock_log")
  echo "$arm" | grep -q ' rearm$' || fail "$name: the probe never re-armed its lock"
  [ "$(echo "$arm" | grep ' locked$\| unlocked$' | tail -1 | awk '{print $2}')" = locked ] \
    || fail "$name: the lock was not active when the toast arrived over the fullscreen game"
  if echo "$during" | grep -q ' unlocked$'; then fail "$name: a toast over the fullscreen game released its pointer lock"; fi

  local up gone osd want=0
  [ "$kind" = floating ] && want=1
  up=$(_fullscreen_count "$toast_layers" "formalshell:notifications")
  gone=$(_fullscreen_count "$after_layers" "formalshell:notifications")
  osd=$(_fullscreen_count "$after_layers" "formalshell:osd")
  echo "$name: notifications layers with the toast up=$up, after it went=$gone (want $want), osd=$osd"
  [ "$up" -eq 1 ] || fail "$name: the toast did not map over the fullscreen game"
  [ "$gone" -eq "$want" ] || fail "$name: the stack after the toast was $gone layers, not $want"
  [ "$osd" -eq 0 ] || fail "$name: the OSD mapped over a fullscreen game"
  local tag="${name#fs-}"; tag="${tag//-/_}"
  echo "SMOKE_FULLSCREEN_FS_${tag^^} $shot_dir/fullscreen-$name.png"
}
