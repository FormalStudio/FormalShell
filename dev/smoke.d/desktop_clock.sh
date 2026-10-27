# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, iso_home, the *_bin paths and fail()
# --desktop-clock: desktopClock.enabled over a dark field with an orange band
# down the screen through the clock's box (x 940 to 979 at 1920x1080, the
# box centred on x 960), and a depth map white over that band alone, the
# --depth leg's fixture moved under the desktop clock. Every frame comes
# with the `debug dump`'s `desktopClock` block beside it:
# - the band over the digits with the defaults;
# - a virtual pointer (dev/vpointer.py) parked near the bottom-left corner:
#   `offset` leaning left and down, by more than 0 and no more than the
#   default `desktopClock.parallax` of 12, then no commit in 3s at rest;
# - a foot window made fullscreen and no commit in 5s under it;
# - the digits over the band once settings.json is retargeted to
#   `desktopClock.depth: false`, then the gooey and serif faces;
# - the first redraw past the next minute: its `minuteUs` and the commit's
#   own `render_us` under 50ms together (read under FS_CPU_QUOTA for
#   e1504g's power saver).
leg_desktop_clock_flag="--desktop-clock"
leg_desktop_clock_order=112
leg_desktop_clock_needs="convert jq python3 foot"

leg_desktop_clock_validate() {
  if leg_on showcase || leg_on wallpaper || leg_on depth || leg_on config_reload; then
    echo "usage: $0 --desktop-clock seeds its own wallpaper and retargets settings.json; run it without --showcase, --wallpaper, --depth or --config-reload" >&2
    exit 1
  fi
}

desktop_clock_dir="$shot_dir/desktop-clock-wallpapers"
desktop_clock_picture="$desktop_clock_dir/band.png"
desktop_clock_results="$shot_dir/desktop-clock-results.txt"
desktop_clock_vpointer="$PWD/dev/vpointer.py"

leg_desktop_clock_fixture() {
  settings_fragment ', "desktopClock": {"enabled": true}'
  mkdir -p "$desktop_clock_dir/.depth"
  $convert_bin -size 1920x1080 xc:'#101418' -fill '#ff7a1a' -draw 'rectangle 940,0 979,1079' "$desktop_clock_picture"
  $convert_bin -size 1920x1080 xc:black -fill white -draw 'rectangle 940,0 979,1079' "$desktop_clock_dir/.depth/band.png.png"
  touch -r "$desktop_clock_picture" "$desktop_clock_dir/.depth/band.png.png"
  mkdir -p "$iso_home/.local/state/formalshell"
  printf '{"wallpaper": "%s"}\n' "$desktop_clock_picture" > "$iso_home/.local/state/formalshell/state.json"
}

leg_desktop_clock_timing() {
  leg_timing 130 170
}

leg_desktop_clock_drive() {
  local script="$shot_dir/desktop-clock-drive.sh" settings_path="$iso_home/.config/formalshell/settings.json"
  write_script "$script" <<EOF
#!/usr/bin/env bash
out="$desktop_clock_results"
: > "\$out"
# A phase's dump and frame.
snap() {
  $ipc call debug dump 2>/dev/null | "$jq_bin" -c '.desktopClock' > "$shot_dir/desktop-clock-\$1.json"
  "$grim_bin" "$shot_dir/desktop-clock-\$1.png" > /dev/null 2>&1
}
commits() { $ipc call debug dump 2>/dev/null | "$jq_bin" '[.desktopClock[].commits] | add'; }
# The commits made over \$2 seconds, as "<phase> commits=<n>", started
# clear of a minute boundary so the minute's own redraw is not counted.
quiet() {
  local c0 c1 s=\$((10#\$(date +%S)))
  [ "\$s" -lt 50 ] || sleep \$((61 - s))
  c0=\$(commits); sleep "\$2"; c1=\$(commits); echo "\$1 commits=\$((c1 - c0))" >> "\$out"
}
# settings.json retargeted to the first generation with \$2 applied.
retarget() {
  "$jq_bin" "\$2" "$shot_dir/desktop-clock-settings-gen1.json" > "$shot_dir/desktop-clock-settings-\$1.json"
  ln -sfn "$shot_dir/desktop-clock-settings-\$1.json" "$settings_path"
  sleep 2
}
sleep 5
snap on
cp "\$(readlink -f "$settings_path")" "$shot_dir/desktop-clock-settings-gen1.json"

# One virtual pointer held through the rest window. wlrctl's goes away as
# it exits, before the shell has bound the wl_pointer its seat just
# gained, so no enter ever reaches the layer; and the leave a vanishing
# pointer sends would land in the rest window.
"$python3_bin" "$desktop_clock_vpointer" wait 500 move -4000 -4000 wait 100 move 20 1060 wait 20000 > /dev/null 2>&1 &
vp=\$!
sleep 3
snap pointer
quiet rest 3
kill \$vp 2>/dev/null

"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-desktop-clock --title='formalshell desktop clock' sh -c 'sleep 300']==])" > /dev/null 2>&1
sleep 3
"$hyprctl_bin" dispatch "hl.dsp.window.fullscreen({ mode = 'fullscreen', action = 'set', window = 'class:formalshell-desktop-clock' })" > /dev/null 2>&1
sleep 2
quiet fullscreen 5
"$hyprctl_bin" -j clients > "$shot_dir/desktop-clock-fullscreen-clients.json" 2>&1
pkill -f -- '--app-id=formalshell-desktop-clock'
sleep 2

retarget off '.desktopClock.depth = false'
snap off
retarget gooey '.desktopClock.face = "gooey"'
snap gooey
retarget serif '.desktopClock.face = "serif"'
snap serif
retarget pixel '.'

# Past the next minute, so the redraw read is the minute's own.
sleep \$((62 - 10#\$(date +%S)))
snap minute
echo "render_us=\$(grep '^commit surface=desktop-clock ' "$shell_log_path" | tail -1 | grep -oE 'render_us=[0-9]+' | cut -d= -f2)" >> "\$out"
echo done >> "\$out"
EOF
  hypr_exec_once "bash $script"
}

_desktop_clock() { "$jq_bin" -r "$2" "$shot_dir/desktop-clock-$1.json"; }
_desktop_clock_quiet() { awk -v p="$1" '$1 == p { sub("commits=", "", $2); print $2 }' "$desktop_clock_results"; }

leg_desktop_clock_assert() {
  local phase
  grep -qx done "$desktop_clock_results" 2>/dev/null || fail "--desktop-clock: the drive never finished: $(cat "$desktop_clock_results" 2>/dev/null)"
  for phase in on pointer off gooey serif minute; do
    [ -s "$shot_dir/desktop-clock-$phase.png" ] && [ -s "$shot_dir/desktop-clock-$phase.json" ] || fail "--desktop-clock produced no $phase frame or dump"
    echo "SMOKE_DESKTOP_CLOCK_$(echo "$phase" | tr '[:lower:]' '[:upper:]') $shot_dir/desktop-clock-$phase.png"
    echo "$phase $(cat "$shot_dir/desktop-clock-$phase.json")"
  done

  _desktop_clock on 'length > 0 and all(.enabled and .depth and .input)' | grep -qx true \
    || fail "--desktop-clock: no clock with depth and input on: $(cat "$shot_dir/desktop-clock-on.json")"

  # The pointer at (20, 1060) of 1920x1080 leans x by -0.98 and y by +0.96.
  _desktop_clock pointer '.[0].offset | .[0] < 0 and .[0] >= -12 and .[1] > 0 and .[1] <= 12' | grep -qx true \
    || fail "--desktop-clock: offset $(_desktop_clock pointer '.[0].offset') does not lean toward the bottom-left corner within 12px"
  [ "$(_desktop_clock_quiet rest)" = 0 ] || fail "--desktop-clock: $(_desktop_clock_quiet rest) commits in 3s at rest"

  local fs_windows
  fs_windows=$("$jq_bin" -r '[.[] | select(.fullscreen != 0 and .fullscreen != false)] | length' "$shot_dir/desktop-clock-fullscreen-clients.json" 2>/dev/null)
  [ "${fs_windows:-0}" -ge 1 ] || fail "--desktop-clock: no window was fullscreen, so the hold went untested"
  [ "$(_desktop_clock_quiet fullscreen)" = 0 ] || fail "--desktop-clock: $(_desktop_clock_quiet fullscreen) commits under a fullscreen window"

  _desktop_clock off 'all(.enabled and (.depth | not))' | grep -qx true \
    || fail "--desktop-clock: still reported depth after desktopClock.depth was set false"
  [ "$(_desktop_clock gooey '.[0].face')" = gooey ] || fail "--desktop-clock: the gooey retarget reported $(_desktop_clock gooey '.[0].face')"
  [ "$(_desktop_clock serif '.[0].face')" = serif ] || fail "--desktop-clock: the serif retarget reported $(_desktop_clock serif '.[0].face')"

  local draw_us render_us total
  draw_us=$(_desktop_clock minute '.[0].minuteUs')
  render_us=$(awk -F= '$1 == "render_us" { print $2 }' "$desktop_clock_results")
  total=$(( ${draw_us:-0} + ${render_us:-0} ))
  echo "SMOKE_DESKTOP_CLOCK_MINUTE draw_us=$draw_us render_us=$render_us quota=${FS_CPU_QUOTA:-none}"
  [ -n "$render_us" ] || fail "--desktop-clock: no desktop-clock commit in the shell log"
  [ "$total" -lt 50000 ] || fail "--desktop-clock: the minute's redraw took ${total}us, past 50ms"
}
