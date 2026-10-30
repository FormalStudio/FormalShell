# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, iso_home, the *_bin paths and fail()
# --launcher-actions: the actions column beside the list, read off `menu
# status` (`actionsOpen`, `actions`, `token`, `ids`) and `hyprctl clients`.
# The staged Actions Probe app opens a foot window of its own app id and
# has one desktop action (Quiet window); its menu.jsonc entry hides the
# built-in Copy desktop file path and adds Write probe note. One probe
# window is up before the first open.
#
# A real Tab on the probe's row lists exactly the app's actions in the
# launcher spec's order (Open on discrete GPU allowed where the machine has
# two cards); `new` typed filters them to New window, and Enter on it maps
# a second window. → on the row steps into its two windows; Tab on one of
# them, `move` and Enter list the workspaces under a `Workspace` step, `2`
# and Enter move that window to workspace 2. Shift+Enter on the list runs
# the second action, a third window. Shift+Tab, Escape and ← with an empty
# filter each close the column and give the list its query back.
# `menu runAction` runs the menu.jsonc entry, and the three new verbs answer
# `error: menu not open` with the launcher shut. A route's column (Clipboard,
# searched by name) is Open then Pin to Recent; a real Down and Enter pins
# it, state.json's `recentPinned` holds it, and the root at rest heads
# `Recent` with it; Unpin from Recent off the root's own row lets it go.
# The column, the targets, the windows and the pinned root are photographed
# in dark, the column again in light.
leg_launcher_actions_flag="--launcher-actions"
leg_launcher_actions_order=30
leg_launcher_actions_needs="jq wtype foot"

launcher_actions_app="apps.formalshell-actions-probe"
launcher_actions_note="$shot_dir/launcher-actions-note"

leg_launcher_actions_fixture() {
  mkdir -p "$iso_home/.local/share/applications" "$iso_home/.config/formalshell"
  cat > "$iso_home/.local/share/applications/formalshell-actions-probe.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Actions Probe
Exec=$foot_bin --app-id=formalshell-actions-probe sleep 300
Actions=quiet;

[Desktop Action quiet]
Name=Quiet window
Exec=$foot_bin --app-id=formalshell-actions-probe sleep 300
EOF
  cat > "$iso_home/.config/formalshell/menu.jsonc" <<EOF
{
  "$launcher_actions_app": { "actions": [
    { "id": "copy-path", "hidden": true },
    { "id": "probe-note", "label": "Write probe note", "action": "touch $launcher_actions_note" }
  ] }
}
EOF
}

leg_launcher_actions_timing() {
  leg_timing 85 135
}

leg_launcher_actions_drive() {
  local script="$shot_dir/launcher-actions-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
call() { $ipc call "\$@"; }
status() { call menu status > "$shot_dir/launcher-actions-\$1.json" 2>&1; }
key() { "$wtype_bin" "\$@"; sleep 0.6; }
shoot() { "$grim_bin" "$shot_dir/launcher-actions-\$1.png" > /dev/null 2>&1; }
clients() { "$hyprctl_bin" clients -j > "$shot_dir/launcher-actions-clients-\$1.json" 2>&1; }
probe() { call menu summon "" > /dev/null 2>&1; sleep 0.4; call menu filter "Actions Probe" > /dev/null 2>&1; sleep 0.6; }
sleep 4
for verb in "actions 0" "runAction 0 open" "into 0"; do
  call menu \$verb >> "$shot_dir/launcher-actions-closed.txt" 2>&1
done
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-actions-probe sleep 300]==])" > /dev/null 2>&1
sleep 2

probe
status list
key -k Tab
sleep 0.6
status tab
shoot column-dark
key new
status filtered
key -k Return
sleep 2
status after-new
clients new

probe
key -k Right
sleep 0.6
status into
shoot into-dark
key -k Tab
status window-actions
key move
key -k Return
sleep 0.6
status targets
shoot targets-dark
key 2
status target-filtered
key -k Return
sleep 1.5
clients moved

probe
key -k Right
call menu runAction 0 float > /dev/null 2>&1
sleep 1
clients float
probe
key -k Right
call menu runAction 0 fullscreen > /dev/null 2>&1
sleep 1
clients fullscreen
probe
key -k Right
call menu runAction 0 fullscreen > /dev/null 2>&1
sleep 1
clients unfullscreen

probe
key -M shift -k Return -m shift
sleep 2
clients alternate

probe
key -k Tab
status close-0
key -M shift -k Tab -m shift
status close-shift-tab
key -k Tab
key -k Escape
status close-escape
key -k Tab
key -k Left
status close-left
call menu actions 0 > "$shot_dir/launcher-actions-ipc-actions.txt" 2>&1
sleep 0.4
status ipc-actions
call menu close > /dev/null 2>&1
probe
i=\$("$jq_bin" --arg id "$launcher_actions_app" '.ids | index(\$id) // -1' "$shot_dir/launcher-actions-ipc-actions.json")
call menu runAction "\$i" probe-note > "$shot_dir/launcher-actions-ipc-run.txt" 2>&1
sleep 1

call menu summon "" > /dev/null 2>&1
sleep 0.4
call menu filter Clipboard > /dev/null 2>&1
sleep 0.6
status pin-list
i=\$("$jq_bin" '.ids | index("clipboard") // -1' "$shot_dir/launcher-actions-pin-list.json")
call menu actions "\$i" > /dev/null 2>&1
sleep 0.6
status pin-column
key -k Down
key -k Return
sleep 1
status pin-ran
cp "$iso_home/.local/state/formalshell/state.json" "$shot_dir/launcher-actions-pin-state.json" 2>/dev/null
call menu close > /dev/null 2>&1
sleep 0.4
call menu summon "" > /dev/null 2>&1
sleep 1
status pin-root
shoot pin-root-dark
call menu actions 0 > /dev/null 2>&1
sleep 0.6
status unpin-column
call menu runAction 0 unpin-recent > /dev/null 2>&1
sleep 1
cp "$iso_home/.local/state/formalshell/state.json" "$shot_dir/launcher-actions-unpin-state.json" 2>/dev/null
call menu close > /dev/null 2>&1
sleep 0.4
call menu summon "" > /dev/null 2>&1
sleep 1
status unpin-root
call menu close > /dev/null 2>&1

call theme mode light > /dev/null 2>&1
sleep 2
probe
key -k Tab
sleep 0.6
shoot column-light
call menu close > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

# `<step> <jq claim> <what it means>`, against that step's status.
launcher_actions_expect() {
  local f="$shot_dir/launcher-actions-$1.json"
  [ -s "$f" ] || fail "launcher-actions: no status for $1"
  "$jq_bin" -e "$2" "$f" > /dev/null || fail "launcher-actions $1: $3, got $("$jq_bin" -c '{isOpen, level, token, query, actionsOpen, actions, ids: .ids[:6]}' "$f")"
}

# The probe's windows in one `hyprctl clients` capture, and how many sit on
# workspace 2.
launcher_actions_windows() {
  "$jq_bin" '[.[] | select(.class == "formalshell-actions-probe")] | length' "$shot_dir/launcher-actions-clients-$1.json"
}

leg_launcher_actions_assert() {
  local closed
  closed=$(grep -c '^error: menu not open$' "$shot_dir/launcher-actions-closed.txt" 2>/dev/null)
  [ "$closed" = 3 ] || fail "launcher-actions: actions, runAction and into did not all answer 'error: menu not open' shut: $(cat "$shot_dir/launcher-actions-closed.txt")"
  echo "SMOKE_LAUNCHER_ACTIONS the three verbs answer shut"

  launcher_actions_expect list ".ids[0] == \"$launcher_actions_app\"" "the probe does not lead its own name"
  launcher_actions_expect tab '.actionsOpen and ([.actions[] | select(. != "discrete-gpu")] == ["open", "new-window", "desktop:quiet", "windows", "move-to-workspace", "probe-note"])' "Tab did not list the app's actions in order, with copy-path hidden and probe-note added"
  launcher_actions_expect tab '.query == "" and .cursorId == "'"$launcher_actions_app"'"' "the column did not take the field or the row lost the selection"
  launcher_actions_expect filtered '.actions == ["new-window"]' "'new' did not filter the column to New window"
  launcher_actions_expect after-new '.isOpen == false' "New window did not close the launcher"
  [ "$(launcher_actions_windows new)" = 2 ] || fail "launcher-actions: New window did not map a second probe window"
  echo "SMOKE_LAUNCHER_ACTIONS Tab listed the app's actions, 'new' filtered to New window, Enter mapped a second window"

  launcher_actions_expect into '.token[-1] == "Actions Probe" and (.ids | length) == 2 and all(.ids[]; startswith("window:"))' "→ did not list the probe's two windows"
  launcher_actions_expect window-actions '.actions == ["focus", "close", "move-to-workspace", "float", "fullscreen"]' "a window's column is not Focus, Close, Move, Floating, Fullscreen"
  launcher_actions_expect targets '.actionsOpen and (.actions | index("workspace:2")) != null and .placeholder == "Search targets"' "Move to workspace did not list the workspaces"
  launcher_actions_expect target-filtered '.actions == ["workspace:2"]' "'2' did not filter the targets to workspace 2"
  local moved
  moved=$("$jq_bin" '[.[] | select(.class == "formalshell-actions-probe" and .workspace.id == 2)] | length' "$shot_dir/launcher-actions-clients-moved.json")
  [ "$moved" = 1 ] || fail "launcher-actions: no probe window moved to workspace 2 ($moved there)"
  echo "SMOKE_LAUNCHER_ACTIONS → listed two windows, Move to workspace then 2 moved one there"

  local floating full
  floating=$("$jq_bin" '[.[] | select(.class == "formalshell-actions-probe" and .floating)] | length' "$shot_dir/launcher-actions-clients-float.json")
  [ "$floating" = 1 ] || fail "launcher-actions: Toggle floating on a window row floated $floating probe windows"
  full=$("$jq_bin" '[.[] | select(.class == "formalshell-actions-probe" and .fullscreen != 0)] | length' "$shot_dir/launcher-actions-clients-fullscreen.json")
  [ "$full" = 1 ] || fail "launcher-actions: Fullscreen on a window row left $full probe windows fullscreen"
  full=$("$jq_bin" '[.[] | select(.class == "formalshell-actions-probe" and .fullscreen != 0)] | length' "$shot_dir/launcher-actions-clients-unfullscreen.json")
  [ "$full" = 0 ] || fail "launcher-actions: Fullscreen a second time did not leave fullscreen ($full still)"
  echo "SMOKE_LAUNCHER_ACTIONS a window row's float and fullscreen read back off hyprctl clients"
  [ "$(launcher_actions_windows alternate)" = 3 ] || fail "launcher-actions: Shift+Enter on the list did not run New window"
  echo "SMOKE_LAUNCHER_ACTIONS Shift+Enter ran the second action"

  launcher_actions_expect close-0 '.actionsOpen' "Tab did not open the column"
  local how
  for how in shift-tab escape left; do
    launcher_actions_expect "close-$how" '.isOpen and (.actionsOpen | not) and .query == "Actions Probe"' "$how did not close the column back onto the list's query"
  done
  echo "SMOKE_LAUNCHER_ACTIONS Shift+Tab, Escape and Left each closed the column"

  grep -q '^ok$' "$shot_dir/launcher-actions-ipc-actions.txt" || fail "launcher-actions: menu actions did not answer ok: $(cat "$shot_dir/launcher-actions-ipc-actions.txt")"
  launcher_actions_expect ipc-actions '.actionsOpen' "menu actions did not open the column"
  grep -q '^ok$' "$shot_dir/launcher-actions-ipc-run.txt" || fail "launcher-actions: menu runAction did not answer ok: $(cat "$shot_dir/launcher-actions-ipc-run.txt")"
  [ -e "$launcher_actions_note" ] || fail "launcher-actions: the menu.jsonc action did not run"
  echo "SMOKE_LAUNCHER_ACTIONS menu actions opened the column and runAction ran the menu.jsonc entry"

  launcher_actions_expect pin-column '.actionsOpen and .actions == ["open", "pin-recent"]' "a route's column is not Open then Pin to Recent"
  launcher_actions_expect pin-ran '.isOpen and (.actionsOpen | not)' "Pin to Recent did not leave the launcher open on the list"
  "$jq_bin" -e '.recentPinned == ["clipboard"]' "$shot_dir/launcher-actions-pin-state.json" > /dev/null \
    || fail "launcher-actions: state.json's recentPinned is not [clipboard] after the pin: $("$jq_bin" -c '.recentPinned' "$shot_dir/launcher-actions-pin-state.json" 2>&1)"
  launcher_actions_expect pin-root '.query == "" and .sections == ["Recent"] and .ids[0] == "clipboard"' "the pinned route does not head Recent at rest"
  launcher_actions_expect unpin-column '.actions == ["open", "unpin-recent"]' "the pinned route's column does not offer Unpin from Recent"
  "$jq_bin" -e '.recentPinned == []' "$shot_dir/launcher-actions-unpin-state.json" > /dev/null \
    || fail "launcher-actions: Unpin from Recent left recentPinned $("$jq_bin" -c '.recentPinned' "$shot_dir/launcher-actions-unpin-state.json" 2>&1)"
  launcher_actions_expect unpin-root '.ids[0] != "clipboard"' "the unpinned route still heads the root"
  echo "SMOKE_LAUNCHER_ACTIONS Pin to Recent put Clipboard at the head of Recent, Unpin took it off"

  local shot
  for shot in column-dark into-dark targets-dark pin-root-dark column-light; do
    [ -s "$shot_dir/launcher-actions-$shot.png" ] || fail "launcher-actions: no $shot frame"
    echo "SMOKE_LAUNCHER_ACTIONS_SHOT $shot_dir/launcher-actions-$shot.png"
  done
}
