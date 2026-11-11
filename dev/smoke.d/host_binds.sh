# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --host-binds: the command lines the owner's Hyprland binds
# run, in the shape the Lua config writes them (`formalshell ipc --any-display call ...`
# fifteen times (`screenshot full` takes its processing word, `default`, as `screenshot pick` does), `formalshell theme mode toggle` once), through the
# package's own `formalshell` entry point against the running shell. Each
# must exit 0 and answer something other than a missing target or function.
# `theme mode toggle` is also read back off `theme status`, and an unknown
# first word must exit non-zero without starting a second shell. The lock
# bind goes last: the session stays locked until the nested compositor is
# torn down.
leg_host_binds_flag="--host-binds"
leg_host_binds_order=215
leg_host_binds_claims="theme.mode=shared lock launcher=shared"
leg_host_binds_fixture_window=keep

host_binds_out="$shot_dir/host-binds.txt"

leg_host_binds_timing() {
  leg_timing 50
}

leg_host_binds_drive() {
  local script="$shot_dir/host-binds-drive.sh"
  local fs
  printf -v fs '%q' "$PWD/result/bin/formalshell"
  write_script "$script" <<EOF
#!/usr/bin/env bash
out="$host_binds_out"
: > "\$out"
run() {
  local reply code
  reply=\$("\$@" 2>&1); code=\$?
  printf 'CMD %s\nEXIT %s\nREPLY %s\n' "\$*" "\$code" "\$reply" >> "\$out"
}
before=\$(pgrep -fc 'formalshell-rs' || true)
run $fs theme status
run $fs theme mode toggle
sleep 2
run $fs theme status
run $fs ipc --any-display call switcher prev
run $fs ipc --any-display call switcher next
run $fs ipc --any-display call switcher commit
run $fs ipc --any-display call display brightnessStep 5
run $fs ipc --any-display call media playPause
run $fs ipc --any-display call media next
run $fs ipc --any-display call media previous
run $fs ipc --any-display call screenshot full default
run $fs ipc --any-display call screenshot pick smart default
run $fs ipc --any-display call menu toggle
run $fs ipc --any-display call menu summon emoji
run $fs ipc --any-display call menu summon clipboard
run $fs ipc --any-display call console toggle
run $fs bogus
after=\$(pgrep -fc 'formalshell-rs' || true)
printf 'DAEMONS %s %s\n' "\$before" "\$after" >> "\$out"
run $fs ipc --any-display call lock lock
EOF
  wm_exec_once "bash $script"
}

leg_host_binds_assert() {
  [ -s "$host_binds_out" ] || fail "host-binds produced no record"
  cat "$host_binds_out"
  local forms
  forms=$(grep -c '^CMD .* ipc --any-display call \|^CMD .* theme mode toggle$' "$host_binds_out")
  [ "$forms" -eq 15 ] || fail "expected 15 distinct host forms, ran $forms"
  awk '
    /^CMD /  { cmd = $0; next }
    /^EXIT / { code = $2; next }
    /^REPLY /{
      if (cmd ~ /ipc --any-display call|theme mode toggle/ && (code != 0 || $0 ~ /not found|required|usage:/)) {
        print "BAD: " cmd " -> exit " code ", " $0; bad = 1
      }
      if (cmd ~ / bogus$/ && (code == 0 || $0 !~ /usage:/)) { print "BAD: bogus " code; bad = 1 }
    }
    END { exit bad }
  ' "$host_binds_out" || fail "a host form did not reach its target (see above)"
  local before_mode after_mode
  before_mode=$(awk '/^CMD .* theme status$/{n++} n==1 && /^REPLY /{sub(/^REPLY /, ""); print; exit}' "$host_binds_out" | "$jq_bin" -r '.mode // empty' 2>/dev/null)
  after_mode=$(awk '/^CMD .* theme status$/{n++} n==2 && /^REPLY /{sub(/^REPLY /, ""); print; exit}' "$host_binds_out" | "$jq_bin" -r '.mode // empty' 2>/dev/null)
  [ -n "$before_mode" ] && [ "$before_mode" != "$after_mode" ] || fail "theme mode toggle left the mode at $before_mode -> $after_mode"
  local daemons
  daemons=$(grep '^DAEMONS ' "$host_binds_out")
  set -- $daemons
  [ "$2" = "$3" ] || fail "an unknown first word changed the daemon count: $daemons"
  echo "SMOKE_HOST_BINDS $host_binds_out"
}
