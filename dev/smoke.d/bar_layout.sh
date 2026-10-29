# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --bar-layout points bar.layout at a left region led by six bar.modules
# entries, swapped ahead of the reordered builtins (activeWindow before
# workspaces, away from the default): one `command` module printing known
# Waybar-JSON, four more each exercising one of the command module's failure
# paths (non-zero exit, malformed JSON, a run outliving its timeout, a binary
# that does not exist), one printing an empty SUCCESS payload (a real answer
# that must render NO cell rather than an empty bordered box).
#
# No drive step decides any of that: Bar/layout.js resolves it from
# settings.json at startup, so the frame is the whole claim. Every other leg
# leaves the `bar` key out entirely, which is itself the no-config-fallback
# proof.
leg_bar_layout_flag="--bar-layout"
leg_bar_layout_order=190
leg_bar_layout_needs="jq"

bar_layout_path="$shot_dir/bar-layout.png"
bar_cmd_fixture_path="$shot_dir/bar-cmd-fixture.sh"
bar_cmd_fail_path="$shot_dir/bar-cmd-fail.sh"
bar_cmd_badjson_path="$shot_dir/bar-cmd-badjson.sh"
bar_cmd_timeout_path="$shot_dir/bar-cmd-timeout.sh"
bar_cmd_empty_path="$shot_dir/bar-cmd-empty.sh"
bar_gh_shim_dir="$shot_dir/gh-shim"
bar_layout_room_path="$shot_dir/bar-layout-room.json"

leg_bar_layout_fixture() {
  write_script "$bar_cmd_fixture_path" <<'EOF'
#!/usr/bin/env bash
printf '{"text": "CMD 42", "tooltip": "smoke fixture tooltip", "class": "warning"}'
EOF
  # Empty SUCCESS, distinct from every failure below it: exit 0 carrying a
  # well-formed payload whose text is "". That is a real answer ("nothing to
  # report"), which is what `dualsense-bar` prints with no controller paired,
  # and it must render no cell at all. Shipped broken until 2026-08-18; the
  # four failure fixtures could not catch it because each renders MODULE
  # ERROR.
  write_script "$bar_cmd_empty_path" <<'EOF'
#!/usr/bin/env bash
printf '{"text": "", "tooltip": "", "class": ""}'
EOF
  # Three of the four failure paths. The fourth, a command path that does not
  # exist at all, needs no script: cmdmissing's own settings entry below
  # points straight at a path under $shot_dir that is never created. Each
  # must render MODULE ERROR rather than staying blank.
  write_script "$bar_cmd_fail_path" <<'EOF'
#!/usr/bin/env bash
printf '{"text": "should not render"}'
exit 1
EOF
  write_script "$bar_cmd_badjson_path" <<'EOF'
#!/usr/bin/env bash
printf 'not json'
EOF
  write_script "$bar_cmd_timeout_path" <<'EOF'
#!/usr/bin/env bash
sleep 5
printf '{"text": "too late"}'
EOF
  # A canned `gh api graphql` answer in the exact shape the GitHub panel's
  # combined query returns, so the leading `github` cell proves the whole
  # poll -> parse -> "3/2" path without network or auth. Real gh behaviour
  # (auth, exit code 4) stays host-trial territory. Exported onto this
  # script's own PATH rather than spliced into a launch command: dev/smoke.sh
  # owns the shell's autostart line, and `env` hands the session whatever
  # environment this process carries. A directory holding nothing but `gh`
  # shadows nothing else the run resolves.
  mkdir -p "$bar_gh_shim_dir"
  write_script "$bar_gh_shim_dir/gh" <<'EOF'
#!/usr/bin/env bash
if [ "${1:-}" = "api" ]; then
  printf '%s\n' '{"data":{"viewer":{"login":"formalsnake"},"prs":{"issueCount":3,"nodes":[{"title":"Sort workspaces by idx","url":"https://github.com/formalshell/formalshell/pull/101","repository":{"nameWithOwner":"formalshell/formalshell"}},{"title":"Tray dbusmenu support","url":"https://github.com/formalshell/formalshell/pull/102","repository":{"nameWithOwner":"formalshell/formalshell"}},{"title":"Panel motion tokens","url":"https://github.com/formalshell/formalshell/pull/103","repository":{"nameWithOwner":"formalshell/formalshell"}}]},"issues":{"issueCount":2,"nodes":[{"title":"Calendar day selection","url":"https://github.com/formalshell/formalshell/issues/201","repository":{"nameWithOwner":"formalshell/formalshell"}},{"title":"Emoji picker should paste","url":"https://github.com/formalshell/formalshell/issues/202","repository":{"nameWithOwner":"formalshell/formalshell"}}]}}}'
  exit 0
fi
exit 1
EOF
  export PATH="$bar_gh_shim_dir:$PATH"
  # The distro mark is the default, so this leg names it only to keep the
  # assertion honest if that default ever changes: the rig's isolated session
  # sees no distributor-logo icon theme, which is exactly the path that has to
  # reach the bundled font-logos table for a real logo.
  settings_fragment ', "bar": {"launcherIcon": "distro", "layout": {"left": ["launcher", "github", "custom:cmdfixture", "custom:cmdfail", "custom:cmdbadjson", "custom:cmdtimeout", "custom:cmdmissing", "custom:cmdempty", "activeWindow", "workspaces"]}, "modules": [{"id": "cmdfixture", "type": "command", "command": ["bash", "'"$bar_cmd_fixture_path"'"], "interval": 2000}, {"id": "cmdfail", "type": "command", "command": ["bash", "'"$bar_cmd_fail_path"'"], "interval": 20000}, {"id": "cmdbadjson", "type": "command", "command": ["bash", "'"$bar_cmd_badjson_path"'"], "interval": 20000}, {"id": "cmdtimeout", "type": "command", "command": ["bash", "'"$bar_cmd_timeout_path"'"], "interval": 20000, "timeout": 1000}, {"id": "cmdmissing", "type": "command", "command": ["'"$shot_dir"'/no-such-formalshell-smoke-binary"], "interval": 20000}, {"id": "cmdempty", "type": "command", "command": ["bash", "'"$bar_cmd_empty_path"'"], "interval": 20000}]}'
}

leg_bar_layout_timing() {
  leg_timing 12 40
}

leg_bar_layout_drive() {
  local script="$shot_dir/bar-layout-drive.sh"
  # 5s: shell startup plus room for the command modules' first poll, which
  # fires as soon as Config.settings resolves, and past the 1s timeout the
  # cmdtimeout module has to blow through. Named separately from the run's own
  # smoke.png so nobody has to remember which run produced that one.
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 5
"$grim_bin" "$bar_layout_path" > /dev/null 2>&1
$ipc call bar room > "$bar_layout_room_path" 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_bar_layout_assert() {
  if [ ! -f "$bar_layout_path" ]; then
    fail "no bar-layout screenshot produced"
  fi
  # A settings.json that does not parse falls back to the default layout,
  # which photographs fine; the left region has to be the one asked for.
  jq -e '[.[0].cells[] | select(.region == "left") | .name] | index("custom:cmdfixture") != null' \
    "$bar_layout_room_path" > /dev/null \
    || fail "the bar did not take the layout from settings.json: $(cat "$bar_layout_room_path")"
  echo "SMOKE_BAR_LAYOUT $bar_layout_path"
}
