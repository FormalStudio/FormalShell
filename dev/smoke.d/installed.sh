# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154  # dev/smoke.sh reads leg_* and supplies shot_dir, the *_bin paths and fail()
# --installed <distro>: install.sh run from nothing in a rootless podman
# image of arch, trixie or fedora, against this tree's install.sh and the
# release tarball the fixture builds from it (dev/tarball.sh in
# debian:bookworm, as `just tarball` and the hosts' fspec package build it),
# and the ~/.local/bin/formalshell it installs run in place of the fspec
# package. The bookworm build keeps its toolchains and third-party builds in
# the slot user's formalshell-tarball volume, so only a cold slot pays for
# them.
# "Nothing" is the distro's own container image plus sudo and an account:
# install.sh installs Hyprland and the runtime tools with the distro's own
# package manager, unpacks the tarball, and `formalshell install --yes`
# writes the units, the Hyprland files and /etc/pam.d/formalshell-lock.
# Arch on aarch64 is Arch Linux ARM, which has no official image.
#
# The container sees the session through its own XDG_RUNTIME_DIR holding
# the session's Wayland socket and Hyprland instance directory, and nothing
# else of the host's: it runs the shell under its own dbus-run-session, so
# neither the host bus nor the rig's is reachable from it. It runs as the
# host uid (keep-id), which the image also gives an account whose password
# is the VM's, so the lock round trip goes through the installer's own
# /etc/pam.d/formalshell-lock and the distro's own pam_unix.
#
# What it proves, all over the installed formalshell-ipc inside the
# container: a debug dump reporting a connected hyprland backend, the bar
# drawn, `menu toggle` opening the launcher, and a lock that a wrong
# password answers with "Wrong password" (a missing or broken PAM service
# reads "PAM error") and the real one unlocks.
leg_installed_flag="--installed <distro>"
leg_installed_order=5
leg_installed_claims="session"
leg_installed_needs="wtype jq convert"

installed_home=$HOME
installed_src="$shot_dir/installed-src"
installed_container=formalshell-installed
installed_prefix=/home/test/.local
installed_dump_path="$shot_dir/installed-dump.json"
installed_menu_path="$shot_dir/installed-menu-status.json"
installed_lock_rc_path="$shot_dir/installed-lock-rc.txt"
installed_islocked1_path="$shot_dir/installed-islocked-1.txt"
installed_islocked2_path="$shot_dir/installed-islocked-2.txt"
installed_wrong_path="$shot_dir/installed-lock-wrong.json"
installed_status_path="$shot_dir/installed-lock-status.json"
installed_bar_png="$shot_dir/installed-bar.png"
installed_menu_png="$shot_dir/installed-menu.png"
installed_locked_png="$shot_dir/installed-locked.png"
installed_error_png="$shot_dir/installed-error.png"
installed_unlocked_png="$shot_dir/installed-unlocked.png"

# Podman's own store and config live under the real HOME: every script the
# session runs sees the rig's isolated one, and the private bus in
# DBUS_SESSION_BUS_ADDRESS has no systemd on it to place the container.
installed_podman_fn() {
  cat <<EOF
pm() {
  env -u DBUS_SESSION_BUS_ADDRESS -u XDG_CONFIG_HOME -u XDG_DATA_HOME -u XDG_STATE_HOME -u XDG_CACHE_HOME \
    HOME="$installed_home" podman "\$@"
}
EOF
}

leg_installed_validate() {
  installed_distro=$(leg_arg installed)
  command -v podman >/dev/null 2>&1 || { echo "--installed needs podman on PATH" >&2; exit 1; }
  case $installed_distro/$(uname -m) in
    arch/x86_64) installed_base=docker.io/library/archlinux:latest ;;
    arch/aarch64) installed_base=docker.io/menci/archlinuxarm:latest ;;
    trixie/*) installed_base=docker.io/library/debian:trixie ;;
    fedora/*) installed_base=docker.io/library/fedora:latest ;;
    *) echo "--installed: no base image for '$installed_distro' on $(uname -m) (arch, trixie or fedora)" >&2; exit 1 ;;
  esac
  installed_image=localhost/formalshell-installed:$installed_distro
}

leg_installed_fixture() {
  local containerfile="$shot_dir/installed.Containerfile" prep
  eval "$(installed_podman_fn)"
  mkdir -p "$installed_src"
  cp "$script_dir/../install.sh" "$installed_src/"
  echo "installed: building the release tarball in debian:bookworm"
  pm run --rm -v "$(cd "$script_dir/.." && pwd):/src" -v formalshell-tarball:/build -v "$installed_src:/out" \
    docker.io/library/debian:bookworm /src/dev/tarball.sh /out > "$shot_dir/installed-tarball.log" 2>&1 \
    || { tail -40 "$shot_dir/installed-tarball.log" >&2; echo "installed: dev/tarball.sh failed" >&2; exit 1; }
  # pacman's download sandbox needs Landlock, which a rootless container is
  # refused. Debian's and Fedora's images carry no sudo, and Fedora's no
  # useradd, nor the dbus-run-session the shell runs under here (a Fedora
  # desktop's session bus is dbus-broker's), which lands in /usr/sbin, off
  # the image's PATH. Fedora's weak dependencies roughly double its image,
  # past what the VM's disk holds while podman commits it.
  case $installed_distro in
    arch) prep="sed -i '/^\[options\]/a DisableSandbox' /etc/pacman.conf && pacman-key --init && pacman-key --populate && pacman -Syu --noconfirm sudo" ;;
    trixie) prep="apt-get update && apt-get install -y sudo" ;;
    fedora) prep="echo install_weak_deps=False >> /etc/dnf/dnf.conf && dnf -y install sudo shadow-utils dbus-daemon && ln -sf /usr/sbin/dbus-run-session /usr/local/bin/dbus-run-session" ;;
  esac
  cat > "$containerfile" <<EOF
FROM $installed_base
ARG UID
RUN $prep \
 && if getent passwd \$UID >/dev/null; then userdel -r "\$(getent passwd \$UID | cut -d: -f1)"; fi \
 && useradd -u \$UID -m test && echo test:$rig_password | chpasswd \
 && echo 'test ALL=(ALL) NOPASSWD: ALL' > /etc/sudoers.d/test
COPY . /src/
USER test
RUN sh /src/install.sh --from /src/formalshell-$(uname -m)-linux.tar.gz --yes \\
 && (sudo pacman -Scc --noconfirm || sudo apt-get clean || sudo dnf clean all || true) > /dev/null 2>&1
EOF
  echo "installed: $installed_distro on $installed_base from $installed_src"
  pm build --build-arg UID="$(id -u)" -t "$installed_image" \
    -f "$containerfile" "$installed_src" > "$shot_dir/installed-build.log" 2>&1 \
    || { tail -40 "$shot_dir/installed-build.log" >&2; echo "installed: install.sh failed in $installed_distro" >&2; exit 1; }
  grep -E '^(not in|[a-z]*'"'"'s repositories|Installing|Unpacked|wrote|note|enabled|Done)' "$shot_dir/installed-build.log" | cut -c1-400
  mkdir -p -m 700 "$shot_dir/installed-runtime"
}

leg_installed_shell() {
  local uid rt
  uid=$(id -u)
  rt=/run/user/$uid
  write_script "$1" <<EOF
#!/usr/bin/env bash
$(installed_podman_fn)
pm rm -f $installed_container > /dev/null 2>&1
pm run --rm --name $installed_container --userns=keep-id --network none \\
  -v "$shot_dir/installed-runtime:$rt" \\
  -v "\$XDG_RUNTIME_DIR/\$WAYLAND_DISPLAY:$rt/\$WAYLAND_DISPLAY" \\
  -v "\$XDG_RUNTIME_DIR/hypr/\$HYPRLAND_INSTANCE_SIGNATURE:$rt/hypr/\$HYPRLAND_INSTANCE_SIGNATURE" \\
  -v "$iso_home:$iso_home" \\
  -e XDG_RUNTIME_DIR=$rt -e WAYLAND_DISPLAY -e HYPRLAND_INSTANCE_SIGNATURE \\
  -e XDG_CURRENT_DESKTOP=Hyprland -e XDG_SESSION_TYPE=wayland \\
  -e HOME="$iso_home" -e XDG_CONFIG_HOME="$iso_home/.config" \\
  -e XDG_STATE_HOME="$iso_home/.local/state" -e XDG_DATA_HOME="$iso_home/.local/share" \\
  -e XDG_CACHE_HOME="$iso_home/.cache" \\
  -e XDG_DATA_DIRS="$iso_home/.local/share:/usr/local/share:/usr/share" \\
  -e LIBGL_ALWAYS_SOFTWARE=1 -e LANG=C.UTF-8 \\
  "$installed_image" dbus-run-session -- $installed_prefix/bin/formalshell > "$shell_log_path" 2>&1 &
for _ in \$(seq 60); do
  pid=\$(pgrep -n -u $uid -f '/lib/formalshell/formalshell-rs\$') && break
  sleep 1
done
[ -n "\${pid:-}" ] && echo "\$pid" > "$shot_dir/shell.pid"
wait
EOF
}

leg_installed_timing() {
  leg_timing 140
}

leg_installed_drive() {
  local script="$shot_dir/installed-drive.sh" stop="$shot_dir/installed-stop.sh"
  write_script "$script" <<EOF
#!/usr/bin/env bash
$(installed_podman_fn)
ipc() { pm exec $installed_container $installed_prefix/bin/formalshell-ipc call "\$@"; }
for _ in \$(seq 60); do
  ipc debug dump > "$installed_dump_path" 2>/dev/null && "$jq_bin" -e '.compositor == "hyprland" and .available == true' "$installed_dump_path" > /dev/null 2>&1 && break
  sleep 1
done
sleep 3
"$grim_bin" "$installed_bar_png" > /dev/null 2>&1
ipc menu toggle > /dev/null 2>&1
sleep 2
ipc menu status > "$installed_menu_path" 2>&1
"$grim_bin" "$installed_menu_png" > /dev/null 2>&1
ipc menu toggle > /dev/null 2>&1
sleep 1
ipc lock lock > /dev/null 2>&1
echo \$? > "$installed_lock_rc_path"
sleep 1
ipc lock isLocked > "$installed_islocked1_path" 2>&1
sleep 3
"$grim_bin" "$installed_locked_png" > /dev/null 2>&1
"$wtype_bin" "wrong-password"
"$wtype_bin" -k Return
sleep 5
"$grim_bin" "$installed_error_png" > /dev/null 2>&1
ipc lock status > "$installed_wrong_path" 2>&1
"$wtype_bin" "$rig_password"
"$wtype_bin" -k Return
sleep 3
"$grim_bin" "$installed_unlocked_png" > /dev/null 2>&1
ipc lock isLocked > "$installed_islocked2_path" 2>&1
ipc lock status > "$installed_status_path" 2>&1
EOF
  write_script "$stop" <<EOF
#!/usr/bin/env bash
$(installed_podman_fn)
pm stop -t 3 $installed_container > /dev/null 2>&1 || true
# Each distro image is several GB of the slot user's disk.
pm image rm -f $installed_image > /dev/null 2>&1
pm image prune -f > /dev/null 2>&1 || true
EOF
  add_cleanup "bash $stop"
  wm_exec_once "bash $script"
}

leg_installed_assert() {
  local f top_sd
  for f in "$installed_bar_png" "$installed_menu_png" "$installed_locked_png" "$installed_error_png" "$installed_unlocked_png"; do
    [ -f "$f" ] || fail "installed: no screenshot at $f"
  done
  echo "SMOKE_INSTALLED_BAR $installed_bar_png"
  echo "SMOKE_INSTALLED_MENU $installed_menu_png"
  echo "SMOKE_INSTALLED_LOCKED $installed_locked_png"
  echo "SMOKE_INSTALLED_ERROR $installed_error_png"
  echo "SMOKE_INSTALLED_UNLOCKED $installed_unlocked_png"
  grep -q '^wrote /etc/pam.d/formalshell-lock' "$shot_dir/installed-build.log" \
    || fail "installed: formalshell install did not write the PAM file"
  cat "$installed_dump_path"; echo
  expect_jq "$installed_dump_path" '.compositor == "hyprland" and .available == true' "installed: the dump does not report a connected hyprland backend"
  # The strip's own band: a bare desktop there is one flat colour.
  top_sd=$($convert_bin "$installed_bar_png" -crop 1920x40+0+0 +repage -colorspace gray -format '%[fx:standard_deviation]' info:)
  echo "installed: bar band stddev $top_sd"
  awk -v s="$top_sd" 'BEGIN { exit !(s > 0.02) }' || fail "installed: nothing drawn in the bar's band"
  $jq_bin -e '.isOpen == true' "$installed_menu_path" > /dev/null \
    || fail "installed: menu toggle did not open the launcher: $(cat "$installed_menu_path")"
  cmp -s "$installed_bar_png" "$installed_menu_png" && fail "installed: the launcher frame is the bar frame"
  grep -q '^0$' "$installed_lock_rc_path" || fail "installed: lock lock exited $(cat "$installed_lock_rc_path")"
  grep -q '^true$' "$installed_islocked1_path" || fail "installed: isLocked after lock: $(cat "$installed_islocked1_path")"
  $jq_bin -e '.authError == "Wrong password"' "$installed_wrong_path" > /dev/null \
    || fail "installed: a wrong password did not read as one through formalshell-lock: $(cat "$installed_wrong_path")"
  grep -q '^false$' "$installed_islocked2_path" \
    || fail "installed: the real password did not unlock: $(cat "$installed_islocked2_path") $(cat "$installed_status_path")"
  cmp -s "$installed_locked_png" "$installed_unlocked_png" && fail "installed: the locked frame is the unlocked frame"
  echo "INSTALLED_OK $installed_distro"
}
