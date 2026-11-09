# Smoke legs

`dev/smoke.sh` is the scaffold (isolated HOME, settings fixture, private
bus, session mode, binaries, Hyprland config, shell launch, `SMOKE_OK`
frame, teardown) and sources every file here. A leg defines:

- `leg_<n>_flag` `"--flag"` or `"--flag <arg>"` (`leg_arg <n>` reads it);
  `leg_<n>_order` its place in the flags, fragments, autostart lines and
  results (default 500); `leg_<n>_needs` the tools it uses, each a row of
  `dev/smoke.sh`'s tool table resolved into `<name>_bin`, or a
  `need_<name>` there for one with more to it (`mpv` and its MPRIS script,
  `python3` with PyGObject, `pil` for python3 with PIL). A new tool goes into
  that table, never into a leg.
- `leg_<n>_vm_only` `"<why>"` for a leg that needs root or a real system
  service (NetworkManager, logind, polkit): a nested run on a real host
  gets a private, empty system bus and refuses the leg with `SMOKE_FAIL`.
- `leg_<n>_budget` `"<why>"` for a leg whose numbers only hold on g815 (its
  CPU and frame budgets): the mac rig
  refuses it, and `dev/remote.sh` leaves it out of that rig's run lists.
- `leg_<n>_scale1` `"<why>"` for a leg that cannot run at the default
  1.25 output scale yet (its probes read fixed pixels, or it makes outputs
  of its own scale): the run then sits at 1, an explicit
  `FS_SMOKE_SCALE` past 1 is refused, and `dev/remote.sh`'s lists run its
  lines at 1 alone. Each one is a leg still to convert: a box read off
  `menu status`, `bar room` or `debug dump` is logical, and a frame grim
  takes is in the output's pixels, so a crop goes through
  `scaled_jq -r "$geometry_jq"'<box> | geometry'` (`dev/smoke.sh`).
- `leg_<n>_fixture_window` `keep` to leave the base run's fixture window in
  the frame; `leg_<n>_validate` usage checks, run before the build.
- `leg_<n>_fixture` `settings_fragment '<json>'` and staged files;
  `settings_pin '<jq assignment>'` for one key inside an object other legs
  write whole (`.bar.style = "solid"`), applied over the finished file, so
  a leg that rewrites settings.json mid-run drops it;
  `leg_<n>_timing` `leg_timing <delay> <timeout> [tail_gap] [drive_cap]`, max-merged.
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
  environment and nothing else in the session (`--bluez-rssi`, `--earbuds`
  and `--headset-card` point the shell's system bus at a private one where
  `dev/bluez-stub.py` plays BlueZ, which Hyprland must not follow).
- `leg_<n>_takeover` runs the whole thing itself and exits, for a leg that
  cannot share the one session (`--screensaver-gif` needs one per effect).
  It runs with the build done, the binaries resolved and the bus baseline
  taken, and owns its own `SMOKE_OK` line.

Every drive sources `$drive_lib` (`drive-lib.sh`, written per run): the
run's paths and resolved `*_bin`, then `call <target> <fn> [args...]`,
`status <target> <path>`, `shoot <path> [grim args]`, `burst <prefix>
<frames> [gap] [grim args]`, `park [x y]`, `click [button]`, `row_index
<path> <id>`, `until_jq <seconds> <path> <claim> <command...>`, `walk_to
<target> <cursor jq> <id> <trace> [key jq]`, `spawn_foot <app id> [rules]
[title]`, `settings_retarget <jq assignment>`, `pw_load_module <module>
[args]` (unloaded at teardown and on exit), `pixel` and `patch_mean`. Each
one's header in `dev/smoke.sh` is its contract. A leg's assert has
`expect_jq <path> <claim> <what failed> [excerpt]`, `pixel <png> <x> <y>`
("r g b"), `patch_mean <png> [crop]` (mean grey in thousandths),
`$layers_jq` (`layers`, `layer($ns)`) beside `$geometry_jq`, and
`panel_language_assert`.

A leg reaches the shell only through `$ipc call <target> <fn> [args...]`,
never by hand: `$ipc` is `formalshell-ipc`, shell-quoted to paste into a
generated script unquoted.
Arguments follow the CLI11 rules the IPC contract was recorded under: one starting with `[` and ending
with `]` is split on commas, a `-x` word is refused, and a bare `show`,
`wait`, `listen` or `prop` switches subcommand. A `--` before the
arguments stops the last two; only a leading space stops the split.

A leg that moves a selection (keys, a wheel, a scroll) wraps the moves in
`$(stale_audit_start)` and `$(stale_audit_stop <label>)` in its drive and
calls `stale_audit_assert <label>` in its assert: every frame committed in
between has to equal the same scene drawn whole (`debug staleAudit`).

Exported: `shot_dir`, `iso_home`, `settings_path`, `rig_password` (the
password PAM takes on the rig), `drive_lib`, `shell_bin`, `shell_log_path`,
`*_bin`, `ipc`, `write_script`, `leg_on <n>`,
`host_notifications_owner_after`. A session that runs past its timeout
fails the run. Legs sharing a surface wait
on the owner's marker (`picker_done_path`); one covering the whole output
starts at its `<n>_t0`, `cover_t0 <own start>`, past every desktop sampler in
the run: a leg photographing the bare desktop declares
`leg_<n>_desktop_until`, the second of its last such frame (`--wallpaper` 16,
`--osd` 37).
