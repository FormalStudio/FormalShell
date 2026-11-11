# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, iso_home, the *_bin paths and fail()
# --flexoki: a wallpaper under a flexoki/ directory, and the claim that the
# pin now reaches every template rather than the shell alone. A user matugen
# config with one template of its own rides along, so the run proves the same
# rewrite on a file this repo does not own.
#
# What makes the assertions real: Flexoki green (#879a39) and yellow
# (#d0a215) cannot come out of a Material scheme seeded on Flexoki blue at
# all, nor can the base tones the surfaces take. So a probe carrying them
# can only have been rewritten before matugen read it.
#
# The post_hook is here for the same reason: matugen renders a hook string
# through its own engine, so the aura/dualsense/ghostty-reload hooks a real
# config carries would otherwise fire with a blue argument.
leg_flexoki_flag="--flexoki"
leg_flexoki_order=105
leg_flexoki_needs="convert"
leg_flexoki_claims="wallpaper theme.mode"

flexoki_wall_dir="$shot_dir/flexoki"
flexoki_wall_path="$flexoki_wall_dir/wall.png"
flexoki_matugen_dir="$iso_home/.config/matugen"
flexoki_probe_tmpl="$flexoki_matugen_dir/templates/probe.tmpl"
flexoki_probe_path="$shot_dir/flexoki-probe.txt"
flexoki_hook_path="$shot_dir/flexoki-hook.txt"
flexoki_status_path="$shot_dir/flexoki-status.json"
flexoki_shot_path="$shot_dir/flexoki.png"
flexoki_gtk_path="$iso_home/.config/gtk-3.0/formalshell-colors.css"
flexoki_qt_path="$iso_home/.config/qt6ct/colors/matugen.conf"
flexoki_theme_json_path="$iso_home/.local/state/formalshell/theme.json"
flexoki_merged_path="$iso_home/.local/state/formalshell/matugen-merged.toml"

leg_flexoki_fixture() {
  mkdir -p "$flexoki_wall_dir" "$flexoki_matugen_dir/templates"
  # The pin is a substring test on the path, so the directory carries it and
  # the file name stays ordinary: the documented "a flexoki/ directory" half
  # of the rule, which no other leg covers.
  flat_wallpaper "$flexoki_wall_path" '#7a3fb0'

  # Deliberately a purple wallpaper. Nothing about its pixels may reach any
  # output, and a purple one makes that visible in the frame if it ever does.
  cat > "$flexoki_probe_tmpl" <<EOF
primary={{colors.primary.default.hex}}
surface={{colors.surface.default.hex}}
on_surface={{colors.on_surface.default.hex}}
outline={{colors.outline.default.hex}}
error={{colors.error.default.hex}}
tertiary={{colors.tertiary.default.hex}}
primary_container={{colors.primary_container.default.hex}}
green={{colors.green.default.hex}}
yellow={{colors.yellow.default.hex}}
magenta_alt={{colors.magenta_alt.default.hex}}
base0a={{base16.base0A.default.hex}}
rgb={{colors.surface.default.rgb}}
strip={{colors.primary.default.hex_stripped}}
mode={{mode}}
EOF

  cat > "$flexoki_matugen_dir/config.toml" <<EOF
[config]

[templates]

[templates.probe]
input_path = "~/.config/matugen/templates/probe.tmpl"
output_path = "$flexoki_probe_path"
post_hook = "printf '%s' {{colors.green.default.hex_stripped}} > $flexoki_hook_path"
EOF
}

leg_flexoki_timing() {
  # The wallpaper lands 3s in and the retheme is two file reads, a rewrite of
  # every template and one matugen run behind it.
  leg_timing 55
}

leg_flexoki_drive() {
  local script="$shot_dir/flexoki-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
$ipc call wallpaper set "$flexoki_wall_path" > /dev/null 2>&1
until_wallpaper "$flexoki_wall_path"
until_settled
$ipc call theme status > "$flexoki_status_path" 2>&1
"$grim_bin" "$flexoki_shot_path" > /dev/null 2>&1
EOF
  wm_exec_once "bash $script"
}

# key=value out of the probe, so a missing line fails loudly rather than
# matching an empty grep.
flexoki_probe_value() {
  sed -n "s/^$1=//p" "$flexoki_probe_path" | head -1
}

flexoki_expect() {
  local got
  got=$(flexoki_probe_value "$1")
  if [ "$got" != "$2" ]; then
    fail "probe $1 is '$got', expected the Flexoki $2"
  fi
}

leg_flexoki_assert() {
  local theme_primary
  [ -s "$flexoki_status_path" ] || fail "no theme status produced"
  cat "$flexoki_status_path"; echo
  grep -qF "$flexoki_wall_path" "$flexoki_status_path" \
    || fail "theme status does not report the flexoki wallpaper. Got: $(cat "$flexoki_status_path")"

  [ -s "$flexoki_probe_path" ] || fail "the user template rendered nothing at $flexoki_probe_path"
  cat "$flexoki_probe_path"
  grep -qF '{{' "$flexoki_probe_path" && fail "the user template still carries an unrendered expression"

  # The Material half of the palette, in real Flexoki tones.
  flexoki_expect primary "#4385be"
  flexoki_expect surface "#100f0f"
  flexoki_expect on_surface "#cecdc3"
  flexoki_expect outline "#575653"
  flexoki_expect error "#d14d41"
  flexoki_expect tertiary "#da702c"
  flexoki_expect primary_container "#12253b"
  # The half Material has no room for at all, and the reason a terminal used
  # to come out blue where it asked for green.
  flexoki_expect green "#879a39"
  flexoki_expect yellow "#d0a215"
  flexoki_expect magenta_alt "#a02f6f"
  flexoki_expect base0a "#d0a215"
  # Formats other than hex, and matugen's own keywords still matugen's.
  flexoki_expect rgb "rgb(16, 15, 15)"
  flexoki_expect strip "4385be"
  flexoki_expect mode "dark"
  echo "SMOKE_FLEXOKI_TEMPLATE $flexoki_probe_path"

  [ -s "$flexoki_hook_path" ] || fail "the template's post_hook never ran (no $flexoki_hook_path)"
  if [ "$(cat "$flexoki_hook_path")" != "879a39" ]; then
    fail "post_hook ran with '$(cat "$flexoki_hook_path")', not the Flexoki green 879a39"
  fi
  echo "SMOKE_FLEXOKI_HOOK post_hook fired with 879a39"

  # The shell's own app-facing templates, which took matugen's scheme before
  # and are what a GTK or Qt app actually reads.
  [ -s "$flexoki_gtk_path" ] || fail "no GTK palette written at $flexoki_gtk_path"
  grep -qF '@define-color accent_color #4385be;' "$flexoki_gtk_path" \
    || fail "GTK accent_color is not Flexoki blue: $(grep -F accent_color "$flexoki_gtk_path" | head -1)"
  grep -qF '@define-color window_bg_color #100f0f;' "$flexoki_gtk_path" \
    || fail "GTK window_bg_color is not Flexoki black: $(grep -F window_bg_color "$flexoki_gtk_path" | head -1)"
  [ -s "$flexoki_qt_path" ] || fail "no Qt palette written at $flexoki_qt_path"
  grep -q '^active_colors=#cecdc3, #100f0f,' "$flexoki_qt_path" \
    || fail "Qt active_colors does not open on Flexoki ink and paper: $(grep '^active_colors' "$flexoki_qt_path")"
  echo "SMOKE_FLEXOKI_APPS gtk-3.0 and qt6ct palettes are Flexoki"

  # And the shell's own half, which the static write still owns.
  theme_primary=$("$jq_bin" -r '.primary // empty' "$flexoki_theme_json_path" 2>/dev/null)
  [ "$theme_primary" = "#4385be" ] || fail "theme.json primary is '$theme_primary', not the Flexoki blue"

  # Nothing above can distinguish a rewrite from a lucky matugen run without
  # this: the merged config the run read points at the staged copies.
  [ -s "$flexoki_merged_path" ] || fail "no merged matugen config at $flexoki_merged_path"
  grep -q "input_path = '.*/pinned-templates/[0-9]*\.tmpl'" "$flexoki_merged_path" \
    || fail "the merged config never got repointed at the rewritten templates"
  echo "SMOKE_FLEXOKI_CONFIG $flexoki_merged_path"
  echo "SMOKE_FLEXOKI $flexoki_shot_path"
}
