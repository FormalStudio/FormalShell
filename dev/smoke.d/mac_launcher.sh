# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154,SC2016  # dev/smoke.sh reads leg_* and supplies shot_dir, iso_home, the *_bin paths and fail()
# --mac-launcher: a Mac app in the launcher
# (docs/superpowers/specs/2026-11-11-mac-apps.md, section 6), every step a
# real key, read off `menu status`, `dock status`, `activity status`, the
# stub's log and the log of an `atoll` standing in for the installer.
# dev/mac-stub.py is the running Mac app. Its desktop entry is what
# `atoll install` writes: `X-Atoll-BundleId`, a `MimeType`, and an `Exec`
# that runs atoll. A second entry of the same shape, Brittle, names a
# bundle the stand-in refuses. Text Probe is a native app that opens
# text/plain, Other Thing one that names no type.
# The stub's row trails `Mac` and its window, Brittle's `Mac` alone, Text
# Probe's nothing. Tab on the stub's row lists Keep in Dock and Quit Stub,
# and the stub's own dock menu joins once it answers; `Document` typed and
# Enter reach the stub's dock menu as a click on New Document. Keep in Dock
# and Enter pin it, and the column then offers Remove from Dock, which lets
# it go. A recent file named notes.stub, whose type the staged globs2
# gives as text/x-stub, lists under Open with the stub and Text Probe (a
# text type stands under text/plain) ahead of a note and Other Thing; Enter
# on the stub runs its entry on the file. Quit Stub and Enter reach the
# stub as its own Quit item. Uninstall on Brittle arms on the first Enter,
# runs atoll on the second, and the refusal reads in Activity as failed
# with atoll's own line; Uninstall on the stub's row ends succeeded, and
# with its entry gone the launcher has no row for it.
leg_mac_launcher_flag="--mac-launcher"
leg_mac_launcher_order=175
leg_mac_launcher_needs="python3 jq wtype"
leg_mac_launcher_claims="launcher=shared"

mac_launcher_prefix="$shot_dir/mac-launcher"
mac_launcher_log="$shot_dir/mac-launcher-stub.log"
mac_launcher_atoll="$shot_dir/mac-launcher-atoll.log"
mac_launcher_file="$iso_home/Documents/notes.stub"

leg_mac_launcher_fixture() {
  local apps="$iso_home/.local/share/applications" bin="$shot_dir/mac-launcher-bin"
  mkdir -p "$apps" "$bin" "$iso_home/.local/share/mime" "$iso_home/Documents"
  cat > "$bin/atoll" <<EOF
#!/bin/sh
echo "\$*" >> "$mac_launcher_atoll"
[ "\$1" = uninstall ] || exit 0
if [ "\$2" != org.atoll.Stub ]; then
  echo "atoll: \$2 is not installed" >&2
  exit 1
fi
sleep 1
rm -f "$apps/org.atoll.stub.desktop"
echo "uninstalled Stub (\$2)"
EOF
  chmod +x "$bin/atoll"
  cat > "$apps/org.atoll.stub.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Stub
Exec="$bin/atoll" run Stub.app %U
StartupWMClass=org.atoll.stub
X-Atoll-BundleId=org.atoll.Stub
Keywords=Mac;
MimeType=text/x-stub;
EOF
  cat > "$apps/org.atoll.brittle.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Brittle
Exec="$bin/atoll" run Brittle.app %U
StartupWMClass=org.atoll.brittle
X-Atoll-BundleId=org.atoll.Brittle
Keywords=Mac;
EOF
  cat > "$apps/formalshell-text-probe.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Text Probe
Exec=true %U
MimeType=text/plain;
EOF
  cat > "$apps/formalshell-other-thing.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Other Thing
Exec=true
EOF
  echo '50:text/x-stub:*.stub' > "$iso_home/.local/share/mime/globs2"
  echo "a note" > "$mac_launcher_file"
  cat > "$iso_home/.local/share/recently-used.xbel" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<xbel version="1.0" xmlns:bookmark="http://www.freedesktop.org/standards/desktop-bookmarks" xmlns:mime="http://www.freedesktop.org/standards/shared-mime-info">
  <bookmark href="file://$mac_launcher_file" added="2026-11-11T10:00:00Z" modified="2026-11-11T10:00:00Z" visited="2026-11-11T10:00:00Z"/>
</xbel>
EOF
}

leg_mac_launcher_timing() {
  leg_timing 150
}

leg_mac_launcher_drive() {
  local script="$shot_dir/mac-launcher-drive.sh"
  local stub="$PWD/dev/mac-stub.py"
  write_script "$script" <<EOF
#!/usr/bin/env bash
m="$mac_launcher_prefix"
log="$mac_launcher_log"
atoll="$mac_launcher_atoll"
key() { "$wtype_bin" "\$@"; sleep 0.6; }
# need <step...>: the step, or the drive over at the first one that failed,
# named in \$m-gave-up.txt beside the launcher's status then. Every later
# step stands on this one.
need() {
  "\$@" && return
  echo "\$*" > "\$m-gave-up.txt"
  call menu status > "\$m-gave-up.json" 2>&1
  exit 0
}
# row <query> <path> <jq claim>: the launcher open on <query>, its status
# in <path> once the claim holds of it.
row() {
  call menu summon "" > /dev/null 2>&1
  until_settled
  call menu filter "\$1" > /dev/null 2>&1
  until_jq 10 "\$2" "\$3" call menu status
}
# column <path> <jq claim>: Tab on the cursor's row, the status once its
# column holds what the claim asks.
column() {
  key -k Tab
  until_jq 25 "\$1" ".actionsOpen and (\$2)" call menu status
}
# logged <file> <fixed line>: until the file holds the line, 10s at most.
logged() { for _ in \$(seq 1 40); do grep -qxF "\$2" "\$1" 2>/dev/null && return 0; sleep 0.25; done; return 1; }
on_stub='.ids[0] == "apps.org.atoll.stub"'
activity() { call activity status; }

"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$python3_bin $stub --name Stub --log \$log]==])" > /dev/null 2>&1
need until_window org.atoll.stub 15

need row Brittle "\$m-brittle-row.json" '.ids[0] == "apps.org.atoll.brittle"'
need row "Text Probe" "\$m-native-row.json" '.ids[0] == "apps.formalshell-text-probe"'
need row Stub "\$m-row.json" "\$on_stub and .drawn[0].value != \"Mac\""
until_settled
shoot "\$m-row.png"
need column "\$m-column.json" '.actions | index("dock:1") != null'
until_settled
shoot "\$m-column.png"
key Document
status menu "\$m-filtered.json"
key -k Return
need logged "\$log" 'dock: Event(1, clicked)'
status menu "\$m-chosen.json"

need row Stub "\$m-row-keep.json" "\$on_stub"
need column "\$m-column-keep.json" '.actions | index("dock-pin") != null'
key Keep
key -k Return
need until_jq 10 "\$m-kept.json" 'any(.apps[]; .app == "org.atoll.stub" and .pinned)' call dock status
need row Stub "\$m-row-kept.json" "\$on_stub"
need column "\$m-column-kept.json" '.actions | index("dock-unpin") != null'
key Remove
key -k Return
need until_jq 10 "\$m-let-go.json" 'all(.apps[]; .pinned | not)' call dock status

need row notes.stub "\$m-file-row.json" '.ids[0] | startswith("files.")'
need column "\$m-file-column.json" '.actions | index("open-with") != null'
key "Open with"
key -k Return
need until_jq 10 "\$m-targets.json" '.actions | index("target-app:org.atoll.stub") != null' call menu status
until_settled
shoot "\$m-targets.png"
key Stub
status menu "\$m-target-filtered.json"
key -k Return
need logged "\$atoll" "run Stub.app $mac_launcher_file"
status activity "\$m-opened.json"

need row Stub "\$m-row-quit.json" "\$on_stub"
need column "\$m-column-quit.json" '.actions | index("quit") != null'
key Quit
status menu "\$m-quit-filtered.json"
key -k Return
need logged "\$log" quit
need until_jq 10 "\$m-quit.json" 'all(.apps[]; .running | not)' call dock status

need row Brittle "\$m-brittle.json" '.ids[0] == "apps.org.atoll.brittle"'
need column "\$m-brittle-column.json" '.actions | index("uninstall") != null'
key Uninstall
key -k Return
status menu "\$m-armed.json"
shoot "\$m-armed.png"
key -k Return
need until_jq 15 "\$m-failed.json" 'any(.groups[].items[]; .id == "atoll:uninstall:org.atoll.brittle" and .state == "failed")' activity
until_settled
shoot "\$m-failed.png"

need row Stub "\$m-row-uninstall.json" "\$on_stub"
need column "\$m-column-uninstall.json" '.actions | index("uninstall") != null'
key Uninstall
key -k Return
key -k Return
need until_jq 5 "\$m-running.json" '[.groups[].items[], .history[]] | any(.id == "atoll:uninstall:org.atoll.stub")' activity
need until_jq 15 "\$m-uninstalled.json" '[.groups[].items[], .history[]] | any(.id == "atoll:uninstall:org.atoll.stub" and .state == "succeeded")' activity
need row Stub "\$m-row-gone.json" '.ids | index("apps.org.atoll.stub") == null'
call menu close > /dev/null 2>&1
EOF
  write_script "$shot_dir/mac-launcher-kill.sh" <<EOF
#!/usr/bin/env bash
pkill -f "$stub --name Stub --log $mac_launcher_log" 2>/dev/null || true
EOF
  add_cleanup "bash $shot_dir/mac-launcher-kill.sh"
  hypr_exec_once "bash $script"
}

leg_mac_launcher_assert() {
  local m="$mac_launcher_prefix"
  local column='{actionsOpen, actions}'
  local shot
  for shot in row column targets armed failed; do
    [ ! -f "$m-$shot.png" ] || echo "SMOKE_MAC_LAUNCHER_$(echo "$shot" | tr 'a-z' 'A-Z') $m-$shot.png"
  done
  echo "SMOKE_MAC_LAUNCHER_SHELL $shell_log_path"
  [ ! -f "$m-gave-up.txt" ] || fail "--mac-launcher: the drive stopped at a step that never came true: $(cat "$m-gave-up.txt"): $("$jq_bin" -c '{isOpen, query, ids: .ids[:4], drawn: .drawn[:2], actionsOpen, actions}' "$m-gave-up.json" 2>/dev/null)"
  [ -s "$mac_launcher_log" ] || fail "--mac-launcher: the stub logged nothing"
  cat "$mac_launcher_log" "$mac_launcher_atoll"

  expect_jq "$m-row.json" '.drawn[0].value == "Mac · 1 window"' "--mac-launcher: the running stub's row does not trail Mac and its window" '.drawn[0]'
  expect_jq "$m-brittle-row.json" '.drawn[0].value == "Mac"' "--mac-launcher: a Mac app with no window does not trail Mac alone" '.drawn[0]'
  expect_jq "$m-native-row.json" '.drawn[0].value == ""' "--mac-launcher: a native app's row trails something" '.drawn[0]'
  expect_jq "$m-column.json" '.actions as $a | ["open", "new-window", "windows", "move-to-workspace", "dock-pin", "quit", "dock:1", "dock:2"] | all(. as $id | $a | index($id) != null)' \
    "--mac-launcher: Tab on the stub's row did not list Keep in Dock, Quit and the stub's own dock menu" "$column"
  expect_jq "$m-column.json" '(.actions | index("dock-pin")) < (.actions | index("dock:1"))' "--mac-launcher: the stub's dock menu did not join under its actions" "$column"
  expect_jq "$m-filtered.json" '.actions == ["dock:1"]' "--mac-launcher: Document typed did not leave New Document alone" "$column"
  expect_jq "$m-chosen.json" '.isOpen | not' "--mac-launcher: Enter on New Document left the launcher open" '{isOpen}'
  expect_jq "$m-kept.json" 'any(.apps[]; .app == "org.atoll.stub" and .pinned)' "--mac-launcher: Keep in Dock did not pin the stub"
  expect_jq "$m-column-kept.json" '(.actions | index("dock-unpin")) != null and (.actions | index("dock-pin")) == null' \
    "--mac-launcher: a kept app's column does not offer Remove from Dock in Keep in Dock's place" "$column"
  expect_jq "$m-let-go.json" 'all(.apps[]; .pinned | not)' "--mac-launcher: Remove from Dock left the stub pinned"

  expect_jq "$m-targets.json" '.actions as $a | ($a | index("target-app:others")) as $note
      | $note != null and ($a | index("target-app:org.atoll.stub")) < $note and ($a | index("target-app:formalshell-text-probe")) < $note
      and ($a | index("target-app:formalshell-other-thing")) > $note' \
    "--mac-launcher: Open with does not list the apps that open the file's type ahead of a note and the rest" "$column"
  expect_jq "$m-target-filtered.json" '.actions[0] == "target-app:org.atoll.stub"' "--mac-launcher: Stub typed did not put the stub first under Open with" "$column"
  grep -qxF "run Stub.app $mac_launcher_file" "$mac_launcher_atoll" || fail "--mac-launcher: Enter on the stub under Open with did not run its entry on the file"
  expect_jq "$m-opened.json" '[.groups[].items[], .history[]] | all(.id != "launch:org.atoll.stub")' \
    "--mac-launcher: a file handed to the running stub left a launch waiting on a window" '{groups, history}'

  expect_jq "$m-quit-filtered.json" '.actions == ["quit"]' "--mac-launcher: Quit typed did not leave Quit Stub alone" "$column"
  grep -qxF 'menu: Event(15, clicked)' "$mac_launcher_log" || fail "--mac-launcher: Quit Stub did not reach the stub as its own Quit item"
  grep -qxF quit "$mac_launcher_log" || fail "--mac-launcher: the stub did not quit"

  expect_jq "$m-armed.json" '.isOpen and .actionsOpen and .actions == ["uninstall"]' "--mac-launcher: the first Enter on Uninstall did not arm it in the column" "$column"
  expect_jq "$m-failed.json" '.groups[].items[] | select(.id == "atoll:uninstall:org.atoll.brittle") | .origin == "atoll" and .kind == "uninstall" and .title == "Uninstalling Brittle" and .summary == "org.atoll.Brittle is not installed"' \
    "--mac-launcher: atoll's refusal does not read in Activity as a failed uninstall with its own line" '.groups'
  grep -qxF 'uninstall org.atoll.Brittle' "$mac_launcher_atoll" || fail "--mac-launcher: Uninstall on Brittle never ran atoll on its bundle id"
  expect_jq "$m-uninstalled.json" '[.groups[].items[], .history[]] | any(.id == "atoll:uninstall:org.atoll.stub" and .state == "succeeded" and .summary == "Uninstalled Stub")' \
    "--mac-launcher: the stub's uninstall did not end succeeded in Activity" '{groups, history}'
  grep -qxF 'uninstall org.atoll.Stub' "$mac_launcher_atoll" || fail "--mac-launcher: Uninstall on the stub never ran atoll on its bundle id"
  expect_jq "$m-row-gone.json" '.ids | index("apps.org.atoll.stub") == null' "--mac-launcher: the uninstalled app kept its row" '{ids: .ids[:6]}'
  for shot in row column targets armed failed; do
    [ -f "$m-$shot.png" ] || fail "--mac-launcher: no $shot frame"
  done
}
