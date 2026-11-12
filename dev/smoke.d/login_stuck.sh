# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --login-stuck: --login-switch's run (dev/smoke.d/login_switch.sh) with
# the keyring stand-in never taking the shell's connection and the
# compositor stopped (SIGSTOP) as the real password is entered. The
# handover goes on without the keyring 5s later, and the Exit it sends
# goes unserved: 10s on, the lock is still held, the session marker is
# gone and the error is up. The compositor is let go after and ends on
# that late Exit, and the login script, finding no marker, starts nothing
# else.
leg_login_stuck_flag="--login-stuck"
leg_login_stuck_order=115
leg_login_stuck_compositors="hyprland shoji"
leg_login_stuck_needs="wtype python3"
leg_login_stuck_claims="session"

leg_login_stuck_fixture() { login_switch_fixture hang; }
leg_login_stuck_cleanup() { login_switch_cleanup; }
leg_login_stuck_timing() { leg_timing 60; }
leg_login_stuck_session() { login_switch_session; }
leg_login_stuck_drive() { login_switch_drive "$shot_dir/login-stuck-drive.sh" yes; }

leg_login_stuck_assert() {
  login_switch_menu_assert
  cat "$login_switch_stuck_json"; echo
  expect_jq "$login_switch_stuck_json" '.locked == true and .authError == "Could not end the session" and .leavingFor == null' "the lock did not take the pick back from a compositor that stayed" '{locked, authError, leavingFor}'
  [ ! -s "$login_switch_keyring_txt" ] || fail "the keyring stand-in answered: $(cat "$login_switch_keyring_txt")"
  grep -q '^lock: the login keyring did not answer in 5s$' "$shell_log_path" || fail "the handover did not go on without the keyring"
  if grep -q '^lock: login keyring' "$shell_log_path"; then fail "the shell had a keyring answer the stand-in never gave"; fi
  cat "$login_switch_stuck_txt"
  grep -qx marker=gone "$login_switch_stuck_txt" || fail "the session marker outlived the compositor's turn"
  grep -qE '^origin=T' "$login_switch_stuck_txt" || fail "the compositor was not held stopped through the wait"
  grep -q '^lock: the compositor did not end' "$shell_log_path" || fail "the shell did not log the compositor staying"
  [ ! -e "$login_switch_dest_txt" ] || fail "the login script started the pick after the lock took it back"
  [ ! -e "$login_switch_marker" ] || fail "a session marker is left after the run"
}
