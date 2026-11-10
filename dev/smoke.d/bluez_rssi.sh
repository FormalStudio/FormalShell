# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --bluez-rssi: rides --idle. The shell's system bus is a private
# dbus-daemon of the leg's own, where dev/bluez-stub.py plays g815's BlueZ
# at idle: an adapter discovering on a scan the shell did not start, a
# paired device and strangers, ~700 RSSI-only PropertiesChanged a minute
# and a new stranger every 10 s. Through --idle's 60 s window the shell
# commits nothing past the clock's minute, calls nothing on the stub, and
# spends under 5 ticks (0.1% of a core) in all: the paired device's own
# share of the RSSI still arrives, since no match rule can tell a Device1
# change carrying RSSI from one carrying Connected.
# After it `bluetooth status` lists the paired device alone, and with the
# panel open the strangers too, so the quiet is a filter and not a dead
# connection.
#
# --idle runs its turn first, so the stub's count and calls are sampled
# with their time from the session's start (bluez-rssi-samples.txt) and
# read at the idle window's marks afterwards.
leg_bluez_rssi_flag="--bluez-rssi"
leg_bluez_rssi_order=902
leg_bluez_rssi_needs=""
leg_bluez_rssi_claims="bus.system"

bluez_rssi_bus="$shot_dir/bluez-bus"
bluez_rssi_count_path="$shot_dir/bluez-rssi-count.txt"
bluez_rssi_log_path="$shot_dir/bluez-stub.log"
bluez_rssi_results_path="$shot_dir/bluez-rssi-results.txt"
bluez_rssi_samples_path="$shot_dir/bluez-rssi-samples.txt"

leg_bluez_rssi_validate() {
  if ! leg_on idle; then
    echo "usage: $0 --idle --bluez-rssi" >&2
    exit 1
  fi
}

leg_bluez_rssi_fixture() {
  need_python3
  local daemon pid
  daemon=$(command -v dbus-daemon) || fail "--bluez-rssi: no dbus-daemon"
  pid=$("$daemon" --session --address="unix:path=$bluez_rssi_bus" --fork --print-pid --nopidfile)
  add_cleanup "kill $pid 2>/dev/null"
  "$python3_bin" dev/bluez-stub.py --address "unix:path=$bluez_rssi_bus" --rate 700 \
    --count "$bluez_rssi_count_path" > "$bluez_rssi_log_path" 2>&1 &
  add_cleanup "kill $! 2>/dev/null"
  for _ in $(seq 50); do
    [ -s "$bluez_rssi_count_path" ] && break
    sleep 0.1
  done
  [ -s "$bluez_rssi_count_path" ] || fail "--bluez-rssi: the stub never emitted"
  (
    set +e
    while :; do
      printf '%s %s %s\n' "$(date +%s%N)" "$(cat "$bluez_rssi_count_path")" "$(grep -c unhandled "$bluez_rssi_log_path")"
      sleep 0.2
    done
  ) > "$bluez_rssi_samples_path" 2>/dev/null &
  add_cleanup "kill $! 2>/dev/null"
  shell_env="DBUS_SYSTEM_BUS_ADDRESS=unix:path=$bluez_rssi_bus"
}

leg_bluez_rssi_drive() {
  local script="$shot_dir/bluez-rssi-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
# The last whole sample at or before each of the idle window's marks (the
# stub rewrites its count file, so a read can land on it empty).
at() {
  local t
  t=\$(awk -v m="\$1" '\$1 == m { print \$2 }' "$shot_dir/idle-marks.txt")
  awk -v t="\$t" 'NF == 3 && \$1 <= t { s = \$2; c = \$3 } END { print s, c }' "$bluez_rssi_samples_path"
}
read -r s0 c0 < <(at idle-start)
read -r s1 c1 < <(at idle-end)
printf 'signals_before %s\ncalls_before %s\nsignals_after %s\ncalls_after %s\n' "\$s0" "\$c0" "\$s1" "\$c1" > "$bluez_rssi_results_path"
$ipc call bluetooth status > "$shot_dir/bluez-status-shut.json" 2>&1
$ipc call panel open bluetooth > /dev/null 2>&1
until_settled
$ipc call bluetooth status > "$shot_dir/bluez-status-open.json" 2>&1
$ipc call panel close > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_bluez_rssi_assert() {
  local s0 s1 c0 c1 ticks commits shut open
  s0=$(awk '/^signals_before/{print $2}' "$bluez_rssi_results_path")
  s1=$(awk '/^signals_after/{print $2}' "$bluez_rssi_results_path")
  c0=$(awk '/^calls_before/{print $2}' "$bluez_rssi_results_path")
  c1=$(awk '/^calls_after/{print $2}' "$bluez_rssi_results_path")
  ticks=$(awk '/^total ticks=/{split($2, a, "="); print a[2]}' "$idle_results_path")
  commits=$(awk '/^commits /{print $2}' "$idle_results_path")
  shut=$(cat "$shot_dir/bluez-status-shut.json")
  open=$(cat "$shot_dir/bluez-status-open.json")
  echo "SMOKE_BLUEZ_RSSI signals=$((s1 - s0)) ticks=$ticks commits=$commits calls=$((c1 - c0))"
  echo "SMOKE_BLUEZ_RSSI shut=$shut"
  echo "SMOKE_BLUEZ_RSSI open=$open"
  [ $((s1 - s0)) -ge 600 ] || fail "--bluez-rssi: the stub sent $((s1 - s0)) signals in the window, not ~700"
  [ "$ticks" -lt 5 ] || fail "--bluez-rssi: the shell spent $ticks ticks idle under the scan"
  [ "$commits" -le 1 ] || fail "--bluez-rssi: the shell committed $commits frames idle under the scan"
  [ $((c1 - c0)) = 0 ] || fail "--bluez-rssi: the shell called the stub idle"
  case "$shut" in
    *'"AA:BB:CC:DD:EE:FF"'*'11:22:33'*) fail "--bluez-rssi: strangers listed with the panel shut" ;;
    *'"AA:BB:CC:DD:EE:FF"'*) ;;
    *) fail "--bluez-rssi: the paired device is missing from status: $shut" ;;
  esac
  case "$open" in
    *'11:22:33:44:55:'*) ;;
    *) fail "--bluez-rssi: no stranger listed with the panel open: $open" ;;
  esac
}
