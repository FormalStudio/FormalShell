# Shoji handoff (2026-11-13, for Tuesday 2026-11-17 13:00)

formalwm is a Rust window manager in `wm/` on the ShojiWM fork
(FormalSnake/ShojiWM, branch `formal`), with FormalShell's `shoji` compositor
backend. The dotfiles spec (FormalSnake/dotfiles `main`, checkout
`~/.config/formalspec`) packages it and makes `formalwm-uwsm` the default
session on g815 and e1504g; Hyprland stays on the session chip.

## Live and merged

- FormalShell `main` was `9260c60e` when g815 was pinned; atoll's
  `mac-electron` merge (1a625e77) followed. Every branch below is merged.
- ShojiWM `formal` is `7783d5e`.
- dotfiles `main` is `5b304225`. That commit pins the shell and formalwm at
  `9260c60e` and `xdg-desktop-portal-shojiwm` at `7783d5e`.
- g815: applied from `5b304225` at about 01:15 WET on 2026-11-13 (see "Apply
  result" at the end). Before that it ran `95f01af3` / core `88ac18e`.
  formalwm takes over at the next login, not in the running session.
- e1504g: offline since 2026-11-11 17:32. It has none of this yet; a waiter
  applies it when it comes online (first actions, step 4).

## Branches

FormalShell, all merged into `main`, no PRs (this repo pushes fix branches
and fast-forwards main):

- `fix/boot-lock-sessions` (aacfad4a): the session chip on the boot lock, and
  the handover to the chosen session.
- `fix/login-legs-g815` (bb97bb4f): the legs' own keyring socket via
  `GNOME_KEYRING_CONTROL`.
- `fix/shoji-legs`, `fix/wm-consumed-keys`, `fix/wm-core-keys{,-2,-3}`: leg
  fixes and core pin bumps.
- `fix/wm-lock-pointer`, `fix/wm-lock-popup`, `fix/wm-lock-text-input`,
  `fix/wm-lock-im{,-2,-3}`: core pin bumps for lock isolation.

Merged after the g815 apply, so not on g815 yet:

- `fix/wm-idle-gpu-start`, rebased onto main as 2c5023e7 and 4ea54642 and
  merged at 01:25 WET. Two fixes:
  1. Under `render.backend: auto`, the shell stays on the CPU when the
     compositor's GPU uses NVIDIA's driver. Any NVIDIA Vulkan device starts
     a driver thread that wakes 100 times a second. That thread takes the
     name of the shell thread that opened the device, `gpu-start`. Only
     formalwm offers the dmabuf format that made `auto` pick the GPU, which
     is why the wakeups showed only there. `render.backend: gpu` still
     opens the device. g815 legs, as slot user fstest4, wakeups per idle
     minute: `--idle` 6,884 to 841, `--idle --bluez-rssi` 7,132 to 1,089.
     Both legs still exit 1 because they look for a Hyprland process.
  2. `dev/smoke.sh` records which user manager it saved the environment
     from, and skips the restore with a log line when another manager, or
     none, answers. Its shell test passes 4/4; main's version fails 2 of
     them.
  Report: `~/.t3/scratch/2026-11-11-can-we-create-the-hyprland-796785aa/idle-gpu-report.md`.
  Evidence: `~/build/idle-gpu-runs` of user fstest4 on g815 (16M). Codex
  approved it with no S1 or S2 findings
  (`~/build/formalshell-review-runs/review-idle-gpu-d323698c.md`). It left
  two timing nits in `dev/smoke.sh:621` and `:641` (the environment
  snapshot and the restore target are not pinned to the manager identity);
  neither was introduced by this branch. The pin bump that ships it to g815
  waits on the wm input and mac legs (first actions, step 2). Check it
  against the `t3/configurable-gpu-rendering` worktree, which touches the
  same choice.
- `mac-electron` (see below) is also merged and not on g815 yet.

Not merged:

- `mac-electron`: atoll's work. Atoll merged it into main after this
  handoff (merge 1a625e77 on 425d41cf, no conflicts). It passed all 34
  wm/input leg runs plus `--mac-electron` on 526f77af. The next pin bump
  ships its Super+Q to g815: run the wm input and mac legs once on 1a625e77
  or later before bumping.
- `t3/configurable-gpu-rendering`: another session's worktree. Check with
  that session before touching `fix/wm-idle-gpu-start`'s GPU choice, since
  both decide where the shell renders.

ShojiWM: `formal` carries every core fix. The fork also has 27 remote branches
that are not ancestors of `formal` (feature branches like `alpha-modifier`,
`auto-hdr`). They were not audited tonight. Check whether `formal` already
carries each one as its own commit before deleting any.

dotfiles: `main` is live. The `shoji` branch (3657572e, pins at 1898f355) is
stale; main has moved past it. Delete it once you have checked there is
nothing on it that main lacks.

## Open review items

- Codex approved core `8218c2b..7783d5e`, with no S1, S2 or nits
  (`~/build/formalshell-review-runs/review-shojiwm-7783d5e.md`). It
  reviewed the Linux red/green tests but did not rerun them.
- Known and accepted: under the lock, another client that creates or
  destroys an input method can make the locker's input method unavailable
  (Smithay tells the locker's input method it is gone). No input leaks to
  the other client. A fix needs Smithay to support more than one input
  method per seat.
- Leg harness, from atoll's Electron work: in the g815 slots, a leg that
  runs Notion or another Chromium app starts gnome-keyring over the session
  bus. With no unlocked login keyring, its "Authentication required" prompt
  takes the keyboard, and the next keys miss the app. Fix it in the leg walk
  the way 4861d423 did for the login legs: per run,
  `printf '' | gnome-keyring-daemon --replace --unlock --components=secrets -d`,
  and launch the app with `--password-store=basic`.
- Earlier reviews' nits that were not acted on are in
  `~/build/formalshell-review-runs/review-*.md`.

## Decisions made

- A core round ships to g815 ahead of its re-review only when it is strictly
  tighter than what is live. `526f77af` (core 8218c2b) was held back:
  Codex found two S1s it introduced (a stale grab release cancelled the
  locker's grab; text-input setters grew memory without bound).
  `9260c60e` (core 7783d5e) fixes both and went out.
- No VMs (owner's instruction). Tests run on the mac rig (lima `fstestmac`,
  formalwm on slot 3, Hyprland on slot 0, `dev/rig_lock.py`) or headless on
  g815 bare metal. On g815 the run uses a private headless labwc on
  renderD129 inside `systemd-run --user --scope -p MemoryMax=6G -p
  CPUWeight=20 nice -n 10`, with a private runtime dir and buses, plus
  shims that refuse `systemctl` and the like. Not sway: nested Hyprland
  cannot start under sway 1.12.
- fstestvm3's disk on g815 was deleted to free space. Atoll's
  `--panel-morph @1.25` leg needs that VM's saved state. Rebuilding it
  (`fspec vm start` then `fspec vm save` for fstestvm3) is the owner's call.
- wvkbd may show under the lock. Every other client's grabs, popups, drags,
  cursors, input methods and text input are refused under the lock.
- The overview closes on Escape only.
- The spec enables the `gnome-keyring-daemon.socket` user unit, so an
  autologin unlock can unlock the keyring.
- The boot lock hands over through a marker in
  `$XDG_RUNTIME_DIR/formalshell/login-session`, with checks on the private
  dir. `formalshell-login.sh` starts the marked session after the
  compositor exits.

## Hardware results (g815, headless, r6)

Report: `~/.t3/scratch/2026-11-11-can-we-create-the-hyprland-796785aa/shoji-shots/r6/REPORT.md`.
These runs were on a loaded machine with labwc's timer instead of vblank,
so the numbers read relative, not absolute.

- `--overview` under Hyprland: proven, 6/6.
- `--menu-budget` under formalwm: failed 2/6. The shell held bar frames for
  66, 44 and 17 ms against a 16 ms budget, mostly on the first launcher
  open. Not investigated yet.
- `--idle`, `--idle --bluez-rssi` under formalwm: failed on the `gpu-start`
  wakeups, which `fix/wm-idle-gpu-start` addresses.
- `--hypr-motion`: inconclusive, timing-bound on this hardware under both
  compositors.

## Still unproven on hardware

- Unlocking with the owner's real password, and the keyring unlocking with it.
- A real VT switch, and suspend and resume, under formalwm.
- Gestures under the lock. Real fcitx5 under the lock.
- `--login-switch @1.25` is flaky on g815: it failed 1 of 8 because Tab or
  Return did not reach the card.
- 18 leg rows never ran: 3 need hardware, 15 needed the VM rig.

## Files left in place

None of these are needed; delete by literal path when done with them.

- Mac: `~/Developer/ecosystem/FormalShell-wt/shoji-sweep-runs` (1.4G),
  `shoji-legs-runs`, `lock-keyring-g815-runs` and `login-legs-g815-runs` in
  the same dir. Also `~/.config/formalspec-wt/shoji-runs`,
  `~/build/formalshell-review-runs/`, and the scratch reports and shots in
  `~/.t3/scratch/2026-11-11-can-we-create-the-hyprland-796785aa/`.
- `~/Developer/ecosystem/FormalShell-wt/idle-gpu`: its agent removes it
  when the branch is pushed. If it is still there, check
  `git -C` it for unpushed work first.
- g815: `~/build/fwm-proof-runs/`, `~/build/fwm-hw-runs/` (199M), and
  fstest4's `~/build/idle-gpu-runs` (16M).
- fstestmac guest (slot 3): `~/fwm-ship`.

## First actions for Tuesday

1. Log in on g815. formalwm should start by default. Check:
   - the session chip on the boot lock;
   - unlock with your password, and that the keyring is unlocked
     (`secret-tool search` returns without a prompt);
   - a VT switch (Ctrl+Alt+F3 and back);
   - suspend and resume;
   - fcitx5 in a text field.
2. Fix the keyring prompt in the leg walk (open review items), then run the
   wm input and mac legs on current main (1a625e77 or later).
3. Bump the spec to current main (idle fix and mac-electron). The recipe:
   - fresh clone of dotfiles into `/tmp/formalspec-apply`;
   - edit the two revs in `modules/linux/store.star`;
   - `fspec update formalshell formalwm xdg-desktop-portal-shojiwm`;
   - commit and push to main;
   - apply over the agent tunnel:
     `ssh -f -N -L /tmp/g815agent.sock:/run/user/1000/gcr/ssh g815`, then
     `SSH_AUTH_SOCK=/tmp/g815agent.sock fspec apply --host g815`.
4. e1504g: a waiter on the macbook (`meshtop hosts wait e1504g ... && fspec
   apply --host e1504g --rev HEAD`, 14-day timeout, `pgrep -fl 'hosts wait
   e1504g'`) applies dotfiles main as soon as e1504g is online and prints the
   running shell's binary. If it is gone, run `meshtop hosts status`, then
   the apply yourself (e1504g never compiles; packages come from the store).
5. Investigate `--menu-budget`'s held bar frames under formalwm.
6. Decide: rebuild fstestvm3 for atoll's `--panel-morph @1.25`, or not.
7. Design question still open: should SUPER+H/L move focus onto floating
   windows?

## Apply result

The g815 apply from dotfiles `5b304225` finished at about 01:15 WET on
2026-11-13, and a re-plan shows no changes. The store has
`a3345354-formalshell-9260c60ecb66` and `8c0ef0e5-formalwm-9260c60ecb66`.
`formalshell.service` restarted onto the new shell in the running Hyprland
session (pid 3411), and the service is active. formalwm 9260c60e starts at
the next login.
