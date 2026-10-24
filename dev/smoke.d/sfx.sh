# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --sfx: the table's blips (spec C10) on fs-audio's own playback stream,
# heard off a null sink of the leg's own made the default, its monitor
# recorded with parec for the whole drive. Real keys through the launcher:
# an open, two Downs, two letters, an Escape clearing them, an Escape
# closing, an input step opened and a Return submitting it, nine blips;
# then the same open, Down and Escape under do-not-disturb, none. The
# shell's `sfx <event>` lines are counted per phase, the recording is cut
# into bursts at its silences and each burst's peak and RMS printed, and
# `pactl list sink-inputs` is read twice: the stream (`formalshell-sfx`)
# up just after a blip, and gone 3s after the last, so a quiet shell holds
# no stream.
#
# Under --cartridge every count above is asserted. Without the rider the
# table has no `sounds`, so the same drive must log no blip and record
# silence.
leg_sfx_flag="--sfx"
leg_sfx_order=226
leg_sfx_needs="wtype pactl parec python3"

need_parec() {
  if [ -z "${parec_bin:-}" ]; then
    if command -v parec >/dev/null 2>&1; then
      parec_bin=$(command -v parec)
    else
      parec_bin=$(nix build --no-link --print-out-paths 'nixpkgs#pulseaudio^out')/bin/parec
    fi
  fi
}

sfx_sink=fs_sfx_sink
sfx_raw_path="$shot_dir/sfx-capture.raw"
sfx_counts_path="$shot_dir/sfx-counts.txt"
sfx_inputs_during_path="$shot_dir/sfx-inputs-during.txt"
sfx_inputs_after_path="$shot_dir/sfx-inputs-after.txt"
sfx_module_path="$shot_dir/sfx-module.txt"
sfx_default_path="$shot_dir/sfx-default-before.txt"
sfx_parec_pid_path="$shot_dir/sfx-parec.pid"
sfx_done_path="$shot_dir/sfx-done"
sfx_ticks_path="$shot_dir/sfx-ticks.txt"

leg_sfx_timing() {
  leg_timing 34 60
}

leg_sfx_drive() {
  local script="$shot_dir/sfx-drive.sh" cleanup="$shot_dir/sfx-cleanup.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
ipc() { $ipc call "\$@" > /dev/null 2>&1; }
count() { grep -c ' sfx [a-z]*\$' "$shell_log_path" 2>/dev/null || true; }
key() { "$wtype_bin" "\$@"; sleep 0.7; }
# utime + stime of the PipeWire client thread and of the whole shell.
ticks() {
  local t
  for t in /proc/[0-9]*/task/*; do
    [ "\$(cat "\$t/comm" 2>/dev/null)" = fs-pipewire ] || continue
    local pid=\${t#/proc/}; pid=\${pid%%/*}
    echo "\$(awk '{print \$14 + \$15}' "\$t/stat") \$(awk '{print \$14 + \$15}' "/proc/\$pid/stat")"
    return
  done
  echo "0 0"
}
sleep 4
"$pactl_bin" get-default-sink > "$sfx_default_path" 2>&1
"$pactl_bin" load-module module-null-sink sink_name=$sfx_sink sink_properties=device.description=SfxSink > "$sfx_module_path"
"$pactl_bin" set-default-sink $sfx_sink
sleep 1
"$parec_bin" -d $sfx_sink.monitor --raw --format=s16le --rate=48000 --channels=1 > "$sfx_raw_path" &
echo \$! > "$sfx_parec_pid_path"
sleep 1
start=\$(count)
ticks > "$sfx_ticks_path"
ipc menu toggle
sleep 1.2
"$wtype_bin" -k Down
sleep 0.3
"$pactl_bin" list sink-inputs > "$sfx_inputs_during_path" 2>&1
sleep 0.4
key -k Down
key w
key i
key -k Escape
key -k Escape
sleep 0.6
ipc menu input "Sfx check" sfx-check
sleep 1.2
key -k Return
sleep 0.6
played=\$(count)
ticks >> "$sfx_ticks_path"
ipc notifications setDnd true
sleep 0.5
ipc menu toggle
sleep 1.2
key -k Down
key -k Escape
sleep 0.6
ipc notifications setDnd false
dnd=\$(count)
sleep 3
"$pactl_bin" list sink-inputs > "$sfx_inputs_after_path" 2>&1
echo "\$start \$played \$dnd" > "$sfx_counts_path"
kill "\$(cat "$sfx_parec_pid_path")" 2>/dev/null || true
touch "$sfx_done_path"
EOF
  write_script "$cleanup" <<EOF
#!/usr/bin/env bash
[ -f "$sfx_parec_pid_path" ] && kill "\$(cat "$sfx_parec_pid_path")" 2>/dev/null
[ -s "$sfx_default_path" ] && "$pactl_bin" set-default-sink "\$(cat "$sfx_default_path")" 2>/dev/null
[ -s "$sfx_module_path" ] && "$pactl_bin" unload-module "\$(cat "$sfx_module_path")" 2>/dev/null
true
EOF
  add_cleanup "bash $cleanup"
  hypr_exec_once "bash $script"
}

# Bursts in the recording: 10ms windows over a floor of 30 (about -61
# dBFS), merged across gaps under 150ms. One line per burst.
sfx_bursts() {
  "$python3_bin" - "$sfx_raw_path" <<'PY'
import math, struct, sys
data = open(sys.argv[1], "rb").read()
pcm = struct.unpack(f"<{len(data) // 2}h", data[: len(data) // 2 * 2])
win, floor, gap = 480, 30, 15
loud = [max((abs(s) for s in pcm[i : i + win]), default=0) > floor for i in range(0, len(pcm), win)]
bursts, start, quiet = [], None, 0
for i, on in enumerate(loud + [False] * gap):
    if on:
        start = i if start is None else start
        quiet = 0
    elif start is not None:
        quiet += 1
        if quiet >= gap:
            bursts.append((start, i - quiet + 1))
            start, quiet = None, 0
db = lambda x: 20 * math.log10(x / 32768) if x > 0 else -120.0
for a, b in bursts:
    seg = pcm[a * win : b * win]
    peak = max(abs(s) for s in seg)
    rms = math.sqrt(sum(s * s for s in seg) / len(seg))
    print(f"{a * 10}ms {(b - a) * 10}ms peak {db(peak):.1f} dBFS rms {db(rms):.1f} dBFS")
PY
}

leg_sfx_assert() {
  [ -f "$sfx_done_path" ] || fail "the sfx drive never finished"
  read -r start played dnd < "$sfx_counts_path"
  local bursts n
  bursts=$(sfx_bursts)
  n=$(printf '%s' "$bursts" | grep -c dBFS || true)
  [ -z "$bursts" ] || printf '%s\n' "$bursts" | sed 's/^/SMOKE_SFX_BURST /'
  echo "SMOKE_SFX_COUNTS before=$start played=$((played - start)) dnd=$((dnd - played)) bursts=$n"
  echo "SMOKE_SFX_TICKS $(awk 'NR==1{p=$1;s=$2} NR==2{print "pipewire=" $1-p " shell=" $2-s}' "$sfx_ticks_path") over the nine"
  echo "SMOKE_SFX_LINES $(grep ' sfx [a-z]*$' "$shell_log_path" | sed 's/.* sfx //' | tr '\n' ' ')"
  if leg_on cartridge; then
    [ "$((played - start))" -eq 9 ] || fail "expected 9 blips through the launcher, the shell logged $((played - start))"
    [ "$((dnd - played))" -eq 0 ] || fail "do-not-disturb let $((dnd - played)) blips through"
    [ "$n" -eq 9 ] || fail "expected 9 bursts on the null sink's monitor, heard $n"
    grep -q 'formalshell-sfx' "$sfx_inputs_during_path" || fail "no formalshell-sfx sink input just after a blip: $(cat "$sfx_inputs_during_path")"
  else
    [ "$dnd" -eq 0 ] || fail "a table with no sounds logged $dnd blips"
    [ "$n" -eq 0 ] || fail "a table with no sounds put $n bursts on the sink"
  fi
  if grep -q 'formalshell-sfx' "$sfx_inputs_after_path"; then
    fail "the blip stream is still up 3s after the last blip: $(cat "$sfx_inputs_after_path")"
  fi
  echo "SMOKE_SFX $sfx_raw_path"
}
