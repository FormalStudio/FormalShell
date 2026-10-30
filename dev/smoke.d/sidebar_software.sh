# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, iso_home, the *_bin paths and fail()
# --sidebar-software: the sidebar's Software card (Plan 5.3) under
# `layout: sidebar`, against a PATH-shimmed `fspec` and a fixture spec
# checkout (`FSPEC_SPEC`) cloned from a bare upstream that has moved on:
# 22 commits the checkout lacks, the oldest of them fspec's own
# `feat(apps): install org.example.Foxglove on <this host>` touching
# apps.star, then 21 for another host that must not hide it.
#
# Read off `packages status`'s `software` and `sidebar status`: the card
# drawn, 22 updates behind `origin/main` and `Installed Foxglove` (the
# staged desktop entry's name). A real click on Apply fast-forwards the
# checkout and reaches the shim as exactly `apply --rev HEAD` with HEAD
# already the upstream's; the shim refuses that first apply, so the row
# reads Apply failed with its retry though nothing is behind any more, and
# a second click applies, the card then up to date. A real click on Undo
# reaches the shim as `app undo --detach`; the shim pushes fspec's
# uninstall commit the way the real undo does, and the card re-checks to
# one update and `Removed Foxglove`. With the branch's upstream unset,
# `packages check` reads the honest error row. The behind, retry,
# up-to-date and undone cards are photographed.
leg_sidebar_software_flag="--sidebar-software"
leg_sidebar_software_order=242
leg_sidebar_software_needs="jq wlrctl git"

need_git() { need_tool git_bin git git-core; }

ss_dir="$shot_dir/sidebar-software"
ss_shim_dir="$ss_dir/shim"
ss_spec="$ss_dir/spec"

leg_sidebar_software_validate() {
  if leg_on bar_position || leg_on frame; then
    echo "usage: --sidebar-software places the column itself and cannot combine with --bar-position or --frame" >&2
    exit 1
  fi
}

# ss_commit <bare repo> <message> <apps.star> [first]: one commit on its
# main through fast-import, which carries the committer itself.
ss_commit() {
  {
    echo "commit refs/heads/main"
    echo "committer smoke <smoke@localhost> $(date +%s) +0000"
    printf 'data <<MSG\n%s\nMSG\n' "$2"
    [ -n "${4:-}" ] || echo "from refs/heads/main^0"
    printf 'M 100644 inline apps.star\ndata <<APPS\n%s\nAPPS\n' "$3"
    [ -z "${4:-}" ] || printf 'M 100644 inline spec.star\ndata <<SPEC\nload("apps.star", "APPS")\nSPEC\n'
    echo
  } | "$git_bin" -C "$1" fast-import --quiet
}

leg_sidebar_software_fixture() {
  settings_fragment ', "layout": "sidebar"'
  mkdir -p "$ss_shim_dir" "$iso_home/.local/share/applications"
  local host origin="$ss_dir/origin.git"
  host=$(cat /proc/sys/kernel/hostname)
  "$git_bin" init -q --bare "$origin"
  ss_commit "$origin" "init" 'APPS = {}' first
  "$git_bin" clone -q --branch main "$origin" "$ss_spec"
  ss_commit "$origin" "feat(apps): install org.example.Foxglove on $host" "APPS = {\"$host\": [{\"name\": \"org.example.Foxglove\"}]}"
  local n
  for n in $(seq 1 21); do
    ss_commit "$origin" "feat(apps): uninstall org.example.Nettle$n on elsewhere" "APPS = {\"$host\": [{\"name\": \"org.example.Foxglove\"}], \"elsewhere\": [$n]}"
  done
  "$git_bin" -C "$origin" rev-parse main > "$ss_dir/upstream-head"
  cat > "$iso_home/.local/share/applications/org.example.Foxglove.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Foxglove
Exec=true
Icon=folder
DESKTOP
  # The shim's undo pushes fspec's own commit, through the same fast-import.
  {
    echo '#!/usr/bin/env bash'
    declare -f ss_commit
    cat <<SHIM
git_bin="$git_bin"
echo "\$*" >> "$ss_dir/shim.log"
case "\$*" in
  "apply --rev HEAD")
    "$git_bin" -C "$ss_spec" rev-parse HEAD > "$ss_dir/applied-head"
    sleep 1
    if [ ! -e "$ss_dir/apply-failed" ]; then
      touch "$ss_dir/apply-failed"
      echo "fspec: error: smoke apply refused" >&2
      exit 1
    fi ;;
  "app undo --detach")
    ss_commit "$origin" "feat(apps): uninstall org.example.Foxglove on $host" 'APPS = {"elsewhere": []}'
    printf '{"op":"smoke-undo"}\n' ;;
  *) echo "fspec shim: \$* is not shimmed" >&2; exit 2 ;;
esac
SHIM
  } > "$ss_shim_dir/fspec"
  chmod +x "$ss_shim_dir/fspec"
  export PATH="$ss_shim_dir:$PATH"
  export FSPEC_SPEC="$ss_spec/spec.star"
}

leg_sidebar_software_timing() {
  leg_timing 35 70
}

leg_sidebar_software_drive() {
  local script="$shot_dir/sidebar-software-drive.sh"
  write_script "$script" <<EOS
#!/usr/bin/env bash
pkgs() { $ipc call packages status > "$ss_dir/\$1.json" 2>&1; }
# Polls packages status into step \$1 until jq claim \$2 holds, at most \$3 s.
until_pkgs() {
  SECONDS=0
  while [ "\$SECONDS" -lt "\$3" ]; do
    pkgs "\$1"
    "$jq_bin" -e "\$2" "$ss_dir/\$1.json" > /dev/null 2>&1 && return 0
    sleep 0.3
  done
}
# A control's centre on the output: its rect in the window plus the layer's
# own origin.
at() {
  local origin status
  status=\$($ipc call sidebar status)
  origin=\$("$hyprctl_bin" -j layers | "$jq_bin" -r '[.[] | .levels[] | .[] | select(.namespace == "formalshell:bar")] | first | "\(.x) \(.y)"')
  read -r ox oy <<< "\$origin"
  "$jq_bin" -r --arg on "\$1" --argjson ox "\$ox" --argjson oy "\$oy" \
    '.hits[] | select(.on == \$on) | "\(\$ox + .x + (.w / 2 | floor)) \(\$oy + .y + (.h / 2 | floor))"' <<< "\$status" | head -1
}
click() {
  local x y
  read -r x y <<< "\$(at "\$1")"
  [ -n "\$x" ] || { echo "no \$1 control" >> "$ss_dir/drive.log"; return; }
  "$wlrctl_bin" pointer move -4000 -4000 > /dev/null 2>&1
  "$wlrctl_bin" pointer move "\$x" "\$y" > /dev/null 2>&1
  sleep 0.4
  "$wlrctl_bin" pointer click left > /dev/null 2>&1
  "$wlrctl_bin" pointer move 600 0 > /dev/null 2>&1
}
for _ in \$(seq 1 40); do
  $ipc call sidebar status 2>/dev/null | grep -q '"software"' && break
  sleep 0.5
done
until_pkgs behind '.software.behind == 22 and .software.last != null' 15
sleep 0.5
$ipc call sidebar status > "$ss_dir/sidebar.json" 2>&1
"$grim_bin" "$ss_dir/behind.png" > /dev/null 2>&1
click software:apply
until_pkgs applyfailed '.software.applying == false and .software.failed != null and .software.behind == 0' 15
sleep 0.5
"$grim_bin" "$ss_dir/retry.png" > /dev/null 2>&1
click software:apply
until_pkgs applied '.software.applying == false and .software.behind == 0 and .software.failed == null' 15
sleep 0.5
"$grim_bin" "$ss_dir/current.png" > /dev/null 2>&1
click software:undo
until_pkgs undone '.software.behind == 1 and .software.last.verb == "uninstall"' 15
sleep 0.5
"$grim_bin" "$ss_dir/undone.png" > /dev/null 2>&1
"$git_bin" -C "$ss_spec" branch -q --unset-upstream
$ipc call packages check > /dev/null 2>&1
until_pkgs noupstream '.software.error != null' 10
EOS
  hypr_exec_once "bash $script"
}

# `<step> <jq claim> <what it means>`, against that step's file.
ss_expect() {
  local f="$ss_dir/$1.json"
  [ -s "$f" ] || fail "sidebar-software: no status for $1"
  "$jq_bin" -e "$2" "$f" > /dev/null || fail "sidebar-software $1: $3, got $("$jq_bin" -c '.software // .' "$f" 2>/dev/null || cat "$f")"
}

leg_sidebar_software_assert() {
  ss_expect sidebar '.cards | index("software") != null' "the sidebar drew no Software card"
  ss_expect behind '.software.behind == 22 and .software.upstream == "origin/main" and .software.last == {"verb": "install", "name": "org.example.Foxglove"}' "the card did not read two updates behind origin/main and this host's install"
  echo "SMOKE_SIDEBAR_SOFTWARE behind: 22 updates on origin/main, Installed Foxglove past 21 newer commits for another host"
  [ "$(head -2 "$ss_dir/shim.log" 2>/dev/null | rg -cx 'apply --rev HEAD')" = "2" ] || fail "sidebar-software: Apply and its retry did not reach the shim as apply --rev HEAD: $(cat "$ss_dir/shim.log" 2>/dev/null)"
  ss_expect applyfailed '.software.behind == 0 and .software.failed == {"change": "apply", "why": "smoke apply refused"}' "a failed apply after the fast-forward did not keep its retry"
  [ "$(cat "$ss_dir/applied-head" 2>/dev/null)" = "$(cat "$ss_dir/upstream-head")" ] || fail "sidebar-software: Apply ran before the checkout reached the upstream's head"
  ss_expect applied '.software.behind == 0 and .software.failed == null' "the card was not up to date after Apply"
  echo "SMOKE_SIDEBAR_SOFTWARE apply: fast-forwarded, applied as apply --rev HEAD, then up to date"
  rg -qx 'app undo --detach' "$ss_dir/shim.log" || fail "sidebar-software: Undo did not reach the shim as app undo --detach: $(cat "$ss_dir/shim.log")"
  ss_expect undone '.software.behind == 1 and .software.last == {"verb": "uninstall", "name": "org.example.Foxglove"} and .software.failed == null' "the card did not re-check to the undo's commit"
  echo "SMOKE_SIDEBAR_SOFTWARE apply failed, then applied again from the same row"
  echo "SMOKE_SIDEBAR_SOFTWARE undo: app undo --detach, then Removed Foxglove one update behind"
  ss_expect noupstream '.software.error == "The spec'"'"'s branch has no upstream"' "a branch with no upstream did not read as one"
  echo "SMOKE_SIDEBAR_SOFTWARE no upstream: the honest error row"
  local shot
  for shot in behind retry current undone; do
    [ -s "$ss_dir/$shot.png" ] || fail "sidebar-software: no $shot frame"
    echo "SMOKE_SIDEBAR_SOFTWARE_SHOT $ss_dir/$shot.png"
  done
}
