# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154,SC2016  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --screensaver shortens screensaver.timeoutSeconds to 3s through the settings
# fixture (a real IdleMonitor, never a faked clock) and plays a real MPRIS
# track so MediaService.isPlaying is genuinely true. Three claims off one
# timeline: the live media guard holds the screensaver off while the session
# is already idle; killing the player alone auto-activates it, with no
# `screensaver start` call involved; and the manual IPC start/stop path works
# independently of the idle timer. The activation's own output is checked
# against the compositor's monitor list, and a read-only frameInfo poll
# watches the cycle counter leave 0, which is the effect converging, holding
# and rerolling with nothing nudging it.
#
# The rest of the multi-head rules (port prefixes, the internal/external
# aliases, what a plug or unplug does to a run in flight) live in
# the unit tests, since this session has one head.
leg_screensaver_flag="--screensaver"
leg_screensaver_order=230
leg_screensaver_needs="mpv ffmpeg jq convert wtype"

# Shared with dev/smoke.d/visualizer.sh, which the scaffold sources on every
# run. openSUSE's mpv-mpris installs the script where mpv never looks on its
# own; the run's mpv wrapper in dev/smoke.sh passes it with --script=.
need_mpv() {
  need_tool mpv_bin mpv mpv
  mpv_mpris=/usr/lib64/mpv/mpris.so
  [ -f "$mpv_mpris" ] || { echo "SMOKE_FAIL: no MPRIS script for mpv at $mpv_mpris; install the openSUSE package mpv-mpris" >&2; exit 1; }
}

need_ffmpeg() { need_tool ffmpeg_bin ffmpeg ffmpeg; }

# This leg's own clock, the same rule menu_t0 and lock_t0 draw: the screensaver
# covers the whole output, so under --wallpaper it starts after that leg's last
# frame (t=14). The guard holds until this clock runs out either way, since the
# fixture track is still playing.
screensaver_t0() {
  cover_t0 4
}

ss_pid_path="$shot_dir/ss-mpv.pid"
ss_track_path="$shot_dir/ss-track.flac"
ss_guard_status_path="$shot_dir/screensaver-guard-status.json"
ss_auto_path="$shot_dir/screensaver-auto.png"
ss_auto_status_path="$shot_dir/screensaver-auto-status.json"
ss_dismiss_status_path="$shot_dir/screensaver-dismiss-status.json"
ss_manual_path="$shot_dir/screensaver-manual.png"
ss_cycle_info1_path="$shot_dir/screensaver-cycle-info-1.json"
ss_cycle_info2_path="$shot_dir/screensaver-cycle-info-2.json"
ss_solid_info_path="$shot_dir/screensaver-solid-frameinfo.json"
ss_solid_path="$shot_dir/screensaver-solid.png"
ss_solid_mask_path="$shot_dir/screensaver-solid-mask.png"
ss_final_status_path="$shot_dir/screensaver-final-status.json"
ss_chain_locked_path="$shot_dir/screensaver-chain-locked.txt"
ss_chain_status_path="$shot_dir/screensaver-chain-status.json"
ss_chain_unlocked_path="$shot_dir/screensaver-chain-unlocked.txt"
ss_chain_path="$shot_dir/screensaver-chain-locked.png"
ss_settings_path="$iso_home/.config/formalshell/settings.json"

leg_screensaver_fixture() {
  local effect_json="" ascii_json="" ascii_path
  # SCREENSAVER_EFFECT (optional, additive, unset by every caller except a
  # per-effect verification run) pins screensaver.effect for this run's
  # screensaver-manual.png instead of leaving it at the default "random".
  if [ -n "${SCREENSAVER_EFFECT:-}" ]; then
    effect_json=', "effect": "'"$SCREENSAVER_EFFECT"'"'
  fi
  # SCREENSAVER_ASCII_TEXT (same rationale): writes a custom banner file and
  # points screensaver.asciiPath at it, proving the bundled art is really
  # swappable rather than hardcoded.
  if [ -n "${SCREENSAVER_ASCII_TEXT:-}" ]; then
    ascii_path="$iso_home/.config/formalshell/custom-screensaver.txt"
    printf '%s\n' "$SCREENSAVER_ASCII_TEXT" > "$ascii_path"
    ascii_json=', "asciiPath": "'"$ascii_path"'"'
  fi
  # Long enough to outlive the guard proof even under --wallpaper's own
  # clock: the drive script is what ends playback, never the track running
  # out. Silent by design, nothing here listens to it.
  "$ffmpeg_bin" -nostdin -loglevel error -f lavfi -i "anullsrc=r=48000:cl=stereo" -t 90 \
    -metadata "title=FormalShell Smoke Track" -metadata "artist=FormalShell Test Artist" \
    -c:a flac -y "$ss_track_path"
  # holdSeconds is shortened the same way timeoutSeconds is: the cycle proof
  # waits out a real convergence and hold, just on an affordable schedule.
  settings_fragment ', "screensaver": {"timeoutSeconds": 3, "guardMediaPlayback": true, "holdSeconds": 2'"$effect_json$ascii_json"'}'
}

leg_screensaver_timing() {
  # ss-drive.sh's cycle poll can run to its own deadline (the worst first
  # random pick spends a fixed 15s before it even starts resolving) and the
  # final status dump lands a couple of seconds after the poll breaks. The
  # run's own frame is taken past that worst case, showing the ordinary
  # session with the screensaver dismissed for good. The solidity capture
  # after the poll costs another seven seconds of pinning before the final
  # stop, which is where the extra ten come from.
  local t0
  t0=$(screensaver_t0)
  # The lock chain after it adds about fifteen more.
  leg_timing $((105 + t0 - 4)) $((125 + t0 - 4))
}

leg_screensaver_drive() {
  local t0 play_script="$shot_dir/ss-media-play.sh" script="$shot_dir/ss-drive.sh"
  t0=$(screensaver_t0)
  # The pid is recorded before the exec: exec replaces this shell's process
  # image without forking, so $$ here is exactly mpv's eventual pid and the
  # drive script below can end playback precisely.
  write_script "$play_script" <<EOF
#!/usr/bin/env bash
sleep 1
echo \$\$ > "$ss_pid_path"
exec "$mpv_bin" --no-video --really-quiet "$ss_track_path"
EOF

  # Ordered: the guard proof first (mpv still playing), then the kill and the
  # auto-trigger proof, then the manual start/stop last, since a manual
  # activation deliberately suppresses the auto-trigger for the rest of the
  # run and nothing after it depends on auto-triggering again. The cycle proof
  # rides the manual activation because its start time is deterministic.
  write_script "$script" <<EOF
#!/usr/bin/env bash
# Merged into the run's settings and written back through the same inode,
# so the shell's own file watch sees it.
settings_set() {
  "$jq_bin" "\$1" "$ss_settings_path" > "$shot_dir/ss-settings.json" && cat "$shot_dir/ss-settings.json" > "$ss_settings_path"
}
sleep $((t0 + 4))
$ipc call screensaver status > "$ss_guard_status_path" 2>&1
if [ -f "$ss_pid_path" ]; then
  kill "\$(cat "$ss_pid_path")" 2>/dev/null || true
fi
sleep 5
"$grim_bin" "$ss_auto_path" > /dev/null 2>&1
$ipc call screensaver status > "$ss_auto_status_path" 2>&1
$ipc call screensaver stop > /dev/null 2>&1
sleep 1
$ipc call screensaver status > "$ss_dismiss_status_path" 2>&1
$ipc call screensaver start > /dev/null 2>&1
sleep 1
"$grim_bin" "$ss_manual_path" > /dev/null 2>&1
$ipc call screensaver frameInfo > "$ss_cycle_info1_path" 2>&1
while [ "\$SECONDS" -lt $((t0 + 71)) ]; do
  $ipc call screensaver frameInfo > "$ss_cycle_info2_path" 2>&1
  grep -q '"cycles":0}' "$ss_cycle_info2_path" || break
  sleep 1
done
# The converged banner, reached by pinning rather than by waiting: pinning
# frame 0 is what makes a streaming ttfx run's frame count knowable at all
# (screensaver_gif.sh's own header explains why), and the last frame of any
# effect is the banner itself, so this frame is the same picture whichever
# effect the reroll happened to land on. A pin also suspends cycling, so
# nothing rerolls out from under grim.
#
# Unless the run pinned an effect, wipe is set first: a reroll can land on
# one that never finishes inside ttfx's 600-frame pin cap (thunderstorm,
# binarypath), and that run's last counted frame is no banner at all.
if [ -z "${SCREENSAVER_EFFECT:-}" ]; then
  settings_set '.screensaver.effect = "wipe"'
  sleep 1
fi
$ipc call screensaver frame 0 > /dev/null 2>&1
sleep 3
$ipc call screensaver frameInfo > "$ss_solid_info_path" 2>&1
ss_convergence=\$(grep -o '"convergenceFrame":[0-9]*' "$ss_solid_info_path" | cut -d: -f2)
if [ -z "\$ss_convergence" ] || [ "\$ss_convergence" -lt 1 ]; then
  ss_convergence=1
fi
$ipc call screensaver frame \$((ss_convergence - 1)) > /dev/null 2>&1
sleep 3
"$grim_bin" "$ss_solid_path" > /dev/null 2>&1
$ipc call screensaver stop > /dev/null 2>&1
sleep 1
$ipc call screensaver status > "$ss_final_status_path" 2>&1
# screensaver.lockAfterSeconds: two seconds of the screensaver showing chain
# into the lock, which the real password then lifts.
settings_set '.screensaver.lockAfterSeconds = 2'
sleep 1
$ipc call screensaver start > /dev/null 2>&1
sleep 4
$ipc call lock isLocked > "$ss_chain_locked_path" 2>&1
$ipc call screensaver status > "$ss_chain_status_path" 2>&1
"$grim_bin" "$ss_chain_path" > /dev/null 2>&1
$ipc call screensaver stop > /dev/null 2>&1
"$wtype_bin" "formalshell-test"
"$wtype_bin" -k Return
for i in \$(seq 1 40); do
  $ipc call lock isLocked > "$ss_chain_unlocked_path" 2>&1
  grep -q '^false\$' "$ss_chain_unlocked_path" && break
  sleep 0.25
done
EOF
  hypr_exec_once "bash $play_script"
  hypr_exec_once "bash $script"
}

# Reduces the converged-banner frame to a two-colour mask: every pixel within
# fuzz of the surface's own background becomes black, everything the banner
# painted becomes white. The background is read out of the frame's own corner
# rather than assumed, since --wallpaper recolours the theme.
screensaver_write_mask() {
  local bg
  bg=$($convert_bin "$1" -crop 1x1+2+2 +repage txt:- 2>/dev/null \
    | sed -n 's/.*#\([0-9A-Fa-f]\{6\}\).*/\1/p' | head -n1)
  [ -n "$bg" ] || return 1
  # Two fuzz values on purpose: 8% forgives the compositor's own rounding on
  # the background fill, 2% then keeps only near-exact black as background, so
  # a half-covered antialiased pixel counts as ink rather than as a gap.
  $convert_bin "$1" -alpha off -fuzz 8% -fill black -opaque "#$bg" \
    -fuzz 2% -fill white +opaque black "$2" 2>/dev/null
}

# Per-scanline ink, one mean grey per row of the mask: 0 is a row the banner
# did not touch at all.
screensaver_row_profile() {
  $convert_bin "$1" -scale "1x$2!" -depth 8 txt:- 2>/dev/null \
    | sed -n 's/^[0-9]*,\([0-9]*\): (\([0-9]*\).*/\1 \2/p'
}

leg_screensaver_assert() {
  local ss_cycle_effect1 ss_cycle_effect2
  local ss_height ss_width ss_profile ss_first ss_last ss_gap_rows ss_busy ss_run
  # Guard proof: mpv still playing, the 3s idle timeout long since elapsed
  # with no real input anywhere in this session, so isIdle has to be true and
  # the guard has to keep active false anyway.
  if [ ! -s "$ss_guard_status_path" ]; then
    fail "no screensaver guard-status produced"
  fi
  cat "$ss_guard_status_path"; echo
  if ! grep -q '"isIdle":true' "$ss_guard_status_path"; then
    fail "screensaver status did not report isIdle:true while mpv was still playing, got: $(cat "$ss_guard_status_path")"
  fi
  if ! grep -q '"active":false' "$ss_guard_status_path"; then
    fail "screensaver activated despite the media guard while mpv was still playing, got: $(cat "$ss_guard_status_path")"
  fi
  if ! grep -q '"mediaPlaying":true' "$ss_guard_status_path"; then
    fail "screensaver status did not report mediaPlaying:true while mpv was still playing, got: $(cat "$ss_guard_status_path")"
  fi
  # Auto-trigger proof: mpv killed, no `screensaver start` call at all, so the
  # guard clearing alone has to flip active true.
  if [ ! -f "$ss_auto_path" ]; then
    fail "no screensaver-auto screenshot produced"
  fi
  echo "SMOKE_SCREENSAVER_AUTO $ss_auto_path"
  if [ ! -s "$ss_auto_status_path" ]; then
    fail "no screensaver auto-status produced"
  fi
  cat "$ss_auto_status_path"; echo
  if ! grep -q '"active":true' "$ss_auto_status_path"; then
    fail "screensaver did not auto-activate once the media guard cleared, got: $(cat "$ss_auto_status_path")"
  fi
  if [ ! -s "$ss_dismiss_status_path" ] || ! grep -q '"active":false' "$ss_dismiss_status_path"; then
    fail "screensaver stop IPC call did not flip active back to false, got: $(cat "$ss_dismiss_status_path" 2>/dev/null)"
  fi
  # Manual IPC start/stop proof, independent of the idle timer.
  if [ ! -f "$ss_manual_path" ]; then
    fail "no screensaver-manual screenshot produced"
  fi
  echo "SMOKE_SCREENSAVER_MANUAL $ss_manual_path"
  # Continuous cycling proof: the baseline frameInfo has to be cycle 0 of the
  # manual activation, and the read-only poll has to have seen the counter
  # leave 0 with no IPC nudge, the reroll coming purely from the effect
  # converging and the hold elapsing.
  if [ ! -s "$ss_cycle_info1_path" ] || ! grep -q '"cycles":0}' "$ss_cycle_info1_path"; then
    fail "screensaver frameInfo baseline was not cycles:0 right after the manual start, got: $(cat "$ss_cycle_info1_path" 2>/dev/null)"
  fi
  cat "$ss_cycle_info1_path"; echo
  if [ ! -s "$ss_cycle_info2_path" ] || ! grep -q '"cycles":' "$ss_cycle_info2_path" || grep -q '"cycles":0}' "$ss_cycle_info2_path"; then
    fail "screensaver cycles never left 0 within the drive's deadline, got: $(cat "$ss_cycle_info2_path" 2>/dev/null)"
  fi
  cat "$ss_cycle_info2_path"; echo
  ss_cycle_effect1=$(grep -o '"effect":"[a-z]*"' "$ss_cycle_info1_path")
  ss_cycle_effect2=$(grep -o '"effect":"[a-z]*"' "$ss_cycle_info2_path")
  if [ -n "${SCREENSAVER_EFFECT:-}" ]; then
    # A pinned effect has to replay itself across the reroll.
    if [ "$ss_cycle_effect1" != "$ss_cycle_effect2" ]; then
      fail "pinned screensaver effect changed across the reroll ($ss_cycle_effect1 -> $ss_cycle_effect2)"
    fi
  else
    # The default "random" must never repeat the immediately previous effect,
    # so cycle 1's report has to differ from cycle 0's.
    if [ "$ss_cycle_effect1" = "$ss_cycle_effect2" ]; then
      fail "random screensaver reroll repeated the previous effect ($ss_cycle_effect1)"
    fi
  fi
  echo "SMOKE_SCREENSAVER_CYCLE $ss_cycle_effect1 -> $ss_cycle_effect2"
  # Solidity (M48): the banner is block characters, and a block character has
  # to fill its whole cell. It stopped doing that when the shell's monospace
  # alias moved to Geist Mono, whose U+2588 inks 1.30em against a cell the
  # painter had padded to 1.495em, so a 6px background stripe ran through
  # every row of a supposedly solid banner. The painter draws the block
  # elements as rectangles on the cell grid now (shell/Screensaver/blocks.js),
  # which is font-independent, and this is the claim: inside the banner's own
  # vertical span there is no scanline the banner failed to touch. A stripe
  # between two rows of solid blocks is exactly such a scanline, so the
  # regression cannot come back quietly.
  if [ ! -s "$ss_solid_info_path" ] || ! grep -q '"convergenceFrame":' "$ss_solid_info_path"; then
    fail "no screensaver frameInfo for the pinned converged frame, got: $(cat "$ss_solid_info_path" 2>/dev/null)"
  fi
  cat "$ss_solid_info_path"; echo
  [ -f "$ss_solid_path" ] || fail "no converged-banner screenshot produced"
  ss_height=$($convert_bin "$ss_solid_path" -format '%h' info: 2>/dev/null)
  ss_width=$($convert_bin "$ss_solid_path" -format '%w' info: 2>/dev/null)
  if [ -z "$ss_height" ] || [ -z "$ss_width" ]; then
    fail "could not read the converged-banner screenshot's size"
  fi
  screensaver_write_mask "$ss_solid_path" "$ss_solid_mask_path" \
    || fail "could not reduce the converged banner to an ink mask"
  ss_profile=$(screensaver_row_profile "$ss_solid_mask_path" "$ss_height")
  if [ -z "$ss_profile" ]; then
    fail "could not read a row profile out of the converged-banner mask"
  fi
  ss_first=$(printf '%s\n' "$ss_profile" | awk '$2 > 0 { print $1; exit }')
  ss_last=$(printf '%s\n' "$ss_profile" | awk '$2 > 0 { row = $1 } END { print row }')
  if [ -z "$ss_first" ] || [ -z "$ss_last" ] || [ "$ss_last" -le "$ss_first" ]; then
    fail "the converged banner has no vertical extent at all, so nothing rendered"
  fi
  ss_gap_rows=$(printf '%s\n' "$ss_profile" \
    | awk -v a="$ss_first" -v b="$ss_last" '$1 >= a && $1 <= b && $2 == 0 { n++ } END { print n + 0 }')
  if [ "$ss_gap_rows" -ne 0 ]; then
    fail "the converged banner carries $ss_gap_rows background-only scanline(s) between rows $ss_first and $ss_last, so the block cells are still not tiling"
  fi
  # The horizontal half of the same claim, on the row the banner inked most:
  # its longest unbroken run of ink, which for a banner whose widest solid
  # stretch is seven full blocks has to be several cells wide rather than one.
  ss_busy=$(printf '%s\n' "$ss_profile" | awk '$2 > best { best = $2; row = $1 } END { print row }')
  ss_run=$($convert_bin "$ss_solid_mask_path" -crop "${ss_width}x1+0+${ss_busy}" +repage txt:- 2>/dev/null \
    | sed -n 's/.*#\([0-9A-Fa-f]\{6\}\).*/\1/p' \
    | awk 'toupper($0) == "FFFFFF" { run++; if (run > best) best = run; next } { run = 0 } END { print best + 0 }')
  if [ "${ss_run:-0}" -lt 80 ]; then
    fail "the converged banner's busiest scanline (row $ss_busy) breaks after ${ss_run:-0}px of ink, so its block cells are separated"
  fi
  echo "SMOKE_SCREENSAVER_SOLID $ss_gap_rows background scanlines across rows $ss_first-$ss_last, longest unbroken ink run ${ss_run}px on row $ss_busy"
  echo "SMOKE_SCREENSAVER_BANNER $ss_solid_path"
  if [ ! -s "$ss_final_status_path" ] || ! grep -q '"active":false' "$ss_final_status_path"; then
    fail "screensaver did not report active:false after the final stop, got: $(cat "$ss_final_status_path" 2>/dev/null)"
  fi
  if ! grep -q '^true$' "$ss_chain_locked_path" 2>/dev/null || ! grep -q '"active":true' "$ss_chain_status_path" 2>/dev/null; then
    fail "screensaver.lockAfterSeconds did not chain the showing screensaver into the lock, isLocked: $(cat "$ss_chain_locked_path" 2>/dev/null), status: $(cat "$ss_chain_status_path" 2>/dev/null)"
  fi
  echo "SMOKE_SCREENSAVER_CHAIN $ss_chain_path"
  if ! grep -q '^false$' "$ss_chain_unlocked_path" 2>/dev/null; then
    fail "the chained lock did not lift for the real password, got: $(cat "$ss_chain_unlocked_path" 2>/dev/null)"
  fi
}
