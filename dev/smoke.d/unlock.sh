# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --unlock proves the lock hands over to the desktop (wayland/veil.rs's
# hand-over, docs/superpowers/specs/2026-10-30-seams.md stage 10): each
# output's last lock picture committed on a veil while the session is still
# locked, then faded off the desktop on the effects curve, so no frame of
# the unlock is a cut or a flash.
#
# A flat grey wallpaper puts a wide luminance gap between the scrimmed lock
# and the bare desktop. The real password is typed at a tenth of the
# shell's speed (`debug motionScale`) and the unlock photographed back to
# back, each frame's mean grey read against the locked and the unlocked
# ends: no frame outside them (a flash to black, or to anything else), the
# means never turning back, at least one frame part way between (the fade,
# where a cut has none), and the last ones on the desktop.
leg_unlock_flag="--unlock"
leg_unlock_order=111
leg_unlock_needs="wtype convert"

unlock_wallpaper="$shot_dir/unlock-grey.png"
unlock_locked_path="$shot_dir/unlock-locked.png"
unlock_unlocked_path="$shot_dir/unlock-unlocked.png"
unlock_scale_path="$shot_dir/unlock-scale.txt"
unlock_islocked_path="$shot_dir/unlock-islocked.txt"
unlock_stamp_path="$shot_dir/unlock-stamps.txt"
unlock_frames=30
unlock_frame_gap=0.05
unlock_scale=1000

leg_unlock_validate() {
  if leg_on lock; then
    echo "usage: --unlock types the password itself and cannot combine with --lock" >&2
    exit 1
  fi
}

leg_unlock_fixture() {
  $convert_bin -size 1920x1080 xc:'#8c8c8c' "png:$unlock_wallpaper"
}

leg_unlock_timing() {
  leg_timing 40 70
}

leg_unlock_drive() {
  local script="$shot_dir/unlock-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
call() { $ipc call "\$@"; }
sleep 5
call wallpaper set "$unlock_wallpaper" > /dev/null 2>&1
sleep 8
call lock lock > /dev/null 2>&1
sleep 4
"$grim_bin" "$unlock_locked_path" > /dev/null 2>&1
call debug motionScale $unlock_scale > "$unlock_scale_path" 2>&1
# The password first, Return on its own wtype so the frames start with it.
"$wtype_bin" -s 300 "formalshell-test"
sleep 1
"$wtype_bin" -s 300 -k Return
for i in \$(seq 1 $unlock_frames); do
  "$grim_bin" "$shot_dir/unlock-\$i.png" > /dev/null 2>&1
  echo "\$i \$(date +%s%N)" >> "$unlock_stamp_path"
  sleep $unlock_frame_gap
done
sleep 4
"$grim_bin" "$unlock_unlocked_path" > /dev/null 2>&1
call lock isLocked > "$unlock_islocked_path" 2>&1
call debug motionScale 100 > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

# A frame's mean grey, 0 to 1000.
unlock_mean() {
  $convert_bin "$1" -colorspace Gray -format '%[fx:int(mean*1000)]' info: 2>/dev/null
}

leg_unlock_assert() {
  local f i
  grep -q '^ok$' "$unlock_scale_path" 2>/dev/null \
    || fail "debug motionScale did not answer ok, got: $(cat "$unlock_scale_path" 2>/dev/null)"
  for f in "$unlock_locked_path" "$unlock_unlocked_path"; do
    [ -f "$f" ] || fail "no screenshot produced at $f"
  done
  echo "SMOKE_UNLOCK_LOCKED $unlock_locked_path"
  echo "SMOKE_UNLOCK_UNLOCKED $unlock_unlocked_path"
  grep -q '^false$' "$unlock_islocked_path" 2>/dev/null \
    || fail "lock isLocked is not false after the real password, got: $(cat "$unlock_islocked_path" 2>/dev/null)"

  local locked unlocked lo hi tol
  locked=$(unlock_mean "$unlock_locked_path")
  unlocked=$(unlock_mean "$unlock_unlocked_path")
  echo "unlock: locked mean $locked, unlocked mean $unlocked (of 1000)"
  [ -n "$locked" ] && [ -n "$unlocked" ] || fail "could not read the end frames' means"
  if [ $((unlocked - locked)) -lt 100 ] && [ $((locked - unlocked)) -lt 100 ]; then
    fail "the locked and unlocked frames are too close to read a hand-over between ($locked, $unlocked)"
  fi
  lo=$locked hi=$unlocked
  [ "$lo" -gt "$hi" ] && { lo=$unlocked; hi=$locked; }
  # Off the frames' own noise: the clock and the cursor move a few points.
  tol=15

  local means=() m prev="" dir between=0 back=0 outside=0
  for i in $(seq 1 $unlock_frames); do
    f="$shot_dir/unlock-$i.png"
    [ -f "$f" ] || fail "no screenshot produced at $f"
    echo "SMOKE_UNLOCK_$i $f"
    m=$(unlock_mean "$f")
    means+=("$m")
    if [ "$m" -lt $((lo - tol)) ] || [ "$m" -gt $((hi + tol)) ]; then
      outside=$((outside + 1))
    fi
    if [ "$m" -gt $((lo + (hi - lo) / 10)) ] && [ "$m" -lt $((hi - (hi - lo) / 10)) ]; then
      between=$((between + 1))
    fi
    if [ -n "$prev" ]; then
      if [ "$unlocked" -gt "$locked" ] && [ "$m" -lt $((prev - tol)) ]; then back=$((back + 1)); fi
      if [ "$unlocked" -lt "$locked" ] && [ "$m" -gt $((prev + tol)) ]; then back=$((back + 1)); fi
    fi
    prev=$m
  done
  echo "unlock: frame means ${means[*]}"
  [ "$outside" -eq 0 ] || fail "$outside unlock frames fall outside the locked and unlocked means ($lo to $hi): a flash"
  [ "$back" -eq 0 ] || fail "the unlock's means turn back $back times: not one fade"
  [ "$between" -ge 1 ] || fail "no unlock frame lies part way between the locked and unlocked means: a cut, not a fade"
  m=${means[$((unlock_frames - 1))]}
  if [ "$m" -lt $((unlocked - tol)) ] || [ "$m" -gt $((unlocked + tol)) ]; then
    fail "the last unlock frame ($m) is not the desktop ($unlocked)"
  fi
}
