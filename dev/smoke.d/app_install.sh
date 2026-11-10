# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --app-install: an app installed and removed under the running shell the
# way fspec installs one, reaching the launcher's rows on its own. The
# shell's XDG_DATA_DIRS names a data directory that does not exist when it
# starts (/usr/local/share on a host where fspec's first app creates it);
# the run then makes it and links the app's desktop entry and its hicolor
# icon in from a store path, the icon theme there carrying no index.theme,
# as fspec leaves it. With the launcher open on a query for the app, `menu
# status` lists its row with its picture a second after the links land,
# and not a second after the entry's link goes; closed, the next summon
# reads the same.
leg_app_install_flag="--app-install"
leg_app_install_order=28
leg_app_install_claims="env.XDG_DATA_DIRS launcher=shared"
leg_app_install_needs="convert jq"

app_install_share="$iso_home/late/share"
app_install_store="$iso_home/store/0c1d2e3f-latecomer-1.0/share"
app_install_id="apps.formalshell-latecomer"
app_install_before_path="$shot_dir/app-install-status-before.json"
app_install_open_added_path="$shot_dir/app-install-status-open-added.json"
app_install_open_removed_path="$shot_dir/app-install-status-open-removed.json"
app_install_closed_added_path="$shot_dir/app-install-status-closed-added.json"
app_install_closed_removed_path="$shot_dir/app-install-status-closed-removed.json"
app_install_png="$shot_dir/app-install-open-added.png"

leg_app_install_fixture() {
  mkdir -p "$app_install_store/applications" "$app_install_store/icons/hicolor/48x48/apps"
  cat > "$app_install_store/applications/formalshell-latecomer.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Latecomer Probe
Exec=true
Icon=formalshell-latecomer
EOF
  $convert_bin -size 48x48 xc:'#2F9E6E' "$app_install_store/icons/hicolor/48x48/apps/formalshell-latecomer.png"
  rm -rf "$iso_home/late"
  shell_env="${shell_env:+$shell_env }XDG_DATA_DIRS=$iso_home/.local/share:$app_install_share"
}

leg_app_install_timing() {
  leg_timing 60
}

leg_app_install_drive() {
  local script="$shot_dir/app-install-drive.sh"
  local entry="$app_install_share/applications/formalshell-latecomer.desktop"
  local icon="$app_install_share/icons/hicolor/48x48/apps/formalshell-latecomer.png"
  local link_both="mkdir -p '$app_install_share/applications' '$app_install_share/icons/hicolor/48x48/apps'; ln -sfn '$app_install_store/applications/formalshell-latecomer.desktop' '$entry'; ln -sfn '$app_install_store/icons/hicolor/48x48/apps/formalshell-latecomer.png' '$icon'"
  write_script "$script" <<EOF
#!/usr/bin/env bash
$ipc call menu summon "" > /dev/null 2>&1
until_settled
$ipc call menu filter "latecomer" > /dev/null 2>&1
sleep 1
$ipc call menu status > "$app_install_before_path" 2>&1
$link_both
sleep 1
"$grim_bin" "$app_install_png" > /dev/null 2>&1
$ipc call menu status > "$app_install_open_added_path" 2>&1
rm -f '$entry'
sleep 1
$ipc call menu status > "$app_install_open_removed_path" 2>&1
$ipc call menu close > /dev/null 2>&1
sleep 1
$link_both
sleep 1
$ipc call menu summon "" > /dev/null 2>&1
$ipc call menu filter "latecomer" > /dev/null 2>&1
sleep 0.3
$ipc call menu status > "$app_install_closed_added_path" 2>&1
$ipc call menu close > /dev/null 2>&1
sleep 1
rm -f '$entry'
sleep 1
$ipc call menu summon "" > /dev/null 2>&1
$ipc call menu filter "latecomer" > /dev/null 2>&1
sleep 0.3
$ipc call menu status > "$app_install_closed_removed_path" 2>&1
$ipc call menu close > /dev/null 2>&1
EOF
  hypr_exec_once "bash $script"
}

# <status file> <what> <present|absent>
app_install_check() {
  local f="$1" what="$2" want="$3" has picture
  [ -s "$f" ] || fail "app-install $what: no menu status at $f"
  has=$("$jq_bin" --arg id "$app_install_id" '.ids | index($id) != null' "$f" 2>/dev/null)
  if [ "$want" = present ]; then
    [ "$has" = true ] || fail "app-install $what: no $app_install_id row: $("$jq_bin" -c '.ids' "$f")"
    picture=$("$jq_bin" --arg id "$app_install_id" '[.drawn[] | select(.id == $id) | .picture] | first' "$f")
    [ "$picture" = true ] || fail "app-install $what: the row draws no icon: $("$jq_bin" -c '.drawn' "$f")"
  else
    [ "$has" = false ] || fail "app-install $what: $app_install_id still listed: $("$jq_bin" -c '.ids' "$f")"
  fi
  echo "SMOKE_APP_INSTALL $what: $want"
}

leg_app_install_assert() {
  app_install_check "$app_install_before_path" "before" absent
  app_install_check "$app_install_open_added_path" "installed while open" present
  app_install_check "$app_install_open_removed_path" "removed while open" absent
  app_install_check "$app_install_closed_added_path" "installed while closed" present
  app_install_check "$app_install_closed_removed_path" "removed while closed" absent
  echo "SMOKE_APP_INSTALL_PNG $app_install_png"
}
