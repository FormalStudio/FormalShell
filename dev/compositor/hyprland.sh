# shellcheck shell=bash
# dev/smoke.sh's Hyprland, FS_COMPOSITOR=hyprland (the default): sourced by
# the scaffold, which calls these and nothing of Hyprland's directly. The
# legs reach it through $wmctl_bin, hyprctl itself here, and wm_exec_once.
# dev/compositor/shoji.sh answers the same names.

wm_config_path="$shot_dir/hyprland.lua"
wm_log_path="$shot_dir/hyprland.log"

wm_need() {
  need_tool hyprland_bin Hyprland hyprland
  need_tool wmctl_bin hyprctl hyprland
}

# The rig boots from a Lua config, the only format Hyprland is supported on,
# and starts its own processes through this helper, one autostart line each.
# A leg's drive scripts go through it too, but land in drives.list under the
# leg's name instead, for drives.sh to start in leg order.
wm_exec_once() {
  if [ "${recording_drives:-}" = 1 ]; then
    printf '%s\t%s\n' "$leg_name" "$1" >> "$drives_list"
    return
  fi
  wm_autostart "$1"
}

# The autostart line itself, for a drive that starts beside the shell
# rather than in leg order after its arrival.
wm_autostart() {
  printf 'hl.on("hyprland.start", function() hl.exec_cmd([==[%s]==]) end)\n' "$1"
}

# wm_bind [--repeat] [--release] [--transparent] [--title <text>] <chord> <action> [<arg>]:
# a bind in the session's own table. The chord and action are formalwm's
# spelling (SUPER+T, wm/core/src/action.rs); --transparent is a modifier's
# release bind that still fires after a chord with it, formalwm's passthrough.
wm_bind() {
  local opts=() chord action arg dsp joined
  while :; do
    case $1 in
      --repeat) opts+=("repeating = true") ;;
      --release) opts+=("release = true") ;;
      --transparent) opts+=("transparent = true") ;;
      --title) opts+=("description = \"$2\""); shift ;;
      *) break ;;
    esac
    shift
  done
  chord=${1//+/ + } action=$2 arg=${3:-}
  case $action in
    sh) dsp="hl.dsp.exec_cmd([==[$arg]==])" ;;
    close) dsp="hl.dsp.window.close()" ;;
    focus-column) dsp="hl.dsp.focus({ direction = \"${arg:0:1}\" })" ;;
    workspace) dsp="hl.dsp.focus({ workspace = $arg })" ;;
    swap-column) dsp="hl.dsp.layout(\"swapcol ${arg:0:1}\")" ;;
    swap-window) dsp="hl.dsp.window.swap({ direction = \"$arg\" })" ;;
    *) echo "SMOKE_FAIL: wm_bind: no Hyprland form for $action" >&2; exit 1 ;;
  esac
  if [ ${#opts[@]} -eq 0 ]; then
    printf 'hl.bind("%s", %s)\n' "$chord" "$dsp"
    return
  fi
  joined=$(printf '%s, ' "${opts[@]}")
  printf 'hl.bind("%s", %s, { %s })\n' "$chord" "$dsp" "${joined%, }"
}

# wm_commented_bind <chord>: a bind the config carries commented out, which
# must not reach the bind table.
wm_commented_bind() {
  printf -- '-- hl.bind("%s", hl.dsp.exit())\n' "${1//+/ + }"
}

# The shipped include's MAC APPS block, which records every bind made after
# it to build the `mac` submap from.
wm_mac_apps() {
  awk '/^-- MAC APPS/ { on = 1 } /^-- Window rounding/ { on = 0 } on { print }' docs/examples/hyprland/formalshell.lua
}

# Binds resolve a keycode through the sending keyboard's own keymap, which
# for wtype is its own; switcher_keys.sh says why the rig needs that.
wm_binds_by_keysym() {
  echo 'hl.config({ input = { resolve_binds_by_sym = true } })'
}

# wm_key_repeat <delay ms> <per second>
wm_key_repeat() {
  echo "hl.config({ input = { repeat_delay = $1, repeat_rate = $2 } })"
}

# wm_window_gaps <outer> <border>: the base config's zero gaps and border
# widened for a leg that measures against them.
wm_window_gaps() {
  echo "hl.config({ general = { gaps_out = $1, border_size = $2 } })"
}

# The blur the base config turns off, back on with the shipped layer rules,
# as the owner's hyprland.lua loads them.
wm_shipped_blur() {
  echo 'local chrome = { blur = true }'
  awk '/^hl.layer_rule\(\{/ { on = 1 } on { print } on && /^\}\)/ { on = 0 }' docs/examples/hyprland/formalshell.lua
  echo 'hl.config({ decoration = { blur = { enabled = true, size = 8, passes = 2, new_optimizations = true, vibrancy = 0 } } })'
}

# wm_border_from_palette <colors.lua>: the focused border off the shell's
# palette, wired the way docs/USAGE.md tells a user to wire it. Appended
# after the base config's own `general` block, so this value survives.
wm_border_from_palette() {
  echo "local colors = dofile([==[$1]==])"
  echo "hl.config({ general = { col = { active_border = colors.primary } } })"
}

# The session shape every run boots with, shared with --screensaver-gif's
# per-effect sessions.
wm_base_config() {
  # Pinned in both session modes: the vkms connector otherwise comes up
  # 1024x768 at scale 2, and a nested window takes aquamarine's own default
  # rather than a size the bar is worth reading at. FS_SMOKE_SCALE=1.25
  # runs it fractional, the g815 panel's scale.
  echo "hl.monitor({ output = \"\", mode = \"${out_w}x${out_h}@60\", position = \"0x0\", scale = ${FS_SMOKE_SCALE:-1} })"
  echo "hl.config({"
  echo "  general = { gaps_in = 0, gaps_out = 0, border_size = 0 },"
  # DESIGN.md: nothing in the shell blurs anything, and a compositor blur
  # behind the bar's own transparent strip is exactly the frame this rig
  # exists to catch.
  echo "  decoration = { rounding = 0, blur = { enabled = false } },"
  echo "  animations = { enabled = false },"
  # disable_watchdog_warning: without start-hyprland (a watchdog wrapper this
  # rig has no use for) Hyprland posts a red full-width notification across
  # the top of the screen for 15s, which is exactly where the bar is.
  echo "  misc = {"
  echo "    disable_watchdog_warning = true,"
  echo "    disable_hyprland_logo = true,"
  echo "    disable_splash_rendering = true,"
  echo "    force_default_wallpaper = 0,"
  echo "    disable_autoreload = true,"
  echo "    allow_session_lock_restore = true,"
  echo "  },"
  # suppress_errors: Hyprland's error overlay is a full-width banner across
  # the top of the screen, which is where the bar is. It renders over the
  # exact thing every leg here photographs.
  echo "  debug = { suppress_errors = true, disable_logs = false, enable_stdout_logs = true },"
  echo "})"
}

# The compositor's own part of the session's environment, after the
# scaffold's. HYPRLAND_NO_SD_VARS and the systemctl shim keep the nested
# Hyprland off the user manager it shares with the host session (see
# host_env_names).
wm_session_env() {
  session_env+=("HYPRLAND_NO_SD_VARS=1")
  [ "$session_mode" = vkms ] || return 0
  session_env+=("AQ_DRM_DEVICES=$vkms_device")
  # Without seatd (the mac rig runs none), libseat finds no seat for an ssh
  # login and aquamarine's backend fails to create. noop opens the card
  # straight, which the video group allows, and lets four slots each be the
  # master of their own card at once.
  [ -S /run/seatd.sock ] || session_env+=("LIBSEAT_BACKEND=noop")
}

# wm_command <seconds>: into wm_cmd, the command line the session runs
# until it exits or the seconds are up. -k 10 force-kills 10s after the
# initial SIGTERM, since Hyprland's SIGTERM handler can hang instead of
# exiting.
wm_command() {
  wm_cmd=(timeout -k 10 "$1" "$hyprland_bin" --config "$wm_config_path")
}

# The line the teardown script ends the session with.
wm_exit_line() {
  printf '%q dispatch "hl.dsp.exit()"\n' "$wmctl_bin"
}

# Hyprland ends the session on its exit dispatch; the config needs no tail.
wm_config_tail() { :; }

# wm_cover_output <selector>: drive-script lines that make a lone window
# cover the output. Hyprland's default dwindle already does, gaps being 0.
wm_cover_output() { :; }

# wm_full_width_columns: a lone tiled window spans the work area. Hyprland's
# already does.
wm_full_width_columns() { :; }

# wm_overview_phase: a drive-script line printing the overview's phase,
# open or closed. The shell draws it under Hyprland.
wm_overview_phase() {
  printf '%s call debug dump 2>/dev/null | %q -r .overview.phase\n' "$ipc" "$jq_bin"
}

# wm_lock_release_control: drive-script lines that release a window's
# pointer lock, for a leg proving its probe sees a release. Hyprland drops
# the lock when any layer maps, the overlay `debug join` hangs here.
wm_lock_release_control() {
  printf '%s call debug join top 100 200 > /dev/null 2>&1\nsleep 1\n%s call debug joinClear > /dev/null 2>&1\n' "$ipc" "$ipc"
}

# wm_wheel <dy>: a drive-script line scrolling the pointer DY wheel units.
# wlrctl names the source finger, which Hyprland's axis request drops, so
# it reads as a wheel here.
wm_wheel() {
  printf '%q pointer scroll %s 0' "$wlrctl_bin" "$1"
}

# wm_blur_on: drive-script lines that turn the compositor's blur on in the
# running session: the config rewritten and reloaded, which the shell reads
# off `configreloaded`.
wm_blur_on() {
  printf '%s\n' "sed -i 's/blur = { enabled = false }/blur = { enabled = true }/' $(printf '%q' "$wm_config_path")"
  printf '%q reload > /dev/null 2>&1\n' "$wmctl_bin"
}

# The palette role the focused border takes: primary, as wm_border_from_palette
# wires it.
wm_border_role=primary

# The shell writes ~/.config/hypr/formalshell-colors.lua for this one.
wm_hypr_palette=true

# How zen holds its window: the shell floats it, centred, and Hyprland's
# scrolling layout centres the narrower tape left of it.
wm_zen=float

# A window sent to a workspace past the last makes it.
wm_workspaces_grow=true

# Whether a window off every monitor still gives a capture frames: Hyprland
# copies one only while its box overlaps its monitor
# (`CScreenshareManager::onOutputCommit`).
wm_offscreen_frames=false

# The tiling layouts a leg can switch to with `hl.config({ general = { layout } })`.
wm_layouts="scrolling dwindle"

# What a compositor process of this run still carries on its command line.
wm_leftover_pattern="--config $wm_config_path"
