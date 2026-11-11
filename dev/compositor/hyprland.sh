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
  printf 'hl.on("hyprland.start", function() hl.exec_cmd([==[%s]==]) end)\n' "$1"
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

# What a compositor process of this run still carries on its command line.
wm_leftover_pattern="--config $wm_config_path"
