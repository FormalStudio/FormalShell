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
leg_firefox_install_claims="layout=sidebar frame=none"

fi_dir="$shot_dir/firefox-install"
fi_pkg="zypper:MozillaFirefox"
fi_row="install.pkg.zypper%3AMozillaFirefox"

leg_firefox_install_fixture() {
  settings_pin '.layout = "sidebar"'
  mkdir -p "$fi_dir"
  rm -rf "$fi_dir/upstream.git" "$fi_dir/spec"
  # A stalled transfer has hung a clone for over an hour and a cut one has
  # left a pack git could not index: each try is bounded, a slow link
  # aborts, and a second try starts clean.
  local try cloned="" err=""
  for try in 1 2; do
    rm -rf "$fi_dir/upstream.git"
    err=$(timeout 300 git -c http.lowSpeedLimit=1000 -c http.lowSpeedTime=30 \
      clone -q --bare https://github.com/FormalSnake/dotfiles "$fi_dir/upstream.git" 2>&1) && { cloned=1; break; }
    err="try $try exited $? (124 is the 300s bound): $err"
    echo "--firefox-install: $err" >&2
  done
  [ -n "$cloned" ] || { echo "SMOKE_FAIL: --firefox-install: could not clone the spec from GitHub: $err" >&2; exit 1; }
  git clone -q "$fi_dir/upstream.git" "$fi_dir/spec" \
    || { echo "SMOKE_FAIL: --firefox-install: could not check the spec out of its bare copy" >&2; exit 1; }
  export FSPEC_SPEC="$fi_dir/spec"
  # zypper puts Firefox's desktop entry under /usr/share, which the
  # isolated session's XDG_DATA_DIRS leaves out.
  session_data_dirs="/usr/local/share:/usr/share"
}

leg_firefox_install_timing() {
  leg_timing 900
}

leg_firefox_install_drive() {
  local script="$shot_dir/firefox-install-drive.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
group='[.groups[] | select(.source == "fspec:install:$fi_pkg")] | first'
call menu summon install > /dev/null 2>&1
until_settled
"$wtype_bin" firefox
until_jq 60 "$fi_dir/rows.json" '.ids | index("$fi_row") != null' call menu status
shoot "$fi_dir/rows.png"
n=\$("$jq_bin" '.ids | index("$fi_row")' "$fi_dir/rows.json")
for _ in \$(seq 1 "\$n"); do "$wtype_bin" -k Down; sleep 0.1; done
status menu "$fi_dir/picked.json"
"$wtype_bin" -k Return
for _ in \$(seq 1 120); do
  status activity "$fi_dir/activity-running.json"
  "$jq_bin" -e "\$group | .state == \"running\"" "$fi_dir/activity-running.json" > /dev/null 2>&1 && break
  sleep 0.5
done
call menu close > /dev/null 2>&1
sleep 1
# Mid-install: once zypper is the step, else whatever the run is at after 20s.
for _ in \$(seq 1 40); do
  status activity "$fi_dir/activity-mid.json"
  "$jq_bin" -e "\$group | .state != \"running\" or (.items[0].phase == \"committing\")" "$fi_dir/activity-mid.json" > /dev/null 2>&1 && break
  sleep 0.5
done
call sidebar status > "$fi_dir/sidebar-mid.json" 2>&1
shoot "$fi_dir/mid.png"
for _ in \$(seq 1 1400); do
  status activity "$fi_dir/activity-end.json"
  "$jq_bin" -e "\$group | .state != \"running\"" "$fi_dir/activity-end.json" > /dev/null 2>&1 && break
  sleep 0.5
done
call sidebar status > "$fi_dir/sidebar-done.json" 2>&1
shoot "$fi_dir/done-card.png"
call menu summon "" > /dev/null 2>&1
until_settled
"$wtype_bin" firefox
sleep 3
status menu "$fi_dir/found.json"
shoot "$fi_dir/found.png"
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
