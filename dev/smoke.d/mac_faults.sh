# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154,SC2016  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --mac-faults: what goes wrong round a Mac window and must not cost the
# session its keys or leave the bar wrong
# (docs/superpowers/specs/2026-11-11-mac-apps.md, section 5). The config is
# --mac-keys': the shipped include's MAC APPS block, then SUPER + H and
# SUPER + L moving focus (reserved), SUPER + C a mark in a file (handed to
# a Mac app) and SUPER + CTRL + P a mark (never handed). dev/mac-stub.py is
# the Mac window, a foot window beside it the native one, both tiled
# (formalwm's settings float an `atoll` window, and its column binds step
# through the tiles only), every key wtype's.
# Hyprland's own "not responding" dialog is off: it takes focus from the
# windows the leg reads.
# The shell runs under a launcher of the leg's own, which starts it again
# when the drive leaves a file, so the drive can end it and have it back.
#
# A reload. `hyprctl reload` with the stub in front, which the shell runs
# itself after every theme publish, drops every bind and the submap with
# them: the session is in `mac` again after it, the submap holds H, L and P
# without C, SUPER + CTRL + P leaves its mark and SUPER + C reaches the
# stub.
#
# The shell gone. Ended with the stub in front, the session stays in `mac`
# (the window in front is still a Mac one). SUPER + L moves focus to the
# foot window and the default map is back with no shell to ask for it: the
# include holds the submap over the one window the shell last called a Mac
# window, and SUPER + C leaves its mark. SUPER + H back onto the stub and
# the session is in `mac` again, SUPER + L and it is out.
#
# A reload with the shell gone. Hyprland reloads by itself when its config
# is saved. Back on the stub and in `mac`, `hyprctl reload` drops the
# submap's binds and no shell is there to build them again: the include
# leaves `mac` as it loads, SUPER + L moves focus to the foot window and
# SUPER + C leaves its mark. Two more reloads and one focus change runs the
# include's follow as many times as one did before them: a reload does not
# leave a second pair of handlers behind. The config counts the runs, a
# line in a file for each read of the submap, which the follow makes once.
#
# An ask that arrives late. The reloads left no `mac` submap, so it is built
# by hand as a shell would have. With the foot window in front the include
# is told, as the shell would tell it, that the stub is in front and a Mac
# window: the foot window is not taken for one, the map stays the default
# one and SUPER + C leaves its mark. What was said is kept for the stub:
# SUPER + H onto it and the session is in `mac` with nobody asking again,
# SUPER + L and it is out.
#
# The shell back. With no shell the session is put in `mac` over the foot
# window by hand, which is where a shell that died between a window's focus
# and its own asking leaves it. The shell started again
# asks the session out on its first look at the compositor: the map is the
# default one and SUPER + C leaves its mark. Back on the stub the submap is
# `mac`, the menu is held again, SUPER + C reaches the stub and
# SUPER + CTRL + P leaves its mark.
#
# An app that stops answering. The stub is stopped (SIGSTOP) and its File
# menu asked to open, an AboutToShow it never answers. With focus on the
# foot window the bar shows no titles, whatever the shell still waits on,
# and a second stub in front has its own four titles within the wait a
# call is given: the first one's silence did not hold the menus up.
#
# Nothing left. The shell ended with the second stub the only window in
# front, and then the stub ended too: with no window focused and no shell
# the map is the default one and SUPER + C leaves its mark.
#
# Under formalwm the binds are wm_bind's and the hand over is the shell's
# `binds.handOver` (the shoji spec, section 3), which `submap` reads as
# `mac` while it holds. It ends with the connection that asked: the shell
# gone over the stub, every bind is back at once and SUPER + C leaves its
# mark there, where the include keeps the stub's keys, and with no shell
# the stub in front is on the default map. A reload (the core's control
# socket) keeps a hand over and leaves the binds whole, with the shell and
# without it; formalwm loads no Lua, so there are no handlers to count. The
# late ask is a `binds.handOver` naming the stub sent with the foot window
# in front, on a connection the drive holds as a shell would. Nothing can
# outlive the asker, so the shell back has no stale map to clear: started
# again over the foot window it reads the default map. The rest is the same.
leg_mac_faults_flag="--mac-faults"
leg_mac_faults_order=173
leg_mac_faults_compositors="hyprland shoji"
leg_mac_faults_needs="python3 jq foot wtype"
leg_mac_faults_claims="focus hyprland.binds"

mac_faults_prefix="$shot_dir/mac-faults"
mac_faults_log="$shot_dir/mac-faults-stub.log"
mac_faults_other_log="$shot_dir/mac-faults-other.log"
mac_faults_marks="$shot_dir/mac-faults-marks.txt"
mac_faults_again="$shot_dir/mac-faults-again"
mac_faults_follows="$shot_dir/mac-faults-follows.txt"

leg_mac_faults_timing() {
  leg_timing 150
}

leg_mac_faults_shell() {
  write_script "$1" <<EOF
#!/usr/bin/env bash
if [ "$session_mode" = vkms ]; then
  export LIBGL_ALWAYS_SOFTWARE=1
fi
export RUST_BACKTRACE=1
$wayland_debug_line
while :; do
  $shell_prefix "$shell_bin" >> "$shell_log_path" 2>&1 &
  echo \$! > "$shot_dir/shell.pid"
  wait
  for _ in \$(seq 1 400); do [ -e "$mac_faults_again" ] && break; sleep 0.25; done
  [ -e "$mac_faults_again" ] || break
  rm -f "$mac_faults_again"
  "$wmctl_bin" -j monitors > /dev/null 2>&1 || break
done
EOF
}

leg_mac_faults_drive() {
  if [ "$FS_COMPOSITOR" = shoji ]; then
    wm_bind SUPER+H focus-column left
    wm_bind SUPER+L focus-column right
    wm_bind SUPER+C sh "echo c >> $mac_faults_marks"
    wm_bind SUPER+CTRL+P sh "echo p >> $mac_faults_marks"
  else
    awk '/^-- MAC APPS/ { on = 1 } /^-- Window rounding/ { on = 0 } on { print }' docs/examples/hyprland/formalshell.lua
    cat <<EOF
hl.config({ input = { resolve_binds_by_sym = true }, misc = { enable_anr_dialog = false } })
hl.bind("SUPER + H", hl.dsp.focus({ direction = "l" }))
hl.bind("SUPER + L", hl.dsp.focus({ direction = "r" }))
hl.bind("SUPER + C", hl.dsp.exec_cmd("sh -c 'echo c >> $mac_faults_marks'"))
hl.bind("SUPER + CTRL + P", hl.dsp.exec_cmd("sh -c 'echo p >> $mac_faults_marks'"))
formalshell_faults_loads = (formalshell_faults_loads or 0) + 1
if not formalshell_faults_submap then
  formalshell_faults_submap = hl.get_current_submap
  hl.get_current_submap = function()
    local file = io.open("$mac_faults_follows", "a")
    if file then
      file:write(formalshell_faults_loads, "\n")
      file:close()
    end
    return formalshell_faults_submap()
  end
end
EOF
  fi
  # Which compositor the drive's steps take: the include's (Hyprland) or
  # the hand over's (formalwm), and what the bind table is read from.
  local hypr=true held="-j binds"
  [ "$FS_COMPOSITOR" = shoji ] && hypr=false held=state
  local script="$shot_dir/mac-faults-drive.sh"
  local stub="$PWD/dev/mac-stub.py"
  write_script "$script" <<EOF
#!/usr/bin/env bash
m="$mac_faults_prefix"
log="$mac_faults_log"
marks="$mac_faults_marks"
jq="$jq_bin"
: > "\$marks"
submap() { "$wmctl_bin" submap 2>&1 | head -n 1; }
# in_submap <name> <path>: until Hyprland's submap is that one, 15s at most;
# the one it ended on in <path>.
in_submap() {
  for _ in \$(seq 1 60); do [ "\$(submap)" = "\$1" ] && break; sleep 0.25; done
  submap > "\$2"
}
active() { "$wmctl_bin" -j activewindow 2>/dev/null | "\$jq" -r '.class // ""'; }
front() { for _ in \$(seq 1 40); do [ "\$(active)" = "\$1" ] && return 0; sleep 0.25; done; return 1; }
pid_of() { "$wmctl_bin" -j clients 2>/dev/null | "\$jq" -r --arg c "\$1" 'first(.[] | select(.class == \$c) | .pid) // empty'; }
seen() { sleep 0.6; grep -c "\$1" "\$log" > "\$2" 2>/dev/null || true; }
mark() { sleep 0.6; grep -c "^\$1\\\$" "\$marks" > "\$2" 2>/dev/null || true; }
follows() { wc -l < "$mac_faults_follows" 2>/dev/null | tr -d '[:space:]'; }
# end_shell: the shell ended as a session's logout ends it, and gone.
end_shell() {
  local pid
  pid=\$(cat "$shot_dir/shell.pid")
  kill -TERM "\$pid" 2>/dev/null
  for _ in \$(seq 1 40); do kill -0 "\$pid" 2>/dev/null || break; sleep 0.25; done
}

"$wmctl_bin" dispatch "hl.dsp.exec_cmd([==[$python3_bin $stub --name Stub --log \$log]==])" > /dev/null 2>&1
until_window org.atoll.stub 15
"$wmctl_bin" dispatch "hl.dsp.window.float({ window = 'class:org.atoll.stub', action = 'disable' })" > /dev/null 2>&1
spawn_foot formalshell-mac-faults
until_window formalshell-mac-faults
"$wmctl_bin" dispatch "hl.dsp.focus({ window = 'class:org.atoll.stub' })" > /dev/null 2>&1
front org.atoll.stub
until_jq 15 "\$m-inside.json" '.macKeys and (.titles | length == 4)' call appmenu status
in_submap mac "\$m-submap-start.txt"

"$wmctl_bin" reload > "\$m-reload.txt" 2>&1
sleep 1
in_submap mac "\$m-submap-reload.txt"
"$wmctl_bin" $held > "\$m-held-reload.json" 2>&1
status appmenu "\$m-status-reload.json"
"$wtype_bin" -M logo -M ctrl p -m ctrl -m logo
mark p "\$m-mark-p-reload.txt"
"$wtype_bin" -M logo c -m logo
seen '^key c down mods=64\$' "\$m-super-c-reload.txt"
mark c "\$m-mark-c-reload.txt"

end_shell
if $hypr; then
  submap > "\$m-submap-ended.txt"
else
  in_submap default "\$m-submap-ended.txt"
  "$wtype_bin" -M logo c -m logo
  seen '^key c down mods=64\$' "\$m-super-c-gone.txt"
  mark c "\$m-mark-c-gone.txt"
fi
"$wtype_bin" -M logo l -m logo
front formalshell-mac-faults; echo "\$?" > "\$m-left-stub.txt"
in_submap default "\$m-submap-ended-native.txt"
"$wtype_bin" -M logo c -m logo
mark c "\$m-mark-c-ended.txt"
"$wtype_bin" -M logo h -m logo
front org.atoll.stub
in_submap $($hypr && echo mac || echo default) "\$m-submap-ended-again.txt"
"$wtype_bin" -M logo l -m logo
front formalshell-mac-faults
in_submap default "\$m-submap-ended-out.txt"

before=\$(follows)
"$wtype_bin" -M logo h -m logo
front org.atoll.stub
in_submap $($hypr && echo mac || echo default) "\$m-submap-ended-third.txt"
sleep 0.5
$hypr && echo \$(( \$(follows) - before )) > "\$m-follows-before.txt"
"$wmctl_bin" reload > "\$m-reload-ended.txt" 2>&1
sleep 1
submap > "\$m-submap-ended-reload.txt"
"$wtype_bin" -M logo l -m logo
front formalshell-mac-faults; echo "\$?" > "\$m-left-stub-reload.txt"
"$wtype_bin" -M logo c -m logo
mark c "\$m-mark-c-ended-reload.txt"
"$wmctl_bin" reload > /dev/null 2>&1
sleep 1
"$wmctl_bin" reload > /dev/null 2>&1
sleep 1
before=\$(follows)
"$wtype_bin" -M logo h -m logo
front org.atoll.stub
sleep 0.5
$hypr && echo \$(( \$(follows) - before )) > "\$m-follows-after.txt"
"$wtype_bin" -M logo l -m logo
front formalshell-mac-faults

stub_address=\$("$wmctl_bin" -j clients 2>/dev/null | "\$jq" -r 'first(.[] | select(.class == "org.atoll.stub") | .address) // empty')
if $hypr; then
  "$wmctl_bin" eval 'formalshell_mac_keys({ "SUPER + H", "SUPER + L" })' > /dev/null 2>&1
  "$wmctl_bin" eval "formalshell_mac_focus(true, \"address:\$stub_address\")" > "\$m-late.txt" 2>&1
else
  "$wmctl_bin" hand-over "\$stub_address" "SUPER + H" "SUPER + L" > "\$m-late.txt" 2>&1 &
  late=\$!
  for _ in \$(seq 1 40); do grep -q . "\$m-late.txt" && break; sleep 0.25; done
fi
sleep 0.5
submap > "\$m-submap-late.txt"
"$wtype_bin" -M logo c -m logo
mark c "\$m-mark-c-late.txt"
"$wtype_bin" -M logo h -m logo
front org.atoll.stub
in_submap mac "\$m-submap-late-stub.txt"
"$wtype_bin" -M logo l -m logo
front formalshell-mac-faults
in_submap default "\$m-submap-late-out.txt"

if $hypr; then
  "$wmctl_bin" dispatch 'hl.dsp.submap("mac")' > /dev/null 2>&1
  in_submap mac "\$m-submap-forced.txt"
else
  kill "\$late"
fi
touch "$mac_faults_again"
in_submap default "\$m-submap-back.txt"
until_jq 15 "\$m-outside.json" '.macKeys | not' call appmenu status
"$wtype_bin" -M logo c -m logo
mark c "\$m-mark-c-back.txt"

"$wtype_bin" -M logo h -m logo
front org.atoll.stub; echo "\$?" > "\$m-back-on-stub.txt"
until_jq 15 "\$m-inside-again.json" '.macKeys and (.titles | length == 4)' call appmenu status
in_submap mac "\$m-submap-again.txt"
"$wtype_bin" -M logo c -m logo
seen '^key c down mods=64\$' "\$m-super-c-again.txt"
mark c "\$m-mark-c-again.txt"
"$wtype_bin" -M logo -M ctrl p -m ctrl -m logo
mark p "\$m-mark-p-again.txt"

stub_pid=\$(pid_of org.atoll.stub)
kill -STOP "\$stub_pid"
call appmenu open File > /dev/null 2>&1
sleep 0.5
call appmenu close > /dev/null 2>&1
"$wtype_bin" -M logo l -m logo
front formalshell-mac-faults
until_jq 2 "\$m-hung-away.json" '.titles == []' call appmenu status
shoot "\$m-hung-away.png"
"$wmctl_bin" dispatch "hl.dsp.exec_cmd([==[$python3_bin $stub --name Other --app-id org.atoll.other --color 8a5a3a --log $mac_faults_other_log]==])" > /dev/null 2>&1
until_window org.atoll.other 15
"$wmctl_bin" dispatch "hl.dsp.focus({ window = 'class:org.atoll.other' })" > /dev/null 2>&1
front org.atoll.other
until_jq 10 "\$m-other.json" '.macKeys and (.titles | length == 4) and .titles[0].label == "Other"' call appmenu status
shoot "\$m-other.png"
kill -CONT "\$stub_pid"
kill "\$stub_pid"

pkill -f "foot --app-id=formalshell-mac-faults" 2>/dev/null
for _ in \$(seq 1 40); do [ "\$("$wmctl_bin" -j clients | "\$jq" length)" = 1 ] && break; sleep 0.25; done
front org.atoll.other
until_jq 10 "\$m-alone.json" '.macKeys' call appmenu status
in_submap mac "\$m-submap-alone.txt"
end_shell
kill "\$(pid_of org.atoll.other)"
for _ in \$(seq 1 40); do [ "\$("$wmctl_bin" -j clients | "\$jq" length)" = 0 ] && break; sleep 0.25; done
"$wmctl_bin" -j clients > "\$m-clients-none.json" 2>&1
in_submap default "\$m-submap-none.txt"
"$wtype_bin" -M logo c -m logo
mark c "\$m-mark-c-none.txt"
touch "$mac_faults_again"
until_jq 20 "\$m-last.json" '.macKeys | not' call appmenu status
EOF
  write_script "$shot_dir/mac-faults-kill.sh" <<EOF
#!/usr/bin/env bash
pkill -CONT -f "$stub --name" 2>/dev/null || true
pkill -f "$stub --name" 2>/dev/null || true
EOF
  add_cleanup "bash $shot_dir/mac-faults-kill.sh"
  wm_exec_once "bash $script"
}

leg_mac_faults_assert() {
  local m="$mac_faults_prefix"
  count() { tr -d '[:space:]' < "$1" 2>/dev/null; }
  is() { [ "$(cat "$m-submap-$1.txt" 2>/dev/null)" = "$2" ] || fail "--mac-faults: $3 (the submap reads '$(cat "$m-submap-$1.txt" 2>/dev/null)')"; }
  # formalwm's shell gone over the stub leaves one mark more from there on.
  local hypr=true o=0
  [ "$FS_COMPOSITOR" = shoji ] && hypr=false o=1
  [ -s "$mac_faults_log" ] || fail "--mac-faults: the stub logged nothing"
  cat "$mac_faults_log"
  echo "marks: $(tr '\n' ' ' < "$mac_faults_marks" 2>/dev/null)"
  for shot in hung-away other; do
    [ ! -f "$m-$shot.png" ] || echo "SMOKE_MAC_FAULTS_$(echo "$shot" | tr 'a-z-' 'A-Z_') $m-$shot.png"
  done
  expect_jq "$m-inside.json" '.macKeys and (.titles | length == 4)' "--mac-faults: the stub in front did not put the session in the mac submap with its menu held"
  is start mac "the stub in front did not put the session in the mac submap"

  is reload mac "a reload with the stub in front left the session out of the mac submap"
  if $hypr; then
    expect_jq "$m-held-reload.json" '[.[] | select(.submap == "mac") | .key | ascii_downcase] | index("l") != null and index("h") != null and index("p") != null and index("c") == null' \
      "--mac-faults: the mac submap after a reload does not hold H, L and P without C" '[.[] | {submap, modmask, key}]'
    expect_jq "$m-status-reload.json" '.macBinds == 4' "--mac-faults: appmenu status after a reload does not count the four binds the include recorded" '{macKeys, macBinds}'
  else
    expect_jq "$m-held-reload.json" '.handOver.active and (.handOver.reserved | map(ascii_upcase | gsub(" "; "")) | index("SUPER+H") != null and index("SUPER+L") != null and index("SUPER+C") == null)' \
      "--mac-faults: the hand over after a reload does not hold the stub with H and L reserved and C handed" '.handOver'
  fi
  [ "$(count "$m-mark-p-reload.txt")" = 1 ] || fail "--mac-faults: SUPER + CTRL + P did not fire over the stub after a reload (marks $(count "$m-mark-p-reload.txt"))"
  [ "$(count "$m-super-c-reload.txt")" = 1 ] || fail "--mac-faults: SUPER + C did not reach the stub after a reload (got $(count "$m-super-c-reload.txt"))"
  [ "$(count "$m-mark-c-reload.txt")" = 0 ] || fail "--mac-faults: the window manager's SUPER + C fired over the stub after a reload"

  if $hypr; then
    is ended mac "the shell ending over the stub took the session out of the mac submap while the stub was still in front"
  else
    is ended default "the shell ending over the stub left its hand over holding with nobody to end it"
    [ "$(count "$m-mark-c-gone.txt")" = 1 ] || fail "--mac-faults: SUPER + C did not fire over the stub with the shell gone (marks $(count "$m-mark-c-gone.txt"))"
    [ "$(count "$m-super-c-gone.txt")" = 1 ] || fail "--mac-faults: SUPER + C still reached the stub with the shell gone (got $(count "$m-super-c-gone.txt"))"
  fi
  [ "$(count "$m-left-stub.txt")" = 0 ] || fail "--mac-faults: SUPER + L did not move focus off the stub with the shell gone"
  is ended-native default "with the shell gone, focus on the foot window left the session in the mac submap"
  [ "$(count "$m-mark-c-ended.txt")" = $((1 + o)) ] || fail "--mac-faults: SUPER + C did not fire over the foot window with the shell gone (marks $(count "$m-mark-c-ended.txt"))"
  if $hypr; then
    is ended-again mac "with the shell gone, focus back on the stub did not put the session in the mac submap"
  else
    is ended-again default "with the shell gone, focus back on the stub handed it the keys with nobody to ask"
  fi
  is ended-out default "with the shell gone, focus on the foot window a second time left the session in the mac submap"

  if $hypr; then
    is ended-third mac "with the shell gone, focus on the stub a third time did not put the session in the mac submap"
  else
    is ended-third default "with the shell gone, focus on the stub a third time handed it the keys with nobody to ask"
  fi
  is ended-reload default "a reload with the shell gone and the stub in front left the session in a mac submap with no bind in it"
  [ "$(count "$m-left-stub-reload.txt")" = 0 ] || fail "--mac-faults: SUPER + L did not move focus off the stub after a reload with the shell gone"
  [ "$(count "$m-mark-c-ended-reload.txt")" = $((2 + o)) ] || fail "--mac-faults: SUPER + C did not fire over the foot window after a reload with the shell gone (marks $(count "$m-mark-c-ended-reload.txt"))"
  if $hypr; then
    echo "follow runs for one focus change: $(count "$m-follows-before.txt") before two more reloads, $(count "$m-follows-after.txt") after; config loads seen by a run: $(sort -u "$mac_faults_follows" 2>/dev/null | tr '\n' ' ')"
    [ "$(count "$m-follows-before.txt")" -ge 1 ] 2>/dev/null || fail "--mac-faults: the include's follow did not run on a focus change (runs $(count "$m-follows-before.txt"))"
    [ "$(count "$m-follows-after.txt")" = "$(count "$m-follows-before.txt")" ] \
      || fail "--mac-faults: one focus change ran the include's follow $(count "$m-follows-after.txt") times after two more reloads, $(count "$m-follows-before.txt") before them: a reload left handlers behind"
  fi

  is late default "told of the stub while the foot window was in front, the session took the foot window for a Mac window: $(cat "$m-late.txt" 2>/dev/null)"
  [ "$(count "$m-mark-c-late.txt")" = $((3 + o)) ] || fail "--mac-faults: SUPER + C did not fire over the foot window after an ask about the stub arrived late (marks $(count "$m-mark-c-late.txt"))"
  is late-stub mac "focus on the stub after an ask about it arrived late left the session on the default map with nobody to ask again"
  is late-out default "focus back on the foot window after the late ask left the session in the mac submap"

  $hypr && is forced mac "the session could not be put in the mac submap by hand"
  is back default "the shell started again left the session in the mac submap over the foot window"
  expect_jq "$m-outside.json" '.macKeys | not' "--mac-faults: the shell started again reads the session as in the mac submap over the foot window"
  [ "$(count "$m-mark-c-back.txt")" = $((4 + o)) ] || fail "--mac-faults: SUPER + C did not fire over the foot window after the shell came back (marks $(count "$m-mark-c-back.txt"))"

  [ "$(count "$m-back-on-stub.txt")" = 0 ] || fail "--mac-faults: SUPER + H did not move focus back onto the stub"
  expect_jq "$m-inside-again.json" '.macKeys and (.titles | length == 4)' "--mac-faults: the shell started again does not hold the stub's menu with the stub in front"
  is again mac "the stub in front of the shell started again did not put the session in the mac submap"
  [ "$(count "$m-super-c-again.txt")" = 2 ] || fail "--mac-faults: SUPER + C did not reach the stub after the shell came back (got $(count "$m-super-c-again.txt"))"
  [ "$(count "$m-mark-c-again.txt")" = $((4 + o)) ] || fail "--mac-faults: the window manager's SUPER + C fired over the stub after the shell came back"
  [ "$(count "$m-mark-p-again.txt")" = 2 ] || fail "--mac-faults: SUPER + CTRL + P did not fire over the stub after the shell came back (marks $(count "$m-mark-p-again.txt"))"

  expect_jq "$m-hung-away.json" '.titles == []' "--mac-faults: the bar still shows the titles of a stub that stopped answering, over the foot window" '{pid, titles: [.titles[].label]}'
  expect_jq "$m-other.json" '.macKeys and (.titles | length == 4) and .titles[0].label == "Other"' \
    "--mac-faults: a second stub's menu is not held after the first stopped answering" '{pid, titles: [.titles[].label]}'

  expect_jq "$m-alone.json" '.macKeys' "--mac-faults: the second stub alone in front did not put the session in the mac submap"
  is alone mac "the second stub alone in front did not put the session in the mac submap"
  expect_jq "$m-clients-none.json" '. == []' "--mac-faults: a window was left open for the last step" '[.[] | {class, title}]'
  is none default "with the shell gone and the last Mac window closed, the session stayed in the mac submap"
  [ "$(count "$m-mark-c-none.txt")" = $((5 + o)) ] || fail "--mac-faults: SUPER + C did not fire with the shell gone and no window open (marks $(count "$m-mark-c-none.txt"))"
  expect_jq "$m-last.json" '.macKeys | not' "--mac-faults: the shell did not come back at the end"
  echo "SMOKE_MAC_FAULTS ok reload rebuilt the submap, the default map came back with the shell gone, back and with no window left, a stub that stopped answering held nothing up"
}
