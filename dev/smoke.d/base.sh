# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --base: the run a bare `dev/smoke.sh` is. The rust shell over IPC:
# `debug`'s replies to a fixed set of calls into rs-ipc.txt for the
# assertions, then the spinner on and off, a panel and a scrim at full speed
# for the frame log, and the same at a tenth of the speed for frames read
# mid-flight and at rest. `debug dump` and `debug query` grow with the shell,
# so they are read as data: every dump key must be one of the keys
# crates/formalshell-rs/src/ipc/debug.rs builds or an owner's `report` in
# crates/formalshell-rs/src/wayland/owner.rs inserts, and a query answers with the
# launcher's rows. Every other reply is matched byte for byte.
leg_base_flag="--base"
leg_base_order=5
leg_base_needs="jq"
leg_base_compositors="hyprland shoji"
leg_base_claims="session"
leg_base_fixture_window=keep

rs_ipc_path="$shot_dir/rs-ipc.txt"

leg_base_timing() {
  leg_timing 52
}

leg_base_drive() {
  rs_drive="$shot_dir/rs-drive.sh"
  write_script "$rs_drive" <<EOF
#!/usr/bin/env bash
call() { $ipc call "\$@" >> "$shot_dir/rs-calls.log" 2>&1; }
reply() { echo "> \$*"; $ipc call "\$@" 2>&1; echo "exit \$?"; }
shot() { "$grim_bin" "$shot_dir/rs-\$1.png" >> "$shot_dir/grim.log" 2>&1; }
{
  reply debug dump
  reply debug join top 100 200
  reply debug dump
  reply debug joinClear
  reply debug join top 10
  reply debug motionScale 0
  reply debug query x
  reply debug nope
  reply nope dump
} > "$rs_ipc_path"
call debug r0Spinner true; sleep 3
call debug r0Spinner false; sleep 3
call debug r0Panel open; sleep 2
call debug r0Panel close; sleep 2
call debug r0Scrim true; sleep 2
call debug r0Scrim false; sleep 2
call debug motionScale 1000; call debug r0Cast true
call debug r0Panel open; sleep 0.8; shot open-early
sleep 0.8; shot open-mid
call debug r0Spinner true; sleep 7.4; shot rest
call debug r0Spinner false
call debug r0Scrim true; sleep 1.2; shot scrim-mid
call debug r0Scrim false; call debug r0Panel close; sleep 7
call debug motionScale 100
EOF
  wm_exec_once "bash $rs_drive"
}

leg_base_assert() {
  # `debug dump` and `debug query` grow with the shell, so they are read
  # as data: every dump key must be one of the contract's and the numbers
  # this rig pins must read back, and a query answers with the launcher's
  # rows. Every other reply is matched byte for byte.
  [ -s "$rs_ipc_path" ] || fail "the rust drive wrote no debug replies"
  need_bin jq
  dump_keys=$({
    awk '/let mut dump = json!\(\{/ { on = 1; next } on && /^    \}\);/ { on = 0 } on && match($0, /^        "[A-Za-z]+":/) { print substr($0, 10, RLENGTH - 11) }' crates/formalshell-rs/src/ipc/debug.rs
    sed -n 's/^    dump\["\([A-Za-z]*\)"\] = .*/\1/p' crates/formalshell-rs/src/ipc/debug.rs
    sed -n 's/^ *dump\.insert("\([A-Za-z]*\)"\.into(), .*/\1/p' crates/formalshell-rs/src/wayland/owner.rs
  } | "$jq_bin" -R . | "$jq_bin" -sc .)
  [ "$dump_keys" != "[]" ] || fail "no dump keys found in crates/formalshell-rs/src/ipc/debug.rs"
  dumps=$(awk 'prev=="> debug dump"{print} {prev=$0}' "$rs_ipc_path")
  [ "$(printf '%s\n' "$dumps" | wc -l)" -eq 2 ] || fail "expected two debug dump replies in $rs_ipc_path"
  check_dump() {
    printf '%s' "$1" | "$jq_bin" -e --argjson allowed "$dump_keys" --argjson join "$2" --arg compositor "$FS_COMPOSITOR" '
      (keys - $allowed) == []
      and (["compositor","configLoaded","join","bar","frame","theme","animating"] - keys) == []
      and .compositor == $compositor
      and (.animating | type) == "array" and all(.animating[]; type == "string")
      and (.bar | length) == 1
      and (.frame | keys) == ["enabled","thickness"]
      and .theme.borderWidth == 1 and .theme.barPosition == "top"
      and (.theme.edgeInset | keys) == ["bottom","left","right","top"]
      and (if $join == null then .join == null
           else (.join | del(.screen)) == $join and (.join.screen | type) == "string" end)' >/dev/null
  }
  first_dump=$(printf '%s\n' "$dumps" | sed -n 1p)
  second_dump=$(printf '%s\n' "$dumps" | sed -n 2p)
  check_dump "$first_dump" null || fail "debug dump (no join) carries a key outside the contract or a wrong value: $first_dump"
  reach=$(printf '%s' "$first_dump" | "$jq_bin" -r '.theme.radiusXl')
  check_dump "$second_dump" "{\"edge\":\"top\",\"x\":100,\"width\":200,\"reach\":$reach}" || fail "debug dump (joined) carries a key outside the contract or a wrong value: $second_dump"
  query_reply=$(awk 'prev=="> debug query x"{print} {prev=$0}' "$rs_ipc_path")
  printf '%s' "$query_reply" | "$jq_bin" -e 'type == "array" and all(.[]; (.id | type) == "string" and (.label | type) == "string" and (.kind | type) == "string")' >/dev/null \
    || fail "debug query x is not a list of launcher rows: $query_reply"
  diff -u - <(awk 'prev=="> debug dump" || prev=="> debug query x"{print "<checked>"; prev=$0; next} {print; prev=$0}' "$rs_ipc_path") <<EOF || fail "debug's replies over formalshell-ipc differ (diff above)"
> debug dump
<checked>
exit 0
> debug join top 100 200
ok
exit 0
> debug dump
<checked>
exit 0
> debug joinClear
ok
exit 0
> debug join top 10
Too few arguments provided (3 required but 2 were provided.)
Function definition: function join(edge: string, x: int, width: int): string
exit 0
> debug motionScale 0
error: percent must be 1..5000
exit 0
> debug query x
<checked>
exit 0
> debug nope
Function not found.
exit 0
> nope dump
Target not found.
exit 0
EOF
  echo "SMOKE_RS_IPC $rs_ipc_path"
}
