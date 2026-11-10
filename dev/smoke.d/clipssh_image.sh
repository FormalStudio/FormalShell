# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --clipssh-image: the two paths that send an image over ssh without anyone
# first putting it on the clipboard and then finding the Clipssh route.
# `clipssh.autoSendImages` sends every image the moment it lands in history,
# and Shift+Enter on a history row sends that row's file. Both resolve a host
# out of `clipssh.alias` rather than off a row under a cursor, which is the
# half `--clipssh` cannot reach: its rows carry their own alias.
#
# The shim stands in for the binary the same way `--clipssh`'s does, and adds
# the one claim no frame can make. clipssh reads the clipboard and nothing
# else, so on every invocation it records the sha256 of what the clipboard
# actually held next to the alias it was handed. A send that reached
# ClipsshService with the right name while the clipboard still held the last
# transfer's reply text (or nothing) looks exactly like a working one from
# the outside; the digest is what separates them.
#
# The two sends are sequential because ClipsshService takes one transfer at a
# time. The order is load-bearing too: the shim reproduces clipssh's last act
# of wl-copying the remote path back as TEXT, so by the time the row is
# activated the image is off the clipboard, and the second digest matching is
# what proves Shift+Enter put it back rather than riding what the auto-send
# left there. That text is also why the row is reached through `menu filter
# image` rather than by index: it lands in history as a row of its own, ahead
# of the image, and the image row is the only one whose label matches.
leg_clipssh_image_flag="--clipssh-image"
leg_clipssh_image_order=136
leg_clipssh_image_needs="wl-copy wl-paste convert jq"
leg_clipssh_image_claims="launcher=shared"

clipssh_image_shim_dir="$shot_dir/clipssh-image-shim"
clipssh_image_calls_path="$shot_dir/clipssh-image-calls.txt"
clipssh_image_fixture_path="$shot_dir/clipssh-image.png"
clipssh_image_digest_path="$shot_dir/clipssh-image-digest.txt"
clipssh_image_summon_reply_path="$shot_dir/clipssh-image-summon-reply.txt"
clipssh_image_filter_reply_path="$shot_dir/clipssh-image-filter-reply.txt"
clipssh_image_alt_reply_path="$shot_dir/clipssh-image-alt-reply.txt"
clipssh_image_route_png="$shot_dir/clipssh-image-route.png"
clipssh_image_remote_path="/tmp/clipboard-1755180000.png"
clipssh_image_paste_auto_path="$shot_dir/clipssh-image-paste-auto.txt"
clipssh_image_dump_auto_path="$shot_dir/clipssh-image-dump-auto.json"
clipssh_image_paste_alt_path="$shot_dir/clipssh-image-paste-alt.txt"
clipssh_image_room_path="$shot_dir/clipssh-image-room.json"
clipssh_image_list_path="$shot_dir/clipssh-image-list.json"
clipssh_image_dump_path="$shot_dir/clipssh-image-dump.json"

leg_clipssh_image_fixture() {
  # One alias saved and one named, so neither path has anything to ask: the
  # "ask" branch is the launcher drilling into the route, which is
  # `--clipssh`'s own timeline.
  mkdir -p "$iso_home/.clipssh"
  printf '%s\n' 'box=test@10.255.255.7' > "$iso_home/.clipssh/aliases"
  settings_pin '.clipssh.alias = "box" | .clipssh.autoSendImages = true'

  # A solid colour at a size nothing else in the run produces, so its digest
  # cannot collide with a screenshot or a thumbnail that happened to reach
  # the clipboard.
  $convert_bin -size 317x181 xc:'#2a6fae' "$clipssh_image_fixture_path"
  # The digest the shim's readings are checked against, taken here rather
  # than in the assert so it is the file that was actually copied.
  sha256sum "$clipssh_image_fixture_path" | cut -d ' ' -f1 > "$clipssh_image_digest_path"

  mkdir -p "$clipssh_image_shim_dir"
  # `<alias> <sha256 of the clipboard>` per invocation, and `none` where the
  # clipboard holds no image at all, which is the failure this leg exists to
  # catch. Then the real one's last act, the remote path wl-copied as text.
  # The second's upload stands in for an ssh with a second's sleep, so the
  # image Shift+Enter put back is in history while the send still runs and
  # the auto-send stays quiet about it, as it does over a real link.
  cat > "$clipssh_image_shim_dir/clipssh" <<EOF
#!/usr/bin/env bash
digest=\$(wl-paste --no-newline --type image/png 2>/dev/null | sha256sum | cut -d ' ' -f1)
if ! wl-paste --list-types 2>/dev/null | grep -q '^image/png$'; then digest=none; fi
printf '%s %s\n' "\${1:-}" "\$digest" >> "$clipssh_image_calls_path"
sleep 1
printf '%s' "$clipssh_image_remote_path" | wl-copy
printf '\033[0;32mUploaded: $clipssh_image_remote_path\033[0m\n'
EOF
  chmod +x "$clipssh_image_shim_dir/clipssh"
  # PATH is not in session_env, so this is what Hyprland and everything it
  # spawns inherit, the same route --clipssh's shim takes. wl-paste has to
  # stay reachable by name for the shim's own reading.
  export PATH="$clipssh_image_shim_dir:$(dirname "$wl_paste_bin"):$PATH"
}

leg_clipssh_image_timing() {
  leg_timing 70
}

leg_clipssh_image_drive() {
  local script="$shot_dir/clipssh-image-drive.sh"
  # The frame is taken inline rather than on a clock of its own, unlike
  # --clipssh's: the two states that leg photographs are six-second toasts,
  # while the launcher here sits open and unchanging between the filter and
  # the activation, and Shift+Enter closes it the moment it lands. A fixed
  # delay raced that close and photographed an empty desktop.
  write_script "$script" <<EOF
#!/usr/bin/env bash
# The auto-send's own trigger: nothing here calls clipssh, or the shell. The
# image landing in history is the whole event.
"$wl_copy_bin" --type image/png < "$clipssh_image_fixture_path"
sleep 6
"$wl_paste_bin" --no-newline > "$clipssh_image_paste_auto_path" 2>&1
$ipc call debug dump > "$clipssh_image_dump_auto_path" 2>&1
$ipc call menu summon clipboard > "$clipssh_image_summon_reply_path" 2>&1
until_settled
$ipc call menu filter image > "$clipssh_image_filter_reply_path" 2>&1
sleep 2
"$grim_bin" "$clipssh_image_route_png" > /dev/null 2>&1
$ipc call menu activateAlternate 0 > "$clipssh_image_alt_reply_path" 2>&1
sleep 4
"$wl_paste_bin" --no-newline > "$clipssh_image_paste_alt_path" 2>&1
$ipc call bar room > "$clipssh_image_room_path" 2>&1
$ipc call clipboard list > "$clipssh_image_list_path" 2>&1
$ipc call debug dump > "$clipssh_image_dump_path" 2>&1
sleep 1
EOF
  hypr_exec_once "bash $script"
}

leg_clipssh_image_assert() {
  local want lines
  want=$(cat "$clipssh_image_digest_path")

  if ! grep -q '^ok$' "$clipssh_image_summon_reply_path" 2>/dev/null; then
    fail "menu summon clipboard did not answer ok, got: $(cat "$clipssh_image_summon_reply_path" 2>/dev/null)"
  fi
  if ! grep -q '^ok$' "$clipssh_image_filter_reply_path" 2>/dev/null; then
    fail "menu filter image did not answer ok, got: $(cat "$clipssh_image_filter_reply_path" 2>/dev/null)"
  fi
  if ! grep -q '^ok$' "$clipssh_image_alt_reply_path" 2>/dev/null; then
    fail "menu activateAlternate on the image row did not answer ok, got: $(cat "$clipssh_image_alt_reply_path" 2>/dev/null)"
  fi

  if [ ! -s "$clipssh_image_calls_path" ]; then
    fail "clipssh was never invoked: neither clipssh.autoSendImages nor Shift+Enter reached ClipsshService"
  fi
  cat "$clipssh_image_calls_path"
  lines=$(wc -l < "$clipssh_image_calls_path")
  if [ "$lines" -ne 2 ]; then
    fail "clipssh ran $lines times, want 2 (the auto-send, then Shift+Enter): $(tr '\n' ' ' < "$clipssh_image_calls_path")"
  fi
  # Both invocations, same two claims: the alias came out of clipssh.alias
  # (no row carried it), and the clipboard held the fixture image when
  # clipssh read it.
  if [ "$(cat "$clipssh_image_calls_path")" != "box $want
box $want" ]; then
    fail "want two 'box $want' invocations, got: $(tr '\n' ' ' < "$clipssh_image_calls_path")"
  fi

  # After each send: the remote path on the clipboard as text, read in the
  # nested session, and one COPIED toast per send raised before anything
  # else copied (a send whose completion waited on the next clipboard
  # change would raise it only then).
  local p f copied
  for p in "$clipssh_image_paste_auto_path" "$clipssh_image_paste_alt_path"; do
    if [ "$(cat "$p" 2>/dev/null)" != "$clipssh_image_remote_path" ]; then
      fail "the clipboard after a send holds '$(cat "$p" 2>/dev/null)', want $clipssh_image_remote_path"
    fi
  done
  for f in "$clipssh_image_dump_auto_path:1" "$clipssh_image_dump_path:2"; do
    copied=$("$jq_bin" --arg b "$clipssh_image_remote_path is on the clipboard" \
      '[.toasts[] | select(.summary == "CLIPSSH COPIED" and .body == $b and .urgency == 1)] | length' "${f%:*}" 2>/dev/null)
    if [ "${copied:-0}" -ne "${f##*:}" ]; then
      fail "want ${f##*:} CLIPSSH COPIED toasts in ${f%:*}, got ${copied:-none}: $(cat "${f%:*}")"
    fi
  done
  if [ "$("$jq_bin" '[.[0].cells[] | select(.name == "indicators")] | length' "$clipssh_image_room_path" 2>/dev/null)" != 0 ]; then
    fail "the clipssh indicator is still up after the Shift+Enter send finished: $(cat "$clipssh_image_room_path")"
  fi
  # History after both sends: the path text on top, the image kept under it.
  if ! "$jq_bin" -e --arg p "$clipssh_image_remote_path" \
    '(.[0].kind == "text") and ((.[0].text // "") | startswith($p)) and ([.[] | select(.kind == "image")] | length) == 1' \
    "$clipssh_image_list_path" > /dev/null 2>&1; then
    fail "want the remote path on top of history and the image still in it: $(cat "$clipssh_image_list_path")"
  fi

  if [ ! -f "$clipssh_image_route_png" ]; then
    fail "no clipboard route screenshot produced at $clipssh_image_route_png"
  fi
  echo "SMOKE_CLIPSSH_IMAGE_ROUTE $clipssh_image_route_png"
}
