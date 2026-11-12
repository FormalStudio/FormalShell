# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154,SC2016  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --mac-keys: the Super chords handed to a focused Mac window
# (docs/superpowers/specs/2026-11-11-mac-apps.md, section 5). The session
# config takes the shipped include's MAC APPS block, which records every
# bind made after it, and then five binds of its own: SUPER + H and
# SUPER + L moving focus (reserved by default), SUPER + C and SUPER + Q
# (handed to a Mac app by default, here a mark in a file and Hyprland's
# close) and SUPER + CTRL + P (never handed: no Mac key equivalent sits on
# that layer). dev/mac-stub.py is the Mac window, a foot window beside it
# the native one; every key is wtype's, and the stub's --log and the marks
# the binds leave are the evidence. wtype types through a keymap of its
# own, so the session resolves binds by the symbol typed
# (`input.resolve_binds_by_sym`), where a real keyboard's keycodes need no
# such thing.
# With the stub in front `appmenu status` reads `macKeys` and Hyprland's
# own submap reads `mac`: SUPER + C reaches the stub as a key with Super
# held and leaves no mark, CTRL + C reaches it with Control held,
# SUPER + CTRL + P still leaves its mark, and SUPER + L moves focus to the
# foot window without the stub seeing an `l`. There the submap is the
# default one again and SUPER + C leaves its mark. Back on the stub with
# `macApps.reservedBinds` rewritten to keep SUPER + C for the window
# manager, it leaves its mark again and the stub sees no further `c`. Last,
# SUPER + Q reaches the stub as its own key equivalent: it logs the key and
# `quit`, where Hyprland's close would have closed it with no key at all.
leg_mac_keys_flag="--mac-keys"
leg_mac_keys_order=173
leg_mac_keys_needs="python3 jq foot wtype"
leg_mac_keys_claims="hyprland.binds"

mac_keys_prefix="$shot_dir/mac-keys"
mac_keys_log="$shot_dir/mac-keys-stub.log"
mac_keys_marks="$shot_dir/mac-keys-marks.txt"

leg_mac_keys_timing() {
  leg_timing 70
}

leg_mac_keys_drive() {
  awk '/^-- MAC APPS/ { on = 1 } /^-- Window rounding/ { on = 0 } on { print }' docs/examples/hyprland/formalshell.lua
  cat <<EOF
hl.config({ input = { resolve_binds_by_sym = true } })
hl.bind("SUPER + H", hl.dsp.focus({ direction = "l" }))
hl.bind("SUPER + L", hl.dsp.focus({ direction = "r" }))
hl.bind("SUPER + C", hl.dsp.exec_cmd("sh -c 'echo c >> $mac_keys_marks'"))
hl.bind("SUPER + Q", hl.dsp.window.close())
hl.bind("SUPER + CTRL + P", hl.dsp.exec_cmd("sh -c 'echo p >> $mac_keys_marks'"))
EOF
  local script="$shot_dir/mac-keys-drive.sh"
  local stub="$PWD/dev/mac-stub.py"
  write_script "$script" <<EOF
#!/usr/bin/env bash
m="$mac_keys_prefix"
log="$mac_keys_log"
marks="$mac_keys_marks"
jq="$jq_bin"
: > "\$marks"
submap() { "$hyprctl_bin" submap 2>&1 | head -n 1; }
active() { "$hyprctl_bin" -j activewindow 2>/dev/null | "\$jq" -r '.class // ""'; }
# front <class>: until that window is the active one, 10s at most.
front() { for _ in \$(seq 1 40); do [ "\$(active)" = "\$1" ] && return 0; sleep 0.25; done; return 1; }
# seen <pattern> <path>: how many of the stub's lines match, once they have
# had a moment to land.
seen() { sleep 0.6; grep -c "\$1" "\$log" > "\$2" 2>/dev/null || true; }
mark() { sleep 0.6; grep -c "^\$1\\\$" "\$marks" > "\$2" 2>/dev/null || true; }

"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$python3_bin $stub --name Stub --log \$log]==])" > /dev/null 2>&1
until_window org.atoll.stub 15
spawn_foot formalshell-mac-keys
until_window formalshell-mac-keys
"$hyprctl_bin" dispatch "hl.dsp.focus({ window = 'class:org.atoll.stub' })" > /dev/null 2>&1
front org.atoll.stub
until_jq 15 "\$m-inside.json" '.macKeys and (.titles | length == 4)' call appmenu status
submap > "\$m-submap-mac.txt"
"$hyprctl_bin" -j clients > "\$m-clients.json" 2>&1
"$hyprctl_bin" -j binds > "\$m-binds.json" 2>&1

"$wtype_bin" -M logo c -m logo
seen '^key c down mods=64\$' "\$m-super-c.txt"
mark c "\$m-mark-c-mac.txt"
"$wtype_bin" -M ctrl c -m ctrl
seen '^key c down mods=4\$' "\$m-ctrl-c.txt"
"$wtype_bin" -M logo -M ctrl p -m ctrl -m logo
mark p "\$m-mark-p-mac.txt"

"$wtype_bin" -M logo l -m logo
front formalshell-mac-keys; echo "\$?" > "\$m-left-stub.txt"
until_jq 10 "\$m-outside.json" '.macKeys | not' call appmenu status
submap > "\$m-submap-default.txt"
seen '^key l down' "\$m-super-l.txt"
"$wtype_bin" -M logo c -m logo
mark c "\$m-mark-c-native.txt"

"$wtype_bin" -M logo h -m logo
front org.atoll.stub; echo "\$?" > "\$m-back-on-stub.txt"
until_jq 10 "\$m-inside-again.json" '.macKeys' call appmenu status
settings_retarget '.macApps.reservedBinds = ["SUPER + H", "SUPER + L", "SUPER + C"]'
# The submap is built again off the new list on the settings' own clock,
# which nothing here can read: Hyprland holds it, not the shell's status.
sleep 2
"$wtype_bin" -M logo c -m logo
mark c "\$m-mark-c-reserved.txt"
seen '^key c down mods=64\$' "\$m-super-c-reserved.txt"

"$wtype_bin" -M logo q -m logo
for _ in \$(seq 1 20); do grep -qx quit "\$log" 2>/dev/null && break; sleep 0.25; done
seen '^key q down mods=64\$' "\$m-super-q.txt"
EOF
  write_script "$shot_dir/mac-keys-kill.sh" <<EOF
#!/usr/bin/env bash
pkill -f "$stub --name Stub --log $mac_keys_log" 2>/dev/null || true
EOF
  add_cleanup "bash $shot_dir/mac-keys-kill.sh"
  hypr_exec_once "bash $script"
}

leg_mac_keys_assert() {
  local m="$mac_keys_prefix"
  count() { tr -d '[:space:]' < "$1" 2>/dev/null; }
  [ -s "$mac_keys_log" ] || fail "--mac-keys: the stub logged nothing"
  cat "$mac_keys_log"
  echo "marks: $(tr '\n' ' ' < "$mac_keys_marks" 2>/dev/null)"
  expect_jq "$m-clients.json" 'any(.[]; .class == "org.atoll.stub" and .xdgTag == "atoll")' \
    "--mac-keys: Hyprland does not list the stub's window with the atoll tag" '[.[] | {class, xdgTag}]'
  expect_jq "$m-inside.json" '.macKeys' "--mac-keys: the stub in front did not put the session in the mac submap"
  expect_jq "$m-binds.json" '[.[] | select(.submap == "mac") | .key | ascii_downcase] | index("l") != null and index("h") != null and index("p") != null and index("c") == null and index("q") == null' \
    "--mac-keys: the mac submap does not hold H, L and P without C and Q" '[.[] | {submap, modmask, key}]'
  [ "$(cat "$m-submap-mac.txt" 2>/dev/null)" = "mac" ] || fail "--mac-keys: Hyprland's submap with the stub in front is '$(cat "$m-submap-mac.txt" 2>/dev/null)', not mac"
  [ "$(count "$m-super-c.txt")" = 1 ] || fail "--mac-keys: SUPER + C did not reach the stub once with Super held (got $(count "$m-super-c.txt"))"
  [ "$(count "$m-mark-c-mac.txt")" = 0 ] || fail "--mac-keys: the window manager's SUPER + C still fired over a Mac window"
  [ "$(count "$m-ctrl-c.txt")" = 1 ] || fail "--mac-keys: CTRL + C did not reach the stub once with Control held (got $(count "$m-ctrl-c.txt"))"
  [ "$(count "$m-mark-p-mac.txt")" = 1 ] || fail "--mac-keys: SUPER + CTRL + P, never handed, did not fire in the mac submap"
  [ "$(count "$m-left-stub.txt")" = 0 ] || fail "--mac-keys: SUPER + L did not move focus off the stub"
  [ "$(count "$m-super-l.txt")" = 0 ] || fail "--mac-keys: the reserved SUPER + L reached the stub as a key"
  expect_jq "$m-outside.json" '.macKeys | not' "--mac-keys: the foot window in front left the session in the mac submap"
  [ "$(cat "$m-submap-default.txt" 2>/dev/null)" = "default" ] || fail "--mac-keys: Hyprland's submap with the foot window in front is '$(cat "$m-submap-default.txt")'"
  [ "$(count "$m-mark-c-native.txt")" = 1 ] || fail "--mac-keys: SUPER + C did not fire over the foot window"
  [ "$(count "$m-back-on-stub.txt")" = 0 ] || fail "--mac-keys: SUPER + H did not move focus back onto the stub"
  [ "$(count "$m-mark-c-reserved.txt")" = 2 ] || fail "--mac-keys: SUPER + C reserved in macApps.reservedBinds did not fire over the stub (marks $(count "$m-mark-c-reserved.txt"))"
  [ "$(count "$m-super-c-reserved.txt")" = 1 ] || fail "--mac-keys: the reserved SUPER + C still reached the stub"
  [ "$(count "$m-super-q.txt")" = 1 ] || fail "--mac-keys: SUPER + Q did not reach the stub as a key"
  grep -qx quit "$mac_keys_log" || fail "--mac-keys: the stub did not quit on SUPER + Q"
  echo "SMOKE_MAC_KEYS ok handed SUPER + C and SUPER + Q, kept SUPER + L and SUPER + CTRL + P, CTRL + C untouched, reservedBinds followed"
}
