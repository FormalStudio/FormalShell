# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --lock's whole sequence lives in one script: everything in it is strictly
# ordered (lock, prove it over IPC, photograph, type a wrong password,
# photograph the error state, type the real one, photograph unlocked, prove
# the flip back over IPC) with nothing needing to interleave.
#
# grim, not any compositor-side capture: the frames have to be taken while
# the session is genuinely locked, and grim talks wlr-screencopy as an
# ordinary client. wtype is a real virtual-keyboard-unstable-v1 client;
# LockIpc deliberately has no "type this password" verb (see its header), so
# the only way to prove the unlock path is to actually type into it.
leg_lock_flag="--lock"
leg_lock_order=110
leg_lock_needs="wtype convert"

lock_locked_path="$shot_dir/lock-locked.png"
lock_typing_path="$shot_dir/lock-typing.png"
lock_error_path="$shot_dir/lock-error.png"
lock_unlocked_path="$shot_dir/lock-unlocked.png"
lock_islocked1_path="$shot_dir/lock-islocked-1.txt"
lock_islocked2_path="$shot_dir/lock-islocked-2.txt"
lock_status_path="$shot_dir/lock-status.json"
lock_call_rc_path="$shot_dir/lock-call-rc.txt"

# This leg's own clock. It starts late under --wallpaper so the two
# wallpaper frames land first, which puts a real matugen-recoloured gradient
# behind the lock plate in the bargain.
lock_t0() {
  if leg_on wallpaper; then echo 16; else echo 5; fi
}

leg_lock_fixture() {
  # A profile picture at the default `avatar.path`, one flat colour nothing
  # else on the lock screen paints, so the locked frame can be searched for it.
  $convert_bin -size 256x256 xc:'#1FB86A' "png:$iso_home/.face"
}

leg_lock_timing() {
  local t0
  t0=$(lock_t0)
  leg_timing $((t0 + 21)) $((t0 + 45))
}

leg_lock_drive() {
  local t0 script="$shot_dir/lock-drive.sh"
  t0=$(lock_t0)
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep $t0
$ipc call lock lock > /dev/null 2>&1
echo \$? > "$lock_call_rc_path"
sleep 1
$ipc call lock isLocked > "$lock_islocked1_path" 2>&1
sleep 3
"$grim_bin" "$lock_locked_path" > /dev/null 2>&1
sleep 2
# Every wtype is a new virtual keyboard, and the seat only gains its
# keyboard once wtype starts, so keys sent before the shell has bound the
# new wl_keyboard reach nobody: a lone \`-k Return\` was lost whole. -s
# holds each one back until the bind has landed.
"$wtype_bin" -s 300 "wrong-password"
sleep 1
"$grim_bin" "$lock_typing_path" > /dev/null 2>&1
"$wtype_bin" -s 300 -k Return
# The PAM round trip for a wrong password forks and execs through the whole
# auth stack, which on this VM is slower than a first glance suggests: a 2s
# buffer intermittently caught the frame before authError had updated.
sleep 5
"$grim_bin" "$lock_error_path" > /dev/null 2>&1
sleep 2
"$wtype_bin" -s 300 "formalshell-test" -k Return
sleep 3
"$grim_bin" "$lock_unlocked_path" > /dev/null 2>&1
sleep 1
$ipc call lock isLocked > "$lock_islocked2_path" 2>&1
$ipc call lock status > "$lock_status_path" 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_lock_assert() {
  local f
  if [ ! -s "$lock_call_rc_path" ] || ! grep -q "^0$" "$lock_call_rc_path"; then
    fail "lock lock IPC call exited non-zero. Got: $(cat "$lock_call_rc_path" 2>/dev/null)"
  fi
  if [ ! -s "$lock_islocked1_path" ] || ! grep -q "^true$" "$lock_islocked1_path"; then
    fail "lock isLocked did not report true right after lock(). Got: $(cat "$lock_islocked1_path" 2>/dev/null)"
  fi
  for f in "$lock_locked_path" "$lock_typing_path" "$lock_error_path" "$lock_unlocked_path"; do
    [ -f "$f" ] || fail "no lock screenshot produced at $f"
  done
  echo "SMOKE_LOCK_LOCKED $lock_locked_path"
  echo "SMOKE_LOCK_TYPING $lock_typing_path"
  echo "SMOKE_LOCK_ERROR $lock_error_path"
  echo "SMOKE_LOCK_UNLOCKED $lock_unlocked_path"
  if [ ! -s "$lock_islocked2_path" ] || ! grep -q "^false$" "$lock_islocked2_path"; then
    fail "lock isLocked did not flip back to false after the real password. Got: $(cat "$lock_islocked2_path" 2>/dev/null)"
  fi
  if [ ! -s "$lock_status_path" ]; then
    fail "no lock status produced"
  fi
  cat "$lock_status_path"; echo
  if ! grep -q '"locked":false' "$lock_status_path"; then
    fail "lock status did not report locked:false after unlock. Got: $(cat "$lock_status_path")"
  fi
  # The swappable-locker half (M45 D2): this run writes no `lock.command`, so
  # the built-in surface is the one that came up and `external` has to say so.
  if ! grep -q '"external":false' "$lock_status_path"; then
    fail "lock status did not report external:false with no lock.command set. Got: $(cat "$lock_status_path")"
  fi
  # The avatar over the clock: ~/.face's colour covering at least a 40px
  # disc's worth of the locked frame (a quarter of the 90px slot).
  local avatar_px
  avatar_px=$($convert_bin "$lock_locked_path" -fuzz 4% -fill black +opaque '#1FB86A' -fill white -opaque '#1FB86A' -format '%[fx:int(mean*w*h)]' info: 2>/dev/null)
  echo "lock avatar pixels: ${avatar_px:-none}"
  if [ "${avatar_px:-0}" -lt 1250 ]; then
    fail "the lock screen drew no avatar from \$HOME/.face (${avatar_px:-0} pixels of its colour)"
  fi
  # A locked frame and an unlocked one cannot be the same picture. The
  # cheapest guard there is against a leg that photographed the desktop four
  # times while every IPC answer stayed perfectly plausible.
  if cmp -s "$lock_locked_path" "$lock_unlocked_path"; then
    fail "lock-locked and lock-unlocked screenshots are byte-identical: the lock surface never mapped"
  fi
}
