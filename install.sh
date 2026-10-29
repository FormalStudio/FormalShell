#!/bin/sh
# FormalShell for Arch, Debian, Ubuntu and Fedora, without Nix:
#
#   curl -fsSL https://raw.githubusercontent.com/FormalStudio/FormalShell/main/install.sh | sh
#
# `install.sh --help` prints the flags.
set -eu

# Not read back from $0: under `curl ... | sh`, $0 is the shell.
usage() {
  cat <<'USAGE'
FormalShell for Arch, Debian, Ubuntu and Fedora, without Nix.

  install.sh [--system] [--from <tarball>] [--yes] [--no-deps]

Installs Hyprland and the shell's runtime tools with the distro's package
manager, unpacks the latest release tarball under ~/.local (/usr/local with
--system), then runs `formalshell install`.

--from installs a local tarball instead of downloading one, --yes accepts
the sudo prompts `formalshell install` asks, --no-deps skips the package
manager.
USAGE
}

repo=FormalStudio/FormalShell
prefix=$HOME/.local
from=
yes=
deps=1
while [ $# -gt 0 ]; do
  case $1 in
    --system) prefix=/usr/local ;;
    --from) [ $# -ge 2 ] || { echo "--from needs a tarball" >&2; exit 2; }; from=$2; shift ;;
    --yes|-y) yes=--yes ;;
    --no-deps) deps= ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

say() { printf '%s\n' "$*"; }
die() { printf 'install.sh: %s\n' "$*" >&2; exit 1; }

[ -e /etc/NIXOS ] && die "this is NixOS: use the flake's NixOS and home-manager modules instead (https://github.com/$repo#nixos)"
[ "$(uname -s)" = Linux ] || die "FormalShell runs on Linux only"
case $(uname -m) in
  x86_64|aarch64) arch=$(uname -m) ;;
  *) die "no release build for $(uname -m); x86_64 and aarch64 only" ;;
esac

if [ "$(id -u)" = 0 ]; then
  sudo=
elif command -v sudo >/dev/null 2>&1; then
  sudo=sudo
else
  die "sudo is needed to install packages; rerun as root or install sudo"
fi

if command -v pacman >/dev/null 2>&1; then pm=pacman
elif command -v apt-get >/dev/null 2>&1; then pm=apt
elif command -v dnf >/dev/null 2>&1; then pm=dnf
else die "no pacman, apt or dnf here; FormalShell's installer covers the Arch, Debian and Fedora families (NixOS: use the flake)"
fi

# One row per tool: required (r), optional (o) or the compositor (c), then
# the pacman, apt and dnf names, - where that family has none. A missing
# optional one leaves its surface on its honest unavailable state. Debian
# before forky and Fedora carry no Hyprland, which then has to come from
# somewhere else.
table='
c hyprland hyprland hyprland
r pipewire pipewire pipewire
r wireplumber wireplumber wireplumber
r fontconfig fontconfig fontconfig
r libxkbcommon libxkbcommon0 libxkbcommon
r noto-fonts-emoji fonts-noto-color-emoji google-noto-color-emoji-fonts
r dbus dbus dbus
r curl curl curl
r tar tar tar
r gzip gzip gzip
r util-linux util-linux util-linux
o polkit polkitd polkit
o networkmanager network-manager NetworkManager
o upower upower upower
o power-profiles-daemon power-profiles-daemon power-profiles-daemon
o bluez bluez bluez
o bluez-utils - -
o glib2 libglib2.0-bin glib2
o libpulse pulseaudio-utils pulseaudio-utils
o brightnessctl brightnessctl brightnessctl
o ddcutil ddcutil ddcutil
o wlsunset wlsunset wlsunset
o cava cava cava
o mpv mpv mpv
o git git git
o qrencode qrencode qrencode
o wl-clipboard wl-clipboard wl-clipboard
o grim grim grim
o slurp slurp slurp
o wf-recorder wf-recorder wf-recorder
o tesseract tesseract-ocr tesseract
o tesseract-data-eng tesseract-ocr-eng tesseract-langpack-eng
o ffmpeg ffmpeg ffmpeg-free
o xdg-utils xdg-utils xdg-utils
o wtype wtype wtype
o openssh openssh-client openssh-clients
o evolution-data-server evolution-data-server evolution-data-server
o geoclue geoclue-2.0 geoclue2
o uxplay uxplay uxplay
o gtk4 libgtk-4-1 gtk4
o libadwaita libadwaita-1-0 libadwaita
o bluez-libs libbluetooth3 bluez-libs
o sqlite libsqlite3-0 sqlite-libs
o python-gobject python3-gi python3-gobject
'

column() {
  case $pm in pacman) echo "$1" ;; apt) echo "$2" ;; dnf) echo "$3" ;; esac
}

available() {
  case $pm in
    pacman) pacman -Si "$1" >/dev/null 2>&1 ;;
    apt) [ -n "$(apt-cache policy "$1" 2>/dev/null | sed -n 's/^ *Candidate: //p' | grep -v '(none)')" ] ;;
    dnf) dnf -q repoquery --available "$1" 2>/dev/null | grep -q . ;;
  esac
}

if [ -n "$deps" ]; then
  say "Refreshing $pm's package lists"
  case $pm in
    pacman) $sudo pacman -Syu --noconfirm ;;
    apt) $sudo apt-get update ;;
    dnf) $sudo dnf -y makecache ;;
  esac
  want=
  nocompositor=
  skipped=
  missing=
  while read -r need p a d; do
    [ -n "$need" ] || continue
    name=$(column "$p" "$a" "$d")
    [ "$name" = - ] && continue
    if available "$name"; then want="$want $name"
    elif [ "$need" = r ]; then missing="$missing $name"
    elif [ "$need" = c ]; then nocompositor=1
    else skipped="$skipped $name"
    fi
  done <<EOF
$table
EOF
  [ -z "$missing" ] || die "this release has no$missing; FormalShell needs it"
  if [ -n "$nocompositor" ] && ! command -v Hyprland >/dev/null 2>&1; then
    say "$pm's repositories have no Hyprland: install Hyprland 0.56 or newer yourself (Debian has it from forky on), then log in to it"
  fi
  [ -z "$skipped" ] || say "not in $pm's repositories, skipped (their surfaces show as unavailable):$skipped"
  say "Installing:$want"
  # shellcheck disable=SC2086  # one word per package
  case $pm in
    pacman) $sudo pacman -S --needed --noconfirm $want ;;
    apt) $sudo env DEBIAN_FRONTEND=noninteractive apt-get install -y $want ;;
    dnf) $sudo dnf -y install $want ;;
  esac
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
if [ -n "$from" ]; then
  tarball=$from
else
  url=https://github.com/$repo/releases/latest/download/formalshell-$arch-linux.tar.gz
  say "Downloading $url"
  curl -fL --progress-bar -o "$tmp/formalshell.tar.gz" "$url"
  tarball=$tmp/formalshell.tar.gz
fi
[ -f "$tarball" ] || die "no tarball at $tarball"

if [ "$prefix" = /usr/local ]; then as=$sudo; else as=; mkdir -p "$prefix"; fi
$as tar -xzf "$tarball" -C "$prefix" --strip-components=1
say "Unpacked FormalShell $(cat "$prefix/share/formalshell/VERSION") into $prefix"

"$prefix/bin/formalshell" install $yes

case ":$PATH:" in
  *":$prefix/bin:"*) ;;
  *) say "Add $prefix/bin to your PATH to run formalshell and formalshell-ipc by name." ;;
esac
say "Done. Log in to Hyprland; the shell starts with it. \`formalshell update\` upgrades, \`formalshell uninstall\` removes it."
