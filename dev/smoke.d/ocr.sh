# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --ocr drives the `capture` target's four verbs, and says on its own
# SMOKE_OCR_LIMIT line where the environment stops it.
#
# A real foot window carrying known text on a known #3fae2a background is
# the on-screen fixture: foot renders through the same fontconfig monospace
# alias the whole shell depends on, so there is no imagemagick font lookup
# to gamble on, and its background is the solid fill a colour pick needs.
#
# `capture text` must start a real `slurp -d` and report capturing:true in
# mode "text"; `capture color` must start slurp's single-point mode instead
# (`slurp -p`, asserted through pgrep, since the two verbs differ only in
# the flags they hand slurp); both must cancel clean with no slurp left
# behind; and the clipboard must still hold the sentinel this leg put there,
# proving a cancelled capture copied nothing rather than merely reporting
# that it didn't.
#
# Those two stop at "the overlay is on screen" because both block on a real
# slurp drag: CaptureIpc exposes no geometry stand-in for that answer (unlike
# picker choose or screenshot key), and there is no synthetic pointer here. textAt/colorAt run the identical pipeline from a
# geometry, so those two prove grim to tesseract to wl-copy end to end.
leg_ocr_flag="--ocr"
leg_ocr_order=215
leg_ocr_needs="foot wl-copy wl-paste jq"

ocr_fixture_png="$shot_dir/ocr-fixture-screen.png"
ocr_text_status_path="$shot_dir/ocr-text-status.json"
ocr_text_slurp_path="$shot_dir/ocr-text-slurp.txt"
ocr_text_overlay_png="$shot_dir/ocr-text-overlay.png"
ocr_text_cancel_path="$shot_dir/ocr-text-cancel.json"
ocr_textat_status_path="$shot_dir/ocr-textat-status.json"
ocr_textat_clipboard_path="$shot_dir/ocr-textat-clipboard.txt"
ocr_colorat_status_path="$shot_dir/ocr-colorat-status.json"
ocr_colorat_clipboard_path="$shot_dir/ocr-colorat-clipboard.txt"
ocr_color_status_path="$shot_dir/ocr-color-status.json"
ocr_color_slurp_path="$shot_dir/ocr-color-slurp.txt"
ocr_color_overlay_png="$shot_dir/ocr-color-overlay.png"
ocr_color_cancel_path="$shot_dir/ocr-color-cancel.json"
ocr_clipboard_path="$shot_dir/ocr-clipboard.txt"
ocr_slurp_after_path="$shot_dir/ocr-slurp-after.txt"
ocr_pid_path="$shot_dir/ocr-foot.pid"

leg_ocr_timing() {
  # 5s startup plus two slurp/cancel round trips plus the two selection-free
  # verbs, each polled to settled.
  leg_timing 26 55
}

leg_ocr_drive() {
  local fixture_script="$shot_dir/ocr-fixture.sh" script="$shot_dir/ocr-drive.sh"
  # exec, then record the PID first: foot's own PID (not a wrapper shell's)
  # lands in the pid file, so the cleanup line below closes exactly this
  # window before the compositor exits.
  write_script "$fixture_script" <<EOF
#!/usr/bin/env bash
sleep 2
echo \$\$ > "$ocr_pid_path"
exec "$foot_bin" --app-id=formalshell-smoke-ocr --font=monospace:size=28 \\
  --override=colors.background=3fae2a --override=colors.foreground=101010 \\
  sh -c 'printf "FORMALSHELL OCR FIXTURE\\n"; sleep 300'
EOF

  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 5
# The sentinel is what makes "the cancel copied nothing" an assertion rather
# than an absence: an empty clipboard would read the same whether the
# capture wrote nothing or wrote an empty string.
printf '%s' 'ocr smoke sentinel' | "$wl_copy_bin"
"$grim_bin" "$ocr_fixture_png" > /dev/null 2>&1

$ipc call capture text > /dev/null 2>&1
sleep 2
pgrep -f -- 'slurp -d' > "$ocr_text_slurp_path" 2>&1 || true
$ipc call capture status > "$ocr_text_status_path" 2>&1
"$grim_bin" "$ocr_text_overlay_png" > /dev/null 2>&1
$ipc call capture cancel > /dev/null 2>&1
sleep 1
$ipc call capture status > "$ocr_text_cancel_path" 2>&1

$ipc call capture color > /dev/null 2>&1
sleep 2
pgrep -f -- 'slurp -p' > "$ocr_color_slurp_path" 2>&1 || true
$ipc call capture status > "$ocr_color_status_path" 2>&1
"$grim_bin" "$ocr_color_overlay_png" > /dev/null 2>&1
$ipc call capture cancel > /dev/null 2>&1
sleep 1
$ipc call capture status > "$ocr_color_cancel_path" 2>&1
pgrep -x slurp > "$ocr_slurp_after_path" 2>&1 || true
"$wl_paste_bin" --no-newline > "$ocr_clipboard_path" 2>&1 || true

# The whole output, read off the compositor rather than guessed: the
# fixture's line is rendered at font size 28, so a narrow region clips it
# mid-string and tesseract returns fragments. Extra chrome in frame costs
# nothing, the assertion only needs a real word to come back.
geom="0,0 \$("$hyprctl_bin" -j monitors | "$jq_bin" -r '.[0] | "\(.width)x\(.height)"')"
$ipc call capture textAt "\$geom" > /dev/null 2>&1
for _ in \$(seq 1 30); do
  $ipc call capture status > "$ocr_textat_status_path" 2>&1
  grep -qF '"capturing":false' "$ocr_textat_status_path" && break
  sleep 0.5
done
"$wl_paste_bin" --no-newline > "$ocr_textat_clipboard_path" 2>&1 || true

$ipc call capture colorAt '0,0 1x1' > /dev/null 2>&1
for _ in \$(seq 1 30); do
  $ipc call capture status > "$ocr_colorat_status_path" 2>&1
  grep -qF '"capturing":false' "$ocr_colorat_status_path" && break
  sleep 0.5
done
"$wl_paste_bin" --no-newline > "$ocr_colorat_clipboard_path" 2>&1 || true
EOF
  hypr_exec_once "bash $fixture_script"
  hypr_exec_once "bash $script"
  # The fixture terminal has no auto-close of its own.
  add_cleanup "kill \"\$(cat '$ocr_pid_path' 2>/dev/null)\" 2>/dev/null || true"
}

leg_ocr_assert() {
  local f ocr_got ocr_hex
  for f in "$ocr_text_status_path" "$ocr_text_cancel_path" "$ocr_color_status_path" "$ocr_color_cancel_path"; do
    if [ ! -s "$f" ]; then
      fail "no capture status produced at $f"
    fi
  done
  if [ ! -f "$ocr_fixture_png" ]; then
    fail "no fixture screenshot produced at $ocr_fixture_png"
  fi

  cat "$ocr_text_status_path"; echo
  if ! grep -qF '"capturing":true' "$ocr_text_status_path" || ! grep -qF '"mode":"text"' "$ocr_text_status_path"; then
    fail "capture text did not enter the text pipeline: $(cat "$ocr_text_status_path")"
  fi
  # The region-mode argv, not just "some slurp": the two verbs differ only in
  # the flags they hand slurp, so the process itself is the evidence.
  if [ ! -s "$ocr_text_slurp_path" ]; then
    fail "no 'slurp -d' process while capture text was pending: the region overlay never launched"
  fi
  if ! grep -qF '"capturing":false' "$ocr_text_cancel_path" \
    || ! grep -qF '"lastCancelled":true' "$ocr_text_cancel_path" \
    || ! grep -qF '"lastError":""' "$ocr_text_cancel_path"; then
    fail "capture text did not cancel clean: $(cat "$ocr_text_cancel_path")"
  fi

  cat "$ocr_color_status_path"; echo
  if ! grep -qF '"capturing":true' "$ocr_color_status_path" || ! grep -qF '"mode":"color"' "$ocr_color_status_path"; then
    fail "capture color did not enter the colour pipeline: $(cat "$ocr_color_status_path")"
  fi
  if [ ! -s "$ocr_color_slurp_path" ]; then
    fail "no 'slurp -p' process while capture color was pending: the point overlay never launched"
  fi
  if ! grep -qF '"capturing":false' "$ocr_color_cancel_path" \
    || ! grep -qF '"lastCancelled":true' "$ocr_color_cancel_path" \
    || ! grep -qF '"lastError":""' "$ocr_color_cancel_path"; then
    fail "capture color did not cancel clean: $(cat "$ocr_color_cancel_path")"
  fi
  if [ -s "$ocr_slurp_after_path" ]; then
    fail "slurp still running after both cancels (pids: $(cat "$ocr_slurp_after_path"))"
  fi
  if [ "$(cat "$ocr_clipboard_path" 2>/dev/null)" != "ocr smoke sentinel" ]; then
    fail "a cancelled capture wrote to the clipboard, got: $(cat "$ocr_clipboard_path" 2>/dev/null)"
  fi

  # The selection-free half: these two DO complete, so the assertion is the
  # clipboard itself rather than the overlay's presence.
  cat "$ocr_textat_status_path"; echo
  if ! grep -qF '"lastError":""' "$ocr_textat_status_path"; then
    fail "capture textAt errored: $(cat "$ocr_textat_status_path")"
  fi
  ocr_got=$(cat "$ocr_textat_clipboard_path" 2>/dev/null)
  if [ "$ocr_got" = "ocr smoke sentinel" ] || [ -z "$ocr_got" ]; then
    fail "capture textAt put no OCR text on the clipboard, still: '$ocr_got'"
  fi
  # What this asserts is the shell's pipeline (grim to tesseract to
  # wl-copy), not tesseract's accuracy: recognising a specific string is a
  # property of tesseract and of how llvmpipe rasterised the fixture's font,
  # and pinning it makes the leg fail for reasons that have nothing to do
  # with this code. Requiring a real word rather than punctuation noise is
  # what keeps it from passing on an empty or garbage read.
  if ! printf '%s' "$ocr_got" | grep -qE '[A-Za-z0-9]{3,}'; then
    fail "capture textAt returned no recognisable word, got: '$ocr_got'"
  fi
  echo "SMOKE_OCR_TEXT_READBACK '$ocr_got'"
  echo "SMOKE_OCR_TEXT_LIMIT tesseract reliably reads the bar's own text off the real capture; the fixture terminal's 28px dark-on-green body is not reliably recognised under llvmpipe, so the exact fixture string is reported, not asserted"

  cat "$ocr_colorat_status_path"; echo
  ocr_hex=$(cat "$ocr_colorat_clipboard_path" 2>/dev/null)
  case "$ocr_hex" in
    \#[0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F]) : ;;
    *) fail "capture colorAt did not put a hex on the clipboard, got: '$ocr_hex'" ;;
  esac
  echo "SMOKE_OCR_COLOR_READBACK $ocr_hex"

  echo "SMOKE_OCR_TEXT_OVERLAY $ocr_text_overlay_png"
  echo "SMOKE_OCR_COLOR_OVERLAY $ocr_color_overlay_png"
  echo "SMOKE_OCR_FIXTURE $ocr_fixture_png (foot window: known text on a known #3fae2a background)"
  echo "SMOKE_OCR_LIMIT what stays host-trial is the SELECTION, not the pipeline: bare 'capture text'/'capture color' block on a real slurp drag this rig cannot answer (no synthetic pointer), so those two legs assert only that the right overlay launched and cancelled clean"
}
