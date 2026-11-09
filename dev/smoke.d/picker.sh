# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --picker: the wallpaper picker (specs/2026-11-04-wallpaper-picker.md), the
# launcher's `wallpaper` route rather than a surface of its own.
#
# The fixture directory carries every case the pairing model reads (§3):
# root pictures, `Dark/` and `Light/`, a same-name pair with different
# bytes (`dusk`), an identical twin in both (`twin`, one picture for both
# modes), a `-dark`/`-light` pair across directories (`orb`), an `-l`/`-d`
# pair (`fold`), a dark-only and a light-only single (`night`, `noon`), and
# `-flexoki`/`-zenbones` siblings that stay two. FS_WALLPAPERS points at a
# fixture catalog of two generated pairs with credits, so Included holds
# them and the Sky; PICKER_INCLUDED=real leaves the result's own set there
# instead and reads Included as whatever it ships, every built-in carrying
# an author, a licence and a source.
#
# Read off `picker status`: the sections, the two Style previews (kind
# `mode`) showing the slots, every tile's kind, filing and title, nothing
# hidden. Read off the frame: the pair tile's light half
# above its diagonal and its dark half under it, a filed single's badge in
# its top-left corner and none on an unfiled one, and the current mark in
# the bottom-right corner of the tile on the desktop alone, and the Style
# previews carrying the two halves of the pair once it is picked.
#
# Then the picks: Enter on the pair writing both slots, the cursor opening
# on that tile at the next summon, Ctrl+Shift+Tab onto the Light preview
# and Return, then Right and Return on Dark, each flipping `theme status`'s
# mode with the card left open and the ring moving to it (under `--light`
# both refused, the heading saying "Set in settings.json"), a light-only and a dark-only pick each
# writing its slot and the mode (under `--light`, which pins theme.mode,
# the slot alone), `chooseFor` (the column's Use in light mode only)
# writing one slot and no mode, and the Sky. Ctrl+Tab and Ctrl+Shift+Tab
# step between the sections' first tiles, a query filters by title, the
# empty Your wallpapers state says so, and `picker select` lists the
# directory's files flat with no pairing. The cursor's walk on real keys
# runs under the stale audit.
#
# picker_done_path is this leg's own finish marker: the picker and the
# clipboard routes are the same launcher surface, so a run carrying both has
# the clipboard leg wait on this file rather than summoning over the grid.
leg_picker_flag="--picker"
leg_picker_env="PICKER_INCLUDED"
leg_picker_order=120
leg_picker_needs="convert jq wtype python3"
leg_picker_claims="launcher=shared picker.directory"

# This leg's own clock, the same rule menu_t0 and lock_t0 draw: the grid
# covers the whole output, so under --wallpaper it starts after that leg's
# last frame (t=14).
picker_t0() {
  cover_t0 4
}

picker_done_path="$shot_dir/picker-drive.done"
picker_dir="$iso_home/.local/share/formalshell/pictures"
picker_included_dir="$iso_home/.local/share/formalshell/included"
picker_state="$iso_home/.local/state/formalshell/state.json"
picker_grid_png="$shot_dir/picker-grid.png"
picker_grid_menu="$shot_dir/picker-grid-menu.json"
picker_grid_status="$shot_dir/picker-grid-status.json"
picker_next_status="$shot_dir/picker-next-section.json"
picker_prev_status="$shot_dir/picker-prev-section.json"
picker_actions_png="$shot_dir/picker-actions.png"
picker_single_actions="$shot_dir/picker-actions-single.json"
picker_pair_actions="$shot_dir/picker-actions-pair.json"
picker_before_enter="$shot_dir/picker-before-enter.json"
picker_pair_state="$shot_dir/picker-pair-state.json"
picker_current_png="$shot_dir/picker-current.png"
picker_current_menu="$shot_dir/picker-current-menu.json"
picker_current_status="$shot_dir/picker-current-status.json"
picker_current_theme="$shot_dir/picker-current-theme.json"
picker_style_at="$shot_dir/picker-style-at.json"
picker_style_light_png="$shot_dir/picker-style-light.png"
picker_style_light_menu="$shot_dir/picker-style-light-menu.json"
picker_style_light_status="$shot_dir/picker-style-light-status.json"
picker_style_light_theme="$shot_dir/picker-style-light-theme.json"
picker_style_dark_png="$shot_dir/picker-style-dark.png"
picker_style_dark_menu="$shot_dir/picker-style-dark-menu.json"
picker_style_dark_status="$shot_dir/picker-style-dark-status.json"
picker_style_dark_theme="$shot_dir/picker-style-dark-theme.json"
picker_light_state="$shot_dir/picker-light-only-state.json"
picker_light_theme="$shot_dir/picker-light-only-theme.json"
picker_dark_state="$shot_dir/picker-dark-only-state.json"
picker_dark_theme="$shot_dir/picker-dark-only-theme.json"
picker_for_reply="$shot_dir/picker-choose-for-reply.txt"
picker_for_state="$shot_dir/picker-choose-for-state.json"
picker_for_theme="$shot_dir/picker-choose-for-theme.json"
picker_sky_state="$shot_dir/picker-sky-state.json"
picker_sky_theme="$shot_dir/picker-sky-theme.json"
picker_sky_status="$shot_dir/picker-sky-status.json"
picker_sky_png="$shot_dir/picker-sky.png"
picker_query_menu="$shot_dir/picker-query-menu.json"
picker_nomatch_png="$shot_dir/picker-no-match.png"
picker_empty_png="$shot_dir/picker-empty.png"
picker_empty_status="$shot_dir/picker-empty-status.json"
picker_select_status="$shot_dir/picker-select-status.json"
picker_select_reply="$shot_dir/picker-select-reply.txt"
picker_selection="$shot_dir/picker-selection.txt"
picker_cache_listing="$shot_dir/picker-thumb-cache.txt"
picker_cached_status="$shot_dir/picker-cached-status.json"

leg_picker_fixture() {
  # Flat fields of known colour at 960x540, so a tile's halves, badge and
  # mark read off the frame against the colour each file carries.
  mkdir -p "$picker_dir/Dark" "$picker_dir/Light"
  local f c
  while read -r f c; do
    $convert_bin -size 960x540 "xc:#$c" "$picker_dir/$f"
  done <<'EOF'
root-a.png c0392b
root-b.png 27ae60
orb-light.png f5efe0
Dark/orb-dark.png 1b2a4a
fold-l.png e8dcc3
fold-d.png 24344f
wave-flexoki.png d35400
wave-zenbones.png 8e44ad
Dark/dusk.png 2f3f5c
Light/dusk.png fbf7ee
Dark/twin.png 16a085
Dark/night.png 7f8c8d
Light/noon.png 95a5a6
EOF
  cp "$picker_dir/Dark/twin.png" "$picker_dir/Light/twin.png"
  settings_pin ".picker.directory = \"$picker_dir\""
  [ "${PICKER_INCLUDED:-}" = real ] && return
  mkdir -p "$picker_included_dir"
  while read -r f c; do
    $convert_bin -size 960x540 "xc:#$c" "$picker_included_dir/$f"
  done <<'EOF'
meadow-l.png dfe8c8
meadow-d.png 20301c
harbour-l.png cfe0ee
harbour-d.png 102030
EOF
  cat > "$picker_included_dir/catalog.json" <<'EOF'
[
  {"id": "meadow", "title": "Meadow", "author": "Fixture One", "license": "CC0-1.0",
   "licenseUrl": "https://creativecommons.org/publicdomain/zero/1.0/",
   "source": "https://example.org/meadow", "origin": "https://example.org/meadow-origin",
   "light": "meadow-l.png", "dark": "meadow-d.png"},
  {"id": "harbour", "title": "Harbour", "author": "Fixture Two", "license": "CC0-1.0",
   "licenseUrl": "https://creativecommons.org/publicdomain/zero/1.0/",
   "source": "https://example.org/harbour", "origin": "https://example.org/harbour-origin",
   "light": "harbour-l.png", "dark": "harbour-d.png"}
]
EOF
  shell_env="${shell_env:+$shell_env }FS_WALLPAPERS=$picker_included_dir"
}

leg_picker_timing() {
  # Past the drive's last step, which leaves the grid up, so the run's own
  # frame is the grid.
  local t0
  t0=$(picker_t0)
  leg_timing $((66 + t0)) $((100 + t0))
}

leg_picker_drive() {
  local t0 script="$shot_dir/picker-drive.sh"
  t0=$(picker_t0)
  write_script "$script" <<EOF
#!/usr/bin/env bash
index() { "$jq_bin" -r --arg id "\$1" '.items | map(.id) | index(\$id)' "$picker_grid_status"; }
sleep $t0
$ipc call picker summon > /dev/null 2>&1
sleep 2.5
"$grim_bin" "$picker_grid_png" > /dev/null 2>&1
$ipc call menu status > "$picker_grid_menu" 2>&1
$ipc call picker status > "$picker_grid_status" 2>&1
$(stale_audit_start)
for key in Right Right Down Down Down Left Up Up Up Down; do "$wtype_bin" -k "\$key"; sleep 0.2; done
sleep 1.5
$(stale_audit_stop picker)
"$wtype_bin" -M ctrl -k Tab -m ctrl
sleep 1
$ipc call picker status > "$picker_next_status" 2>&1
"$wtype_bin" -M ctrl -M shift -k Tab -m shift -m ctrl
sleep 1
$ipc call picker status > "$picker_prev_status" 2>&1
$ipc call menu actions "\$(index "$picker_dir/Dark/night.png")" > /dev/null 2>&1
sleep 1.5
$ipc call menu status > "$picker_single_actions" 2>&1
"$grim_bin" "$picker_actions_png" > /dev/null 2>&1
$ipc call menu actions "\$(index "$picker_dir/Light/dusk.png")" > /dev/null 2>&1
sleep 1
$ipc call menu status > "$picker_pair_actions" 2>&1
$ipc call menu close > /dev/null 2>&1
sleep 0.5
$ipc call picker summon > /dev/null 2>&1
sleep 1.5
$ipc call picker status > "$picker_before_enter" 2>&1
"$wtype_bin" -k Return
sleep 3
cp "$picker_state" "$picker_pair_state" 2>/dev/null
$ipc call picker summon > /dev/null 2>&1
sleep 2.5
"$grim_bin" "$picker_current_png" > /dev/null 2>&1
$ipc call menu status > "$picker_current_menu" 2>&1
$ipc call picker status > "$picker_current_status" 2>&1
cp "$iso_home/.local/state/formalshell/theme.json" "$picker_current_theme" 2>/dev/null
"$wtype_bin" -M ctrl -M shift -k Tab -m shift -m ctrl
sleep 0.8
$ipc call picker status > "$picker_style_at" 2>&1
"$wtype_bin" -k Return
sleep 3
$ipc call theme status > "$picker_style_light_theme" 2>&1
$ipc call picker status > "$picker_style_light_status" 2>&1
$ipc call menu status > "$picker_style_light_menu" 2>&1
"$grim_bin" "$picker_style_light_png" > /dev/null 2>&1
cp "$iso_home/.local/state/formalshell/theme.json" "$shot_dir/picker-style-light-colours.json" 2>/dev/null
"$wtype_bin" -k Right
sleep 0.4
"$wtype_bin" -k Return
sleep 3
$ipc call theme status > "$picker_style_dark_theme" 2>&1
$ipc call picker status > "$picker_style_dark_status" 2>&1
$ipc call menu status > "$picker_style_dark_menu" 2>&1
"$grim_bin" "$picker_style_dark_png" > /dev/null 2>&1
cp "$iso_home/.local/state/formalshell/theme.json" "$shot_dir/picker-style-dark-colours.json" 2>/dev/null
$ipc call picker choose "$picker_dir/Light/noon.png" > /dev/null 2>&1
sleep 3
cp "$picker_state" "$picker_light_state" 2>/dev/null
$ipc call theme status > "$picker_light_theme" 2>&1
$ipc call picker summon > /dev/null 2>&1
sleep 1.5
$ipc call picker choose "$picker_dir/Dark/night.png" > /dev/null 2>&1
sleep 3
cp "$picker_state" "$picker_dark_state" 2>/dev/null
$ipc call theme status > "$picker_dark_theme" 2>&1
$ipc call picker summon > /dev/null 2>&1
sleep 1.5
$ipc call picker chooseFor "$picker_dir/root-a.png" light > "$picker_for_reply" 2>&1
sleep 2.5
cp "$picker_state" "$picker_for_state" 2>/dev/null
$ipc call theme status > "$picker_for_theme" 2>&1
$ipc call picker summon > /dev/null 2>&1
sleep 1.5
$ipc call picker choose sky > /dev/null 2>&1
sleep 3
cp "$picker_state" "$picker_sky_state" 2>/dev/null
$ipc call theme status > "$picker_sky_theme" 2>&1
$ipc call picker summon > /dev/null 2>&1
sleep 2
$ipc call picker status > "$picker_sky_status" 2>&1
"$grim_bin" "$picker_sky_png" > /dev/null 2>&1
$ipc call menu close > /dev/null 2>&1
sleep 0.5
$ipc call picker summon > /dev/null 2>&1
sleep 1.5
"$wtype_bin" fold
sleep 1.5
$ipc call menu status > "$picker_query_menu" 2>&1
"$wtype_bin" zz
sleep 1.5
"$grim_bin" "$picker_nomatch_png" > /dev/null 2>&1
$ipc call menu close > /dev/null 2>&1
sleep 0.5
mv "$picker_dir" "$picker_dir.away"
$ipc call picker summon > /dev/null 2>&1
sleep 2.5
$ipc call picker status > "$picker_empty_status" 2>&1
"$grim_bin" "$picker_empty_png" > /dev/null 2>&1
$ipc call menu close > /dev/null 2>&1
mv "$picker_dir.away" "$picker_dir"
sleep 0.5
$ipc call picker select "$picker_dir" tok-picker > "$picker_select_reply" 2>&1
sleep 2
$ipc call picker status > "$picker_select_status" 2>&1
$ipc call picker choose "$picker_dir/root-b.png" > /dev/null 2>&1
sleep 1
cat "$iso_home/.local/state/formalshell/picker-selection.txt" > "$picker_selection" 2>&1
$ipc call picker summon > /dev/null 2>&1
sleep 3
$ipc call picker status > "$picker_cached_status" 2>&1
ls -1 "$iso_home/.cache/formalshell/thumbnails" > "$picker_cache_listing" 2>&1
touch "$picker_done_path"
EOF
  hypr_exec_once "bash $script"
}

leg_picker_assert() {
  local f
  for f in "$picker_grid_png" "$picker_current_png" "$picker_style_light_png" "$picker_style_dark_png" "$picker_actions_png" "$picker_sky_png" "$picker_empty_png" "$picker_nomatch_png"; do
    [ -f "$f" ] || fail "picker: no frame at $f"
  done
  echo "SMOKE_PICKER_GRID $picker_grid_png"
  echo "SMOKE_PICKER_CURRENT $picker_current_png"
  echo "SMOKE_PICKER_STYLE_LIGHT $picker_style_light_png"
  echo "SMOKE_PICKER_STYLE_DARK $picker_style_dark_png"
  echo "SMOKE_PICKER_ACTIONS $picker_actions_png"
  echo "SMOKE_PICKER_SKY $picker_sky_png"
  echo "SMOKE_PICKER_EMPTY $picker_empty_png"
  echo "SMOKE_PICKER_NO_MATCH $picker_nomatch_png"
  stale_audit_assert picker
  "$jq_bin" -c '{sections, count, cursor, cursorId, included}' "$picker_grid_status"
  local real=${PICKER_INCLUDED:-}
  "$python3_bin" - "$picker_dir" "$real" "$picker_grid_status" <<'PY' || fail "picker: the listing is not read as the fixture's pairs and singles"
import json, sys
d, real, status = sys.argv[1], sys.argv[2], json.load(open(sys.argv[3]))
items = status["items"]
style = [i for i in items if i["section"] == "style"]
yours = [i for i in items if i["section"] == "yours"]
included = [i for i in items if i["section"] == "included"]
want = {
    "Dusk": ("pair", f"{d}/Light/dusk.png", f"{d}/Dark/dusk.png", None),
    "Fold": ("pair", f"{d}/fold-l.png", f"{d}/fold-d.png", None),
    "Night": ("single", None, f"{d}/Dark/night.png", "dark"),
    "Noon": ("single", f"{d}/Light/noon.png", None, "light"),
    "Orb": ("pair", f"{d}/orb-light.png", f"{d}/Dark/orb-dark.png", None),
    "Root a": ("single", f"{d}/root-a.png", f"{d}/root-a.png", None),
    "Root b": ("single", f"{d}/root-b.png", f"{d}/root-b.png", None),
    "Twin": ("single", f"{d}/Dark/twin.png", f"{d}/Dark/twin.png", None),
    "Wave flexoki": ("single", f"{d}/wave-flexoki.png", f"{d}/wave-flexoki.png", None),
    "Wave zenbones": ("single", f"{d}/wave-zenbones.png", f"{d}/wave-zenbones.png", None),
}
ok = True
cur = status["current"]
got = [(i["id"], i["kind"], i["title"], i["light"], i["dark"]) for i in style]
wanted = [("mode-light", "mode", "Light", cur["light"], None), ("mode-dark", "mode", "Dark", None, cur["dark"])]
if items[:2] != style or got != wanted:
    print(f"Style {got}, wanted {wanted} first")
    ok = False
got = [i["title"] for i in yours]
if got != list(want):
    print(f"Your wallpapers in order {got}, wanted {list(want)}")
    ok = False
for i in yours:
    w = want.get(i["title"])
    if w and (i["kind"], i["light"], i["dark"], i["filed"]) != w:
        print(f"{i['title']}: {(i['kind'], i['light'], i['dark'], i['filed'])}, wanted {w}")
        ok = False
if real != "real":
    titles = [(i["title"], i["kind"], i["author"], i["license"]) for i in included]
    wanted = [("Meadow", "pair", "Fixture One", "CC0-1.0"), ("Harbour", "pair", "Fixture Two", "CC0-1.0"), ("Sky", "sky", None, None)]
    if titles != wanted:
        print(f"Included {titles}, wanted {wanted}")
        ok = False
else:
    bare = [i["title"] for i in included[:-1] if not (i["author"] and i["license"] and i["source"])]
    if len(included) < 2 or bare:
        print(f"Included has no built-in, or ones without a credit: {bare}")
        ok = False
if not included or included[-1]["kind"] != "sky":
    print("Included does not end on the Sky")
    ok = False
sections = [(s["id"], s["title"], s["count"]) for s in status["sections"]]
if sections != [("style", "Style", 2), ("yours", "Your wallpapers", 10), ("included", "Included", len(included))]:
    print(f"sections {sections}")
    ok = False
if status["count"] != len(items) or status["cursor"] != 2:
    print(f"count {status['count']} cursor {status['cursor']}")
    ok = False
print("listing: " + ("ok" if ok else "WRONG"))
sys.exit(0 if ok else 1)
PY
  # The cursor opens past the Style previews; Ctrl+Tab from Your wallpapers
  # lands on Included's first tile, and Ctrl+Shift+Tab back on the first of
  # Your wallpapers.
  "$jq_bin" -e '.cursor == 12' "$picker_next_status" > /dev/null \
    || fail "Ctrl+Tab did not land on Included's first tile: $("$jq_bin" -c '{cursor, cursorId}' "$picker_next_status")"
  "$jq_bin" -e '.cursor == 2' "$picker_prev_status" > /dev/null \
    || fail "Ctrl+Shift+Tab did not land back on the first tile: $("$jq_bin" -c '{cursor, cursorId}' "$picker_prev_status")"
  echo "SMOKE_PICKER_SECTIONS Ctrl+Tab to 12, Ctrl+Shift+Tab to 2"
  "$jq_bin" -e '.actionsOpen and .actions == ["set", "light-only", "dark-only", "open-file", "copy-path"]' "$picker_single_actions" > /dev/null \
    || fail "a single's column is not Set, Use in light mode only, Use in dark mode only, Open file, Copy path: $("$jq_bin" -c '{actionsOpen, actions}' "$picker_single_actions")"
  "$jq_bin" -e '.actionsOpen and .actions == ["set", "open-file", "copy-path"]' "$picker_pair_actions" > /dev/null \
    || fail "a pair's column is not Set, Open file, Copy path: $("$jq_bin" -c '{actionsOpen, actions}' "$picker_pair_actions")"
  # Enter on the pair under the cursor: both slots.
  "$jq_bin" -e --arg id "$picker_dir/Light/dusk.png" '.items[.cursor].id == $id' "$picker_before_enter" > /dev/null \
    || fail "the cursor did not open on the first tile: $("$jq_bin" -c '{cursor, cursorId}' "$picker_before_enter")"
  "$jq_bin" -e --arg l "$picker_dir/Light/dusk.png" --arg d "$picker_dir/Dark/dusk.png" '.wallpaper == $l and .wallpaperDark == $d and .wallpaperColor == false' "$picker_pair_state" > /dev/null \
    || fail "Enter on the pair did not write both slots: $("$jq_bin" -c '{wallpaper, wallpaperDark, wallpaperColor}' "$picker_pair_state")"
  "$jq_bin" -e --arg id "$picker_dir/Light/dusk.png" '.cursorId == "wallpaper." + $id and ([.items[] | select(.current and .kind != "mode")] | map(.id)) == [$id]' "$picker_current_status" > /dev/null \
    || fail "the next summon did not open on the pair, marked current alone: $("$jq_bin" -c '{cursorId, current: [.items[] | select(.current and .kind != "mode") | .id]}' "$picker_current_status")"
  echo "SMOKE_PICKER_PAIR both slots written, the cursor opening on the pair"
  # The Style previews: Ctrl+Shift+Tab off the pair onto Light, Return on
  # it and on Dark running `theme mode`, the card left open and the ring
  # (the preview `current`) on the mode in effect.
  local style_key want_light=light want_dark=dark want_note=""
  style_key=$("$jq_bin" -r '.modeKey' "$picker_style_light_theme")
  if [ "$style_key" = light ] || [ "$style_key" = dark ]; then
    want_light=$style_key want_dark=$style_key want_note="Set in settings.json"
  fi
  "$jq_bin" -e '.items[.cursor].id == "mode-light"' "$picker_style_at" > /dev/null \
    || fail "Ctrl+Shift+Tab off the pair did not land on the Light preview: $("$jq_bin" -c '{cursor, cursorId}' "$picker_style_at")"
  local step
  for step in light dark; do
    local theme_f="$shot_dir/picker-style-$step-theme.json" status_f="$shot_dir/picker-style-$step-status.json" want=$want_light
    [ "$step" = dark ] && want=$want_dark
    "$jq_bin" -e --arg m "$want" '.mode == $m' "$theme_f" > /dev/null \
      || fail "Return on the $step preview left the mode at $("$jq_bin" -r .mode "$theme_f"), wanted $want"
    "$jq_bin" -e --arg m "$want" --arg n "$want_note" '.open and .items[.cursor].id == "mode-'"$step"'" and ([.items[] | select(.kind == "mode" and .current) | .id] == ["mode-" + $m]) and ([.sections[] | select(.id == "style") | .far] == [$n])' "$status_f" > /dev/null \
      || fail "after Return on the $step preview: $("$jq_bin" -c '{open, cursor, style: [.items[] | select(.kind == "mode") | {id, current}], far: [.sections[] | select(.id == "style") | .far]}' "$status_f")"
  done
  echo "SMOKE_PICKER_STYLE Return on Light then Dark: mode $want_light then $want_dark, the card open (theme.mode '${style_key}')"
  # A light-only then a dark-only pick: their slot, and the mode unless
  # settings.json pins it.
  local pinned
  pinned=$("$jq_bin" -r '.modeKey' "$picker_light_theme")
  local light_mode=light dark_mode=dark
  if [ "$pinned" = light ] || [ "$pinned" = dark ]; then light_mode=$pinned; dark_mode=$pinned; fi
  "$jq_bin" -e --arg l "$picker_dir/Light/noon.png" --arg d "$picker_dir/Dark/dusk.png" --arg m "$light_mode" '.wallpaper == $l and .wallpaperDark == $d and .mode == $m' "$picker_light_state" > /dev/null \
    || fail "a light-only pick did not write the light slot and mode $light_mode: $("$jq_bin" -c '{wallpaper, wallpaperDark, mode}' "$picker_light_state")"
  "$jq_bin" -e --arg l "$picker_dir/Light/noon.png" --arg d "$picker_dir/Dark/night.png" --arg m "$dark_mode" '.wallpaper == $l and .wallpaperDark == $d and .mode == $m' "$picker_dark_state" > /dev/null \
    || fail "a dark-only pick did not write the dark slot and mode $dark_mode: $("$jq_bin" -c '{wallpaper, wallpaperDark, mode}' "$picker_dark_state")"
  "$jq_bin" -e --arg d "$picker_dir/Dark/night.png" --arg m "$dark_mode" '.desktop == (if $m == "dark" then $d else .wallpaper end)' "$picker_dark_theme" > /dev/null \
    || fail "the desktop is not the mode's slot after the dark-only pick: $("$jq_bin" -c '{desktop, wallpaper, wallpaperDark, mode}' "$picker_dark_theme")"
  echo "SMOKE_PICKER_FILED light-only to mode $light_mode, dark-only to mode $dark_mode (theme.mode '${pinned}')"
  grep -qx ok "$picker_for_reply" || fail "picker chooseFor was refused: $(cat "$picker_for_reply")"
  "$jq_bin" -e --arg l "$picker_dir/root-a.png" --arg d "$picker_dir/Dark/night.png" --arg m "$dark_mode" '.wallpaper == $l and .wallpaperDark == $d and .mode == $m' "$picker_for_state" > /dev/null \
    || fail "Use in light mode only did not write the light slot alone: $("$jq_bin" -c '{wallpaper, wallpaperDark, mode}' "$picker_for_state")"
  echo "SMOKE_PICKER_CHOOSE_FOR the light slot alone, mode kept"
  "$jq_bin" -e '.wallpaperColor == "sky"' "$picker_sky_state" > /dev/null \
    || fail "the Sky pick is not saved as wallpaperColor sky: $("$jq_bin" -c .wallpaperColor "$picker_sky_state")"
  "$jq_bin" -e '.desktop | startswith("sky:")' "$picker_sky_theme" > /dev/null \
    || fail "choosing the Sky did not put the sky on the desktop: $("$jq_bin" -c .desktop "$picker_sky_theme")"
  "$jq_bin" -e '.items[.cursor].kind == "sky" and ([.items[] | select(.current and .kind != "mode") | .kind] == ["sky"])' "$picker_sky_status" > /dev/null \
    || fail "the Sky is not the current tile under the cursor: $("$jq_bin" -c '{cursor, current: [.items[] | select(.current) | .id]}' "$picker_sky_status")"
  echo "SMOKE_PICKER_SKY_PICK the sky on the desktop and current"
  "$jq_bin" -e --arg id "wallpaper.$picker_dir/fold-l.png" '.ids == [$id]' "$picker_query_menu" > /dev/null \
    || fail "the query fold did not list the Fold pair alone: $("$jq_bin" -c '{query, ids}' "$picker_query_menu")"
  "$jq_bin" -e '[.sections[] | select(.id == "yours") | .count] == [0] and .directory != ""' "$picker_empty_status" > /dev/null \
    || fail "a missing directory did not read as an empty Your wallpapers: $("$jq_bin" -c '{directory, sections}' "$picker_empty_status")"
  grep -qx ok "$picker_select_reply" || fail "picker select was refused: $(cat "$picker_select_reply")"
  "$jq_bin" -e '.mode == "select" and .count == 14 and (.sections | map(.id)) == ["files"] and ([.items[].kind] | unique) == ["single"] and ([.items[].filed] | unique) == [null]' "$picker_select_status" > /dev/null \
    || fail "a select request is not the 14 files flat: $("$jq_bin" -c '{mode, count, sections}' "$picker_select_status")"
  "$jq_bin" -e --arg v "$picker_dir/root-b.png" '.token == "tok-picker" and .value == $v' "$picker_selection" > /dev/null \
    || fail "picker-selection.txt did not answer tok-picker with the chosen path: $(cat "$picker_selection")"
  echo "SMOKE_PICKER_SELECT 14 files flat, tok-picker answered"
  # The `wide` thumbnails behind every tile: the 13 files of the directory
  # and, with the fixture catalog, its 4.
  local cached want_cached=13
  [ "$real" = real ] || want_cached=17
  cached=$(grep -c -- '-wide640\.jpg$' "$picker_cache_listing" || true)
  [ "$cached" -ge "$want_cached" ] || fail "expected $want_cached wide thumbnails, got $cached: $(cat "$picker_cache_listing")"
  if grep -q '\.part\.jpg$' "$picker_cache_listing"; then
    fail "the thumbnail warm left partial files behind: $(cat "$picker_cache_listing")"
  fi
  "$jq_bin" -e '.cachedThumbnails == ([.items[] | select(.kind != "sky" and .kind != "mode")] | length)' "$picker_cached_status" > /dev/null \
    || fail "not every tile is backed by cached thumbnails: $("$jq_bin" -c '{count, cachedThumbnails}' "$picker_cached_status")"
  # The frames: the pair's halves either side of its diagonal, the badges,
  # and the current mark on the pair alone.
  local primary
  primary=$("$jq_bin" -r '.primary // empty' "$picker_current_theme" 2>/dev/null)
  [ -n "$primary" ] || fail "no primary in theme.json after the pair pick"
  "$python3_bin" - "${out_scale:-1}" "$primary" "$picker_dir" \
    "$picker_grid_png" "$picker_grid_menu" "$picker_current_png" "$picker_current_menu" \
    "$picker_style_light_png" "$picker_style_light_menu" "$picker_style_dark_png" "$picker_style_dark_menu" \
    "$("$jq_bin" -r .mode "$picker_style_light_theme")" "$("$jq_bin" -r .mode "$picker_style_dark_theme")" \
    "$("$jq_bin" -r '.ring // empty' "$shot_dir/picker-style-light-colours.json" 2>/dev/null)" \
    "$("$jq_bin" -r '.ring // empty' "$shot_dir/picker-style-dark-colours.json" 2>/dev/null)" <<'PY' || fail "picker: a tile's picture, badge or mark is wrong"
import json, sys
from PIL import Image
scale, primary, d = float(sys.argv[1]), sys.argv[2], sys.argv[3]
hexrgb = lambda h: tuple(int(h.lstrip("#")[i:i + 2], 16) for i in (0, 2, 4))
far = lambda a, b: max(abs(x - y) for x, y in zip(a, b))
prim = hexrgb(primary)

def picture(png, menu, path):
    status = json.load(open(menu))
    index = status["ids"].index(f"wallpaper.{path}")
    box = next(b for b in status["rowBoxes"] if b["index"] == index)
    # The cell holds the cursor's ring an `xs` (4) outside the picture.
    w = box["width"] - 8
    h = round(w * 9 / 16)
    x0, y0 = round((box["x"] + 4) * scale), round((box["y"] + 4) * scale)
    return Image.open(png).convert("RGB").crop((x0, y0, x0 + round(w * scale), y0 + round(h * scale)))

def mean(im, fx, fy):
    px = im.load()
    cx, cy = round(im.width * fx), round(im.height * fy)
    pts = [px[x, y] for y in range(cy - 2, cy + 3) for x in range(cx - 2, cx + 3)]
    return tuple(sum(p[c] for p in pts) // len(pts) for c in range(3))

def corner(im, fixture, left, top):
    px = im.load()
    w, h = im.width // 3, im.height // 2
    xs = range(0, w) if left else range(im.width - w, im.width)
    ys = range(0, h) if top else range(im.height - h, im.height)
    # The picture's own round corner (radius lg, 12) shows what is under it.
    r = int(12 * scale) + 2
    cut = lambda x, y: (x < r if left else x >= im.width - r) and (y < r if top else y >= im.height - r)
    pts = [px[x, y] for y in ys for x in xs if not cut(x, y)]
    return sum(far(p, fixture) > 40 for p in pts), sum(far(p, prim) <= 24 for p in pts)

ok = True
area = scale * scale
def claim(what, good):
    global ok
    print(f"{what}: {'ok' if good else 'WRONG'}")
    ok = ok and good

for png, menu, frame in [(sys.argv[4], sys.argv[5], "grid"), (sys.argv[6], sys.argv[7], "after the pick")]:
    im = picture(png, menu, f"{d}/Light/dusk.png")
    light, dark = hexrgb("fbf7ee"), hexrgb("2f3f5c")
    for fx, fy, want, half in [(0.25, 0.25, light, "light"), (0.6, 0.3, light, "light"), (0.4, 0.7, dark, "dark"), (0.62, 0.72, dark, "dark")]:
        got = mean(im, fx, fy)
        claim(f"{frame}: the Dusk pair at ({fx}, {fy}) is its {half} half {got}", far(got, want) <= 24)

grid = (sys.argv[4], sys.argv[5])
for name, colour, badge in [("Dark/night.png", "7f8c8d", True), ("Light/noon.png", "95a5a6", True), ("root-a.png", "c0392b", False), ("Dark/twin.png", "16a085", False)]:
    marked, _ = corner(picture(*grid, f"{d}/{name}"), hexrgb(colour), True, True)
    claim(f"{name}: {marked} px off its colour in the top-left corner, {'a badge' if badge else 'no badge'} wanted", marked >= 150 * area if badge else marked < 10)

after = (sys.argv[6], sys.argv[7])
marked, near = corner(picture(*after, f"{d}/Light/dusk.png"), hexrgb("2f3f5c"), False, False)
claim(f"the picked pair: {marked} px off its dark half and {near} px of primary {primary} in its bottom-right corner", marked >= 100 * area and near >= 300 * area)
for name, colour in [("fold-l.png", "e8dcc3"), ("root-a.png", "c0392b")]:
    path = f"{d}/{name}"
    marked, _ = corner(picture(*after, path), hexrgb("24344f" if name == "fold-l.png" else colour), False, False)
    claim(f"{name}: {marked} px off its colour in the bottom-right corner, no mark wanted", marked < 10)

# The Style previews after the pair pick: the light half on Light, the dark
# half on Dark, and the theme's ring colour along the picture's top edge on
# the mode in effect alone.
light, dark = hexrgb("fbf7ee"), hexrgb("2f3f5c")
for tile, want in [("mode-light", light), ("mode-dark", dark)]:
    got = mean(picture(*after, tile), 0.5, 0.5)
    claim(f"the {tile} preview after the pair pick reads {got}", far(got, want) <= 24)

def ringed(im, ring):
    # Rows of ring colour along the middle of the top edge.
    px = im.load()
    xs = range(round(im.width * 0.3), round(im.width * 0.7))
    rows = range(0, int(5 * scale) + 1)
    return sum(1 for y in rows if sum(far(px[x, y], ring) <= 30 for x in xs) >= 0.9 * len(xs))

for png, menu, mode, ring in [(sys.argv[8], sys.argv[9], sys.argv[12], sys.argv[14]), (sys.argv[10], sys.argv[11], sys.argv[13], sys.argv[15])]:
    if not ring:
        claim(f"mode {mode}: theme.json carries a ring colour", False)
        continue
    for tile in ["mode-light", "mode-dark"]:
        rows = ringed(picture(png, menu, tile), hexrgb(ring))
        on = tile == f"mode-{mode}"
        claim(f"mode {mode}: {rows} rows of ring {ring} along {tile}'s top edge, {'the ring' if on else 'none'} wanted", rows >= 1 if on else rows == 0)
sys.exit(0 if ok else 1)
PY
  echo "SMOKE_PICKER_TILES the pair split on its diagonal, the badges and the current mark in their corners"
}
