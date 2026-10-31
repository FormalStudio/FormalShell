# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --polkit runs a real `pkexec true` against the real setuid wrapper, so the
# shell's registered agent is what the authority hands the conversation to.
# Both frames are the live dialog (prompt, then the error state after a wrong
# password), and the claim no frame can make is the last one: pkexec's own
# exit code, which is 0 only if the real password went through the agent and
# out to PAM.
#
# pkexec runs backgrounded inside the drive script with a trailing `wait`
# rather than as its own autostart entry: the script has to keep typing into
# the dialog while pkexec sits blocked on the conversation, then collect its
# real exit code once that resolves.
#
# The open itself is photographed on the way in, at a tenth speed, before the
# scale goes back and the conversation proper starts.
#
# Before the real password, junk is typed, selected with Ctrl+A
# (polkit-selected.png has the selection over the dots) and deleted with one
# Backspace: pkexec only exits 0 if that left the field empty for the real
# password to go into alone.
leg_polkit_flag="--polkit"
leg_polkit_order=280
leg_polkit_vm_only="it registers the shell as the polkit agent and authenticates a real pkexec"
leg_polkit_needs="pkexec wtype convert"

polkit_active_path="$shot_dir/polkit-active.png"
polkit_error_path="$shot_dir/polkit-error.png"
polkit_selected_path="$shot_dir/polkit-selected.png"
polkit_rc_path="$shot_dir/polkit-rc.txt"
polkit_stderr_path="$shot_dir/polkit-stderr.txt"
polkit_debug_path="$shot_dir/polkit-debug.log"

# No nix fallback: a pkexec built as a plain derivation has no setuid bit
# (the store never preserves one), so it cannot escalate at all. This one has
# to already be the wrapper-owned binary a real polkit host installs.
need_pkexec() {
  if [ -z "${pkexec_bin:-}" ]; then
    if command -v pkexec >/dev/null 2>&1; then
      pkexec_bin=$(command -v pkexec)
    else
      echo "SMOKE_FAIL: pkexec not found on PATH, this leg needs a real security.polkit.enablePkexecWrapper host (the setuid wrapper), not a nix fallback" >&2
      exit 1
    fi
    # pkexec resets PATH to its own compiled-in secure default before exec'ing
    # the target, which has no notion of the nix store, so a bare `true`
    # 127s. `type -P`, not `command -v`: bash's own builtin shadows the real
    # binary and command -v returns the bare word, which 127s just the same.
    polkit_true_bin=$(type -P true)
  fi
}

# The dialog covers the whole output, so under --wallpaper this starts after
# that leg's last frame rather than sampling the desktop through a scrim.
polkit_t0() {
  cover_t0 3
}

# How many frames the open is photographed across, and at what fraction of
# speed. The consent card buds off the top line like every other
# centre-floating card (M57 D5) and is gone into its resting place inside one
# screencopy at full speed, so the open runs at a tenth and is sampled across
# it. Read by eye: `--menu-emerge` is where that travel is measured, and
# repeating its probes here would pin this leg to the launcher's geometry.
polkit_open_frames=10

leg_polkit_fixture() {
  # One flat colour nothing else on the card paints, at the default
  # avatar.path: the identity row shows it when the dialog asks for this
  # session's own account.
  $convert_bin -size 256x256 xc:'#1FB86A' "png:$iso_home/.face"
}

leg_polkit_timing() {
  local t0
  t0=$(polkit_t0)
  # The script's own sleeps sum to t0+29; the `wait` past that blocks for
  # however long the socket-activated polkit-agent-helper conversation takes
  # to resolve, and a first-ever activation in this session is the slow path.
  leg_timing $((t0 + 35)) $((t0 + 66))
}

leg_polkit_drive() {
  local t0 script="$shot_dir/polkit-drive.sh"
  t0=$(polkit_t0)
  write_script "$script" <<EOF
#!/usr/bin/env bash
: > "$polkit_debug_path"
log() { echo "\$(date +%s.%N) \$1" >> "$polkit_debug_path"; }
log start
sleep $t0
$ipc call debug motionScale 1000 >> "$polkit_debug_path" 2>&1
"$pkexec_bin" "$polkit_true_bin" 2> "$polkit_stderr_path" &
polkit_pid=\$!
log "pkexec launched pid=\$polkit_pid"
for i in \$(seq 1 $polkit_open_frames); do
  sleep 0.4
  "$grim_bin" "$shot_dir/polkit-open-\$i.png" > /dev/null 2>&1
done
log open-sampled
$ipc call debug motionScale 100 >> "$polkit_debug_path" 2>&1
sleep 2
"$grim_bin" "$polkit_active_path" > /dev/null 2>&1
log active-screenshot
sleep 1
"$wtype_bin" "wrong-password"
"$wtype_bin" -k Return
log wrong-password-typed
# polkit's PAM round trip is socket-activated (polkit-agent-helper@.service)
# rather than an in-process PamContext like --lock's, so the first
# conversation is genuinely slower to fail than --lock's 5s buffer allows.
sleep 10
"$grim_bin" "$polkit_error_path" > /dev/null 2>&1
log error-screenshot
sleep 1
"$wtype_bin" "junk-to-select"
"$wtype_bin" -M ctrl -k a -m ctrl
sleep 1
"$grim_bin" "$polkit_selected_path" > /dev/null 2>&1
"$wtype_bin" -k BackSpace
log junk-selected-and-deleted
sleep 1
"$wtype_bin" "formalshell-test"
"$wtype_bin" -k Return
log real-password-typed
sleep 3
wait \$polkit_pid
polkit_rc=\$?
log "pkexec exited rc=\$polkit_rc"
echo \$polkit_rc > "$polkit_rc_path"
EOF
  hypr_exec_once "bash $script"
}

leg_polkit_assert() {
  local i path
  for i in $(seq 1 $polkit_open_frames); do
    path="$shot_dir/polkit-open-$i.png"
    [ -f "$path" ] || fail "no polkit-open screenshot produced at $path"
    echo "SMOKE_POLKIT_OPEN_$i $path"
  done
  [ -f "$polkit_active_path" ] || fail "no polkit-active screenshot produced"
  echo "SMOKE_POLKIT_ACTIVE $polkit_active_path"
  [ -f "$polkit_error_path" ] || fail "no polkit-error screenshot produced"
  echo "SMOKE_POLKIT_ERROR $polkit_error_path"
  [ -f "$polkit_selected_path" ] || fail "no polkit-selected screenshot produced"
  echo "SMOKE_POLKIT_SELECTED $polkit_selected_path"
  if [ ! -s "$polkit_rc_path" ] || ! grep -q "^0$" "$polkit_rc_path"; then
    fail "pkexec true did not exit 0 after real authentication. Got: $(cat "$polkit_rc_path" 2>/dev/null); stderr: $(cat "$polkit_stderr_path" 2>/dev/null); debug: $(cat "$polkit_debug_path" 2>/dev/null)"
  fi
  # A prompt frame and an error frame cannot be the same picture: the wrong
  # password has to have visibly landed somewhere.
  if cmp -s "$polkit_active_path" "$polkit_error_path"; then
    fail "polkit-active and polkit-error screenshots are byte-identical: the wrong password never reached the dialog"
  fi
}
