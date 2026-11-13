# Shoji handoff (2026-11-13, for Tuesday 2026-11-17 13:00)

formalwm is a Rust window manager in `wm/` on the ShojiWM fork
(FormalSnake/ShojiWM, branch `formal`), with FormalShell's `shoji` compositor
backend. The dotfiles spec (FormalSnake/dotfiles `main`, checkout
`~/.config/formalspec`) packages it and makes `formalwm-uwsm` the default
session on g815 and e1504g; Hyprland stays on the session chip.

## Live and merged

- FormalShell `main` is `9260c60e`. Every branch below is merged into it.
- ShojiWM `formal` is `7783d5e`.
- dotfiles `main` is `5b304225`. That commit pins the shell and formalwm at
  `9260c60e` and `xdg-desktop-portal-shojiwm` at `7783d5e`.
- g815: applied from `5b304225` at 01:0x WET on 2026-11-13 (see "Apply
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

Not merged:

- `fix/wm-idle-gpu-start` (pushed at d323698c, also has 698b581c). Two
  fixes:
  1. Under `auto`, the shell leaves NVIDIA's driver to the CPU path.
     NVIDIA's open Vulkan device woke the shell 100 times a second (driver
     threads named `gpu-start`). The g815 idle leg went from 6749 to 871
     wakeups.
  2. `dev/smoke.sh` no longer restores the saved environment into a
     different user manager than the one it read from. On g815 that would
     have unset WAYLAND_DISPLAY and PATH on the owner's session.
  Report: `~/.t3/scratch/2026-11-11-can-we-create-the-hyprland-796785aa/idle-gpu-report.md`.
  Its status line says whether the legs finished. Needs a review, then a
  merge and a pin bump.
- `mac-electron`: atoll's work, not part of this.
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

- The Codex re-review of core `8218c2b..7783d5e` was running when this was
  written. Output:
  `~/build/formalshell-review-runs/review-shojiwm-7783d5e.md`. Its S1s are
  the first thing to fix.
- Known and accepted: under the lock, another client that creates or
  destroys an input method can make the locker's input method unavailable
  (Smithay tells the locker's input method it is gone). No input leaks to
  the other client. A fix needs Smithay to support more than one input
  method per seat.
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
- g815: `~/build/fwm-proof-runs/`, `~/build/fwm-hw-runs/` (199M).
- fstestmac guest (slot 3): `~/fwm-ship`.

## First actions for Tuesday

1. Log in on g815. formalwm should start by default. Check:
   - the session chip on the boot lock;
   - unlock with your password, and that the keyring is unlocked
     (`secret-tool search` returns without a prompt);
   - a VT switch (Ctrl+Alt+F3 and back);
   - suspend and resume;
   - fcitx5 in a text field.
2. Read `review-shojiwm-7783d5e.md` and fix any S1 in the core before
   anything else ships.
3. Review `fix/wm-idle-gpu-start`, merge it, and bump the spec. The recipe:
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

Filled in after the g815 apply finished.
