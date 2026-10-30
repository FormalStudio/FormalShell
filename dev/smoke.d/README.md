# Smoke legs

`dev/smoke.sh` is the scaffold (isolated HOME, settings fixture, private
bus, session mode, binaries, Hyprland config, shell launch, `SMOKE_OK`
frame, teardown) and sources every file here. A leg defines:

- `leg_<n>_flag` `"--flag"` or `"--flag <arg>"` (`leg_arg <n>` reads it);
  `leg_<n>_order` its place in the flags, fragments, autostart lines and
  results (default 500); `leg_<n>_needs` binaries to resolve.
- `leg_<n>_vm_only` `"<why>"` for a leg that needs root or a real system
  service (NetworkManager, logind, polkit): a nested run on a real host
  gets a private, empty system bus and refuses the leg with `SMOKE_FAIL`.
- `leg_<n>_budget` `"<why>"` for a leg whose numbers only hold on g815 (its
  CPU and frame budgets, or HyprExpo's x86_64-only hooks): the mac rig
  refuses it, and `dev/remote.sh` leaves it out of that rig's run lists.
- `leg_<n>_fixture_window` `keep` to leave the base run's fixture window in
  the frame; `leg_<n>_validate` usage checks, run before the build.
- `leg_<n>_fixture` `settings_fragment '<json>'` and staged files;
  `settings_pin '<jq assignment>'` for one key inside an object other legs
  write whole (`.bar.style = "solid"`), applied over the finished file, so
  a leg that rewrites settings.json mid-run drops it;
  `leg_<n>_timing` `leg_timing <delay> <timeout> [tail_gap]`, max-merged.
- `leg_<n>_drive` writes the drive scripts, echoes their autostart lines (`hypr_exec_once`),
  may `add_cleanup '<line>'`; `leg_<n>_assert` prints `SMOKE_*`, calls `fail`.
- `leg_<n>_wayland_debug=1` runs the shell under `WAYLAND_DEBUG`, so
  `$shell_log_path` carries its wire traffic for an assert that has to read
  what the shell sent rather than what the screen shows (`--frame`).
- `leg_<n>_shell <path>` writes the shell start script at `<path>` in place
  of the scaffold's, for a leg that runs some other build of the shell
  (`--installed`). It still logs to `$shell_log_path` and may write the
  shell's pid to `$shot_dir/shell.pid` for the memory sample.
- A fixture may set `shell_env` (`NAME=value ...`) for the shell's own
  environment and nothing else in the session (`--bluez-rssi` points the
  shell's system bus at a private one, which Hyprland must not follow).
- `leg_<n>_takeover` runs the whole thing itself and exits, for a leg that
  cannot share the one session (`--screensaver-gif` needs one per effect).
  It runs with the build done, the binaries resolved and the bus baseline
  taken, and owns its own `SMOKE_OK` line.

A leg reaches the shell only through `$ipc call <target> <fn> [args...]`,
never by hand: `$ipc` is `formalshell-ipc`, shell-quoted to paste into a
generated script unquoted. `$ipc_wrapper` is the package's own
`formalshell-ipc` entry point, for the leg that proves it (`--dump`).
Arguments follow the CLI11 rules the IPC contract was recorded under: one starting with `[` and ending
with `]` is split on commas, a `-x` word is refused, and a bare `show`,
`wait`, `listen` or `prop` switches subcommand. A `--` before the
arguments stops the last two; only a leading space stops the split.

Exported: `shot_dir`, `iso_home`, `shell_path`, `shell_log_path`, `*_bin`,
`ipc`, `ipc_wrapper`, `write_script`, `leg_on <n>`,
`host_notifications_owner_after`. Legs sharing a surface wait
on the owner's marker (`picker_done_path`); one covering the whole output
starts at its `<n>_t0`, past any desktop sampler (`--wallpaper` sets 16).
