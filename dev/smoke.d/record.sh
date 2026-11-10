# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --record drives the `record` target through a real wf-recorder child, start
# to finished GIF. `record start screen desktop` answers with the destination
# path synchronously, `record status` has to report active:true against that
# exact path, and the bar is screenshotted mid recording (record-active.png),
# the only state the recording cell exists in. `record stop`
# then polls until `active` goes false (SIGTERM asks wf-recorder to finalize
# the container rather than truncate it) and until `finalizing` settles too
# (RecordingService's own trim/loudnorm pass), so the ffprobe read lands on
# a settled file rather than one mid-rewrite: it must show a positive
# duration and an audio stream. Finally `record gif <path>` runs the two-pass
# transcode and the result has to be a real GIF matching status's own
# lastGifPath.
#
# settings.json sets recording.noDmabuf, the one key software rendering makes
# non-optional: there is no GPU buffer to import in this session. Every other
# recording key stays at its default so this proves those defaults resolve,
# which is why the assertions read the destination back out of `record
# status` rather than off a path this leg chose. `desktop` audio needs only
# the VM's own pipewire null-sink monitor, so the finalize pass has a real
# audio track to normalize. webcam is on to exercise the overlay path: on a
# machine with no /dev/video* the honest WEBCAM UNAVAILABLE leg, and on
# g815, whose slot users can open its cameras, a real overlay mapped and
# placed before wf-recorder starts, up to seven seconds in, so the leg
# waits for `active` rather than reading it at a fixed second, and fails
# on an overlay the shell never saw settle.
leg_record_flag="--record"
leg_record_order=210
leg_record_claims="recording.noDmabuf=true recording.webcam=true"
leg_record_needs="file ffprobe jq"

record_start_reply_path="$shot_dir/record-start-reply.txt"
record_status1_path="$shot_dir/record-status-1.json"
record_status2_path="$shot_dir/record-status-2.json"
record_status3_path="$shot_dir/record-status-3.json"
record_stop_reply_path="$shot_dir/record-stop-reply.txt"
record_gif_reply_path="$shot_dir/record-gif-reply.txt"
record_active_path="$shot_dir/record-active.png"
record_finalize_status_path="$shot_dir/record-finalize-status.json"
record_ffprobe_path="$shot_dir/record-ffprobe.txt"

leg_record_fixture() {
  settings_pin '.recording.noDmabuf = true | .recording.webcam = true'
}

leg_record_timing() {
  # The drive script's worst case: 4s startup, a 15s start ceiling, 4s
  # recording, 2s, a 15s stop-settle ceiling and a 40s transcode ceiling.
  # The typical path is closer to 20s, since both ceilings are polls that break the moment the
  # state settles.
  leg_timing 110
}

leg_record_drive() {
  local script="$shot_dir/record-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
$ipc call record start screen desktop > "$record_start_reply_path" 2>&1
until_jq 15 "$record_status1_path" '.active == true' call record status
# Long enough that the container holds real frames rather than a header: the
# GIF transcode below has to have something to read.
sleep 4
$ipc call record status > "$record_status1_path" 2>&1
"$grim_bin" "$record_active_path" > /dev/null 2>&1
sleep 2
$ipc call record stop > "$record_stop_reply_path" 2>&1
until_jq 15 "$record_status2_path" '.active == false' call record status
# wf-recorder has already exited here, but RecordingService's finalize pass
# (two ffprobes plus one ffmpeg encode) still owns the file; waiting on the
# flag it exposes for exactly this is what makes the ffprobe read below land
# on the settled file.
until_jq 20 "$record_finalize_status_path" '.finalizing == false' call record status
rec_file=\$(head -n1 "$record_start_reply_path" | tr -d '\r')
"$ffprobe_bin" -v error -show_entries format=duration:stream=codec_type \\
  -of default=noprint_wrappers=1 "\$rec_file" > "$record_ffprobe_path" 2>&1
$ipc call record gif "\$rec_file" > "$record_gif_reply_path" 2>&1
# A populated lastError is a settled answer too, and waiting out the
# ceiling for it would only delay the failure below.
until_jq 40 "$record_status3_path" '(.transcoding == false and ((.lastGifPath // "") | startswith("/"))) or .lastError != ""' call record status || true
EOF
  hypr_exec_once "bash $script"
}

leg_record_assert() {
  local f record_file finalize_duration record_gif_file
  for f in "$record_start_reply_path" "$record_stop_reply_path" "$record_gif_reply_path" \
    "$record_status1_path" "$record_status2_path" "$record_status3_path" \
    "$record_finalize_status_path" "$record_ffprobe_path"; do
    if [ ! -s "$f" ]; then
      fail "no record artifact produced at $f"
    fi
  done
  cat "$record_start_reply_path"; echo
  record_file=$(head -n1 "$record_start_reply_path" | tr -d '\r')
  case "$record_file" in
    error*|"") fail "record start replied with an error: $(cat "$record_start_reply_path")" ;;
  esac
  # RecordingService builds the destination from the answering instance's own
  # $HOME (recording.directory is left at its default here), so a path
  # outside this run's isolated HOME names a different formalshell on this
  # machine and record-active.png is a photo of a session that was never
  # asked to record.
  case "$record_file" in
    "$iso_home"/*) ;;
    *) fail "record start was answered by an instance outside this run: $record_file is not under $iso_home" ;;
  esac

  # The one failure this leg must never soften: wf-recorder either captured
  # or it did not, and its own stderr is what says why.
  cat "$record_status1_path"; echo
  if ! "$jq_bin" -e '.active == true' "$record_status1_path" > /dev/null 2>&1; then
    echo "SMOKE_FAIL: wf-recorder stderr: $("$jq_bin" -r '.lastError // empty' "$record_status1_path" 2>/dev/null)" >&2
    fail "wf-recorder never started, record status: $(cat "$record_status1_path")"
  fi
  expect_jq "$record_status1_path" ".path == \"$record_file\"" "record status reported a different destination than start replied ($record_file)"
  if [ ! -f "$record_active_path" ]; then
    fail "no recording-indicator screenshot produced at $record_active_path"
  fi
  # Placed or honestly unavailable: a mapped camera the shell never saw
  # reach its rect is the overlay left wherever it landed.
  if grep -q 'webcam overlay did not settle' "$shell_log_path"; then
    fail "the webcam overlay was placed but never seen to settle: $(grep 'RecordingService' "$shell_log_path" | tr '\n' ' ')"
  fi

  if ! grep -q '^ok$' "$record_stop_reply_path"; then
    fail "record stop did not return ok. Got: $(cat "$record_stop_reply_path" 2>/dev/null)"
  fi
  cat "$record_status2_path"; echo
  expect_jq "$record_status2_path" '.active == false and .lastError == ""' "recording did not settle clean after stop"

  cat "$record_finalize_status_path"; echo
  expect_jq "$record_finalize_status_path" '.finalizing == false' "finalize never settled"
  cat "$record_ffprobe_path"; echo
  if ! grep -qF "codec_type=audio" "$record_ffprobe_path"; then
    fail "finalized recording lost its audio stream: $(cat "$record_ffprobe_path")"
  fi
  finalize_duration=$(sed -n 's/^duration=//p' "$record_ffprobe_path" | head -n1)
  case "$finalize_duration" in
    ''|N/A) fail "finalized recording reports no duration: $(cat "$record_ffprobe_path")" ;;
  esac
  echo "SMOKE_RECORD_FINALIZE duration=${finalize_duration}s"

  if [ ! -s "$record_file" ]; then
    fail "recording file is missing or empty: $record_file"
  fi
  if ! "$file_bin" "$record_file" | grep -qE 'ISO Media|MP4'; then
    fail "recording is not an MP4 container, file(1) says: $("$file_bin" -b "$record_file")"
  fi
  echo "SMOKE_RECORD $record_file ($(wc -c < "$record_file" | tr -d ' ') bytes, $("$file_bin" -b "$record_file"))"

  record_gif_file=$(head -n1 "$record_gif_reply_path" | tr -d '\r')
  case "$record_gif_file" in
    error*|"") fail "record gif replied with an error: $(cat "$record_gif_reply_path" 2>/dev/null)" ;;
  esac
  cat "$record_status3_path"; echo
  if [ ! -s "$record_gif_file" ]; then
    fail "GIF is missing or empty at $record_gif_file, record status: $(cat "$record_status3_path")"
  fi
  if ! "$file_bin" "$record_gif_file" | grep -q "GIF image data"; then
    fail "transcode is not a GIF, file(1) says: $("$file_bin" -b "$record_gif_file")"
  fi
  expect_jq "$record_status3_path" ".lastGifPath == \"$record_gif_file\" and .transcoding == false and .lastError == \"\"" \
    "transcode did not settle clean"
  echo "SMOKE_RECORD_GIF $record_gif_file ($(wc -c < "$record_gif_file" | tr -d ' ') bytes)"
  echo "SMOKE_RECORD_INDICATOR $record_active_path"
}
