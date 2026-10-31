# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --panel <name> opens one per-widget popout over the real `panel` IPC route
# and leaves it open for the run's own frame. No bar-cell click happened, so
# the panel's anchorX stays unset and the frame falls back under the bar's
# right region. `panel state` is read back beside the route's own answer:
# the claim a screenshot cannot make is that the shell's own bookkeeping
# says this panel, and only this one, is the open one.
#
# The header read off one frame against `debug dump`'s rest rect: the gap
# from the card's top edge to the title's ink and the gap from its baseline
# to the hairline under it agree within 2px (owner, 2026-10-31: titles stuck
# to the top). The baseline is the lowest row of dense ink, so a descender
# does not count; a panel with no header (settings, the tray) skips it.
leg_panel_flag="--panel <name>"
leg_panel_needs="jq grim python3"
leg_panel_order=60

panel_open_path="$shot_dir/panel-open.txt"
panel_state_path="$shot_dir/panel-state.txt"
panel_dump_path="$shot_dir/panel-dump.json"
panel_header_shot="$shot_dir/panel-header.png"

leg_panel_validate() {
  if [ -z "$(leg_arg panel)" ]; then
    echo "usage: $0 --panel <name>" >&2
    exit 1
  fi
}

leg_panel_fixture() {
  # --panel systemupdate needs a real flake to read: the panel parses
  # flake.lock itself (no nix invocation), so pointing it at the fixture
  # flake renders the actual input rows instead of the honest NO FLAKE cell.
  # Only that leg gets the key, so every other run keeps proving the unset
  # default. The upstream probes are the one part that needs network; a VM
  # with none lands on NO NETWORK, which is an honest state and still shows
  # the rows.
  if [ "$(leg_arg panel)" = "systemupdate" ]; then
    settings_fragment ', "systemUpdate": {"flakeDir": "'"$PWD/dev/smoke.d/fixtures/flake"'"}'
  fi
}

leg_panel_drive() {
  local panel_name script="$shot_dir/panel-drive.sh"
  panel_name=$(leg_arg panel)
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 3
$ipc call panel open "$panel_name" > "$panel_open_path" 2>&1
sleep 2
$ipc call panel state > "$panel_state_path" 2>&1
$ipc call debug dump > "$panel_dump_path" 2>&1
"$grim_bin" "$panel_header_shot" > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_panel_assert() {
  local panel_name
  panel_name=$(leg_arg panel)
  if [ ! -s "$panel_open_path" ] || ! grep -q "^ok$" "$panel_open_path"; then
    fail "panel open $panel_name did not answer ok, got: $(cat "$panel_open_path" 2>/dev/null)"
  fi
  cat "$panel_open_path"
  if [ ! -s "$panel_state_path" ] || ! grep -q "^$panel_name$" "$panel_state_path"; then
    fail "panel state is not '$panel_name', got: $(cat "$panel_state_path" 2>/dev/null)"
  fi
  cat "$panel_state_path"
  panel_header_assert "$panel_name"
}

panel_header_assert() {
  local rest out
  rest=$("$jq_bin" -c '.panel.rest' "$panel_dump_path" 2>/dev/null)
  [ -n "$rest" ] && [ "$rest" != "null" ] || fail "panel: no rest rect in debug dump"
  [ -s "$panel_header_shot" ] || fail "panel: no header frame"
  if [ "$("$jq_bin" -r '.panel.header' "$panel_dump_path")" = false ]; then
    echo "panel: $1 draws no header, skipped"
    return
  fi
  out=$("$python3_bin" - "$panel_header_shot" "$rest" <<'PY'
import json, sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGB")
r = json.loads(sys.argv[2])
x0, y0, w = int(r["x"]), int(r["y"]), int(r["width"])
px = im.load()
fill = px[x0 + w // 2, y0 + 3]
def far(c, d):
    return sum(abs(a - b) for a, b in zip(c, fill)) > d
# The hairline: the first row under the top whose middle half is all off
# the fill by a little and level.
line = None
for y in range(y0 + 8, y0 + 120):
    row = [px[x, y] for x in range(x0 + w // 4, x0 + 3 * w // 4)]
    if all(far(c, 6) for c in row) and max(c[0] for c in row) - min(c[0] for c in row) < 12:
        line = y
        break
if line is None:
    print("none")
    sys.exit()
# The title's ink, left of the middle, above the hairline.
counts = []
for y in range(y0 + 1, line):
    counts.append((y, sum(1 for x in range(x0 + 12, x0 + w // 2) if far(px[x, y], 120))))
ink = [y for y, n in counts if n]
if not ink:
    print("noink")
    sys.exit()
peak = max(n for _, n in counts)
base = max(y for y, n in counts if n >= peak * 0.3)
print(ink[0] - y0, line - base - 1)
PY
) || fail "panel: header check crashed: $out"
  case $out in
    none) fail "panel: $1 has a header but no hairline was found under it" ;;
    noink) fail "panel: header hairline but no title ink" ;;
    *[!0-9\ -]*|'') fail "panel: header check printed '$out'" ;;
  esac
  local above below
  read -r above below <<< "$out"
  echo "panel header: ${above}px above the title, ${below}px from its baseline to the hairline"
  [ $((above - below)) -le 2 ] && [ $((below - above)) -le 2 ] \
    || fail "panel header: title off centre, ${above}px above against ${below}px below"
}
