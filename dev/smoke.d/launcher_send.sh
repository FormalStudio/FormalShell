# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, iso_home, the *_bin paths and fail()
# --launcher-send: the launcher's `Send to…` (launcher spec §5) on a
# clipboard entry and on a file, against a second, independent
# `localsend-cli recv` standing in for another device, read off `menu
# status` (`actions`, `token`) and the files the peer writes. The peer runs
# in a network namespace of its own behind a veth pair, with the multicast
# range routed onto it: LocalSend finds no peer on its own host (a `recv`
# beside the scan is never listed), and its port is fixed at 53317, which
# the host's own receiver may hold. The namespace takes sudo, hence the VM
# rig.
#
# With no peer up, `Send to…` on the clipboard entry lists the one honest
# note once the scan is over (`peer:none`, No devices found). With the peer
# up, the list fills from the next scan under a `Send to` token, a real
# Enter on the peer sends the copied text and it lands in the peer's
# directory word for word; a recent file's `Send to…` then lands
# byte-identical (sha256). The empty list and the peers are photographed.
leg_launcher_send_flag="--launcher-send"
leg_launcher_send_order=33
leg_launcher_send_vm_only="its stand-in peer runs in a network namespace it makes with sudo"
leg_launcher_send_needs="jq wtype wl-copy localsend-cli"

launcher_send_text="launcher send probe $(date -u +%s)"
launcher_send_peer_dir="$shot_dir/launcher-send-peer"
launcher_send_file="$iso_home/Documents/send-probe.txt"

leg_launcher_send_fixture() {
  mkdir -p "$iso_home/Documents" "$launcher_send_peer_dir" "$iso_home/.local/share"
  { echo "launcher send probe file"; seq 1 200; } > "$launcher_send_file"
  cat > "$iso_home/.local/share/recently-used.xbel" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<xbel version="1.0" xmlns:bookmark="http://www.freedesktop.org/standards/desktop-bookmarks" xmlns:mime="http://www.freedesktop.org/standards/shared-mime-info">
  <bookmark href="file://$launcher_send_file" added="2026-10-30T10:00:00Z" modified="2026-10-30T10:00:00Z" visited="2026-10-30T10:00:00Z"/>
</xbel>
EOF
}

leg_launcher_send_timing() {
  leg_timing 90 130
}

leg_launcher_send_drive() {
  local script="$shot_dir/launcher-send-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
call() { $ipc call "\$@"; }
status() { call menu status > "$shot_dir/launcher-send-\$1.json" 2>&1; }
shoot() { "$grim_bin" "$shot_dir/launcher-send-\$1.png" > /dev/null 2>&1; }
# Polls status into step \$1 until jq claim \$2 holds, for at most \$3 seconds.
until_status() {
  SECONDS=0
  while [ "\$SECONDS" -lt "\$3" ]; do
    status "\$1"
    "$jq_bin" -e "\$2" "$shot_dir/launcher-send-\$1.json" > /dev/null 2>&1 && return 0
    sleep 0.5
  done
}
# Send to… on the first clipboard entry, the targets settled into step \$1.
clip_send() {
  call menu summon clipboard > /dev/null 2>&1
  sleep 1
  status "\$1-list"
  call menu runAction 0 send-to > /dev/null 2>&1
  until_status "\$1" "\$2" 20
}
sleep 4
"$wl_copy_bin" -- "$launcher_send_text"
sleep 1.5

clip_send empty '.actions != ["peer:scanning"]'
shoot empty
call menu close > /dev/null 2>&1
sleep 1

sudo ip netns add fs-send-peer
sudo ip link add fs-send-host type veth peer name fs-send-peer
sudo ip link set fs-send-peer netns fs-send-peer
sudo ip addr add 10.77.0.1/24 dev fs-send-host
sudo ip link set fs-send-host up
sudo ip route add 224.0.0.0/4 dev fs-send-host
sudo ip netns exec fs-send-peer ip addr add 10.77.0.2/24 dev fs-send-peer
sudo ip netns exec fs-send-peer ip link set fs-send-peer up
sudo ip netns exec fs-send-peer ip link set lo up
sudo ip netns exec fs-send-peer ip route add default via 10.77.0.1
sudo ip netns exec fs-send-peer sudo -u "\$(id -un)" "$localsend_cli_bin" recv -n "Send Probe Peer" -d "$launcher_send_peer_dir" > "$shot_dir/launcher-send-peer.log" 2>&1 &
for _ in \$(seq 1 40); do
  curl -skf -o /dev/null https://10.77.0.2:53317/api/localsend/v2/info && break
  sleep 0.5
done

clip_send peers '.actions | index("peer:0") != null'
shoot peers
"$wtype_bin" -k Return
for _ in \$(seq 1 30); do
  ls "$launcher_send_peer_dir"/localsend-text-*.txt > /dev/null 2>&1 && break
  sleep 1
done
sleep 1
cat "$launcher_send_peer_dir"/localsend-text-*.txt > "$shot_dir/launcher-send-text-received.txt" 2>/dev/null
status text-sent

call menu summon "" > /dev/null 2>&1
sleep 0.5
call menu filter send-probe > /dev/null 2>&1
sleep 1.5
status file-list
i=\$("$jq_bin" '[.ids[] | startswith("files.")] | index(true) // -1' "$shot_dir/launcher-send-file-list.json")
call menu runAction "\$i" send-to > /dev/null 2>&1
until_status file-peers '.actions | index("peer:0") != null' 20
"$wtype_bin" -k Return
for _ in \$(seq 1 30); do
  [ -f "$launcher_send_peer_dir/send-probe.txt" ] && break
  sleep 1
done
sleep 1
sha256sum "$launcher_send_peer_dir/send-probe.txt" > "$shot_dir/launcher-send-file.sha256" 2>/dev/null
sudo ip netns pids fs-send-peer | xargs -r sudo kill
sudo ip netns del fs-send-peer
sudo ip link del fs-send-host
EOF
  hypr_exec_once "bash $script"
}

# `<step> <jq claim> <what it means>`, against that step's status.
launcher_send_expect() {
  local f="$shot_dir/launcher-send-$1.json"
  [ -s "$f" ] || fail "launcher-send: no status for $1"
  "$jq_bin" -e "$2" "$f" > /dev/null || fail "launcher-send $1: $3, got $("$jq_bin" -c '{isOpen, level, token, actionsOpen, actions, ids: .ids[:4]}' "$f")"
}

leg_launcher_send_assert() {
  launcher_send_expect empty-list '.level == "clipboard" and (.ids[0] | startswith("clipboard."))' "the copied text is not the clipboard mode's first row"
  launcher_send_expect empty '.actionsOpen and .actions == ["peer:none"]' "with no peer up, Send to did not settle on No devices found"
  echo "SMOKE_LAUNCHER_SEND no peer: one honest note"

  echo "peer: $(cat "$shot_dir/launcher-send-peer.log" 2>&1)"
  launcher_send_expect peers '.actionsOpen and (.actions | index("peer:0")) != null and (.actions | all(startswith("peer:") and . != "peer:none" and . != "peer:scanning"))' "the peer never joined the Send to list"
  [ -s "$shot_dir/launcher-send-text-received.txt" ] || fail "launcher-send: the peer never received the clipboard text ($(ls "$launcher_send_peer_dir"))"
  [ "$(cat "$shot_dir/launcher-send-text-received.txt")" = "$launcher_send_text" ] \
    || fail "launcher-send: the peer received '$(cat "$shot_dir/launcher-send-text-received.txt")', not the copied text"
  launcher_send_expect text-sent '.isOpen | not' "a pick off Send to did not close the launcher"
  echo "SMOKE_LAUNCHER_SEND Enter on the peer delivered the clipboard text"

  launcher_send_expect file-peers '.actionsOpen and (.actions | index("peer:0")) != null' "a file's Send to did not list the peer"
  local want got
  want=$(sha256sum "$launcher_send_file" | awk '{print $1}')
  got=$(awk '{print $1}' "$shot_dir/launcher-send-file.sha256" 2>/dev/null)
  [ -n "$got" ] && [ "$got" = "$want" ] || fail "launcher-send: the sent file is not byte-identical: sent=$want received=${got:-nothing}"
  echo "SMOKE_LAUNCHER_SEND a recent file landed byte-identical"

  local shot
  for shot in empty peers; do
    [ -s "$shot_dir/launcher-send-$shot.png" ] || fail "launcher-send: no $shot frame"
    echo "SMOKE_LAUNCHER_SEND_SHOT $shot_dir/launcher-send-$shot.png"
  done
}
