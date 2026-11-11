# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --bar-style <transparent|solid|corners> pins `bar.style` in the settings
# fixture (a `settings_pin`, so it lands inside whatever `bar` object the
# ridden legs write) and rides any other leg, the way --bar-position pins
# the edge: `--bar-style solid --join` is the join off a solid band. Under
# every table a written style puts the bar on wingpanel's band.
#
# Its own claims, read once the shell has taken the key (`debug dump`'s
# `theme.barStyle`): `bar paint` reports the style, and solid and corners
# report the `maximized` paint, which they hold whatever the wallpaper says.
# The band's own outer rows, on whichever edge `debug dump` puts it and under
# a frame ring too, are then opaque black.
#
# Alone it also sets a flat grey wallpaper first, since the rig's bare
# desktop is near black and a black band on it says nothing, and reads what
# corners adds: the bar's window reaching a fillet's radius past the band
# while the output's reserved top stays the band's own thickness (no new
# layer, the exclusive zone unchanged), each fillet's corner pixel under
# both ends of the band black, and the pixel at its arc's centre whatever
# is under the window (the fixture window's grey, or the wallpaper's). Ridden, the wallpaper is the ridden leg's business and
# the frame is taken as early as the key lands, before most legs drive.
# With --frame the ring's own fillets are the corners (frame.sh reads them).
leg_bar_style_flag="--bar-style <style>"
leg_bar_style_order=7
leg_bar_style_needs="convert jq"
leg_bar_style_fixture_window=keep

bar_style_wp="$shot_dir/bar-style-grey.png"
bar_style_dump_path="$shot_dir/bar-style-dump.json"
bar_style_paint_path="$shot_dir/bar-style-paint.json"
bar_style_layers_path="$shot_dir/bar-style-layers.json"
bar_style_monitors_path="$shot_dir/bar-style-monitors.json"
bar_style_frame_path="$shot_dir/bar-style.png"

bar_style_alone() {
  [ "${#active_legs[@]}" -eq 1 ]
}

leg_bar_style_validate() {
  case "$(leg_arg bar_style)" in
    transparent|solid|corners) ;;
    *)
      echo "usage: $0 --bar-style <transparent|solid|corners>" >&2
      exit 1
      ;;
  esac
  leg_bar_style_claims="bar.style=$(leg_arg bar_style)"
}

leg_bar_style_fixture() {
  settings_pin ".bar.style = \"$(leg_arg bar_style)\""
  if bar_style_alone; then
    flat_wallpaper "$bar_style_wp" '#808080'
  fi
}

leg_bar_style_timing() {
  if bar_style_alone; then leg_timing 40; fi
}

leg_bar_style_drive() {
  local script="$shot_dir/bar-style-drive.sh" wallpaper=""
  if bar_style_alone; then
    wallpaper="$ipc call wallpaper set \"$bar_style_wp\" > /dev/null 2>&1
until_wallpaper \"$bar_style_wp\"
until_settled"
  fi
  write_script "$script" <<EOS
#!/usr/bin/env bash
until_jq 20 "$bar_style_dump_path" '.theme.barStyle == "$(leg_arg bar_style)"' call debug dump
sleep 1
$wallpaper
$ipc call debug dump > "$bar_style_dump_path" 2>&1
$ipc call bar paint > "$bar_style_paint_path" 2>&1
"$wmctl_bin" -j layers > "$bar_style_layers_path" 2>&1
"$wmctl_bin" -j monitors > "$bar_style_monitors_path" 2>&1
"$grim_bin" "$bar_style_frame_path" > /dev/null 2>&1
EOS
  wm_exec_once "bash $script"
}

# One pixel's red channel, 0..255: every surface read here is grey.
bar_style_pixel() {
  pixel "$bar_style_frame_path" "$1" "$2" | cut -d' ' -f1
}

leg_bar_style_assert() {
  local style paint t box bh mw reserved r px
  style=$(leg_arg bar_style)
  [ -s "$bar_style_dump_path" ] || fail "no debug dump produced for the bar style check"
  [ -f "$bar_style_frame_path" ] || fail "no frame produced at $bar_style_frame_path"
  echo "SMOKE_BAR_STYLE $bar_style_frame_path"
  "$jq_bin" -e --arg s "$style" '.theme.barStyle == $s' "$bar_style_dump_path" > /dev/null 2>&1 \
    || fail "debug dump reports barStyle $("$jq_bin" -c '.theme.barStyle' "$bar_style_dump_path"), not $style"
  paint=$("$jq_bin" -c '.[0].paint' "$bar_style_paint_path" 2>/dev/null)
  echo "bar paint: $paint"
  "$jq_bin" -e --arg s "$style" '.[0].paint.style == $s' "$bar_style_paint_path" > /dev/null 2>&1 \
    || fail "bar paint does not report the style $style: $paint"
  if [ "$style" = transparent ]; then
    "$jq_bin" -e '.[0].paint.paint | IN("light", "dark")' "$bar_style_paint_path" > /dev/null 2>&1 \
      || fail "a transparent band should read the wallpaper without the busy branch: $paint"
    return 0
  fi
  "$jq_bin" -e '.[0].paint.paint == "maximized"' "$bar_style_paint_path" > /dev/null 2>&1 \
    || fail "a $style band should wear the maximized paint: $paint"

  local edge band
  edge=$("$jq_bin" -r '.theme.barPosition' "$bar_style_dump_path")
  t=$("$jq_bin" -r --arg e "$edge" '.theme.edgeInset[$e]' "$bar_style_dump_path")
  # The band's own outer rows, outside every cell (cells sit barMargin in),
  # halfway along its edge.
  case $edge in
    top) band="40x4+$((out_w / 2 - 60))+1" ;;
    bottom) band="40x4+$((out_w / 2 - 60))+$((out_h - 5))" ;;
    left) band="4x40+1+$((out_h / 2 - 20))" ;;
    right) band="4x40+$((out_w - 5))+$((out_h / 2 - 20))" ;;
    *) fail "debug dump puts the bar on no edge: $edge" ;;
  esac
  px=$(patch_mean "$bar_style_frame_path" "$band")
  echo "band's outer rows on the $edge edge at $band: $px thousandths (thickness $t)"
  [ -n "$px" ] && [ "$px" -le 47 ] || fail "a $style band should be opaque black over its own outer rows on the $edge edge, read ${px:-nothing} at $band"

  [ "$style" = corners ] && bar_style_alone || return 0
  box=$("$jq_bin" -r "$layers_jq"'layer("formalshell:bar") | "\(length) \(first.h)"' "$bar_style_layers_path")
  read -r _ bh <<< "$box"
  [ "${box%% *}" = 1 ] || fail "corners should draw on the bar's own window, got $box formalshell:bar layers"
  reserved=$("$jq_bin" -r 'first | .reserved[1]' "$bar_style_monitors_path")
  mw=$("$jq_bin" -r 'first | .width / .scale | floor' "$bar_style_monitors_path")
  r=$((bh - t))
  echo "corners: window $bh tall over a $t band, fillet $r, reserved top $reserved"
  [ "$r" -gt 0 ] || fail "the bar's window ($bh) reaches no further than the band ($t), so it has no room for a fillet"
  [ "$reserved" = "$t" ] || fail "corners should reserve the band's own $t, the output reserves $reserved"
  local at left right centre
  for at in "1 $((t + 1))" "$((mw - 2)) $((t + 1))"; do
    read -r left right <<< "$at"
    px=$(bar_style_pixel "$left" "$right")
    echo "fillet corner ($left,$right): $px"
    [ -n "$px" ] && [ "$px" -le 12 ] || fail "no fillet at ($left,$right): read ${px:-nothing}, not the band's black"
  done
  centre=$(bar_style_pixel "$r" "$((t + r))")
  echo "fillet arc centre ($r,$((t + r))): $centre"
  [ -n "$centre" ] && [ "$centre" -ge 24 ] || fail "the fillet covers its own arc's centre ($r,$((t + r))): read ${centre:-nothing}, not what is under the window"
}
