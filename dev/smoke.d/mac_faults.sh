# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154,SC2016  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --mac-faults: what goes wrong round a Mac window and must not cost the
# session its keys or leave the bar wrong
# (docs/superpowers/specs/2026-11-11-mac-apps.md, section 5). The config is
# --mac-keys': the shipped include's MAC APPS block, then SUPER + H and
# SUPER + L moving focus (reserved), SUPER + C a mark in a file (handed to
# a Mac app) and SUPER + CTRL + P a mark (never handed). dev/mac-stub.py is
# the Mac window, a foot window beside it the native one, every key wtype's.
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
# The shell back. With no shell the session is put in `mac` over the foot
# window by hand, which is where a shell that died between a window's focus
# and its own asking leaves it. The shell started again asks the session out
# on its first look at the compositor: the map is the default one and
# SUPER + C leaves its mark. Back on the stub the submap is `mac`, the menu
# is held again, SUPER + C reaches the stub and SUPER + CTRL + P leaves its
# mark.
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
leg_mac_faults_flag="--mac-faults"
leg_mac_faults_order=173
leg_mac_faults_needs="python3 jq foot wtype"
leg_mac_faults_claims="hyprland.binds"

mac_faults_prefix="$shot_dir/mac-faults"
mac_faults_log="$shot_dir/mac-faults-stub.log"
mac_faults_other_log="$shot_dir/mac-faults-other.log"
mac_faults_marks="$shot_dir/mac-faults-marks.txt"
mac_faults_again="$shot_dir/mac-faults-again"

leg_mac_faults_timing() {
  leg_timing 130
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
  "$hyprctl_bin" version > /dev/null 2>&1 || break
done
EOF
}

leg_mac_faults_drive() {
  awk '/^-- MAC APPS/ { on = 1 } /^-- Window rounding/ { on = 0 } on { print }' docs/examples/hyprland/formalshell.lua
  cat <<EOF
hl.config({ input = { resolve_binds_by_sym = true }, misc = { enable_anr_dialog = false } })
hl.bind("SUPER + H", hl.dsp.focus({ direction = "l" }))
hl.bind("SUPER + L", hl.dsp.focus({ direction = "r" }))
hl.bind("SUPER + C", hl.dsp.exec_cmd("sh -c 'echo c >> $mac_faults_marks'"))
hl.bind("SUPER + CTRL + P", hl.dsp.exec_cmd("sh -c 'echo p >> $mac_faults_marks'"))
EOF
  local script="$shot_dir/mac-faults-drive.sh"
  local stub="$PWD/dev/mac-stub.py"
  write_script "$script" <<EOF
#!/usr/bin/env bash
m="$mac_faults_prefix"
log="$mac_faults_log"
marks="$mac_faults_marks"
jq="$jq_bin"
: > "\$marks"
submap() { "$hyprctl_bin" submap 2>&1 | head -n 1; }
# in_submap <name> <path>: until Hyprland's submap is that one, 15s at most;
# the one it ended on in <path>.
in_submap() {
  for _ in \$(seq 1 60); do [ "\$(submap)" = "\$1" ] && break; sleep 0.25; done
  submap > "\$2"
}
active() { "$hyprctl_bin" -j activewindow 2>/dev/null | "\$jq" -r '.class // ""'; }
front() { for _ in \$(seq 1 40); do [ "\$(active)" = "\$1" ] && return 0; sleep 0.25; done; return 1; }
pid_of() { "$hyprctl_bin" -j clients 2>/dev/null | "\$jq" -r --arg c "\$1" 'first(.[] | select(.class == \$c) | .pid) // empty'; }
seen() { sleep 0.6; grep -c "\$1" "\$log" > "\$2" 2>/dev/null || true; }
mark() { sleep 0.6; grep -c "^\$1\\\$" "\$marks" > "\$2" 2>/dev/null || true; }
# end_shell: the shell ended as a session's logout ends it, and gone.
end_shell() {
  local pid
  pid=\$(cat "$shot_dir/shell.pid")
  kill -TERM "\$pid" 2>/dev/null
  for _ in \$(seq 1 40); do kill -0 "\$pid" 2>/dev/null || break; sleep 0.25; done
}

"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$python3_bin $stub --name Stub --log \$log]==])" > /dev/null 2>&1
until_window org.atoll.stub 15
spawn_foot formalshell-mac-faults
until_window formalshell-mac-faults
"$hyprctl_bin" dispatch "hl.dsp.focus({ window = 'class:org.atoll.stub' })" > /dev/null 2>&1
front org.atoll.stub
until_jq 15 "\$m-inside.json" '.macKeys and (.titles | length == 4)' call appmenu status
in_submap mac "\$m-submap-start.txt"

"$hyprctl_bin" reload > "\$m-reload.txt" 2>&1
sleep 1
in_submap mac "\$m-submap-reload.txt"
"$hyprctl_bin" -j binds > "\$m-binds-reload.json" 2>&1
status appmenu "\$m-status-reload.json"
"$wtype_bin" -M logo -M ctrl p -m ctrl -m logo
mark p "\$m-mark-p-reload.txt"
"$wtype_bin" -M logo c -m logo
seen '^key c down mods=64\$' "\$m-super-c-reload.txt"
mark c "\$m-mark-c-reload.txt"

end_shell
submap > "\$m-submap-ended.txt"
"$wtype_bin" -M logo l -m logo
front formalshell-mac-faults; echo "\$?" > "\$m-left-stub.txt"
in_submap default "\$m-submap-ended-native.txt"
"$wtype_bin" -M logo c -m logo
mark c "\$m-mark-c-ended.txt"
"$wtype_bin" -M logo h -m logo
front org.atoll.stub
in_submap mac "\$m-submap-ended-again.txt"
"$wtype_bin" -M logo l -m logo
front formalshell-mac-faults
in_submap default "\$m-submap-ended-out.txt"

"$hyprctl_bin" dispatch 'hl.dsp.submap("mac")' > /dev/null 2>&1
in_submap mac "\$m-submap-forced.txt"
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
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$python3_bin $stub --name Other --app-id org.atoll.other --color 8a5a3a --log $mac_faults_other_log]==])" > /dev/null 2>&1
until_window org.atoll.other 15
"$hyprctl_bin" dispatch "hl.dsp.focus({ window = 'class:org.atoll.other' })" > /dev/null 2>&1
front org.atoll.other
until_jq 10 "\$m-other.json" '.macKeys and (.titles | length == 4) and .titles[0].label == "Other"' call appmenu status
shoot "\$m-other.png"
kill -CONT "\$stub_pid"
kill "\$stub_pid"

pkill -f "foot --app-id=formalshell-mac-faults" 2>/dev/null
for _ in \$(seq 1 40); do [ "\$("$hyprctl_bin" -j clients | "\$jq" length)" = 1 ] && break; sleep 0.25; done
front org.atoll.other
until_jq 10 "\$m-alone.json" '.macKeys' call appmenu status
in_submap mac "\$m-submap-alone.txt"
end_shell
kill "\$(pid_of org.atoll.other)"
for _ in \$(seq 1 40); do [ "\$("$hyprctl_bin" -j clients | "\$jq" length)" = 0 ] && break; sleep 0.25; done
"$hyprctl_bin" -j clients > "\$m-clients-none.json" 2>&1
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
  hypr_exec_once "bash $script"
}

leg_mac_faults_assert() {
  local m="$mac_faults_prefix"
  count() { tr -d '[:space:]' < "$1" 2>/dev/null; }
  is() { [ "$(cat "$m-submap-$1.txt" 2>/dev/null)" = "$2" ] || fail "--mac-faults: $3 (Hyprland's submap is '$(cat "$m-submap-$1.txt" 2>/dev/null)')"; }
  [ -s "$mac_faults_log" ] || fail "--mac-faults: the stub logged nothing"
  cat "$mac_faults_log"
  echo "marks: $(tr '\n' ' ' < "$mac_faults_marks" 2>/dev/null)"
  for shot in hung-away other; do
    [ ! -f "$m-$shot.png" ] || echo "SMOKE_MAC_FAULTS_$(echo "$shot" | tr 'a-z-' 'A-Z_') $m-$shot.png"
  done
  expect_jq "$m-inside.json" '.macKeys and (.titles | length == 4)' "--mac-faults: the stub in front did not put the session in the mac submap with its menu held"
  is start mac "the stub in front did not put the session in the mac submap"

  is reload mac "a reload with the stub in front left the session out of the mac submap"
  expect_jq "$m-binds-reload.json" '[.[] | select(.submap == "mac") | .key | ascii_downcase] | index("l") != null and index("h") != null and index("p") != null and index("c") == null' \
    "--mac-faults: the mac submap after a reload does not hold H, L and P without C" '[.[] | {submap, modmask, key}]'
  expect_jq "$m-status-reload.json" '.macBinds == 4' "--mac-faults: appmenu status after a reload does not count the four binds the include recorded" '{macKeys, macBinds}'
  [ "$(count "$m-mark-p-reload.txt")" = 1 ] || fail "--mac-faults: SUPER + CTRL + P did not fire over the stub after a reload (marks $(count "$m-mark-p-reload.txt"))"
  [ "$(count "$m-super-c-reload.txt")" = 1 ] || fail "--mac-faults: SUPER + C did not reach the stub after a reload (got $(count "$m-super-c-reload.txt"))"
  [ "$(count "$m-mark-c-reload.txt")" = 0 ] || fail "--mac-faults: the window manager's SUPER + C fired over the stub after a reload"

  is ended mac "the shell ending over the stub took the session out of the mac submap while the stub was still in front"
  [ "$(count "$m-left-stub.txt")" = 0 ] || fail "--mac-faults: SUPER + L did not move focus off the stub with the shell gone"
  is ended-native default "with the shell gone, focus on the foot window left the session in the mac submap"
  [ "$(count "$m-mark-c-ended.txt")" = 1 ] || fail "--mac-faults: SUPER + C did not fire over the foot window with the shell gone (marks $(count "$m-mark-c-ended.txt"))"
  is ended-again mac "with the shell gone, focus back on the stub did not put the session in the mac submap"
  is ended-out default "with the shell gone, focus on the foot window a second time left the session in the mac submap"

  is forced mac "the session could not be put in the mac submap by hand"
  is back default "the shell started again left the session in the mac submap over the foot window"
  expect_jq "$m-outside.json" '.macKeys | not' "--mac-faults: the shell started again reads the session as in the mac submap over the foot window"
  [ "$(count "$m-mark-c-back.txt")" = 2 ] || fail "--mac-faults: SUPER + C did not fire over the foot window after the shell came back (marks $(count "$m-mark-c-back.txt"))"

  [ "$(count "$m-back-on-stub.txt")" = 0 ] || fail "--mac-faults: SUPER + H did not move focus back onto the stub"
  expect_jq "$m-inside-again.json" '.macKeys and (.titles | length == 4)' "--mac-faults: the shell started again does not hold the stub's menu with the stub in front"
  is again mac "the stub in front of the shell started again did not put the session in the mac submap"
  [ "$(count "$m-super-c-again.txt")" = 2 ] || fail "--mac-faults: SUPER + C did not reach the stub after the shell came back (got $(count "$m-super-c-again.txt"))"
  [ "$(count "$m-mark-c-again.txt")" = 2 ] || fail "--mac-faults: the window manager's SUPER + C fired over the stub after the shell came back"
  [ "$(count "$m-mark-p-again.txt")" = 2 ] || fail "--mac-faults: SUPER + CTRL + P did not fire over the stub after the shell came back (marks $(count "$m-mark-p-again.txt"))"

  expect_jq "$m-hung-away.json" '.titles == []' "--mac-faults: the bar still shows the titles of a stub that stopped answering, over the foot window" '{pid, titles: [.titles[].label]}'
  expect_jq "$m-other.json" '.macKeys and (.titles | length == 4) and .titles[0].label == "Other"' \
    "--mac-faults: a second stub's menu is not held after the first stopped answering" '{pid, titles: [.titles[].label]}'

  expect_jq "$m-alone.json" '.macKeys' "--mac-faults: the second stub alone in front did not put the session in the mac submap"
  is alone mac "the second stub alone in front did not put the session in the mac submap"
  expect_jq "$m-clients-none.json" '. == []' "--mac-faults: a window was left open for the last step" '[.[] | {class, title}]'
  is none default "with the shell gone and the last Mac window closed, the session stayed in the mac submap"
  [ "$(count "$m-mark-c-none.txt")" = 3 ] || fail "--mac-faults: SUPER + C did not fire with the shell gone and no window open (marks $(count "$m-mark-c-none.txt"))"
  expect_jq "$m-last.json" '.macKeys | not' "--mac-faults: the shell did not come back at the end"
  echo "SMOKE_MAC_FAULTS ok reload rebuilt the submap, the default map came back with the shell gone, back and with no window left, a stub that stopped answering held nothing up"
}
