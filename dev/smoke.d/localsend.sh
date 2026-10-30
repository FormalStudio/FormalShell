# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, iso_home, the *_bin paths and fail()
# --localsend: a real loopback transfer into the shell's own receiver, the
# check --share's own header defers to this leg for. `localsend.receive`
# pins the shell's `localsend-cli recv` child to a fixture directory, and
# this leg's drive script runs a SECOND, independent `localsend-cli send`
# process (the same packaged binary, resolved a second time by
# need_localsend_cli below) against it over `--ip 127.0.0.1`, standing in
# for a real second device the way --wifi's two real hostapd radios do for
# a real access point.
#
# What is under test is the shell's receiver end to end: LocalsendService
# spawns the real CLI, the file lands in the real fixture directory over the
# real LocalSend v2 HTTPS handshake (see the service's header on why no
# `--https=false`), `recv`'s own `Recv file` log line raises the RECEIVED toast, and
# files something else writes into the same directory (a browser download,
# here a `cp` and a `.part` temp name before the transfer and a second
# `cp` after it) raise none: exactly one toast for the run. A sha256 match against the
# source file is the one claim a screenshot cannot make: a truncated or
# corrupted transfer would still produce a same-named file and a toast.
#
# It runs on the VM rig: the port below is fixed, and a nested run on a
# real host shares that host's network with the owner's own shell, whose
# receiver already holds it.
#
# The reverse direction -- the shell's own `localsend send` IPC route
# against a second real `recv` -- is NOT run here. 0w0mewo/localsend-cli's
# `recv` and `send` (cmd/recv/recv.go, internal/models/discovery.go
# NewDeviceInfo) carry no port flag at all: the receive port is the
# hardcoded LocalSend default, 53317, so a second `recv` on this same host
# can only ever collide with the first one already bound to it. A real
# second-device test of that direction needs a second host, which is
# g815/e1504g territory (CLAUDE.md's macOS verification loop section), not
# this rig; `--share`'s own `localsend send`/`localsend peers` IPC round
# trip is what this rig can honestly still prove of that half.
leg_localsend_flag="--localsend"
leg_localsend_order=231
leg_localsend_vm_only="localsend-cli fixes its receive port at 53317, which a host's own shell holds"
leg_localsend_needs="localsend-cli jq"

localsend_receive_dir="$shot_dir/localsend-receive"
localsend_payload_name="fixture-payload.txt"
localsend_payload_path="$shot_dir/$localsend_payload_name"
localsend_send_out_path="$shot_dir/localsend-send-out.txt"
localsend_status_before_path="$shot_dir/localsend-status-before.json"
localsend_notify_before_path="$shot_dir/localsend-notify-before.json"
localsend_notify_after_path="$shot_dir/localsend-notify-after.json"
localsend_received_sha_path="$shot_dir/localsend-received.sha256"
localsend_received_png="$shot_dir/localsend-received.png"

leg_localsend_fixture() {
  settings_fragment ', "localsend": {"receive": true, "alias": "FormalShell Fixture", "dir": "'"$localsend_receive_dir"'"}'
  mkdir -p "$localsend_receive_dir"
  printf 'M75 localsend loopback fixture, run at %s\n' "$(date -u +%s)" > "$localsend_payload_path"
}

leg_localsend_timing() {
  # screenshot_delay has to clear this leg's own last action (the send, the
  # up-to-30s poll for the file landing, and the notify status check),
  # since the base run's teardown (shot.sh) fires at that mark and tears
  # the session down under whatever the drive script is still doing.
  leg_timing 50 80
}

leg_localsend_drive() {
  local script="$shot_dir/localsend-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 3
$ipc call localsend status > "$localsend_status_before_path" 2>&1
$ipc call notifications status > "$localsend_notify_before_path" 2>&1

# "receiving" means the child is running, not that its HTTPS server answers
# yet (cert generation comes first); a send before that is dropped with
# "server closed connection before returning the first response byte".
for _ in \$(seq 1 40); do
  curl -skf -o /dev/null https://127.0.0.1:53317/api/localsend/v2/info && break
  sleep 0.5
done
cp "$localsend_payload_path" "$localsend_receive_dir/browser-download.txt"
: > "$localsend_receive_dir/browser-download.bin.part"
"$localsend_cli_bin" send --ip 127.0.0.1 -f "$localsend_payload_path" > "$localsend_send_out_path" 2>&1

for _ in \$(seq 1 30); do
  [ -f "$localsend_receive_dir/$localsend_payload_name" ] && break
  sleep 1
done
sha256sum "$localsend_receive_dir/$localsend_payload_name" > "$localsend_received_sha_path" 2>/dev/null

cp "$localsend_payload_path" "$localsend_receive_dir/browser-download-late.txt"
sleep 5
$ipc call notifications status > "$localsend_notify_after_path" 2>&1
"$grim_bin" "$localsend_received_png" > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_localsend_assert() {
  local total_before total_after expected_sha got_sha

  if [ ! -s "$localsend_status_before_path" ]; then fail "no localsend status reply"; fi
  cat "$localsend_status_before_path"; echo
  if ! grep -qF '"installed":true' "$localsend_status_before_path" || ! grep -qF '"receiving":true' "$localsend_status_before_path"; then
    fail "localsend status never reported a running receiver: $(cat "$localsend_status_before_path")"
  fi

  if [ ! -s "$localsend_send_out_path" ]; then fail "no output from the second localsend-cli send process"; fi
  cat "$localsend_send_out_path"
  if grep -q 'level=ERROR' "$localsend_send_out_path"; then
    fail "the loopback send logged an error: $(cat "$localsend_send_out_path")"
  fi

  if [ ! -f "$localsend_receive_dir/$localsend_payload_name" ]; then
    fail "the payload never landed in the receive directory: $localsend_receive_dir/$localsend_payload_name"
  fi
  expected_sha=$(sha256sum "$localsend_payload_path" | awk '{print $1}')
  got_sha=$(awk '{print $1}' "$localsend_received_sha_path" 2>/dev/null)
  echo "sha256 sent=$expected_sha received=$got_sha"
  if [ -z "$got_sha" ] || [ "$got_sha" != "$expected_sha" ]; then
    fail "the received file is not byte-identical to what was sent: sent=$expected_sha received=$got_sha"
  fi

  total_before=$("$jq_bin" -r '.pending + .popups' "$localsend_notify_before_path" 2>/dev/null)
  total_after=$("$jq_bin" -r '.pending + .popups' "$localsend_notify_after_path" 2>/dev/null)
  echo "notification centre totals (pending+popups): before=$total_before after=$total_after"
  [ -n "$total_before" ] && [ -n "$total_after" ] || fail "no notifications status reply to read a RECEIVED toast off"
  [ "$total_after" -eq $((total_before + 1)) ] || fail "expected exactly one LOCALSEND RECEIVED toast (the real transfer, none for the three foreign files): $total_before -> $total_after"

  if [ ! -f "$localsend_received_png" ]; then fail "no localsend-received screenshot produced"; fi
  echo "SMOKE_LOCALSEND_RECEIVED $localsend_received_png"
}
