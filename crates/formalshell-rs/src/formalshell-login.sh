#!/bin/sh
# Written by `formalshell install`; `formalshell update` rewrites it.
# The same fallback as the shell's paths::runtime, so it finds the lock marker.
case "${XDG_RUNTIME_DIR:-}" in
  /*) runtime=$XDG_RUNTIME_DIR session=yes ;;
  *) runtime=/tmp/formalshell-$(id -u) session= ;;
esac
mkdir -p "$runtime/formalshell"
: > "$runtime/formalshell/login-lock"
# This script runs the command the boot lock leaves, so it takes one only
# from a session runtime dir no other user can reach, never the /tmp fallback.
private() {
  [ -d "$1" ] && [ ! -L "$1" ] && [ -O "$1" ] &&
    case $(stat -c %a "$1") in *00) ;; *) false ;; esac
}
next=
if [ -n "$session" ] && private "$runtime" && [ ! -L "$runtime/formalshell" ]; then
  chmod 700 "$runtime/formalshell"
  next=$runtime/formalshell/login-session
  rm -f "$next" && [ ! -e "$next" ] && [ ! -L "$next" ] || next=
fi
uwsm start -g -1 -e -D Hyprland -- hyprland.desktop -- --locked-cmd '@START@'
status=$?
# The boot lock leaves the session picked on it once the password has been
# checked, then ends Hyprland: its environment a line each, then its command.
[ -n "$next" ] && private "$runtime" && private "$runtime/formalshell" &&
  [ -f "$next" ] && [ ! -L "$next" ] && [ -O "$next" ] || exit "$status"
command=
while IFS= read -r line; do
  case $line in
    XDG_SESSION_TYPE=* | XDG_SESSION_DESKTOP=* | XDG_CURRENT_DESKTOP=*) export "$line" ;;
    *) command=$line ;;
  esac
done < "$next"
rm -f "$next" && [ ! -e "$next" ] && [ -n "$command" ] || exit "$status"
exec sh -c "$command"
