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
  wm_autostart "$1"
}

wm_autostart() {
  printf 'sh -c %q &\n' "$1"
}

shoji_host_path="$iso_home/.config/formalwm/host.json"

# The session script's head. formalwm reads its settings at start, so what
# the rig pins goes in a host.json beside them, merged over settings.json as
# on a real machine, where a list replaces the shared one whole. One mode for
# whatever connector the card has, at the run's scale; the us layout with no
# options; no layer blurs (the Hyprland base turns blur off for the reason
# hyprland.sh gives); and no binds but the ones legs add, as on Hyprland.
wm_base_config() {
  mkdir -p "$iso_home/.config/formalwm"
  cp "$PWD/wm/example/settings.json" "$iso_home/.config/formalwm/settings.json"
  "$jq_bin" -n --arg mode "${out_w}x${out_h}@60" --argjson scale "${FS_SMOKE_SCALE:-1}" '{
    outputs: [{ mode: $mode, position: [0, 0], scale: $scale }],
    input: { keyboard: { layout: "us", variant: null, options: null } },
    layers: [],
    binds: []
  }' > "$shoji_host_path"
  echo '#!/usr/bin/env bash'
  echo "echo \"\$SHOJI_RUN_DIR\" > $(printf '%q' "$shot_dir/shoji-run-dir")"
  # shoji-run.sh gives the session a runtime dir of its own, where a nested
  # Hyprland shares the slot's: the slot's PipeWire, and whatever a leg
  # wrote there before the session (the login marker), linked in beside the
  # run's own socket and bus.
  # shoji-run.sh makes the run's own dir inside the slot's, so not that one.
  printf 'for e in %q/*; do case ${e##*/} in shoji-run-*) continue ;; esac; [ -e "$XDG_RUNTIME_DIR/${e##*/}" ] || ln -s "$e" "$XDG_RUNTIME_DIR/"; done\n' "$XDG_RUNTIME_DIR"
}

wm_session_env() { :; }

# shoji_host <jq filter> [jq args]: the filter laid over the rig's host.json
# before the session starts. formalwm reads it once, at start.
shoji_host() {
  local filter=$1
  shift
  "$jq_bin" "$@" "$filter" "$shoji_host_path" > "$shoji_host_path.new" \
    && mv "$shoji_host_path.new" "$shoji_host_path"
}

# The forms of hyprland.sh's helpers of the same names; see there.
wm_bind() {
  local repeat=false release=false title=""
  while :; do
    case $1 in
      --repeat) repeat=true ;;
      --release) release=true ;;
      --transparent) ;;
      --title) title=$2; shift ;;
      *) break ;;
    esac
    shift
  done
  # formalwm refuses a settings file with one bad bind whole, the session
  # with it, so a bind it cannot take stops the run here instead.
  case $2:${3:-} in
    swap-window:left | swap-window:right)
      echo "SMOKE_FAIL: formalwm swaps windows up or down in a column; $1's sideways swap is Hyprland's dwindle" >&2
      exit 1 ;;
  esac
  shoji_host '.binds += [{ chord: $chord, action: $action, args: $args, title: $title, repeat: $repeat, release: $release }]' \
    --arg chord "$1" --arg action "$2" --arg title "$title" --argjson repeat "$repeat" --argjson release "$release" \
    --argjson args "$("$jq_bin" -nc '$ARGS.positional' --args "${@:3}")"
}

# The rig's host.json is jq's output, which keeps no comments.
wm_commented_bind() { :; }

# formalwm resolves a bind through the sending keyboard's own keymap already.
wm_binds_by_keysym() { :; }

wm_key_repeat() {
  shoji_host '.input.keyboard += { repeatDelay: $d, repeatRate: $r }' --argjson d "$1" --argjson r "$2"
}

# The shell owns formalwm's gaps and border, through theme.set from its own
# style, so a leg measures against what the shell sent.
wm_window_gaps() { :; }

# The shipped settings' layer rules, which the base host.json emptied.
wm_shipped_blur() {
  shoji_host 'del(.layers)'
}

# The shell sends formalwm its palette through theme.set; nothing to wire.
wm_border_from_palette() { :; }

# wm_command <seconds>: into wm_cmd, the session: formalwm on the card,
# the session script inside it, everything torn down once the script ends.
# shoji-run.sh finds the card's lock and its stable tree under the slot
# user's own HOME, so only the script gets the run's.
wm_command() {
  wm_cmd=(env HOME="$HOME" SHOJI_BIN="$shoji_bin" SHOJI_RUN_TIMEOUT="$1" "$HOME/shoji-run.sh" -- env HOME="$iso_home" bash "$wm_config_path")
}

# The session script's tail waits for this, so the teardown is the
# script's own end rather than a request the engine might not answer.
wm_exit_line() {
  printf ': > %q\n' "$shot_dir/session-over"
}

wm_config_tail() {
  printf 'until [ -e %q ]; do sleep 0.2; done\n' "$shot_dir/session-over"
}

# A lone formalwm column keeps the settings' width, so the window floats,
# placed over the whole output.
wm_cover_output() {
  cat <<EOS
read -r cover_w cover_h <<< "\$("$wmctl_bin" -j monitors | "$jq_bin" -r '.[0] | "\(.width / .scale | floor) \(.height / .scale | floor)"')"
"$wmctl_bin" dispatch "hl.dsp.window.float({ window = '$1', action = 'enable' })" > /dev/null 2>&1
"$wmctl_bin" dispatch "hl.dsp.window.resize({ x = \$cover_w, y = \$cover_h, window = '$1' })" > /dev/null 2>&1
"$wmctl_bin" dispatch "hl.dsp.window.move({ x = 0, y = 0, window = '$1' })" > /dev/null 2>&1
sleep 1
EOS
}

# formalwm's zen is a layout mode of the workspace (spec section 5): the
# window stays tiled, centred, and each other column slides off its own side.
wm_zen=layout

# formalwm's column tape, which shoji-ctl answers as Hyprland's scrolling.
wm_layouts="scrolling"

wm_leftover_pattern="bash $wm_config_path"
