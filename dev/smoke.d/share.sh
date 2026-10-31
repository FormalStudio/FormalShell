# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --share proves the menu's SHARE route against the real packaged
# localsend-cli (M75 Task 5 restructured the route around it; M17's
# GUI-only shape is now the fallback, providers.js's sharePeerEntries).
#
# The launcher appends the result's helper directory (dev/build.sh), which
# carries localsend-cli, to whatever PATH it inherited, so the CLI is
# unconditionally present for anything launched through
# `result/bin/formalshell` in this rig: no PATH trick can take it away. That makes the CLI-present branch the only
# one this rig can honestly exercise end to end; the CLI-absent fallback
# shape (M17's Clipboard/Pick From History/Receive-launches-the-GUI rows)
# cannot be produced honestly on this rig, so it isn't reproduced here.
#
# What is under test: the route resolves and renders (a screenshot, read by
# eye, of the honest "No devices found" state -- this rig's isolated
# network genuinely has no second LocalSend peer to discover; a real
# two-instance transfer is Task 7's own dedicated leg), and the
# `localsend` IPC target's status/scan/peers round trip against the real
# CLI process LocalsendService actually spawns.
leg_share_flag="--share"
leg_share_order=230

share_status_path="$shot_dir/share-status.json"
share_menu_path="$shot_dir/share-menu.png"
share_scan_reply_path="$shot_dir/share-scan-reply.txt"
share_peers_path="$shot_dir/share-peers.json"

# This leg's own clock: the menu covers the whole output, so under
# --wallpaper it starts past that leg's last frame.
share_t0() {
  cover_t0 2
}

leg_share_timing() {
  local t0
  t0=$(share_t0)
  # `scan -t 4`'s own four-second wait plus a comfortable round-trip margin
  # either side.
  leg_timing $((12 + t0 - 2)) $((40 + t0 - 2))
  # Sharing the launcher surface with --menu costs this leg that leg's whole
  # timeline before it may touch the route at all.
  if leg_on menu; then leg_timing 42 65 3; fi
}

leg_share_drive() {
  local t0 script="$shot_dir/share-drive.sh" menu_wait=""
  t0=$(share_t0)
  if leg_on menu; then
    # menu-finish.sh's own last action writes the selection file, and its
    # pending select() would otherwise still be armed when this leg's own
    # `menu summon share` lands here.
    menu_wait="for _ in \$(seq 1 90); do [ -f \"$selection_path\" ] && break; sleep 0.5; done"
  fi
  write_script "$script" <<EOF
#!/usr/bin/env bash
$menu_wait
sleep $t0
$ipc call localsend status > "$share_status_path" 2>&1
$ipc call menu summon share > /dev/null 2>&1
sleep 1
"$grim_bin" "$share_menu_path" > /dev/null 2>&1
$ipc call menu close > /dev/null 2>&1
$ipc call localsend scan > "$share_scan_reply_path" 2>&1
sleep 5
$ipc call localsend peers > "$share_peers_path" 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_share_assert() {
  if [ ! -s "$share_status_path" ]; then
    fail "no localsend status reply produced"
  fi
  cat "$share_status_path"; echo
  if ! grep -qF '"installed":true' "$share_status_path"; then
    fail "localsend status never reported installed:true; the packaged localsend-cli should always resolve through the formalshell wrapper's own PATH. Got: $(cat "$share_status_path")"
  fi

  if [ ! -f "$share_menu_path" ]; then
    fail "no share-menu screenshot produced"
  fi
  echo "SMOKE_SHARE_MENU $share_menu_path"

  if [ ! -s "$share_scan_reply_path" ]; then
    fail "no reply to 'localsend scan'"
  fi
  cat "$share_scan_reply_path"; echo
  if ! grep -qF "ok" "$share_scan_reply_path"; then
    fail "'localsend scan' did not answer ok. Got: $(cat "$share_scan_reply_path")"
  fi

  if [ ! -s "$share_peers_path" ]; then
    fail "no reply to 'localsend peers'"
  fi
  cat "$share_peers_path"; echo
  # A JSON array is the whole claim: a stray LocalSend peer answering the
  # isolated VM's multicast scan would be a flake here, not a bug, so this
  # never asserts the rig is peer-free, only that the round trip works.
  if ! grep -qE '^\[.*\]$' "$share_peers_path"; then
    fail "'localsend peers' did not answer a JSON array. Got: $(cat "$share_peers_path")"
  fi
}
