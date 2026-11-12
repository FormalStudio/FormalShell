# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --clipboard drives both directions: wl-copy seeds the ledger, wl-paste
# reads the system clipboard back to prove what the shell put there.
#
# The head is one copy, one settle, one read, and it is an assertion of its
# own: the ledger has to hold something copied before anything had read it.
# ClipboardService owns the two `wl-paste --watch` children and is built at
# startup, not by its first reader. It is
# cleared afterwards so the fixture sequence starts from the same empty
# state the count assertions expect. Polling the ledger here instead would
# construct the service itself and prove nothing.
#
# Two list dumps prove capture, newest-first order, and that re-copying an
# existing entry moves it to the front rather than duplicating it. Then the
# leg that matters: a sentinel is copied, the route is summoned, `menu
# filter` narrows it to one known row, `menu activate 0` (the rig's Enter
# stand-in) fires, and the clipboard is read back. It has to hold the ROW's
# entry, not the sentinel: row activation is in-process
# (`@ipc:clipboard.copy:<id>`), and a spawned `ipc call` would be a silent
# exit 127 on any install that does not put the binary on PATH.
#
# The image fixture is copied last so the cursor lands on it when the route
# is resummoned, which is what puts the split pane's framed image preview in
# this run's own screenshot.
#
# The burst rides in after the round trip: twenty one-character copies
# 20ms apart, the shape an app that rewrites the clipboard on every
# keystroke produces. Capture settles before it records
# (ClipboardService's own `_settleMs`), so the ledger has to gain exactly
# one row, holding the value the burst ended on. A second row means a
# spammer can turn the whole 300-entry ledger over again.
#
# A capture of copied markup rides in just before it. Row labels and the
# split pane both take provider data verbatim, and a Text left on AutoText
# parses anything the rich-text heuristic accepts, so the row would show
# bold "clipboard smoke markup" with its tags eaten and its font no longer
# the row's. The ledger dump asserts the markup survived capture; the frame
# is where the row is read back with its angle brackets intact.
#
# Before that, a file copy of the shape a GTK4 app makes of an image: a
# `text/uri-list` offer naming one jpeg, a space in its name so the percent
# escape is real. No image/* type is ever on the clipboard, so the row can
# only come from the uri-list watcher, and the stored file has to open with
# the png signature: the jpeg went through ffmpeg on the way in.
#
# Last, two emoji-only clips over one text clip carrying an emoji, and a
# second frame with the cursor on the newest: an emoji-only row and its
# preview draw the glyph at display size, the inline one stays body text.
leg_clipboard_flag="--clipboard"
leg_clipboard_order=130
leg_clipboard_compositors="hyprland shoji"
leg_clipboard_needs="wl-copy wl-paste convert"
leg_clipboard_claims="launcher=shared"

clip_list1_path="$shot_dir/clipboard-list1.json"
clip_list2_path="$shot_dir/clipboard-list2.json"
clip_list3_path="$shot_dir/clipboard-list3.json"
clip_copy_path="$shot_dir/clipboard-copy.txt"
clip_paste_path="$shot_dir/clipboard-paste.txt"
clip_activate_path="$shot_dir/clipboard-activate.txt"
clip_activate_paste_path="$shot_dir/clipboard-activate-paste.txt"
clip_image_fixture_path="$shot_dir/clip-image.png"
clip_route_png="$shot_dir/clipboard-route.png"
clip_thumb_cache_path="$shot_dir/clipboard-thumb-cache.txt"
clip_warmup_path="$shot_dir/clipboard-warmup.json"
clip_burst_path="$shot_dir/clipboard-burst.json"
clip_uri_fixture_path="$shot_dir/clip uri image.jpg"
clip_uri_list_path="$shot_dir/clipboard-uri.json"
clip_emoji_png="$shot_dir/clipboard-emoji.png"
clip_emoji_list_path="$shot_dir/clipboard-emoji.json"

leg_clipboard_fixture() {
  # The ledger's image entry: a small solid PNG copied last, so the route's
  # newest row is the image one and the split pane's screenshot shows the
  # framed preview rather than the text one.
  $convert_bin -size 320x180 xc:'#3fae2a' "$clip_image_fixture_path"
  $convert_bin -size 200x120 xc:'#ae2a3f' "$clip_uri_fixture_path"
}

leg_clipboard_timing() {
  # The head costs a fixed 4s and every frame after it stacks on top. The
  # end state (the route summoned over the image entry) is stable, so a
  # generous delay only ever lands on it.
  leg_timing 82
}

leg_clipboard_drive() {
  local script="$shot_dir/clipboard-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
"$wl_copy_bin" "clipboard smoke warmup"
sleep 3
$ipc call clipboard list > "$clip_warmup_path" 2>&1
$ipc call clipboard clear > /dev/null 2>&1
sleep 1
"$wl_copy_bin" "clipboard smoke one"
sleep 1
"$wl_copy_bin" "clipboard smoke two"
sleep 1
"$wl_copy_bin" "clipboard smoke three"
sleep 1
$ipc call clipboard list > "$clip_list1_path" 2>&1
sleep 1
"$wl_copy_bin" "clipboard smoke three"
sleep 1
$ipc call clipboard list > "$clip_list2_path" 2>&1
copy_id=\$("$jq_bin" -r '.[1].id // empty' "$clip_list2_path" 2>/dev/null)
$ipc call clipboard copy "\$copy_id" > "$clip_copy_path" 2>&1
sleep 1
"$wl_paste_bin" --no-newline > "$clip_paste_path" 2>&1
sleep 1
for c in a b c d e f g h i j k l m n o p q r s t; do
  "$wl_copy_bin" "burst-\$c"
  sleep 0.02
done
sleep 1
$ipc call clipboard list > "$clip_burst_path" 2>&1
"$wl_copy_bin" "clipboard smoke sentinel"
sleep 1
$ipc call menu summon clipboard > /dev/null 2>&1
until_settled
$ipc call menu filter "clipboard smoke one" > /dev/null 2>&1
sleep 1
$ipc call menu activate 0 > "$clip_activate_path" 2>&1
sleep 2
"$wl_paste_bin" --no-newline > "$clip_activate_paste_path" 2>&1
sleep 1
printf 'file://%s\r\n' "\$(printf '%s' "$clip_uri_fixture_path" | sed 's/ /%20/g')" | "$wl_copy_bin" --type text/uri-list
sleep 3
$ipc call clipboard list > "$clip_uri_list_path" 2>&1
"$wl_copy_bin" '<b>clipboard smoke markup</b>'
sleep 1
"$wl_copy_bin" --type image/png < "$clip_image_fixture_path"
sleep 2
$ipc call clipboard list > "$clip_list3_path" 2>&1
$ipc call menu summon clipboard > /dev/null 2>&1
# The image row's thumbnail is made off the UI thread.
for _ in \$(seq 1 20); do
  ls -1 "$iso_home/.cache/formalshell/thumbnails" 2>/dev/null | grep -q 'fit[0-9]*\.jpg\$' && break
  sleep 0.5
done
until_settled
"$grim_bin" "$clip_route_png" > /dev/null 2>&1
ls -1 "$iso_home/.cache/formalshell/thumbnails" > "$clip_thumb_cache_path" 2>&1
$ipc call menu close > /dev/null 2>&1
"$wl_copy_bin" 'clipboard smoke 🌹 inline'
sleep 1
"$wl_copy_bin" '❤️'
sleep 1
"$wl_copy_bin" '😂'
sleep 2
$ipc call clipboard list > "$clip_emoji_list_path" 2>&1
$ipc call menu summon clipboard > /dev/null 2>&1
until_settled
"$grim_bin" "$clip_emoji_png" > /dev/null 2>&1
EOF
  wm_exec_once "bash $script"
}

leg_clipboard_assert() {
  local f count1 count2
  for f in "$clip_list1_path" "$clip_list2_path" "$clip_list3_path"; do
    if [ ! -s "$f" ]; then
      fail "no clipboard list produced at $f"
    fi
  done
  # A copy made before anything read the ledger. It fails if the two
  # `wl-paste --watch` children only start when something first reads
  # ClipboardService, which a poll would paper over.
  cat "$clip_warmup_path"; echo
  expect_jq "$clip_warmup_path" 'any(.[]; (.text // "") | contains("clipboard smoke warmup"))' \
    "a copy made before anything read the ledger was never captured"
  cat "$clip_list1_path"; echo
  cat "$clip_list2_path"; echo
  expect_jq "$clip_list1_path" 'any(.[]; (.text // "") | contains("clipboard smoke three"))' \
    "the clipboard ledger never captured the fixture strings"
  # The count has to stay 3 across the re-copy: a fourth entry would mean
  # the dedup-to-front path inserted a duplicate instead of moving the
  # existing "clipboard smoke three" entry.
  count1=$("$jq_bin" -r 'length' "$clip_list1_path" 2>/dev/null)
  count2=$("$jq_bin" -r 'length' "$clip_list2_path" 2>/dev/null)
  if [ "$count1" != "3" ] || [ "$count2" != "3" ]; then
    fail "clipboard list item count drifted (before=$count1 after-recopy=$count2, want 3/3)"
  fi
  if ! grep -q '^ok$' "$clip_copy_path" 2>/dev/null; then
    fail "clipboard copy did not answer ok, got: $(cat "$clip_copy_path" 2>/dev/null)"
  fi
  # ThumbnailService's `fit` half. The ledger's image row draws a
  # prerendered letterboxed copy rather than decoding the capture itself,
  # and a warmed row and a fallback row paint the same pixels, so the frame
  # cannot tell them apart. The cache filename can: `fit` rather than the
  # picker's `cover`, which is the mode this route asks for.
  if [ ! -s "$clip_thumb_cache_path" ]; then
    fail "no thumbnail cache directory produced for the clipboard route"
  fi
  cat "$clip_thumb_cache_path"
  if ! grep -q 'fit[0-9]*\.jpg$' "$clip_thumb_cache_path"; then
    fail "the clipboard capture was never prerendered in fit mode, cache holds: $(cat "$clip_thumb_cache_path")"
  fi
  if grep -q '\.part\.jpg$' "$clip_thumb_cache_path"; then
    fail "the thumbnail warm left partial files behind: $(cat "$clip_thumb_cache_path")"
  fi
  # wl-paste --no-newline leaves this file without one of its own, so the
  # explicit echo keeps the next line from landing appended to it.
  cat "$clip_paste_path"; echo
  if ! grep -q 'clipboard smoke two' "$clip_paste_path"; then
    fail "the system clipboard did not flip to the copied entry, got: $(cat "$clip_paste_path")"
  fi
  # Row activation, the Enter stand-in. The sentinel copied right before it
  # is what makes this a real assertion rather than a tautology: the system
  # clipboard held "clipboard smoke sentinel" when the row was activated, so
  # reading "clipboard smoke one" back can only have come from the row's own
  # in-process action running.
  if ! grep -q '^ok$' "$clip_activate_path" 2>/dev/null; then
    fail "menu activate on the clipboard row did not answer ok, got: $(cat "$clip_activate_path" 2>/dev/null)"
  fi
  cat "$clip_activate_paste_path"; echo
  if ! grep -q 'clipboard smoke one' "$clip_activate_paste_path"; then
    fail "activating the clipboard row left the clipboard at $(cat "$clip_activate_paste_path"), not the row's own entry"
  fi
  # Twenty copies 20ms apart, one row. The value has to be the one the
  # burst ended on, so a settle that fires on the first capture instead of
  # the last is a failure too.
  if [ ! -s "$clip_burst_path" ]; then
    fail "no clipboard list produced after the burst"
  fi
  cat "$clip_burst_path"; echo
  burst_rows=$("$jq_bin" -r '[.[] | (.text // "") | scan("burst-[a-t]")] | length' "$clip_burst_path" 2>/dev/null)
  if [ "$burst_rows" != "1" ]; then
    fail "a 20-copy burst left $burst_rows ledger rows, want 1"
  fi
  expect_jq "$clip_burst_path" 'any(.[]; (.text // "") | contains("burst-t"))' \
    "the burst row is not the value the clipboard settled on"
  # The uri-list copy, dumped before any image/png was ever copied.
  cat "$clip_uri_list_path"; echo
  uri_store=$("$jq_bin" -r '[.[] | .path | strings] | first // empty' "$clip_uri_list_path" 2>/dev/null)
  if [ -z "$uri_store" ] || [ ! -s "$uri_store" ]; then
    fail "a text/uri-list copy of one jpeg never reached the ledger as an image: $(cat "$clip_uri_list_path" 2>/dev/null)"
  fi
  if [ "$(head -c 8 "$uri_store" | od -An -tx1 | tr -d ' \n')" != "89504e470d0a1a0a" ]; then
    fail "the uri-list capture at $uri_store is not a png"
  fi
  # The image entry is what the split pane's preview frame is read against.
  expect_jq "$clip_list3_path" 'any(.[]; .kind == "image")' \
    "the image fixture never reached the ledger" '[.[].kind]'
  cat "$clip_list3_path"; echo
  expect_jq "$clip_list3_path" 'any(.[]; (.text // "") | contains("<b>clipboard smoke markup</b>"))' \
    "the markup capture never reached the ledger"
  if [ ! -f "$clip_route_png" ]; then
    fail "no clipboard-route screenshot produced"
  fi
  echo "SMOKE_CLIPBOARD_ROUTE $clip_route_png"
  cat "$clip_emoji_list_path"; echo
  expect_jq "$clip_emoji_list_path" 'any(.[]; (.text // "") | contains("😂"))' \
    "the emoji-only capture never reached the ledger"
  if [ ! -f "$clip_emoji_png" ]; then
    fail "no clipboard-emoji screenshot produced"
  fi
  echo "SMOKE_CLIPBOARD_EMOJI $clip_emoji_png"
}
