# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154,SC2086  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail(); the *_bin values expand unquoted
# --screensaver-gif records five ttfx effects as GIFs, one independent
# Hyprland session per effect, since screensaver.effect is pinned through the
# settings fixture a session reads at startup. That is why this leg takes the
# run over instead of joining the shared timeline: it owns its own sessions
# and exits when the last GIF is written.
#
# Each run pins `screensaver frame 0` (which is what makes a streaming run's
# frame count knowable at all), reads the count off `screensaver frameInfo`,
# then steps `screensaver frame(n)` across a strided sample of the run plus an
# 8-frame hold on the converged banner, grim-screenshotting each. Frame-
# stepped, not wall-clock-timed, so software rendering cannot produce uneven
# spacing; the GIF's own delay is derived from the stride so playback is real
# time. frameInfo has to report the pinned effect AND "engine":"ttfx" before a
# run is accepted: a missing ttfx would otherwise record a perfectly
# plausible GIF of the builtin fallback.
#
# matrix and thunderstorm are deliberately not in the list: both are gated on
# wall-clock time, so the same frame index means something different on the
# next machine.
leg_screensaver_gif_flag="--screensaver-gif"
leg_screensaver_gif_order=240
leg_screensaver_gif_needs="convert"

leg_screensaver_gif_takeover() {
  local hold=8 budget=120 fps=60
  local effects=(decrypt rain expand slide scattered)
  local gif_home media_dir effect effect_dir frames_dir cfg info_path
  local drive_script shell_script shell_log hypr_log convergence stride
  local expected_frames frame_count out_gif gif_delay last_frame i
  local -a frame_files
  local -a gif_env
  local gif_shell="$PWD/result/bin/formalshell"

  # SCREENSAVER_GIF_EFFECTS (optional, space-separated) limits the run to a
  # subset: verifying the recorder itself needs one effect and must not
  # regenerate every committed GIF to prove it.
  if [ -n "${SCREENSAVER_GIF_EFFECTS:-}" ]; then
    read -r -a effects <<< "$SCREENSAVER_GIF_EFFECTS"
  fi

  gif_home=$(mktemp -d)
  media_dir="$PWD/docs/media"
  mkdir -p "$media_dir" "$gif_home/.config/formalshell"

  for i in "${!effects[@]}"; do
    effect="${effects[$i]}"
    # One temp dir per effect (frames, config and scripts together), removed
    # once its GIF exists, except the last one's: dev/remote.sh still has to
    # pull this run's SMOKE_OK frame back after the script exits.
    effect_dir=$(mktemp -d)
    frames_dir="$effect_dir/frames"
    mkdir -p "$frames_dir"
    cfg="$effect_dir/hyprland.lua"
    info_path="$frames_dir/frame-info.json"
    shell_script="$effect_dir/shell-start.sh"
    shell_log="$effect_dir/shell.log"
    hypr_log="$effect_dir/hyprland.log"
    cat > "$gif_home/.config/formalshell/settings.json" <<EOF
{"screensaver": {"effect": "$effect"}}
EOF

    write_script "$shell_script" <<EOF
#!/usr/bin/env bash
export LIBGL_ALWAYS_SOFTWARE=1
exec "$gif_shell" > "$shell_log" 2>&1
EOF

    drive_script="$effect_dir/drive.sh"
    write_script "$drive_script" <<EOF
#!/usr/bin/env bash
sleep 3
$ipc call screensaver start > /dev/null 2>&1
sleep 1
# Pin frame 0 before asking for the count: a ttfx run streams, so the only
# honest frame total is the one a completed pinned run actually produced, and
# frameInfo answers 0 until then rather than guessing.
$ipc call screensaver frame 0 > /dev/null 2>&1
sleep 2
$ipc call screensaver frameInfo > "$info_path" 2>&1
convergence=\$(grep -o '"convergenceFrame":[0-9]*' "$info_path" | cut -d: -f2)
stride=\$(( (convergence + $budget - 1) / $budget ))
[ "\$stride" -lt 1 ] && stride=1
idx=0
for ((i = 0; i < convergence; i += stride)); do
  $ipc call screensaver frame "\$i" > /dev/null 2>&1
  # A pin regenerates the whole run to reach the frame, so the surface needs
  # that generation plus one repaint before grim reads the framebuffer.
  sleep 0.4
  printf -v padded "%04d" "\$idx"
  "$grim_bin" "$frames_dir/frame-\$padded.png" > /dev/null 2>&1
  idx=\$((idx + 1))
done
# The hold is the converged banner repeated, which is exactly what the live
# surface shows between convergence and the next cycle: copied rather than
# recaptured, since re-pinning the same frame regenerates the whole run again
# for a byte-identical screenshot.
printf -v last "%04d" "\$((idx - 1))"
for ((h = 0; h < $hold; h++)); do
  printf -v padded "%04d" "\$((idx + h))"
  cp "$frames_dir/frame-\$last.png" "$frames_dir/frame-\$padded.png"
done
$ipc call screensaver stop > /dev/null 2>&1
"$hyprctl_bin" dispatch "hl.dsp.exit()"
EOF

    # The same session shape dev/smoke.sh builds for the shared run: the size
    # pinned so the banner is worth reading, no blur, no animations, and both
    # of Hyprland's own full-width top-of-screen banners (the watchdog warning
    # and the error overlay) suppressed, since every frame here is a picture
    # of the whole output.
    {
      hypr_base_config
      hypr_exec_once "bash $shell_script"
      hypr_exec_once "bash $drive_script"
    } > "$cfg"

    gif_env=(
      "HOME=$gif_home"
      "XDG_CONFIG_HOME=$gif_home/.config"
      "XDG_STATE_HOME=$gif_home/.local/state"
      "XDG_DATA_HOME=$gif_home/.local/share"
      "XDG_DATA_DIRS=$gif_home/.local/share"
      "XDG_CACHE_HOME=$gif_home/.cache"
    )
    if [ "$session_mode" = "nested" ]; then
      gif_env+=("WAYLAND_DISPLAY=$wayland_display")
      env "${gif_env[@]}" dbus-run-session -- \
        timeout -k 10 300 $hyprland_bin --config "$cfg" > "$hypr_log" 2>&1 || true
    else
      # -u WAYLAND_DISPLAY, not an empty value: aquamarine reads the
      # variable's presence and an empty one still sends it down the wayland
      # backend, which then fails to connect at all.
      gif_env+=("AQ_DRM_DEVICES=$vkms_device")
      env -u WAYLAND_DISPLAY "${gif_env[@]}" dbus-run-session -- \
        timeout -k 10 300 $hyprland_bin --config "$cfg" > "$hypr_log" 2>&1 || true
    fi

    if [ ! -s "$info_path" ] || ! grep -q "\"effect\":\"$effect\"" "$info_path"; then
      echo "SMOKE_FAIL: screensaver frameInfo did not report effect '$effect', got: $(cat "$info_path" 2>/dev/null)" >&2
      tail -20 "$shell_log" >&2 2>/dev/null || true
      exit 1
    fi
    # The engine is asserted, not assumed: with ttfx missing from the shell's
    # wrapper PATH the surface silently falls back to effect.js's builtin
    # effects, which would still record a plausible GIF of the wrong thing.
    if ! grep -q '"engine":"ttfx"' "$info_path"; then
      echo "SMOKE_FAIL: screensaver is not running the ttfx engine, got: $(cat "$info_path" 2>/dev/null)" >&2
      exit 1
    fi
    convergence=$(grep -o '"convergenceFrame":[0-9]*' "$info_path" | cut -d: -f2)
    if [ -z "$convergence" ] || [ "$convergence" -le 0 ]; then
      echo "SMOKE_FAIL: screensaver frameInfo reported no frames for '$effect', got: $(cat "$info_path" 2>/dev/null)" >&2
      exit 1
    fi
    stride=$(( (convergence + budget - 1) / budget ))
    [ "$stride" -lt 1 ] && stride=1
    expected_frames=$(( (convergence + stride - 1) / stride + hold ))

    shopt -s nullglob
    frame_files=("$frames_dir"/frame-*.png)
    shopt -u nullglob
    frame_count=${#frame_files[@]}
    # Exact, not "enough frames": a session killed mid-capture would otherwise
    # still produce a plausible-looking but truncated GIF that never reaches
    # the banner, and this would exit 0 regardless.
    if [ "$frame_count" -ne "$expected_frames" ]; then
      echo "SMOKE_FAIL: $effect captured $frame_count frame(s), expected $expected_frames (convergence $convergence at stride $stride plus hold $hold), session likely killed mid-capture" >&2
      exit 1
    fi

    out_gif="$media_dir/screensaver-$effect.gif"
    # Frame delay in centiseconds, derived from the stride so the GIF plays at
    # the speed the animation really runs: one captured frame stands for
    # `stride` frames at screensaver.frameRate. Floored at 2cs, as fast as GIF
    # playback goes in practice.
    gif_delay=$(( (stride * 100 + fps / 2) / fps ))
    [ "$gif_delay" -lt 2 ] && gif_delay=2
    $convert_bin -delay "$gif_delay" -loop 0 "$frames_dir"/frame-*.png -resize 640x -coalesce -layers Optimize -colors 96 "$out_gif"
    if [ ! -s "$out_gif" ]; then
      echo "SMOKE_FAIL: imagemagick did not produce $out_gif for effect $effect" >&2
      exit 1
    fi
    echo "SMOKE_SCREENSAVER_GIF_${effect^^} $out_gif ($(wc -c < "$out_gif" | tr -d ' ') bytes, $frame_count frames)"

    last_frame="${frame_files[$((frame_count - 1))]}"
    if [ "$i" -lt "$((${#effects[@]} - 1))" ]; then
      rm -rf "$effect_dir"
    fi
  done

  host_notifications_owner_after=$(host_notifications_owner)
  if [ "$host_notifications_owner_before" != "$host_notifications_owner_after" ]; then
    echo "SMOKE_FAIL: host org.freedesktop.Notifications owner PID changed ($host_notifications_owner_before -> $host_notifications_owner_after), a recording session's NotificationServer touched the host bus" >&2
    exit 1
  fi

  echo "SMOKE_OK $last_frame"
  exit 0
}
