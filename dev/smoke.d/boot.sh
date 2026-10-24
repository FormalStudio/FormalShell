# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --boot rides --cartridge, the one table with a boot: the sequence played
# over the desktop as the shell's first frame, photographed as a burst of
# frames (`boot-burst-<ms>.png`, ms since the session's autostart) from
# before the overlay maps until well past the black lifting. Once the shell
# logs `boot done`, a second daemon takes the primary over in the same
# login and must log `boot already played for this login` without playing
# it; then `boot` is cut out of state.json, a third daemon takes over and
# plays it again, and one real key (wtype) 1s into its sequence sends it to
# the way out before the emblem has assembled.
#
# Asserted: the primary played, left on its own and committed through it;
# the burst holds a black stage and moved; the second daemon never played;
# the third left on the key, earlier than it would have left alone.
leg_boot_flag="--boot"
leg_boot_order=220
leg_boot_needs="wtype convert jq"
leg_boot_fixture_window=keep

boot_status_path="$shot_dir/boot-status.json"
boot_second_log_path="$shot_dir/boot-second.log"
boot_third_log_path="$shot_dir/boot-third.log"

leg_boot_validate() {
  if ! leg_on cartridge; then
    echo "usage: --boot plays the cartridge table's boot and needs --cartridge" >&2
    exit 1
  fi
}

leg_boot_timing() {
  leg_timing 40 80
}

leg_boot_drive() {
  local script="$shot_dir/boot-drive.sh"
  local find_daemons launch state
  find_daemons='for pid in $(pgrep -f -- formalshell-rs); do case "$(readlink /proc/$pid/exe 2>/dev/null)" in *formalshell-rs-wrapped|*/bin/formalshell-rs) echo "$pid" ;; esac; done'
  launch="$PWD/result/bin/formalshell-rs"
  state="$iso_home/.local/state/formalshell/state.json"
  write_script "$script" <<EOF
#!/usr/bin/env bash
start=\$(date +%s%3N)
ms() { echo \$(( \$(date +%s%3N) - start )); }
# Waits up to \$3 ms for \$2 in log \$1.
wait_for() {
  local waited=0
  while [ "\$waited" -lt "\$3" ]; do
    grep -q -- "\$2" "\$1" 2>/dev/null && return 0
    sleep 0.1
    waited=\$((waited + 100))
  done
  return 1
}
# The burst, from before the overlay maps to well past the lift.
while [ "\$(ms)" -lt 9000 ]; do
  "$grim_bin" "$shot_dir/boot-burst-\$(printf '%05d' "\$(ms)").png" >> "$shot_dir/grim.log" 2>&1
  sleep 0.08
done
wait_for "$shell_log_path" "boot done" 20000 || true
"$grim_bin" "$shot_dir/boot-rest.png" >> "$shot_dir/grim.log" 2>&1
find_daemon_pids() {
  $find_daemons
}
settle() {
  local waited=0
  while [ "\$waited" -lt 15000 ]; do
    [ "\$(find_daemon_pids | wc -l | tr -d ' ')" = 1 ] && return
    sleep 0.5
    waited=\$((waited + 500))
  done
}
export LIBGL_ALWAYS_SOFTWARE=1
"$launch" > "$boot_second_log_path" 2>&1 &
second=\$!
wait_for "$boot_second_log_path" "boot \(already played\|playing\|off\)" 20000 || true
settle
sleep 1
"$grim_bin" "$shot_dir/boot-second.png" >> "$shot_dir/grim.log" 2>&1
"$jq_bin" 'del(.boot)' "$state" > "$state.boot" && mv "$state.boot" "$state"
"$launch" > "$boot_third_log_path" 2>&1 &
third=\$!
if wait_for "$boot_third_log_path" "boot playing" 20000; then
  sleep 1
  "$grim_bin" "$shot_dir/boot-third-playing.png" >> "$shot_dir/grim.log" 2>&1
  "$wtype_bin" -k space
fi
wait_for "$boot_third_log_path" "boot done" 15000 || true
sleep 1
"$grim_bin" "$shot_dir/boot-third-done.png" >> "$shot_dir/grim.log" 2>&1
printf '{"second":%s,"third":%s,"survivor":%s}\n' "\$second" "\$third" "\$(find_daemon_pids | head -n1)" > "$boot_status_path"
EOF
  hypr_exec_once "bash $script"
}

# One frame's mean luminance, 0..255.
_boot_mean() {
  $convert_bin "$1" -colorspace Gray -format '%[fx:int(mean*255+0.5)]' info: 2>/dev/null
}

leg_boot_assert() {
  local frames darkest dark_frame rest distinct line commits t f m
  [ -s "$boot_status_path" ] || fail "no boot-status.json, the drive script never finished"
  cat "$boot_status_path"
  echo "-- primary --"
  grep "boot " "$shell_log_path" || true
  echo "-- second --"
  grep "boot \|instance lock" "$boot_second_log_path" || true
  echo "-- third --"
  grep "boot \|instance lock" "$boot_third_log_path" || true

  grep -q "boot playing" "$shell_log_path" || fail "the primary never played the boot"
  grep -q "boot leaving auto" "$shell_log_path" || fail "the primary's boot never left on its own"
  line=$(grep -o "boot done after [0-9]* commits" "$shell_log_path" | head -n1)
  commits=${line//[!0-9]/}
  [ -n "$commits" ] || fail "the primary's boot never finished"
  [ "$commits" -ge 20 ] || fail "the boot committed $commits frames, too few to have animated"

  frames=$(find "$shot_dir" -maxdepth 1 -name 'boot-burst-*.png' | wc -l | tr -d ' ')
  [ "$frames" -ge 10 ] || fail "only $frames burst frames"
  darkest=255
  for f in "$shot_dir"/boot-burst-*.png; do
    m=$(_boot_mean "$f")
    [ -n "$m" ] || continue
    if [ "$m" -lt "$darkest" ]; then darkest=$m; dark_frame=$f; fi
  done
  rest=$(_boot_mean "$shot_dir/boot-rest.png")
  distinct=$(md5sum "$shot_dir"/boot-burst-*.png | awk '{print $1}' | sort -u | wc -l | tr -d ' ')
  # Twelve of the burst, evenly spaced, as one sheet to read.
  local picked=() rows=() row i n=0
  mapfile -t picked < <(find "$shot_dir" -maxdepth 1 -name 'boot-burst-*.png' | sort | awk -v n="$frames" 'NR == 1 || (NR - 1) % int((n + 10) / 11) == 0' | head -n 12)
  for ((i = 0; i < ${#picked[@]}; i += 4)); do
    row="$shot_dir/boot-sheet-row$n.png"
    $convert_bin "${picked[@]:i:4}" -resize 480x +append "$row" 2>/dev/null && rows+=("$row")
    n=$((n + 1))
  done
  [ "${#rows[@]}" -gt 0 ] && $convert_bin "${rows[@]}" -background black -append "$shot_dir/boot-burst-sheet.png" 2>/dev/null \
    && echo "SMOKE_BOOT_BURST $shot_dir/boot-burst-sheet.png"
  echo "burst frames=$frames distinct=$distinct darkest=$darkest ($(basename "${dark_frame:-none}")) rest=$rest"
  [ "$darkest" -le 12 ] || fail "no burst frame shows the black stage (darkest mean $darkest)"
  [ "${rest:-0}" -gt $((darkest + 3)) ] || fail "the desktop at rest ($rest) is no lighter than the stage ($darkest)"
  [ "$distinct" -ge 6 ] || fail "only $distinct distinct burst frames, the sequence did not move"

  grep -q "boot already played for this login" "$boot_second_log_path" || fail "the second daemon did not see the boot as played"
  if grep -q "boot playing" "$boot_second_log_path"; then
    fail "the second daemon played the boot again in the same login"
  fi

  grep -q "boot playing" "$boot_third_log_path" || fail "the third daemon did not play the boot with state.json's key cut"
  line=$(grep -o "boot leaving [a-z]* t=[0-9]*ms" "$boot_third_log_path" | head -n1)
  case "$line" in
    "boot leaving key "*) ;;
    *) fail "the third daemon's boot did not leave on the key: '${line:-none}'" ;;
  esac
  t=${line##*t=}
  t=${t%ms}
  [ "$t" -lt 2100 ] || fail "the key landed at ${t}ms, past the emblem's assembly"
  echo "SMOKE_BOOT commits=$commits frames=$frames darkest=$darkest rest=$rest skip=${t}ms"
  echo "SMOKE_BOOT_REST $shot_dir/boot-rest.png"
  echo "SMOKE_BOOT_THIRD $shot_dir/boot-third-playing.png"
}
