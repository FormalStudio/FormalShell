# shellcheck shell=bash
# dev/smoke.sh's ShojiWM, FS_COMPOSITOR=shoji: formalwm with this tree's
# wm/example/settings.json on the mac rig's slot 3 (fstest3), whose
# ~/shoji-run.sh owns the vkms card's lock and gives the session its own
# runtime dir and bus. The session is a script shoji-run.sh runs inside
# the compositor's session. $wmctl_bin is shoji-ctl.py, which answers
# hyprctl's command line off formalwm's socket. FS_SHOJI_BIN names the
# compositor binary, the one wm/dev/rig.sh builds by default.

wm_config_path="$shot_dir/session.sh"
wm_log_path="$shot_dir/shoji-run.log"
shoji_bin=${FS_SHOJI_BIN:-$HOME/fwm-target/release-fast/formalwm}

wm_need() {
  [ -x "$HOME/shoji-run.sh" ] \
    || { echo "SMOKE_FAIL: FS_COMPOSITOR=shoji runs on the mac rig's slot 3, whose ~/shoji-run.sh this slot lacks" >&2; exit 1; }
  [ -x "$shoji_bin" ] \
    || { echo "SMOKE_FAIL: no formalwm at $shoji_bin (wm/dev/rig.sh build --profile release-fast -p formalwm, or FS_SHOJI_BIN)" >&2; exit 1; }
  wmctl_bin="$script_dir/compositor/shoji-ctl.py"
}

# One line of the session script per process, as Hyprland's exec-once
# would start it; a leg's drives land in drives.list instead.
wm_exec_once() {
  if [ "${recording_drives:-}" = 1 ]; then
    printf '%s\t%s\n' "$leg_name" "$1" >> "$drives_list"
    return
  fi
  printf 'sh -c %q &\n' "$1"
}

# The session script's head. formalwm reads its settings at start, so the
# output the rig pins goes in a host.json beside them (merged over
# settings.json, as on a real machine): one mode for whatever connector the
# card has, at the run's scale.
wm_base_config() {
  mkdir -p "$iso_home/.config/formalwm"
  cp "$PWD/wm/example/settings.json" "$iso_home/.config/formalwm/settings.json"
  printf '{ "outputs": [{ "mode": "%sx%s@60", "position": [0, 0], "scale": %s }] }\n' \
    "$out_w" "$out_h" "${FS_SMOKE_SCALE:-1}" > "$iso_home/.config/formalwm/host.json"
  echo '#!/usr/bin/env bash'
  echo "echo \"\$SHOJI_RUN_DIR\" > $(printf '%q' "$shot_dir/shoji-run-dir")"
}

wm_session_env() { :; }

# wm_command <seconds>: into wm_cmd, the session: formalwm on the card,
# the session script inside it, everything torn down once the script ends.
wm_command() {
  wm_cmd=(env SHOJI_BIN="$shoji_bin" SHOJI_RUN_TIMEOUT="$1" "$HOME/shoji-run.sh" -- bash "$wm_config_path")
}

# The session script's tail waits for this, so the teardown is the
# script's own end rather than a request the engine might not answer.
wm_exit_line() {
  printf ': > %q\n' "$shot_dir/session-over"
}

wm_config_tail() {
  printf 'until [ -e %q ]; do sleep 0.2; done\n' "$shot_dir/session-over"
}

wm_leftover_pattern="bash $wm_config_path"
