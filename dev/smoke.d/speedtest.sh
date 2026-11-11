# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --speedtest opens the network panel (so the run's own frame shows the SPEED
# TEST section) and drives `network speedtest` to completion. The two bounded
# 5s phases settle `phase` to "done" regardless of real transfer speed; the
# poll ceiling is a safety net, not the expected path.
#
# Honest bifurcation is the contract: "done" with real numbers is the
# expected result on a VM with NAT internet, and an honest NO NETWORK / NO
# CURL abort (which also settles to "done") is equally real evidence. Only a
# poll ceiling reached with neither fails the run.
leg_speedtest_flag="--speedtest"
leg_speedtest_order=200
leg_speedtest_needs="jq"
leg_speedtest_claims="panel=shared"

speedtest_panel_path="$shot_dir/speedtest-panel.png"
speedtest_status_path="$shot_dir/speedtest-status.json"

leg_speedtest_timing() {
  leg_timing 60
}

leg_speedtest_drive() {
  local script="$shot_dir/speedtest-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
$ipc call panel open network > /dev/null 2>&1
until_settled
$ipc call network speedtest > /dev/null 2>&1
until_jq 25 "$speedtest_status_path" '.phase == "done"' call network speedstatus
"$grim_bin" "$speedtest_panel_path" > /dev/null 2>&1
EOF
  wm_exec_once "bash $script"
}

leg_speedtest_assert() {
  [ -f "$speedtest_panel_path" ] || fail "no speedtest-panel screenshot produced"
  echo "SMOKE_SPEEDTEST_PANEL $speedtest_panel_path"
  if [ ! -s "$speedtest_status_path" ]; then
    fail "no network speedstatus produced"
  fi
  cat "$speedtest_status_path"; echo
  expect_jq "$speedtest_status_path" '.phase == "done"' "speed test never reached phase:done"
  if "$jq_bin" -e '.error == ""' "$speedtest_status_path" > /dev/null 2>&1; then
    echo "speedtest: reached done with a real measurement"
  else
    echo "speedtest: reached done via an honest failure surface"
  fi
}
