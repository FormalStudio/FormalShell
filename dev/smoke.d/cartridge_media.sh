# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154,SC2086  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail(); $convert_bin expands unquoted
# --cartridge-media rides --cartridge (and --showcase, for the flat teal
# wallpaper) and records the README's cartridge GIFs into docs/media/, the
# way --screensaver-gif does: the boot as the session's first frame, the
# backdrop turning on an empty desktop, the launcher opening and its cursor
# walking the root, the monitor panel, and the lock taking a wrong password
# then the real one. Each is a burst of grim frames named by the ms they
# were taken at; everything after the boot runs at a third of its speed
# (`debug motionScale 300`) and plays back at full speed, so the GIF is
# smooth however few frames a second grim manages. A camera, not a test: it asserts only that
# every GIF was written.
leg_cartridge_media_flag="--cartridge-media"
leg_cartridge_media_order=230
leg_cartridge_media_needs="wtype convert"
leg_cartridge_media_fixture_window=none

cartridge_media_dir="$shot_dir/cartridge-media"
cartridge_media_done="$shot_dir/cartridge-media-done"

leg_cartridge_media_validate() {
  if ! leg_on cartridge; then
    echo "usage: $0 --cartridge --cartridge-media [--showcase]" >&2
    exit 1
  fi
}

leg_cartridge_media_timing() {
  leg_timing 110 170
}

leg_cartridge_media_drive() {
  local script="$shot_dir/cartridge-media-drive.sh"
  mkdir -p "$cartridge_media_dir"
  write_script "$script" <<EOF
#!/usr/bin/env bash
start=\$(date +%s%3N)
ms() { echo \$(( \$(date +%s%3N) - start )); }
# grim into \$1's directory for \$2 ms, each frame named by its stamp.
burst() {
  local dir="$cartridge_media_dir/\$1" until=\$(( \$(ms) + \$2 ))
  mkdir -p "\$dir"
  echo "\${3:-100}" > "\$dir/scale"
  while [ "\$(ms)" -lt "\$until" ]; do
    "$grim_bin" "\$dir/\$(printf '%06d' "\$(ms)").png" >> "$shot_dir/grim.log" 2>&1
  done
}
# The session's motion at \$1 percent of its time, so grim's few frames a
# second become a smooth GIF once the delays are divided back down.
slow() { $ipc call debug motionScale "\$1" > /dev/null 2>&1; }
wait_for() {
  local waited=0
  while [ "\$waited" -lt "\$2" ]; do
    grep -q -- "\$1" "$shell_log_path" 2>/dev/null && return 0
    sleep 0.1
    waited=\$((waited + 100))
  done
  return 1
}
burst boot 9000
wait_for "boot done" 15000
sleep 2
slow 300
burst backdrop 9000 300
$ipc call menu toggle > /dev/null 2>&1 &
burst launcher 15000 300 &
sleep 4.5
for k in Down Down Down Up; do "$wtype_bin" -k \$k; sleep 1.6; done
sleep 1
$ipc call menu toggle > /dev/null 2>&1
wait
sleep 2
$ipc call panel open monitor > /dev/null 2>&1 &
burst panel 8000 300
$ipc call panel close > /dev/null 2>&1
sleep 3
$ipc call lock lock > /dev/null 2>&1 &
burst lock 22000 300 &
sleep 7
"$wtype_bin" "nope"
"$wtype_bin" -k Return
sleep 5
"$wtype_bin" "formalshell-test"
"$wtype_bin" -k Return
wait
slow 100
touch "$cartridge_media_done"
EOF
  hypr_exec_once "bash $script"
}

# One GIF out of a burst: each frame held until the next one's stamp, the
# last for a second and a half, played back at the speed the session's
# motion ran at rather than grim's, cut to \$3 (a geometry, or "" for the
# whole frame) and scaled to \$4 wide. \$5 is the colour distance a pixel
# may drift and still be kept from the last frame: none for a fade, which
# moves every pixel a little each frame and would otherwise leave bands.
_cartridge_media_gif() {
  local name="$1" out="$2" crop="$3" width="$4" fuzz="${5:-2%}"
  local dir="$cartridge_media_dir/$name" frames=() args=() i stamp next delay scale
  scale=$(cat "$dir/scale" 2>/dev/null || echo 100)
  mapfile -t frames < <(find "$dir" -maxdepth 1 -name '*.png' | sort)
  [ "${#frames[@]}" -ge 2 ] || return 1
  for ((i = 0; i < ${#frames[@]}; i++)); do
    stamp=$((10#$(basename "${frames[$i]}" .png)))
    if [ $((i + 1)) -lt ${#frames[@]} ]; then
      next=$((10#$(basename "${frames[$((i + 1))]}" .png)))
      delay=$(((next - stamp) * 10 / scale))
      [ "$delay" -ge 2 ] || delay=2
    else
      delay=150
    fi
    args+=(-delay "$delay" "${frames[$i]}")
  done
  if [ -n "$crop" ]; then
    $convert_bin "${args[@]}" -crop "$crop" +repage -resize "${width}x" -fuzz "$fuzz" -layers Optimize -loop 0 "$out"
  else
    $convert_bin "${args[@]}" -resize "${width}x" -fuzz "$fuzz" -layers Optimize -loop 0 "$out"
  fi
}

leg_cartridge_media_assert() {
  [ -f "$cartridge_media_done" ] || fail "the cartridge media drive never finished"
  local media="$PWD/docs/media" name made=0
  mkdir -p "$media"
  _cartridge_media_gif boot "$media/cartridge-boot.gif" "" 960 0 && made=$((made + 1))
  _cartridge_media_gif backdrop "$media/cartridge-backdrop.gif" "1200x900+720+90" 640 && made=$((made + 1))
  _cartridge_media_gif launcher "$media/cartridge-launcher.gif" "800x640+560+260" 640 && made=$((made + 1))
  _cartridge_media_gif panel "$media/cartridge-panel.gif" "760x600+1160+0" 640 && made=$((made + 1))
  _cartridge_media_gif lock "$media/cartridge-lock.gif" "1200x540+360+200" 720 0 && made=$((made + 1))
  for name in boot backdrop launcher panel lock; do
    echo "SMOKE_CARTRIDGE_MEDIA $name frames=$(find "$cartridge_media_dir/$name" -maxdepth 1 -name '*.png' | wc -l | tr -d ' ') gif=$(stat -c %s "$media/cartridge-$name.gif" 2>/dev/null || echo 0)"
  done
  # The frames themselves, for reading on the mac.
  tar -cf "$shot_dir/cartridge-media.tar" -C "$cartridge_media_dir" . && echo "SMOKE_CARTRIDGE_MEDIA_FRAMES $shot_dir/cartridge-media.tar"
  [ "$made" -eq 5 ] || fail "wrote $made of 5 cartridge GIFs"
}
