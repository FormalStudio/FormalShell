# CLAUDE.md

Guidance for Claude Code (claude.ai/code) when working in this repository.

## Standing orders

- Plans are created autonomously, with no user approval gate before writing
  one.
- Implementation, mapping, testing, and docs all run through subagent
  workflows (one subagent per plan task, sequential, verification evidence
  required before commit; see `docs/superpowers/plans/`).
- The approved design lives at `docs/superpowers/specs/`. Plans live at
  `docs/superpowers/plans/`. **The spec wins over any plan on conflict.**
- `docs/superpowers/specs/2026-10-06-rust-rewrite.md` is the architecture:
  one Rust binary under `crates/` (threads, renderer, services, command
  plugins, installing). It wins over the 2026-07-27 spec's runtime choice.
- `docs/superpowers/specs/2026-08-25-shadcn-omarchy-redesign.md` is the
  design (Omarchy behaviour, shadcn chrome, wallpaper palette, keyboard
  everywhere, Hyprland only). The 2026-07-27 spec still holds for the IPC
  contract and config; the 08-25 spec wins where the two disagree.
- `docs/superpowers/specs/2026-09-18-theme-boundary.md` decides what a
  theme owns (styling, bar position, motion) and what is global (layout,
  behaviour, spacing, casing, palette source). It wins over the two design
  specs above.

## Verification loop

There is no Nix anywhere: the macbook, g815 and e1504g carry none.
Everything that compiles or runs the shell goes through `dev/remote.sh` to
g815, or to the mac rig while g815 is off, since the mac itself has no
pipewire, PAM or Wayland to link.

- The rig is g815: `dev/remote.sh {build|lint|cargo|check|smoke|smoke-affected|smoke-all|run|sync|pull|shell}`
  rsyncs the working tree (`.gitignore` applies, so `crates/target` stays
  per slot and cargo stays incremental) to one of eight slot users
  (`fstest0` to `fstest7`, a mac-side flock each, `FS_G815_SLOTS`) and runs
  there natively on x86_64, nested in the slot's headless labwc
  (`formalshell-rig-parent.service`), with screenshots and artifacts pulled
  into `./artifacts/`. A command takes the free slot its checkout last ran
  on first (a warm target: a no-op build is ~3s, a change to
  `formalshell-rs` alone ~25s, a cold one ~50s). Every checkout syncs into
  the slot's one `~/formalshell` with its one target (capped at 8G), so
  the rig's disk holds eight targets however many worktrees run. Any
  number of agents can run at once; past eight they queue. Its header
  carries the slot model.
- **The VM rig** is four persistent openSUSE Tumbleweed guests on g815,
  `fstestvm0` to `fstestvm3` (the owner's spec, `hosts/fstestvm`; FormalSpec's
  `docs/formalshell-vm-rig.md`), each with a system of its own:
  NetworkManager against two hostapd radios on mac80211_hwsim, BlueZ with no
  controller, UPower and power-profiles-daemon, polkit (wheel is its admin
  group, pkexec installed), logind, PAM (`formalshell-lock`, password
  `formalshell-test`), PipeWire, greetd running the FormalShell greeter, a
  vkms card for the legs and a VGA display for `dev/seams.sh`. `dev/vm.sh`
  drives them, and `dev/remote.sh` hands it every run with a `vm_only` leg
  and the five daemon-backed checks: a run builds on a rig slot (or takes
  smoke-all's `FS_RESULT`), takes a free guest (a mac-side flock each,
  `/tmp/formalshell-fstestvm-<n>.lock`, `FS_VM_SLOTS`), resumes it from its
  saved state on a throwaway overlay (`fspec vm resume`, ~1.5s), copies the
  tree and the build in, runs, pulls the artifacts and stops it, which drops
  everything the run wrote. Nothing compiles in a guest. `dev/vm.sh shell`
  is a shell in one. The guests are g815's, so the mac rig refuses these
  runs. A save goes stale once an apply changes a guest's disk: `fspec vm
  stop`, `fspec vm start`, `fspec vm save` after `fspec apply --host
  fstestvm<n>`. The mac's `fspec` has to carry `vm resume` (FormalSpec main).
- **The mac rig** takes over when g815 is off (it powers down around 22:00
  for the night): `FS_RIG=g815|mac` picks one, and unset, a g815 that does
  not answer ssh in 5s sends the command to `mac`, booting the guest first
  when it is stopped (`dev/mac-vm.sh {start|stop|status|apply}`, ~10s from
  cold). It is `fstestmac`, an openSUSE Tumbleweed aarch64 lima guest on
  Apple's Virtualization framework (8 vCPU, 16 GiB, its own disk, nothing of
  the mac mounted), with four slot users of the same shape (`fstest0` to
  `fstest3`, locks `/tmp/formalshell-mac-<n>.lock`). Every verb works there;
  each Hyprland runs on a vkms card of its own (`vkms-<user>`) with
  llvmpipe instead of nesting, at about 33 frames a second. Measured
  against g815 on the same tree: a no-op build 1.7s (g815 1.6s), one
  `formalshell-rs` file 37s (27s), a cold worktree 108s (54s), `just smoke
  --menu` 37s warm (38s), a one-file `smoke-affected` 117s (105s), the core
  set 91s over four slots, `smoke-all` ~27 min. Budgets do not hold there: `leg_<n>_budget` legs
  (`--idle`, `--menu-budget`, `--r0-measure`, so `dev/perf-gate.py` too,
  which pins `FS_RIG=g815`) are refused and left out of its run lists, as
  are the `vm_only` legs. `--menu-hover` (its drive outruns the 40s
  frame) and `--lyrics` (the lit row reads as still moving at a burst's
  end) fail there on timing alone; read them on g815. Release tarballs and
  the hosts' fspec packages build on g815 only; shipping waits for it.
- **The default check for a change is `just smoke-affected [base]`** (or
  `just smoke <flags>` for the legs you know it touches). It diffs against
  the merge base with `origin/main` (or `base`), uncommitted and untracked
  files included, maps each changed path to runs through
  `dev/smoke-map.toml`, prints which runs it chose and why, and runs them
  over the free slots off one build. Docs, tests, the plans and the
  packaging run nothing; shared code (the ui kit, fs-theme, the wayland
  core, motion, the scaffold) and any path the map does not name run the
  nine-run `core` set; a leg's own file runs that leg; vm_only legs run on
  the VM rig beside the rig's runs (on the mac rig they are named and
  skipped). Expected: one surface file ~1m45s warm (a ~25s build,
  then its legs side by side), ~2m15s from a cold worktree, the core set
  ~1m40s, a docs-only change a few seconds; a change reaching a VM leg
  takes that leg's own time (`--wifi` ~3m40s), 4m10s to 5m for the core
  set with `--sleep` and `--wifi` beside it. A new surface, service or leg
  gets its paths into the map in the same commit. In the shared main
  checkout the uncommitted files include other agents' edits, so it can
  pick more than your change alone needs.
- `just smoke-all [list]`: every run in `dev/smoke-all.list` (one line of
  flags each, every leg the nested rig can run, then the nine vm_only legs
  spread over the VM rig's four guests, ~5m30s for those alone), **for
  milestones only** (a
  phase shipping, a release) or when the owner asks: ~15 min (884s for 127
  runs, against 768s of run time over eight slots). One runs at a time per
  mac (`/tmp/formalshell-smoke-all.lock`); a second queues and says behind
  whom.
- Both run longest first off the run times recorded in
  `~/.local/state/formalshell-rig/leg-times.tsv` (the median of the last
  three, a run never timed going first), share one build
  (`dev/share-result.sh` copies it where every slot user can read it, each
  run gets it as `FS_RESULT`), put each run's log and artifacts in
  `artifacts/<smoke-affected|smoke-all>-<ts>/<n>-<flags>/`, list PASS or
  FAIL with seconds and slot in `results`, and exit non-zero on any
  failure. `FS_SMOKE_JOBS` caps the parallel runs. A slot user's very first
  run has failed `--notify`'s toast-column check once; a second run passes.
- `just build`: `dev/build.sh` on the rig: `cargo build --locked --release
  -p formalshell-rs` in dev.star's environment (`fspec dev -- cargo`), then
  `./result` laid out as the release tarball unpacks (`bin/formalshell`,
  `bin/formalshell-ipc`, `lib/formalshell/{formalshell-rs,bin}`,
  `share/formalshell`). The binaries, templates, branding and examples come
  from the tree; the fonts, Blue Marble tiles, earbuds art and the helpers
  no distro carries (ttfx, localsend-cli, tensaku, clipssh, the iPhone
  bridge, the earbuds CLIs, matugen) are linked out of the host's installed
  fspec `formalshell` package, the same tarball at its pinned rev. A host
  without it fails the build saying so.
- `just lint`: `dev/lint.sh` on the rig: `cargo clippy --workspace
  --all-targets` (deny-level lints fail it, warnings print), then the pure
  crates' tests (fs-js,
  fs-chrome, fs-info, fs-activity, fs-system, fs-devices, fs-media, fs-menu,
  fs-screensaver, fs-theme, fs-audio). ci.yml runs the same script on a
  GitHub runner.
- `just check <name>`: `dev/check.sh` on the rig. `fs-tray`, `fs-notifd` and
  `fs-mpris` run their tests under `dbus-run-session` against their own
  dbus-daemon. `fs-auth`, `fs-audio`, `fs-bluez`, `fs-network` and
  `fs-upower` drive real system daemons and run on the VM rig: `dev/check.sh
  build` compiles their test binaries (ignored tests included) and probes
  on a rig slot, and the guest runs them against its own PAM and polkit
  (`pam_probe`, `polkit_probe` over a real pkexec), PipeWire (the probe
  against two null sinks), BlueZ (`fake_bluez` and the no-controller test;
  the two-controller one needs btvirt, which Tumbleweed does not build),
  NetworkManager and the hostapd radios, and UPower and
  power-profiles-daemon. 15s (fs-upower) to 80s (fs-network) each, the
  build included. fs-upower's profile switch has timed out once with nine
  runs sharing the guests; alone it passes.
- `just cargo <args>`: cargo in dev.star's environment on the rig, for a
  quick `test -p <crate>` or `clippy`.
- `just smoke <flags>`: `dev/smoke.sh` on the rig, the runtime loop itself.
  Builds the shell (`dev/build.sh`), brings up an isolated Hyprland session
  under `dbus-run-session`, drives the legs the flags asked for,
  screenshots, tears it down. **Read the PNGs rather than assuming they look
  right.** `FS_RESULT=<dir>` runs a result built elsewhere (an unpacked
  release tarball has the same layout) instead of building one. A tool a leg
  needs and the host lacks fails the run naming the openSUSE package that
  carries it. Legs declaring `leg_<n>_vm_only` run on the VM rig
  (`dev/vm.sh smoke`, which `just smoke` picks itself): `--mirror`,
  `--polkit`, `--power` and `--sleep` ~1m to 1m20s, `--wifi`,
  `--device-routes` and `--panel-morph` ~3m40s.
- `just local-smoke <flags>`: `dev/smoke.sh` in this machine's own session,
  on a Linux host.
- `scripts/shots` / `just shots`: regenerates `docs/shots/` (the README's and
  the website's images and clips) through `--showcase --shots`, once per
  theme, on the rig from a mac and nested on Linux.
- `just greeter` (`dev/vm.sh greeter`): `dev/smoke-greeter.sh` in a VM rig
  guest, with this tree's build linked over the guest's installed shell for
  the run. greetd's `default_session` is a standing system service, so it
  drives the already-running greeter session (restart greetd, screenshot
  pre-auth, a wrong password then the real one over `wtype`), waits for the
  login's Hyprland to map the shell's `formalshell:bar` layer, screenshots
  that desktop, and writes `artifacts/greeter/`. It fails unless the session
  log shows a real `pam_authenticate: AUTH_ERR` on the wrong attempt and the
  bar comes up. ~55s.
- `dev/seams.sh`: the boot chain on film, from a VM rig guest's power-on
  (OVMF, Limine, systemd-boot, the FormalOS splash, the locked login's
  lock over the splash picture, the password, the desktop) and out of it by
  logout (to the greeter), reboot (to the lock) and shutdown to power-off,
  as PNG frames over QMP screendump (frame n is n x 100ms, the boot leg's
  n x 20ms), typed over QMP send-key, into
  `artifacts/seams/{boot,logout,reboot,shutdown}/frames/`. The boot fails on
  more than 100ms of black after the splash, the other legs past 1.5s.
  The guests autologin at boot (greetd's `initial_session`,
  `docs/superpowers/specs/2026-10-30-locked-login.md`), so `just greeter`'s
  greetd restart is what brings the greeter up.
  The guest's installed package, not the tree's build. ~4 min, the guest
  saved again at the end.
- `crates/formalshell-rs/tests/ipc-golden.jsonl` is the recorded IPC
  contract, kept fixed: argument parsing, argument errors and output
  strings, replayed through `formalshell-ipc`'s parser and the registry by
  `ipc/golden.rs`. The live list of targets is `formalshell-ipc show`.
- `just tarball`: the release tarball install.sh unpacks
  (`dev/tarball.sh`), built for this machine's arch in a debian:bookworm
  container, into `artifacts/tarball/`. It fails on a binary needing glibc
  past bookworm's 2.36 or linking a library outside its soname allowlist.
  The files it reads that are not crates (the tile and earbuds-art scripts,
  the iPhone bridge patches, the watchdog template) live in `packaging/`.

How a run works:

- Session mode is decided by the machine, never by a flag: a machine with a
  vkms card (the VM rig and the mac rig load one) runs Hyprland on that
  software KMS card, and a real host nests it in the running session. A
  guest's display card must carry no render node (the VM rig's is plain
  VGA): Hyprland on vkms allocates on any render node it finds, and on
  virtio-gpu's, with no 3D behind it, it never drew a frame. A vkms run takes the slot user's own `vkms-<user>` card
  where the machine made one, through `LIBSEAT_BACKEND=noop` where no
  seatd runs. `/etc/formalshell-rig/kind` reading `mac` gives a vkms run the
  nested rig's private system bus and refusals, and a virtual pointer held
  for the session (`dev/vpointer.py`), standing in for the one g815's
  parent holds.
- The nested session runs in a transient user scope of its own
  (`fs-run-hypr-<pid>`) and is stopped as a cgroup at teardown, so a
  compositor that outlives `timeout` goes with it; a leftover carrying the
  run's config path fails the run.
- `FS_CPU_QUOTA=10% FS_CPU_QUOTA_PERIOD=2ms` runs the shell alone under that
  CPU quota in a scope of its own, the stand-in for e1504g in power saver
  (its launcher and cold start numbers land in the same range). Every
  budget leg (`--menu-budget`, `--idle`) is read under it.
- Flags combine into one session: `leg_<n>_order` sets the order, timings are
  max-merged so a combination outlives its slowest half, and a leg that
  cannot share the session takes the run over (`--screensaver-gif`).
- Legs sharing a surface wait on the owner's marker (`picker_done_path`), and
  a leg covering the whole output starts at its own `<leg>_t0`, past any
  desktop sampler, which is why `--wallpaper` pushes `--menu` and `--lock`
  back and leaves a matugen-recoloured desktop behind them.
- Every run prints `SMOKE_OK <path>` for its own frame and `SMOKE_<NAME>
  <path>` for each named artifact; `dev/remote.sh smoke` pulls all of them
  plus the run's stdout JSON into `./artifacts/` on the mac.
- matugen's source colour is pinned to its own rank 0 rather than left to
  `--prefer`, which is a bad proxy for what colour a wallpaper is;
  `crates/formalshell-rs/src/services/theme.rs`'s header carries the why. Force the source
  the same way if you run matugen by hand while debugging.

Every leg is one file, `dev/smoke.d/<name>.sh`, whose header carries the
detail; `dev/smoke.d/README.md` is the file contract. What each proves:

- `activity.sh` `--activity`: `services/activity.rs` over its three
  sources, read off `activity status` at known steps and once off `debug
  dump`. `dev/fspec-activity-stub.py` plays FormalSpec's operation stream
  in the session's real `$XDG_RUNTIME_DIR/fspec/activity`: an apply at 1/4
  then 3/4 resources and succeeded, an update that fails, and a rollback
  whose final line is cut short with its socket gone, read as failed. Four
  seconds on the success has folded into the history with both failures
  kept, and `activity seen` folds them: the notification centre
  photographed with the three under Activity, and real Ups and a
  Return on its Clear emptying the history. A Gio client's
  `com.canonical.Unity.LauncherEntry` updates for a staged desktop entry
  land as progress, a badge count and urgency, merged property by
  property, the progress hidden ends the item and the client leaving the
  bus takes its badge. notify-send's `value` hint updates one item in
  place through `-r` and through `x-canonical-private-synchronous`, and a
  replace without a value ends it. One real `fspec run --detach jq` is read
  to a succeeded `run` item where the rig's fspec has `run`, else skipped
  saying so, and a stubbed run whose first connection fspec stops writing
  to reaches 1/1 and succeeded off a fresh connection.
  The Activity panel photographed with the apply at 3/4 beside the running
  update and again with the update failed; `activity dismiss` taking a
  running group's card (`toasts`) and not its row, the card back once it
  succeeds, no second card for a `value` notification; a stubbed failed
  `run` retried over `activity retry`, reaching a PATH-shimmed `fspec` as
  exactly `run --detach smoke-pkg`, and `activity open` on it opening the
  panel. The bar cell off `bar room`: no indicators cell at idle, one
  while the apply runs, none after `seen`, photographed at 1/4 and with the
  update failed, and a lone rollback's ring with no progress differing
  inside the cell between two frames 300ms apart. The same frames carry
  the live toast cards, and one more the `value` notifications' bubbles
  with their ring. Ten long-titled runs at once: one cell, no right-region
  cell hidden or cut, and the panel photographed with all ten.
- `airplay.sh` `--airplay`: `services/airplay.rs` against a PATH-shimmed
  `uxplay` (the real one decodes into a GL texture this rig's software KMS
  card has no path for), which prints uxplay's own connect log line and
  writes the `-md`/`-ca`/`-dacp` files at its own startup, standing in for
  a client already connected. `airplay status` and the media panel's
  `airplay` source read the metadata and cover art back; a drive-script
  trigger then tells the shim to clear the files and print the disconnect
  line, and both go back to idle in the frame and over IPC.
- `app_grid.sh` `--app-grid`: `menu.appGrid: true` drawing the launcher's
  app results as icons over their names, read off the probe entry's own
  colour covering a 64px cell rather than a row's icon, with real arrow keys
  moving the cursor one cell across and a column count down, a real Enter
  launching through the row path, apps and a ranked route sharing the card,
  and the key deleted underneath the running shell putting rows back, the
  probe's icon at the row's 22px.
  Ridden by `--no-morph` it also reads the grid inside its popover card.
  Then typing on real keys: `cursorId` agreeing with the view's own
  `viewCursor` after a re-rank and a Backspace, a query no app matches
  keeping the grid with no rows in it, Escape clearing the query and
  leaving the launcher open, and a held Backspace climbing one level.
- `appmenu.sh` `--appmenu`: the app menu over the real `panel` route with
  the base run's fixture window focused (its desktop entry names it "Iconic
  Test App" with a themed icon): the hero carrying the entry's name and
  picture, and the window list holding the fixture window. `--panel
  appmenu` alone only ever shows the empty "No window" state.
- `backdrop.sh` `--backdrop`: pins `wallpaper.animate`; the shard
  sculpture over a bare desktop, read off `debug dump`'s `backdrop` block and the
  shell's `commit surface=shards` lines: frames a second apart differing
  in its half of the screen, 20 s of it giving the shell's CPU as a share
  of one core and a frame rate never past 24 (under `FS_CPU_QUOTA` it fails
  past 10%), `debug motionScale 1000` slowing its clock to a tenth with the
  beat unchanged, no commit in 3 s under the open launcher (`modal`),
  then no commit in 5 s under a fullscreen foot window and
  none with the same window tiled over the whole output (`covered`), and
  the frames moving again once it goes to workspace 2.
- `bar_adaptive.sh` `--bar-adaptive`: the wingpanel band reading its own
  paint off the wallpaper under it, four wallpapers in one session (a flat
  bright field, a flat dark one, a half-and-half field, and the dark one
  under a fullscreen window), each read twice: the paint and the three
  numbers `bar paint` reports, and the fill the band actually drew, off a
  patch of the frame above the cells. Pins `bar.style: transparent` and
  `fullscreen.hideChrome` itself, so the half-and-half field is a bare band
  with light ink; any other style is refused.
- `bar_ink.sh` `--bar-ink`: each cell of the wingpanel band reading its ink
  off its own stretch of the wallpaper: one half black, one half white,
  under the default `transparent` style, every whole cell clear of the
  seam reporting light ink on the black half and dark on the white in
  `bar paint`'s `cells`, and one cell per half read off the frame over its
  `bar room` rect for a glyph of that ink. Refuses any other `--bar-style`.
- `bar_layout.sh` `--bar-layout`: user `bar.modules` and a reordered layout
  resolved from settings.json alone, every `command` module failure path
  (`services/commands.rs`) in the one frame.
- `bar_position.sh` `--bar-position <edge>`: the strip on a bottom, left or
  right edge, read off the compositor's own layer geometry, with a chevron
  collapsing and expanding along it and a panel hanging off its inner edge.
  Ridden by a leg that photographs its own frames (`--join`,
  `--menu-emerge`, `--panel-morph`) it pins the edge alone: the layout, the
  chevron and the panel are then the rider's business.
- `bar_room.sh` `--bar-room`: a strip crowded past its own length, `bar
  room` reporting a hidden cell in the right region and a now-playing
  budget under 220, the frame read for a whole cell at the region's own
  inner edge, never a cut one, and every drawn cell rect `bar room`
  reports clear of every other.
- `bar_style.sh` `--bar-style <transparent|solid|corners>`: pins
  `bar.style` (a `settings_pin`, inside any `bar` object the ridden legs
  write) and rides any leg, the bar then wingpanel's band under every
  table: `debug dump`'s `barStyle` and `bar paint`'s `style` agreeing, solid
  and corners on the `maximized` paint and the band's own rows black on a
  top bar. Alone it sets a flat grey wallpaper and reads corners' fillets:
  one `formalshell:bar` layer a fillet's radius taller than the band, the
  output's reserved top still the band's, black at each fillet's corner
  and grey at its arc's centre.
- `bar_title.sh` `--bar-title`: a window title far longer than the strip
  in a crowded bar, read off `bar room`: the title drawn at exactly the
  budget its region has with the marquee running, no cell hidden, and no
  two cell rects intersecting, cut by a clip or past the strip's ends,
  settled and in 24 samples taken while focus switches to a short title
  and back. A real mpv plays a long-titled track throughout: both labels
  keep a non-zero budget, the track at least its minimum or an even share
  and the title no more than the track while the track is short, and the
  centre's middle within 2px of the strip's where the cells let it be.
  Then settings.json rewritten to the title cell alone, the title
  stopping at its own ceiling with the marquee running. Rides
  `--bar-position <edge>`, which pins the edge in this leg's own `bar` key.
- `bluez_rssi.sh` `--bluez-rssi`: rides `--idle`; the shell's system bus
  a private one where `dev/bluez-stub.py` plays a BlueZ scanning on a
  discovery the shell did not start, ~700 RSSI-only signals a minute over
  a paired device and strangers. Through the idle window nothing past the
  clock commits, the stub is never called and the shell spends under 5
  ticks; `bluetooth status` lists the paired device alone, and the
  strangers too once the panel is open.
- `brightness.sh` `--brightness`: three bursts of eight steps 40ms apart
  against PATH-shimmed `ddcutil` (a bus flock held 100 to 300ms per call,
  overlaps logged) and `brightnessctl` (reads answering after 20 to 150ms),
  the shims ahead of the launcher's PATH: the keybind path's `osd state` brightness never stepping back
  and landing on 70, the display panel's HDMI-A-1 row written one setvcp at
  a time rising to 70, and its backlight row falling to 30. Then `display
  brightnessStep` in two bursts of eight, focused on headless outputs named
  HDMI-A-1 (DDC, -5) and eDP-1 (backlight, +5): `osd state` showing the
  asked value straight after the first call, samples and device writes
  moving one way only to 30 and 70, and the DDC burst landing in fewer
  than eight setvcp calls. Then overnight over the same pair: enabled,
  the display panel reading the dimmed devices, disabled as a new output's
  DDC detection starts, and again disabled 0.3s after enable; every value
  each device was given between 1 and its level before, the last one that
  level (70 and 30), no setvcp overlapping another, and a zero
  `brightnessStep` showing 70 and 30, the rows the next key steps from.
- `caffeinate.sh` `--caffeinate`: `caffeinate.onStartup` starting the
  session caffeinated with its `formalshell:caffeinate` layer surface
  mapped, the real ext-idle-notify monitor staying non-idle three screensaver
  timeouts in, and after `caffeinate disable` the surface gone and the same
  timeout firing the screensaver on its own.
- `capture.sh` `--capture`: the shell's own region picker (smart pick, tab
  cycling, commit) measured against the compositor's output, the toolbar's
  record commit, and the cancel path.
- `capture_edit.sh` `--capture-edit`: the SAVED notification's EDIT action
  reaching a real editor with the capture's own path on argv.
- `center.sh` `--center`: the notification centre listing the pending tier,
  with the toast stack suppressed for as long as it is open, content-tall on
  a short history and capped and scrolling on thirty rows, then one open at
  a tenth speed over a band down the trailing edge, for the card coming out
  of the bar's line under the bell like any panel.
- `chevron.sh` `--chevron`: a right-region chevron holding the five cells
  before it off the strip entirely, and `bar chevron expand` opening them in
  the second bar under it, the two frames asserted to differ. It also sets a
  flat bright wallpaper first, so the band goes to dark ink, and reads a patch inside the card for a dark plate carrying
  light words: the second bar keeps its own ink whatever the band wears.
- `chevron_quiet.sh` `--chevron-quiet`: the same second bar OPENING, read
  off a stamped burst of frames: the card is never narrower than the width
  it settles on, and nothing inside it moves once its own entrance is over.
- `clipboard.sh` `--clipboard`: the ledger's capture order and in-process row
  activation, with the image entry's preview and a copied-markup row's own
  angle brackets in the frame, and a `text/uri-list` copy of one jpeg (how a
  GTK4 app copies an image) landing as a png image row.
- `clipssh.sh` `--clipssh`: the clipssh route's send, its bar indicator and
  its copied/failed toasts, against a shimmed binary.
- `clipssh_image.sh` `--clipssh-image`: the two sends that resolve a host out
  of `clipssh.alias` rather than off a row, `clipssh.autoSendImages` and
  Shift+Enter on a history image row, each checked against the sha256 of
  what the clipboard actually held when the shimmed binary read it.
- `config_reload.sh` `--config-reload`: a settings.json whose symlink is
  retargeted (what home-manager does on every activation, the one write a
  file watch cannot see) still reaching a running shell, read off the bar's
  own edge moving right to left.
- `console.sh` `--console`: the quake console parking on a special workspace
  and coming back with the same window id.
- `deform.sh` `--deform`: a burst through one open ending on the card's
  resting box exactly three clocks later, the springs unwound. The table's
  curves never overshoot, so no frame past rest is asked for.
- `depth.sh` `--depth`: rides `--lock`; a seeded wallpaper with a
  `.depth/` map beside it marking an orange band through the clock digits
  as the subject: `lock status` reporting `depth` true and the band drawn
  over the digits, then settings.json retargeted to `lock.depth: false`,
  `depth` false and the digits back over the band. `FS_DEPTH_GENERATE=1`
  swaps the band for the mountains photograph and has the result's own
  `formalshell-depth` (the tarball's bundled Python, onnxruntime and model)
  write its map first: a second run `fresh`, the map not flat and within 0.1
  of the committed reference, then the status checks.
- `desktop_clock.sh` `--desktop-clock`: `desktopClock.enabled` over the
  `--depth` leg's band fixture, read off `debug dump`'s `desktopClock`
  block and a frame per phase: the band over the digits, then no commit in
  3s at rest; no
  commit in 5s under a fullscreen foot window; the digits over the band
  once `desktopClock.depth` is retargeted false; the gooey and serif
  faces; and the first redraw past the next minute (`minuteUs` plus its
  commit's `render_us`) under 50ms, read under `FS_CPU_QUOTA` too.
- `device_routes.sh` `--device-routes`: the launcher's Wi-Fi, Bluetooth, Audio
  and Radio Stations routes over real keys and IPC, read off `menu status`
  (its `ids` and `checked` lists) and each service's own status. The Wi-Fi
  row on the two hostapd radios opens the masked password step, a wrong
  password comes back as "Wrong password" on the row and Enter asks again,
  the real one connects and ticks the row, Show password draws it back in
  the secret step (NetworkManager's `GetSecrets`, through the shell's
  polkit agent if it asks; read off the frame by tesseract, masked in `menu
  status`) and Escape drops it, Shift+Enter forgets it, and no password
  reaches `menu-selection.txt` or the shell log. A null
  sink of the leg's own becomes the default over Enter, read back with
  `pactl`, the tick moving with it. Bluetooth is the honest no-adapter row
  while `panel open bluetooth` still opens the panel. A favourite served by
  the leg's own looping ffmpeg listener plays and ticks, Shift+Enter
  removes it, and `:r` leaves its searching row (a failed search passes).
  A root query reaches the saved network, the sink and the favourite, and
  never a nearby network or a search result. Refuses `--wifi` and `--radio`
  in the same run.
- `display.sh` `--display`: `display scale|mirror|enable` reaching the
  running compositor as `hyprctl eval 'hl.monitor{...}'` calls, each read
  back off `hyprctl monitors all -j`: the rig's output at scale 1.5, then a
  headless second output created for the purpose mirroring it, unmirrored,
  disabled and enabled again, removed before the panel frame is taken.
- `dump.sh` `--dump`: the `debug` target's whole state dump, saved as the
  run's JSON sidecar and read by other legs for what the shell resolved.
- `earbuds.sh` `--earbuds`: the earbuds panel against PATH-shimmed
  `nothingctl` and `openscq30` answering with fixtures captured off the
  real tools, and `dev/librepods-stub.py` playing the omarchy-pods
  daemon's socket and status file (`FORMALSHELL_SMOKE_LIBREPODS_SOCKET`
  points the shell at it, never the host's daemon), in five
  phases: the B175 alone (wrapped listening mode and EQ rows, custom bands
  in signed dB), the Soundcore pair alone, both with the device choice
  heading the panel, the AirPods, and a device nothingctl refuses. `earbuds
  set` over IPC reaches nothingctl's stdin as exactly `anc transparency`
  and `eq-custom 5 0 -2` and openscq30 as one `--set
  ambientSoundMode=NoiseCanceling`; every AirPods listening mode reaches
  the stub as exactly `noise:<mode>` and the panel holds the mode it
  reports back, and a stub that never accepts (a daemon whose event loop
  is stuck) leaves the device reading "librepods not responding"; the refused device's `watch` prints the
  `unsupported-model` line and exits 3, and 12s later it was started once
  and no device is listed. The rig has no Bluetooth controller, so the leg
  exports `FORMALSHELL_SMOKE_BLUETOOTH`, a device list that replaces the
  adapter's (`fs-devices` `earbuds::bluetooth_devices`), with the Soundcore
  pair connected.
- `emoji.sh` `--emoji`: the launcher's emoji route by search and by order:
  `:e sob` reaching 😭 through CLDR's keywords (its Unicode name has no
  "sob" in it), and one copy through the row's own Enter path putting that
  emoji at the head of its own rank and no higher, with no settings key
  written. Then the whole grid walked on real keys (Down, Page_Down, End),
  the bottom band of the body's viewport carrying emoji ink at every stop.
- `flexoki.sh` `--flexoki`: a wallpaper under a `flexoki/` directory, and
  the rewrite reaching a user template, its `post_hook`, and the shell's own
  GTK and Qt palettes: Flexoki green and yellow, which no Material scheme
  seeded on Flexoki blue can produce.
- `frame.sh` `--frame`: pins `frame.thickness` in the settings fixture so the
  bar's window grows to the output and paints the frame round it, and reads
  that box and the four exclusion zones off the compositor's own layer list.
  Ridden by `--bar-style corners`, the ring's own fillets read black where
  the band meets the cut-out, the style adding no layer.
- `fullscreen.sh` `--fullscreen`: the fullscreen chrome auto-hide, read off
  the compositor's own layer list: bar, frame zones and hot corners gone
  while the fixture window is fullscreen and back to their starting counts
  after, with `hyprctl clients` confirming a window really was fullscreen.
  A monitor and a visualizer cell sit in the bar over a real mpv playing:
  a cava child and the monitor collector's execs (strace) before, neither
  while the bar is covered, both back after.
  Then `dev/lockpointer.py` floated over the whole output under the
  scrolling layout, holding a persistent `zwp_locked_pointer_v1`: two
  notify-sends and an `osd volume` over it leave the lock active, the
  notifications layer a column with an empty input region, no OSD layer,
  and the cursor and active window where they were; a `debug join` map
  after them releases the lock, so the probe sees a release at all.
  Then the same window in Hyprland's own fullscreen, tiled under scrolling,
  tiled under dwindle and floating under scrolling, its lock re-armed over
  SIGUSR1: a toast comes and goes with the lock held throughout, the stack
  unmapped after it when tiled and still mapped when floating.
- `glass.sh` `--glass`: the cards' opacity following Hyprland's own blur
  switch: with the rig's `decoration.blur.enabled` false, `debug dump`'s
  `blur` false and `surfaceOpacity` 1; the session's config rewritten with
  blur on and reloaded, `blur` true and 0.85 off `configreloaded`. The audio
  panel and a sticky toast over a flat bright wallpaper photographed in
  both states, the share of the right half dark enough to be their opaque
  fill at least halving.
- `firefox_install.sh` `--firefox-install` (vm_only): Firefox installed
  from the launcher for real under `layout: sidebar`: Install mode, "firefox"
  typed, Down to `zypper:MozillaFirefox` and Return on real keys, the
  guest's fspec running `app install --detach` against a clone of the
  owner's public spec whose upstream is a bare copy in the guest. The
  install's group running with the Activity card under notifications
  (photographed), then succeeded, the pushed commit authored `fspec
  <fspec@fstestvm<n>>` (the guest has no git identity), and a root search
  for "firefox" listing Firefox's app row (photographed). Several minutes,
  most of them zypper.
- `gallery.sh` `--gallery`: the dev gallery sheet, every shared component
  drawn against the live theme. The power flow is drawn from
  `fs_system::power::flow::sample_flow` (labelled a sample), the only
  populated view of it on
  a rig with no battery; `--panel power` proves the honest "No power
  sources" state.
- `glass_behind.sh` `--glass-behind`: the shipped layer rules from
  `docs/examples/hyprland/formalshell.lua` loaded and blur turned on, the
  launcher, the audio panel, the notification centre, the volume OSD, the
  switcher and a sticky toast each photographed over a foot window full of
  text and again over a bare wallpaper: what each card lets through (the
  edge energy of the two frames' difference inside it) at most 0.06 of the
  text's own, where a translucent card with nothing blurred under it lets
  through 0.11 to 0.52. Ridden by `--no-morph` it reads the popover cards
  (the launcher's opaque), by `--light` the light ones; run it at
  `FS_SMOKE_SCALE=1.25` too.
- `gpu.sh` `--gpu`: both cards of a hybrid laptop this rig is not, and the
  four PRIME offload variables reaching a launched child.
- `grid_relaunch.sh` `--grid-relaunch`: the root grid after launches that
  re-rank it and desktop entry rescans while it is closed and open, read
  off `menu status`'s `cells`: eight ids, none twice, the launched app
  first.
- `headset_card.sh` `--headset-card`: the headset connect card.
  `FORMALSHELL_SMOKE_BLUETOOTH`
  is exported as `@<file>`, a JSON device list the shell reads again on every
  change (the rig has no controller), so a device connects and disconnects
  by the leg rewriting that file. A device connected at startup raising no
  card; a headphone connecting raising one with its BlueZ battery ring, held
  past four seconds by a real pointer parked on it and dismissed by the
  pointer leaving;
  the same device off and on inside a second raising none; AirPods off the
  staged librepods status file raising left, right and case rings, the
  pointer leaving dismissing them; a real Escape typed into a focused foot
  dismissing a card the pointer holds up, through the non-consuming bind
  the card adds while up (one Escape bind in `hyprctl binds` then none,
  foot focused throughout and reading every key, the Escape included); and
  do-not-disturb raising none. Counted
  off the shell log's `headset card mapped` and `unmapped` lines, with the
  frames read by eye.
- `hdr.sh` `--hdr`: HDR on the rig's EDID-less vkms output, so the honest
  unavailable path: `hdr status` unsupported with a reason, `enable`,
  `toggle` and `setOutput` refusing with their error strings and leaving
  state.json alone, the Display panel frame carrying the dim "HDR
  unavailable" line, and `hdr rule <output>` (the rule an enable would
  send, unsent) restating the mode, position, scale and transform
  `hyprctl monitors all -j` reports and carrying no vrr, which the merge
  leaves to the output's own rule or `misc.vrr`. The real toggle is g815's
  to confirm.
- `idle.sh` `--idle`: the rewrite spec's idle and cold start budgets under the
  default bar: the launch stamp to the bar's first commit with the shell's
  `phase` lines between (the bar's configure among them, the compositor's
  share), then 60 s undriven, CPU ticks and voluntary switches (wakeups)
  per thread and the commits made, then 20 s of `strace -f` naming each
  waker.
- `hotcorner.sh` `--hotcorner`: the screensaver's and the lock's hot
  corner surfaces mapped on the right layer, and none for the default
  top-left overview corner on a rig with no HyprExpo, logged as off.
- `hotcorner_relock.sh` `--hotcorner-relock`: locks from the corner, unlocks
  by typing, proves the corner stays quiet while the pointer sits in it and
  fires again only after a leave plus the 400ms cooldown. Then the same on
  the screensaver corner: dismissed with the cursor parked in it, quiet for
  3s after the overlay unmaps, firing again after a leave. Then
  `topLeft: "overview"` written under a Hyprland with no HyprExpo,
  mapping no surface for it.
- `hypr_motion.sh` `--hypr-motion`: Hyprland's window animations on the
  shell's two clock families. The rig boots with animations off; the leg
  appends the shipped `docs/examples/hyprland/formalshell.lua` motion block,
  reading the formalshell-chrome.lua the shell publishes: `hyprctl
  animations` carrying the six `formalshell_<family>` beziers at
  `MOTION_CURVES`' control points and nine leaves (windows, layers, fades,
  border, workspaces) on their family's bezier and speed,
  `animations:enabled` on; a burst of one-row strips across a foot
  window's arrival with its middle drawn over a still bare edge; and
  settings.json rewritten with `motion.enabled: false` turning
  `animations:enabled` off.
- `installed.sh` `--installed <distro>`: `install.sh --from` the tarball
  in `~/install-check`, run from nothing in a rootless podman image of
  arch, trixie or fedora (pacman, apt or dnf installing Hyprland and the
  tools, `formalshell install --yes` writing the PAM file), and the shell
  it installs run from that image against the session over its Wayland
  socket and Hyprland instance directory, on its own private bus: a
  connected hyprland backend in the dump, the bar, `menu toggle` opening
  the launcher, and a wrong then the real password through the installer's
  own `formalshell-lock` PAM file.
- `instance.sh` `--instance`: a second daemon taking the lock, exactly one
  survivor, and the survivor being the new pid.
- `iphone.sh` `--iphone`: `services/info/iphone.rs` and the notification
  filter against
  PATH-shimmed `omarchy-iphone-bridge`/`omarchy-iphone-ams`, both a
  `tail -F` over a JSONL fixture the drive script paces in real time.
  Connected state and device name; a normal arrival's toast carrying the
  iPhone source mark, its positive action reaching the bridge shim's own
  call record; a silent arrival under `focus: respect` landing in the
  centre's pending tier with no toast, and under `hide` (retargeted
  mid-run by rewriting settings.json in place) not reaching the centre at
  all while the phone's own recent list still keeps it; the
  `com.apple.MobileSMS`/`Messages` dedupe rule collapsing a phone message
  and the same one over a real `notify-send -a Messages` to one card in
  both arrival orders, the local one surviving; and the panel populated,
  Recent and Now playing off the ams shim's own line. The ams shim's first
  run fails its subscribe (error line, then a non-zero exit) the way a phone
  not yet GATT-ready does: `iphone status` shows the error, worded as the
  LE link being down rather than the GDBus string, with no media, ams is
  started again on its backoff, and the second run's now playing lands
  with the error cleared. Then a status line for a phone BlueZ holds a bond
  for with its LE link down (a phone that forgot this laptop): not
  connected and bonded, ams not started again, the panel offering "Pair
  again", and `iphone pair` reaching the bridge shim with `--forget
  <address>`, the bond cleared and a pairing code landing in the panel.
- `join.sh` `--join`: the join itself mid-flight under `debug motionScale`,
  four opens sampled frame by frame: a panel against the far end of the
  line, the chevron's second bar, a panel clicked out of a cell inside that
  second bar, and one hanging off the line's own start. Asserts the line's
  row painted once, the walled run-out to the screen's edge, a bud held
  inside its owner's span and bordered on both sides while it widens. It
  rides `--bar-position <edge>` and `--frame`, which put the same four cases
  on a vertical hairline or against a ring's line; every probe is cut for the
  top bar, so those runs print each claim as skipped and are read by eye.
- `keybinds.sh` `--keybinds`: the launcher's binds route rendering rows off
  Hyprland's own expanded bind table.
- `launch.sh` `--launch`: launch feedback (`services/launch.rs`), read
  off `activity status` and `bar room`: a staged entry whose foot window
  maps two seconds after its spawn, launched by `activity launch` (the
  verb a keybind calls) and by the launcher's row, is a running
  `launch:<id>` item with the bar's indicators cell up and no toast card
  while it waits, and gone with nothing in the history once its window
  maps; a staged `sleep` that maps none reads failed, "Launch Ghost has
  not opened a window", 16s on; an id with no entry answers the error
  string. The waiting and failed bars photographed.
- `launcher_abbrev.sh` `--launcher-abbrev`: abbreviations off `menu
  status`'s `ids` and `abbrev`: `fx` leading with an app labelled `fx`
  until Foxtrot is picked twice under it and leads as `learned`, a record
  taught 60 days ago not leading, a configured abbreviation in
  settings.json beating a pin in state.json for the same query and a pin
  alone leading, `menu abbreviations` holding the two picks, and
  settings.json's sha256 unchanged. Then the column, through `menu
  actions` and `menu runAction`: a configured abbreviation's column the
  settings.json note alone, a learned one's ending Forget, Edit and Pin,
  Pin writing `pinned.fx`, Edit answered `fo` on real keys moving the pin
  to `fo` and dropping `fx`, and Forget emptying it.
- `launcher_actions.sh` `--launcher-actions`: the actions column off
  `menu status`'s `actionsOpen` and `actions` and `hyprctl clients`, over a
  staged app opening a foot window of its own app id: a real Tab listing
  exactly its actions in order (a menu.jsonc entry hiding Copy desktop file
  path and adding one), `new` typed filtering to New window and Enter
  mapping a second window, → listing its two windows, Tab on one, `move`,
  Enter, `2` and Enter moving it to workspace 2, Shift+Enter on the list
  mapping a third, Shift+Tab, Escape and ← each closing the column onto the
  list's query, `menu runAction` running the menu.jsonc entry, and
  `actions`, `runAction` and `into` answering `error: menu not open` shut.
  The column, the targets and the windows photographed dark, the column
  light.
- `launcher_density.sh` `--launcher-density`: the launcher as a popover
  (`motion.morph` false) on a headless second output at scale 1.25 beside
  the session's own at 1, the bar's: every buffer the card, its look's four
  corners (which log as the card) and its content commit read off the
  shell's log at 1.25, since Hyprland sends a subsurface no preferred scale
  and a corner left at the bar's 1 is a stretched picture on the panel. The
  frame of that output is pulled for reading.
- `launcher_install.sh` `--launcher-install`: Install mode (launcher
  spec §8) against a PATH-shimmed `fspec` speaking FormalSpec's
  `docs/shell-install.md` (a fixture `search --json`, and `app install
  --detach` starting `dev/fspec-activity-stub.py`'s `install` on the
  session's real operation stream), read off `menu status`, `packages
  status` and `activity status`: the rows with source and version, a
  catalog icon, the installed one ticked and the description pane; every
  dim state (Searching…, a failed search's reason, No packages, and fspec
  gone reading `Installing apps needs fspec`); a real Enter reaching the
  shim as exactly `app install <id> --detach` with the launcher left open
  and the row's ring and value following the stream (Resolving,
  Downloading 40%, Installing 50%); the live toast once it closes; the
  shim's desktop entry found as the install's app, picked once in
  state.json, Activity's Open (`activity openApp`) running it, and a search
  for it heading with the app above its package, which reads Installed; a
  failing install's reason on the row and in Activity, and `activity
  retry` reaching the shim as the same install. Then Uninstall on a
  staged app the shim's `app owner` names this host's spec's a second
  late: a real Tab before that answer, Uninstall joining the open column
  last, End and Return only arming it (`Uninstall Foxberry?`), the second
  Return reaching the shim as exactly `app uninstall
  flatpak:org.example.Foxberry --detach`, Activity reading it succeeded,
  the app gone from the rows once its entry is, and a staged app the
  distro owns offering no Uninstall. Photographed dark and light
  (`--launcher-install --light`).
- `launcher_modes.sh` `--launcher-modes`: modes as a token in the field,
  read off `menu status`'s `level` and `token`: every route the rig lists
  entered by its name and Enter and by `menu summon`, the five prefixes
  (`:e`, `:k`, `:r`, `:f` and a menu.jsonc fixture's `:m`) typed on real keys
  with what follows kept as the query, a query inside the fixture mode
  listing its own leaf alone and Clipboard and Emoji listing none of a
  staged app, Backspace, a 1500ms held Backspace and ← on an empty field
  each leaving one level, Escape clearing the query then leaving each
  mode then closing, and a real click on the token leaving it. The
  three-deep token, Emoji's and the scoped list photographed dark and
  light.
- `launcher_mouse.sh` `--launcher-mouse`: the launcher under a real
  pointer, read off `menu status` (`cursorId`, `hover`, `actionHover`,
  `actionsOpen`, `actionsScroll`, `footHints`, `actionsBox`) and the
  services the rows drive: the pointer crossing five root rows moving
  `hover` and leaving the selection on row 0; in the toggle hub a click
  running Caffeinate with the hub left open on it, a right-click opening
  Do Not Disturb's actions with it selected and DND untouched, and a click
  on Overnight with the column open closing it onto Overnight unrun,
  which the foot's Enter button then runs; the foot's Actions, Back and
  Into buttons doing Tab, Shift+Tab and →; over a staged app with 16
  menu.jsonc actions, a wheel notch and a touchpad flick
  (`dev/vpointer.py`) over the column scrolling it alone, a notch over the
  list scrolling the list alone; a click outside closing the card. The
  hover beside the selection and the right-click column photographed dark
  and light.
- `launcher_overflow.sh` `--launcher-overflow`: §11's overflow on a
  headless 1280x720 output made for it, read off `menu status`'s
  `cardBox`, `rowBoxes`, `actionsBox` and the column's keys: a staged app
  named 200 characters long with every row's box inside the card's, a
  recent file under a long directory whose dim value, read off the frame,
  is at most 40% of its row with the elided name one gap before it, and a
  launcher plugin's row carrying 40 actions of its own, Tab opening the 840
  card whole on that output with the column's last visible row reading
  `n more`, a real End scrolling it to the last action, and the same Tab
  on a second headless output 800x600 cutting the card to 776 with the
  column at most 45% of it. Photographed off those outputs alone, the
  column dark and light.
- `launcher_preview.sh` `--launcher-preview`: the preview pane, read off
  `menu status`'s `cardWidth` and `pane`, the dump's modal card and
  `captures`: a staged app at 560 with no pane, a foot window at 840 with
  one captured frame under its title, a text file's first line in mono, a
  folder's count and names, an image file's swatch and a clipboard image's
  colour read off the pane, `→` listing the folder folders first with its
  hidden entry only after `.`, ← back, and the launcher capture gone once
  the selection leaves the window and once the card closes. The Files mode
  over a staged `recently-used.xbel` and `user-dirs.dirs`, a file that has
  gone left out. Photographed dark and light.
- `launcher_send.sh` `--launcher-send` (vm_only): `Send to…` on a
  clipboard entry and on a recent file, a second `localsend-cli recv` in a
  network namespace of the guest's (veth, the multicast range routed onto
  it) as the peer: LocalSend never lists a `recv` on its own host, and its
  port is fixed at 53317. With no peer
  the list settles on one `No devices found` note; with the peer up the
  next scan lists it under a `Send to` token, a real Enter delivers the
  copied text word for word and the file byte-identical (sha256), read
  off the peer's directory. The empty list and the peer photographed.
- `light.sh` `--light`: pins `theme.mode` light (a `settings_pin`) and
  rides any leg, so its frames are read in light mode; asserts `theme
  status` reports light.
- `lights.sh` `--lights`: `services/lights.rs` against PATH-shimmed
  `asusctl` and
  `busctl` standing in for asusd's Aura object: the startup repaint off the
  palette under the default wallpaper source, effect and speed, a custom
  colour switching the source, the toggle off and back to its level, an
  unknown effect refused, and the launcher's effect level with its tick,
  each read off `lights status` and the shim's own argv log.
- `lock.sh` `--lock`: the lock round trip over real PAM, wrong password to
  unlocked, typed by a real virtual-keyboard client, with a staged `~/.face`
  found in the locked frame as the avatar over the clock.
- `localsend.sh` `--localsend` (vm_only: localsend-cli fixes its receive
  port at 53317, which g815's own shell holds): a real loopback transfer, a second
  independent `localsend-cli send` process against the shell's own real
  receiver child. The file lands byte-identical (sha256) in the fixture
  directory and one RECEIVED toast follows, raised by `recv`'s own `Recv
  file` log line; three files something else writes into that directory
  (a `cp`, a `.part` name, a late `cp`) raise none. The reverse direction (the
  shell's own `send` IPC route against a second real `recv`) can't run
  here: 0w0mewo/localsend-cli hardcodes the receive port to 53317 with no
  flag to move it, so a second `recv` on the same host only collides with
  the first; `--share`'s own `localsend send`/`peers` IPC round trip is
  what this rig can still honestly prove of that half.
- `lock_media.sh` `--lock-media`: a real MPRIS player looping a fixture
  track while the session locks over a flat white wallpaper, the now-playing
  card under the field photographed with its cover, real Tab, Right and
  Return through the password field's own key filter pausing the player
  (`media status`), and the clock's ink flipping from dark to light when a
  flat dark wallpaper replaces the white one under the same lock, read off
  `lock status`'s per-output report and off the frame inside the clock's
  own rect. Then the mode goes light under the same lock: the card is the
  theme's `card` box, a band of its padding (`mediaCard` off the report)
  one flat fill in both modes, dark then light. The real password typed
  last still unlocks.
- `lock_restore.sh` `--lock-restore`: a shell SIGKILLed holding the lock
  secure, Hyprland's dead-locker page photographed, and a new shell
  started through the wrapped launcher taking the lock back off the
  compositor-instance marker it finds (`misc.allow_session_lock_restore`
  in the rig's config and the shipped one): its log naming the restore,
  `lock status` locked and secure, the avatar fixture on screen, and the
  real password unlocking it. Runs alone, since it kills the session's
  shell.
- `login_lock.sh` `--login-lock`: the boot's lock
  (`docs/superpowers/specs/2026-10-30-locked-login.md`) raised by the shell
  itself off a `login-lock` marker written before it starts, naming a flat
  pink picture, with no `lock lock` anywhere and the shell at a hundredth
  of its speed (`FORMALSHELL_SMOKE_MOTION_SCALE`). The lock's first commit
  ahead of any bar or wallpaper commit off the shell's log, `lock status`
  locked, secure and `boot` before anything asks; the first frame the
  picture alone at 1:1 in the output's centre, a later one the card over
  it; real Tabs reaching Shutdown, Reboot and Other user (`buttonCursor`),
  Return arming Other user, a confirmed Reboot the nested rig's empty
  system bus refuses saying so under the field, and the real password
  unlocking with `boot` false and the marker gone.
- `logind_lock.sh` `--logind-lock`: logind's Lock signal on the VM rig's
  real logind, the shell started with `XDG_SESSION_ID` emptied the way uwsm
  starts it, so it finds its session through logind's `auto`: `loginctl
  lock-session <id>` locking (`lock status`, LockedHint yes), `loginctl
  unlock-session` lifting it with LockedHint no (polkit gates it, as on
  GNOME and KDE), then the launcher's Lock row, typed for and activated,
  coming back through the same signal, locked again and unlocked by the
  real password.
- `lyrics.sh` `--lyrics`: three tracks (two cached, one a sibling `.lrc` of
  the shape a line-synced provider really returns), the lit set on a duet and
  background overlap, quality and estimated timing, the estimated wipe read
  off pixels, a real wheel notch taking follow over and a track change
  re-arming it, the none track's panel narrower, the card's centre held on
  its own bar cell's centre through a lyrics arrival, the lit line broken
  inside a chunk wider than the pane, a card resting on the screen's far
  padding keeping its far edge there through a track change, lyrics
  synced before the panel ever opens with the first open landing on the
  split width, a close and reopen with no track change between putting
  `follow` back, two consecutive line changes photographed off mpv's own
  seeks (one into a line that fits a row, one into the line that wraps),
  the pane never emptying out mid-change and the lit row on one place across
  the tail of both bursts, the wipe crossing a wrapped line's row break in
  reading order with the rows under it untouched (two frames off a paused
  player), the rows the pane draws counted against the rows its own height
  holds, the instrumental note over a gap a track with no end stamps leaves
  and empty again once the song is past it, and the same two words held for
  10s and for 3s each wiping to the fraction of the row its own span is
  through. Then the player moved onto a pw-loopback sink declaring 250ms of
  latency, a bluez5 sink's shape, and the lit line held back by exactly
  that, read off `media lyrics` and three paused frames around one line's
  start.
- `lyrics_blur.sh` `--lyrics-blur`: one real MPRIS player, `media.lyricsBlur`'s
  default true against a settings retarget to false, a crop over a far
  unlit row read by edge energy and lower with the blur on, the active
  line never blurred.
- `media.sh` `--media`: the media panel read off a real MPRIS player, both
  marquee states, shuffle/loop/volume set over IPC and read back out of mpv,
  and the players switcher.
- `media_progress.sh` `--media-progress`: the media panel opened once and
  held through playback, a pause and resume, a seek, a track change and a
  player switch, with and without synced lyrics, the drawn progress fill
  measured off the frame against the position `media status` reports at
  every sample, each frame cropped to its own panel rect (the panel narrows
  when lyrics go) and the track found as the longest bar three rows deep
  off the card's own shade.
- `menu.sh` `--menu`: the launcher's root at rest, one heading and nothing
  else: `Modes` (the root routes in declaration order, no app among them)
  over a state.json staging two fixture picks and one of an app not
  installed, then `Recent` holding exactly the three fixture apps once a
  real search and Enter makes the third pick; its fuzzy ranking against the
  live tree, and the select round trip, with every status read's
  `cursorId` checked against the row the live view holds its current item
  on (`viewCursor`). Real arrow keys on the `Modes` rows: one Down from a
  fresh open scrolls nothing, and four Downs and four Ups back each leave
  the cursor's item whole inside the viewport (`viewCursor.top`/`bottom`)
  and the cursor on the first row. Then the
  field on real keys, read off `menu status`'s `query`, `caret` and
  `selection`: a word typed, Ctrl+A selecting it (photographed), a second
  word replacing it, and Shift+Home then Backspace emptying it.
- `menu_budget.sh` `--menu-budget`: the rewrite spec's launcher budget off the
  shell's own commit log, the bar's badge spinning throughout five `menu
  toggle` opens: toggle to the launcher's first commit under 50 ms (read
  from the card's first configure where the compositor took over 20 ms to
  send it, as the VM's llvmpipe Hyprland does), the
  shell's own hold on any bar frame under 16 ms (each gap split into the
  compositor's callback wait and the shell's turn), no gap between bar
  commits over 33 ms where the compositor kept 60 Hz, and no launcher
  commit in five seconds open at rest, nor in five more with the actions
  column open, with a recent image in the preview pane, or with → on a
  staged app showing its foot window's captured frame.
- `menu_actions.sh` `--menu-actions`: the launcher's in-process `@ipc:`
  actions reached through its own rows: the toggle hub's Do Not Disturb,
  Caffeinate and Overnight rows each flipping their service with the hub
  left open and the rows ticked, Set Reminder's input step setting a
  reminder typed on real keys, and Clear Reminders dropping it.
- `menu_hover.sh` `--menu-hover`: a real pointer moving onto emoji cell
  20 and one cell on at a tenth speed: `cursor` held on cell 0 and `hover`
  20 then 21, the hover tint apart from both the rest and the selection
  fill, the cell sampled frame by frame with no sample darker than its
  rest or hover tint, then a real Right with some frame holding the body's
  one selection fill part way between cells 0 and 1.
- `menu_emerge.sh` `--menu-emerge`: the launcher budding off the top line,
  sampled frame by frame under `debug motionScale`: card fill under the line
  with nothing at its resting floor, the bar's own band undimmed while it is
  still attached, a plain bordered card over a dimmed band at rest, and the
  open-to-rest time printed before any of it is asserted. Which row that
  line is on comes out of a `debug dump`, so the same probes hold on a frame
  ring's top band; a top edge carrying neither prints them as skipped.
  Then the close photographed the same way, and on every theme the card's
  and the scrim's share of their rest level rising through the open and
  falling through the close with a frame between 0.15 and 0.85 each way,
  never cut from 0.9 to 0.1 between neighbouring frames.
- `menu_morph.sh` `--menu-morph`: the card opened on the root at a tenth
  speed, then re-levelled onto the clipboard's split route, its left edge read
  off one header row per frame: the root and the rest frames on the
  `popupWidthMenu` and `popupWidthMenuSplit` edges, a frame between them.
  Then a real Tab on the root's first row and a real Shift+Tab, the card
  travelling to the split edge for the actions column and back, a frame
  between each way.
- `menu_resize.sh` `--menu-resize`: the root, the monitor view reached by
  typing `sysmon` and a real Return (so the card's look grows past the size
  it was first drawn at), a real Escape back to the root and `menu summon
  monitor` again at a fifth speed, a burst of frames through each resize:
  the scrim's patches holding on every frame, along three rows and two
  columns the pixels past the scrim one run at most, so no content shows
  outside the card or over a card not drawn yet, and on every settled frame
  and the last ones of each burst card fill under every row and column of
  the content's box (`debug dump`'s `content` rect). Worth running under
  `FS_CPU_QUOTA` too, where the card draws in slices across loop turns.
- `menu_rows.sh` `--menu-rows`: the toggle hub filtered at a tenth speed, a
  frame of the rows' band that matches neither settled list: rows that kept
  their place slide, new ones fade in, gone ones fade out.
- `mic.sh` `--mic`: the opt-in mic cell rendering its honest no-device state
  on a machine with no capture device.
- `minimap.sh` `--minimap`: the sidebar's Spaces map acted on under
  `layout: sidebar`, three foot windows over two workspaces: `sidebar
  focus` putting the cursor on the focused window, then real keys off
  `wtype`: Left to the row's first window, Shift+Right swapping it with its
  neighbour (their places traded in `hyprctl clients` and in the row's
  order), Shift+Down carrying it onto workspace 2 and again past the last
  row onto a new workspace 3, the cursor following it, and Escape giving
  the keyboard back. Then held drags off `dev/vpointer.py`: the window
  dragged onto workspace 2's row (status read mid-drag, lifted over row
  1), the two there trading places by a drag along the row, one dropped
  on the empty row making workspace 3, and a drag let go past the column
  moving nothing and holding no order. Last the scrolling layout, three
  windows on workspace 4: the first icon dragged past the last moving its
  column to the tape's end (`e f d` by x, where a swap gives `f e d`) and
  Shift+Left carrying it back one (`e d f`), the map's rows between the
  two matching a whole redraw (a collapse and expand) pixel for pixel, so
  no lifted icon left a trail. Then `sidebar focus` over an
  open launcher closing it with the cursor up, `sidebar collapse` taking
  the cursor down, and 26 more workspaces: the map cut to the rows that
  fit above the foot's buttons and a real wheel notch moving the first
  row shown. The cursor's, the carried, the mid-drag, the dropped, both
  column and the crowded frames pulled.
- `mirror.sh` `--mirror`: the launcher's camera mirror against two v4l2loopback
  nodes (built by the VM rig, loaded by the leg) fed by real ffmpeg
  writers, a colour pattern on video10 and a GREY one on video11 standing in
  for an IR sensor, alternating a dim frame and a near-black one the way an
  emitter lights every other frame. The view opened with no `/dev/video*`
  at all (No camera, no node held), then reopened on the colour camera with
  non-flat coloured pixels in the feed box and only that node held open by
  the shell, a real Tab stepping to the grey camera (grey pixels, the other node released),
  `mirror previous`/`next` over IPC, and the launcher closed with the shell
  holding no video node, all read off `/proc/<pid>/fd`. On the IR camera
  `mirror status` reports `irFilter`, and ten frames grabbed in a burst
  never show the dark frame and show the dim one levelled up: mean and
  spread each at least 1.3 times the raw frame's, measured on the
  `picture` rect against both frames rendered by ffmpeg to PNG.
- `monitor.sh` `--monitor`: the monitor bar cell, its panel and the
  launcher's monitor view, against this machine's own `/proc` and `/sys`.
- `motion_art.sh` `--motion-art`: the animated Apple Music cover in the
  bar's now-playing cell and the media panel with no network, its mp4
  already in the lookup's cache and flipping red and blue every half second
  over a flat green static art: crops of the bar cell taken while playing
  carry both colours off one ffmpeg child, the panel's frames too with that
  one child shared, two frames after a pause carry the same one in both
  slots with the bar not committing, the bar alone keeps its decoder once
  the panel closes, and `media.animatedBarCover: false` written under the
  running shell leaves no decoder, the static art in the bar and the bar not
  committing, even with the panel animating over it.
- `multi_output.sh` `--multi-output`: a wallpaper, a bar and four frame
  zones (none with no ring drawn, off `debug dump`) on every
  output, read off `hyprctl -j layers`, with a headless
  second output created after the shell started at scale 1.25 (g815's
  laptop panel shape), each output grabbed with `grim -o`, and the second
  removed again leaving the first's chrome alone. A fullscreen window on
  the second unmapping its bar, and once it closes every cell the window
  and the clock did not change back as it was, per cell crop, with the
  pointer kept off the bar. The screensaver on both
  outputs, a `formalshell:screensaver` layer and a mostly black frame on
  each, dismissed off both by one real pointer move on the second, then
  the second removed and created again under a running screensaver and
  given an overlay of its own; and the lock drawn on both outputs, lifted
  by the real password.
- `nix_run.sh` `--nix-run`: a menu.jsonc row carrying `@ipc:nix.run:hello`
  run through `menu activate`, read off `hyprctl clients` and `pgrep`: a
  `formalshell-console.run` window running `nix run nixpkgs#hello; read`,
  and no quake console spawned by it.
- `nightlight.sh` `--nightlight`: the wlsunset-backed night light on and off,
  honest about a session that cannot gamma-control.
- `no_morph.sh` `--no-morph`: pins `motion.morph` false (a
  `settings_pin`) and rides any leg, every card then dropping from its cell
  as a popover under every table; asserts `debug dump`'s `morph` false.
- `notify.sh` `--notify`: the toast stack collapsed and expanded, critical
  holding the front slot over a newer normal, the icon resolution order over
  three cards, and the layer surface the same size either way.
- `notify_close.sh` `--notify-close`: one sticky critical notification closed
  by a real pointer parked on its close button and a real click, `notifications
  status` counting one popup parked and none after: the bubble's button
  only exists while the card's hover holds.
- `notify_emerge.sh` `--notify-emerge`: one sticky notification arriving at
  a tenth speed, read as a ladder down the card's own centre column: the
  bubble unfolds off its top edge, so the covered rungs are always the
  shallow ones and at least one frame carries a fraction of the card. Then
  at full speed the card at rest committing nothing for four seconds.
- `ocr.sh` `--ocr`: the `capture` target's text and colour verbs against a
  window carrying known text on a known background.
- `overnight.sh` `--overnight`: overnight enabled and disabled over IPC, the
  restore record's full shape in `overnight status` (the rig's honest
  backlight -1, no LEDs, no asusctl), the bar's moon glyph in the frame,
  and state.json's record gone after disable. The screens' dim and restore
  are `--brightness`'s, against its shims.
- `osd.sh` `--osd`: the pill's own entrance off the bottom line sampled frame
  by frame under `debug motionScale`, ink on the output's very last row early
  and a plain pill a `screenPadding` clear of it at rest, then at full speed a
  manual call, a real `wpctl` change, and a machine with no backlight.
- `overview.sh` `--overview`: HyprExpo, the workspace overview the
  owner's spec builds as the fspec package `hyprexpo` (`FS_HYPREXPO` points
  at another build), loaded only by this leg and only while `hl.version()`
  is the Hyprland its `hyprland-version` file names: `hyprctl plugin list`
  naming it, `plugin:hyprexpo:columns` taking hl.config's 3 and `debug
  dump`'s `overview` true. Under `layout: sidebar` it is opened by Super+O,
  by `workspaces overview open`, by a 900ms press on a minimap icon, by an
  icon dragged off its row and by the top-left hot corner (not again
  while parked under it, again after a leave), each read off
  `hyprctl submap` (the plugin's `hyprexpo` submap while it is up), and
  closed over IPC; g815 only, since its hooks need x86_64. The bind's and
  the IPC's frames differ from the desktop's and are read by eye for the
  grid. The plugin takes every pointer event while it is up, so the
  minimap hands it the gesture and the release lands on the overview.
- `panel.sh` `--panel <name>`: one popout opened over the `panel` route, with
  `panel state` agreeing it is the only open one.
- `panel_anchor.sh` `--panel-anchor`: `panel toggle` hanging its card under
  the cell that owns the panel, read off one crop of the box under a
  left-region audio cell, the card in it once and the bare desktop once.
- `panel_at.sh` `--panel-at <n>`: `panel toggleAt <n>` walking the resolved
  right region and stopping on a panel-bearing cell.
- `panel_emerge.sh` `--panel-emerge`: `panel open network` sampled frame by
  frame, the drawer open: a ladder of probes down the card's gutter showing
  its visible height grow out of the bar's inner line to the resting rect,
  and no card pixel inside the bar's own band in any frame. Ridden by
  `--no-morph` it reads the popover: the ladder whole or empty on every
  frame.
- `panel_handoff.sh` `--panel-handoff`: `audio` alone in the left region and
  `network` alone in the right, so the two cards rest ~1500px apart, and
  `panel toggle audio` over an open network panel is one card rather than
  two crossing: five mid-flight frames, none carrying a card at both
  resting places, one of them carrying a card between them, and the settled
  frame equal to the audio card's own rest frame over the band under the bar.
- `panel_keys.sh` `--panel-keys`: row-level keyboard navigation inside a
  panel, driven by real keystrokes rather than the IPC shortcuts.
- `panel_morph.sh` `--panel-morph`: a card whose content changes size
  travels to the new one, read off its far edge against the two rests: the
  Wi-Fi list collapsing under a settled card, the same collapse landing two
  seconds into a five-second emerge, and the now-playing card growing from
  NO PLAYER to a real track. The band is cut for a top bar with no frame, so
  a run on another edge drives the same three cases, prints each claim as
  skipped and is read by eye.
- `picker.sh` `--picker`: the wallpaper grid, the pick becoming the
  wallpaper, the select token round trip, the Dark/Light variants, and
  the prerendered thumbnail cache (`fs-menu` `thumbnails`) backing every
  cell (the cache
  directory's own contents, since a warmed cell and a fallback cell paint
  the same picture), with no colour swatches ahead of the pictures. The
  Sky cell after the pictures, picked like any other, putting the sky on
  the desktop with state.json's `wallpaperColor` reading `sky`, and a
  select request listing no Sky cell.
- `plugins.sh` `--plugins`: a plugin directory dropped into the config home
  placing its own bar cell, no `bar` key involved; panel, overlay and
  crashing plugins; and a `launcher` plugin running only while the
  launcher is open, its mode a token over the rows it prints, a typed
  query reaching it as a `query` event and answered, its rows in a root
  search, and its action joining a recent file's column and reaching it
  with the file's id.
- `polkit.sh` `--polkit`: a real `pkexec` conversation through the shell's
  agent, the prompt, the error state and pkexec's own exit code, with the
  card's own open off the top line sampled at a tenth speed first. Before
  the real password, junk typed, selected with Ctrl+A (photographed over
  the dots) and deleted with one Backspace, which pkexec's exit 0 proves
  left the field empty.
- `power.sh` `--power`: the launcher's Shutdown and Reboot rows, which
  call logind over D-Bus with interactive auth (`services/power.rs`), with a
  second user's login session open on seat0 so logind answers
  `CanPowerOff` with challenge. Shutdown typed on real keys: the first
  Return only arms the row, the second raises the shell's polkit prompt, a
  wrong password lands in polkitd's journal against
  `power-off-multiple-sessions` and the shell, and Escape leaves a critical
  "Shutdown failed" toast; Reboot is cancelled the same way. The real
  password is never typed: a runtime-masked `reboot.target` and a root
  block inhibitor both failed to stop logind on systemd 261, and the VM
  rebooted.
- `processes.sh` `--processes`: the process table's search, the two-press
  TERM, and `monitor restart` re-running the same argv under a new pid.
- `record.sh` `--record`: `record` start to finished GIF through a real
  wf-recorder child, with the bar's recording cell mid-run.
- `r0_measure.sh` `--r0-measure <seconds>`: R0's numbers on one
  timeline: CPU off `/proc/<pid>/stat` over
  60s idle and 60s with an agent badge spinning, ten panel opens and ten
  scrim fades, the launcher stall (first frame after `menu toggle`, longest
  gap between bar frames), RSS at `<seconds>`, and the launch stamp a cold
  start counts from. Records, never judges: the budgets are read by hand.
- `radio.sh` `--radio`: Radio Atlas's mpv tuned with `radio play` to a
  favourite served on loopback, read back as a media source (`media status`
  kind, title and playing, `media players` listing it once, its own Pipewire
  stream not listed again as an app), the media panel's source menu opened
  on real keys and photographed, and the source gone after `radio stop`.
- `radio_atlas.sh` `--radio-atlas`: Radio Atlas opened with `panel open
  radio` over a world cache holding one loopback-served station, Enter on a
  real key playing it and turning the globe to centre it (the globe's box
  differing from the open frame), and after `radio stop` a real click on
  the globe's centre picking it again, read off `radio status`; then a
  held drag over `dev/vpointer.py` that stops before its release leaving
  the globe's longitude where the release left it, and one let go while
  moving coasting less than 2.5 times its own travel. Escape closes it.
  The globe wears the packaged Blue Marble (`packaging/earth-tiles.py`, a
  512px tile pyramid 2700 to 21600 wide under `FS_EARTH_TILES`):
  `radio status` reports `imagery` true and a square
  of the disc carries saturation and colours the flat globe never had,
  the whole disc's mean luminance clears 0.18 (the untoned picture read
  0.127), and every globe build and card commit is summarised as
  `SMOKE_RADIO_ATLAS_FRAME`. Then a real wheel to scale 24 over the
  station on the Tibetan plateau: the 21600 level with no tile missing,
  VmRSS falling 16 MB or more once Escape closes it, and with the tiles
  link gone the same pose on the single 4096 `FS_EARTH_IMAGE` carrying
  under half the tiles' high-frequency detail; a drag on each gives the
  `SMOKE_RADIO_ATLAS_ZOOM_FRAME` timings.
- `radio_atlas_cold.sh` `--radio-atlas-cold`: Radio Atlas opened with no
  world cache, a frame a second in and another once `radio status`
  reports stations (fetched over the real network and cached) or the
  honest unavailable error, neither frame a flat card.
- `reactive_rims.sh` `--reactive-rims`: the cards' reactive rims
  (`docs/superpowers/specs/2026-10-30-reactive-rims.md`) over a wallpaper
  red on its left half and blue on its right, `audio` alone in the left
  region and `network` in the right: each card's rim leaning to its half
  in `debug dump`'s `rims.panel` and in the frame's 1px edge columns, past
  what the fill a few pixels in carries; no `commit surface=panel` in 5s
  at rest with the window poll running; a floating foot window moved
  beside the network card by Hyprland's own dispatcher (no event) lifting
  and brightening the facing rim along the stretch it spans, the fill
  10px inside that edge brighter than 10px inside the far one and moved
  by less than the rim; and `theme.reactiveRims: false`
  retargeted under the running shell putting the flat border back.
- `reminder.sh` `--reminder`: a real countdown firing inside the run and
  bypassing DND into the popup tier.
- `render.sh` `--render`: `render.backend` resolved at start, three
  shells in one session, each replacing the last: `gpu` drawing on the GPU
  in a nested session (`debug dump`'s `render` block naming the render
  node) and falling back to cpu with the reason on a vkms rig, `cpu`
  asked, and `gpu` under a `VK_DRIVER_FILES` naming no driver falling back
  saying no Vulkan device was found. Each opens the launcher: the card's
  crop off `menu status`'s `cardBox` not flat, and the GPU's and the
  fallback's within 3% RMSE of the CPU's. Runs alone.
- `screensaver.sh` `--screensaver`: the live media guard, auto-activation off
  the idle timer alone, and the manual start/stop path.
- `screensaver_gif.sh` `--screensaver-gif`: five ttfx effects recorded frame
  by frame into `docs/media/`, one session each, taking the run over.
- `screensaver_inhibit.sh` `--screensaver-inhibit`: the shell's
  `org.freedesktop.ScreenSaver` (`services/inhibit.rs`), the way Steam and
  SDL inhibit, with caffeinate's toggle off: a Gio client that stays on the
  bus taking a cookie holds the inhibitor and is listed in `caffeinate
  status`, the session not going idle three timeouts past, and once it is
  killed its cookie goes off NameOwnerChanged and the timeout fires the
  screensaver, read back over `GetActive`. A one-shot `busctl call` on
  `/ScreenSaver` loses its cookie as it exits, and a second client's
  `UnInhibit` releases its own while it is still on the bus.
- `screenshot.sh` `--screenshot`: the `screenshot` target's region cancel and
  full-screen routes.
- `share.sh` `--share`: the share route present (the copied text reaching
  LocalSend as a real file) and honestly absent with no binary on PATH.
- `sidebar.sh` `--sidebar`: `layout: sidebar` from settings.json alone,
  the formalshell:bar layer read off `hyprctl -j layers` as a column 154
  wide down the left edge (Pond's sidebar at 136/156, the cards 9 off the
  screen and 9 off the windows), under Hyprland's `gaps_out` 10 the fixture
  window's border as far off the cards as the cards are off the screen's
  edge (the column's zone leaves the outer gap to the compositor, read at
  runtime off `getoption`), `sidebar status` naming the time, Spaces and
  notifications cards and a window on the map, a real click on the status
  row's network button opening its panel, one on its collapse button
  folding the column to the 48 wide rail with state.json carrying
  `sidebarCollapsed`, `sidebar expand` opening it again, and settings.json
  rewritten with `bar.position: right` moving it to that edge. The open,
  panel and rail frames are read dark and light (`--sidebar --light`).
  The time card's clock over a staged red and green wallpaper with a
  depth-mapped orange band, read off `debug dump`'s `sidebarClock`: the
  desktop clock's pixel face frosted, red at the digits' left end and
  green at their right, the band's orange laid back over their middle,
  the date box carrying its words, `clockFormat` "h:mm AP" read as the
  bar clock's own output and `date`'s, the draw under 50ms, and no bar
  commit in 3s at rest.
  Ridden by `--idle`, `--menu-budget`, `--tray` or `--tray-overflow` it
  pins the layout alone, and `--menu-budget` skips its bar frame claims,
  the sidebar having no badge to spin.
- `showcase.sh` `--showcase`: seeds state.json with a generated wallpaper
  before the shell starts, so the leg it rides photographs a matugen palette
  over a real desktop. The README's screenshots and GIFs are taken under it.
- `shots.sh` `--shots`: rides `--showcase`; a camera, not a check. Takes
  `docs/shots/` with demo data (a pink noise track, made-up notifications,
  `USER=kyan` for the lock's name): launcher and calendar panel bursts at a
  third speed encoded to H.264, the media card, six visualizer styles,
  notifications, the centre, the lock, and the decrypt screensaver frame
  stepped.
- `shoulders.sh` `--shoulders`: rides `--bar-style solid` or `corners` (the
  transparent band has no line); a join published over `debug join`, the gap
  the bar opens in its own line read off the shell's numbers as the card's
  rect plus a `radiusXl` fillet at either end, and the pixel where the line
  stops and the arc starts.
- `sky.sh` `--sky`: the sky wallpaper (`wallpaper.color: "sky"`) over a
  pinned Amsterdam: the session starting on the sky for the sun now
  (`debug dump`'s `sky` block located, unpinned, a next change), a picture
  over IPC taking it down and `wallpaper set sky` putting it back with
  state.json's `wallpaperColor` reading `sky`, then `debug skyAt` pinning
  noon, the low sun, dusk and night of one day in UTC: each its step in
  the dump and its own frame (blue down the screen, the foot warmer than
  the top twice, near black), theme.json's primary differing between noon
  and the low sun, and no desktop commit in six seconds under a sky that
  did not move.
- `sleep.sh` `--sleep`: `sudo systemctl suspend` with the session unlocked,
  the real logind path with only the kernel's freeze stubbed, the lock read
  back after wake: the stub's millisecond stamp has to come after the shell
  let its delay inhibitor go with the surface secure, and the `FormalShell`
  inhibitor shows in `systemd-inhibit --list` before the suspend and again
  after wake. All of it in light mode over a dark wallpaper at a tenth
  speed with quarter-size frames grabbed throughout: the lock black while
  logind holds the machine asleep, fading in after wake, blanking on
  `lock.blankAfterSeconds` and waking on a real Shift, and from the first
  black frame on no frame's mean luminance past the settled lock's (the
  white flash a light theme's background put through every fade).
- `sidebar_activity.sh` `--sidebar-activity`: the Activity card under
  `layout: sidebar`, Pond's installs card: three stubbed fspec operations
  (Firefox installing at 40%, a long-named install with no measure, an
  apply of a package build) as three 32-tall lines straight under the
  notifications card, read off `sidebar status`, tesseract reading
  "Firefox" whole off the card's crop, and the install's line reading done
  once it succeeds. The build's ring follows FormalSpec's `step` while its
  own count sits at 0 of 3: indeterminate fetching dependencies, 120 then
  300 of 450 units compiling, indeterminate and "no output" once
  packaging stalls.
- `sidebar_join.sh` `--sidebar-join`: `layout: sidebar` over a flat grey
  wallpaper, the network panel and the launcher each opened at a tenth
  speed with a burst of frames, the card read as what is darker than the
  scrimmed grey or well lighter than the grey right of the column: its
  first frame on the column's edge, a frame still on it part way out, none
  further off the edge than it rests, the panel resting within the bar's
  margin of the edge and the launcher travelling on to rest centred in
  the area right of the column, within 2px both ways. Read dark and
  `--light`.
- `sidebar_software.sh` `--sidebar-software`: the sidebar's Software card
  against a PATH-shimmed `fspec` and a fixture spec checkout
  (`FSPEC_SPEC`) cloned from a bare upstream 22 commits ahead, the
  oldest fspec's own `feat(apps): install ... on <this host>` and 21
  newer ones for another host, read off `packages status`'s `software`:
  22 updates behind `origin/main` and `Installed Foxglove`; a real click
  on Apply fast-forwarding the checkout before reaching the shim as
  exactly `apply --rev HEAD`, the shim refusing it and the row keeping a
  retry with nothing behind, and a second click applying, the card then
  up to date; a real click on Undo
  reaching it as `app undo --detach`, the shim pushing fspec's uninstall
  commit and the card re-checking to one update and `Removed Foxglove`;
  and the branch's upstream unset reading the honest error row. The
  behind, retry, up-to-date and undone cards photographed, dark and
  `--light`.
- `spaces.sh` `--spaces`: the Spaces cell over windows on two workspaces,
  each listed with its icon under its own chip in `workspaces status`, every
  occupied chip showing its icons and every chip its ordinal, and the chip
  on screen wider than a bare one; a real wheel notch over the cell moving
  focus there and back; `workspaces peek 2` and a real pointer parked on
  that chip both opening its preview with both windows captured live (the
  hover open without taking the keyboard, and staying open while the
  pointer keeps moving over the chip), and the pointer leaving closing it;
  the card and the chip row cropped for reading; a floated window of
  workspace 2 moved mostly past the output's right edge, the miniature
  opening on the output's own region over a wider strip with that window's
  thumbnail at its full size beyond the viewport, a real wheel notch over
  the peeked card scrolling the strip along x and back, and a horizontal
  axis event (a trackpad's sideways swipe) doing the same on the
  hover-opened card without closing it; and the T3 Code agent badge read
  off an `ssh` shim answering t3-agents for `fakehost` (and refusing
  `deadhost`), on the T3 Code window only, agreeing in `debug dump`,
  `workspaces status` and the frame, where the blocked `!` is red and gone
  once fakehost reports no threads.
- `spectrum.sh` `--spectrum`: the media panel's own spectrum band, a cava
  child wanted by the open panel alone with no `visualizer` cell anywhere in
  bar.layout, pgrep proving it appears on open and dies on close.
- `speedtest.sh` `--speedtest`: `network speedtest` settling both phases in
  the network panel.
- `switcher.sh` `--switcher`: the Alt+Tab switcher under whatever theme the
  run rides (it pins none), three
  windows of the fixture's own app id on the focused workspace and a fourth
  on workspace 2 that must not be offered (the leg pins
  `switcher.currentWorkspace`, off by default), `switcher next` twice and
  `prev` once over IPC against `switcher state`, the frame read for the
  fixture icon in three captions and for the selection fill travelling
  from the third cell to the second (cell positions off `switcher state`'s `cells`), all
  three thumbnails holding a captured frame while open (one frame each,
  the selected one refreshed five times a second, never `live`) and none
  holding any capture source after the commit, then a commit landing focus
  on the window the card named (`hyprctl activewindow`).
- `switcher_keys.sh` `--switcher-keys`: the same card driven by the
  compositor's own binds and real keys instead of IPC, one `wtype` process
  holding Alt across two taps of Tab and letting go, with the same fourth
  window on workspace 2 held out of the card. Reads the card open on
  the third entry mid-hold, closed with focus moved on the release, and two
  probe binds on the same key beside the shipped one: a plain `bindr` that
  must not fire (Hyprland shadows a held key's release bind once another
  bind consumes a press) and a no-mods `bindrt` that must not fire either
  (the modifier is still held as far as the bind table is concerned). A bare
  Alt tap first, which has to leave the plain probe's marker. Three fast
  taps close it out, each one wtype process with no sleep between the Tab
  and the Alt release, asserting the active window alternates every time
  rather than a commit racing ahead of the next that opened the card, and
  none of them mapping the card (`switcher state`'s `shows` and a polled
  layer list): it shows only 150ms into a hold. Then nine windows and one
  hold: Tab held on a slowed repeat stepping the cursor several entries
  (the Tab binds carry `repeating`), Shift+Tab with Alt still down stepping
  back one and the release landing there, and all nine thumbnails captured.
  Last the scrolling layout, where Hyprland copies no window whose box
  misses its monitor: the cells with no frame are exactly the windows off
  the monitor, drawn with their icon.
- `switcher_off.sh` `--switcher-off`: the same target with
  `switcher.enabled: false` in the settings fixture: all five verbs answering
  the error string and no switcher surface mapped at all.
- `systemupdate.sh` `--systemupdate`: the flake-inputs-behind cell and panel
  reading a fixture flake (`dev/smoke.d/fixtures/flake`) through one shared
  poll.
- `theme_auto.sh` `--theme-auto`: `theme.mode: "auto"` resolving off the
  fixture's own coordinates rather than the 20:00 to 06:00 fallback, the
  sunrise/sunset pair read back out of `theme status` and checked against
  the wall clock it was sampled at, and one toggle snoozing the schedule for
  a cycle: the override reported, carried in state.json, and expiring on a
  boundary.
- `theme_toggle.sh` `--theme-toggle`: `theme mode toggle` both ways, and with
  `--wallpaper` that a toggle re-runs matugen instead of resetting to the
  fallback palette.
- `toast_swipe.sh` `--toast-swipe`: one sticky critical toast swiped by
  two fingers off `dev/vpointer.py` (`hfinger`, `hlift`) with the pointer
  parked on it: ~22px slowly carrying the card right mid-gesture and
  springing it back onto its place on the lift, one popup left; ~67px
  lifted with no speed left dismissing it (sonner's 45px); a fresh card
  flicked ~17px in 16ms dismissed on its speed (sonner's 110 px/s); and a
  sideways wheel notch over a third one moving and dismissing nothing,
  read off `notifications status` and the card's left edge in each frame.
- `toggles.sh` `--toggles`: the toggle hub's rows repainting from a `@state:`
  snapshot without the surface moving under them.
- `tooltip.sh` `--tooltip`: rides `--panel <name>`; the tooltip surface
  absent before the pointer parks on a header button and present after.
- `tooltip_travel.sh` `--tooltip-travel`: rides `--panel <name>`; the card
  parked on the header's close button, then the pointer one button left and
  a frame 250ms later showing one card, on the rescan button, with one
  tooltip layer in both dumps: the group's grace window, not a second delay.
- `tray.sh` `--tray`: six real StatusNotifierItem producers on a strip pinned
  to carry them (`tray.maxVisible: -1`), the D-Bus Activate round trip, and
  the shell-owned menu. Ridden by `--sidebar`, which has no strip, the
  status card's tray button in `sidebar status`, a real click on it
  opening the second bar off the column, a real click on fixture 1's
  square (found by its colour) reaching the stub as Activate and a real
  right click on fixture 2's opening its menu.
- `tray_overflow.sh` `--tray-overflow`: the shipped default, the whole tray
  in its second bar (the `trayoverflow` panel, in the panel host like any
  other) behind the strip's dots toggle with no settings at all, read off
  `tray status`. Ridden by `--sidebar`, the second bar opened off the status
  card's tray button with a sticky toast arriving under it: the panel's rest
  rect and every toast card's rect in `debug dump` (`panel`, `toastCards`)
  sharing x and never intersecting, both photographed. Ridden by `--sidebar` the status card's tray
  button stands in for the dots: the chevron round trip is skipped and the
  four real clicks (open, desktop, reopen, the time card's calendar) start
  on that button.
- `unlock.sh` `--unlock`: the lock handing over to the desktop
  (`wayland/veil.rs`): over a flat grey wallpaper the real password typed
  at a tenth speed and the unlock photographed back to back, each frame's
  mean grey read against the locked and unlocked frames: none outside
  them (no flash), never turning back, at least one part way between (a
  fade, where a cut has none), and the last ones the desktop.
- `visualizer.sh` `--visualizer`: the `cava` child owned and killed with
  playback, proven by pgrep rather than by the frame.
- `visualizer_styles.sh` `--visualizer-styles`: pink noise through a real
  MPRIS player so every band actually differs, the media panel's spectrum
  box found by pixel diff rather than a hardcoded rect, every id `visualizer
  styles` reports set over IPC in turn and cropped to `visualizer-style-
  <id>.png`, no crop empty and no two consecutive ones byte-identical. Then
  the player volume forced to 100 and `visualizer status`'s levels read back
  once cava has settled, none of them pegged above 0.9, with both stereo
  channels present and not silent.
- `wallpaper.sh` `--wallpaper`: the matugen recolour on a set wallpaper, the
  crossfade to a second one, and both sides of the opt-in dither key.
- `wifi.sh` `--wifi`: the network panel against two real hostapd radios:
  scan, wrong password, connect, forget, and the enterprise round trip.
  First a saved profile for an SSID neither radio broadcasts: `inRange`
  false in `network status`, left out of the Networks list and its count,
  and under the closed "Known networks" disclosure until real Down, Up,
  Up and Return open it (`knownOpen`), read off tesseract over both frames.
- `wheel.sh` `--wheel`: a virtual-pointer scroll moves the picker grid
  (`menu status` `scrollTop`) without moving the cursor or the cell the
  pointer tints (`hover`), and a wheel over the
  bar's audio cell still steps the volume. Between the two, touchpad frames
  off `dev/vpointer.py` (`finger`, `lift`): 100px of finger travel moving
  the grid 112 (`scroll.rs`'s gain), a held pull past the end reading
  `scrollTop` past `scrollMax` by under 40px, exactly on it 400ms after the
  lift, and a wheel notch at the end never past it in five reads. Then the
  coast after a lift (`scroll.rs` `Touchpad`), status read back to back: a
  70 px/s drag lifted at once not moving after, a fast flick's `scrollTop`
  rising after the lift, never back and slower each third, settled within
  1.3s no further than one `viewHeight` past the lift, a finger 150ms in
  stopping it short of that, and the same flick near the end bouncing under
  40px of `overscroll` and settling exactly on `scrollMax`, the last row
  whole in the frame.
- `workspaces.sh` `--workspaces`: the bar's workspace indicator as one pill
  that travels, read off the bar region alone before, 80ms into, and after a
  workspace switch.

## Hosts and the g815 rig

Sessions run from the macbook, which has no Wayland at all, so every build,
check and smoke run happens on g815 through `dev/remote.sh` (the
verification loop above), or on the mac rig while g815 is off. g815 and e1504g are both openSUSE Tumbleweed
managed by fspec (FormalSpec); the owner's spec is `~/.config/formalspec`
(FormalSnake/dotfiles, branch `main`). Both are reachable over ssh with
passwordless sudo.

The rig lives in that spec: `hosts/g815/rig.star` declares the slot users
`fstest0` to `fstest7` (uids 1001 to 1008, group `fstest`, plus `render`
and `video` for the Intel render node, each slice at `CPUWeight=20` so a
slot yields to the desktop), their `formalshell-rig-parent` user service (a headless
labwc that writes its socket name to
`$XDG_RUNTIME_DIR/formalshell-rig-display`) and the tools the legs need.
A slot user picks up a new group only when its user manager restarts
(`sudo systemctl restart user@<uid>.service`). A leg that needs a tool g815
lacks fails naming the openSUSE package; add it to the spec, not to the
leg.

Both hosts run FormalShell as the fspec package `formalshell`, built from a
pinned rev with `dev/tarball.sh` in debian:bookworm (`system.star`). A
change reaches them like this:

1. Push it to `FormalStudio/FormalShell`.
2. Bump the formalshell `rev` in `~/.config/formalspec/system.star`.
3. `fspec update formalshell` there, then commit and push the spec.
4. `fspec apply --host g815` and `fspec apply --host e1504g` (g815 builds
   for e1504g; never compile on e1504g).

e1504g's `/etc/pam.d/formalshell-lock` is declared in that same
`system.star`. The hosts are the place to confirm what the nested rig
cannot show (real GPU rendering, a real session bus owner, real hardware
devices, e1504g's power-saver budgets), but they are NOT a test target: the
host-session-safety and lock-screen rules below forbid running the shell,
the theme engine or any compositor action against a live session there.
Apply and look; anything that drives a surface goes through the nested
rig, which on e1504g means `dev/smoke.sh` with `FS_RESULT` pointing at a
result built on g815 and copied over.

The mac rig lives in the same spec: `hosts/fstestmac/lima.yaml` is the VM
(its first boot builds an fspec for the guest), `hosts/fstestmac/host.star`
what runs in it (the slot users, `formalshell-rig-vkms` making a card per
slot through vkms's configfs, Hyprland and the legs' tools, wlrctl built
from source since Tumbleweed carries it for x86_64 only, and the
formalshell package built in the guest for aarch64). Recreate it with
`limactl delete fstestmac`, `dev/mac-vm.sh start`, then `dev/mac-vm.sh
apply`. fspec's own `vm()` drives qemu on a KVM host only, hence the
script; aarch64 Tumbleweed has no history snapshots, so the guest's repos
roll and `fspec.lock` pins none for it.

The VM rig lives there too: `hosts/fstestvm/host.star` declares the four
guests (4 CPUs, 3 GiB each, KSM on g815 merging what they share) and what
runs in them, and FormalSpec's `fspec vm` (`start`, `stop`, `save`,
`resume`, `status`, `ssh`, `record`, `type`, `key`, `frames`) drives them.
A run never boots one: it resumes the slot's save and stops it after.

Screenshots and JSON always land on the **mac** filesystem under
`./artifacts/` (gitignored); Read-verify them there exactly as you would on
a Linux host.

## Hard rules

- **Host-session safety**: the owner's live session is NOT a test target.
  Never run the shell, the theme engine, or any compositor action in an
  environment carrying the host's `HYPRLAND_INSTANCE_SIGNATURE`. All runtime
  testing happens inside nested sessions via `dev/smoke-*.sh` (which scrub and
  restore the env). If you must run `formalshell` or `formalshell-ipc` ad
  hoc, unset that variable first or export the nested session's own
  explicitly. Observed failure mode: host compositor config reloads firing
  during isolated testing (2026-07-27).
- **Lock-screen safety**: never run a lock surface (the ext-session-lock
  surface in `crates/formalshell-rs/src/wayland/lock.rs`) against anything
  but a nested test session. All lock testing happens inside the nested
  Hyprland session `dev/smoke.sh` boots and tears down; a lock bug there is
  harmless (the whole nested compositor gets killed regardless), but the
  same bug against a real host session would leave it genuinely locked.
  This is the same nested-only contract the host-session-safety rule above
  already establishes, called out separately because a stuck lock is a much
  worse failure mode than a stuck bar.
- **D-Bus isolation**: the shell acquires `org.freedesktop.Notifications` on
  the session bus (`crates/fs-notifd`). The owner's live session bus is
  owned by DMS on the Linux hosts. NEVER run the shell's notification server
  against the host bus: acquiring that name would steal it out from under
  the real desktop. `dev/smoke.sh` wraps the whole nested
  compositor invocation in `dbus-run-session --`, giving every nested run (and
  `notify-send` fired inside it) a private bus; it asserts `busctl --user status
  org.freedesktop.Notifications`'s owner PID on the **host** bus is unchanged
  before and after every run (`|| true`-tolerant of a legitimate "no owner"
  answer, e.g. on a VM with no desktop bus owner at all).
  A nested run on a real host also gets a private, empty system bus
  (`DBUS_SYSTEM_BUS_ADDRESS`), so the shell never reaches the host's
  NetworkManager, BlueZ, UPower, logind or polkit (a nested run took g815's
  Wi-Fi down, 2026-10-09); legs that need those declare `leg_<n>_vm_only`
  and are refused there; `dev/remote.sh` sends them to the VM rig, whose
  system bus is the guest's own.
- **Design language**: every UI surface follows `docs/DESIGN.md`: shadcn/ui
  chrome (`card` fill, 1px `border`, `radiusMd` controls and `radiusXl`
  cards, one `ring` for focus, `accent` hover, `primary` for the wallpaper
  colour) on Omarchy quattro's surface set and habits. Read it before
  building or restyling any surface. mek.gallery and the ledger grammar are
  gone (2026-08-25).
- **`panel` IPC target is a spec addendum, not a conflict.** The design
  spec's §IPC target list (`docs/superpowers/specs/2026-07-27-formalshell-design.md`)
  predates per-widget popouts and doesn't name `panel`. The M6 plan added it
  (`panel.open(name)`, `close()`, `toggle(name)`, `state()`) because
  per-widget popouts otherwise have no summon path for compositor keybinds
  and no way to be verified headlessly in the smoke rig; treat it as part
  of the IPC contract going forward, alongside `menu`/`osd`/`notifications`/
  `clipboard`/etc. Unknown panel names return an error string, never a
  silent no-op.
- **Honest unavailable states, never faked data**, as a standing expectation
  for every smoke run: a panel/widget with nothing to show from its
  backend renders a single dim cell (`NO ADAPTER`, `NO DEVICES`,
  `NO LOCATION`, an absent battery bar cell, ...) rather than a stubbed
  value or an invented device. Enabling a real service in the VM rig
  so a panel has a genuine backend to talk to is the sanctioned way to make
  a screenshot show more; inventing fake `/sys` entries or synthetic
  devices is not. The one exception (owner, 2026-09-28): with the iPhone as the
  active media source, whose audio never reaches this machine, the
  visualizer draws a frame off the track's Deezer bpm
  (`fs_media::visualizer::model::beat_frame`, 120 when Deezer has none)
  instead of cava.
- One binary, no Node/npm/bun anywhere. Third-party CLIs the shell shells
  out to (matugen, grim, cava, ttfx, ...) stay child processes, spawned and
  read on the service thread, never linked in, and every one of them has an
  honest fallback or unavailable state when it isn't installed.
- **Everything ships** (owner, 2026-09-28 and 2026-10-07): never tell a
  user to install something by hand. `install.sh`'s one name table for
  pacman, apt and dnf covers what those distros carry, and the release
  tarball (`dev/tarball.sh`) carries the rest in `lib/formalshell/bin`,
  ahead of PATH in its launcher. The owner's hosts install that tarball as
  the fspec package `formalshell`, and every system piece it needs (a
  daemon, a D-Bus policy, a firewall port, avahi, the PAM file) is declared
  beside it in the fspec spec. A new runtime CLI goes into the name table
  or the tarball in the same commit, and into the spec for the owner's
  hosts.
- **ttfx is a spec addendum, not a conflict.** Spec §10 says the
  screensaver draws TTE-style rain/decrypt/matrix itself in the shell's
  mono font, with no spawned terminal windows. The screensaver runs `ttfx`
  (built into the release tarball by `dev/tarball.sh`) as a frame source and parses its ANSI stream
  (`crates/fs-screensaver/src/ttfx.rs`), still drawing every glyph itself
  in the shell's mono font with no terminal window anywhere. The palette
  comes from ttfx, since each effect carries its own gradient (owner's
  call: match omarchy exactly, 2026-08-11). `fs-screensaver`'s own
  `effect.rs` is the engine for an install with no ttfx on PATH.
  `screensaver frameInfo` reports which engine is live.
- Compositor window/workspace ids are **opaque strings** end to end. Never
  parse, compare numerically, or assume stability; only the backend that
  minted one takes it apart. Hyprland's window ids are hex addresses and
  reach its dispatchers verbatim, as an `address:0x...` selector built in
  `services/compositor/hyprland/lua.rs` and nowhere else; mango's are client
  ids, as a `client,<id>` selector built in
  `services/compositor/mango/dispatch.rs`, and its workspace ids are
  `<monitor>:<tag>`.
- The shell only ever **reads** `~/.config/formalshell/settings.json`; it
  never writes it. Runtime-mutable state goes to
  `$XDG_STATE_HOME/formalshell/state.json`.
- Chrome defaults (2026-10-29): every number below is the one table
  (`crates/fs-theme/formalshell.json`, elementary OS 8's material, read
  through `fs_theme::table()` and `fs_theme::Theme::box_style`); see
  `docs/DESIGN.md` §1 "The table" for the schema. Radius base 6, 1px
  `border`, the table's own casts and faces, no blur drawn by the shell (Hyprland blurs behind the bar/panel/launcher cards
  via layerrules; `theme.surfaceOpacity`, default 0.85, drawn at 1
  while the compositor's own blur is off, and the popover card opaque
  always), dither only behind
  `wallpaper.dither`/`lock.dither` (both default false), fonts = the
  fontconfig `sans-serif` alias for words and `monospace` for values (Geist
  Sans/Mono by intent, never a hardcoded family; values ask the face for
  `tnum`, and a face without it is logged once in `text.rs`), icons by name
  through `ui::w::icon` and `fs_theme::icons` (Lucide; no raw glyphs in
  surface code, no SVG icon assets). Nothing in the shell blurs anything: a modal surface sits over a plain 0.5 black scrim that only
  darkens the desktop (the modal namespaces take `ignore_alpha = 0.6`, so
  the scrim falls under the compositor's blur and the card over it stays
  above), every other surface sits over the desktop with its border doing
  the work. The lyrics pane is the one exception (owner, 2026-09-17, M56
  spec P12): depth of field on every line but the lit ones, behind
  `media.lyricsBlur`, and a glow on the chunk being sung. Radio Atlas's
  globe is the other (owner, 2026-10-09): an atmosphere glow and rim haze
  over a starfield, part of the illustration and kept inside its pane.
  The desktop clock's digits are the third (owner, 2026-10-27): frosted,
  filled with the wallpaper under them blurred.
  Motion is
  `docs/DESIGN.md` §1 "Motion": the two clock families in
  `crates/formalshell-rs/src/motion.rs`, and the joined shape a card hanging
  off the bar draws instead of a plain card.
- **Capture rule**: the shell captures windows in exactly three places, the
  Spaces preview (`wayland/preview.rs`, drawn by
  `surfaces/panel/workspace_preview.rs`), the Alt+Tab switcher
  (`wayland/switcher.rs`, `surfaces/switcher/`) and the launcher's preview
  pane (`wayland/launcher.rs`, drawn by `surfaces/launcher.rs`: one frame of
  the selected window, never live, held only while the pane shows that
  window), and only while their card is open. All three go through `wayland/capture.rs` (ext-image-copy-capture on
  each window's foreign-toplevel handle); closing the card drops every
  frame, session and source it held. The lock surface never captures
  anything, and no other surface gets window capture. Screenshots and
  recordings are grim and wf-recorder child processes, not this path.
- License MIT. Every file substantially ported from DankMaterialShell keeps
  a `// Portions from DankMaterialShell (MIT, Copyright 2025 Avenge Media LLC)`
  header line.
- Hyprland is the primary compositor; mango (0.18, a dwl descendant) is
  supported through a second backend (owner, 2026-10-13). Compositor work
  goes in `crates/formalshell-rs/src/services/compositor/`: the shared model
  (`model.rs`), the typed `Command`s every caller sends, and one backend per
  compositor picked once at start by `compositor.backend` (`auto` follows
  the session's instance signature). `hyprland/` holds the `.socket.sock`
  and `.socket2.sock` clients and `hyprctl eval` for writes; `mango/` holds
  the `MANGO_INSTANCE_SIGNATURE` socket client (two `watch` streams,
  `dispatch` lines for writes) and the config walker its keybinds route and
  layout list read. Anything mango cannot do (a runtime non-consuming
  Escape bind, output scale, mirror, HDR) is a logged no-op, never an error.
  The smoke rig runs Hyprland only.
- Lua (`hyprland.lua`) is the only Hyprland config format (owner,
  2026-09-29). Runtime changes go through `hyprctl eval` with the `hl.*` API,
  never `hyprctl keyword`, and nothing the shell or its installer writes is
  hyprlang. The smoke rig boots from a Lua config too.
- **Units live in types.** A percentage-shaped value keeps the unit its
  daemon sends and carries it in a type, converted only through a named
  method: `fs_upower::Percent` (UPower's 0..100),
  `fs_network::SignalStrength` (`as_percent`, `as_fraction`), and
  `fs-audio`'s visual-scale volumes (the cube root of PipeWire's linear channel volume).
  Mixing 0..1 and 0..100 has already shipped two bugs. A new service value
  gets the same treatment: read the daemon's own spec or source for its
  unit, never a raw `f64` crossing a crate boundary.
- Animations run at the output's refresh rate off its frame callbacks,
  moving by the clock with fractional offsets. Never a fixed low step rate
  or whole-pixel snapping to save CPU; make the per-frame work cheap instead.
- Commits: conventional style, lowercase imperative subject
  (`feat(compositor): ...`), no Co-Authored-By lines, no commit descriptions.
- Every task ends with its verification commands actually run and their
  output read. No claiming green without evidence.

## Reference repos

- `github.com/basecamp/omarchy` (branch `quattro`): architecture and UX
  reference (single-process shell, unified surfaces, IPC contract
  patterns). Read, don't copy; check its license before ever porting code
  from it.
- `github.com/AvengeMedia/DankMaterialShell` (MIT): Hyprland backend
  prior art and matugen orchestration patterns. MIT, so code can be ported
  directly, but every substantially-ported file needs the attribution
  header above.
- The crates the shell is built on (`smithay-client-toolkit`, `calloop`,
  `vello_cpu`, `parley`, `zbus`, `pipewire`, `pam-sys2`): docs.rs
  at the version in `crates/Cargo.lock` is ground truth. When an API's
  behaviour is uncertain, read the crate's source at that version rather
  than guessing.
