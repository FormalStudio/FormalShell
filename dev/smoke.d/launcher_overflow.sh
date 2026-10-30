# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, iso_home, the *_bin paths and fail()
# --launcher-overflow: the launcher spec's §11 overflow, on a headless
# 1280x720 output created for the purpose and focused before every open,
# read off `menu status` (`cardBox`, `rowBoxes`, `actionsBox`, `cardWidth`,
# `actions`, `actionCursor`, `actionsScroll`, `actionsMore`).
#
# A staged app whose name runs 200 characters, searched by its first two
# words: every row's drawn box sits inside the card's. A launcher plugin
# whose mode holds one row carrying 40 actions of its own: Tab on it opens
# the 840 card, which fits the 1280 output whole, the column's last visible
# row reading `n more`; a real End then takes the cursor to the last action
# with the column scrolled to its end and nothing more below. The long row
# and the column at both ends are photographed off that output alone, the
# column again in light.
#
# A recent file whose name and parent directory both run past the row: its
# dim trailing value (the directory) is read off the frame, its ink no
# wider than 40% of the row and its left edge one gap from the elided
# name's, so the name takes the rest.
leg_launcher_overflow_flag="--launcher-overflow"
leg_launcher_overflow_order=34
leg_launcher_overflow_needs="jq wtype python3"

launcher_overflow_output="HEADLESS-OVERFLOW"
launcher_overflow_dir="$iso_home/Documents$(printf '/a-directory-nested-far-down-%s' 1 2 3 4)"
launcher_overflow_file="$launcher_overflow_dir/overflow-trail-probe-with-a-file-name-that-runs-on-and-on.txt"

leg_launcher_overflow_fixture() {
  local d="$iso_home/.config/formalshell/plugins/overflow-probe" name
  mkdir -p "$launcher_overflow_dir"
  echo probe > "$launcher_overflow_file"
  cat > "$iso_home/.local/share/recently-used.xbel" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<xbel version="1.0" xmlns:bookmark="http://www.freedesktop.org/standards/desktop-bookmarks" xmlns:mime="http://www.freedesktop.org/standards/shared-mime-info">
  <bookmark href="file://$launcher_overflow_file" added="2026-10-30T10:00:00Z" modified="2026-10-30T10:00:00Z" visited="2026-10-30T10:00:00Z"/>
</xbel>
EOF
  name="Overflow Probe$(printf ' with a name that keeps on going%.0s' $(seq 1 6))"
  name="${name:0:200}"
  mkdir -p "$iso_home/.local/share/applications" "$d"
  cat > "$iso_home/.local/share/applications/formalshell-overflow-probe.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=$name
Exec=true
EOF
  cat > "$d/manifest.json" <<'EOF'
{
  "apiVersion": 1,
  "id": "overflow-probe",
  "kind": "launcher",
  "entry": "entry.sh",
  "mode": "Overflow"
}
EOF
  cat > "$d/entry.sh" <<'EOF'
#!/usr/bin/env bash
actions=$(seq 1 40 | sed 's/.*/{"id":"act-&","label":"Plugin action &"}/' | paste -sd,)
printf '{"rows":[{"id":"many","icon":"list","text":"Forty actions","actions":[%s]}]}\n' "$actions"
while IFS= read -r _; do :; done
EOF
  chmod +x "$d/entry.sh"
}

leg_launcher_overflow_timing() {
  leg_timing 35 95
}

leg_launcher_overflow_drive() {
  local script="$shot_dir/launcher-overflow-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
call() { $ipc call "\$@"; }
status() { call menu status > "$shot_dir/launcher-overflow-\$1.json" 2>&1; }
shoot() { "$grim_bin" -o "$launcher_overflow_output" "$shot_dir/launcher-overflow-\$1.png" > /dev/null 2>&1; }
focus() { "$hyprctl_bin" dispatch "hl.dsp.focus({ monitor = '$launcher_overflow_output' })" > /dev/null 2>&1; sleep 0.4; }
sleep 4
width=\$("$hyprctl_bin" -j monitors | "$jq_bin" -r '.[0].width')
"$hyprctl_bin" output create headless "$launcher_overflow_output" > /dev/null 2>&1
# A theme change reloads the compositor's config, which drops this rule.
place() { "$hyprctl_bin" eval "hl.monitor({ output = \\"$launcher_overflow_output\\", mode = \\"1280x720@60\\", position = \\"\${width}x0\\", scale = 1 })" > /dev/null 2>&1; sleep 3; }
place
"$hyprctl_bin" -j monitors > "$shot_dir/launcher-overflow-monitors.json" 2>&1

focus
call menu summon "" > /dev/null 2>&1
sleep 0.6
call menu filter "Overflow Probe" > /dev/null 2>&1
sleep 1.5
status long
shoot long
call menu close > /dev/null 2>&1
sleep 1

focus
call menu summon "" > /dev/null 2>&1
sleep 0.6
call menu filter overflow-trail-probe > /dev/null 2>&1
sleep 1.5
status trail
shoot trail
call menu close > /dev/null 2>&1
sleep 1

focus
call menu summon plugin:overflow-probe > /dev/null 2>&1
sleep 2
status mode
"$wtype_bin" -k Tab
sleep 1.5
status column
shoot column
"$wtype_bin" -k End
sleep 1.5
status end
shoot end
call menu close > /dev/null 2>&1
sleep 1

call theme mode light > /dev/null 2>&1
sleep 2
place
"$hyprctl_bin" -j monitors > "$shot_dir/launcher-overflow-monitors-light.json" 2>&1
focus
call menu summon plugin:overflow-probe > /dev/null 2>&1
sleep 2
"$wtype_bin" -k Tab
sleep 1.5
shoot column-light
call menu close > /dev/null 2>&1
sleep 1
"$hyprctl_bin" output remove "$launcher_overflow_output" > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

# `<step> <jq claim> <what it means>`, against that step's status.
launcher_overflow_expect() {
  local f="$shot_dir/launcher-overflow-$1.json"
  [ -s "$f" ] || fail "launcher-overflow: no status for $1"
  "$jq_bin" -e "$2" "$f" > /dev/null || fail "launcher-overflow $1: $3, got $("$jq_bin" -c '{isOpen, level, cardWidth, cardBox, rowBoxes: .rowBoxes[:3], actionsBox, actionCursor, actionsScroll, actionsScrollMax, actionsMore, n: (.actions | length)}' "$f")"
}

leg_launcher_overflow_assert() {
  local inside='def inside($c): .x >= $c.x and .y >= $c.y and .x + .width <= $c.x + $c.width and .y + .height <= $c.y + $c.height;'
  local m
  for m in monitors monitors-light; do
    "$jq_bin" -e --arg o "$launcher_overflow_output" '.[] | select(.name == $o and .width == 1280 and .height == 720)' "$shot_dir/launcher-overflow-$m.json" > /dev/null \
      || fail "launcher-overflow: no 1280x720 headless output ($m): $("$jq_bin" -c '[.[] | {name, width, height}]' "$shot_dir/launcher-overflow-$m.json")"
  done

  launcher_overflow_expect long '.isOpen and (.drawn[0].label | length) == 200' "the 200-character app does not lead its own search"
  launcher_overflow_expect long "$inside"' .cardBox as $c | (.rowBoxes | length) > 0 and all(.rowBoxes[]; inside($c)) and .cardBox.x >= 0 and .cardBox.x + .cardBox.width <= 1280' "a row's box is not inside the card's"
  echo "SMOKE_LAUNCHER_OVERFLOW a 200-character name elides inside the card"

  launcher_overflow_expect trail '.isOpen and (.ids[0] | startswith("files.")) and (.drawn[0].value | length) > 100' "the long-named recent file does not lead its own search"
  local box
  box=$("$jq_bin" -r '.rowBoxes[] | select(.index == 0) | "\(.x) \(.y) \(.width) \(.height)"' "$shot_dir/launcher-overflow-trail.json")
  [ -n "$box" ] || fail "launcher-overflow trail: no box for the first row"
  # shellcheck disable=SC2086  # the box is four numbers
  "$python3_bin" - "$shot_dir/launcher-overflow-trail.png" $box <<'PY' || fail "launcher-overflow: the row's trailing value is not held to 40% of it"
import sys
from PIL import Image
path, x, y, w, h = sys.argv[1], *map(int, sys.argv[2:])
img = Image.open(path).convert("RGB")
band = range(y + h // 3, y + 2 * h // 3)
bg = img.getpixel((x + w - 3, y + h // 2))
def ink(cx):
    return any(sum(abs(a - b) for a, b in zip(img.getpixel((cx, cy)), bg)) > 60 for cy in band)
cols = [cx for cx in range(x + 2, x + w - 2) if ink(cx)]
if not cols:
    sys.exit("no ink in the row")
right = cols[-1]
left = right
for cx in reversed(cols):
    if left - cx > 10:
        break
    left = cx
label = [cx for cx in cols if cx < left - 10]
value = right - left + 1
gap = left - label[-1] if label else -1
print(f"SMOKE_LAUNCHER_OVERFLOW_TRAIL row={w} value={value} share={value / w:.2f} label_end={label[-1] if label else -1} gap={gap}")
if value > 0.4 * w or value < 0.3 * w:
    sys.exit(f"the value's ink is {value}px of a {w}px row")
if not label or gap > 48:
    sys.exit(f"the name does not take the rest: gap {gap}")
PY
  echo "SMOKE_LAUNCHER_OVERFLOW a long trailing value elides at 40% of its row"

  launcher_overflow_expect mode '.level == "plugin:overflow-probe" and .ids == ["plugin:overflow-probe.many"]' "the plugin's mode did not list its one row"
  launcher_overflow_expect column '.actionsOpen and (.actions | length) == 41 and .actions[0] == "run" and .actions[-1] == "act-40"' "Tab did not list the row's activate and its 40 actions"
  launcher_overflow_expect column '.cardWidth == 840 and .cardBox.x >= 0 and .cardBox.x + .cardBox.width <= 1280 and .cardBox.width >= 840' "the 840 card does not fit the 1280 output"
  launcher_overflow_expect column "$inside"' .cardBox as $c | (.actionsBox | inside($c)) and all(.rowBoxes[]; inside($c))' "the column or a row reached past the card"
  launcher_overflow_expect column '.actionCursor == 0 and .actionsScroll == 0 and .actionsMore > 0' "the column's last visible row does not read n more"
  launcher_overflow_expect end '.actionCursor == 40 and .actionsScrollMax > 0 and .actionsScroll == .actionsScrollMax and .actionsMore == 0' "End did not scroll the column to its last action"
  echo "SMOKE_LAUNCHER_OVERFLOW 41 actions in the 840 card on 1280x720, n more, End scrolling to the last"

  local shot
  for shot in long trail column end column-light; do
    [ -s "$shot_dir/launcher-overflow-$shot.png" ] || fail "launcher-overflow: no $shot frame"
    echo "SMOKE_LAUNCHER_OVERFLOW_SHOT $shot_dir/launcher-overflow-$shot.png"
  done
}
