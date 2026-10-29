# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --bar-adaptive: the wingpanel band deciding its own paint off the
# wallpaper under it (M60 T3, the 2026-09-17 spec's Part 2). Four wallpapers
# in one session, each set over IPC, and for each of them both halves of the
# claim: the paint the shell says it settled on (`bar paint`, the three
# numbers behind it included) and the band as drawn, read off a patch of the
# frame in the rows above the cells.
#
# The four, in the order they are driven, with the paint each has to give on
# this rig's dark mode:
#   a flat #e6e6e6 field (mean 230, no spread)      -> dark, no fill at all
#   a flat #1e1e1e field (mean 30, no spread)       -> light, no fill at all
#   half black, half white (spread 127)             -> translucentDark
#   the dark field with the fixture window full     -> maximized, solid
# The fourth band is not in its frame: Hyprland draws a fullscreen window
# above the top layer the bar is on, which is what the chrome auto-hide
# exists for, so that case is asserted off `bar paint` and the whole-state
# dump and its frame is the window.
# The busy case is a coarse split rather than pixel noise on purpose: the
# sampler decodes the wallpaper small, and a smooth downscale averages a
# per-pixel pattern away while a half-and-half field keeps its spread at any
# scale.
#
# It pins `theme.preset` itself, so it needs no --pantheon (and does not
# fight one), and it pins `fullscreen.hideChrome` off: the auto-hide takes
# the bar off the overlay layer while a window covers the output, and the
# solid paint is exactly what a session that turned that off gets to see.
leg_bar_adaptive_flag="--bar-adaptive"
leg_bar_adaptive_order=101
leg_bar_adaptive_needs="convert jq"
# The base run's own focused window is what goes fullscreen for the fourth
# case, the same way --fullscreen takes it over.
leg_bar_adaptive_fixture_window=keep

bar_adaptive_bright_wp="$shot_dir/bar-adaptive-bright.png"
bar_adaptive_dark_wp="$shot_dir/bar-adaptive-dark.png"
bar_adaptive_busy_wp="$shot_dir/bar-adaptive-busy.png"

bar_adaptive_bright_png="$shot_dir/bar-adaptive-bright-band.png"
bar_adaptive_dark_png="$shot_dir/bar-adaptive-dark-band.png"
bar_adaptive_busy_png="$shot_dir/bar-adaptive-busy-band.png"
bar_adaptive_max_png="$shot_dir/bar-adaptive-maximized-band.png"

bar_adaptive_bright_json="$shot_dir/bar-adaptive-bright.json"
bar_adaptive_dark_json="$shot_dir/bar-adaptive-dark.json"
bar_adaptive_busy_json="$shot_dir/bar-adaptive-busy.json"
bar_adaptive_max_json="$shot_dir/bar-adaptive-maximized.json"
bar_adaptive_max_dump="$shot_dir/bar-adaptive-maximized-dump.json"
bar_adaptive_max_clients="$shot_dir/bar-adaptive-maximized-clients.json"

# The band's own rows, above every cell on it: the strip is barCellHeight
# plus a barMargin band either side (40px), the cells sit barMargin in, so
# rows 1 to 4 are band and nothing else at any x. The x is in the white half
# of the busy wallpaper, where black at 0.3 over it lands at 178 and is
# telling apart from every other case.
bar_adaptive_patch="40x4+1200+1"

leg_bar_adaptive_validate() {
  if leg_on bar_style && [ "$(leg_arg bar_style)" != transparent ]; then
    echo "usage: --bar-adaptive reads the wallpaper, which only --bar-style transparent does" >&2
    exit 1
  fi
}

leg_bar_adaptive_fixture() {
  # --pantheon pins the same preset; two "theme" keys in one settings.json
  # would be one object silently winning over the other.
  if ! leg_on pantheon; then
    settings_fragment ', "theme": {"preset": "pantheon"}'
  fi
  # The rule itself is the claim, and pantheon's own table pins the band
  # transparent, so the key puts the rule back. Ridden by --bar-style
  # transparent the style is the rule instead, which has no busy branch.
  if ! leg_on bar_style; then
    settings_fragment ', "bar": {"paint": "auto"}'
  fi
  settings_fragment ', "fullscreen": {"hideChrome": false}'
  $convert_bin -size 1920x1080 xc:'#e6e6e6' "$bar_adaptive_bright_wp"
  $convert_bin -size 1920x1080 xc:'#1e1e1e' "$bar_adaptive_dark_wp"
  $convert_bin -size 1920x1080 xc:black -fill white \
    -draw "rectangle 960,0 1919,1079" "$bar_adaptive_busy_wp"
}

leg_bar_adaptive_timing() {
  leg_timing 40 100
}

leg_bar_adaptive_drive() {
  local script="$shot_dir/bar-adaptive-drive.sh"
  write_script "$script" <<EOS
#!/usr/bin/env bash
# One case per block: set the wallpaper, let the retheme and the sampler
# settle, then read the shell's own answer and photograph the band.
sleep 5
$ipc call wallpaper set "$bar_adaptive_bright_wp" > /dev/null 2>&1
sleep 6
$ipc call bar paint > "$bar_adaptive_bright_json" 2>&1
"$grim_bin" "$bar_adaptive_bright_png" > /dev/null 2>&1

$ipc call wallpaper set "$bar_adaptive_dark_wp" > /dev/null 2>&1
sleep 6
$ipc call bar paint > "$bar_adaptive_dark_json" 2>&1
"$grim_bin" "$bar_adaptive_dark_png" > /dev/null 2>&1

$ipc call wallpaper set "$bar_adaptive_busy_wp" > /dev/null 2>&1
sleep 6
$ipc call bar paint > "$bar_adaptive_busy_json" 2>&1
"$grim_bin" "$bar_adaptive_busy_png" > /dev/null 2>&1

# The fourth case is the second one again with a window over the output, so
# the paint can only be the window's doing.
$ipc call wallpaper set "$bar_adaptive_dark_wp" > /dev/null 2>&1
sleep 5
"$hyprctl_bin" dispatch "hl.dsp.window.fullscreen({ mode = 'fullscreen', action = 'set' })" > /dev/null 2>&1
sleep 6
$ipc call bar paint > "$bar_adaptive_max_json" 2>&1
$ipc call debug dump > "$bar_adaptive_max_dump" 2>&1
"$hyprctl_bin" -j clients > "$bar_adaptive_max_clients" 2>&1
"$grim_bin" "$bar_adaptive_max_png" > /dev/null 2>&1
"$hyprctl_bin" dispatch "hl.dsp.window.fullscreen({ mode = 'fullscreen', action = 'unset' })" > /dev/null 2>&1
EOS
  hypr_exec_once "bash $script"
}

# The paint the first mapped bar reported, and the three numbers with it.
_bar_adaptive_paint() {
  "$jq_bin" -r '.[0].paint.paint // "none"' "$1" 2>/dev/null
}

_bar_adaptive_numbers() {
  "$jq_bin" -r '.[0].paint | "mean=\(.mean | floor) std=\(.std | floor) acutance=\(.acutance | floor) sampled=\(.sampled) fullscreen=\(.fullscreen)"' \
    "$1" 2>/dev/null
}

# The mean luminance of one patch of a frame, 0..255, which is all the fill
# assertions need: every expected band here is flat over the patch.
# Read off one channel rather than through a grayscale conversion: every
# band here is grey, and a colorspace conversion is one linearization away
# from answering in numbers these expectations are not written in.
_bar_adaptive_band() {
  $convert_bin "$1" -crop "$bar_adaptive_patch" +repage \
    -format '%[fx:int(mean.r*255+0.5)]' info: 2>/dev/null
}

# One case: the reported paint, then the band it drew, against a tolerance
# wide enough for the rig's own rounding and narrow enough to tell the fills
# apart (they land 30, 178 and 230 apart). A `want_band` of `unseen` reports
# the patch and asserts nothing about it, for the one case whose band is not
# photographable: Hyprland draws a fullscreen window ABOVE the top layer the
# bar sits on, which is the whole reason the chrome auto-hide exists, so the
# solid paint is the shell's own answer twice over (`bar paint` and the
# dump) and never a frame.
_bar_adaptive_case() {
  local label="$1" json="$2" png="$3" want="$4" want_band="$5"
  local got numbers band
  [ -s "$json" ] || fail "$label: no bar paint reply produced at $json"
  [ -f "$png" ] || fail "$label: no frame produced at $png"
  got=$(_bar_adaptive_paint "$json")
  numbers=$(_bar_adaptive_numbers "$json")
  band=$(_bar_adaptive_band "$png")
  echo "$label: paint=$got $numbers band=$band (want $want, band ~$want_band)"
  if [ "$got" != "$want" ]; then
    fail "$label: the band reports '$got', not '$want' ($numbers)"
  fi
  if [ -z "$band" ]; then
    fail "$label: could not read the band patch $bar_adaptive_patch out of $png"
  fi
  if [ "$want_band" != "unseen" ]; then
    if [ "$band" -lt "$((want_band - 12))" ] || [ "$band" -gt "$((want_band + 12))" ]; then
      fail "$label: the band drew $band over the patch, not ~$want_band, so the $want fill is not what reached the frame"
    fi
  fi
  bar_adaptive_summary="$bar_adaptive_summary $label=$got/$band"
}

leg_bar_adaptive_assert() {
  bar_adaptive_summary=""
  # A flat bright field: nothing busy about it, so the band draws no fill at
  # all and the patch is the wallpaper's own 230.
  _bar_adaptive_case bright "$bar_adaptive_bright_json" "$bar_adaptive_bright_png" dark 230
  # A flat dark one: the same bare band, light ink, the wallpaper's own 30.
  _bar_adaptive_case dark "$bar_adaptive_dark_json" "$bar_adaptive_dark_png" light 30
  # Half black and half white: busy by spread, so black at 0.3 over the
  # white half, which is 178. `bar.style: transparent` takes the busy branch
  # out, so the same field is a bare band whose mean (127) asks light ink,
  # over the white half's own 255.
  if leg_on bar_style; then
    _bar_adaptive_case busy "$bar_adaptive_busy_json" "$bar_adaptive_busy_png" light 255
  else
    _bar_adaptive_case busy "$bar_adaptive_busy_json" "$bar_adaptive_busy_png" translucentDark 178
  fi
  # A window covering the output: solid black, whatever the wallpaper says.
  # The frame shows the window rather than the band (see `_bar_adaptive_case`),
  # so this one is asserted off the shell's own answers alone.
  _bar_adaptive_case maximized "$bar_adaptive_max_json" "$bar_adaptive_max_png" maximized unseen

  # The fullscreen case has to be the window's doing rather than a paint the
  # wallpaper would have given anyway: the same wallpaper read `light` two
  # cases earlier. Both ends of it, the compositor's own client list and the
  # shell's reading of it, so a window that never went fullscreen fails here
  # instead of looking like a paint that did not fire.
  local fs_windows
  [ -s "$bar_adaptive_max_clients" ] || fail "maximized: no client list produced"
  fs_windows=$("$jq_bin" -r '[.[] | select(.fullscreen != 0 and .fullscreen != false)] | length' \
    "$bar_adaptive_max_clients" 2>/dev/null)
  echo "maximized: fullscreen windows per hyprctl: $fs_windows"
  [ "${fs_windows:-0}" -ge 1 ] || fail "maximized: no window was actually fullscreen; the solid paint would be untested"
  "$jq_bin" -e '.[0].paint.fullscreen == true' "$bar_adaptive_max_json" > /dev/null 2>&1 \
    || fail "maximized: the shell does not report a window covering the output, so the solid paint is untested"

  # The same answer in the whole-state dump, which is where a leg that is
  # not this one reads it (`bar[].paint`).
  [ -s "$bar_adaptive_max_dump" ] || fail "maximized: no debug dump produced"
  "$jq_bin" -e '.bar[0].paint.paint == "maximized"' "$bar_adaptive_max_dump" > /dev/null 2>&1 \
    || fail "maximized: debug dump reports bar[0].paint of $("$jq_bin" -c '.bar[0].paint' "$bar_adaptive_max_dump")"

  echo "SMOKE_BAR_ADAPTIVE ok$bar_adaptive_summary"
  echo "SMOKE_BAR_ADAPTIVE_BRIGHT $bar_adaptive_bright_png"
  echo "SMOKE_BAR_ADAPTIVE_DARK $bar_adaptive_dark_png"
  echo "SMOKE_BAR_ADAPTIVE_BUSY $bar_adaptive_busy_png"
  echo "SMOKE_BAR_ADAPTIVE_MAXIMIZED $bar_adaptive_max_png"
}
