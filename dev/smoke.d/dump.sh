# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --dump: the debug IPC target's whole state dump, taken in-session and
# printed on stdout so dev/remote.sh saves it as this run's JSON sidecar. Other
# legs read the same file for the settings the shell actually resolved.
#
# The reply is captured with stderr folded in, so "not empty" alone would
# green-light an error string. What the dump has to carry is live compositor
# state: the backend CompositorService picked, connected and streaming.
leg_dump_flag="--dump"
leg_dump_order=10
leg_dump_needs="jq"

dump_path="$shot_dir/dump.json"

leg_dump_drive() {
  local script="$shot_dir/dump.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 4
$ipc call debug dump > "$dump_path" 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_dump_assert() {
  if [ ! -s "$dump_path" ]; then
    fail "no debug dump produced"
  fi
  cat "$dump_path"; echo
  # One grep for both halves: `available` is the dump's own next key after
  # `compositor` (DebugIpc.dump), and it is also an audio/brightness key, so
  # matching it on its own would pass on a shell with no backend at all.
  if ! grep -qF '"compositor":"hyprland","available":true' "$dump_path"; then
    fail "the dump does not report a connected hyprland backend: $(cat "$dump_path")"
  fi
  # A backend that connected but never got its state populated reports an
  # empty list here, which is what an event stream that dropped looks like.
  if grep -qF '"workspaces":[]' "$dump_path"; then
    fail "the dump carries no workspaces, the backend connected but its state never populated: $(cat "$dump_path")"
  fi
  # The four keys the theme reads off settings.json, against the file the
  # shell loaded: a written value reads back, an absent or unknown one is
  # the default (`layout` bar, `bar.style` transparent, `motion.morph`
  # true, `render.backend` auto), and the backend drawing names its device
  # (gpu) or why it is not on one (cpu).
  "$jq_bin" -e '
    def pick($v; $allowed; $absent): if ($v | type) == "string" and ($allowed | index($v)) then $v else $absent end;
    .configLoaded as $c
    | .theme.layout == pick($c.layout; ["bar","sidebar"]; "bar")
    and .theme.barStyle == pick($c.bar.style; ["transparent","solid","corners"]; "transparent")
    and .theme.morph == (if ($c.motion.morph | type) == "boolean" then $c.motion.morph else true end)
    and .render.asked == pick($c.render.backend; ["auto","gpu","cpu"]; "auto")
    and ((.render.backend == "gpu" and .render.device != null) or (.render.backend == "cpu" and .render.reason != null))' \
    "$dump_path" > /dev/null \
    || fail "the dump's layout, barStyle, morph or render do not match settings.json: $("$jq_bin" -c '{layout: .theme.layout, barStyle: .theme.barStyle, morph: .theme.morph, render}' "$dump_path")"
}
