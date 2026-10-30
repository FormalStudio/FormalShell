# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --launcher-abbrev: abbreviations learned from picks and set in
# settings.json, read off `menu status` (`ids`, `abbrev`) and `menu
# abbreviations`. Staged apps: Foxtrot, one labelled exactly `fx`, Zulu,
# and two probes no query below spells. `fx` first leads with the `fx` app
# (its exact label); Foxtrot picked once under `fx` still sits below it,
# picked twice it leads as `learned`. A record taught 60 days ago under
# `zu` does not lead Zulu's own match. `qq` is configured to one probe in
# settings.json and pinned to the other in state.json: the configured one
# leads and the pinned one is not listed; `pp`, pinned alone, leads with
# its probe. settings.json's sha256 is the same before and after: the
# shell only reads it. Each pick launches its app (`Exec=true`), which
# closes the launcher, so every step summons it again.
leg_launcher_abbrev_flag="--launcher-abbrev"
leg_launcher_abbrev_order=28
leg_launcher_abbrev_needs="jq"

launcher_abbrev_dir="$iso_home/.local/share/applications"
launcher_abbrev_settings="$iso_home/.config/formalshell/settings.json"
launcher_abbrev_sha_before="$shot_dir/launcher-abbrev-settings-before.sha"

launcher_abbrev_entry() {
  printf '[Desktop Entry]\nType=Application\nName=%s\nExec=true\n' "$2" > "$launcher_abbrev_dir/formalshell-abbrev-$1.desktop"
}

leg_launcher_abbrev_fixture() {
  settings_fragment ', "menu": {"abbreviations": {"qq": "app:formalshell-abbrev-config.desktop"}}'
  mkdir -p "$launcher_abbrev_dir"
  launcher_abbrev_entry foxtrot "Foxtrot"
  launcher_abbrev_entry fx "fx"
  launcher_abbrev_entry zulu "Zulu"
  launcher_abbrev_entry config "Config Probe"
  launcher_abbrev_entry pinned "Pinned Probe"
  mkdir -p "$iso_home/.local/state/formalshell"
  local then
  then=$(( $(date +%s) - 60 * 24 * 3600 ))000
  cat > "$iso_home/.local/state/formalshell/state.json" <<EOF
{"abbreviations":{"pinned":{"qq":"app:formalshell-abbrev-pinned","pp":"app:formalshell-abbrev-pinned"},"learned":{"zu":[{"id":"app:formalshell-abbrev-foxtrot","count":5,"lastMs":$then}]}}}
EOF
}

leg_launcher_abbrev_timing() {
  leg_timing 30 70
}

leg_launcher_abbrev_drive() {
  local script="$shot_dir/launcher-abbrev-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sha256sum "$launcher_abbrev_settings" | cut -d' ' -f1 > "$launcher_abbrev_sha_before"
step() {
  $ipc call menu summon "" > /dev/null 2>&1
  sleep 1
  $ipc call menu filter "\$1" > /dev/null 2>&1
  sleep 1
  $ipc call menu status > "$shot_dir/launcher-abbrev-\$2.json" 2>&1
}
pick_foxtrot() {
  local i
  i=\$("$jq_bin" '.ids | index("apps.formalshell-abbrev-foxtrot")' "$shot_dir/launcher-abbrev-\$1.json")
  $ipc call menu activate "\$i" > /dev/null 2>&1
  sleep 2
}
sleep 3
step fx fresh
pick_foxtrot fresh
step fx once
pick_foxtrot once
step fx twice
"$grim_bin" "$shot_dir/launcher-abbrev.png" > /dev/null 2>&1
step zu stale
step qq configured
step pp pinned
$ipc call menu abbreviations > "$shot_dir/launcher-abbrev-book.json" 2>&1
$ipc call menu close > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

launcher_abbrev_expect() {
  local step="$1" head="$2" abbrev="$3" f="$shot_dir/launcher-abbrev-$1.json" got
  [ -s "$f" ] || fail "launcher-abbrev: no status for $step"
  got=$("$jq_bin" -r '"\(.ids[0]) \(.abbrev)"' "$f")
  [ "$got" = "$head $abbrev" ] || fail "launcher-abbrev $step: expected $head led as $abbrev, got $got ($("$jq_bin" -c '.ids' "$f"))"
  echo "SMOKE_LAUNCHER_ABBREV $step: $got"
}

leg_launcher_abbrev_assert() {
  launcher_abbrev_expect fresh apps.formalshell-abbrev-fx null
  launcher_abbrev_expect once apps.formalshell-abbrev-fx null
  "$jq_bin" -e '.ids | index("apps.formalshell-abbrev-foxtrot") == 1' "$shot_dir/launcher-abbrev-once.json" > /dev/null \
    || fail "launcher-abbrev once: Foxtrot is not second: $("$jq_bin" -c '.ids' "$shot_dir/launcher-abbrev-once.json")"
  launcher_abbrev_expect twice apps.formalshell-abbrev-foxtrot learned
  launcher_abbrev_expect stale apps.formalshell-abbrev-zulu null
  launcher_abbrev_expect configured apps.formalshell-abbrev-config configured
  "$jq_bin" -e '.ids | index("apps.formalshell-abbrev-pinned") == null' "$shot_dir/launcher-abbrev-configured.json" > /dev/null \
    || fail "launcher-abbrev: a pin listed beside the configured abbreviation for the same query"
  launcher_abbrev_expect pinned apps.formalshell-abbrev-pinned pinned
  local book="$shot_dir/launcher-abbrev-book.json"
  "$jq_bin" -e '.learned.fx[] | select(.id == "app:formalshell-abbrev-foxtrot" and .count == 2)' "$book" > /dev/null \
    || fail "launcher-abbrev: menu abbreviations has no fx record of two picks: $(cat "$book")"
  "$jq_bin" -e '.configured.qq == "app:formalshell-abbrev-config.desktop"' "$book" > /dev/null \
    || fail "launcher-abbrev: menu abbreviations lost the configured one: $(cat "$book")"
  local before after
  before=$(cat "$launcher_abbrev_sha_before")
  after=$(sha256sum "$launcher_abbrev_settings" | cut -d' ' -f1)
  [ -n "$before" ] && [ "$before" = "$after" ] || fail "launcher-abbrev: settings.json changed ($before -> $after)"
  echo "SMOKE_LAUNCHER_ABBREV settings.json unchanged $after"
  echo "SMOKE_LAUNCHER_ABBREV_SHOT $shot_dir/launcher-abbrev.png"
}
