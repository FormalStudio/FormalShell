#!/usr/bin/env bash
# One service crate's tests against its real daemon, on a Linux host
# (dev/remote.sh check <name> runs it on g815).
#
#   dev/check.sh fs-tray|fs-notifd|fs-mpris
#
# Those three start their own dbus-daemon (and fs-notifd the real
# notify-send) on a private session bus; dbus-run-session keeps anything
# they miss off the host's. fs-auth, fs-audio, fs-bluez, fs-network and
# fs-upower drive real system daemons (PAM, PipeWire, BlueZ,
# NetworkManager, UPower) and run in a throwaway VM, which this host does
# not have yet.
set -euo pipefail
cd "$(dirname "$0")/../crates"

[ $# -eq 1 ] || { echo "usage: $0 <crate>" >&2; exit 1; }
case "$1" in
  fs-tray|fs-notifd|fs-mpris)
    for tool in dbus-daemon dbus-run-session; do
      command -v "$tool" >/dev/null 2>&1 || { echo "$1: $tool is not on PATH; install the openSUSE package dbus-1-daemon" >&2; exit 1; }
    done
    if [ "$1" = fs-notifd ] && ! command -v notify-send >/dev/null 2>&1; then
      echo "fs-notifd: notify-send is not on PATH; install the openSUSE package libnotify-tools" >&2
      exit 1
    fi
    exec dbus-run-session -- fspec dev -- cargo test --locked --release --package "$1"
    ;;
  fs-auth|fs-audio|fs-bluez|fs-network|fs-upower)
    echo "$1: needs the VM rig (a throwaway VM running the real daemon), which g815 does not have yet" >&2
    exit 1
    ;;
  *)
    echo "$1: no such check (fs-tray, fs-notifd, fs-mpris; fs-auth, fs-audio, fs-bluez, fs-network, fs-upower need the VM rig)" >&2
    exit 1
    ;;
esac
