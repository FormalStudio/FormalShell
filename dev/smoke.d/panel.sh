# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --panel <name> opens one per-widget popout over the real `panel` IPC route
# and leaves it open for the run's own frame. No bar-cell click happened, so
# the panel's anchorX stays unset and the frame falls back under the bar's
# right region. `panel state` is read back beside the route's own answer:
# the claim a screenshot cannot make is that the shell's own bookkeeping
# says this panel, and only this one, is the open one.
#
# One frame against `debug dump`'s rest rect through the shared
# panel-language assertion (dev/panel-language.py): the header centred, the
# text column clear on both sides, the body ending panelInset above the
# edge, one inset level. A headerless card (the tray's second bar) is read
# at panelRowInset with no header claim.
leg_panel_flag="--panel <name>"
leg_panel_needs="jq grim python3"
leg_panel_order=60

panel_open_path="$shot_dir/panel-open.txt"
panel_state_path="$shot_dir/panel-state.txt"
panel_dump_path="$shot_dir/panel-dump.json"
panel_header_shot="$shot_dir/panel-header.png"
panel_audio_modules="$shot_dir/panel-audio-modules.txt"

leg_panel_validate() {
  if [ -z "$(leg_arg panel)" ]; then
    echo "usage: $0 --panel <name>" >&2
    exit 1
  fi
}

leg_panel_fixture() {
  # --panel systemupdate needs a real flake to read: the panel parses
  # flake.lock itself (no nix invocation), so pointing it at the fixture
  # flake renders the actual input rows instead of the honest NO FLAKE cell.
  # Only that leg gets the key, so every other run keeps proving the unset
  # default. The upstream probes are the one part that needs network; a VM
  # with none lands on NO NETWORK, which is an honest state and still shows
  # the rows.
  if [ "$(leg_arg panel)" = "systemupdate" ]; then
    settings_fragment ', "systemUpdate": {"flakeDir": "'"$PWD/dev/smoke.d/fixtures/flake"'"}'
  fi
}

leg_panel_drive() {
  local panel_name script="$shot_dir/panel-drive.sh" stage=""
  panel_name=$(leg_arg panel)
  # --panel audio photographs a real choice: a second null sink and a
  # virtual source beside the session's own, where pactl reaches PipeWire.
  if [ "$panel_name" = audio ] && command -v pactl > /dev/null; then
    stage="pactl load-module module-null-sink sink_name=fs_smoke_speakers sink_properties=device.description=Speakers >> '$panel_audio_modules'
pactl load-module module-null-sink sink_name=fs_smoke_mic media.class=Audio/Source/Virtual sink_properties=device.description=Microphone >> '$panel_audio_modules'"
    add_cleanup "[ -s '$panel_audio_modules' ] && while read -r m; do pactl unload-module \"\$m\" 2>/dev/null; done < '$panel_audio_modules'; true"
  fi
  write_script "$script" <<EOF
#!/usr/bin/env bash
$stage
sleep 3
$ipc call panel open "$panel_name" > "$panel_open_path" 2>&1
sleep 2
$ipc call panel state > "$panel_state_path" 2>&1
$ipc call debug dump > "$panel_dump_path" 2>&1
"$grim_bin" "$panel_header_shot" > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_panel_assert() {
  local panel_name
  panel_name=$(leg_arg panel)
  if [ ! -s "$panel_open_path" ] || ! grep -q "^ok$" "$panel_open_path"; then
    fail "panel open $panel_name did not answer ok, got: $(cat "$panel_open_path" 2>/dev/null)"
  fi
  cat "$panel_open_path"
  if [ ! -s "$panel_state_path" ] || ! grep -q "^$panel_name$" "$panel_state_path"; then
    fail "panel state is not '$panel_name', got: $(cat "$panel_state_path" 2>/dev/null)"
  fi
  cat "$panel_state_path"
  panel_language_check "$panel_name"
}

panel_language_check() {
  local rest header=no space=rowInset
  rest=$("$jq_bin" -c '.panel.rest' "$panel_dump_path" 2>/dev/null)
  if [ -z "$rest" ] || [ "$rest" = null ]; then
    echo "panel: $1 is not a card in the panel host (Radio Atlas draws its own), skipped"
    return
  fi
  if [ "$("$jq_bin" -r '.panel.header' "$panel_dump_path")" = true ]; then
    header=yes space=inset
  fi
  echo "SMOKE_PANEL_DUMP $panel_dump_path"
  panel_language_assert "$1" "$panel_header_shot" "$rest" "$header" "$panel_dump_path" "$space"
  panel_language_verdict
}
