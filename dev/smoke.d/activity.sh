# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --activity drives the Activity service (`services/activity.rs`, the model
# in `crates/fs-activity`) from all three of its sources, read off `activity
# status` at known steps and once off `debug dump`.
#
# fspec: dev/fspec-activity-stub.py plays three operations on their own
# sockets in the session's real `$XDG_RUNTIME_DIR/fspec/activity`, found by
# inotify: an apply at 1/4 then 3/4 resources and succeeded, an update that
# fails, and a rollback whose final line is cut short with its socket gone,
# which reads failed. Four seconds after the apply succeeds it has folded
# into the history while both failures stay; `activity seen` folds them.
# A Gio client sends `com.canonical.Unity.LauncherEntry` updates for a staged
# desktop entry (progress, count, urgent, then progress hidden) and leaves
# the bus, taking its badge. notify-send sends a `value` hint replaced
# in place by `-r` and by an `x-canonical-private-synchronous` tag, and a
# replace without a value ends it.
#
# Where the rig's fspec has `fspec run`, one real `fspec run --detach` of a
# small package is read as a `run` item; an fspec without it prints why the
# check was skipped.
leg_activity_flag="--activity"
leg_activity_order=236
leg_activity_needs="jq python3 notify-send"

act_marks="$shot_dir/act"
act_app_id="formalshell-smoke-activity"

leg_activity_fixture() {
  cat > "$iso_home/.local/share/applications/$act_app_id.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Smoke Activity
Exec=true
Icon=folder
EOF
}

leg_activity_timing() {
  leg_timing 44 90
}

leg_activity_drive() {
  local script="$shot_dir/activity-drive.sh" launcher="$shot_dir/activity-launcher.py"
  local stub="$script_dir/fspec-activity-stub.py"
  write_script "$launcher" <<'EOF'
import os
import sys
import time

from gi.repository import Gio, GLib

uri, marks = sys.argv[1:3]
bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
steps = [
    {"progress": GLib.Variant("d", 0.4), "progress-visible": GLib.Variant("b", True),
     "count": GLib.Variant("x", 3), "count-visible": GLib.Variant("b", True)},
    {"progress": GLib.Variant("d", 0.8), "urgent": GLib.Variant("b", True)},
    {"progress-visible": GLib.Variant("b", False), "urgent": GLib.Variant("b", False)},
]
for n, props in enumerate(steps, start=1):
    if n > 1:
        while not os.path.exists(f"{marks}.go{n - 1}"):
            time.sleep(0.05)
    bus.emit_signal(None, "/com/canonical/unity/launcherentry/1", "com.canonical.Unity.LauncherEntry", "Update",
                    GLib.Variant("(sa{sv})", (uri, props)))
    bus.flush_sync(None)
    with open(f"{marks}.{n}", "w") as f:
        f.write("sent\n")
while not os.path.exists(f"{marks}.go{len(steps)}"):
    time.sleep(0.05)
EOF
  write_script "$script" <<EOF
#!/usr/bin/env bash
m="$act_marks"
dir="\${XDG_RUNTIME_DIR:-/run/user/\$(id -u)}/fspec/activity"
status() { $ipc call activity status > "\$m-\$1.json" 2>&1; }
await() { for _ in \$(seq 200); do [ -e "\$1" ] && return 0; sleep 0.05; done; echo "timed out on \$1" >> "\$m-drive.log"; }
for _ in \$(seq 100); do
  $ipc call activity status 2>/dev/null | jq -e '.fspec.watching' > /dev/null 2>&1 && break
  sleep 0.1
done
command -v fspec > "\$m-fspec-path.txt" 2>&1 || true
status idle

# fspec: an apply that succeeds, an update that fails, a rollback that dies.
"$python3_bin" "$stub" "\$dir" smoke-ok ok "\$m.ok" 2>> "\$m-drive.log" &
await "\$m.ok.1"; sleep 0.5; status ok-1
touch "\$m.ok.go1"; await "\$m.ok.2"; sleep 0.5; status ok-2
"$python3_bin" "$stub" "\$dir" smoke-fail fail "\$m.fail" 2>> "\$m-drive.log" &
await "\$m.fail.1"; sleep 0.5; status fail-1
$ipc call debug dump > "\$m-dump.json" 2>&1
touch "\$m.ok.go2"; await "\$m.ok.3"; sleep 0.5; status ok-3
touch "\$m.fail.go1"; await "\$m.fail.2"; sleep 0.5; status fail-2
"$python3_bin" "$stub" "\$dir" smoke-die die "\$m.die" 2>> "\$m-drive.log" &
await "\$m.die.1"; sleep 0.5; status die-1
touch "\$m.die.go1"; await "\$m.die.2"; sleep 0.5; status die-2
sleep 4; status folded
$ipc call activity seen > "\$m-seen.txt" 2>&1; sleep 0.3; status seen

# com.canonical.Unity.LauncherEntry from a client that stays on the bus.
"$python3_bin" "$launcher" "application://$act_app_id.desktop" "\$m.l" 2>> "\$m-drive.log" &
lpid=\$!
await "\$m.l.1"; sleep 0.5; status l-1
touch "\$m.l.go1"; await "\$m.l.2"; sleep 0.5; status l-2
touch "\$m.l.go2"; await "\$m.l.3"; sleep 0.5; status l-3
touch "\$m.l.go3"; wait "\$lpid"; sleep 0.5; status l-gone

# The notification value hint, replaced by id and by synchronous tag.
id=\$("$notify_send_bin" -p -a smoke-dl -h int:value:10 Downloading a.iso)
echo "\$id" > "\$m-notify-id.txt"
sleep 0.5; status n-1
"$notify_send_bin" -r "\$id" -a smoke-dl -h int:value:60 Downloading a.iso
"$notify_send_bin" -a smoke-sync -h string:x-canonical-private-synchronous:smoke-copy -h int:value:30 Copying b.img
"$notify_send_bin" -a smoke-sync -h string:x-canonical-private-synchronous:smoke-copy -h int:value:70 Copying b.img
sleep 0.5; status n-2
"$notify_send_bin" -r "\$id" -a smoke-dl Downloaded a.iso
sleep 0.5; status n-3

# A real fspec run, where this fspec publishes one.
if command -v fspec > /dev/null 2>&1 && fspec run --help > /dev/null 2>&1; then
  fspec run --detach jq -- --version > "\$m-run.log" 2>&1 &
  for _ in \$(seq 120); do
    $ipc call activity status > "\$m-run.json" 2>&1
    jq -e '[.groups[].items[], .history[]] | any(.kind == "run")' "\$m-run.json" > /dev/null 2>&1 && break
    sleep 0.5
  done
else
  echo "this host's fspec has no run verb: \$(fspec --version 2>&1 | head -1)" > "\$m-run-skipped.txt"
fi
echo done > "\$m-drive-done.txt"
EOF
  add_cleanup "rm -f \"\${XDG_RUNTIME_DIR:-/run/user/\$(id -u)}\"/fspec/activity/smoke-*.sock"
  hypr_exec_once "bash $script"
}

leg_activity_assert() {
  local m="$act_marks" f id
  [ -s "$m-drive-done.txt" ] || fail "activity: the drive script never finished ($(cat "$m-drive.log" 2>/dev/null))"
  for f in idle ok-1 ok-2 fail-1 ok-3 fail-2 die-1 die-2 folded seen l-1 l-2 l-3 l-gone n-1 n-2 n-3; do
    [ -s "$m-$f.json" ] || fail "activity: $m-$f.json missing or empty"
    jq -e . "$m-$f.json" > /dev/null || fail "activity: $f is not JSON: $(cat "$m-$f.json")"
  done
  local fspec_dir
  fspec_dir=$(jq -r '.fspec.dir' "$m-idle.json")
  jq -e '.fspec.watching and .fspec.error == null and .groups == []' "$m-idle.json" > /dev/null \
    || fail "activity: not watching an empty fspec directory at start: $(cat "$m-idle.json")"
  if [ -s "$m-fspec-path.txt" ] && [ -x "$(cat "$m-fspec-path.txt")" ]; then
    jq -e '.fspec.installed == true' "$m-idle.json" > /dev/null || fail "activity: fspec is on PATH but reads not installed"
  else
    jq -e '.fspec.installed == false' "$m-idle.json" > /dev/null || fail "activity: no fspec on PATH but reads installed"
  fi
  echo "SMOKE_ACTIVITY_FSPEC dir=$fspec_dir installed=$(jq -r .fspec.installed "$m-idle.json")"

  local item='[.groups[].items[]] | map(select(.id == $id)) | first'
  jq -e --arg id fspec:smoke-ok "$item"' | .state == "running" and .kind == "apply" and .phase == "preparing"
    and .progress == {"fraction": 0.25, "done": 1, "total": 4, "unit": "resources"}
    and .detail == "zypper: hello" and .keys == ["pkg:hello"] and .source == "fspec:apply:smoke"' "$m-ok-1.json" > /dev/null \
    || fail "activity: the apply's first step did not read 1/4: $(cat "$m-ok-1.json")"
  jq -e --arg id fspec:smoke-ok "$item"' | .phase == "committing" and .progress.fraction == 0.75 and .keys == ["file:~/.config/smoke"]' \
    "$m-ok-2.json" > /dev/null || fail "activity: the apply's update did not replace the step whole: $(cat "$m-ok-2.json")"
  jq -e '[.groups[].source] == ["fspec:apply:smoke", "fspec:update:/smoke/spec"] and all(.groups[]; .count == 1)' \
    "$m-fail-1.json" > /dev/null || fail "activity: two sources are not two groups in start order: $(cat "$m-fail-1.json")"
  jq -e '.activity.groups | map(.source) == ["fspec:apply:smoke", "fspec:update:/smoke/spec"]' "$m-dump.json" > /dev/null \
    || fail "activity: debug dump does not carry the same groups: $(jq -c .activity "$m-dump.json" 2>&1)"
  jq -e --arg id fspec:smoke-ok "$item"' | .state == "succeeded" and .summary == "4 changes" and .ended != null' \
    "$m-ok-3.json" > /dev/null || fail "activity: the succeeded apply is not held as finished: $(cat "$m-ok-3.json")"
  jq -e '.groups[0].source == "fspec:update:/smoke/spec" and .groups[0].state == "failed" and .groups[0].urgent
    and .groups[0].items[0].summary == "resolve hello: no release on github"' "$m-fail-2.json" > /dev/null \
    || fail "activity: the failed update is not the urgent first group: $(cat "$m-fail-2.json")"
  jq -e --arg id fspec:smoke-die "$item"' | .state == "running" and .kind == "rollback" and .progress == null' \
    "$m-die-1.json" > /dev/null || fail "activity: the rollback did not read indeterminate: $(cat "$m-die-1.json")"
  jq -e --arg id fspec:smoke-die "$item"' | .state == "failed" and .urgent and .summary == "fspec stopped without a result"' \
    "$m-die-2.json" > /dev/null || fail "activity: a cut final line and a gone socket did not read as a death: $(cat "$m-die-2.json")"
  jq -e '([.groups[].items[].id] | sort) == ["fspec:smoke-die", "fspec:smoke-fail"] and ([.history[].id] == ["fspec:smoke-ok"])' \
    "$m-folded.json" > /dev/null || fail "activity: past four seconds the success did not fold with both failures kept: $(cat "$m-folded.json")"
  grep -qx ok "$m-seen.txt" || fail "activity: seen answered $(cat "$m-seen.txt")"
  jq -e '.groups == [] and ([.history[].id] | sort) == ["fspec:smoke-die", "fspec:smoke-fail", "fspec:smoke-ok"]' \
    "$m-seen.json" > /dev/null || fail "activity: seen did not fold the failures: $(cat "$m-seen.json")"

  jq -e --arg id "launcher:$act_app_id" "$item"' | .state == "running" and .title == "Smoke Activity" and .progress == {"fraction": 0.4}
    and (.urgent | not)' "$m-l-1.json" > /dev/null || fail "activity: the LauncherEntry progress did not land: $(cat "$m-l-1.json")"
  jq -e --arg app "$act_app_id" '.badges == [{"app": $app, "count": 3, "urgent": false}]' "$m-l-1.json" > /dev/null \
    || fail "activity: the LauncherEntry count is not the app's badge: $(cat "$m-l-1.json")"
  jq -e --arg id "launcher:$act_app_id" "$item"' | .progress.fraction == 0.8 and .urgent' "$m-l-2.json" > /dev/null \
    || fail "activity: a partial update did not merge into the entry: $(cat "$m-l-2.json")"
  jq -e --arg app "$act_app_id" '.badges == [{"app": $app, "count": 3, "urgent": true}] and .groups[0].urgent' "$m-l-2.json" > /dev/null \
    || fail "activity: urgent did not reach the badge and the group: $(cat "$m-l-2.json")"
  jq -e --arg id "launcher:$act_app_id" "$item"' | .state == "done" and (.urgent | not)' "$m-l-3.json" > /dev/null \
    || fail "activity: hiding the progress did not end the item: $(cat "$m-l-3.json")"
  jq -e '.badges == []' "$m-l-gone.json" > /dev/null || fail "activity: the badge outlived its sender on the bus: $(cat "$m-l-gone.json")"

  id=$(cat "$m-notify-id.txt" 2>/dev/null)
  [ -n "$id" ] || fail "activity: notify-send -p printed no id"
  jq -e --arg id "notification:$id" "$item"' | .state == "running" and .title == "Downloading" and .detail == "a.iso"
    and .progress == {"fraction": 0.1} and .app == "smoke-dl"' "$m-n-1.json" > /dev/null \
    || fail "activity: the value hint did not land: $(cat "$m-n-1.json")"
  local started
  started=$(jq --arg id "notification:$id" "$item"' | .started' "$m-n-1.json")
  jq -e --arg id "notification:$id" --argjson started "$started" "$item"' | .progress.fraction == 0.6 and .started == $started' \
    "$m-n-2.json" > /dev/null || fail "activity: replaces_id did not update the item in place: $(cat "$m-n-2.json")"
  jq -e '[.groups[] | select(.source == "app:smoke-sync") | .items[]] | length == 1 and .[0].progress.fraction == 0.7' \
    "$m-n-2.json" > /dev/null || fail "activity: the synchronous tag did not update in place: $(cat "$m-n-2.json")"
  jq -e --arg id "notification:$id" "$item"' | .state == "done" and .title == "Downloaded"' "$m-n-3.json" > /dev/null \
    || fail "activity: a replace without a value did not end the item: $(cat "$m-n-3.json")"

  if [ -s "$m-run-skipped.txt" ]; then
    echo "SMOKE_ACTIVITY_FSPEC_RUN skipped: $(cat "$m-run-skipped.txt")"
  else
    jq -e '[.groups[].items[], .history[]] | any(.kind == "run" and .origin == "fspec")' "$m-run.json" > /dev/null \
      || fail "activity: a real fspec run never showed as a run item: $(cat "$m-run.json") $(cat "$m-run.log")"
    echo "SMOKE_ACTIVITY_FSPEC_RUN $(jq -c '[.groups[].items[], .history[]] | map(select(.kind == "run")) | first' "$m-run.json")"
  fi
  for f in ok-2 fail-2 l-2 n-2; do
    echo "SMOKE_ACTIVITY_STATUS_$(printf %s "$f" | tr a-z- A-Z_) $(jq -c . "$m-$f.json")"
  done
}
