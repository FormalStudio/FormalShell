# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, iso_home, the *_bin paths and fail()
# --firefox-install: Firefox installed from the launcher for real, the way
# Pond does it, under `layout: sidebar`. The guest's fspec runs a real
# `fspec app install zypper:MozillaFirefox --detach` against a checkout of
# the owner's public spec (FormalSnake/dotfiles) cloned from a bare copy in
# the guest, so the commit is pushed there and never to GitHub, and the
# guest's own git has no identity, so the commit is fspec's own. The apply
# runs zypper over the network; the overlay drops all of it at the end.
#
# Real keys: Install mode summoned, "firefox" typed, Down to the zypper row
# and Return. Read off `menu status`, `activity status` and `sidebar
# status`: the install's group running with the card's line under the
# notifications card (photographed mid-install), then succeeded, the
# commit in the bare upstream authored `fspec <fspec@fstestvm<n>>`, and a
# root search for "firefox" heading with Firefox's own app row
# (photographed with the done card).
leg_firefox_install_flag="--firefox-install"
leg_firefox_install_order=239
leg_firefox_install_vm_only="a real zypper install through the guest's fspec, its sudo and its network"
leg_firefox_install_needs="jq wtype git"

fi_dir="$shot_dir/firefox-install"
fi_pkg="zypper:MozillaFirefox"
fi_row="install.pkg.zypper%3AMozillaFirefox"

leg_firefox_install_validate() {
  if leg_on bar_position || leg_on frame; then
    echo "usage: --firefox-install places the sidebar itself and cannot combine with --bar-position or --frame" >&2
    exit 1
  fi
}

leg_firefox_install_fixture() {
  leg_on sidebar || settings_fragment ', "layout": "sidebar"'
  mkdir -p "$fi_dir"
  rm -rf "$fi_dir/upstream.git" "$fi_dir/spec"
  git clone -q --bare https://github.com/FormalSnake/dotfiles "$fi_dir/upstream.git" \
    || fail "--firefox-install: could not clone the spec from GitHub"
  git clone -q "$fi_dir/upstream.git" "$fi_dir/spec"
  export FSPEC_SPEC="$fi_dir/spec"
  # zypper puts Firefox's desktop entry under /usr/share, which the
  # isolated session's XDG_DATA_DIRS leaves out.
  session_data_dirs="/usr/local/share:/usr/share"
}

leg_firefox_install_timing() {
  leg_timing 30 900 0 840
}

leg_firefox_install_drive() {
  local script="$shot_dir/firefox-install-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
call() { $ipc call "\$@"; }
status() { call menu status > "$fi_dir/\$1.json" 2>&1; }
acts() { call activity status > "$fi_dir/activity-\$1.json" 2>&1; }
shoot() { "$grim_bin" "$fi_dir/\$1.png" > /dev/null 2>&1; }
until_status() {
  SECONDS=0
  while [ "\$SECONDS" -lt "\$3" ]; do
    status "\$1"
    "$jq_bin" -e "\$2" "$fi_dir/\$1.json" > /dev/null 2>&1 && return 0
    sleep 0.5
  done
}
group='[.groups[] | select(.source == "fspec:install:$fi_pkg")] | first'
sleep 4
call menu summon install > /dev/null 2>&1
sleep 1
"$wtype_bin" firefox
until_status rows '.ids | index("$fi_row") != null' 60
shoot rows
n=\$("$jq_bin" '.ids | index("$fi_row")' "$fi_dir/rows.json")
for _ in \$(seq 1 "\$n"); do "$wtype_bin" -k Down; sleep 0.1; done
status picked
"$wtype_bin" -k Return
for _ in \$(seq 1 120); do
  acts running
  "$jq_bin" -e "\$group | .state == \"running\"" "$fi_dir/activity-running.json" > /dev/null 2>&1 && break
  sleep 0.5
done
call menu close > /dev/null 2>&1
sleep 1
# Mid-install: once zypper is the step, else whatever the run is at after 20s.
for _ in \$(seq 1 40); do
  acts mid
  "$jq_bin" -e "\$group | .state != \"running\" or (.items[0].phase == \"committing\")" "$fi_dir/activity-mid.json" > /dev/null 2>&1 && break
  sleep 0.5
done
call sidebar status > "$fi_dir/sidebar-mid.json" 2>&1
shoot mid
for _ in \$(seq 1 1400); do
  acts end
  "$jq_bin" -e "\$group | .state != \"running\"" "$fi_dir/activity-end.json" > /dev/null 2>&1 && break
  sleep 0.5
done
call sidebar status > "$fi_dir/sidebar-done.json" 2>&1
shoot done-card
call menu summon "" > /dev/null 2>&1
sleep 0.5
"$wtype_bin" firefox
sleep 3
status found
shoot found
call menu close > /dev/null 2>&1
git -C "$fi_dir/upstream.git" log -1 --format='%an <%ae>|%s' > "$fi_dir/upstream-head.txt" 2>&1
cp "\${XDG_STATE_HOME:-\$HOME/.local/state}"/fspec/log/* "$fi_dir/" 2>/dev/null
touch "$fi_dir/drive.done"
EOF
  hypr_exec_once "bash $script"
}

leg_firefox_install_assert() {
  [ -e "$fi_dir/drive.done" ] || fail "--firefox-install: the drive never finished"
  local group='[.groups[] | select(.source == "fspec:install:'"$fi_pkg"'")] | first'
  "$jq_bin" -e '.ids | index("'"$fi_row"'") != null' "$fi_dir/rows.json" > /dev/null \
    || fail "--firefox-install: no $fi_pkg row for \"firefox\": $("$jq_bin" -c '{empty, ids: .ids[:6]}' "$fi_dir/rows.json")"
  "$jq_bin" -e '.cursorId == "'"$fi_row"'"' "$fi_dir/picked.json" > /dev/null \
    || fail "--firefox-install: Down did not land on the zypper row: $("$jq_bin" -c .cursorId "$fi_dir/picked.json")"
  "$jq_bin" -e "$group | .state == \"running\"" "$fi_dir/activity-mid.json" > /dev/null \
    || fail "--firefox-install: the install was not running mid-way: $("$jq_bin" -c "$group" "$fi_dir/activity-mid.json")"
  "$jq_bin" -e '.cards | index("notifications") as $n | index("activity") == $n + 1' "$fi_dir/sidebar-mid.json" > /dev/null \
    || fail "--firefox-install: the activity card is not under notifications mid-install: $("$jq_bin" -c .cards "$fi_dir/sidebar-mid.json")"
  "$jq_bin" -e "$group | .state == \"succeeded\"" "$fi_dir/activity-end.json" > /dev/null \
    || fail "--firefox-install: the install did not succeed: $("$jq_bin" -c "$group" "$fi_dir/activity-end.json")"
  grep -q "^fspec <fspec@" "$fi_dir/upstream-head.txt" \
    || fail "--firefox-install: the upstream's head is not fspec's own commit: $(cat "$fi_dir/upstream-head.txt")"
  "$jq_bin" -e '[.ids[] | select(startswith("apps.") and (ascii_downcase | contains("firefox")))] | length > 0' "$fi_dir/found.json" > /dev/null \
    || fail "--firefox-install: Firefox is not in the results once installed: $("$jq_bin" -c '.ids[:6]' "$fi_dir/found.json")"
  echo "SMOKE_FIREFOX_INSTALL_COMMIT $(cat "$fi_dir/upstream-head.txt")"
  echo "SMOKE_FIREFOX_INSTALL_ROWS $fi_dir/rows.png"
  echo "SMOKE_FIREFOX_INSTALL_MID $fi_dir/mid.png"
  echo "SMOKE_FIREFOX_INSTALL_DONE $fi_dir/done-card.png"
  echo "SMOKE_FIREFOX_INSTALL_FOUND $fi_dir/found.png"
}
