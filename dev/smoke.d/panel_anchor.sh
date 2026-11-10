# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --panel-anchor proves the one thing `panel toggle` does: an open
# with no click behind it still hangs its card under the cell that owns the
# panel, instead of at the end of the bar where an anchorless open puts it.
#
# The default right region cannot show that. Every panel-bearing cell there
# sits within half a card's width of the screen edge, so the geometry
# clamp lands a cell-anchored frame on exactly the x an anchorless one gets,
# and the two frames are pixel-identical. This leg therefore puts the audio
# cell in the LEFT region, where the anchored card lands against the left
# screen padding and the anchorless one is still 1500px away.
#
# The claim is read off the screen rather than off a reply: one 200x100 crop
# of the same box under the left region, taken once with the panel toggled
# (anchored) and once with it opened (anchorless), and the two asserted to
# differ. Nothing else in that box moves between the two frames, since it
# holds neither the bar nor the clock, so a difference there is the card
# being in it exactly once.
leg_panel_anchor_flag="--panel-anchor"
leg_panel_anchor_order=75
leg_panel_anchor_claims="bar.layout.left bar.position=top panel=shared"

panel_anchor_toggle_reply_path="$shot_dir/panel-anchor-toggle.txt"
panel_anchor_toggle_state_path="$shot_dir/panel-anchor-toggle-state.txt"
panel_anchor_close_reply_path="$shot_dir/panel-anchor-close.txt"
panel_anchor_open_reply_path="$shot_dir/panel-anchor-open.txt"
panel_anchor_open_state_path="$shot_dir/panel-anchor-open-state.txt"
panel_anchor_anchored_path="$shot_dir/panel-anchor-anchored.png"
panel_anchor_anchorless_path="$shot_dir/panel-anchor-anchorless.png"
panel_anchor_anchored_crop_path="$shot_dir/panel-anchor-anchored-crop.png"
panel_anchor_anchorless_crop_path="$shot_dir/panel-anchor-anchorless-crop.png"

# Under the left region and clear of the strip: inside the anchored card
# (which starts at one screenPadding, the clamp having pulled it in from a
# cell centre well under half its own width) and over bare desktop with the
# card at the far end of the bar.
panel_anchor_crop_geometry="100,60 200x100"

leg_panel_anchor_fixture() {
  settings_pin '.bar.layout.left = ["launcher", "workspaces", "audio"]'
}

leg_panel_anchor_timing() {
  leg_timing 45
}

leg_panel_anchor_drive() {
  local script="$shot_dir/panel-anchor-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
$ipc call panel toggle audio > "$panel_anchor_toggle_reply_path" 2>&1
sleep 2
$ipc call panel state > "$panel_anchor_toggle_state_path" 2>&1
"$grim_bin" "$panel_anchor_anchored_path" > /dev/null 2>&1
"$grim_bin" -g "$panel_anchor_crop_geometry" "$panel_anchor_anchored_crop_path" > /dev/null 2>&1
$ipc call panel toggle audio > "$panel_anchor_close_reply_path" 2>&1
sleep 2
$ipc call panel open audio > "$panel_anchor_open_reply_path" 2>&1
sleep 2
$ipc call panel state > "$panel_anchor_open_state_path" 2>&1
"$grim_bin" "$panel_anchor_anchorless_path" > /dev/null 2>&1
"$grim_bin" -g "$panel_anchor_crop_geometry" "$panel_anchor_anchorless_crop_path" > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_panel_anchor_assert() {
  local f
  for f in "$panel_anchor_toggle_reply_path" "$panel_anchor_close_reply_path" "$panel_anchor_open_reply_path"; do
    if ! grep -q '^ok$' "$f" 2>/dev/null; then
      fail "a panel route did not answer ok at $f, got: $(cat "$f" 2>/dev/null)"
    fi
  done
  for f in "$panel_anchor_toggle_state_path" "$panel_anchor_open_state_path"; do
    if ! grep -q '^audio$' "$f" 2>/dev/null; then
      fail "panel state is not audio at $f, got: $(cat "$f" 2>/dev/null)"
    fi
  done
  for f in "$panel_anchor_anchored_path" "$panel_anchor_anchorless_path" \
    "$panel_anchor_anchored_crop_path" "$panel_anchor_anchorless_crop_path"; do
    [ -f "$f" ] || fail "no screenshot produced at $f"
  done
  echo "SMOKE_PANEL_ANCHOR_ANCHORED $panel_anchor_anchored_path"
  echo "SMOKE_PANEL_ANCHOR_ANCHORLESS $panel_anchor_anchorless_path"
  if cmp -s "$panel_anchor_anchored_crop_path" "$panel_anchor_anchorless_crop_path"; then
    fail "the box at $panel_anchor_crop_geometry is identical with the panel toggled and opened: the toggle did not hang the card under its own cell"
  fi
}
