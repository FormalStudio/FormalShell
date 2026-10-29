# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --no-morph pins `motion.morph` false in the settings fixture (a
# `settings_pin`) and rides any other leg: every card drops from its cell on
# its own cast and fades, the popover, instead of budding off the bar's
# line, under every table. `--no-morph --panel-emerge` is that drop read off
# its frames. Its own claim is the one the frames cannot make: `debug
# dump`'s `theme.morph` is false once the shell has the key.
leg_no_morph_flag="--no-morph"
leg_no_morph_order=8
leg_no_morph_needs="jq"
leg_no_morph_fixture_window=keep

no_morph_dump_path="$shot_dir/no-morph-dump.json"

leg_no_morph_fixture() {
  settings_pin '.motion.morph = false'
}

leg_no_morph_drive() {
  local script="$shot_dir/no-morph-drive.sh"
  write_script "$script" <<EOS
#!/usr/bin/env bash
for _ in \$(seq 1 40); do
  $ipc call debug dump > "$no_morph_dump_path" 2>&1
  grep -q '"morph":false' "$no_morph_dump_path" && break
  sleep 0.5
done
EOS
  hypr_exec_once "bash $script"
}

leg_no_morph_assert() {
  [ -s "$no_morph_dump_path" ] || fail "no debug dump produced for the no-morph check"
  "$jq_bin" -e '.theme.morph == false' "$no_morph_dump_path" > /dev/null 2>&1 \
    || fail "debug dump reports morph $("$jq_bin" -c '.theme.morph' "$no_morph_dump_path"), not false"
  echo "SMOKE_NO_MORPH morph=false"
}
