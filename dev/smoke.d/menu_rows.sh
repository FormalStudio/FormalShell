# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --menu-rows: typing in the launcher redraws the list for the new query on
# the next frame, with nothing in between. Six staged apps whose names
# share "no" re-rank and change the row count across the prefixes of
# "notebook"; the card grows and shrinks with them.
#
# First each prefix typed on real keys and left 1.5s to settle: its frame
# and its `menu status`. Then Escape clears the field and the whole word is
# typed at 80ms a key while grim takes frames back to back and `menu
# status` is read in a loop. Every frame of that burst has to equal one of
# the settled frames over the card's box (dev/settled-frames.py): a slide,
# a fade, a card height morph or a row left behind matches none. Every
# status read through it has rows that do not intersect and are cut by
# nothing but the body's own scroll viewport. The frame taken
# `menu_rows_after_ms` after the last key (a refresh and wtype's exit)
# equals the frame 1.5s later. Run at FS_SMOKE_SCALE=1.25 as well, the
# panel scale the owner saw rows clip each other at.
leg_menu_rows_flag="--menu-rows"
leg_menu_rows_order=127
leg_menu_rows_needs="convert wtype wlrctl jq python3 pil"
leg_menu_rows_claims="launcher"

menu_rows_word="notebook"
menu_rows_after_ms=50
menu_rows_dir="$iso_home/.local/share/applications"

leg_menu_rows_fixture() {
  local name i=0
  mkdir -p "$menu_rows_dir"
  for name in "Notes" "Nova Player" "Notebook Pro" "Nomad" "Snow Notes" "Annotate"; do
    printf '[Desktop Entry]\nType=Application\nName=%s\nExec=true\n' "$name" > "$menu_rows_dir/formalshell-rows-$i.desktop"
    i=$((i + 1))
  done
}

leg_menu_rows_timing() {
  leg_timing 110
}

leg_menu_rows_drive() {
  local script="$shot_dir/menu-rows-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
d="$shot_dir/menu-rows"
mkdir -p "\$d"
park 120 1000
call menu summon "" > /dev/null 2>&1
until_settled
"$grim_bin" "\$d/settled-0.png" > /dev/null 2>&1
call menu status > "\$d/settled-0.json" 2>&1
word="$menu_rows_word"
for i in \$(seq 1 \${#word}); do
  "$wtype_bin" "\${word:\$((i - 1)):1}"
  sleep 1.5
  "$grim_bin" "\$d/settled-\$i.png" > /dev/null 2>&1
  call menu status > "\$d/settled-\$i.json" 2>&1
done
"$wtype_bin" -k Escape
sleep 1.5
"$grim_bin" "\$d/cleared.png" > /dev/null 2>&1
call menu status > "\$d/cleared.json" 2>&1
( n=0; while [ ! -e "\$d/stop" ]; do n=\$((n + 1)); "$grim_bin" "\$d/burst-\$(printf %03d \$n).png" > /dev/null 2>&1; done ) &
grims=\$!
( n=0; while [ ! -e "\$d/stop" ]; do n=\$((n + 1)); call menu status > "\$d/burst-\$(printf %03d \$n).json" 2>&1; sleep 0.03; done ) &
reads=\$!
"$wtype_bin" -d 80 "\$word"
sleep $(awk -v ms="$menu_rows_after_ms" 'BEGIN { printf "%.3f", ms / 1000 }')
"$grim_bin" "\$d/after.png" > /dev/null 2>&1
touch "\$d/stop"
wait "\$grims" "\$reads"
sleep 1.5
"$grim_bin" "\$d/final.png" > /dev/null 2>&1
call menu status > "\$d/final.json" 2>&1
call menu close > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_menu_rows_assert() {
  local d="$shot_dir/menu-rows" i word="$menu_rows_word" settled=() burst=() out status=0
  for i in $(seq 0 ${#word}); do
    [ -s "$d/settled-$i.json" ] || fail "menu-rows: no status with \"${word:0:$i}\" typed"
    settled+=("$d/settled-$i.png")
  done
  "$jq_bin" -e --arg q "$word" '.query == $q' "$d/settled-${#word}.json" > /dev/null \
    || fail "menu-rows: the field does not read $word after typing it: $("$jq_bin" -c '{query, ids: .ids[:6]}' "$d/settled-${#word}.json")"
  "$jq_bin" -e '.query == ""' "$d/cleared.json" > /dev/null || fail "menu-rows: Escape did not clear the field"
  "$jq_bin" -e --arg q "$word" '.query == $q' "$d/final.json" > /dev/null \
    || fail "menu-rows: the burst typed $("$jq_bin" -r .query "$d/final.json"), not $word"
  local counts
  counts=$(for i in $(seq 0 ${#word}); do "$jq_bin" -r '.rowBoxes | length' "$d/settled-$i.json"; done | sort -u | wc -l)
  [ "$counts" -ge 3 ] || fail "menu-rows: the prefixes of $word drew only $counts row counts, the fixture no longer re-ranks"
  for i in $(seq 0 ${#word}); do
    "$jq_bin" -c --arg q "${word:0:$i}" '{q: $q, ids: [.ids[:6][] | sub("^apps\\.formalshell-rows-"; "")], rows: (.rowBoxes | length), card: .cardBox.height}' "$d/settled-$i.json"
  done
  # The card's box at its tallest across the settled prefixes, the field
  # included: the query's words differ per prefix like the rows do.
  "$jq_bin" -s '[.[].cardBox] | {x: (map(.x) | min), y: (map(.y) | min)} as $o
    | $o + {width: ((map(.x + .width) | max) - $o.x), height: ((map(.y + .height) | max) - $o.y)}' \
    "$d"/settled-*.json > "$d/box.json"
  # The root Escape left is the list before the burst's first key.
  settled+=("$d/cleared.png")
  burst=("$d"/burst-*.png)
  [ "${#burst[@]}" -ge 3 ] || fail "menu-rows: only ${#burst[@]} frames in the burst"
  out=$("$python3_bin" "$PWD/dev/settled-frames.py" frames "${FS_SMOKE_SCALE:-1}" "$d/box.json" "${settled[@]}" -- "${burst[@]}" 2>&1) || status=$?
  echo "$out"
  awk -v d="$d" '/^NONE/ && ++n <= 4 { print "SMOKE_MENU_ROWS_MISS_" n " " d "/" $2 }' <<< "$out"
  [ "$status" -eq 0 ] || fail "menu-rows: $(grep -c '^NONE' <<< "$out") of ${#burst[@]} frames while typing match no settled list (a transition, an overlap or a stale row)"
  status=0
  out=$("$python3_bin" "$PWD/dev/settled-frames.py" rows "$d"/burst-*.json "$d"/settled-*.json 2>&1) || status=$?
  echo "$out"
  [ "$status" -eq 0 ] || fail "menu-rows: rows intersect or are cut while typing"
  status=0
  out=$("$python3_bin" "$PWD/dev/settled-frames.py" frames "${FS_SMOKE_SCALE:-1}" "$d/box.json" "$d/final.png" -- "$d/after.png" 2>&1) || status=$?
  echo "$out"
  [ "$status" -eq 0 ] || fail "menu-rows: ${menu_rows_after_ms}ms after the last key the card is not yet the list it settles on"
  echo "SMOKE_MENU_ROWS $d/after.png (${#burst[@]} frames while typing, each a settled list)"
}
