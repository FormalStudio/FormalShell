# Switching a real machine over

FormalShell is early, and it is the everyday session on both development
machines. Read this before you put it on a machine you depend on: what has run on real hardware, what has only run in a VM, and
what the host has to provide before a given surface can work at all. The
install steps live in [`README.md`](../README.md#install); this document is
the risk.

The verification rig is a nested Hyprland session under software rendering
in an aarch64 VM. It proves that a surface works, not that it works against
your GPU, your Bluetooth adapter or your network. As of 2026-10-07 every leg
of that rig passes (113 of 113, plus the greeter's own run). Daily use on the
two development machines covers what the rig cannot, and since 2026-10-21
the greeter is the login screen on both of them.

## Two ways in

**NixOS.** `nixosModules.formalshell` for the system side,
`nixosModules.formalshell-greeter` for the login screen, and the
home-manager module for the user service, the settings file and
`~/.config/hypr/formalshell.lua`. Every system piece the shell needs is a
`services.formalshell.*` option, set with `mkDefault` so your own config
wins. This is the path both development hosts use.

**Everything else.** `install.sh` detects pacman, apt or dnf, installs
Hyprland and the runtime tools from one name table, unpacks the release
tarball under `~/.local` (or `/usr/local` with `--system`) and runs
`formalshell install`, which writes the systemd user units, the Hyprland
Lua files and, after asking, the `formalshell-lock` PAM file and a greetd
config. `formalshell update` rewrites only what the installer owns and
`formalshell uninstall` reverses it. The tarball carries the shell,
`formalshell-ipc` and the helpers distros do not package (matugen, ttfx,
tensaku, clipssh, localsend-cli, nothingctl, openscq30 and the iPhone
bridge), built against
bookworm's glibc 2.36.

This path is new and the least proven. `dev/install-check.sh` runs
`install.sh` from nothing in rootless containers of Arch, Debian trixie and
Fedora inside the VM, against an aarch64 tarball, and checks a connected
Hyprland backend, the bar, the launcher and a lock round trip through the
installer's PAM file. That is the whole of its testing: no bare-metal
install of any distro, and no x86_64 tarball run anywhere. Debian trixie
and Fedora carry no Hyprland package, so there you bring Hyprland 0.56 or
newer yourself.

## Host prerequisites

Each of these is something the shell cannot do from a user session. Where
one is missing the surface says so rather than failing silently, but it
still does not work.

| Surface | Needs |
| --- | --- |
| Lock screen | the `formalshell-lock` PAM service. `services.formalshell.pam.enable` declares it on NixOS; `formalshell install` writes `/etc/pam.d/formalshell-lock` elsewhere, and without it the lock says "PAM error" |
| Weather | geoclue2 and its agent (`services.formalshell.geoclue.enable` on NixOS, the distro's geoclue package elsewhere) |
| External monitor brightness | `hardware.i2c.enable = true` (or `modprobe i2c-dev`) and your user in the `i2c` group. Without both, `ddcutil detect` finds nothing and the panel shows backlight rows only |
| Polkit prompts | your existing polkit agent dropped. Only one agent can register per session, and this one never fights for the name |
| Menu share and LocalSend | inbound 53317/tcp and 53317/udp. `services.formalshell.localsend.enable` opens them on NixOS; elsewhere your firewall is yours to open |
| AirPlay | avahi and UxPlay's fixed ports (`services.formalshell.airplay.enable` on NixOS) |
| Earbuds panel, AirPods | the `omarchy-pods` fork of the `librepods` daemon running as `librepods --headless`. The stock librepods tray app is write-only and cannot feed this panel |
| DualSense panel | `hid-playstation` bound, and your own udev LED rule if you want lightbar and player LEDs readable |
| Tailscale toggle | `sudo tailscale set --operator=$USER`, once per host. Status polling needs no grant; only up and down do |
| Calendar via online accounts | evolution-data-server and GNOME Online Accounts running (`services.gnome.evolution-data-server.enable` and `services.gnome.gnome-online-accounts.enable` on NixOS) |

On NixOS the package's wrapper carries matugen, grim, slurp, wl-clipboard,
wf-recorder, tesseract, ffmpeg, ttfx, cava, ddcutil, qrencode,
brightnessctl, mpv and wlsunset, so none of those are prerequisites there.
On other distros `install.sh` installs them from the package manager as
optional tools: one your distro does not carry is skipped, and its surface
stays on its honest unavailable state.

## What has run on real hardware

Both development machines run FormalShell as their everyday Hyprland session
on NixOS: e1504g (i3-N305, Intel graphics) since 2026-07-30 and g815
(RTX 5070) since 2026-08-10. Every day that exercises the bar and its cells,
the launcher, the panels, notifications, the screensaver, media and lyrics,
and the lock screen against the real `formalshell-lock` PAM service, plus the
iPhone integration, AirPlay, LocalSend and a calendar signed in through GNOME
Online Accounts. Both machines log in through SDDM, so the greeter is not
part of that.

One nested run on e1504g (i3-N305, 8 GB, 1920x1080, power saver), measuring
the shell against the rewrite's budgets on 2026-10-07. RSS at 600 s was
65 MB against a 120 MB ceiling. Three budgets failed: 1.46% CPU at idle
against 0.0%, 174 ms from launcher toggle to first commit against 50 ms,
and 1249 ms of cold start to the first bar frame against 300 ms. The
spinner and panel frame-time rows were not measured, because the nested
window sat off screen for that run. The full table is in
`docs/superpowers/plans/2026-10-07-r9-cutover.md` under Task 4.

Real devices are where populated-state defects show up: anywhere a real
string or number from hardware gets formatted for display (signal strength,
battery levels, device names). Daily use catches those as they appear, but no
one has swept them systematically.

The greeter is the login screen on both development hosts since
2026-10-21: greetd under sway on the e1504g's Intel GPU and on the g815's
RTX 5070, its backdrop and palette mirrored from the user's own shell.

## What has only run in a VM

**The `install.sh` path**, as described above. **Screen recording is proven
only under software rendering**: the rig's `--record` leg runs a real
wf-recorder child with
`recording.noDmabuf` set, because there is no GPU buffer to import in that
session. The recorder child, the finalize pass and the GIF transcode are all
real there; the dmabuf path a real GPU would take is not.

## Rough edges to know about

**The budgets are not met yet.** The e1504g numbers above are the current
state: a launcher open and a cold start are slower than the targets, and the
idle shell is not yet at zero CPU.

**A window the compositor reports no box for cannot be captured.** Hyprland
reports one for every window it does not hide, so this is normally nothing;
an unfocused member of a tabbed group is the case that hits it. Those
windows are listed in a card saying so rather than disappearing from a mode
that lists their neighbours.

## What "ready" would mean

1. The e1504g budgets met, with the frame-time rows measured.
2. `install.sh` run on bare metal on each distro family it supports, on
   x86_64 as well as aarch64.
3. Screen recording proven on a real GPU.
