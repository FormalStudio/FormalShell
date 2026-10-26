# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --hdr: HDR on an output that cannot do it. The rig's vkms card carries no
# EDID, so the honest answers are the whole contract here: `hdr status` says
# unsupported with a reason, every verb that would turn HDR on refuses with
# its error string and writes nothing to state.json, and the Display panel
# frame carries the dim "HDR unavailable" line under the output.
#
# The rule builder is held against the compositor: `hdr rule <output>` (the
# rule an enable would send, unsent) must restate the mode, position, scale
# and transform that `hyprctl monitors all -j` reports for the same output,
# and carry no vrr, which the merge leaves to the output's own rule or
# `misc.vrr`. The real toggle on a panel with HDR in its EDID is g815's to
# confirm by eye.
leg_hdr_flag="--hdr"
leg_hdr_order=215
leg_hdr_needs="jq"

hdr_status_path="$shot_dir/hdr-status.json"
hdr_enable_path="$shot_dir/hdr-enable.txt"
hdr_toggle_path="$shot_dir/hdr-toggle.txt"
hdr_set_path="$shot_dir/hdr-set.txt"
hdr_rule_path="$shot_dir/hdr-rule.json"
hdr_monitors_path="$shot_dir/hdr-monitors.json"
hdr_state_path="$shot_dir/hdr-state.json"
hdr_panel_png="$shot_dir/hdr-display-panel.png"

leg_hdr_timing() {
  leg_timing 20 44
}

leg_hdr_drive() {
  local script="$shot_dir/hdr-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 4
"$hyprctl_bin" monitors all -j > "$hdr_monitors_path" 2>&1
out=\$("$jq_bin" -r '.[0].name' "$hdr_monitors_path")
SECONDS=0
while [ "\$SECONDS" -lt 10 ]; do
  $ipc call hdr status > "$hdr_status_path" 2>&1
  grep -qF '"reason":"Checking"' "$hdr_status_path" || grep -qF '"outputs":[]' "$hdr_status_path" || break
  sleep 1
done
$ipc call hdr enable > "$hdr_enable_path" 2>&1
$ipc call hdr toggle > "$hdr_toggle_path" 2>&1
$ipc call hdr setOutput "\$out" true > "$hdr_set_path" 2>&1
$ipc call hdr rule "\$out" > "$hdr_rule_path" 2>&1
$ipc call panel open display > /dev/null 2>&1
sleep 3
"$grim_bin" "$hdr_panel_png" > /dev/null 2>&1
cp "$iso_home/.local/state/formalshell/state.json" "$hdr_state_path" 2>/dev/null
EOF
  hypr_exec_once "bash $script"
}

leg_hdr_assert() {
  local f
  for f in "$hdr_status_path" "$hdr_enable_path" "$hdr_toggle_path" "$hdr_set_path" "$hdr_rule_path" "$hdr_monitors_path" "$hdr_state_path"; do
    [ -s "$f" ] || fail "hdr leg produced no $(basename "$f")"
  done
  cat "$hdr_status_path"; echo

  "$jq_bin" -e '.active == false and (.outputs | length) >= 1
      and (.outputs | all(.supported == false and .on == false and .wanted == false
        and (.reason | length) > 0 and .reason != "Checking"))' "$hdr_status_path" > /dev/null \
    || fail "hdr status is not the honest unsupported state: $(cat "$hdr_status_path")"

  grep -qxF 'no output supports HDR' "$hdr_enable_path" \
    || fail "hdr enable did not refuse, got: $(cat "$hdr_enable_path")"
  grep -qxF 'no output supports HDR' "$hdr_toggle_path" \
    || fail "hdr toggle did not refuse, got: $(cat "$hdr_toggle_path")"
  grep -q '^HDR unavailable on .*: ' "$hdr_set_path" \
    || fail "hdr setOutput did not refuse with its reason, got: $(cat "$hdr_set_path")"

  "$jq_bin" -e '(.hdr // {}) | length == 0' "$hdr_state_path" > /dev/null \
    || fail "a refused HDR request reached state.json: $(cat "$hdr_state_path")"

  cat "$hdr_rule_path"; echo
  "$jq_bin" -e --slurpfile mon "$hdr_monitors_path" '
      .rule as $r | $mon[0][0] as $m
      | $r.output == $m.name
        and $r.mode == "\($m.width)x\($m.height)@\($m.refreshRate * 100000 | round / 100000)"
        and $r.position == "\($m.x)x\($m.y)"
        and (($r.scale | tonumber) - $m.scale | fabs) < 0.005
        and $r.transform == $m.transform
        and ($r | has("vrr") | not) and (.lua | contains("vrr") | not)
        and $r.cm == "hdr" and $r.bitdepth == 10' "$hdr_rule_path" > /dev/null \
    || fail "the rule builder disagrees with hyprctl monitors: $(cat "$hdr_rule_path") vs $("$jq_bin" -c '.[0] | {name,width,height,refreshRate,x,y,scale,transform}' "$hdr_monitors_path")"

  [ -f "$hdr_panel_png" ] || fail "no display panel screenshot produced"
  echo "SMOKE_HDR_PANEL $hdr_panel_png"
}
