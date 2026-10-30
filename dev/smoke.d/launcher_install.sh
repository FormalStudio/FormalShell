# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, iso_home, the *_bin paths and fail()
# --launcher-install: the launcher's Install mode (launcher spec §8)
# against a PATH-shimmed `fspec` speaking FormalSpec's shell-install.md
# interface, never a real install. The shim answers `search --json` off a
# fixture (Foxglove installable from flatpak with a catalog icon, a
# foxtrot-cli the distro already has, a Foxhole fspec cannot write here and
# a Foxfire whose install fails), and `app install --detach` starts
# dev/fspec-activity-stub.py's `install` (or `installfail`) on the session's
# real operation stream directory and prints its `{"op"}`, as fspec does.
#
# Read off `menu status` (`ids`, `empty`, `drawn` values), `packages
# status` and `activity status`: every dim state (Searching…, a failed
# search's reason, No packages for "x", and fspec gone from PATH reading
# `Installing apps needs fspec`); Enter on Foxglove reaching the shim as
# exactly `app install flatpak:org.example.Foxglove --detach` with the
# launcher left open on the row, its value following the stream (Resolving,
# Downloading 40%, Installing 50%); the live toast once the launcher closes;
# the shim's desktop entry landing before the success, the app found as the
# install's (`packages status`), one pick of it in state.json, Activity's
# Open running the entry's Exec, and a search for it now heading with the
# app subject above the package, which reads Installed and is ticked.
# Foxfire's failure puts its reason on the row and in Activity, and
# `activity retry` reaches the shim as the same install. Then Uninstall on
# a staged Foxberry, the shim's `app owner` saying a second late that this
# host's spec holds it: a real Tab before that answer, Uninstall joining
# the open column last, End and Return only arming it, the second Return
# reaching the shim as exactly `app uninstall flatpak:org.example.Foxberry
# --detach`, Activity reading it succeeded and the app gone from the rows
# once its entry is, and a staged Foxtail the distro owns offering no
# Uninstall. The rows with the description pane, the ring mid-download, the
# toast, a dim state and the armed Uninstall are photographed.
leg_launcher_install_flag="--launcher-install"
leg_launcher_install_order=34
leg_launcher_install_needs="jq python3 wtype convert"

li_dir="$shot_dir/launcher-install"
li_shim_dir="$li_dir/shim"
li_apps="$iso_home/.local/share/applications"
li_icon="$li_dir/foxglove.png"

leg_launcher_install_fixture() {
  mkdir -p "$li_shim_dir" "$li_apps"
  "$convert_bin" -size 128x128 xc:'#ff7a00' "$li_icon"
  local stub="$script_dir/fspec-activity-stub.py"
  cat > "$li_dir/search.jsonl" <<EOF
{"id":"flatpak:org.example.Foxglove","name":"Foxglove","source":"flatpak","version":"1.4.2","icon":"$li_icon","description":"A field guide to the hedgerow, with a description long enough to wrap inside the preview pane twice over.","installed":false,"owner":null,"installable":true,"run":true}
{"id":"zypper:foxtrot-cli","name":"foxtrot-cli","source":"zypper","version":null,"icon":null,"description":"Dance steps in a terminal","installed":true,"owner":"distro","installable":true,"run":false}
{"id":"flatpak:org.example.Foxfire","name":"Foxfire","source":"flatpak","version":"0.9","icon":null,"description":null,"installed":false,"owner":null,"installable":true,"run":true}
{"id":"cask:foxhole","name":"Foxhole","source":"cask","version":"2.0","icon":null,"description":"Not for this machine","installed":false,"owner":null,"installable":false,"run":false}
EOF
  # One operation per call, numbered, so a retry is a stream of its own.
  cat > "$li_shim_dir/fspec" <<EOF
#!/usr/bin/env bash
echo "\$*" >> "$li_dir/shim.log"
[ -e "$li_dir/gone" ] && exit 127
start() {
  local verb=\$1 id=\$2 title=\$3 n op
  n=\$(( \$(wc -l < "$li_dir/shim.log") ))
  op="smoke-\$verb-\$n"
  local scenario=\$verb
  case "\$id" in *Foxfire*) scenario=installfail ;; esac
  FSPEC_STUB_PKG="\$id" FSPEC_STUB_TITLE="\$title" setsid "$python3_bin" "$stub" "\$XDG_RUNTIME_DIR/fspec/activity" "\$op" "\$scenario" "$li_dir/\$op" < /dev/null > /dev/null 2>> "$li_dir/stub.log" &
  for _ in \$(seq 1 50); do
    [ -S "\$XDG_RUNTIME_DIR/fspec/activity/\$op.sock" ] && break
    sleep 0.1
  done
  echo "\$op" > "$li_dir/last-op"
  printf '{"op":"%s"}\n' "\$op"
}
case "\$1 \$2" in
  "search --json")
    q="\${*: -1}"
    sleep 1.5
    case "\$q" in
      fox*) cat "$li_dir/search.jsonl" ;;
      broken*) echo "fspec: error: flathub catalog is unreadable" >&2; exit 1 ;;
    esac
    exit 0 ;;
  "app install")
    title=\$("$jq_bin" -r --arg id "\$3" 'select(.id == \$id) | .name' "$li_dir/search.jsonl")
    start install "\$3" "\$title" ;;
  "app uninstall")
    start uninstall "\$3" Foxberry ;;
  "app owner")
    case "\$3" in
      org.example.Foxberry) sleep 1; printf '{"path":null,"id":"flatpak:org.example.Foxberry","pkg":"org.example.Foxberry","source":"flatpak","owner":"spec","removable":true}\n' ;;
      org.example.Foxtail) printf '{"path":null,"id":"zypper:foxtail","pkg":"foxtail","source":"zypper","owner":"distro","removable":false}\n' ;;
      *) printf '{"path":null,"id":null,"pkg":null,"source":null,"owner":null,"removable":false}\n' ;;
    esac ;;
  *) echo "fspec shim: \$* is not shimmed" >&2; exit 2 ;;
esac
EOF
  chmod +x "$li_shim_dir/fspec"
  # PATH rides into the session (see clipssh.sh), ahead of any real fspec.
  export PATH="$li_shim_dir:$PATH"
}

leg_launcher_install_timing() {
  leg_timing 95 140
}

leg_launcher_install_drive() {
  local script="$shot_dir/launcher-install-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
call() { $ipc call "\$@"; }
status() { call menu status > "$li_dir/\$1.json" 2>&1; }
pkgs() { call packages status > "$li_dir/\$1.json" 2>&1; }
acts() { call activity status > "$li_dir/\$1.json" 2>&1; }
shoot() { "$grim_bin" "$li_dir/\$1.png" > /dev/null 2>&1; }
# Polls menu status into step \$1 until jq claim \$2 holds, at most \$3 s.
until_status() {
  SECONDS=0
  while [ "\$SECONDS" -lt "\$3" ]; do
    status "\$1"
    "$jq_bin" -e "\$2" "$li_dir/\$1.json" > /dev/null 2>&1 && return 0
    sleep 0.2
  done
}
# Waits for the stub's mark \$1 (op.step), then lets it on.
step() {
  for _ in \$(seq 1 100); do [ -e "$li_dir/\$1" ] && return 0; sleep 0.1; done
}
go() { touch "$li_dir/\$1"; }
sleep 4
call menu summon install > /dev/null 2>&1
sleep 1
status idle
call menu filter fox > /dev/null 2>&1
until_status searching '.empty == "install.searching"' 3
until_status rows '(.ids | length) >= 4' 10
sleep 1
shoot rows
call menu filter broken > /dev/null 2>&1
until_status failed '.empty == "install.failed"' 10
shoot failed
call menu filter zz > /dev/null 2>&1
until_status none '.empty == "install.none"' 10
touch "$li_dir/gone"
call menu filter gone > /dev/null 2>&1
until_status gone '.empty == "install.nofspec"' 10
rm -f "$li_dir/gone"

call menu filter fox > /dev/null 2>&1
until_status ready '.ids[0] == "install.pkg.flatpak%3Aorg%2Eexample%2EFoxglove"' 10
"$wtype_bin" -k Return
step "\$(sleep 1; cat "$li_dir/last-op").1"
op=\$(cat "$li_dir/last-op")
until_status resolving '.drawn[0].value == "Resolving"' 5
go "\$op.go1"; step "\$op.2"
until_status downloading '.drawn[0].value == "Downloading 40%"' 5
shoot ring
go "\$op.go2"; step "\$op.3"
until_status committing '.drawn[0].value == "Installing 50%"' 5
call menu close > /dev/null 2>&1
sleep 0.8
shoot toast
acts running
cat > "$li_apps/org.example.Foxglove.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Foxglove
Exec=touch $li_dir/foxglove-opened
Icon=folder
DESKTOP
sleep 1.5
go "\$op.go3"
for _ in \$(seq 1 30); do
  pkgs landed
  "$jq_bin" -e '.started[0].app != null' "$li_dir/landed.json" > /dev/null 2>&1 && break
  sleep 0.3
done
acts succeeded
call activity openApp "fspec:install:flatpak:org.example.Foxglove" > "$li_dir/launch.txt" 2>&1
sleep 1
cp "$iso_home/.local/state/formalshell/state.json" "$li_dir/state.json" 2>/dev/null

call menu summon install > /dev/null 2>&1
sleep 0.5
call menu filter fox > /dev/null 2>&1
until_status found '.ids[0] | startswith("apps.")' 10

i=\$("$jq_bin" '.ids | index("install.pkg.flatpak%3Aorg%2Eexample%2EFoxfire")' "$li_dir/found.json")
call menu activate "\$i" > /dev/null 2>&1
sleep 1
fop=\$(cat "$li_dir/last-op")
step "\$fop.1"
go "\$fop.go1"; step "\$fop.2"
sleep 0.6
status failrow
acts failact
call menu close > /dev/null 2>&1
sleep 0.3
call activity retry "fspec:install:flatpak:org.example.Foxfire" > "$li_dir/retry.txt" 2>&1
sleep 2

cat > "$li_apps/org.example.Foxberry.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Foxberry
Exec=true
Icon=folder
DESKTOP
sleep 1.5
call menu summon install > /dev/null 2>&1
sleep 0.5
call menu filter foxb > /dev/null 2>&1
until_status own '.ids[0] == "apps.org.example.Foxberry"' 10
# Tab before the owner query answers: Uninstall joins the open column.
"$wtype_bin" -k Tab
until_status early '.actionsOpen' 3
until_status column '.actions[-1] == "uninstall"' 5
"$wtype_bin" -k End
sleep 0.3
"$wtype_bin" -k Return
sleep 0.4
status armed
shoot armed
before=\$(cat "$li_dir/last-op")
"$wtype_bin" -k Return
for _ in \$(seq 1 30); do [ "\$(cat "$li_dir/last-op")" != "\$before" ] && break; sleep 0.2; done
uop=\$(cat "$li_dir/last-op")
step "\$uop.1"
go "\$uop.go1"; step "\$uop.2"
rm -f "$li_apps/org.example.Foxberry.desktop"
sleep 1.5
go "\$uop.go2"
for _ in \$(seq 1 30); do
  pkgs removed
  "$jq_bin" -e '.started[] | select(.verb == "uninstall") | .op != null' "$li_dir/removed.json" > /dev/null 2>&1 && break
  sleep 0.3
done
sleep 1.5
acts uninstalled
call menu summon install > /dev/null 2>&1
sleep 0.5
call menu filter foxb > /dev/null 2>&1
until_status gonerows '(.ids | length) > 0 and (.ids | index("apps.org.example.Foxberry")) == null' 10

cat > "$li_apps/org.example.Foxtail.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Foxtail
Exec=true
Icon=folder
DESKTOP
sleep 1.5
call menu filter foxtail > /dev/null 2>&1
until_status distro '.ids[0] == "apps.org.example.Foxtail"' 10
for _ in \$(seq 1 30); do
  pkgs distroowner
  "$jq_bin" -e '.owners["org.example.Foxtail"] != null' "$li_dir/distroowner.json" > /dev/null 2>&1 && break
  sleep 0.2
done
call menu actions 0 > /dev/null 2>&1
sleep 0.3
status distrocolumn
call menu close > /dev/null 2>&1
for m in "$li_dir"/smoke-*.[0-9]; do touch "\${m%.*}.go1" "\${m%.*}.go2" "\${m%.*}.go3"; done
EOF
  hypr_exec_once "bash $script"
}

# `<step> <jq claim> <what it means>`, against that step's file.
li_expect() {
  local f="$li_dir/$1.json"
  [ -s "$f" ] || fail "launcher-install: no status for $1"
  "$jq_bin" -e "$2" "$f" > /dev/null || fail "launcher-install $1: $3, got $("$jq_bin" -c '{level, empty, ids: .ids[:5], drawn: .drawn[:3], checked, started, groups}' "$f" 2>/dev/null || cat "$f")"
}

leg_launcher_install_assert() {
  local fox="install.pkg.flatpak%3Aorg%2Eexample%2EFoxglove"
  li_expect idle '.level == "install" and .rows == 0' "Install mode with no query is not an empty list"
  li_expect searching '.empty == "install.searching"' "a query never read Searching… while fspec answered"
  li_expect rows ".ids[0] == \"$fox\" and .drawn[0].value == \"flatpak 1.4.2\" and .drawn[0].picture and (.checked | index(\"install.pkg.zypper%3Afoxtrot-cli\")) != null and .drawn[1].value == \"Installed\" and .pane.kind == \"package\"" "the packages are not listed with their source, version, tick and pane"
  echo "SMOKE_LAUNCHER_INSTALL search rows: source and version, the installed one ticked, a description pane"
  li_expect failed '.empty == "install.failed"' "a failed search did not read as one"
  li_expect none '.empty == "install.none"' "no packages did not read as one"
  li_expect gone '.empty == "install.nofspec"' "fspec gone did not read Installing apps needs fspec"
  "$jq_bin" -r '.empty' "$li_dir/failed.json" > /dev/null
  echo "SMOKE_LAUNCHER_INSTALL dim states: searching, failed, none, no fspec"

  rg -qx 'app install flatpak:org.example.Foxglove --detach' "$li_dir/shim.log" || fail "launcher-install: Enter never reached the shim as app install --detach: $(cat "$li_dir/shim.log")"
  li_expect resolving ".isOpen and .ids[0] == \"$fox\"" "the launcher did not stay open on the row"
  li_expect downloading '.drawn[0].value == "Downloading 40%"' "the row did not follow the stream into Downloading 40%"
  li_expect committing '.drawn[0].value == "Installing 50%"' "the row did not follow the stream into Installing 50%"
  echo "SMOKE_LAUNCHER_INSTALL the row followed the stream: Resolving, Downloading 40%, Installing 50%"
  li_expect running '.groups | map(select(.source == "fspec:install:flatpak:org.example.Foxglove" and .state == "running")) | length == 1' "Activity had no running install row"
  li_expect landed '.started[0] | .verb == "install" and .app == "org.example.Foxglove"' "the landed app was not found as the install's"
  li_expect succeeded '.groups | map(select(.source == "fspec:install:flatpak:org.example.Foxglove" and .state == "succeeded")) | length == 1' "Activity did not read the install succeeded"
  [ "$(cat "$li_dir/launch.txt")" = "ok" ] || fail "launcher-install: activity openApp answered $(cat "$li_dir/launch.txt")"
  [ -e "$li_dir/foxglove-opened" ] || fail "launcher-install: Activity's Open never ran the landed app"
  "$jq_bin" -e '.picks | map(select(.id == "app:org.example.Foxglove")) | length == 1' "$li_dir/state.json" > /dev/null \
    || fail "launcher-install: no pick of the landed app in state.json: $("$jq_bin" -c .picks "$li_dir/state.json")"
  echo "SMOKE_LAUNCHER_INSTALL landed: the app found, picked once, opened from Activity"
  li_expect found ".ids[0] == \"apps.org.example.Foxglove\" and (.ids | index(\"$fox\")) > 0 and (.checked | index(\"$fox\")) != null and (.drawn[] | select(.id == \"$fox\") | .value) == \"Installed\"" "the app subject does not head its package, or the package does not read Installed"
  echo "SMOKE_LAUNCHER_INSTALL the app ranks above its package, which reads Installed"

  li_expect failrow '(.drawn[] | select(.id == "install.pkg.flatpak%3Aorg%2Eexample%2EFoxfire") | .value) == "flatpak: remote flathub is not reachable"' "the failure's reason is not on the row"
  li_expect failact '.groups | map(select(.source == "fspec:install:flatpak:org.example.Foxfire" and .state == "failed")) | length == 1' "Activity did not read the install failed"
  [ "$(cat "$li_dir/retry.txt")" = "ok" ] || fail "launcher-install: activity retry answered $(cat "$li_dir/retry.txt")"
  [ "$(rg -c 'app install flatpak:org.example.Foxfire --detach' "$li_dir/shim.log")" = "2" ] || fail "launcher-install: retry did not reach the shim as the same install: $(cat "$li_dir/shim.log")"
  echo "SMOKE_LAUNCHER_INSTALL a failure on the row and in Activity, retried as the same install"
  li_expect early '.actionsOpen and (.actions | index("uninstall")) == null' "the owner query answered before Tab, so a late answer joining the column went unexercised"
  li_expect column '.actions[-1] == "uninstall"' "Uninstall never joined the open column of an app fspec owns, or not last"
  li_expect armed '.isOpen and .actionsOpen and .actions[.actionCursor] == "uninstall"' "the first Enter on Uninstall did not only arm it"
  rg -qx 'app uninstall flatpak:org.example.Foxberry --detach' "$li_dir/shim.log" || fail "launcher-install: the second Enter never reached the shim as app uninstall --detach: $(cat "$li_dir/shim.log")"
  [ "$(rg -c '^app uninstall' "$li_dir/shim.log")" = "1" ] || fail "launcher-install: the first Enter ran the uninstall too: $(cat "$li_dir/shim.log")"
  li_expect uninstalled '.groups | map(select(.source == "fspec:uninstall:flatpak:org.example.Foxberry" and .state == "succeeded")) | length == 1' "Activity did not read the uninstall succeeded"
  li_expect gonerows '(.ids | index("apps.org.example.Foxberry")) == null' "the uninstalled app is still among the rows"
  li_expect distrocolumn '.actionsOpen and (.actions | index("open")) != null and (.actions | index("uninstall")) == null' "an app the distro owns offered Uninstall"
  echo "SMOKE_LAUNCHER_INSTALL uninstall: listed last, armed then run, the app gone, none on a distro app"
  if rg -qv '^(search --json|app install|app uninstall|app owner)' "$li_dir/shim.log"; then fail "launcher-install: the shell asked fspec something unshimmed: $(cat "$li_dir/shim.log")"; fi

  local shot
  for shot in rows failed ring toast armed; do
    [ -s "$li_dir/$shot.png" ] || fail "launcher-install: no $shot frame"
    echo "SMOKE_LAUNCHER_INSTALL_SHOT $li_dir/$shot.png"
  done
}
