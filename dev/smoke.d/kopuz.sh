# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, iso_home, the *_bin paths and fail()
# --kopuz: services/kopuz.rs against dev/kopuz-stub.py, which plays both
# halves of Kopuz: `daemon` a kopuzd on the session's own
# `$XDG_RUNTIME_DIR/kopuz/kopuzd.sock` (Python's stdlib speaking the h2
# and protobuf crates/fs-kopuz sends by hand; the rig has no grpcio, and a
# stub in Python keeps it out of the release build), logging every call it
# takes as a JSON line, and `player` Kopuz's MPRIS player on
# `org.mpris.MediaPlayer2.kopuz` through Gio (mpv-mpris can only own
# `.mpv`), playing no audio. In one session:
#
#  1. the daemon's row matching the player's track: `media status`'s kopuz
#     block connected with the stub's key, `actions` the heart alone, and
#     `media lyrics` synced off the stub's GetLyrics with source `kopuz`
#     (a `.miss` seeded for the track keeps the web providers off the
#     network while the key is on its way);
#  2. `media dontRecommend` refused while the source does not take it,
#     then the daemon's source taking it (LibraryInvalidated{6}) and
#     `actions` gaining it;
#  3. paused, then `media like toggle` reaching the stub as exactly
#     SetFavorite{key, true}, the daemon's invalidation turning `favorite`
#     true and the heart's button filled, and dontRecommend refused while
#     the track is a favourite;
#  4. the daemon unfavouriting it on its own (another frontend): `favorite`
#     false and the panel back to the unliked frame;
#  5. `media dontRecommend` reaching the stub as DontRecommend{key} then
#     Next;
#  6. the daemon reporting another track than MPRIS's: no kopuz block,
#     never a heart for the wrong track, and back once it agrees;
#  7. the daemon SIGKILLed: no kopuz block and no heart (photographed),
#     and the player's next track resolving through the existing chain, a
#     sibling `.lrc`, with source `local`.
#
# The panel frames are read over `debug dump`'s rest rect with the player
# paused, so nothing but the heart moves between them: liked against
# unliked has to differ, and the daemon's unlike has to land back on the
# first frame. Every commit from the panel's open to that frame runs under
# the stale audit.
leg_kopuz_flag="--kopuz"
leg_kopuz_order=178
leg_kopuz_claims="kopuz media.player=shared panel=shared"
leg_kopuz_needs="python3 jq grim convert"

kopuz_dir="$shot_dir/kopuz"
kopuz_stub="$PWD/dev/kopuz-stub.py"
kopuz_log="$kopuz_dir/calls.jsonl"
kopuz_fifo="$kopuz_dir/control"
kopuz_lyrics="$kopuz_dir/lyrics.json"
kopuz_tracks="$kopuz_dir/tracks.json"
kopuz_sock_path_file="$kopuz_dir/socket-path"
kopuz_daemon_pid="$kopuz_dir/daemon.pid"
kopuz_player_pid="$kopuz_dir/player.pid"
kopuz_key="kopuz-smoke-track-1"
kopuz_title="Kopuz Smoke Track"
kopuz_artist="Kopuz Smoke Artist"
kopuz_album="Kopuz Smoke Album"
kopuz_local_title="Kopuz Local Track"

kopuz_status() { printf '%s/status-%s.json' "$kopuz_dir" "$1"; }
kopuz_png() { printf '%s/kopuz-%s.png' "$shot_dir" "$1"; }

leg_kopuz_timing() {
  leg_timing 150
}

leg_kopuz_fixture() {
  mkdir -p "$kopuz_dir"
  : > "$kopuz_log"
  cat > "$kopuz_lyrics" <<'JSON'
[
 {"start": 1000, "end": 4000, "text": "Kopuz holds the words", "chunks": [[1000, "Kopuz"], [1800, " holds"], [2600, " the"], [3200, " words"]]},
 {"start": 4000, "end": 8000, "text": "The daemon sings along", "chunks": [[4000, "The"], [4800, " daemon"], [6000, " sings"], [7000, " along"]]},
 {"start": 8000, "end": 12000, "text": "Hearts fill on the wire"},
 {"start": 12000, "end": 16000, "text": "Nothing comes from the web"},
 {"start": 16000, "end": 20000, "text": "One socket, one stream"},
 {"start": 20000, "end": 24000, "text": "And the shell keeps time"}
]
JSON
  # The second track's url names a file that is never played: only the
  # `.lrc` beside it is read.
  cat > "$kopuz_dir/local.lrc" <<'LRC'
[00:01.00]Read off the sibling file
[00:05.00]The chain still answers
[00:09.00]With the daemon gone
LRC
  cat > "$kopuz_tracks" <<JSON
[
 {"title": "$kopuz_title", "artist": "$kopuz_artist", "album": "$kopuz_album", "length": 120},
 {"title": "$kopuz_local_title", "artist": "$kopuz_artist", "album": "$kopuz_album", "length": 90, "url": "file://$kopuz_dir/local.flac"}
]
JSON
  local cache_dir="$iso_home/.cache/formalshell/lyrics"
  mkdir -p "$cache_dir"
  touch "$cache_dir/kopuz-smoke-artist-kopuz-smoke-track-kopuz-smoke-album-120.miss"
}

leg_kopuz_drive() {
  local script="$shot_dir/kopuz-drive.sh" kill_script="$shot_dir/kopuz-kill.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
dir="$kopuz_dir"
# kopuz's client::default_socket_path, as fs_kopuz::socket_path reads it.
sock="\${XDG_RUNTIME_DIR:-\${XDG_CACHE_HOME:-\$HOME/.cache}}/kopuz/kopuzd.sock"
echo "\$sock" > "$kopuz_sock_path_file"

shot() {
  sleep 0.6
  shoot "$shot_dir/kopuz-\$1.png"
}
control() {
  echo "\$1" > "$kopuz_fifo"
}

sleep 2
"$python3_bin" "$kopuz_stub" daemon --socket "\$sock" --control "$kopuz_fifo" --log "$kopuz_log" \
  --lyrics "$kopuz_lyrics" --key "$kopuz_key" --title "$kopuz_title" --artist "$kopuz_artist" \
  --album "$kopuz_album" --length 120 > "\$dir/daemon.log" 2>&1 &
echo \$! > "$kopuz_daemon_pid"
SECONDS=0
until [ -S "\$sock" ] && [ -p "$kopuz_fifo" ] || [ "\$SECONDS" -ge 5 ]; do sleep 0.2; done
"$python3_bin" "$kopuz_stub" player --tracks "$kopuz_tracks" --log "$kopuz_log" > "\$dir/player.log" 2>&1 &
echo \$! > "$kopuz_player_pid"

until_jq 15 "\$dir/status-connected.json" '.kopuz.connected' call media status
$(stale_audit_start)
$ipc call panel open media > "\$dir/panel-open.txt" 2>&1
until_jq 15 "\$dir/lyrics-kopuz.json" '.state == "synced"' call media lyrics
$ipc call media dontRecommend > "\$dir/reply-dont-recommend-refused.txt" 2>&1
control "dont-recommend on"
until_jq 5 "\$dir/status-dont-recommend.json" '.kopuz.actions | index("dontRecommend")' call media status

$ipc call media playPause > /dev/null 2>&1
until_jq 5 "\$dir/status-paused.json" '.isPlaying == false' call media status
sleep 1
$ipc call debug dump > "\$dir/dump.json" 2>&1
shot unliked

$ipc call media like toggle > "\$dir/reply-like.txt" 2>&1
until_jq 5 "\$dir/status-liked.json" '.kopuz.favorite' call media status
shot liked
$ipc call media dontRecommend > "\$dir/reply-dont-recommend-favorite.txt" 2>&1

control "favorite off"
until_jq 5 "\$dir/status-unliked-by-daemon.json" '.kopuz.connected and (.kopuz.favorite | not)' call media status
shot unliked-by-daemon
$(stale_audit_stop kopuz)
$ipc call media dontRecommend > "\$dir/reply-dont-recommend.txt" 2>&1
SECONDS=0
until grep -q '"call": "Next"' "$kopuz_log" || [ "\$SECONDS" -ge 5 ]; do sleep 0.2; done

control "row other"
until_jq 5 "\$dir/status-mismatch.json" '.kopuz.connected | not' call media status
control "row playing"
until_jq 5 "\$dir/status-matched-again.json" '.kopuz.connected' call media status

kill -9 "\$(cat "$kopuz_daemon_pid")"
until_jq 10 "\$dir/status-killed.json" '.kopuz.connected | not' call media status
shot killed
$ipc call media next > /dev/null 2>&1
until_jq 5 "\$dir/status-local-track.json" '.title == "$kopuz_local_title"' call media status
until_jq 10 "\$dir/lyrics-local.json" '.source == "local"' call media lyrics
shot local
echo done > "\$dir/drive-done"
EOF
  hypr_exec_once "bash $script"

  write_script "$kill_script" <<EOF
#!/usr/bin/env bash
for pidfile in "$kopuz_daemon_pid" "$kopuz_player_pid"; do
  [ -f "\$pidfile" ] && kill "\$(cat "\$pidfile")" 2>/dev/null
done
[ -s "$kopuz_sock_path_file" ] && rm -f "\$(cat "$kopuz_sock_path_file")"
rm -f "$kopuz_fifo"
true
EOF
  add_cleanup "bash $kill_script"
}

# The share of a panel crop's pixels two frames differ in.
kopuz_diff() {
  local w h n
  IFS='x+' read -r w h _ <<< "$3"
  n=$(diff_px "$1" "$2" 6% "$3")
  [ -z "$n" ] || awk -v n="$n" -v a="$((w * h))" 'BEGIN { print n / a }'
}

kopuz_calls() {
  "$jq_bin" -c "select($1)" "$kopuz_log" 2>/dev/null
}

leg_kopuz_assert() {
  local s crop name
  for name in unliked liked unliked-by-daemon killed local; do
    [ -s "$(kopuz_png "$name")" ] && echo "SMOKE_KOPUZ_$(tr 'a-z-' 'A-Z_' <<< "$name") $(kopuz_png "$name")"
  done
  [ -f "$kopuz_dir/drive-done" ] || fail "the kopuz drive never finished (daemon log: $(tail -3 "$kopuz_dir/daemon.log" 2>/dev/null))"
  echo "SMOKE_KOPUZ_CALLS $kopuz_log"

  s=$(kopuz_status connected)
  cat "$s"; echo
  "$jq_bin" -e --arg k "$kopuz_key" '.kopuz == {connected: true, key: $k, favorite: false, actions: ["like"]}' "$s" > /dev/null \
    || fail "media status never carried the stub's row as the kopuz block, got: $("$jq_bin" -c .kopuz "$s")"
  "$jq_bin" -e '.state == "synced" and .source == "kopuz" and (.lines | length) == 6 and .lines[0].text == "Kopuz holds the words"' "$kopuz_dir/lyrics-kopuz.json" > /dev/null \
    || fail "media lyrics did not come off the stub's GetLyrics, got: $("$jq_bin" -c '{state, source, n: (.lines | length)}' "$kopuz_dir/lyrics-kopuz.json")"
  [ -n "$(kopuz_calls '.call == "GetLyrics" and .key == "'"$kopuz_key"'"')" ] || fail "the stub never took GetLyrics for $kopuz_key"
  grep -qx 'error: source does not take dontRecommend' "$kopuz_dir/reply-dont-recommend-refused.txt" \
    || fail "media dontRecommend was not refused for a source without it, got: $(cat "$kopuz_dir/reply-dont-recommend-refused.txt")"

  grep -qx ok "$kopuz_dir/reply-like.txt" || fail "media like toggle answered: $(cat "$kopuz_dir/reply-like.txt")"
  [ "$(kopuz_calls '.call == "SetFavorite"')" = "{\"call\":\"SetFavorite\",\"favorite\":true,\"key\":\"$kopuz_key\"}" ] \
    || fail "the stub took SetFavorite as: $(kopuz_calls '.call == "SetFavorite"')"
  "$jq_bin" -e '.kopuz.favorite' "$(kopuz_status liked)" > /dev/null || fail "favorite never read true after the like"
  grep -qx 'error: track is a favorite' "$kopuz_dir/reply-dont-recommend-favorite.txt" \
    || fail "media dontRecommend on a favourite answered: $(cat "$kopuz_dir/reply-dont-recommend-favorite.txt")"
  "$jq_bin" -e '.kopuz.connected and (.kopuz.favorite | not)' "$(kopuz_status unliked-by-daemon)" > /dev/null \
    || fail "the daemon's own unlike never reached media status: $("$jq_bin" -c .kopuz "$(kopuz_status unliked-by-daemon)")"

  "$jq_bin" -e '.kopuz.actions == ["like", "dontRecommend"]' "$(kopuz_status dont-recommend)" > /dev/null \
    || fail "dontRecommend never joined the actions once the source took it: $("$jq_bin" -c .kopuz "$(kopuz_status dont-recommend)")"
  grep -qx ok "$kopuz_dir/reply-dont-recommend.txt" || fail "media dontRecommend answered: $(cat "$kopuz_dir/reply-dont-recommend.txt")"
  [ "$(kopuz_calls '.call == "DontRecommend" or .call == "Next"' | tr '\n' ' ')" = "{\"call\":\"DontRecommend\",\"key\":\"$kopuz_key\"} {\"call\":\"Next\"} " ] \
    || fail "the stub took dontRecommend as: $(kopuz_calls '.call == "DontRecommend" or .call == "Next"' | tr '\n' ' ')"

  "$jq_bin" -e '.kopuz.connected == false and .kopuz.key == "" and .kopuz.actions == []' "$(kopuz_status mismatch)" > /dev/null \
    || fail "a daemon row for another track still drove the kopuz block: $("$jq_bin" -c .kopuz "$(kopuz_status mismatch)")"
  "$jq_bin" -e '.kopuz.connected' "$(kopuz_status matched-again)" > /dev/null || fail "the kopuz block never came back once the rows agreed"

  "$jq_bin" -e '.kopuz.connected == false and .kopuz.favorite == false and .kopuz.actions == [] and .title == "'"$kopuz_title"'"' "$(kopuz_status killed)" > /dev/null \
    || fail "the kopuz block outlived the daemon: $("$jq_bin" -c '{title, kopuz}' "$(kopuz_status killed)")"
  "$jq_bin" -e '.state == "synced" and .source == "local" and .lines[0].text == "Read off the sibling file"' "$kopuz_dir/lyrics-local.json" > /dev/null \
    || fail "with the daemon gone the next track's lyrics did not come off its sibling .lrc: $("$jq_bin" -c '{state, source}' "$kopuz_dir/lyrics-local.json")"

  stale_audit_assert kopuz
  crop=$(scaled_jq -r "$geometry_jq"'.panel.rest | geometry' "$kopuz_dir/dump.json")
  [ -n "$crop" ] && [ "$crop" != null ] || fail "no media panel rest rect in the dump"
  local liked back
  for name in unliked liked unliked-by-daemon killed local; do
    [ -s "$(kopuz_png "$name")" ] || fail "no $name frame"
  done
  liked=$(kopuz_diff "$(kopuz_png unliked)" "$(kopuz_png liked)" "$crop")
  back=$(kopuz_diff "$(kopuz_png unliked)" "$(kopuz_png unliked-by-daemon)" "$crop")
  echo "SMOKE_KOPUZ_HEART crop=$crop liked=$liked back=$back"
  awk -v l="${liked:-0}" 'BEGIN { exit !(l > 0.0005) }' || fail "the panel did not change when the track became a favourite (diff $liked)"
  awk -v l="${liked:-0}" -v b="${back:-1}" 'BEGIN { exit !(b < l / 4) }' \
    || fail "the daemon's unlike did not repaint the heart back (unliked against unliked-by-daemon $back, liked $liked)"
}
