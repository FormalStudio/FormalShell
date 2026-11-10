# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --grid-relaunch: the root grid after a launch that re-ranks it. Ten apps
# are staged, so two sit past the eight root cells; each is found by search
# and launched, which records the launch, rebuilds the tree as the launcher
# closes and moves that app to the head of the grid. `menu status`'s
# `cells` is read on every reopen: eight ids, none of them twice, the
# launched app first, and `drawn` agreeing with it: every cell's delegate
# holding its own model id and drawing a name. Last, a query widened to
# every staged app and narrowed to one pools the rest, the clipboard route
# takes the view's model away and the root takes it back. On Qt 6.11 that
# reused the pooled cells still holding the query's ids, in reverse, over a
# `cells` that read right: another app's name in a cell, or with a real
# app list, whose query results mostly sit outside the root's eight, no
# name and the placeholder icon.
leg_grid_relaunch_flag="--grid-relaunch"
leg_grid_relaunch_order=27
leg_grid_relaunch_claims="menu.appGrid=true launcher=shared"
leg_grid_relaunch_needs="jq"

grid_relaunch_dir="$iso_home/.local/share/applications"
grid_relaunch_open_path="$shot_dir/grid-relaunch-status-open.json"
grid_relaunch_first_path="$shot_dir/grid-relaunch-status-first.json"
grid_relaunch_second_path="$shot_dir/grid-relaunch-status-second.json"
grid_relaunch_first_png="$shot_dir/grid-relaunch-first.png"
grid_relaunch_second_png="$shot_dir/grid-relaunch-second.png"
grid_relaunch_rescan_closed_path="$shot_dir/grid-relaunch-status-rescan-closed.json"
grid_relaunch_rescan_open_path="$shot_dir/grid-relaunch-status-rescan-open.json"
grid_relaunch_rescan_png="$shot_dir/grid-relaunch-rescan.png"
grid_relaunch_return_path="$shot_dir/grid-relaunch-status-return.json"
grid_relaunch_return_png="$shot_dir/grid-relaunch-return.png"

grid_relaunch_entry() {
  local file="$1" name="$2"
  {
    echo "[Desktop Entry]"
    echo "Type=Application"
    echo "Name=$name"
    echo "Exec=true"
  } > "$grid_relaunch_dir/$file"
}

leg_grid_relaunch_fixture() {
  settings_pin '.menu.appGrid = true'
  mkdir -p "$grid_relaunch_dir"
  local letter
  for letter in alpha bravo charlie delta echo foxtrot golf hotel india juliet; do
    grid_relaunch_entry "formalshell-relaunch-$letter.desktop" "Relaunch $letter"
  done
  # Launch records the shell reads after its first tree build, so the
  # first reorder lands on a grid whose window has never mapped. A season
  # old (six half-lives), so a launch inside the run outranks every one of them.
  mkdir -p "$iso_home/.local/state/formalshell"
  local then
  then=$(( $(date +%s) - 90 * 24 * 3600 ))000
  cat > "$iso_home/.local/state/formalshell/state.json" <<EOF
{"appLaunches":[{"id":"formalshell-relaunch-hotel","count":9,"lastMs":$then},{"id":"formalshell-relaunch-bravo","count":7,"lastMs":$then},{"id":"formalshell-relaunch-foxtrot","count":5,"lastMs":$then},{"id":"formalshell-relaunch-delta","count":3,"lastMs":$then}]}
EOF
}

leg_grid_relaunch_timing() {
  leg_timing 84
}

leg_grid_relaunch_drive() {
  local script="$shot_dir/grid-relaunch-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
$ipc call menu summon "" > /dev/null 2>&1
# The launch records land after the shell's first tree build.
until_jq 10 "$grid_relaunch_open_path" '.ids[0] == "apps.formalshell-relaunch-hotel"' call menu status
until_settled
$ipc call menu status > "$grid_relaunch_open_path" 2>&1
$ipc call menu close > /dev/null 2>&1
sleep 1
$ipc call menu summon "" > /dev/null 2>&1
until_settled
$ipc call menu filter "relaunch india" > /dev/null 2>&1
sleep 1
$ipc call menu activate 0 > /dev/null 2>&1
sleep 2
$ipc call menu summon "" > /dev/null 2>&1
until_settled
"$grim_bin" "$grid_relaunch_first_png" > /dev/null 2>&1
$ipc call menu status > "$grid_relaunch_first_path" 2>&1
$ipc call menu filter "relaunch juliet" > /dev/null 2>&1
sleep 1
$ipc call menu activate 0 > /dev/null 2>&1
sleep 2
$ipc call menu summon "" > /dev/null 2>&1
until_settled
"$grim_bin" "$grid_relaunch_second_png" > /dev/null 2>&1
$ipc call menu status > "$grid_relaunch_second_path" 2>&1
$ipc call menu close > /dev/null 2>&1
sleep 1
printf '[Desktop Entry]\nType=Application\nName=Relaunch kilo\nExec=true\n' > "$grid_relaunch_dir/formalshell-relaunch-kilo.desktop"
rm -f "$grid_relaunch_dir/formalshell-relaunch-alpha.desktop"
sleep 3
$ipc call menu summon "" > /dev/null 2>&1
until_settled
$ipc call menu status > "$grid_relaunch_rescan_closed_path" 2>&1
printf '[Desktop Entry]\nType=Application\nName=Relaunch lima\nExec=true\n' > "$grid_relaunch_dir/formalshell-relaunch-lima.desktop"
rm -f "$grid_relaunch_dir/formalshell-relaunch-hotel.desktop"
sleep 3
"$grim_bin" "$grid_relaunch_rescan_png" > /dev/null 2>&1
$ipc call menu status > "$grid_relaunch_rescan_open_path" 2>&1
$ipc call menu filter "relaunch" > /dev/null 2>&1
sleep 1
$ipc call menu filter "relaunch golf" > /dev/null 2>&1
sleep 1
$ipc call menu close > /dev/null 2>&1
sleep 1
$ipc call menu summon clipboard > /dev/null 2>&1
until_settled
$ipc call menu close > /dev/null 2>&1
sleep 1
$ipc call menu summon "" > /dev/null 2>&1
until_settled
"$grim_bin" "$grid_relaunch_return_png" > /dev/null 2>&1
$ipc call menu status > "$grid_relaunch_return_path" 2>&1
EOF
  hypr_exec_once "bash $script"
}

grid_relaunch_check() {
  local f="$1" what="$2" head="$3" cells count unique drawn
  [ -s "$f" ] || fail "no grid-relaunch status at $f"
  cells=$("$jq_bin" -c '.cells' "$f" 2>/dev/null)
  count=$("$jq_bin" '.cells | length' "$f")
  unique=$("$jq_bin" '.cells | unique | length' "$f")
  if [ "$count" != "8" ] || [ "$unique" != "8" ]; then
    fail "$what: the grid holds $count cells, $unique distinct: $cells"
  fi
  if [ -n "$head" ] && [ "$("$jq_bin" -r '.cells[0]' "$f")" != "$head" ]; then
    fail "$what: expected $head first, got $cells"
  fi
  drawn=$("$jq_bin" -c '[.drawn[] | if . == null then null else .id end]' "$f")
  if [ "$drawn" != "$cells" ]; then
    fail "$what: the cells draw $drawn over a model holding $cells"
  fi
  if [ "$("$jq_bin" '[.drawn[] | select(. == null or .label == "")] | length' "$f")" != "0" ]; then
    fail "$what: a cell draws no name: $("$jq_bin" -c '.drawn' "$f")"
  fi
  echo "SMOKE_GRID_RELAUNCH $what: $cells"
}

leg_grid_relaunch_assert() {
  cat "$grid_relaunch_open_path"; echo
  grid_relaunch_check "$grid_relaunch_open_path" "fresh" "apps.formalshell-relaunch-hotel"
  grid_relaunch_check "$grid_relaunch_first_path" "after india" "apps.formalshell-relaunch-india"
  grid_relaunch_check "$grid_relaunch_second_path" "after juliet" "apps.formalshell-relaunch-juliet"
  grid_relaunch_check "$grid_relaunch_rescan_closed_path" "rescan while closed" "apps.formalshell-relaunch-juliet"
  grid_relaunch_check "$grid_relaunch_rescan_open_path" "rescan while open" "apps.formalshell-relaunch-juliet"
  grid_relaunch_check "$grid_relaunch_return_path" "back from the clipboard" "apps.formalshell-relaunch-juliet"
  echo "SMOKE_GRID_RELAUNCH_PNG $grid_relaunch_rescan_png"
  echo "SMOKE_GRID_RELAUNCH_RETURN_PNG $grid_relaunch_return_png"
}
