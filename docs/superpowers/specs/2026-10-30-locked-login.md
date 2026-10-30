# Locked login: one compositor from boot to desktop

Owner-approved 2026-10-30. Amends the seams spec (2026-10-30-seams.md)
stages 4 to 7 for the boot path; logout, reboot and shutdown keep theirs.

## Why

The film's last held black (1.3 s, seams spec stage 6) is the gap between
two compositors: sway exits, logind takes the greeter's DRM device, and
nothing can hold a picture until Hyprland and the shell are up. With one
compositor from boot there is no second hand-over to bridge.

## The boot path

1. greetd's `initial_session` runs at its first start since boot, as the
   configured user, with no password: `formalshell-login`, a script the
   installer writes (`/etc/greetd/formalshell-login.sh`), under the PAM
   service `[general] service = "formalshell-login"` (greetd 0.10.3 opens
   `initial_session` and every session a greeter starts under the general
   service). The fspec hosts
   already run `initial_session` for session-switch; their
   `session-switch-login` falls through to the same script when no switch
   target is set, and the tmpfiles line that pre-creates
   `/run/session-switch/greetd.run` at boot goes, so greetd's first start
   is a first start again.
2. The script writes `$XDG_RUNTIME_DIR/formalshell/login-lock` (its content
   the path of the splash's finished frame, `greeter-<s>.png`, picked as the
   greeter session script picks it, or empty) and execs
   `uwsm start -e -D Hyprland -- hyprland.desktop -- --locked-cmd <start>`,
   `<start>` being the include's own `import-environment` plus `systemctl
   --user start formalshell.service`. The unit is the shell's only
   launcher: the include's `hyprland.start` hook runs the same command, and
   a second `start` of a running unit does nothing, so no second shell
   replaces the one holding the lock. The unit loses its
   `After=graphical-session.target` (uwsm reaches that target only once
   the compositor reports ready), keeping `PartOf` and `WantedBy`, so a
   start from the locked command runs at once. The VM rig's config, which
   execs the binary itself today, moves to the same unit.
3. `--locked-cmd` (Hyprland 0.56, `src/main.cpp`, `Compositor.cpp`)
   force-locks the session before any output is added and spawns the
   command. Until a client takes the lock Hyprland draws only its black
   primer (`renderSessionLockPrimer`): no workspace, no layer, and with a
   command set, not the dead-locker page either. That is the compositor's
   own fallback, locked: a shell that never starts leaves a black locked
   session (tty2's getty still answers), never a desktop.
4. The shell starts, finds `login-lock`, and raises its lock at once, the
   path it already takes for its predecessor's held marker. Hyprland lets it
   take the forced lock (`onNewSessionLock`: allowed while a locked command
   is set, and under `misc.allow_session_lock_restore` after). Its first
   frame on each output is the `login-lock` picture, the lock card fading in
   over it on the `effects` clock; with no picture it fades in from black.
   From here on Hyprland's `m_startLocked` is cleared, so a shell dying
   while locked shows the dead-locker page, still locked, and the restarted
   shell takes the lock back off its held marker as today. That needs
   `misc.allow_session_lock_restore = true` (Hyprland's default is false):
   the shipped include sets it, and the VM's own config gains it.
5. The first successful unlock deletes `login-lock` and hands over to the
   desktop as stage 10 does. The arrival veil (stage 7) runs under the lock
   and is not seen.

The only black left is Hyprland's primer between its first modeset and the
shell's first lock commit, under 100 ms on the VM rig. Today's arrival puts
about 400 ms between Hyprland's first frame and the shell's, most of it
uwsm's graphical-session start and the shell building the bar, wallpaper
and services before anything is drawn. The login lock takes neither path:

- `--locked-cmd` is spawned in Hyprland's init, before its outputs are
  added (`Compositor.cpp`), so the shell's start runs beside the modeset
  rather than after uwsm's target.
- With `login-lock` present the shell raises the lock straight after its
  registry roundtrip, before any other surface, service or theme work, and
  the lock surfaces' first commit is the picture alone: decoded on a thread
  started before the Wayland connection (as the fonts already are), drawn
  with no text, so no glyph work sits in front of it. The card and its
  clock come on the next frames as they fade in.
- The seams boot leg films the hand-over at 20 ms a frame (`fspec vm
  record --every-ms 20`) and fails on more than 100 ms of black between the
  splash and the lock. The shell logs its own `phase` stamps from launch to
  the lock's first commit, so a miss names its stage.

## What the greeter's login gave

- **Keyring**: greetd's PAM session (`session optional pam_gnome_keyring.so
  auto_start`) still starts gnome-keyring, locked, since no password was
  typed. `/etc/pam.d/formalshell-lock` gains `auth optional
  pam_gnome_keyring.so`, whose `auth` step unlocks a running daemon's login
  keyring over `$XDG_RUNTIME_DIR/keyring/control` with the typed password.
  Every unlock repeats it harmlessly. kwallet is not covered: pam_kwallet
  hands its key over only in `open_session`, which a lock never runs, and
  no host configures it today.
- **logind session**: greetd's worker sets `XDG_SEAT=seat0`, the class and
  the VT before `pam_open_session`, but `initial_session` passes no
  environment, so pam_systemd would register type `tty` where the greeter's
  sessions say `wayland`. `/etc/pam.d/formalshell-login` is greetd's own
  stack with one line ahead of its session stack, `session required
  pam_env.so readenv=0 conffile=/etc/formalshell/login-env.conf`, which
  sets `XDG_SESSION_TYPE DEFAULT=wayland OVERRIDE=${XDG_SESSION_TYPE}` and
  `XDG_SESSION_DESKTOP DEFAULT=Hyprland` before pam_systemd reads them. A
  greeter's own values still win. Proven by `loginctl show-session` in the
  new leg: class `user`, seat0, VT1, type `wayland`, service
  `formalshell-login`.
- **Account checks**: the lock runs `pam_acct_mgmt` after a successful
  `pam_authenticate` on every unlock, not only the first, so an account
  expired or locked out since boot stays locked ("Account unavailable").
  A password past its age (`PAM_NEW_AUTHTOK_REQD`) also stays locked,
  saying to change it on tty2: the lock has no password-change step.
- **Environment**: the same PAM stack (pam_env's own files, pam_systemd)
  and the same uwsm start as the greeter's `hyprland-uwsm` entry.

## Other users, and the fallback

- Logout from the locked-login session lands in greetd's `default_session`,
  the sway greeter, since `initial_session` runs once per boot: other users
  log in there, as today.
- The boot lock (only while `login-lock` is present, before the first
  unlock, when nothing of the autologin user's has started past the
  shell) carries an "Other user" button: Return arms it, a second Return
  ends this session through logind's `TerminateSession` on its own id, and
  greetd shows the greeter. A lock raised later in the session has no
  such button, since ending the session there would drop the user's work.
- `formalshell install` asks between `autologin` (the user running it) and
  `greeter`; `--login autologin|greeter` answers without asking. `greeter`
  writes no `initial_session` and is today's path exactly. The fspec hosts
  set it in their own greetd config; the VM rig runs autologin, so the seams
  boot leg films it, and `just greeter` keeps driving the greeter (a greetd
  restart never reruns `initial_session`).

## Shutdown and reboot from the lock

The lock card gains Shutdown and Reboot buttons beside the field, Tab
reaching them and Return arming then confirming, as the launcher's rows do.
They call logind's `PowerOff`/`Reboot` with `interactive` false: no polkit
agent can show over the lock. Refused (another user's session open), the
card says so under the field and nothing happens.

The departure fade (seams stage 8) is drawn by an overlay, which Hyprland
does not draw over a session lock. While locked, `PrepareForShutdown` fades
the lock surfaces themselves to black over the same 300 ms, and the
shutdown inhibitor goes on the frame after it lands, as it does unlocked.

## Proof

- `dev/seams.sh`: the boot leg waits for the lock instead of the greeter,
  types the password, and reads no black past 100 ms from the splash to the
  desktop; logout still reaches the greeter; reboot reaches the lock.
- `just greeter`, `--lock-restore`, the lock legs, smoke-affected.
- A new vm_only leg `--login-lock` on a guest booted cold: the lock up and
  secure before any desktop frame (frames read for the bar), the keyring
  locked before the unlock and unlocked after (`busctl` on
  `org.freedesktop.secrets`' login collection `Locked`), logind's class,
  seat, VT, type and service, a shell killed after it took the lock and
  the restarted one taking it back with no desktop frame between, the
  unit masked for one boot leaving the primer black and the session
  locked, and "Other user" reaching the greeter.
- Lock buttons: a `--lock` frame with them, and `--power`'s logind path for
  the refused case.
