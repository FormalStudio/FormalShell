# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, iso_home, the *_bin paths and fail()
# --spaces (M74): the Spaces cell's own reads, off one session with windows
# on two workspaces. A sibling of --workspaces rather than more of it: that
# leg photographs the pill's travel on a burst clock, and everything here
# waits on agent polls and a real pointer instead, so the two timelines
# would only get in each other's way.
#
# Fixture: the base window plus two windows of one foot server on
# workspace 1, sharing its pid the way every ghostty window does: one under
# the T3 Code desktop app's id (com.t3tools.T3Code), the other a plain
# shell. Two foots on workspace 2 carrying nothing. `workspaces.agentHosts`
# lists fakehost and deadhost. `ssh` is a PATH shim, and the only ssh this
# session can reach: the wrapper suffixes openssh for exactly this, so each
# host's `t3-agents` stream lands here. fakehost answers one thread
# `blocked` (or none once the drive says so) every two seconds; deadhost
# and any other host are refused without ever being handed on. Every
# invocation is logged, which is the proof the poll was taken at all.
#
# What is read, in order:
# - `workspaces status`: every window listed under its own chip with its
#   icon resolved and shown (icons on every occupied chip, not just the
#   one on screen), every chip labelled with its ordinal, the blocked
#   badge on the T3 Code icon, and none on its same-pid sibling or the
#   plain windows, and the chip on screen wider than a bare one.
# - `debug dump`'s agents block agreeing: the T3 Code window alone in
#   stateByWindow, fakehost blocked and deadhost empty in stateByHost.
# - The frame: the blocked icon's badge, read as the difference between the
#   chip with fakehost answering blocked and the same chip once it answers
#   no threads, red-dominant and inside that icon's own box (the icon's rect out
#   of `workspaces status`). The idle dump drops the window from
#   stateByWindow, which is the "nothing for idle" half.
# - A wheel notch over workspace 1's slot (wlrctl, a real axis event) moving
#   focus to workspace 2 and a notch back returning it, off hyprctl. A
#   notch is `scroll 15`: Qt reads wlrctl's continuous axis at 8 per unit,
#   so the 10 wheel.sh sends is an angleDelta of 80, short of the 120 the
#   cell sums to before it steps.
# - `workspaces peek 2` opening the preview on workspace 2 with both of its
#   windows drawn live (the thumbnails' ScreencopyViews holding a frame),
#   taking the keyboard as any panel does; photographed, then closed.
# - A real pointer parked on workspace 2's chip opening the same preview
#   after the hover delay without taking the keyboard, and staying open
#   while the pointer keeps moving over the chip, which is what a hand on a
#   real mouse does and what a card that primes Exclusive focus fails:
#   Hyprland pulls the pointer onto a layer that maps Exclusive and hands
#   it back on the next motion. Moving off it closes it again.
# - A window of workspace 2 floated and moved mostly past the output's right
#   edge (`movewindowpixel exact`), the case a scrolling layout produces on
#   its own: the peeked card's miniature opens scrolled to the output's own
#   region, lays that window out at its real size and place beyond the
#   viewport (its thumbnail as wide as the window's rect at the miniature's
#   scale, not a clamped sliver), and a real wheel notch over the card
#   scrolls the strip along x, then a notch the other way scrolls it back.
#
# The rust shell also logs every commit with its damage, so a
# stretch with the blocked pulse animating is read off that log: every
# damage rect has to sit on that icon's badge, which is what "a badge
# damages only its own rect" means.
leg_spaces_flag="--spaces"
leg_spaces_order=196
leg_spaces_needs="foot jq convert wlrctl"
leg_spaces_fixture_window=keep

spaces_shim_dir="$shot_dir/spaces-shim"
spaces_ssh_calls_path="$shot_dir/spaces-ssh-calls.txt"
spaces_remote_state_path="$shot_dir/spaces-remote-state"
spaces_dispatch_path="$shot_dir/spaces-dispatch.txt"
spaces_foot_socket="$shot_dir/formalshell-spaces-foot.sock"
spaces_done_path="$shot_dir/spaces-done"
spaces_status_one_path="$shot_dir/spaces-status-one.json"
spaces_status_two_path="$shot_dir/spaces-status-two.json"
spaces_status_peek_path="$shot_dir/spaces-status-peek.json"
spaces_status_hover_path="$shot_dir/spaces-status-hover.json"
spaces_status_left_path="$shot_dir/spaces-status-left.json"
spaces_status_moved_path="$shot_dir/spaces-status-moved.json"
spaces_status_pad_path="$shot_dir/spaces-status-pad.json"
spaces_status_pad_back_path="$shot_dir/spaces-status-pad-back.json"
spaces_status_wheel_path="$shot_dir/spaces-status-wheel.json"
spaces_status_wheel_back_path="$shot_dir/spaces-status-wheel-back.json"
spaces_status_quiet_path="$shot_dir/spaces-status-quiet.json"
spaces_dump_path="$shot_dir/spaces-dump.json"
spaces_dump_idle_path="$shot_dir/spaces-dump-idle.json"
spaces_peek_reply_path="$shot_dir/spaces-peek-reply.txt"
spaces_ws_down_path="$shot_dir/spaces-ws-down.txt"
spaces_ws_up_path="$shot_dir/spaces-ws-up.txt"
spaces_blocked_png="$shot_dir/spaces-blocked.png"
spaces_idle_png="$shot_dir/spaces-idle.png"
spaces_two_png="$shot_dir/spaces-two.png"
spaces_peek_png="$shot_dir/spaces-peek.png"
spaces_wheel_png="$shot_dir/spaces-wheel.png"
spaces_hover_png="$shot_dir/spaces-hover.png"
spaces_left_png="$shot_dir/spaces-left.png"
spaces_crop_dir="$shot_dir/spaces-crops"

leg_spaces_validate() {
  local other
  for other in workspaces wheel switcher switcher_keys tooltip tooltip_travel notify_close hotcorner_relock; do
    if leg_on "$other"; then
      echo "usage: --spaces drives the pointer and the focused workspace and cannot combine with --${other//_/-}" >&2
      exit 1
    fi
  done
}

leg_spaces_fixture() {
  local name colour
  mkdir -p "$spaces_shim_dir"
  echo blocked > "$spaces_remote_state_path"
  settings_fragment ', "workspaces": {"agentHosts": ["fakehost", "deadhost"]}'
  cat > "$spaces_shim_dir/ssh" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$*" >> "$spaces_ssh_calls_path"
args=" \$* "
host=""
while [ \$# -gt 0 ]; do
  case "\$1" in
    -o) shift 2 ;;
    -*) shift ;;
    *) host="\$1"; break ;;
  esac
done
if [ "\$host" != fakehost ]; then
  printf 'refused %s\n' "\$host" >> "$spaces_ssh_calls_path"
  exit 255
fi
# A real remote whose bash, run by sshd, sources a ~/.bashrc that starts
# an interactive shell on the channel's stdin. Short of -n that shell waits
# for an EOF the shell's open stdin pipe never sends.
case "\$args" in
  *" -n "*) ;;
  *) cat > /dev/null ;;
esac
# t3-agents' own shape, a thread title with a non-ASCII glyph, only the
# status driven.
while :; do
  state=\$(cat "$spaces_remote_state_path" 2>/dev/null)
  if [ "\$state" = blocked ]; then
    printf '%s\n' '{"environment": "e0", "threads": [{"id": "t1", "title": "\u25d1 Fixture thread", "project": "rig", "status": "blocked", "seq": "2026-01-01T00:00:00.000Z"}]}'
  else
    printf '%s\n' '{"environment": "e0", "threads": []}'
  fi
  sleep 2
done
EOF
  chmod +x "$spaces_shim_dir/ssh"
  # clipssh.sh's route onto the shell's PATH: the rig's own environment is
  # what Hyprland, the foots and the shell's pollers all inherit.
  export PATH="$spaces_shim_dir:$PATH"

  # One entry and one flat icon per fixture app id, so every window has an
  # icon of its own and none of them is red, which the badge read relies on.
  mkdir -p "$iso_home/.local/share/applications" "$iso_home/.local/share/icons/hicolor/48x48/apps"
  for name in com.t3tools.T3Code:'#3A7BD5' formalshell-spaces-sibling:'#9A6AD5' formalshell-spaces-other:'#3AAA5D' formalshell-spaces-plain:'#8A8A8A'; do
    colour=${name#*:}
    name=${name%%:*}
    cat > "$iso_home/.local/share/applications/$name.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Spaces $name
Exec=true
Icon=$name
EOF
    $convert_bin -size 48x48 "xc:$colour" "$iso_home/.local/share/icons/hicolor/48x48/apps/$name.png"
  done
  # The base run writes the same index only when every leg keeps its
  # fixture window, which a rider like --frame does not, and QIconLoader
  # enumerates nothing under hicolor without one.
  cat > "$iso_home/.local/share/icons/hicolor/index.theme" <<'EOF'
[Icon Theme]
Name=Hicolor
Comment=Fallback icon theme
Directories=48x48/apps

[48x48/apps]
Size=48
Context=Applications
Type=Threshold
EOF
}

leg_spaces_timing() {
  leg_timing 80 120
}

leg_spaces_drive() {
  local script="$shot_dir/spaces-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
call() { $ipc call "\$@"; }
centre() { "$jq_bin" -r ".slots[\$1].rect | \"\\(.x + (.width / 2) | floor) \\(.y + (.height / 2) | floor)\"" "\$2"; }
card_centre() { "$jq_bin" -r ".preview.rect | \"\\(.x + (.width / 2) | floor) \\(.y + (.height / 2) | floor)\"" "\$1"; }
park() {
  "$wlrctl_bin" pointer move -4000 -4000 >> "$spaces_dispatch_path" 2>&1
  sleep 0.5
  "$wlrctl_bin" pointer move "\$1" "\$2" >> "$spaces_dispatch_path" 2>&1
}
sleep 4
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --server=$spaces_foot_socket]==])" > "$spaces_dispatch_path" 2>&1
sleep 1
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[${foot_bin%/*}/footclient --server-socket=$spaces_foot_socket --app-id=com.t3tools.T3Code sh -c 'sleep 300']==])" >> "$spaces_dispatch_path" 2>&1
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[${foot_bin%/*}/footclient --server-socket=$spaces_foot_socket --app-id=formalshell-spaces-sibling sh -c 'sleep 300']==])" >> "$spaces_dispatch_path" 2>&1
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-spaces-other sh -c 'sleep 300']==], { workspace = '2 silent' })" >> "$spaces_dispatch_path" 2>&1
sleep 2
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-spaces-plain sh -c 'sleep 300']==], { workspace = '2 silent' })" >> "$spaces_dispatch_path" 2>&1
sleep 2
# Workspace 2's plain window floated and put mostly past the output's right
# edge, its real rect kept as it is. Fully past it Hyprland exports no
# frames for it and the capture count below would drop.
out_w=\$("$hyprctl_bin" monitors -j | "$jq_bin" -r '.[0] | (.width / .scale) | floor')
"$hyprctl_bin" dispatch "hl.dsp.window.float({ window = 'class:^(formalshell-spaces-plain)\$', action = 'toggle' })" >> "$spaces_dispatch_path" 2>&1
"$hyprctl_bin" dispatch "hl.dsp.window.resize({ x = 640, y = 400, window = 'class:^(formalshell-spaces-plain)\$' })" >> "$spaces_dispatch_path" 2>&1
"$hyprctl_bin" dispatch "hl.dsp.window.move({ x = \$((out_w - 200)), y = 150, window = 'class:^(formalshell-spaces-plain)\$' })" >> "$spaces_dispatch_path" 2>&1
# A footclient window belongs to the server's pid, which exec's window rules
# never see, so both are moved by class instead.
"$hyprctl_bin" dispatch "hl.dsp.window.move({ workspace = 1, follow = false, window = 'class:^(com\\.t3tools\\.T3Code)\$' })" >> "$spaces_dispatch_path" 2>&1
"$hyprctl_bin" dispatch "hl.dsp.window.move({ workspace = 1, follow = false, window = 'class:^(formalshell-spaces-sibling)\$' })" >> "$spaces_dispatch_path" 2>&1
sleep 6
call workspaces status > "$spaces_status_one_path" 2>&1
call debug dump > "$spaces_dump_path" 2>&1
"$grim_bin" "$spaces_blocked_png" > /dev/null 2>&1
echo idle > "$spaces_remote_state_path"
sleep 5
call debug dump > "$spaces_dump_idle_path" 2>&1
"$grim_bin" "$spaces_idle_png" > /dev/null 2>&1
echo blocked > "$spaces_remote_state_path"
sleep 4
# A stretch with the blocked pulse running and nothing else moving, clear of the clock's minute edge.
while s=\$(date +%S); [ "\$((10#\$s))" -ge 52 ] || [ "\$((10#\$s))" -lt 2 ]; do sleep 1; done
call debug query spaces-quiet-start
sleep 3
call debug query spaces-quiet-end
call workspaces status > "$spaces_status_quiet_path" 2>&1
park \$(centre 0 "$spaces_status_one_path")
sleep 1
"$wlrctl_bin" pointer scroll 15 0 >> "$spaces_dispatch_path" 2>&1
sleep 2
"$hyprctl_bin" activeworkspace -j > "$spaces_ws_down_path" 2>&1
call workspaces status > "$spaces_status_two_path" 2>&1
"$grim_bin" "$spaces_two_png" > /dev/null 2>&1
"$wlrctl_bin" pointer scroll -15 0 >> "$spaces_dispatch_path" 2>&1
sleep 2
"$hyprctl_bin" activeworkspace -j > "$spaces_ws_up_path" 2>&1
park 960 900
sleep 1
call workspaces peek 2 > "$spaces_peek_reply_path" 2>&1
sleep 2
call workspaces status > "$spaces_status_peek_path" 2>&1
"$grim_bin" "$spaces_peek_png" > /dev/null 2>&1
park \$(card_centre "$spaces_status_peek_path")
sleep 1
"$wlrctl_bin" pointer scroll 15 0 >> "$spaces_dispatch_path" 2>&1
sleep 1
call workspaces status > "$spaces_status_wheel_path" 2>&1
"$grim_bin" "$spaces_wheel_png" > /dev/null 2>&1
"$wlrctl_bin" pointer scroll -15 0 >> "$spaces_dispatch_path" 2>&1
sleep 1
call workspaces status > "$spaces_status_wheel_back_path" 2>&1
call workspaces close >> "$spaces_peek_reply_path" 2>&1
sleep 2
park \$(centre 1 "$spaces_status_one_path")
sleep 2
call workspaces status > "$spaces_status_hover_path" 2>&1
"$grim_bin" "$spaces_hover_png" > /dev/null 2>&1
for step in 3 -3 3 -3 3 -3 3 -3 3 -3; do
  "$wlrctl_bin" pointer move "\$step" 0 >> "$spaces_dispatch_path" 2>&1
  sleep 0.15
done
sleep 0.5
call workspaces status > "$spaces_status_moved_path" 2>&1
# Still the hover-opened card, no keyboard: the pointer crosses onto it and a
# horizontal axis event (what a trackpad's sideways swipe sends) scrolls the
# strip without the card closing.
read -r chip_x chip_y <<< "\$(centre 1 "$spaces_status_one_path")"
read -r card_x card_y <<< "\$(card_centre "$spaces_status_moved_path")"
"$wlrctl_bin" pointer move \$((card_x - chip_x)) \$((card_y - chip_y)) >> "$spaces_dispatch_path" 2>&1
sleep 1
"$wlrctl_bin" pointer scroll 0 15 >> "$spaces_dispatch_path" 2>&1
sleep 1
call workspaces status > "$spaces_status_pad_path" 2>&1
"$wlrctl_bin" pointer scroll 0 -15 >> "$spaces_dispatch_path" 2>&1
sleep 1
call workspaces status > "$spaces_status_pad_back_path" 2>&1
park 960 900
sleep 2
call workspaces status > "$spaces_status_left_path" 2>&1
"$grim_bin" "$spaces_left_png" > /dev/null 2>&1
touch "$spaces_done_path"
EOF
  add_cleanup "pkill -f formalshell-spaces- 2>/dev/null || true"
  hypr_exec_once "bash $script"
}

# The icon a fixture app id resolved to in one status read, as a jq object
# carrying its slot index and its place among that slot's icons.
spaces_icon() {
  "$jq_bin" -c --arg app "$2" \
    '[.slots | to_entries[] | .key as $s | .value.icons | to_entries[] | select(.value.appId == $app) | .value + {slot: $s, at: .key}] | first' "$1"
}

# Every commit the rust shell made between the two quiet markers, read off
# its log: the bar's damage rects, each of which has to lie on the badge of
# an icon marked working or blocked (the icon's own box grown by the
# badge's reach), and enough of them that the animations really ran.
spaces_badge_damage() {
  local t0 t1 boxes commits verdict
  t0=$(sed -n 's/^ipc t=\([0-9]*\)ms .*spaces-quiet-start.*/\1/p' "$shell_log_path" | head -n 1)
  t1=$(sed -n 's/^ipc t=\([0-9]*\)ms .*spaces-quiet-end.*/\1/p' "$shell_log_path" | head -n 1)
  [ -n "$t0" ] && [ -n "$t1" ] || fail "the shell log carries no quiet-stretch markers"
  boxes=$("$jq_bin" -r '[.slots[].icons[] | select(.agent == "working" or .agent == "blocked") | .rect | "\(.x - 10),\(.y - 10),\(.x + .width + 10),\(.y + .height + 10)"] | join(";")' "$spaces_status_quiet_path")
  echo "badge boxes: $boxes"
  [ -n "$boxes" ] || fail "no icon carries a working or blocked badge in the quiet stretch"
  verdict=$(grep '^commit surface=bar ' "$shell_log_path" | awk -v t0="$t0" -v t1="$t1" -v boxes="$boxes" '
    BEGIN { n = split(boxes, b, ";") }
    {
      if (match($0, / t=[0-9]+ms/) == 0) next
      t = substr($0, RSTART + 3, RLENGTH - 5) + 0
      if (t <= t0 || t > t1) next
      commits++
      if (match($0, /damage=\[[^]]*\]/) == 0) next
      list = substr($0, RSTART + 8, RLENGTH - 9)
      m = split(list, rects, " ")
      for (i = 1; i <= m; i++) {
        split(rects[i], p, /[,x]/)
        x0 = p[1]; y0 = p[2]; x1 = p[1] + p[3]; y1 = p[2] + p[4]
        inside = 0
        for (k = 1; k <= n; k++) {
          split(b[k], q, ",")
          if (x0 >= q[1] && y0 >= q[2] && x1 <= q[3] && y1 <= q[4]) inside = 1
        }
        if (!inside) { bad++; print "outside: " rects[i] " in " $0 > "/dev/stderr" }
        rectsn++
      }
    }
    END { printf "commits=%d rects=%d outside=%d\n", commits, rectsn, bad }')
  echo "quiet stretch t=${t0}..${t1}ms: $verdict"
  commits=${verdict#commits=}
  commits=${commits%% *}
  [ "${commits:-0}" -ge 20 ] || fail "the spinner stretch made only $commits commits: the badges did not animate ($verdict)"
  case "$verdict" in
    *"outside=0") ;;
    *) fail "a commit while the badges animated damaged more than a badge ($verdict)" ;;
  esac
}

leg_spaces_assert() {
  local f blocked other plain sibling blocked_id sibling_id pids slot_one slot_bare
  [ -f "$spaces_done_path" ] || fail "the spaces drive never finished; last dispatch output: $(tail -n 5 "$spaces_dispatch_path" 2>/dev/null)"
  for f in "$spaces_status_one_path" "$spaces_dump_path" "$spaces_dump_idle_path" "$spaces_status_quiet_path" \
    "$spaces_status_two_path" "$spaces_status_peek_path" "$spaces_status_hover_path" \
    "$spaces_status_moved_path" "$spaces_status_left_path" \
    "$spaces_status_pad_path" "$spaces_status_pad_back_path" \
    "$spaces_status_wheel_path" "$spaces_status_wheel_back_path"; do
    [ -s "$f" ] || fail "no read produced at $f"
  done
  cat "$spaces_status_one_path"; echo
  cat "$spaces_dispatch_path" 2>/dev/null || true
  echo "ssh shim calls:"; cat "$spaces_ssh_calls_path" 2>/dev/null || true

  # Both hosts were asked, through t3-agents, and no other.
  grep -q 'fakehost .local/bin/t3-agents' "$spaces_ssh_calls_path" 2>/dev/null \
    || fail "no t3-agents ssh call for fakehost: the agent host was never polled"
  grep -q '^refused deadhost' "$spaces_ssh_calls_path" \
    || fail "deadhost was never polled"
  if grep '^refused' "$spaces_ssh_calls_path" | grep -vq '^refused deadhost'; then
    fail "the shell asked ssh for a host not in agentHosts: $(grep '^refused' "$spaces_ssh_calls_path")"
  fi

  # Chips: the ordinal on every one, whatever the compositor names it.
  "$jq_bin" -e '.slots | all(.label == (.idx | tostring))' "$spaces_status_one_path" > /dev/null \
    || fail "a chip is labelled with something other than its ordinal: $("$jq_bin" -c '[.slots[] | .label]' "$spaces_status_one_path")"

  # Icons: every fixture window under its own chip, each resolved, and
  # shown on every chip that holds a window, not only the one on screen.
  blocked=$(spaces_icon "$spaces_status_one_path" com.t3tools.T3Code)
  other=$(spaces_icon "$spaces_status_one_path" formalshell-spaces-other)
  plain=$(spaces_icon "$spaces_status_one_path" formalshell-spaces-plain)
  sibling=$(spaces_icon "$spaces_status_one_path" formalshell-spaces-sibling)
  echo "blocked: $blocked"; echo "sibling: $sibling"; echo "other: $other"; echo "plain: $plain"
  echo "agents in debug dump: $("$jq_bin" -c .agents "$spaces_dump_path")"
  "$jq_bin" -c '.windows[] | select(.appId | test("^(formalshell-spaces-|com\\.t3tools\\.)")) | {appId, pid, title}' "$spaces_dump_path"
  [ "$(echo "$blocked" | "$jq_bin" -r .slot)" = 0 ] || fail "the T3 Code window is not under workspace 1's slot: $blocked"
  [ "$(echo "$other" | "$jq_bin" -r .slot)" = 1 ] || fail "the other window is not under workspace 2's slot: $other"
  [ "$(echo "$plain" | "$jq_bin" -r .slot)" = 1 ] || fail "the plain window is not under workspace 2's slot: $plain"
  [ "$(echo "$sibling" | "$jq_bin" -r .slot)" = 0 ] || fail "the sibling window is not under workspace 1's slot: $sibling"
  for f in "$blocked" "$sibling" "$other" "$plain"; do
    [ "$(echo "$f" | "$jq_bin" -r .icon)" = true ] || fail "a fixture window resolved no icon: $f"
  done
  "$jq_bin" -e '[.slots[] | select(.icons | length > 0) | .appsShown] | all' "$spaces_status_one_path" > /dev/null \
    || fail "an occupied chip hides its icons: $("$jq_bin" -c '[.slots[] | {idx, appsShown}]' "$spaces_status_one_path")"
  # Only there when every leg in the run keeps the base fixture window.
  if $fixture_window_mode; then
    "$jq_bin" -e '.slots[0].icons | map(select(.appId == "formalshell-smoke-iconic")) | length == 1' "$spaces_status_one_path" > /dev/null \
      || fail "the base fixture window is not under workspace 1's slot"
  fi

  # Badges in status, then the same answer in the service itself.
  [ "$(echo "$blocked" | "$jq_bin" -r .agent)" = blocked ] || fail "the T3 Code window carries no blocked badge: $blocked"
  [ "$(echo "$other" | "$jq_bin" -r .agent)" = "" ] || fail "a window that is not T3 Code carries a badge: $other"
  [ "$(echo "$plain" | "$jq_bin" -r .agent)" = "" ] || fail "a window that is not T3 Code carries a badge: $plain"
  [ "$(echo "$sibling" | "$jq_bin" -r .agent)" = "" ] \
    || fail "the plain window sharing the T3 Code window's pid carries its badge: $sibling"
  blocked_id=$(echo "$blocked" | "$jq_bin" -r .id)
  sibling_id=$(echo "$sibling" | "$jq_bin" -r .id)
  pids=$("$jq_bin" -r --arg a "$blocked_id" --arg b "$sibling_id" '[.windows[] | select(.id == $a or .id == $b) | .pid] | unique | map(tostring) | join(" ")' "$spaces_dump_path")
  echo "blocked and sibling pids: $pids"
  case "$pids" in ""|*" "*) fail "the blocked and sibling windows do not share one pid: '$pids'" ;; esac
  [ "$("$jq_bin" -r '.agents.stateByWindow | keys | join(" ")' "$spaces_dump_path")" = "$blocked_id" ] \
    || fail "debug dump's stateByWindow badges something other than the T3 Code window $blocked_id"
  [ "$("$jq_bin" -r --arg id "$blocked_id" '.agents.stateByWindow[$id] // ""' "$spaces_dump_path")" = blocked ] \
    || fail "debug dump's stateByWindow has no blocked entry for $blocked_id"
  [ "$("$jq_bin" -r '.agents.stateByHost.fakehost // ""' "$spaces_dump_path")" = blocked ] \
    || fail "debug dump's stateByHost has no blocked fakehost"
  [ "$("$jq_bin" -r '.agents.stateByHost.deadhost // ""' "$spaces_dump_path")" = "" ] \
    || fail "the unreachable deadhost carries a state in stateByHost"
  "$jq_bin" -c .agents "$spaces_dump_idle_path"
  [ "$("$jq_bin" -r --arg id "$blocked_id" '.agents.stateByWindow[$id] // ""' "$spaces_dump_idle_path")" = "" ] \
    || fail "fakehost answering no threads left $blocked_id in stateByWindow"

  # The slot on screen is wider than a bare one.
  slot_one=$("$jq_bin" -r '.slots[0].extent' "$spaces_status_one_path")
  slot_bare=$("$jq_bin" -r '[.slots[] | select(.icons | length == 0) | .extent] | first // 0' "$spaces_status_one_path")
  echo "workspace 1 slot $slot_one wide, a bare slot $slot_bare"
  "$jq_bin" -e '.slots[0].active and .slots[0].appsShown' "$spaces_status_one_path" > /dev/null \
    || fail "workspace 1's slot is not the active one showing its icons"
  [ "${slot_one%.*}" -gt "${slot_bare%.*}" ] || fail "the active slot ($slot_one) is no wider than a bare one ($slot_bare)"

  for f in "$spaces_blocked_png" "$spaces_idle_png" "$spaces_two_png" "$spaces_peek_png" "$spaces_hover_png" "$spaces_left_png"; do
    [ -f "$f" ] || fail "no spaces screenshot produced at $f"
  done
  echo "SMOKE_SPACES_BLOCKED $spaces_blocked_png"
  echo "SMOKE_SPACES_IDLE $spaces_idle_png"
  echo "SMOKE_SPACES_TWO $spaces_two_png"
  echo "SMOKE_SPACES_PEEK $spaces_peek_png"
  echo "SMOKE_SPACES_HOVER $spaces_hover_png"
  echo "SMOKE_SPACES_LEFT $spaces_left_png"

  # The badge in the frame: what changed in workspace 1's chip when fakehost
  # went from a blocked thread to none, which has to sit on the blocked icon and be
  # red. The icon's own rect comes out of the status read.
  local geo sx sy sw sh icon_left icon_width diff_box dx dw red_on red_off
  read -r sx sy sw sh < <("$jq_bin" -r '.slots[0].rect | "\(.x) \(.y) \(.width) \(.height)"' "$spaces_status_one_path")
  icon_left=$(echo "$blocked" | "$jq_bin" -r .rect.x)
  icon_width=$(echo "$blocked" | "$jq_bin" -r .rect.width)
  geo="${sw}x${sh}+${sx}+${sy}"
  mkdir -p "$spaces_crop_dir"
  $convert_bin "$spaces_blocked_png" -crop "$geo" +repage "$spaces_crop_dir/slot-blocked.png" \
    || fail "could not crop workspace 1's slot at $geo"
  $convert_bin "$spaces_idle_png" -crop "$geo" +repage "$spaces_crop_dir/slot-idle.png"
  echo "SMOKE_SPACES_SLOT_BLOCKED $spaces_crop_dir/slot-blocked.png"
  echo "SMOKE_SPACES_SLOT_IDLE $spaces_crop_dir/slot-idle.png"
  diff_box=$($convert_bin "$spaces_crop_dir/slot-blocked.png" "$spaces_crop_dir/slot-idle.png" \
    -compose difference -composite -threshold 10% -format '%@' info: 2>/dev/null)
  echo "slot $geo, blocked icon from x=$icon_left, badge diff box $diff_box"
  dw=${diff_box%%x*}
  dx=$(echo "$diff_box" | sed -n 's/^[0-9]*x[0-9]*+\([0-9]*\)+[0-9]*$/\1/p')
  [ -n "$dx" ] && [ "${dw:-0}" -gt 0 ] && [ "$diff_box" != "0x0+0+0" ] \
    || fail "workspace 1's slot looks the same with fakehost blocked and idle: no badge drawn"
  dx=$((dx + sx))
  if [ "$dx" -lt $((icon_left - 3)) ] || [ $((dx + dw)) -gt $((icon_left + icon_width + 4)) ]; then
    fail "the badge change spans x=$dx..$((dx + dw)), outside the blocked icon at $icon_left..$((icon_left + icon_width))"
  fi
  # Redness as the mean of red over the other two, read off both frames in
  # the same box. The badge pulses between 0.4 and 1, so a fixed colour
  # match would depend on the phase a frame caught; the blocked frame only
  # has to be clearly redder than the idle one, whose box holds a blue icon.
  local red='%[fx:mean.r - (mean.g + mean.b) / 2]'
  red_on=$($convert_bin "$spaces_crop_dir/slot-blocked.png" -crop "$diff_box" +repage -format "$red" info: 2>/dev/null)
  red_off=$($convert_bin "$spaces_crop_dir/slot-idle.png" -crop "$diff_box" +repage -format "$red" info: 2>/dev/null)
  echo "redness in the badge box: blocked $red_on, idle $red_off"
  awk -v on="$red_on" -v off="$red_off" 'BEGIN { exit !(on != "" && off != "" && on > off + 0.1) }' \
    || fail "the blocked badge is not drawn in the destructive colour: redness $red_on against $red_off idle"

  # The wheel: one notch down to workspace 2, one back.
  echo "after notch down: $("$jq_bin" -c '{id, name}' "$spaces_ws_down_path" 2>/dev/null)"
  echo "after notch up: $("$jq_bin" -c '{id, name}' "$spaces_ws_up_path" 2>/dev/null)"
  [ "$("$jq_bin" -r .id "$spaces_ws_down_path")" = 2 ] || fail "a wheel notch over the cell did not move focus to workspace 2"
  [ "$("$jq_bin" -r .id "$spaces_ws_up_path")" = 1 ] || fail "a wheel notch back did not return focus to workspace 1"
  "$jq_bin" -e '.slots[1].active' "$spaces_status_two_path" > /dev/null \
    || fail "workspace 2's slot is not active after the notch: $(cat "$spaces_status_two_path")"

  spaces_badge_damage

  # The preview, by IPC and by pointer.
  cat "$spaces_peek_reply_path"
  "$jq_bin" -c .preview "$spaces_status_peek_path" "$spaces_status_hover_path" "$spaces_status_moved_path" "$spaces_status_left_path"
  grep -q '^ok$' "$spaces_peek_reply_path" || fail "workspaces peek 2 did not answer ok: $(cat "$spaces_peek_reply_path")"
  "$jq_bin" -e '.preview.open and .preview.idx == 2 and .preview.windows == 2 and .preview.keyboard' "$spaces_status_peek_path" > /dev/null \
    || fail "peek 2 did not open workspace 2's preview with two windows and the keyboard: $("$jq_bin" -c .preview "$spaces_status_peek_path")"
  "$jq_bin" -e '.preview.captured == 2' "$spaces_status_peek_path" > /dev/null \
    || fail "peek 2's thumbnails are not both live window captures: $("$jq_bin" -c .preview "$spaces_status_peek_path")"
  "$jq_bin" -e '.preview.open and .preview.idx == 2 and .preview.captured == 2 and (.preview.keyboard | not)' "$spaces_status_hover_path" > /dev/null \
    || fail "a pointer parked on workspace 2's chip did not open its live preview without the keyboard: $("$jq_bin" -c .preview "$spaces_status_hover_path")"
  "$jq_bin" -e '.preview.open and .preview.idx == 2' "$spaces_status_moved_path" > /dev/null \
    || fail "the preview closed while the pointer kept moving over workspace 2's chip: $("$jq_bin" -c .preview "$spaces_status_moved_path")"

  # The window parked past the right edge: the miniature opened on the
  # output's own region over a strip wider than itself, drew that window
  # whole beyond the viewport, and a wheel notch scrolls the strip along x.
  local mini before after back
  mini=$("$jq_bin" -c .preview.miniature "$spaces_status_peek_path")
  echo "miniature at peek: $mini"
  "$jq_bin" -e '.content.width > .view.width and .scroll == .home' <<< "$mini" > /dev/null \
    || fail "the miniature did not open on the output's own region over a wider strip: $mini"
  "$jq_bin" -e '
    .scale as $k | .inset as $i
    | (.thumbs | max_by(.rect.x)) as $t
    | ($t.x + $t.width) > .view.width
      and (($t.width - ($t.rect.width * $k - 2 * $i)) | fabs) <= 2
      and (($t.height - ($t.rect.height * $k - 2 * $i)) | fabs) <= 2' <<< "$mini" > /dev/null \
    || fail "the window past the output's edge is not drawn whole beyond the viewport: $mini"
  before=$("$jq_bin" -r .preview.miniature.scroll.x "$spaces_status_peek_path")
  after=$("$jq_bin" -r .preview.miniature.scroll.x "$spaces_status_wheel_path")
  back=$("$jq_bin" -r .preview.miniature.scroll.x "$spaces_status_wheel_back_path")
  echo "miniature scroll x: peek $before, after a notch $after, after the reverse notch $back"
  [ "$after" -gt "$before" ] || fail "a wheel notch over the preview did not scroll the miniature right: $before -> $after"
  [ "$back" -lt "$after" ] || fail "the reverse wheel notch did not scroll the miniature back: $after -> $back"
  local pad pad_back
  pad=$("$jq_bin" -r .preview.miniature.scroll.x "$spaces_status_pad_path")
  pad_back=$("$jq_bin" -r .preview.miniature.scroll.x "$spaces_status_pad_back_path")
  echo "hover-opened card, horizontal axis: after a sideways notch $pad, back $pad_back"
  "$jq_bin" -e '.preview.open and .preview.idx == 2 and (.preview.keyboard | not)' "$spaces_status_pad_path" > /dev/null \
    || fail "the hover-opened preview closed under a horizontal scroll: $("$jq_bin" -c .preview "$spaces_status_pad_path")"
  [ "$pad" -gt 0 ] || fail "a horizontal axis event over the hover-opened preview did not scroll the miniature: $pad"
  [ "$pad_back" -lt "$pad" ] || fail "the reverse horizontal event did not scroll the miniature back: $pad -> $pad_back"
  [ -f "$spaces_wheel_png" ] || fail "no spaces screenshot produced at $spaces_wheel_png"
  echo "SMOKE_SPACES_WHEEL $spaces_wheel_png"

  # The card and the chip row, cropped for reading.
  local card chips
  card=$("$jq_bin" -r '.preview.rect | "\(.width)x\(.height)+\(.x)+\(.y)"' "$spaces_status_peek_path")
  chips=$("$jq_bin" -r '[.slots[].rect] | "\((map(.x + .width) | max) - (map(.x) | min) + 8)x\((map(.y + .height) | max) - (map(.y) | min) + 8)+\((map(.x) | min) - 4)+\((map(.y) | min) - 4)"' "$spaces_status_one_path")
  $convert_bin "$spaces_peek_png" -crop "$card" +repage "$spaces_crop_dir/peek-card.png" \
    || fail "could not crop the preview card at $card"
  $convert_bin "$spaces_wheel_png" -crop "$card" +repage "$spaces_crop_dir/wheel-card.png" || true
  echo "SMOKE_SPACES_WHEEL_CARD $spaces_crop_dir/wheel-card.png"
  $convert_bin "$spaces_hover_png" -crop "$card" +repage "$spaces_crop_dir/hover-card.png" || true
  $convert_bin "$spaces_blocked_png" -crop "$chips" +repage -scale 300% "$spaces_crop_dir/chips.png" \
    || fail "could not crop the chip row at $chips"
  echo "SMOKE_SPACES_PEEK_CARD $spaces_crop_dir/peek-card.png"
  echo "SMOKE_SPACES_HOVER_CARD $spaces_crop_dir/hover-card.png"
  echo "SMOKE_SPACES_CHIPS $spaces_crop_dir/chips.png"
  "$jq_bin" -e '.preview.open | not' "$spaces_status_left_path" > /dev/null \
    || fail "moving the pointer off the slot left the preview open: $("$jq_bin" -c .preview "$spaces_status_left_path")"
}
