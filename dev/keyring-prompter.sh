#!/usr/bin/env bash
# dev/keyring-prompter.sh <prompter_probe>: fs_auth::prompter against a real
# gnome-keyring-daemon, on a private session bus and runtime dir of its own.
# A login keyring made with a password, the daemon restarted the way an
# autologin's PAM session starts it (locked), and the Secret portal asking
# for an app's secret, which has the daemon prompt on its main thread: the
# control socket answers nothing while that prompt waits, the probe answers
# it, and the portal returns with the login keyring unlocked.
set -euo pipefail

fail() { echo "keyring-prompter: $*" >&2; exit 1; }

if [ "${1:-}" != --inner ]; then
  probe=$(realpath "${1:?usage: $0 <prompter_probe>}")
  for tool in gnome-keyring-daemon dbus-run-session busctl python3; do
    command -v "$tool" >/dev/null 2>&1 || fail "$tool is not on PATH"
  done
  python3 -c 'import gi; gi.require_version("Gio", "2.0")' 2>/dev/null || fail "no python gobject; install the openSUSE package python313-gobject"
  dir=$(mktemp -d)
  trap 'pkill -u "$(id -u)" -f "^$dir/" 2>/dev/null || true; for p in $(pgrep -u "$(id -u)" -x gnome-keyring-d); do tr "\0" "\n" < /proc/$p/environ 2>/dev/null | grep -qx "HOME=$dir" && { kill "$p" || true; }; done; rm -rf "$dir"' EXIT
  export HOME=$dir XDG_DATA_HOME=$dir/data XDG_CONFIG_HOME=$dir/config XDG_RUNTIME_DIR=$dir/run
  unset GNOME_KEYRING_CONTROL SSH_AUTH_SOCK
  mkdir -p "$XDG_DATA_HOME" "$XDG_CONFIG_HOME"
  mkdir -m 700 "$XDG_RUNTIME_DIR"
  dbus-run-session -- "$0" --inner "$probe" "$dir"
  exit $?
fi

probe=$2 dir=$3 pw=formalshell-test
locked() { busctl --user get-property org.freedesktop.secrets /org/freedesktop/secrets/collection/login org.freedesktop.Secret.Collection Locked; }
daemons() { for p in $(pgrep -u "$(id -u)" -x gnome-keyring-d); do tr "\0" "\n" < /proc/$p/environ | grep -qx "HOME=$dir" && echo "$p"; done; true; }

printf '%s' "$pw" | gnome-keyring-daemon --daemonize --login --components=secrets >/dev/null 2>&1
# The keyring is written once the secrets component is first asked for it.
[ "$(locked)" = "b false" ] || fail "the new login keyring is locked"
[ -f "$XDG_DATA_HOME/keyrings/login.keyring" ] || fail "no login keyring made"
kill $(daemons); sleep 1
[ -z "$(daemons)" ] || fail "the first daemon is still up"

mkfifo "$dir/probe.in"
"$probe" <"$dir/probe.in" >"$dir/probe.log" 2>&1 &
probe_pid=$!
exec {to_probe}>"$dir/probe.in"
for _ in $(seq 50); do busctl --user status org.gnome.keyring.SystemPrompter >/dev/null 2>&1 && break; sleep 0.1; done
owner=$(busctl --user status org.gnome.keyring.SystemPrompter 2>/dev/null | sed -n 's/^PID=//p')
[ "$owner" = "$probe_pid" ] || fail "the probe does not hold the prompter (owner ${owner:-none})"
echo "the probe holds org.gnome.keyring.SystemPrompter"

# The answer is the login password, sealed for whoever asked: a client
# holding org.gnome.keyring before the daemon does, from a binary named
# gnome-keyring-daemon outside /usr/bin, is refused before an exchange
# starts. It never answers the refusal, which must not hold up the real
# prompt after it.
cp "$(readlink -f "$(command -v python3)")" "$dir/gnome-keyring-daemon"
"$dir/gnome-keyring-daemon" - <<'PY' || fail "a fake gnome-keyring-daemon was offered a prompt"
import sys
import gi
gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib
xml = """<node><interface name="org.gnome.keyring.internal.Prompter.Callback">
<method name="PromptReady"><arg type="s" direction="in"/><arg type="a{sv}" direction="in"/><arg type="s" direction="in"/></method>
<method name="PromptDone"/></interface></node>"""
bus = Gio.bus_get_sync(Gio.BusType.SESSION)
owned = bus.call_sync("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "RequestName",
    GLib.Variant("(su)", ("org.gnome.keyring", 4)), None, 0, 5000, None).unpack()[0]
if owned != 1:
    sys.exit("could not take org.gnome.keyring")
loop = GLib.MainLoop()
got = []
def call(conn, sender, path, iface, method, params, inv):
    got.append(method)
    loop.quit()
bus.register_object("/org/example/Rogue", Gio.DBusNodeInfo.new_for_xml(xml).interfaces[0], call)
bus.call_sync("org.gnome.keyring.SystemPrompter", "/org/gnome/keyring/Prompter", "org.gnome.keyring.internal.Prompter",
    "BeginPrompting", GLib.Variant("(o)", ("/org/example/Rogue",)), None, 0, 5000, None)
GLib.timeout_add(3000, loop.quit)
loop.run()
print("fake daemon got", got or "nothing")
sys.exit(0 if got == ["PromptDone"] else 1)
PY
grep -q 'refused a prompt from' "$dir/probe.log" || fail "the probe did not log the refusal"
echo "a fake gnome-keyring-daemon is refused"

gnome-keyring-daemon --daemonize --login < /dev/null >/dev/null 2>&1
gnome-keyring-daemon --start --foreground --components=secrets >"$dir/start.log" 2>&1 &
for _ in $(seq 50); do locked >/dev/null 2>&1 && break; sleep 0.1; done
[ "$(locked)" = "b true" ] || fail "the login keyring is not locked: $(locked)"
echo "login keyring locked"

# xdg-desktop-portal reaches it as org.gnome.keyring (gnome-keyring.portal).
python3 - "$dir/portal.out" <<'PY' &
import os, sys
import gi
gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib
bus = Gio.bus_get_sync(Gio.BusType.SESSION)
r, w = os.pipe()
fds = Gio.UnixFDList.new()
idx = fds.append(w)
os.close(w)
args = GLib.Variant("(osha{sv})", ("/org/freedesktop/portal/desktop/request/1_1/probe", "org.example.Probe", idx, {}))
out = open(sys.argv[1], "w")
try:
    res, _ = bus.call_with_unix_fd_list_sync("org.gnome.keyring", "/org/freedesktop/portal/desktop",
        "org.freedesktop.impl.portal.Secret", "RetrieveSecret", args, GLib.VariantType("(ua{sv})"), 0, 60000, fds, None)
    secret = os.read(r, 4096)
    print(f"response {res.unpack()[0]} secret {len(secret)} bytes", file=out)
except GLib.Error as e:
    print(f"error {e.message}", file=out)
PY
portal=$!

for _ in $(seq 100); do grep -q '^keyring prompter: a prompt from' "$dir/probe.log" && break; sleep 0.1; done
grep -q '^keyring prompter: a prompt from' "$dir/probe.log" || fail "no prompt reached the probe: $(cat "$dir/probe.log" "$dir/portal.out" "$dir/start.log" 2>&1)"
echo "the portal's unlock prompt reached the probe"

python3 - "$XDG_RUNTIME_DIR/keyring/control" <<'PY' && fail "the control socket answered while the portal's prompt waits"
import socket, struct, sys
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1]); s.settimeout(2)
pw = b"wrong"
s.sendall(b"\0" + struct.pack(">III", 12 + len(pw), 1, len(pw)) + pw)
try:
    s.recv(8)
except TimeoutError:
    sys.exit(1)
PY
echo "the control socket answers nothing while the prompt waits"

echo "$pw" >&"$to_probe"
for _ in $(seq 100); do [ -s "$dir/portal.out" ] && break; sleep 0.1; done
wait "$portal" || true
cat "$dir/probe.log"
grep -q '^response 0 ' "$dir/portal.out" || fail "the portal did not return a secret: $(cat "$dir/portal.out" 2>/dev/null)"
cat "$dir/portal.out"
[ "$(locked)" = "b false" ] || fail "the login keyring is still locked"
echo "login keyring unlocked by the probe's answer"
for p in $(pgrep -u "$(id -u)" -x gcr-prompter); do
  tr "\0" "\n" < /proc/$p/environ | grep -qx "HOME=$dir" && fail "gcr-prompter was started"
done
echo "no gcr-prompter"
kill "$probe_pid" 2>/dev/null || true
echo "keyring-prompter: ok"
