# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --toggles: the launcher's toggle hub, proven live rather than by frame
# alone. `debug query` carries each row's resolved `checked`, so a row's
# checkmark is readable from outside the process, and `menu status` before
# and after must be identical but for its own `checked` list: the surface
# stays open at the same level, cursor and scroll across the toggle, so the
# row repainted from a @state: snapshot rather than a level being re-entered.
#
# Two toggles, and both are needed. The nightlight row's checkmark must agree
# with `nightlight status` in both samples, whichever way wlsunset went on
# this session, which is what proves the row renders the service's real state
# instead of the fact that a toggle was asked for. The DND row's @state: path
# has no external dependency at all, so it is what proves a checkmark
# actually flips inside an open hub.
leg_toggles_flag="--toggles"
leg_toggles_order=230
leg_toggles_needs="jq"

toggles_menu_status1_path="$shot_dir/toggles-menu-status-1.json"
toggles_menu_status2_path="$shot_dir/toggles-menu-status-2.json"
toggles_nl_status1_path="$shot_dir/toggles-nightlight-status-1.json"
toggles_nl_status2_path="$shot_dir/toggles-nightlight-status-2.json"
toggles_nl_query1_path="$shot_dir/toggles-nightlight-query-1.json"
toggles_nl_query2_path="$shot_dir/toggles-nightlight-query-2.json"
toggles_dnd_status1_path="$shot_dir/toggles-dnd-status-1.json"
toggles_dnd_status2_path="$shot_dir/toggles-dnd-status-2.json"
toggles_dnd_query1_path="$shot_dir/toggles-dnd-query-1.json"
toggles_dnd_query2_path="$shot_dir/toggles-dnd-query-2.json"
toggles_hub_png="$shot_dir/toggles-hub.png"
toggles_toggled_png="$shot_dir/toggles-toggled.png"

# This leg's own clock: the launcher covers the whole output, so under
# --wallpaper it starts after that leg's last frame, the same rule menu_t0
# draws.
toggles_t0() {
  cover_t0 4
}

leg_toggles_timing() {
  local t0
  t0=$(toggles_t0)
  leg_timing $((t0 + 10)) $((t0 + 40))
}

leg_toggles_drive() {
  local t0 script="$shot_dir/toggles-drive.sh"
  t0=$(toggles_t0)
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep $t0
$ipc call menu summon toggles > /dev/null 2>&1
sleep 2
$ipc call menu status > "$toggles_menu_status1_path" 2>&1
$ipc call nightlight status > "$toggles_nl_status1_path" 2>&1
$ipc call debug query nightlight > "$toggles_nl_query1_path" 2>&1
$ipc call notifications status > "$toggles_dnd_status1_path" 2>&1
$ipc call debug query dnd > "$toggles_dnd_query1_path" 2>&1
"$grim_bin" "$toggles_hub_png" > /dev/null 2>&1

$ipc call nightlight toggle > /dev/null 2>&1
$ipc call notifications toggleDnd > /dev/null 2>&1
sleep 2
$ipc call nightlight status > "$toggles_nl_status2_path" 2>&1
$ipc call debug query nightlight > "$toggles_nl_query2_path" 2>&1
$ipc call notifications status > "$toggles_dnd_status2_path" 2>&1
$ipc call debug query dnd > "$toggles_dnd_query2_path" 2>&1
$ipc call menu status > "$toggles_menu_status2_path" 2>&1
"$grim_bin" "$toggles_toggled_png" > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

# One ranked row out of a debug query reply. The row objects hold no nested
# objects, so the id-anchored span up to the next "}" is the whole row.
# `|| true`: a query that ranked no such row is a real outcome this leg
# reports itself, and under pipefail grep's own miss would otherwise abort
# with no message at all.
toggles_row() {
  grep -o "{\"id\":\"$2\"[^}]*}" "$1" | head -n1 || true
}

toggles_checked() {
  case "$1" in
    *'"checked":true'*) echo true ;;
    *) echo false ;;
  esac
}

leg_toggles_assert() {
  local f nl_row1 nl_row2 nl_checked1 nl_checked2 nl_active1 nl_active2 dnd_row1 dnd_row2
  for f in "$toggles_menu_status1_path" "$toggles_menu_status2_path" \
    "$toggles_nl_status1_path" "$toggles_nl_status2_path" \
    "$toggles_nl_query1_path" "$toggles_nl_query2_path" \
    "$toggles_dnd_status1_path" "$toggles_dnd_status2_path" \
    "$toggles_dnd_query1_path" "$toggles_dnd_query2_path"; do
    [ -s "$f" ] || fail "no toggles artifact produced at $f"
  done

  cat "$toggles_menu_status1_path"; echo
  if ! grep -qF '"isOpen":true' "$toggles_menu_status1_path" \
    || ! grep -qF '"level":"toggles"' "$toggles_menu_status1_path"; then
    fail "menu did not summon to the toggles hub: $(cat "$toggles_menu_status1_path")"
  fi
  if [ "$(jq -c 'del(.checked)' "$toggles_menu_status1_path")" != "$(jq -c 'del(.checked)' "$toggles_menu_status2_path")" ]; then
    diff "$toggles_menu_status1_path" "$toggles_menu_status2_path" >&2 || true
    fail "menu state changed across the toggles (a rebuild or reopen, not a live repaint)"
  fi

  nl_row1=$(toggles_row "$toggles_nl_query1_path" "toggles.nightlight")
  nl_row2=$(toggles_row "$toggles_nl_query2_path" "toggles.nightlight")
  if [ -z "$nl_row1" ] || [ -z "$nl_row2" ]; then
    cat "$toggles_nl_query1_path" >&2
    fail "the nightlight toggle row never ranked (its wlsunset when-gate unresolved?)"
  fi
  echo "$nl_row1"
  echo "$nl_row2"
  nl_checked1=$(toggles_checked "$nl_row1")
  nl_checked2=$(toggles_checked "$nl_row2")
  nl_active1=false
  nl_active2=false
  grep -qF '"active":true' "$toggles_nl_status1_path" && nl_active1=true
  grep -qF '"active":true' "$toggles_nl_status2_path" && nl_active2=true
  if [ "$nl_checked1" != "$nl_active1" ] || [ "$nl_checked2" != "$nl_active2" ]; then
    cat "$toggles_nl_status1_path" >&2; echo >&2
    cat "$toggles_nl_status2_path" >&2
    fail "the nightlight row's checkmark disagrees with nightlight status (row $nl_checked1/$nl_checked2 vs service $nl_active1/$nl_active2)"
  fi
  if [ "$nl_active2" = "true" ]; then
    echo "SMOKE_TOGGLES_NIGHTLIGHT wlsunset held: the row's checkmark flipped with it"
  else
    echo "SMOKE_TOGGLES_NIGHTLIGHT wlsunset could not hold this session (lastError: $(sed -n 's/.*"lastError":"\([^"]*\)".*/\1/p' "$toggles_nl_status2_path")); the row honestly stayed unchecked"
  fi

  dnd_row1=$(toggles_row "$toggles_dnd_query1_path" "toggles.dnd")
  dnd_row2=$(toggles_row "$toggles_dnd_query2_path" "toggles.dnd")
  if [ -z "$dnd_row1" ] || [ -z "$dnd_row2" ]; then
    cat "$toggles_dnd_query1_path" >&2
    fail "the DND toggle row never ranked"
  fi
  echo "$dnd_row1"
  echo "$dnd_row2"
  # This row's @state: path is pure in-process state, so it can only fail if
  # the checkmark itself is not live.
  if [ "$(toggles_checked "$dnd_row1")" != "false" ] || [ "$(toggles_checked "$dnd_row2")" != "true" ]; then
    echo "$dnd_row1" >&2; echo "$dnd_row2" >&2
    fail "the DND row's checkmark did not flip false -> true inside the open hub"
  fi
  if ! grep -qF '"dnd":false' "$toggles_dnd_status1_path" || ! grep -qF '"dnd":true' "$toggles_dnd_status2_path"; then
    cat "$toggles_dnd_status1_path" >&2; echo >&2
    cat "$toggles_dnd_status2_path" >&2
    fail "notifications status does not back the DND row's own flip"
  fi
  for f in "$toggles_hub_png" "$toggles_toggled_png"; do
    [ -f "$f" ] || fail "no toggles screenshot produced at $f"
  done
  echo "SMOKE_TOGGLES_HUB $toggles_hub_png"
  echo "SMOKE_TOGGLES_TOGGLED $toggles_toggled_png"
}
