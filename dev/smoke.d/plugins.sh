# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --plugins drops real plugin directories into the isolated config home, in
# the exact manifest shape fs-chrome's plugins.rs parses, and reads them back through the
# `plugins` target. No `bar` key is written for this leg on purpose: the
# manifest's own `region` is what places the cell, so dropping the directory
# in is the whole install, which is the contract worth proving. `list` is the
# resolved manifest record and `status` is the load outcome.
#
# A plugin is an executable (the spec's "User code" section): `entry` names a shell script that prints JSON lines and reads
# events as JSON lines. Two plugins prove the contract. smoke-bar prints one
# line and logs every event it is sent, and the leg then clicks and scrolls
# its cell with a real pointer. smoke-crash logs its start and exits, which
# has to read as a PLUGIN ERROR in `status` and be started again on the
# backoff, so its start log has to grow.
#
# Three more cover the surface kinds. smoke-panel is a panel plugin that is
# not kept loaded: no process before `panel open plugin:smoke-panel`, one
# while its card is up, rows of every type on the card, a real Enter on the
# row under the cursor and another on the toggle (`activate` events with
# the row id, `checked` on the toggle, the plugin reprinting its rows as
# the reply), and no process again after `panel close`. smoke-panel-crash is
# a kept-loaded panel that exits, so its card reads PLUGIN ERROR and it is
# started again on the backoff. smoke-overlay is an overlay plugin opened
# over IPC, one real Enter reaching it and a real Escape closing it.
#
# smoke-launcher is a launcher plugin (launcher spec §10) with a mode and
# `subjects: ["file"]`. No process while the launcher is shut, one while it
# is open. `menu summon plugin:smoke-launcher` stands its mode in the field
# as a token over the two rows it prints; `beta` typed there reaches it as
# a `query` event and it answers with that row alone, which a real Enter
# activates. It prints Alpha alone at start and both rows only in answer to
# a query, so the same mode opened again after the launcher shut has to
# send its query again to list both. Its rows also rank in a root search.
# Tab over the staged recent file sends an `actions` event carrying the
# file's kind, path and `file:` id, the plugin's answer (the spec's shape,
# no `subject`) joins the column, and running it sends `activate` with the
# file's id and the action.
leg_plugins_flag="--plugins"
leg_plugins_order=200
leg_plugins_needs="jq wlrctl wtype"

plugins_list_path="$shot_dir/plugins-list.json"
plugins_status_path="$shot_dir/plugins-status.json"
plugins_room_path="$shot_dir/plugins-room.json"
plugins_events_path="$shot_dir/plugins-events.log"
plugins_starts_path="$shot_dir/plugins-crash-starts.log"
plugins_bar_png="$shot_dir/plugins-bar.png"
plugins_root="$iso_home/.config/formalshell/plugins"
plugins_panel_events_path="$shot_dir/plugins-panel-events.log"
plugins_panel_starts_path="$shot_dir/plugins-panel-starts.log"
plugins_panel_crash_starts_path="$shot_dir/plugins-panel-crash-starts.log"
plugins_overlay_events_path="$shot_dir/plugins-overlay-events.log"
plugins_panel_state_path="$shot_dir/plugins-panel-state.txt"
plugins_panel_procs_path="$shot_dir/plugins-panel-procs.txt"
plugins_overlay_state_path="$shot_dir/plugins-overlay-state.txt"
plugins_unknown_path="$shot_dir/plugins-unknown.txt"
plugins_panel_png="$shot_dir/plugins-panel.png"
plugins_panel_after_png="$shot_dir/plugins-panel-activated.png"
plugins_panel_crash_png="$shot_dir/plugins-panel-crash.png"
plugins_overlay_png="$shot_dir/plugins-overlay.png"
plugins_launcher_events_path="$shot_dir/plugins-launcher-events.log"
plugins_launcher_procs_path="$shot_dir/plugins-launcher-procs.txt"
plugins_launcher_file="$iso_home/Documents/smoke-plugin-file.txt"
plugin_dir="$iso_home/.config/formalshell/plugins/smoke-bar"
plugin_crash_dir="$iso_home/.config/formalshell/plugins/smoke-crash"

leg_plugins_fixture() {
  mkdir -p "$plugin_dir"
  mkdir -p "$plugin_crash_dir"
  cat > "$plugin_dir/manifest.json" <<'EOF'
{
  "apiVersion": 1,
  "id": "smoke-bar",
  "kind": "bar",
  "entry": "entry.sh",
  "name": "Smoke Bar Plugin",
  "region": "right"
}
EOF
  cat > "$plugin_dir/entry.sh" <<EOF
#!/usr/bin/env bash
printf '%s\n' '{"text": "PLUGIN OK", "icon": "puzzle", "tooltip": "smoke plugin", "class": ""}'
while IFS= read -r line; do
  printf '%s\n' "\$line" >> "$plugins_events_path"
done
EOF
  cat > "$plugin_crash_dir/manifest.json" <<'EOF'
{
  "apiVersion": 1,
  "id": "smoke-crash",
  "kind": "bar",
  "entry": "entry.sh",
  "region": "right"
}
EOF
  cat > "$plugin_crash_dir/entry.sh" <<EOF
#!/usr/bin/env bash
echo start >> "$plugins_starts_path"
exit 3
EOF
  chmod +x "$plugin_dir/entry.sh" "$plugin_crash_dir/entry.sh"
  leg_plugins_surface_fixture
}

leg_plugins_surface_fixture() {
  local d
  d="$plugins_root/smoke-panel"
  mkdir -p "$d"
  cat > "$d/manifest.json" <<'EOF'
{
  "apiVersion": 1,
  "id": "smoke-panel",
  "kind": "panel",
  "entry": "entry.sh",
  "name": "Smoke Panel",
  "width": "narrow"
}
EOF
  cat > "$d/entry.sh" <<EOF
#!/usr/bin/env bash
echo start >> "$plugins_panel_starts_path"
detail=idle
checked=false
show() {
  printf '{"rows":[{"type":"label","text":"Smoke rows"},'
  printf '{"id":"r1","icon":"zap","text":"First row","detail":"%s"},' "\$detail"
  printf '{"type":"toggle","id":"t1","icon":"moon","text":"A toggle","checked":%s},' "\$checked"
  printf '{"type":"button","id":"b1","text":"Press me"}]}\n'
}
show
while IFS= read -r line; do
  printf '%s\n' "\$line" >> "$plugins_panel_events_path"
  case \$(jq -r .id <<<"\$line") in
    r1) detail=activated ;;
    t1) checked=\$(jq -r .checked <<<"\$line") ;;
  esac
  show
done
EOF
  d="$plugins_root/smoke-panel-crash"
  mkdir -p "$d"
  cat > "$d/manifest.json" <<'EOF'
{
  "apiVersion": 1,
  "id": "smoke-panel-crash",
  "kind": "panel",
  "entry": "entry.sh",
  "name": "Smoke Panel Crash",
  "keepLoaded": true
}
EOF
  cat > "$d/entry.sh" <<EOF
#!/usr/bin/env bash
echo start >> "$plugins_panel_crash_starts_path"
exit 4
EOF
  d="$plugins_root/smoke-overlay"
  mkdir -p "$d"
  cat > "$d/manifest.json" <<'EOF'
{
  "apiVersion": 1,
  "id": "smoke-overlay",
  "kind": "overlay",
  "entry": "entry.sh",
  "name": "Smoke Overlay"
}
EOF
  cat > "$d/entry.sh" <<EOF
#!/usr/bin/env bash
printf '%s\n' '{"rows":[{"id":"ov1","icon":"zap","text":"Overlay row","detail":"summoned"}]}'
while IFS= read -r line; do
  printf '%s\n' "\$line" >> "$plugins_overlay_events_path"
done
EOF
  chmod +x "$plugins_root/smoke-panel/entry.sh" "$plugins_root/smoke-panel-crash/entry.sh" "$plugins_root/smoke-overlay/entry.sh"
  leg_plugins_launcher_fixture
}

leg_plugins_launcher_fixture() {
  local d="$plugins_root/smoke-launcher" xbel="$iso_home/.local/share/recently-used.xbel"
  mkdir -p "$d" "$iso_home/Documents" "$iso_home/.local/share"
  cat > "$d/manifest.json" <<'EOF'
{
  "apiVersion": 1,
  "id": "smoke-launcher",
  "kind": "launcher",
  "entry": "entry.sh",
  "name": "Smoke Launcher",
  "mode": "Smoke Mode",
  "subjects": ["file"]
}
EOF
  cat > "$d/entry.sh" <<EOF
#!/usr/bin/env bash
rows() {
  local out=()
  case alpha in *"\$1"*) out+=('{"id":"alpha","icon":"zap","text":"Alpha note","detail":"first"}') ;; esac
  case beta in *"\$1"*) out+=('{"id":"beta","icon":"moon","text":"Beta note","detail":"second","actions":[{"id":"pin","label":"Pin note"}]}') ;; esac
  printf '{"rows":[%s]}\n' "\$(IFS=,; echo "\${out[*]}")"
}
rows alpha
while IFS= read -r line; do
  printf '%s\n' "\$line" >> "$plugins_launcher_events_path"
  case \$(jq -r .event <<<"\$line") in
    query) rows "\$(jq -r .text <<<"\$line")" ;;
    actions) jq -c '{actions: [{id: "smoke-file", label: "Smoke file action", icon: "zap"}]}' <<<"\$line" ;;
  esac
done
EOF
  chmod +x "$d/entry.sh"
  printf 'smoke plugin file\n' > "$plugins_launcher_file"
  # A leg riding along that stages its own recent list keeps it.
  [ -e "$xbel" ] || cat > "$xbel" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<xbel version="1.0">
  <bookmark href="file://$plugins_launcher_file" added="2026-10-29T10:00:00Z" modified="2026-10-29T12:00:00Z" visited="2026-10-29T12:00:00Z"/>
</xbel>
EOF
}

leg_plugins_timing() {
  # A 5s startup wait plus two dumps and one grim; 12 leaves llvmpipe real
  # margin on the capture rather than racing the default 8s frame.
  leg_timing 12 136
}

leg_plugins_drive() {
  local script="$shot_dir/plugins-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 5
$ipc call plugins list > "$plugins_list_path" 2>&1
$ipc call bar room > "$plugins_room_path" 2>&1
rect=\$(jq -r '.[].cells[] | select(.name == "plugin:smoke-bar") | "\(.x + .width / 2 | floor) \(.y + .height / 2 | floor)"' "$plugins_room_path")
if [ -n "\$rect" ]; then
  "$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1
  sleep 1
  "$wlrctl_bin" pointer move \${rect% *} \${rect#* } > /dev/null 2>&1
  sleep 1
  "$wlrctl_bin" pointer click left > /dev/null 2>&1
  sleep 1
  "$wlrctl_bin" pointer scroll -10 0 > /dev/null 2>&1
  sleep 1
fi
$ipc call plugins status > "$plugins_status_path" 2>&1
"$grim_bin" "$plugins_bar_png" > /dev/null 2>&1
# Off the card's rows, whose hover would put the keyboard cursor on one.
"$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1
procs() { pgrep -fc '$plugins_root/smoke-panel/entry.sh' || true; }
echo "before: \$(procs)" > "$plugins_panel_procs_path"
$ipc call panel open plugin:nope > "$plugins_unknown_path" 2>&1
$ipc call panel open plugin:smoke-panel > "$plugins_panel_state_path" 2>&1
sleep 2
$ipc call panel state >> "$plugins_panel_state_path" 2>&1
echo "open: \$(procs)" >> "$plugins_panel_procs_path"
"$grim_bin" "$plugins_panel_png" > /dev/null 2>&1
# Up clamps at the first row wherever the cursor starts, then one row down
# per Enter: the row, the toggle, the button.
"$wtype_bin" -k Up -k Up -k Up -k Return
sleep 1
"$wtype_bin" -k Down -k Return
sleep 1
"$wtype_bin" -k Down -k Return
sleep 1
"$grim_bin" "$plugins_panel_after_png" > /dev/null 2>&1
$ipc call panel close > /dev/null 2>&1
sleep 2
echo "closed: \$(procs)" >> "$plugins_panel_procs_path"
$ipc call panel open plugin:smoke-panel-crash > /dev/null 2>&1
sleep 3
"$grim_bin" "$plugins_panel_crash_png" > /dev/null 2>&1
$ipc call panel close > /dev/null 2>&1
sleep 1
$ipc call panel open plugin:smoke-overlay > /dev/null 2>&1
sleep 2
$ipc call panel state > "$plugins_overlay_state_path" 2>&1
"$grim_bin" "$plugins_overlay_png" > /dev/null 2>&1
"$wtype_bin" -k Down -k Return
sleep 1
"$wtype_bin" -k Escape
sleep 2
$ipc call panel state >> "$plugins_overlay_state_path" 2>&1
lprocs() { pgrep -fc '$plugins_root/smoke-launcher/entry.sh' || true; }
lstatus() { $ipc call menu status > "$shot_dir/plugins-launcher-\$1.json" 2>&1; }
echo "before: \$(lprocs)" > "$plugins_launcher_procs_path"
$ipc call menu summon plugin:smoke-launcher > /dev/null 2>&1
sleep 2
echo "open: \$(lprocs)" >> "$plugins_launcher_procs_path"
lstatus mode
"$grim_bin" "$shot_dir/plugins-launcher-mode.png" > /dev/null 2>&1
"$wtype_bin" beta
sleep 1.5
lstatus query
"$grim_bin" "$shot_dir/plugins-launcher-query.png" > /dev/null 2>&1
"$wtype_bin" -k Return
sleep 2
echo "closed: \$(lprocs)" >> "$plugins_launcher_procs_path"
$ipc call menu summon plugin:smoke-launcher > /dev/null 2>&1
sleep 2
lstatus reopen
$ipc call menu close > /dev/null 2>&1
sleep 2
$ipc call menu summon "" > /dev/null 2>&1
sleep 1.5
$ipc call menu filter "Alpha note" > /dev/null 2>&1
sleep 1
lstatus root
"$grim_bin" "$shot_dir/plugins-launcher-root.png" > /dev/null 2>&1
$ipc call menu filter smoke-plugin-file > /dev/null 2>&1
sleep 1.5
lstatus file
i=\$(jq '[.ids[] | startswith("files.")] | index(true) // -1' "$shot_dir/plugins-launcher-file.json")
$ipc call menu actions "\$i" > /dev/null 2>&1
sleep 1
lstatus actions
"$grim_bin" "$shot_dir/plugins-launcher-actions.png" > /dev/null 2>&1
$ipc call menu runAction "\$i" smoke-file > /dev/null 2>&1
sleep 2
echo "after: \$(lprocs)" >> "$plugins_launcher_procs_path"
$ipc call plugins status > "$plugins_status_path" 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_plugins_assert() {
  local f
  for f in "$plugins_list_path" "$plugins_status_path"; do
    if [ ! -s "$f" ]; then
      fail "no plugins artifact produced at $f"
    fi
  done
  cat "$plugins_list_path"; echo
  cat "$plugins_status_path"; echo
  if [ ! -f "$plugins_bar_png" ]; then
    fail "no plugins screenshot produced at $plugins_bar_png"
  fi
  leg_plugins_assert_command
  echo "SMOKE_PLUGINS $plugins_bar_png (no bar key in settings.json: the manifest's own region is what placed the cell)"
}

leg_plugins_assert_command() {
  local id
  for id in smoke-bar smoke-crash; do
    jq -e --arg id "$id" --arg url "file://$iso_home/.config/formalshell/plugins/$id/entry.sh" \
      '.[] | select(.id == $id and .kind == "bar" and .region == "right" and .entryUrl == $url)' \
      "$plugins_list_path" > /dev/null \
      || fail "the drop-in plugin $id did not resolve out of its manifest: $(cat "$plugins_list_path")"
  done
  jq -e '[.errors[] | select(.id == "smoke-crash") | .message | test("status 3")] == [true]' "$plugins_status_path" > /dev/null \
    || fail "the crashed bar plugin should read as failed with its exit status: $(cat "$plugins_status_path")"
  [ "$(wc -l < "$plugins_starts_path" 2>/dev/null || echo 0)" -ge 2 ] \
    || fail "the crashed plugin was not started again on its backoff: $(cat "$plugins_starts_path" 2>/dev/null)"
  grep -qF '"event":"click"' "$plugins_events_path" 2>/dev/null \
    && grep -qF '"button":"left"' "$plugins_events_path" \
    || fail "a click on the plugin's cell never reached its stdin: $(cat "$plugins_events_path" 2>/dev/null) room: $(cat "$plugins_room_path")"
  grep -qF '"event":"scroll"' "$plugins_events_path" \
    || fail "a wheel notch over the plugin's cell never reached its stdin: $(cat "$plugins_events_path" 2>/dev/null)"
  leg_plugins_assert_surfaces
}

leg_plugins_assert_surfaces() {
  local f
  for f in "$plugins_panel_png" "$plugins_panel_after_png" "$plugins_panel_crash_png" "$plugins_overlay_png"; do
    [ -f "$f" ] || fail "no plugin surface screenshot produced at $f"
  done
  jq -e '[.[] | select(.kind == "panel" or .kind == "overlay")] | length == 3' "$plugins_list_path" > /dev/null \
    || fail "the panel and overlay plugins did not all resolve: $(cat "$plugins_list_path")"
  jq -e '.loaded == true and .count == 6 and .bar == 2 and .surface == 3 and .launcher == 1 and .warnings == [] and (.surfaces | index("plugin:smoke-panel") != null)' \
    "$plugins_status_path" > /dev/null \
    || fail "plugins status did not report two bar, three surface and one launcher plugin: $(cat "$plugins_status_path")"
  jq -e '([.errors[].id] | sort) == ["smoke-crash", "smoke-panel-crash"] and ([.errors[] | select(.id == "smoke-panel-crash") | .message | test("status 4")] | all)' \
    "$plugins_status_path" > /dev/null \
    || fail "exactly the two plugins that exited should read as failed, the closed panel plugin not among them: $(cat "$plugins_status_path")"
  grep -qF "unknown panel" "$plugins_unknown_path" \
    || fail "a plugin that does not exist resolved as a panel: $(cat "$plugins_unknown_path")"
  grep -qxF 'before: 0' "$plugins_panel_procs_path" \
    || fail "the panel plugin ran before its card opened: $(cat "$plugins_panel_procs_path")"
  grep -qxF 'open: 1' "$plugins_panel_procs_path" \
    || fail "the panel plugin did not run while its card was open: $(cat "$plugins_panel_procs_path")"
  grep -qxF 'closed: 0' "$plugins_panel_procs_path" \
    || fail "the panel plugin kept running after its card closed: $(cat "$plugins_panel_procs_path")"
  [ "$(sed -n 1p "$plugins_panel_state_path")" = ok ] && [ "$(sed -n 2p "$plugins_panel_state_path")" = plugin:smoke-panel ] \
    || fail "panel open/state did not answer for the panel plugin: $(cat "$plugins_panel_state_path")"
  grep -qF '"event":"activate"' "$plugins_panel_events_path" 2>/dev/null \
    && grep -qF '"id":"r1"' "$plugins_panel_events_path" \
    || fail "Enter on the first row never reached the panel plugin: $(cat "$plugins_panel_events_path" 2>/dev/null)"
  grep -qF '"id":"t1","checked":true' "$plugins_panel_events_path" \
    || fail "Enter on the toggle row did not send its new state: $(cat "$plugins_panel_events_path" 2>/dev/null)"
  grep -qF '{"event":"activate","id":"b1"}' "$plugins_panel_events_path" \
    || fail "Enter on the button row never reached the panel plugin: $(cat "$plugins_panel_events_path" 2>/dev/null)"
  [ "$(wc -l < "$plugins_panel_events_path")" -eq 3 ] \
    || fail "one Enter per row should have sent exactly three events: $(cat "$plugins_panel_events_path")"
  [ "$(wc -l < "$plugins_panel_crash_starts_path" 2>/dev/null || echo 0)" -ge 2 ] \
    || fail "the crashed panel plugin was not started again on its backoff: $(cat "$plugins_panel_crash_starts_path" 2>/dev/null)"
  [ "$(sed -n 1p "$plugins_overlay_state_path")" = plugin:smoke-overlay ] && [ -z "$(sed -n 2p "$plugins_overlay_state_path")" ] \
    || fail "the overlay did not open on panel open and close on Escape: $(cat "$plugins_overlay_state_path")"
  grep -qF '"id":"ov1"' "$plugins_overlay_events_path" 2>/dev/null \
    || fail "Enter on the overlay's row never reached its plugin: $(cat "$plugins_overlay_events_path" 2>/dev/null)"
  echo "SMOKE_PLUGINS_PANEL $plugins_panel_png"
  echo "SMOKE_PLUGINS_PANEL_ACTIVATED $plugins_panel_after_png"
  echo "SMOKE_PLUGINS_PANEL_CRASH $plugins_panel_crash_png"
  echo "SMOKE_PLUGINS_OVERLAY $plugins_overlay_png"
  leg_plugins_assert_launcher
}

# `<step> <jq claim> <what it means>`, against that step's menu status.
plugins_launcher_expect() {
  local f="$shot_dir/plugins-launcher-$1.json"
  [ -s "$f" ] || fail "plugins: no launcher status for $1"
  jq -e "$2" "$f" > /dev/null || fail "plugins launcher $1: $3, got $(jq -c '{isOpen, level, token, query, ids: .ids[:6], actions}' "$f")"
}

leg_plugins_assert_launcher() {
  local events="$plugins_launcher_events_path" procs="$plugins_launcher_procs_path" step
  jq -e '.[] | select(.id == "smoke-launcher" and .kind == "launcher" and .mode == "Smoke Mode" and .subjects == ["file"])' "$plugins_list_path" > /dev/null \
    || fail "the launcher plugin did not resolve with its mode and subjects: $(cat "$plugins_list_path")"
  for step in "before: 0" "open: 1" "closed: 0" "after: 0"; do
    grep -qxF "$step" "$procs" || fail "the launcher plugin did not run exactly while the launcher was open ($step): $(cat "$procs")"
  done
  plugins_launcher_expect mode '.level == "plugin:smoke-launcher" and .token == ["Smoke Mode"] and .ids == ["plugin:smoke-launcher.alpha", "plugin:smoke-launcher.beta"]' "the plugin's mode did not stand as a token over its two rows"
  plugins_launcher_expect query '.query == "beta" and .ids == ["plugin:smoke-launcher.beta"]' "the plugin's answer to the query did not replace its rows"
  plugins_launcher_expect reopen '.level == "plugin:smoke-launcher" and .ids == ["plugin:smoke-launcher.alpha", "plugin:smoke-launcher.beta"]' "the mode opened again after the launcher shut did not send its query"
  plugins_launcher_expect root '.level == null and (.ids | index("plugin:smoke-launcher.alpha")) != null' "the plugin's rows did not rank in a root search"
  plugins_launcher_expect actions '.actionsOpen and (.actions | index("smoke-file")) != null and (.actions | index("open")) < (.actions | index("smoke-file"))' "the plugin's action did not join the file's column under its built-ins"
  jq -se '[.[] | select(.event == "query" and .text == "beta")] | length >= 1' "$events" > /dev/null \
    || fail "typing in the plugin's mode sent no query event: $(cat "$events")"
  jq -se '[.[] | select(.event == "activate" and .id == "beta" and (has("action") | not))] | length == 1' "$events" > /dev/null \
    || fail "Enter on the plugin's row did not send exactly one activate: $(cat "$events")"
  jq -se --arg p "$plugins_launcher_file" '[.[] | select(.event == "actions" and .subject.kind == "file" and .subject.path == $p and .subject.id == "file:" + $p)] | length == 1' "$events" > /dev/null \
    || fail "Tab over the file did not send one actions event with its kind and path: $(cat "$events")"
  jq -se --arg p "$plugins_launcher_file" '[.[] | select(.event == "activate" and .action == "smoke-file" and .id == "file:" + $p)] | length == 1' "$events" > /dev/null \
    || fail "running the plugin's action on the file did not reach it: $(cat "$events")"
  for step in mode query root actions; do
    [ -s "$shot_dir/plugins-launcher-$step.png" ] || fail "plugins: no launcher $step frame"
    echo "SMOKE_PLUGINS_LAUNCHER_SHOT $shot_dir/plugins-launcher-$step.png"
  done
  echo "SMOKE_PLUGINS_LAUNCHER a mode as a token, its query answered, its rows in a root search, its action on a file"
}
