# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --wallpaper: set the solid fixture, dump `theme status` (the matugen
# recolour), photograph it while it is still the only thing on screen, then
# crossfade to the gradient and photograph that. Two frames, because the two
# dither assertions read opposite things and neither can be made off the
# other's source.
#
# The retro dither is opt-in since M45, so the default run writes no
# `wallpaper.dither` key at all and this leg asserts the plain image reached
# the screen. SMOKE_WALLPAPER_DITHER=1 turns the opt-in on for one run and
# flips the assertion with it, so both sides of the setting are provable
# without a second flag.
#
# The same retheme also publishes the compositor's own palette, so the leg
# reads the hypr file back and then asks the running Hyprland what its
# active border actually is: the files being right proves the write, and only
# the running value proves the reload.
leg_wallpaper_flag="--wallpaper"
leg_wallpaper_order=100
leg_wallpaper_needs="convert"

theme_status_path="$shot_dir/theme-status.json"
wallpaper_get_path="$shot_dir/wallpaper-get.txt"
wallpaper_solid_path="$shot_dir/wallpaper-solid.png"
wallpaper_gradient_path="$shot_dir/wallpaper-gradient.png"
wp_path="$shot_dir/wp.png"
wp2_path="$shot_dir/wp2.png"
wallpaper_dither=false

# The palette's second half: theme.json themes the shell, this file themes the
# compositor. hyprland_border_path is what proves the reload, see the fixture.
hypr_dir="$iso_home/.config/hypr"
hypr_lua_path="$hypr_dir/formalshell-colors.lua"
hypr_border_path="$shot_dir/hyprland-border.txt"
theme_json_path="$iso_home/.local/state/formalshell/theme.json"
# Neither palette can ever produce it, so it can only be the seeded file.
hypr_sentinel="ff00ff"

leg_wallpaper_fixture() {
  if [ "${SMOKE_WALLPAPER_DITHER:-0}" = "1" ]; then
    wallpaper_dither=true
    settings_fragment ', "wallpaper": {"dither": true}'
  elif leg_on retro; then
    # The retro preset turns `theme.dither` on and `wallpaper.dither`
    # defaults to it, so the quantized strip is the expected frame here.
    wallpaper_dither=true
  fi
  # Seeded before Hyprland starts, because the config below dofiles it and
  # the shell does not write its own copy until a second or two into the
  # session. Every value is the sentinel, so anything else read back off
  # `hyprctl getoption` later came from a rewrite the shell published.
  #
  # The rig's config carries `disable_autoreload = true`, which is what makes
  # that read an assertion rather than a coincidence: with the watcher off,
  # the only thing that can put a new colour into the running compositor is
  # the explicit `hyprctl reload` ThemeEngine issues after each publish.
  mkdir -p "$hypr_dir"
  cat > "$hypr_lua_path" <<EOF
return {
  primary = "rgb($hypr_sentinel)",
  primaryForeground = "rgb($hypr_sentinel)",
  background = "rgb($hypr_sentinel)",
  foreground = "rgb($hypr_sentinel)",
  border = "rgb($hypr_sentinel)",
  destructive = "rgb($hypr_sentinel)",
  warning = "rgb($hypr_sentinel)",
}
EOF
  # 1920x1080, not something small: a source below the screen's own size
  # never exercises Background/LockSurface's sourceSize cap at all (Qt only
  # scales a decode down), so a fixture has to at least meet the screen to
  # prove the cap engages.
  #
  # The first is monotone and the second a left-to-right gradient, and the
  # pair is what makes the dither assertions readable in both directions: a
  # solid source has one color whether or not a pass ran, so it can only
  # ever prove flatness, while a gradient carries far more colors than any
  # derived palette is allowed and so is the only one that can tell a plain
  # image from a quantized one. The gradient is generated portrait and
  # rotated so the ramp runs along the axis a full-width sample crosses.
  $convert_bin -size 1920x1080 xc:'#7a3fb0' "$wp_path"
  $convert_bin -size 1080x1920 gradient:'#3fb07a-#0b2d20' -rotate 90 "$wp2_path"
}

leg_wallpaper_timing() {
  leg_timing 18 55
}

leg_wallpaper_drive() {
  local script="$shot_dir/wallpaper-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 3
$ipc call wallpaper set "$wp_path" > /dev/null 2>&1
sleep 3
$ipc call theme status > "$theme_status_path" 2>&1
sleep 2
"$grim_bin" "$wallpaper_solid_path" > /dev/null 2>&1
sleep 1
$ipc call wallpaper set "$wp2_path" > /dev/null 2>&1
sleep 4
$ipc call wallpaper get > "$wallpaper_get_path" 2>&1
sleep 1
"$grim_bin" "$wallpaper_gradient_path" > /dev/null 2>&1
sleep 2
"$hyprctl_bin" getoption general:col.active_border > "$hypr_border_path" 2>&1
EOF
  hypr_exec_once "bash $script"
  # The compositor half of the palette, wired the way docs/USAGE.md tells a
  # user to wire it. Appended after the base config's own `general` block, so
  # this is the value that survives.
  echo "local colors = dofile([==[$hypr_lua_path]==])"
  echo "hl.config({ general = { col = { active_border = colors.primary } } })"
}

# Reads whole pixels out of a cropped patch. The trailing [,)] tolerates an
# alpha component, which grim's PNG may or may not carry; a pattern anchored
# on ")" would silently parse nothing.
wallpaper_read_patch() {
  $convert_bin "$1" -crop "$2" +repage txt:- 2>/dev/null \
    | sed -n 's/^[0-9]*,[0-9]*: (\([0-9]*\),\([0-9]*\),\([0-9]*\)[,)].*/\1 \2 \3/p'
}

leg_wallpaper_assert() {
  local f wallpaper_solid_pixels wallpaper_solid_off wallpaper_patch_pixels wallpaper_patch_colors
  local theme_primary lua_primary
  if [ ! -s "$theme_status_path" ]; then
    fail "no theme status produced"
  fi
  cat "$theme_status_path"; echo

  # The compositor palette. `theme status` reports the wallpaper and the mode
  # but not a colour, so theme.json is the primary's source of truth here and
  # the two hypr files are checked against it rather than against each other.
  theme_primary=$(sed -n 's/.*"primary": "#\([0-9a-fA-F]\{6\}\)".*/\1/p' "$theme_json_path" 2>/dev/null | head -1)
  if [ -z "$theme_primary" ]; then
    fail "no primary in $theme_json_path"
  fi
  [ -s "$hypr_lua_path" ] || fail "no Hyprland palette written at $hypr_lua_path"
  # dofile is what a hyprland.lua reads this with, so a file that is not a
  # table literal comes back nil and every colour silently falls through to
  # the config's static fallback.
  head -c 8 "$hypr_lua_path" | grep -q '^--' || fail "$hypr_lua_path does not start with a Lua comment"
  grep -q '^return {' "$hypr_lua_path" || fail "$hypr_lua_path has no 'return {' table literal"
  lua_primary=$(sed -n 's/^ *primary = "rgb(\([0-9a-fA-F]\{6\}\))",$/\1/p' "$hypr_lua_path" | head -1)
  if [ "$lua_primary" != "$theme_primary" ]; then
    fail "formalshell-colors.lua primary is '$lua_primary', theme.json's is '$theme_primary'"
  fi
  echo "SMOKE_WALLPAPER_HYPRCOLORS primary $theme_primary in theme.json and formalshell-colors.lua"

  # And the claim neither file can make on its own: the value reached the
  # RUNNING compositor. The rig disables Hyprland's config watcher, so the
  # only way the sentinel the fixture seeded can be gone is the `hyprctl
  # reload` ThemeEngine issues after publishing.
  [ -s "$hypr_border_path" ] || fail "no hyprctl getoption dump produced"
  cat "$hypr_border_path"; echo
  if grep -qiF "$hypr_sentinel" "$hypr_border_path"; then
    fail "general:col.active_border still carries the seeded sentinel, so no hyprctl reload reached the compositor"
  fi
  if ! grep -qiF "$theme_primary" "$hypr_border_path"; then
    fail "general:col.active_border does not carry the wallpaper primary $theme_primary. Got: $(cat "$hypr_border_path")"
  fi
  echo "SMOKE_WALLPAPER_HYPRRELOAD general:col.active_border is $theme_primary after the shell's hyprctl reload"
  if [ ! -s "$wallpaper_get_path" ] || ! grep -qF "$wp2_path" "$wallpaper_get_path"; then
    fail "wallpaper get did not report the second wallpaper. Got: $(cat "$wallpaper_get_path" 2>/dev/null)"
  fi
  for f in "$wallpaper_solid_path" "$wallpaper_gradient_path"; do
    [ -f "$f" ] || fail "no wallpaper screenshot produced at $f"
  done

  # Half one, and the same claim whether or not the pass ran: a monotone
  # source's own color is its whole derived palette, so every cell matches an
  # entry exactly and nothing has a second-nearest to dither against. A 64x64
  # patch of bare wallpaper, well clear of the bar, must be #7a3fb0
  # (122,63,176) and nothing else. This is the owner's 2026-08-12 report as
  # an assertion: the posterize grid that preceded the derived palette
  # speckled exactly here.
  wallpaper_solid_pixels=$(wallpaper_read_patch "$wallpaper_solid_path" 64x64+100+500)
  if [ -z "$wallpaper_solid_pixels" ]; then
    fail "could not read any pixel out of the solid-wallpaper patch"
  fi
  wallpaper_solid_off=$(printf '%s\n' "$wallpaper_solid_pixels" \
    | awk '{ if ($1 != 122 || $2 != 63 || $3 != 176) { print; exit } }')
  if [ -n "$wallpaper_solid_off" ]; then
    fail "monotone wallpaper pixel ($wallpaper_solid_off) is not the source color 7a3fb0"
  fi
  echo "SMOKE_WALLPAPER_FLAT 64x64 patch of the monotone wallpaper is 7a3fb0 end to end"
  echo "SMOKE_WALLPAPER_SOLID $wallpaper_solid_path"

  # Half two, and the half that reads the setting: a full-width strip of the
  # gradient crosses the whole ramp. With the pass off (M45's default, and
  # what this run's settings.json leaves unset) the strip has to carry far
  # more colors than any derived palette is allowed, which is what proves the
  # plain image reached the screen. With SMOKE_WALLPAPER_DITHER=1 the same
  # strip has to land inside the palette cap instead, and carry more than a
  # couple of colors, which is what proves the opt-in still quantizes AND
  # dithers. Read as whole rows because the strip spans every column; a small
  # square could legitimately sit inside one flat palette cell.
  wallpaper_patch_pixels=$(wallpaper_read_patch "$wallpaper_gradient_path" 1920x40+0+500)
  if [ -z "$wallpaper_patch_pixels" ]; then
    fail "could not read any pixel out of the gradient-wallpaper patch"
  fi
  wallpaper_patch_colors=$(printf '%s\n' "$wallpaper_patch_pixels" | sort -u | wc -l | tr -d ' ')
  if $wallpaper_dither; then
    if [ "${wallpaper_patch_colors:-0}" -lt 3 ]; then
      fail "wallpaper.dither is on and the gradient strip carries only $wallpaper_patch_colors color(s), so nothing dithered"
    fi
    if [ "$wallpaper_patch_colors" -gt 6 ]; then
      fail "wallpaper.dither is on and the gradient strip carries $wallpaper_patch_colors colors, more than the 6-entry derived palette allows, so the pass did not quantize"
    fi
    echo "SMOKE_WALLPAPER_DITHER $wallpaper_patch_colors palette colors across a 1920x40 strip, wallpaper.dither on"
  else
    if [ "${wallpaper_patch_colors:-0}" -le 16 ]; then
      fail "wallpaper.dither is off and the gradient strip carries only $wallpaper_patch_colors colors, so something quantized it anyway"
    fi
    echo "SMOKE_WALLPAPER_PLAIN $wallpaper_patch_colors distinct colors across a 1920x40 strip, wallpaper.dither off"
  fi
  echo "SMOKE_WALLPAPER_GRADIENT $wallpaper_gradient_path"
}
