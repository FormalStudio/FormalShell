# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, iso_home, the *_bin paths and fail()
# --nix-run: a launcher row carrying `@ipc:nix.run:<attr>` (kept for a
# user's own menu.jsonc rows) run through the menu's Enter path, which hands
# `nix run nixpkgs#<attr>; read` to the console's one-off drop-down: the
# console's own argv with its app id swapped for `<appId>.run`. The row is a
# user menu.jsonc entry, so the leg needs no search index and no network.
#
# The claim is the window and its command line: a client of class
# `formalshell-console.run` maps, and the process under it carries the
# script. Whether the package then builds is nix's business, not the
# shell's; `read` holds the window either way. The quake console itself
# must not have been spawned by it.
leg_nix_run_flag="--nix-run"
leg_nix_run_order=95
leg_nix_run_needs="foot jq"

nix_run_activate_path="$shot_dir/nix-run-activate.txt"
nix_run_clients_path="$shot_dir/nix-run-clients.json"
nix_run_procs_path="$shot_dir/nix-run-procs.txt"
nix_run_png="$shot_dir/nix-run.png"

leg_nix_run_fixture() {
  settings_fragment ', "console": {"command": ["'"$foot_bin"'", "--app-id=formalshell-console", "--override=colors.background=1f6feb"], "appId": "formalshell-console", "share": 0.5}'
  mkdir -p "$iso_home/.config/formalshell"
  cat > "$iso_home/.config/formalshell/menu.jsonc" <<EOF
{
  "nixrunfixture": { "label": "Nix Run Fixture", "action": "@ipc:nix.run:hello" }
}
EOF
}

leg_nix_run_timing() {
  leg_timing 16 45
}

leg_nix_run_drive() {
  local script="$shot_dir/nix-run-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
sleep 5
$ipc call menu summon "" > /dev/null 2>&1
sleep 1
$ipc call menu filter "Nix Run Fixture" > /dev/null 2>&1
sleep 1
$ipc call menu activate 0 > "$nix_run_activate_path" 2>&1
sleep 5
"$hyprctl_bin" clients -j > "$nix_run_clients_path" 2>&1
pgrep -af 'nixpkgs#hello' > "$nix_run_procs_path" 2>&1
"$grim_bin" "$nix_run_png" > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

leg_nix_run_assert() {
  grep -q '^ok$' "$nix_run_activate_path" 2>/dev/null \
    || fail "menu activate on the fixture row did not answer ok: $(cat "$nix_run_activate_path" 2>/dev/null)"
  [ -s "$nix_run_clients_path" ] || fail "no hyprctl clients dump"
  "$jq_bin" -e 'map(.class) | index("formalshell-console.run")' "$nix_run_clients_path" > /dev/null \
    || fail "no formalshell-console.run window mapped: $("$jq_bin" -c 'map(.class)' "$nix_run_clients_path")"
  if "$jq_bin" -e 'map(.class) | index("formalshell-console")' "$nix_run_clients_path" > /dev/null; then
    fail "the one-off spawned the quake console itself"
  fi
  cat "$nix_run_procs_path"
  grep -q 'nix run nixpkgs#hello; read' "$nix_run_procs_path" \
    || fail "nothing runs 'nix run nixpkgs#hello; read': $(cat "$nix_run_procs_path")"
  [ -f "$nix_run_png" ] || fail "no nix-run screenshot"
  echo "SMOKE_NIX_RUN $nix_run_png"
}
