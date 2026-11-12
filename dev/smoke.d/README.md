# Smoke legs

`dev/smoke.sh` is the scaffold (isolated HOME, settings fixture, private
bus, session mode, binaries, compositor, shell launch, `SMOKE_OK` frame,
teardown) and sources every file here. The compositor is
`dev/compositor/$FS_COMPOSITOR.sh` (`hyprland` by default): its config,
launch and `$wmctl_bin`, which answers hyprctl's command line, so a leg
reads clients, layers and monitors and dispatches through it alike under
either. A leg defines:

- `leg_<n>_flag` `"--flag"` or `"--flag <arg>"` (`leg_arg <n>` reads it);
  `leg_<n>_order` its place in the flags, fragments, autostart lines and
  results (default 500); `leg_<n>_needs` the tools it uses, each a row of
  `dev/smoke.sh`'s tool table resolved into `<name>_bin`, or a
  `need_<name>` there for one with more to it (`mpv` and its MPRIS script,
  `python3` with PyGObject, `pil` for python3 with PIL). A new tool goes into
  that table, never into a leg.
- `leg_<n>_compositors` `"hyprland shoji"`: the `FS_COMPOSITOR` values it
  runs under, `hyprland` when unset; a run under any other refuses it unless
  `FS_SMOKE_ANY_COMPOSITOR=1`. A leg prints no compositor config of its
  own: binds, key repeat, gaps, blur and the palette border go through the
  helpers both `dev/compositor/` files define (`wm_bind`,
  `wm_binds_by_keysym`, `wm_key_repeat`, `wm_window_gaps`,
  `wm_shipped_blur`, `wm_border_from_palette`, `wm_autostart`). A leg that
  calls `$wmctl_bin` beyond what `dev/compositor/shoji-ctl.py` answers
  stays Hyprland's.
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
- `leg_<n>_env` `"NAME ..."`: the environment variables the leg reads, which
  `dev/remote.sh` and `dev/vm.sh` forward to the run. `leg_<n>_commits`
  `"<dir> <glob>"`: output the run commits, pulled back into the checkout's
  `<dir>`. Both, and `vm_only`, `budget` and `scale1`, reach the other tools
  through `dev/smoke.sh --manifest`, which also lists the `dev/*.py` helpers
  a leg's code names (a change to one runs that leg).
- `leg_<n>_fixture_window` `keep` to leave the base run's fixture window in
  the frame; `leg_<n>_validate` checks a leg's own arguments and the legs it
  needs beside it, run before the build, and may set its claims.
- `leg_<n>_claims` `"name name=value ..."`, required of a leg that drives
  (`none` when it owns nothing): what of the session a leg owns
  (dotted names, a name covering every name under it) or shares (`=value`,
  shared with a leg claiming the same value). Two legs meeting on a claim
  are refused once, before the build, naming both and the claim. `session`
  owns the whole run, except beside a leg declaring `leg_<n>_rider`
  `"<why>"` (one that pins a key and drives nothing another leg sees). A
  `vm_only` leg claims `bus.system=real`.
- `leg_<n>_fixture` `settings_pin '<jq>'` (an assignment or a `. * {...}`
  merge) over the base settings.json, in leg order, and staged files;
  `leg_<n>_timing` `leg_timing <seconds>`, how long its turn may take
  (45 at least); the session's timeout is their sum.
- `leg_<n>_drive` writes the drive scripts and lists them (`wm_exec_once`, `dev/compositor/`),
  may `add_cleanup '<line>'`; `leg_<n>_assert` prints `SMOKE_*`, calls `fail`.
  A mid-run settings change is `settings_retarget <jq>` from a drive: the
  current file with the jq applied, written beside it and the symlink moved
  onto it, so two legs' changes compose.
- `leg_<n>_cleanup` runs at the scaffold's exit for a leg in the run, for
  state its fixture made outside the run's own directory (`--login-lock`'s
  marker).
- `leg_<n>_wayland_debug=1` runs the shell under `WAYLAND_DEBUG`, so
  `$shell_log_path` carries its wire traffic for an assert that has to read
  what the shell sent rather than what the screen shows (`--frame`).
- `leg_<n>_shell <path>` writes the shell start script at `<path>` in place
  of the scaffold's, for a leg that runs some other build of the shell
  (`--installed`). It still logs to `$shell_log_path` and may write the
  shell's pid to `$shot_dir/shell.pid` for the memory sample.
- `leg_<n>_session` may replace `wm_cmd`, the command line the session
  runs, with one that starts it (`--login-switch` runs it through the login
  script, as `uwsm`).
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
[title]`, `until_window <app id> [seconds]`, `until_settled [surface...]`,
`until_wallpaper <path> [mode]`, `settings_retarget <jq assignment>`, `pw_load_module <module>
[args]` (unloaded at teardown and on exit), the frame reads below, and
`$out_scale` (the asked scale). Each one's header in `dev/smoke.sh` is its
contract. A leg's assert has `expect_jq <path> <claim> <what failed>
[excerpt]` and the frame reads: `pixel <png> <x> <y>` ("r g b"),
`patch_mean <png> [crop]` (mean grey in thousandths), `patch_rgb <png>
<crop>` ("r g b"), `card_box <bare> <frame> <top> <threshold>`, `diff_px
<a> <b> <threshold> [crop]`, `colour_px <png> <hex> <fuzz> [crop]`,
`logical_px <n>` and `frame_logical <n>`,
`$layers_jq` (`layers`, `layer($ns)`, `layer_on($o; $ns)`, `bar_origin`,
in drives too) beside `$geometry_jq`, and
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
`*_bin`, `ipc`, `write_script`, `slot_port <base>`, `leg_on <n>`,
`host_notifications_owner_after`. A session that runs past its timeout
fails the run.

The legs take turns, in leg order (`drives.sh`): a leg's drive scripts start
together once the previous leg's have exited and the desktop has arrived
(the bar mapped, the arrival veil gone, and in a run that keeps it the
fixture window mapped), so a drive starts on its first action with no
leading sleep, and the run's own frame is taken after the last turn. A
script that execs a long-lived child (a player, a fixture window) has ended
its part of the turn. A drive waits on the shell's state, never on another
leg's clock: what the previous turn left (a panel open, a toast up) is
there when its turn starts, and a leg that needs something of its own (the
lock, a player, a window) makes it and waits for it (`until_jq`,
`until_window`). A card, a fade or a recolour is waited out the same way
(`until_settled`, `until_wallpaper`), never slept through; a sleep is
left only where it paces a burst taken mid-motion or waits on something
outside the shell, and says which. Each turn's start and end, in seconds into the session,
are printed as `drive <leg> start|end <s>`.
