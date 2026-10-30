# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --sidebar-join: the network panel and the launcher budding off the
# sidebar's inner edge (docs/superpowers/specs/2026-10-30-sidebar.md §3),
# over a flat grey wallpaper. Each is photographed at rest, then the
# desktop bare, then one open at a tenth speed (`debug motionScale 1000`)
# with a burst of frames armed before the call. The card is what, right
# of the column (`sidebar status`'s `width`), is darker than the grey under
# the launcher's full scrim or well lighter than the grey, and the bare
# frame does not draw there; so the leg reads dark and `--light` alike.
# Through the open the card comes out of that edge: its first frame is
# on it, some frame carries it still on it and between a sixth and five
# sixths of the way to its resting far side, and none carries it further
# off the edge than it rests. The panel rests within the bar's margin of
# the edge; the launcher travels on to its own place, 30% across. The sidebar's default transparent column
# draws no line, so there is no gap in one to read.
leg_sidebar_join_flag="--sidebar-join"
leg_sidebar_join_order=238
leg_sidebar_join_needs="jq convert python3"

sidebar_join_flat="$shot_dir/sidebar-join-flat.png"
sidebar_join_status="$shot_dir/sidebar-join-status.json"
sidebar_join_frames=12

leg_sidebar_join_validate() {
  if leg_on bar_position || leg_on frame || leg_on sidebar; then
    echo "usage: --sidebar-join sets the sidebar itself and cannot combine with --bar-position, --frame or --sidebar" >&2
    exit 1
  fi
}

leg_sidebar_join_fixture() {
  $convert_bin -size 1920x1080 xc:'#808080' "$sidebar_join_flat"
  settings_fragment ', "layout": "sidebar"'
}

leg_sidebar_join_timing() {
  leg_timing 30 70
}

# One target's drive: `open` and `close` are the ipc calls, `name` the
# frames' prefix. Frames every 150ms from the call, past the 1.5s a
# tenth-speed open takes.
sidebar_join_burst() {
  local open="$1" close="$2" name="$3" i
  cat <<EOF
$ipc call $open > /dev/null 2>&1
sleep 2
"$grim_bin" "$shot_dir/sidebar-join-$name-rest.png" > /dev/null 2>&1
$ipc call $close > /dev/null 2>&1
sleep 2
"$grim_bin" "$shot_dir/sidebar-join-$name-bare.png" > /dev/null 2>&1
$ipc call debug motionScale 1000 > /dev/null 2>&1
sleep 0.5
EOF
  for ((i = 0; i < sidebar_join_frames; i++)); do
    echo "( sleep $(printf '%d.%03d' $((i * 150 / 1000)) $((i * 150 % 1000))); \"$grim_bin\" \"$shot_dir/sidebar-join-$name-mid-$i.png\" > /dev/null 2>&1 ) &"
  done
  cat <<EOF
$ipc call $open > /dev/null 2>&1
wait
$ipc call debug motionScale 100 > /dev/null 2>&1
sleep 1
$ipc call $close > /dev/null 2>&1
sleep 1.5
EOF
}

leg_sidebar_join_drive() {
  local script="$shot_dir/sidebar-join-drive.sh"
  {
    cat <<EOF
#!/usr/bin/env bash
for _ in \$(seq 1 40); do
  $ipc call sidebar status 2>/dev/null | grep -q '"cards"' && break
  sleep 0.5
done
$ipc call wallpaper set "$sidebar_join_flat" > /dev/null 2>&1
sleep 3
$ipc call sidebar status > "$sidebar_join_status" 2>&1
EOF
    sidebar_join_burst "panel open network" "panel close" panel
    sidebar_join_burst "menu toggle" "menu toggle" menu
  } > "$script"
  chmod +x "$script"
  hypr_exec_once "bash $script"
}

# The card off one target's frames, then the claims, printed as one line
# of numbers and exiting non-zero with the reason on a miss.
sidebar_join_read() {
  "$python3_bin" - "$shot_dir" "$1" "$2" "$sidebar_join_frames" <<'PY'
import sys
from PIL import Image, ImageChops
d, name, edge, n = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
L = lambda p: Image.open(f"{d}/sidebar-join-{name}-{p}.png").convert("RGB")
bare = L("bare")
w, h = bare.size

# Darker than the launcher's full scrim over the grey (0.5 of 128), or
# well lighter than the grey: a dark or a light card's fill.
fill = lambda img: img.convert("L").point(lambda v: 255 if v < 50 or v > 170 else 0)
under = fill(bare)

def card(frame):
    return ImageChops.subtract(fill(frame), under).crop((edge, 0, w, h)).getbbox()

rest = card(L("rest"))
if not rest:
    sys.exit(f"{name}: nothing differs from the bare desktop at rest")
x0, y0, x1, y1 = rest
print(f"{name}: rest x={edge + x0}..{edge + x1} y={y0}..{y1}", end="")
if name == "panel" and x0 > 12:
    sys.exit(f"\n{name}: the resting card stands {x0}px off the column's edge, not against it")
seen = [b for b in (card(L(f"mid-{i}")) for i in range(n)) if b]
print(f" frames={[(b[0], b[2]) for b in seen]}", end="")
if not seen or seen[0][0] > 2:
    sys.exit(f"\n{name}: the card's first frame is not on the column's edge: {seen[:1]}")
if not any(b[0] <= 2 and x1 / 6 < b[2] < x1 * 5 / 6 for b in seen):
    sys.exit(f"\n{name}: no frame carries the card part way out of the edge (rest reaches {x1})")
if any(b[0] > x0 + 2 for b in seen):
    sys.exit(f"\n{name}: a frame carries the card further off the edge than it rests")
print()
PY
}

leg_sidebar_join_assert() {
  local name edge out
  [ -s "$sidebar_join_status" ] || fail "sidebar-join: no sidebar status"
  edge=$("$jq_bin" -r '.width' "$sidebar_join_status")
  [ "$edge" = 160 ] || fail "sidebar-join: the column is not 160 wide: $(cat "$sidebar_join_status")"
  for name in panel menu; do
    local tag i
    tag=$(echo "$name" | tr a-z A-Z)
    echo "SMOKE_SIDEBAR_JOIN_${tag}_REST $shot_dir/sidebar-join-$name-rest.png"
    for ((i = 0; i < sidebar_join_frames; i++)); do
      echo "SMOKE_SIDEBAR_JOIN_${tag}_MID_$i $shot_dir/sidebar-join-$name-mid-$i.png"
    done
    out=$(sidebar_join_read "$name" "$edge" 2>&1) || fail "sidebar-join: $out"
    echo "$out"
  done
}
