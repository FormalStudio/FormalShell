# FormalOS polish plan

Goal: FormalShell stops reading as a bar plus a launcher and becomes one
shell with one look, configured rather than themed, with the system flow of
Pond (butterfly.so/pond) on Hyprland and fspec. The specs under
`docs/superpowers/specs/` still hold where this plan is silent; where it
disagrees, this plan's spec (written in task 1.1) wins.

## Facts this plan starts from (checked 2026-10-29)

- Nix is gone from every host. The mac has no `nix` binary; g815 and e1504g
  both run openSUSE Tumbleweed under fspec. `just build`, `just lint`, the
  mac VM rig (`dev/vm.sh`, `nix/testvm.nix`) and the g815 slot rig all fail
  at `nix: command not found`. Nothing can be verified until phase 0 lands.
- fspec ships the shell: `~/.config/formalspec/system.star:490` builds a
  pinned rev with `dev/tarball.sh` in debian:bookworm, cached as an OCI
  blob. Shipping a commit is: bump that rev, `fspec update formalshell`,
  `fspec apply --host e1504g` (g815 builds).
- g815 has Hyprland, labwc and cargo on the system, so the nested rig can
  run natively there with no Nix at all.
- Four presets (metamorphosis, retro, pantheon, cartridge) are JSON tables
  read through ~40 habit reads in `formalshell-rs`; nothing matches on a
  preset name outside tests. Cartridge-only code is ~3.8k LOC (boot,
  shards, sfx, catalogue, hex grid) plus retro's dither.
- `bar.position` (four edges) and `bar.paint` (seven pins) already exist.
  There is no floating, inset-corner or sidebar layout.
- Rendering is `vello_cpu` into `wl_shm` per layer surface, ~79k LOC in
  `formalshell-rs/src`, ~27k of it surfaces.

## Decisions

1. **One look, named FormalShell.** Pantheon's table becomes the only table,
   reworked toward GNOME HIG (Adwaita spacing, 12px card radius, heading
   weights, symbolic icons) and kept lightly skeuomorphic (raised buttons,
   a lit rim, a soft cast). The four presets (three tables: retro shares
   metamorphosis's) go, and with them the features only a preset reached:
   cartridge's boot, shards, sfx, hex grid, catalogue motion. Global
   options that work under Pantheon today (`wallpaper.dither`,
   `lock.dither`, `wallpaper.animate`) stay. `theme.preset` is removed,
   not aliased.
2. **Configuration replaces themes.** Four keys carry what presets did:
   - `layout`: `bar` (default) or `sidebar`.
   - `bar.position`: `top` (default), `bottom`, `left`, `right` (exists).
     The sidebar takes `left` or `right`; under `top` or `bottom` it sits
     on the left.
   - `bar.style`: `transparent` (default, today's adaptive wingpanel),
     `solid`, `corners` (solid with rounded screen corners under it, the
     frame's fillets without the ring).
   - `motion.morph`: `true` (default; cards bud off the bar or sidebar and
     morph between sizes, today's `join` emerge) or `false` (Pantheon's
     `popover` rise and fade).
3. **GPUI: no.** A port rewrites ~27k LOC of surfaces and has to rebuild,
   on GPUI's terms, the lock host (ext-session-lock), the per-surface damage
   tracking that holds the idle and CPU-quota budgets, and the motion
   system, for a renderer gain we can test in place. Blur is the
   compositor's job either way (Hyprland layer blur). Phase 6 is not a
   swap either: `vello_hybrid` renders into GPU textures while `surface.rs`
   hands `wl_shm` buffers to the compositor, so the spike has to solve
   presentation (dmabuf or readback) and keep damage tracking, and lands only
   at or under the performance baseline.
4. **fspec is the package manager the shell talks to**, the way Pond's
   shell talks to pacman/Flatpak/Nix: install, update and rollback show up
   as live activity in the shell, not in a terminal. fspec has no
   imperative `install` or catalog search today, so that is designed in
   FormalSpec first (phase 4.0), declaratively.

5. **The polish is the shell's, not the sidebar's.** Activity, the glass,
   the morph and the seams are services and surfaces every layout gets.
   In the bar layout Activity is a bar cell (a ring and a label that
   widens while something runs, the same cell the sidebar card is built
   from) plus a live toast that updates in place instead of stacking.
6. **Live activity speaks the Linux standards that exist** before our own:
   - `com.canonical.Unity.LauncherEntry` on the session bus (progress,
     count, urgent per desktop entry): what Electron's `setProgressBar`,
     Chromium downloads and many GTK/Qt apps already send. It feeds
     Activity and badges on the Spaces icons.
   - The notification spec's `value` hint (0..100, as `notify-send -h
     int:value:40`), drawn as a progress toast, updated in place through
     `replaces_id` or `x-canonical-private-synchronous` (fs-notifd has the
     latter, not `value`).
   - KDE's `org.kde.JobViewServer` for file copies, after the first two.
   - fspec's own operation stream for installs and updates (2.2).
7. **Performance is a gate, not a goal.** Before phase 1, `--r0-measure`
   and `--idle` record a baseline (idle CPU, RSS, wakeups, launcher open,
   GPU use at idle) on g815 and e1504g's power-saver stand-in
   (`FS_CPU_QUOTA`). Every phase lands only at or under it; a feature that
   costs more says so in its commit and is cut or made opt-in. Idle stays
   at zero commits and no GPU work.
8. **The UI kit gets better in place.** What GPUI does better (a
   declarative element tree with flex layout, GPU text and shapes, smooth
   high-refresh animation) is taken one piece at a time into `ui::w` where
   it measures well: a flex layout pass (taffy, already the layout crate
   GPUI uses) replacing hand-placed rects, text polish (subpixel
   positioning, optical sizes, tabular figures), and hit testing and focus
   from the element tree. A GPU renderer costs tens of MB of RSS per
   process for its device and atlases, so it is only taken where its CPU
   saving outweighs that on e1504g.

9. **Blur is the compositor's, and never required.** The shell asks the
   compositor whether it blurs (Hyprland's `decoration:blur:enabled`;
   mango never does) and, where nothing blurs, draws its cards at the
   opaque end of their fill so text never sits on a busy wallpaper. Window
   blur is limited to real windows: no blur on popups, menus and
   tooltips (XWayland override-redirect menus and xdg popups carry
   transparent shadow margins that read as a halo), shipped as rules in
   fspec's `home/.config/hypr/`.
10. **Rendering is configurable.** `render.backend`: `auto` (GPU where a
    usable render node exists, which is most machines with an iGPU),
    `gpu`, `cpu`. The CPU path stays first class for the greeter, the
    lock's fallback and machines without a GPU. Phase 6 builds it, gated
    on the performance baseline for both RSS and CPU.
11. **Under load the shell stays smooth.** It already runs every
    service thread at nice 10 and draws only off frame callbacks. What it
    adds: the session's own slice (compositor, shell) gets a higher
    `CPUWeight` than `app.slice`, so a compile on g815 cannot starve a
    frame (the cgroup answer, the same problem GNOME 51's frame-clock
    rework went after inside Mutter); child tools the shell spawns
    (matugen, ffmpeg, grim encodes) run at idle priority; and the
    `--menu-budget` leg runs with a CPU hog beside it.
12. **Nothing piles up.** Activity groups by source (one row per app or
    operation), shows three rows at most in the sidebar card and one in
    the bar cell with a count, and the rest behind "+N" in a panel. One
    live toast per source, updated in place. Finished items stay four
    seconds, then fold into the notification centre. Every new card and
    cell gets an overflow case in its smoke leg (long titles, ten sources,
    narrow outputs).

## Phases

Each task is one subagent, sequential, ends with its verification command
run and read, and lands as one commit. Every visible change is checked by
reading the PNGs, then shipped to e1504g and photographed there with
`grim` over ssh (reading the live screen, never driving it).

### Phase 0: a rig without Nix (blocker, running)

- 0.1 g815 nested rig on cargo: the build staged into the `result/bin`
  layout `dev/smoke.sh` expects (`formalshell-rs`, `formalshell-ipc`),
  tools from the host, no nix fallbacks. Slot users given the render node
  in fspec (done, `3f1bfb8c` in dotfiles).
- 0.2 `nix/` inputs `dev/tarball.sh` still reads move to `packaging/`
  before `flake.nix`, `nix/`, the mac VM rig and CI's nix steps go.
- 0.3 A VM rig declared and kept by fspec on g815 (new fspec work) for the
  `vm_only` legs and the service checks that were NixOS VM tests. None of
  that coverage is dropped.
- 0.4 CLAUDE.md tells the truth: g815 and e1504g are openSUSE under fspec,
  shipping is a `rev` bump in `~/.config/formalspec/system.star`.

### Phase 1: one look, configured

- 1.1 Spec `docs/superpowers/specs/2026-10-29-one-look.md`: the four keys,
  the HIG token set, what each removed habit becomes, which global options
  stay.
- 1.2 `motion.morph`, `bar.style` and the frame read from configuration
  instead of the table's habits, while every preset still exists, so the
  join and the frame keep working when the presets go.
- 1.3 Extract what shared surfaces borrow from cartridge code
  (`boot::progress` and `boot::EASE` drive every unlock at
  `wayland/lock.rs:664`; `effects::since` is used across surfaces, ~57
  sites) into motion helpers, then delete cartridge, retro and
  metamorphosis with their legs and IPC. Lock, unlock and the greeter
  handoff re-verified on their own.
- 1.4 HIG pass over the token table and every surface, `gallery` leg as the
  sheet. Ship to e1504g after 1.3 and after 1.4.

### Phase 2: glass and fspec activity

- 2.1 Translucent role fills (Pantheon defaults `surfaceOpacity` to 1 and
  toasts to an opaque fill, so compositor blur has nothing to show through)
  tuned together with the Hyprland layerrules fspec ships per namespace,
  checked on g815's GPU.
- 2.2 FormalSpec: an operation stream (operation id, phase, progress,
  result) from `apply` and `update`, on a socket the shell reads, with a
  snapshot on reconnect, so a shell restart mid-apply still shows it.
- 2.3 Shell: an Activity service over that stream, a progress toast
  (Pond's "installing Firefox" ring), honest unavailable without fspec.

### Phase 3: the sidebar

Our take on Pond's left column, kept quiet: about 160 logical px wide
(Pond's is ~12% of a 1280px screen), compact cards, no card wider than its
content needs. A layer surface on the left or right edge
with an exclusive zone, one per output, collapsible to a slim rail. Cards,
top to bottom: time and date; a Spaces minimap (one cell per workspace
with its windows' icons, from compositor metadata, never captures: the
capture rule keeps those to the open preview card); Activity; notifications
summary; now playing when a player exists. A status row at the foot:
network, audio, settings, collapse. Panels and the launcher bud off its
inner edge with the same join the bar uses.

### Phase 4: installing from the shell

- 4.0 FormalSpec design: how an install from the launcher lands in a
  declarative spec (a user apps module fspec edits, commits and applies),
  catalog search over its sources, privilege handling.
- 4.1 Launcher install route on that, progress in Activity, the app in the
  grid when it lands. Updates and rollback rows in the sidebar.

### Phase 5: the seams

Greeter to session with no flash, lock to desktop, first run with no
settings.json, launch feedback (the spinner in Activity until the window
maps), every unavailable state.

### Phase 6: GPU rendering, configurable

`render.backend` (decision 10): `vello_hybrid` behind the existing scene,
presented through dmabuf with damage kept, launcher first, then every
surface. Lands only at or under the baseline on g815 and e1504g.

### Phase 7: later, each on its own

- Sky wallpaper: the sky's gradient for the sun's position now
  (`fs-theme/src/sun.rs` has the times), feeding matugen like a picture.
- Hyprland window motion on the shell's curves.

## Open questions for the owner

- A settings UI: the shell never writes settings.json. Proposal: the UI
  writes overrides into state.json and marks keys settings.json pins.
- Keep a Nix flake for outside users, or ship only install.sh and fspec?
  This plan deletes it.

## Pending verification

- Desktop clock: raised over the subject (`UNDER` 0.4 to 0.2), harder
  frost blur with more picture through, and the subject's cut pushed
  toward 0 or 1 (`harden`, which still leaves exact half coverage at half).
  Explicit `desktopClock.y`, subjectless placement and the serif face are
  untouched. Needs the owner's wallpaper and settings reproduced on the rig
  before it counts as fixed.
