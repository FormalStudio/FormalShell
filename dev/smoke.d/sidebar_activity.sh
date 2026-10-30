# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, iso_home, the *_bin paths and fail()
# --sidebar-activity: the Activity card under `layout: sidebar`, Pond's
# installs card (shell/installs/SidebarCard.qml). dev/fspec-activity-stub.py
# plays three fspec operations on the session's real stream: an install of
# Firefox held at 40%, an install of "Visual Studio Code Insiders" held at
# resolving (no measure) and an apply of a package build whose ring follows
# FormalSpec's `step`: indeterminate fetching dependencies, compiling
# rising 120 then 300 of 450 units, packaging stalled, read off `activity
# status` at each step while the operation's own count sits at 0 of 3.
# Read off `sidebar status`: the `activity` card straight after `notifications`, one line a
# group, each line's hit 32 tall and the next 32 below it, and tesseract
# over the card's crop reading "Firefox" whole, where the old row cut a
# title to its first letter. Then the Firefox install runs to success and
# its line reads done. The card mid-install, mid-compile and done is
# cropped for reading.
leg_sidebar_activity_flag="--sidebar-activity"
leg_sidebar_activity_order=238
leg_sidebar_activity_needs="jq python3 convert tesseract"

sa_marks="$shot_dir/sidebar-activity"

leg_sidebar_activity_validate() {
  if leg_on bar_position || leg_on frame; then
    echo "usage: --sidebar-activity places the column itself and cannot combine with --bar-position or --frame" >&2
    exit 1
  fi
}

leg_sidebar_activity_fixture() {
  leg_on sidebar || settings_fragment ', "layout": "sidebar"'
}

leg_sidebar_activity_timing() {
  leg_timing 20 60
}

leg_sidebar_activity_drive() {
  local script="$shot_dir/sidebar-activity-drive.sh"
  local stub="$script_dir/fspec-activity-stub.py"
  write_script "$script" <<EOF
#!/usr/bin/env bash
m="$sa_marks"
dir="\${XDG_RUNTIME_DIR:-/run/user/\$(id -u)}/fspec/activity"
await() { for _ in \$(seq 200); do [ -e "\$1" ] && return 0; sleep 0.05; done; echo "timed out on \$1" >> "\$m-drive.log"; }
for _ in \$(seq 100); do
  $ipc call activity status 2>/dev/null | "$jq_bin" -e '.fspec.watching' > /dev/null 2>&1 && break
  sleep 0.1
done
# Firefox to preparing 40% (its second step), Code at resolving, the apply at 1/4.
FSPEC_STUB_PKG=flatpak:org.mozilla.firefox FSPEC_STUB_TITLE=Firefox "$python3_bin" "$stub" "\$dir" sa-firefox install "\$m.ff" 2>> "\$m-drive.log" &
await "\$m.ff.1"; touch "\$m.ff.go1"; await "\$m.ff.2"
FSPEC_STUB_PKG=flatpak:com.visualstudio.code.insiders FSPEC_STUB_TITLE="Visual Studio Code Insiders" "$python3_bin" "$stub" "\$dir" sa-code install "\$m.code" 2>> "\$m-drive.log" &
await "\$m.code.1"
"$python3_bin" "$stub" "\$dir" sa-build build "\$m.build" 2>> "\$m-drive.log" &
await "\$m.build.1"
sleep 1
$ipc call sidebar status > "\$m-status-running.json" 2>&1
$ipc call activity status > "\$m-activity-running.json" 2>&1
"$hyprctl_bin" -j layers > "\$m-layers.json" 2>&1
"$grim_bin" "\$m-running.png" > /dev/null 2>&1
# The build through its steps: compiling twice, then packaging stalled.
for n in 1 2 3; do
  touch "\$m.build.go\$n"; await "\$m.build.\$((n + 1))"; sleep 0.5
  $ipc call activity status > "\$m-activity-build\$((n + 1)).json" 2>&1
  [ \$n = 2 ] && "$grim_bin" "\$m-rising.png" > /dev/null 2>&1
done
# Firefox on to committing and succeeded; read inside the fold's 4s.
touch "\$m.ff.go2"; await "\$m.ff.3"; touch "\$m.ff.go3"; await "\$m.ff.4"
sleep 0.8
$ipc call sidebar status > "\$m-status-done.json" 2>&1
$ipc call activity status > "\$m-activity-done.json" 2>&1
"$grim_bin" "\$m-done.png" > /dev/null 2>&1
touch "\$m.code.go1" "\$m.build.go4" "\$m-drive.done"
EOF
  hypr_exec_once "bash $script"
}

# The card's crop off a frame: the union of the `open:` lines' hits, plus
# the card's own 8px above and below, moved by the layer's origin.
sa_crop() {
  local status=$1 png=$2 out=$3 geom
  geom=$("$jq_bin" -r --slurpfile layers "$sa_marks-layers.json" '
    ([$layers[0][] | .levels[] | .[] | select(.namespace == "formalshell:bar")] | first) as $l
    | [.hits[] | select(.on | startswith("open:"))] as $h
    | ($h | map(.y) | min) as $y0 | ($h | map(.y + .h) | max) as $y1
    | "\(.width - 8)x\($y1 - $y0 + 16)+\($l.x + 4)+\($l.y + $y0 - 8)"' "$status")
  "$convert_bin" "$png" -crop "$geom" +repage "$out"
}

leg_sidebar_activity_assert() {
  local running="$sa_marks-status-running.json" done="$sa_marks-status-done.json"
  [ -e "$sa_marks-drive.done" ] || fail "--sidebar-activity: the drive never finished ($(cat "$sa_marks-drive.log" 2>/dev/null))"
  "$jq_bin" -e '.cards | index("notifications") as $n | index("activity") == $n + 1' "$running" > /dev/null \
    || fail "--sidebar-activity: the activity card is not straight after notifications: $("$jq_bin" -c .cards "$running")"
  local lines
  lines=$("$jq_bin" -c '[.hits[] | select(.on | startswith("open:")) | {on, y, h}] | sort_by(.y)' "$running")
  "$jq_bin" -e 'length == 3' <<< "$lines" > /dev/null || fail "--sidebar-activity: expected three lines, got $lines"
  "$jq_bin" -e 'all(.h == 32) and ([range(1; length) as $i | .[$i].y - .[$i - 1].y] | all(. == 32))' <<< "$lines" > /dev/null \
    || fail "--sidebar-activity: lines are not 32 tall and 32 apart: $lines"
  sa_crop "$running" "$sa_marks-running.png" "$shot_dir/sidebar-activity-card-running.png"
  local text
  text=$("$convert_bin" "$shot_dir/sidebar-activity-card-running.png" -resize 400% -colorspace Gray -negate - | "$tesseract_bin" - - 2>/dev/null | tr '\n' ' ')
  echo "$text" > "$sa_marks-ocr.txt"
  grep -q "Firefox" <<< "$text" || fail "--sidebar-activity: the card does not read Firefox whole: $text"
  "$jq_bin" -e '[.. | objects | select(.source? == "fspec:install:flatpak:org.mozilla.firefox" and .state? == "succeeded")] | length > 0' \
    "$sa_marks-activity-done.json" > /dev/null || fail "--sidebar-activity: the Firefox install never read succeeded"
  "$jq_bin" -e '[.hits[] | select(.on == "open:fspec:install:flatpak:org.mozilla.firefox")] | length == 1' "$done" > /dev/null \
    || fail "--sidebar-activity: the finished Firefox line left the card before its fold"
  # The build's ring off FormalSpec's `step`: none while fetching, rising
  # through compiling, none again while packaging stalls, never the
  # operation's own 0 of 3.
  local build='[.. | objects | select(.source? == "fspec:apply:smoke-build" and has("detail") and has("progress"))] | first'
  "$jq_bin" -e "$build | .progress == null" "$sa_marks-activity-running.json" > /dev/null \
    || fail "--sidebar-activity: fetching dependencies is not indeterminate: $("$jq_bin" -c "$build" "$sa_marks-activity-running.json")"
  "$jq_bin" -e "$build | .progress.done == 120 and .progress.total == 450" "$sa_marks-activity-build2.json" > /dev/null \
    || fail "--sidebar-activity: compiling does not read 120 of 450: $("$jq_bin" -c "$build" "$sa_marks-activity-build2.json")"
  "$jq_bin" -e "$build | .progress.done == 300" "$sa_marks-activity-build3.json" > /dev/null \
    || fail "--sidebar-activity: compiling did not rise to 300: $("$jq_bin" -c "$build" "$sa_marks-activity-build3.json")"
  "$jq_bin" -e "$build | .progress == null and (.detail | test(\"packaging, no output\"))" "$sa_marks-activity-build4.json" > /dev/null \
    || fail "--sidebar-activity: the stalled packaging step does not read so: $("$jq_bin" -c "$build" "$sa_marks-activity-build4.json")"
  sa_crop "$running" "$sa_marks-rising.png" "$shot_dir/sidebar-activity-card-rising.png"
  sa_crop "$done" "$sa_marks-done.png" "$shot_dir/sidebar-activity-card-done.png"
  echo "SMOKE_SIDEBAR_ACTIVITY_RUNNING $shot_dir/sidebar-activity-card-running.png"
  echo "SMOKE_SIDEBAR_ACTIVITY_RISING $shot_dir/sidebar-activity-card-rising.png"
  echo "SMOKE_SIDEBAR_ACTIVITY_DONE $shot_dir/sidebar-activity-card-done.png"
  echo "SMOKE_SIDEBAR_ACTIVITY_FRAME $sa_marks-running.png"
}
