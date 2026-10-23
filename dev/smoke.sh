#!/usr/bin/env bash
# Hyprland smoke rig (M41): the shell's runtime verification loop on the
# backend the redesign ships. Builds the shell, brings up an isolated
# Hyprland session, drives the legs a run asked for inside it, screenshots
# with grim, tears the session down, then asserts each leg's own evidence.
#
# One file per leg under dev/smoke.d/, sourced from here, each owning its
# flag, fixture, drive script and assertions: dump, menu, notify, center,
# osd, panel (and tooltip, which rides on it), panel_at, console, wallpaper,
# lock, picker, clipboard, monitor, processes, hotcorner. What a leg proves
# is in its own header; dev/smoke.d/README.md is the file contract.
#
# Two ways the session comes up, decided by what the machine can actually
# do (render_node_present and vkms_card_device below), never by a flag:
#   nested  the ordinary case and the only one a real host takes: Hyprland
#           is a Wayland client of the running session. aquamarine's wayland
#           backend needs the parent to advertise zwp_linux_dmabuf_v1, and
#           it allocates from the DRM render node that feedback names.
#   vkms    the mac VM rig, which has neither: its parent is a pixman
#           wlroots session on a guest with no GPU at all, so nesting dies
#           at CBackend::create() ("Missing protocols", then "no allocator
#           available"). Hyprland instead takes the software KMS card the
#           kernel's vkms module draws, pinned by AQ_DRM_DEVICES. The branch
#           is gated on that card's sysfs device really being vkms, so it
#           can never take a real GPU out from under a real session, and
#           opening it unprivileged needs seatd (nix/testvm.nix).
#
# D-Bus isolation (M5 hard rule): the whole
# Hyprland invocation runs under `dbus-run-session --`, giving the shell's
# NotificationServer a private session bus instead of the host's. The host's
# is owned by DMS, and acquiring org.freedesktop.Notifications on it would
# steal that name out from under the real desktop. Verified every run: the
# host's `busctl --user status org.freedesktop.Notifications` owner PID must
# be identical before and after.
#
# Host-session safety: the Hyprland invocation and everything it spawns runs
# under an isolated HOME/XDG_*_HOME, never this user's own. Without it the
# ThemeEngine re-executes the owner's real matugen post_hooks against the
# live desktop (observed 2026-07-27). XDG_RUNTIME_DIR stays the host's,
# since the session has to publish its sockets somewhere the drive scripts
# can reach; the shell's single-instance lock keys its socket on
# WAYLAND_DISPLAY for exactly that reason.
set -euo pipefail
script_dir="$(cd "$(dirname "$0")" && pwd)"
cd "$script_dir/.."

# Both temp dirs exist before the leg files are sourced: a leg names its own
# artifacts off them at source time, so a leg that is off this run still has
# readable paths for the leg that coordinates with it.
shot_dir=$(mktemp -d)
iso_home=$(mktemp -d)

legs=()
active_legs=()
settings_extra=""
fixture_cleanup=""
shell_env=""
screenshot_delay=8
session_timeout=40
tail_gap=1

leg_on() {
  local v="leg_${1}_on"
  [ "${!v:-false}" = true ]
}

leg_arg() {
  local v="leg_${1}_arg"
  printf '%s' "${!v:-}"
}

# Merged by max across the active legs, so a combination always outlives its
# slowest half.
leg_timing() {
  if [ "${1:-0}" -gt "$screenshot_delay" ]; then screenshot_delay="$1"; fi
  if [ "${2:-0}" -gt "$session_timeout" ]; then session_timeout="$2"; fi
  if [ "${3:-0}" -gt "$tail_gap" ]; then tail_gap="$3"; fi
}

settings_fragment() {
  settings_extra="$settings_extra$1"
}

# A line for the teardown script, run after the run's own frame and before
# the compositor exits.
add_cleanup() {
  if [ -n "$fixture_cleanup" ]; then
    fixture_cleanup="$fixture_cleanup
$1"
  else
    fixture_cleanup="$1"
  fi
}

# Every leg is a standalone script rather than an inline autostart string:
# `menu select`'s JSON array argument would otherwise have to survive both
# this generator and Hyprland's own config parsing.
write_script() {
  local path="$1"
  cat > "$path"
  chmod +x "$path"
}

for leg_file in "$script_dir"/smoke.d/*.sh; do
  leg_name=$(basename "$leg_file" .sh)
  # shellcheck source=/dev/null
  . "$leg_file"
  legs+=("$leg_name")
done

# Sourcing is alphabetical; the flag list, the settings fragments, the
# autostart lines and the results all follow leg_<name>_order instead.
leg_order_list() {
  local n v
  for n in "${legs[@]}"; do
    v="leg_${n}_order"
    printf '%s %s\n' "${!v:-500}" "$n"
  done | sort -k1,1n -k2,2
}
ordered=()
while read -r _ leg_name; do
  ordered+=("$leg_name")
done < <(leg_order_list)
legs=("${ordered[@]}")

usage() {
  local n v line=""
  for n in "${legs[@]}"; do
    v="leg_${n}_flag"
    line="$line [${!v}]"
  done
  echo "usage: $0$line" >&2
  exit 1
}

while [ $# -gt 0 ]; do
  matched=false
  for leg_name in "${legs[@]}"; do
    flag_var="leg_${leg_name}_flag"
    flag_decl="${!flag_var}"
    [ "$1" = "${flag_decl%% *}" ] || continue
    matched=true
    eval "leg_${leg_name}_on=true"
    if [ "$flag_decl" = "${flag_decl%% *}" ]; then
      shift
    else
      [ $# -ge 2 ] || usage
      eval "leg_${leg_name}_arg=\$2"
      shift 2
    fi
    break
  done
  $matched || usage
done

for leg_name in "${legs[@]}"; do
  if leg_on "$leg_name"; then active_legs+=("$leg_name"); fi
done

# Long enough for the base run's drive below and ten seconds of rest after it.
screenshot_delay=52
session_timeout=75

for leg_name in ${active_legs[@]+"${active_legs[@]}"}; do
  if declare -F "leg_${leg_name}_validate" >/dev/null; then "leg_${leg_name}_validate"; fi
done

# Only the base run gets the focused fixture window: every other leg's own
# screenshot exists to show a summoned surface instead. A leg that
# photographs the ordinary desktop
# anyway declares leg_<name>_fixture_window=keep.
fixture_window_mode=true
for leg_name in ${active_legs[@]+"${active_legs[@]}"}; do
  keep_var="leg_${leg_name}_fixture_window"
  if [ "${!keep_var:-}" != "keep" ]; then fixture_window_mode=false; fi
done

# FS_RESULT names a store path built elsewhere and copied in, for a host
# that must not build (e1504g): the result link points at it and nothing is
# evaluated here.
if [ -n "${FS_RESULT:-}" ]; then
  ln -sfn "$FS_RESULT" result
else
  git add -A >/dev/null 2>&1 || true   # flakes only see tracked files
  nix build .#formalshell
fi

# Resolved once, on demand: a leg names what it needs in leg_<name>_needs
# and the scaffold's own four are always resolved.
need_bin() {
  local name
  for name in "$@"; do
    [ -n "$name" ] || continue
    "need_${name//-/_}"
  done
}

need_hyprland() {
  if [ -z "${hyprland_bin:-}" ]; then
    if command -v Hyprland >/dev/null 2>&1; then
      hyprland_bin=Hyprland
    else
      hyprland_bin="nix run nixpkgs#hyprland --"
    fi
  fi
}

need_hyprctl() {
  if [ -z "${hyprctl_bin:-}" ]; then
    if command -v hyprctl >/dev/null 2>&1; then
      hyprctl_bin=$(command -v hyprctl)
    else
      hyprctl_bin=$(nix build --no-link --print-out-paths 'nixpkgs#hyprland^out')/bin/hyprctl
    fi
  fi
}

need_grim() {
  if [ -z "${grim_bin:-}" ]; then
    if command -v grim >/dev/null 2>&1; then
      grim_bin=$(command -v grim)
    else
      grim_bin=$(nix build --no-link --print-out-paths 'nixpkgs#grim^out')/bin/grim
    fi
  fi
}

need_foot() {
  if [ -z "${foot_bin:-}" ]; then
    if command -v foot >/dev/null 2>&1; then
      foot_bin=$(command -v foot)
    else
      foot_bin=$(nix build --no-link --print-out-paths 'nixpkgs#foot^out')/bin/foot
    fi
  fi
}

need_notify_send() {
  if [ -z "${notify_send_bin:-}" ]; then
    if command -v notify-send >/dev/null 2>&1; then
      notify_send_bin=$(command -v notify-send)
    else
      notify_send_bin=$(nix build --no-link --print-out-paths 'nixpkgs#libnotify^out')/bin/notify-send
    fi
  fi
}

need_wlrctl() {
  if [ -z "${wlrctl_bin:-}" ]; then
    if command -v wlrctl >/dev/null 2>&1; then
      wlrctl_bin=$(command -v wlrctl)
    else
      wlrctl_bin=$(nix build --no-link --print-out-paths 'nixpkgs#wlrctl^out')/bin/wlrctl
    fi
  fi
}

need_wpctl() {
  if [ -z "${wpctl_bin:-}" ]; then
    if command -v wpctl >/dev/null 2>&1; then
      wpctl_bin=$(command -v wpctl)
    else
      wpctl_bin=$(nix build --no-link --print-out-paths 'nixpkgs#wireplumber^out')/bin/wpctl
    fi
  fi
}

need_convert() {
  if [ -z "${convert_bin:-}" ]; then
    if command -v convert >/dev/null 2>&1; then
      convert_bin=convert
    else
      convert_bin="nix run nixpkgs#imagemagick -- convert"
    fi
  fi
}

need_wtype() {
  if [ -z "${wtype_bin:-}" ]; then
    if command -v wtype >/dev/null 2>&1; then
      wtype_bin=$(command -v wtype)
    else
      wtype_bin=$(nix build 'nixpkgs#wtype^out' --no-link --print-out-paths)/bin/wtype
    fi
  fi
}

need_jq() {
  if [ -z "${jq_bin:-}" ]; then
    if command -v jq >/dev/null 2>&1; then
      jq_bin=$(command -v jq)
    else
      jq_bin=$(nix build --no-link --print-out-paths 'nixpkgs#jq^out')/bin/jq
    fi
  fi
}

need_wl_copy() {
  if [ -z "${wl_copy_bin:-}" ]; then
    if command -v wl-copy >/dev/null 2>&1; then
      wl_copy_bin=$(command -v wl-copy)
    else
      wl_copy_bin=$(nix build 'nixpkgs#wl-clipboard^out' --no-link --print-out-paths)/bin/wl-copy
    fi
  fi
}

need_wl_paste() {
  if [ -z "${wl_paste_bin:-}" ]; then
    if command -v wl-paste >/dev/null 2>&1; then
      wl_paste_bin=$(command -v wl-paste)
    else
      wl_paste_bin=$(nix build 'nixpkgs#wl-clipboard^out' --no-link --print-out-paths)/bin/wl-paste
    fi
  fi
}

need_localsend_cli() {
  if [ -z "${localsend_cli_bin:-}" ]; then
    if command -v localsend-cli >/dev/null 2>&1; then
      localsend_cli_bin=$(command -v localsend-cli)
    else
      # This repo's own package (nix/localsend-cli.nix), not upstream
      # nixpkgs -- --localsend's own second CLI instance, standing in for a
      # real second device, is built the same way `nix build .#formalshell`
      # already builds the wrapped shell above.
      localsend_cli_bin=$(nix build --no-link --print-out-paths '.#localsend-cli')/bin/localsend-cli
    fi
  fi
}

need_bin hyprland hyprctl grim
if $fixture_window_mode; then
  need_bin foot convert
fi
for leg_name in ${active_legs[@]+"${active_legs[@]}"}; do
  needs_var="leg_${leg_name}_needs"
  leg_needs=()
  read -r -a leg_needs <<< "${!needs_var:-}" || true
  need_bin ${leg_needs[@]+"${leg_needs[@]}"}
done

# The one way a leg reaches the shell: `$ipc call <target> <fn> [args...]`,
# shell-quoted so it pastes into a generated script as it stands: the
# package's own `formalshell-ipc`, the same command a user's binds run.
printf -v ipc '%q' "$PWD/result/bin/formalshell-ipc"

# A dead Wayland socket file outlives its compositor (nothing left to unlink
# it), so a value read back out of the systemd user environment has to be
# checked against a live listener rather than mere existence.
wayland_socket_live() {
  ss -xl 2>/dev/null | awk -v p="$1" '$1 == "u_str" && $2 == "LISTEN" && $5 == p { found=1 } END { exit !found }'
}

wayland_display="${WAYLAND_DISPLAY:-}"
if [ -z "$wayland_display" ]; then
  fallback=$(systemctl --user show-environment 2>/dev/null | sed -n 's/^WAYLAND_DISPLAY=//p')
  if [ -n "$fallback" ] && wayland_socket_live "${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/$fallback"; then
    wayland_display="$fallback"
  fi
fi

# A nested Hyprland imports its own environment into the systemd user
# manager it shares with the host session (PATH, XDG_DATA_DIRS and the
# instance signature among them), and for the whole run anything that
# restarts formalshell.service lands the live shell on the nested display.
# HYPRLAND_NO_SD_VARS in session_env gates two of Hyprland's three call
# sites; 0.56's CExecutor runs `systemctl --user import-environment` on its
# start event with no such check (src/config/supplementary/executor/
# Executor.cpp). So the session's PATH starts with a systemctl that drops
# every environment write and runs the real one for anything else, logging
# what it dropped. Every name Hyprland imports is still put back as it was
# at exit, or unset if it was not there, as the backstop.
host_env_names="DISPLAY WAYLAND_DISPLAY HYPRLAND_INSTANCE_SIGNATURE XDG_CURRENT_DESKTOP QT_QPA_PLATFORMTHEME PATH XDG_DATA_DIRS"
host_env_before=$(systemctl --user show-environment 2>/dev/null || true)
host_wayland_before=$(printf '%s\n' "$host_env_before" | sed -n 's/^WAYLAND_DISPLAY=//p')
host_env_mid_path="$shot_dir/host-env-mid.txt"
sd_shim_dir="$shot_dir/sd-shim"
sd_shim_log="$shot_dir/sd-shim.log"
mkdir -p "$sd_shim_dir"
write_script "$sd_shim_dir/systemctl" <<EOF
#!/usr/bin/env bash
for arg in "\$@"; do
  case "\$arg" in
    import-environment|set-environment|unset-environment)
      echo "dropped: systemctl \$*" >> "$sd_shim_log"
      exit 0 ;;
  esac
done
exec $(printf '%q' "$(command -v systemctl)") "\$@"
EOF
restore_host_env() {
  local name line
  for name in $host_env_names; do
    line=$(printf '%s\n' "$host_env_before" | grep -m1 "^$name=" || true)
    if [ -n "$line" ]; then
      systemctl --user set-environment "$line" 2>/dev/null || true
    else
      systemctl --user unset-environment "$name" 2>/dev/null || true
    fi
  done
}
system_bus_pid=""
on_exit() {
  restore_host_env
  if [ -n "$system_bus_pid" ]; then kill "$system_bus_pid" 2>/dev/null || true; fi
}
trap on_exit EXIT

# A render node is the cheap stand-in for "this machine can nest": both
# halves of aquamarine's wayland backend (the parent's dmabuf advertisement
# and the allocator opened from that feedback's main device) come from one,
# and no software path substitutes for it. A KMS-only card cannot allocate
# dumb buffers from a second, non-master process, which is exactly what a
# nested compositor is.
render_node_present() {
  for node in /dev/dri/renderD*; do
    [ -e "$node" ] && return 0
  done
  return 1
}

# The driver name is not readable off card*/device/driver (vkms sits on the
# faux bus, whose symlink says faux_driver), but the device path itself is
# /sys/devices/faux/vkms/..., which nothing else produces.
vkms_card_device() {
  for sysfs in /sys/class/drm/card[0-9]*; do
    [ -e "$sysfs/device" ] || continue
    case "$(readlink -f "$sysfs/device")" in
      */vkms*) echo "/dev/dri/$(basename "$sysfs")"; return 0 ;;
    esac
  done
  return 1
}

session_mode=""
vkms_device=""
if [ -n "$wayland_display" ] && render_node_present; then
  session_mode="nested"
elif vkms_device=$(vkms_card_device); then
  session_mode="vkms"
else
  echo "SMOKE_FAIL: no way to bring up Hyprland here: no live parent session with a DRM render node to nest in, and no vkms card to render on" >&2
  exit 1
fi

# System bus isolation: dbus-run-session below only replaces the session
# bus. On a real host the system bus is the machine's own, so a nested
# shell's network panel reaches the real NetworkManager (a driven run took
# g815's Wi-Fi down, 2026-10-09), and BlueZ, UPower, logind and polkit the
# same way. A nested run gets a private system bus with nothing on it, so
# each of those services shows its unavailable state. Exported rather than
# put in session_env so a takeover leg's own sessions and every helper
# started from here inherit it too. sudo resets the environment, so a leg
# that needs root or a real system service declares leg_<n>_vm_only and is
# refused here. The VM rig keeps its system bus, which is the VM's own.
if [ "$session_mode" = "nested" ]; then
  for leg_name in ${active_legs[@]+"${active_legs[@]}"}; do
    vm_only_var="leg_${leg_name}_vm_only"
    if [ -n "${!vm_only_var:-}" ]; then
      flag_var="leg_${leg_name}_flag"
      flag_decl="${!flag_var}"
      echo "SMOKE_FAIL: ${flag_decl%% *} needs the VM rig (dev/vm-lock.sh just vm-smoke): ${!vm_only_var}" >&2
      exit 1
    fi
  done
  system_bus_dir="$shot_dir/system-bus"
  mkdir -p "$system_bus_dir"
  cat > "$system_bus_dir/system.conf" <<EOF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>system</type>
  <listen>unix:path=$system_bus_dir/socket</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
EOF
  dbus-daemon --config-file="$system_bus_dir/system.conf" --nofork --nopidfile > "$system_bus_dir/daemon.log" 2>&1 &
  system_bus_pid=$!
  for _ in $(seq 50); do
    [ -S "$system_bus_dir/socket" ] && break
    sleep 0.1
  done
  if [ ! -S "$system_bus_dir/socket" ]; then
    echo "SMOKE_FAIL: the private system bus never came up: $(cat "$system_bus_dir/daemon.log")" >&2
    exit 1
  fi
  export DBUS_SYSTEM_BUS_ADDRESS="unix:path=$system_bus_dir/socket"
  echo "system bus: private ($DBUS_SYSTEM_BUS_ADDRESS)"
fi

# D-Bus isolation check (see the header): captured now, compared against the
# same query once the session has torn down.
host_notifications_owner() {
  # `|| true`: busctl exits 1 with ENXIO when the name has no owner at all
  # (the VM rig's normal answer, it has no desktop bus owner). That is a
  # legitimate reply, not a connectivity failure, so it must not trip
  # pipefail here.
  busctl --user status org.freedesktop.Notifications 2>/dev/null | sed -n 's/^PID=//p' || true
}
host_notifications_owner_before=$(host_notifications_owner)

# The rig boots from a Lua config, the only format Hyprland is supported on.
# Legs append to it through this helper, one autostart line per drive script.
hypr_exec_once() {
  printf 'hl.on("hyprland.start", function() hl.exec_cmd([==[%s]==]) end)\n' "$1"
}

# The session shape every run boots with, shared with --screensaver-gif's
# per-effect sessions.
hypr_base_config() {
  # Pinned in both session modes: the vkms connector otherwise comes up
  # 1024x768 at scale 2, and a nested window takes aquamarine's own default
  # rather than a size the bar is worth reading at. FS_SMOKE_SCALE=1.25
  # runs it fractional, the g815 panel's scale.
  echo "hl.monitor({ output = \"\", mode = \"1920x1080@60\", position = \"0x0\", scale = ${FS_SMOKE_SCALE:-1} })"
  echo "hl.config({"
  echo "  general = { gaps_in = 0, gaps_out = 0, border_size = 0 },"
  # DESIGN.md: nothing in the shell blurs anything, and a compositor blur
  # behind the bar's own transparent strip is exactly the frame this rig
  # exists to catch.
  echo "  decoration = { rounding = 0, blur = { enabled = false } },"
  echo "  animations = { enabled = false },"
  # disable_watchdog_warning: without start-hyprland (a watchdog wrapper this
  # rig has no use for) Hyprland posts a red full-width notification across
  # the top of the screen for 15s, which is exactly where the bar is.
  echo "  misc = {"
  echo "    disable_watchdog_warning = true,"
  echo "    disable_hyprland_logo = true,"
  echo "    disable_splash_rendering = true,"
  echo "    force_default_wallpaper = 0,"
  echo "    disable_autoreload = true,"
  echo "  },"
  # suppress_errors: Hyprland's error overlay is a full-width banner across
  # the top of the screen, which is where the bar is. It renders over the
  # exact thing every leg here photographs.
  echo "  debug = { suppress_errors = true, disable_logs = false, enable_stdout_logs = true },"
  echo "})"
}

# A leg that cannot share the single session below takes the run over here,
# with the build done, the binaries resolved and the bus baseline captured,
# and exits itself: --screensaver-gif pins screensaver.effect through the
# settings fixture, which a session reads at startup, so recording five
# effects means five sessions.
for leg_name in ${active_legs[@]+"${active_legs[@]}"}; do
  if declare -F "leg_${leg_name}_takeover" >/dev/null; then "leg_${leg_name}_takeover"; fi
done

shot_path="$shot_dir/smoke.png"
shell_log_path="$shot_dir/shell.log"
hypr_log_path="$shot_dir/hyprland.log"
cfg="$shot_dir/hyprland.lua"

# Isolated HOME for the Hyprland process and everything it spawns, see the
# host-session safety note in the header.
mkdir -p "$iso_home/.config/formalshell" "$iso_home/.local/share/applications" \
  "$iso_home/.local/share/formalshell/calendar"

for leg_name in ${active_legs[@]+"${active_legs[@]}"}; do
  if declare -F "leg_${leg_name}_fixture" >/dev/null; then "leg_${leg_name}_fixture"; fi
done

cat > "$iso_home/.config/formalshell/settings.json" <<EOF
{"calendar": {"icsDir": "$iso_home/.local/share/formalshell/calendar"}, "location": {"latitude": 52.52, "longitude": 13.41}, "nightlight": {"schedule": "off"}$settings_extra}
EOF

# The calendar leg's own events, dated at run time so the fixture never goes
# stale: --panel calendar's screenshot then proves real events render (the
# dot on today's day cell, the rows under the TODAY label) rather than just
# that the grid draws. Three of them, one all-day and two timed, since the
# rows sort all-day first and the rest chronologically and only a timed one
# prints a time column. Fixed hours rather than offsets from the run's own
# clock: an "in progress" window computed at run time would cross midnight
# on a late run and drop off today entirely. No other leg reads this file.
today_ics=$(date -u +%Y%m%d)
cat > "$iso_home/.local/share/formalshell/calendar/smoke-fixture.ics" <<EOF
BEGIN:VCALENDAR
BEGIN:VEVENT
UID:smoke-fixture-1
SUMMARY:Smoke fixture event
DTSTART;VALUE=DATE:$today_ics
END:VEVENT
BEGIN:VEVENT
UID:smoke-fixture-2
SUMMARY:Smoke fixture standup
DTSTART:${today_ics}T091500
DTEND:${today_ics}T094500
END:VEVENT
BEGIN:VEVENT
UID:smoke-fixture-3
SUMMARY:Smoke fixture review
DTSTART:${today_ics}T143000
DTEND:${today_ics}T160000
END:VEVENT
END:VCALENDAR
EOF

# The fixture window's app id resolves against this entry, so ActiveWindow
# renders a real display name and a real themed icon rather than whatever
# the machine running the rig happens to have installed.
if $fixture_window_mode; then
  cat > "$iso_home/.local/share/applications/formalshell-smoke-iconic.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Iconic Test App
Exec=true
Icon=formalshell-smoke
EOF
  mkdir -p "$iso_home/.local/share/icons/hicolor/48x48/apps"
  $convert_bin -size 48x48 xc:'#CE5D97' "$iso_home/.local/share/icons/hicolor/48x48/apps/formalshell-smoke.png"
  # QIconLoader enumerates no directories at all without an index.theme, so
  # a bare PNG in the right place still resolves to no icon.
  cat > "$iso_home/.local/share/icons/hicolor/index.theme" <<'EOF'
[Icon Theme]
Name=Hicolor
Comment=Fallback icon theme
Directories=48x48/apps

[48x48/apps]
Size=48
Context=Applications
Type=Threshold
EOF
fi

# The shell's own wire traffic, for a leg that has to assert on what the
# shell said rather than on what the screen shows: a layer surface's
# exclusive zone is committed once and never read back, so a wrong one is
# invisible in a screenshot and in `hyprctl layers` alike (frame.sh). Opt-in
# per leg because it is a protocol dump: `leg_<n>_wayland_debug=1`.
wayland_debug_line=""
for leg_name in ${active_legs[@]+"${active_legs[@]}"}; do
  wayland_debug_var="leg_${leg_name}_wayland_debug"
  if [ "${!wayland_debug_var:-0}" = 1 ]; then wayland_debug_line="export WAYLAND_DEBUG=1"; fi
done

shell_start_script="$shot_dir/shell-start.sh"
shell_launcher=""
for leg_name in ${active_legs[@]+"${active_legs[@]}"}; do
  if declare -F "leg_${leg_name}_shell" >/dev/null; then shell_launcher="leg_${leg_name}_shell"; fi
done
shell_bin="$PWD/result/bin/formalshell-rs"
# FS_CPU_QUOTA=25% runs the shell alone under a CPU quota, in a transient
# user scope of its own, so a fast machine stands in for e1504g's N305 in
# power saver. systemd-run reaches the user manager over its private socket,
# never the session's own bus. The scope is stopped after the session, and
# its name puts it under dev/scoped-run.sh's sweep of leftover fs-run-*.
shell_prefix=""
shell_unit=""
if [ -n "${FS_CPU_QUOTA:-}" ]; then
  shell_unit="fs-run-shell-$$"
  shell_prefix="systemd-run --user --scope --quiet --collect --unit=$shell_unit -p CPUQuota=$FS_CPU_QUOTA -p CPUQuotaPeriodSec=${FS_CPU_QUOTA_PERIOD:-100ms} -p RuntimeMaxSec=2h --"
  echo "shell cpu quota: $FS_CPU_QUOTA per ${FS_CPU_QUOTA_PERIOD:-100ms}"
fi
# FS_TRACE: the shell logs every commit, which the budget legs read.
shell_prefix="env FS_TRACE=1${shell_env:+ $shell_env}${shell_prefix:+ $shell_prefix}"
if [ -n "$shell_launcher" ]; then
  "$shell_launcher" "$shell_start_script"
else
write_script "$shell_start_script" <<EOF
#!/usr/bin/env bash
# LIBGL_ALWAYS_SOFTWARE: on the vkms card the compositor advertises dmabuf
# with a main device Mesa cannot open for a client ("failed to get driver
# name for fd -1"), and a client's EGL init fails on it. Forcing the
# software driver keeps the shell and its children on the wl_shm/llvmpipe
# path they render on anyway. Scoped to this process: Hyprland's
# own EGL runs on gbm over that same card and must keep doing so.
export LIBGL_ALWAYS_SOFTWARE=1
# Captured, not discarded: a shell that dies at startup would otherwise
# fail every assertion below with no way to see why.
# Backgrounded rather than exec'd so the shell's pid lands in a file the
# screenshot script reads for its memory sample; the wrapper execs the
# binary in place, so the pid stays the shell's own.
$wayland_debug_line
$shell_prefix "$shell_bin" > "$shell_log_path" 2>&1 &
echo \$! > "$shot_dir/shell.pid"
wait
EOF
fi

if $fixture_window_mode; then
  # Spawned through the compositor (hl.dsp.exec_cmd), so the window is
  # tracked from the moment it maps and dies with the session.
  fixture_script="$shot_dir/fixture-window.sh"
  write_script "$fixture_script" <<EOF
#!/usr/bin/env bash
sleep 2
"$hyprctl_bin" dispatch "hl.dsp.exec_cmd([==[$foot_bin --app-id=formalshell-smoke-iconic --title='formalshell smoke session' sh -c 'sleep 300']==])"
EOF
fi

# The output the session really got. The monitor rule above is a request: a
# nested Hyprland's output follows the size its parent gives the window, so
# a leg cropping frames reads these rather than 1920x1080. Sampled a few
# seconds in, once the parent has configured the window, and again at
# teardown: a parent that resized the window mid-run leaves frames of two
# sizes, which no crop can read.
output_json_path="$shot_dir/output.json"
output_end_json_path="$shot_dir/output-end.json"
output_script="$shot_dir/output.sh"
write_script "$output_script" <<EOF
#!/usr/bin/env bash
sleep 3
"$hyprctl_bin" monitors -j > "$output_json_path" 2>&1
EOF

{
  hypr_base_config
  hypr_exec_once "bash $shell_start_script"
  hypr_exec_once "bash $output_script"
  if $fixture_window_mode; then
    hypr_exec_once "bash $fixture_script"
  fi
} > "$cfg"

# Not a subshell: a drive function writes its own scripts, echoes its
# autostart lines onto the config, and may register a teardown line.
for leg_name in ${active_legs[@]+"${active_legs[@]}"}; do
  if declare -F "leg_${leg_name}_drive" >/dev/null; then "leg_${leg_name}_drive" >> "$cfg"; fi
done

for leg_name in ${active_legs[@]+"${active_legs[@]}"}; do
  if declare -F "leg_${leg_name}_timing" >/dev/null; then "leg_${leg_name}_timing"; fi
done

# The rust shell over IPC: `debug`'s replies to a fixed set of calls into
# rs-ipc.txt for the assertions below, then the spinner on and off, a panel
# and a scrim at full speed for the frame log, and the same at a tenth of
# the speed for frames read mid-flight and at rest.
rs_ipc_path="$shot_dir/rs-ipc.txt"
if [ ${#active_legs[@]} -eq 0 ]; then
  rs_drive="$shot_dir/rs-drive.sh"
  write_script "$rs_drive" <<EOF
#!/usr/bin/env bash
call() { $ipc call "\$@" >> "$shot_dir/rs-calls.log" 2>&1; }
reply() { echo "> \$*"; $ipc call "\$@" 2>&1; echo "exit \$?"; }
shot() { "$grim_bin" "$shot_dir/rs-\$1.png" >> "$shot_dir/grim.log" 2>&1; }
sleep 5
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
  hypr_exec_once "bash $rs_drive" >> "$cfg"
fi

shot_script="$shot_dir/shot.sh"
write_script "$shot_script" <<EOF
#!/usr/bin/env bash
sleep $screenshot_delay
"$grim_bin" "$shot_path" > "$shot_dir/grim.log" 2>&1
sleep $tail_gap
# The host user manager's environment while the session is still up, for
# the check after it that nothing in the run wrote to it.
systemctl --user show-environment > "$host_env_mid_path" 2>/dev/null
# Memory sample of the shell at the end of the run, once every leg has
# driven what it drives: RSS, the number a memory regression shows up in
# first.
if shell_pid=\$(cat "$shot_dir/shell.pid" 2>/dev/null) && [ -r "/proc/\$shell_pid/status" ]; then
  rss_kb=\$(awk '/^VmRSS/{print \$2}' "/proc/\$shell_pid/status")
  echo "SMOKE_MEM rss_kb=\$rss_kb" > "$shot_dir/mem.txt"
  # Every process under the shell right now, for the teardown's check that
  # none of them outlives it.
  ps -eo pid=,ppid=,args= | awk -v root="\$shell_pid" '
    { parent[\$1] = \$2; line[\$1] = \$0 }
    END { for (p in parent) { q = parent[p]; while (q != "" && q != root && q > 1) q = parent[q]; if (q == root) print line[p] } }' \
    > "$shot_dir/shell-children.txt"
fi
"$hyprctl_bin" monitors -j > "$output_end_json_path" 2>&1
$fixture_cleanup
"$hyprctl_bin" dispatch "hl.dsp.exit()"
EOF
hypr_exec_once "bash $shot_script" >> "$cfg"

# HYPRLAND_NO_SD_VARS and the systemctl shim keep the nested Hyprland off
# the user manager it shares with the host session (see host_env_names).
# The restore only runs at exit, and a home-manager activation during a
# run restarted e1504g's live formalshell.service onto the nested display,
# which took it down with the session (2026-10-09).
session_env=(
  "HYPRLAND_NO_SD_VARS=1"
  "PATH=$sd_shim_dir:$PATH"
  "HOME=$iso_home"
  "XDG_CONFIG_HOME=$iso_home/.config"
  "XDG_STATE_HOME=$iso_home/.local/state"
  "XDG_DATA_HOME=$iso_home/.local/share"
  "XDG_DATA_DIRS=$iso_home/.local/share"
  "XDG_CACHE_HOME=$iso_home/.cache"
)
if [ "$session_mode" = "nested" ]; then
  session_env+=("WAYLAND_DISPLAY=$wayland_display")
else
  # Unset, not empty: aquamarine reads WAYLAND_DISPLAY's presence, and an
  # empty value still sends it down the wayland backend, which then fails
  # to connect at all.
  session_env+=("AQ_DRM_DEVICES=$vkms_device")
  unset WAYLAND_DISPLAY
fi

echo "session mode: $session_mode${vkms_device:+ ($vkms_device)}"

# -k 10 force-kills 10s after the initial SIGTERM: Hyprland's SIGTERM
# handler can hang instead of exiting, and a stuck session would sit on the
# card (or on the host's screen) indefinitely. That only reaches the pid
# timeout started, and on NixOS that is /run/wrappers/bin/Hyprland, a
# capability wrapper whose real compositor can outlive it, reparented to
# pid 1 (e1504g, 2026-10-07). So the whole session runs in a transient user
# scope of its own and is torn down as a cgroup: every process in it gets
# SIGTERM, then SIGKILL five seconds on, wherever it was reparented to.
# systemd-run reaches the user manager over its private socket, never the
# session bus. Its fs-run- name puts a leftover under dev/scoped-run.sh's
# sweep in the VM.
hypr_unit="fs-run-hypr-$$"
session_scope=()
if systemd-run --user --scope --quiet --collect --unit="$hypr_unit-probe" true > /dev/null 2>&1; then
  session_scope=(systemd-run --user --scope --quiet --collect --unit="$hypr_unit"
    -p TimeoutStopSec=5 -p RuntimeMaxSec=$((session_timeout + 60)) --)
fi
env "${session_env[@]}" ${session_scope[@]+"${session_scope[@]}"} dbus-run-session -- \
  timeout -k 10 "$session_timeout" $hyprland_bin --config "$cfg" > "$hypr_log_path" 2>&1 || true
# The shell goes with its compositor and takes every child it started with
# it; read before the scopes below are stopped, which would kill them all
# regardless.
orphans=""
if [ -s "$shot_dir/shell-children.txt" ]; then
  for _ in $(seq 30); do
    orphans=$(while read -r pid _ args; do
      if [ -r "/proc/$pid/cmdline" ] && [ "$(tr '\0' ' ' < "/proc/$pid/cmdline" | sed 's/ $//')" = "$args" ]; then echo "$pid $args"; fi
    done < "$shot_dir/shell-children.txt")
    [ -z "$orphans" ] && break
    sleep 0.2
  done
fi
if [ -n "$shell_unit" ]; then systemctl --user stop "$shell_unit.scope" 2>/dev/null || true; fi
if [ ${#session_scope[@]} -gt 0 ]; then systemctl --user stop "$hypr_unit.scope" 2>/dev/null || true; fi
# The config path is this run's own, so anything still carrying it is a
# leftover of this session, whichever way it was started.
if leftover=$(pgrep -f -- "--config $cfg"); then
  kill -KILL $leftover 2>/dev/null || true
  echo "SMOKE_FAIL: the nested compositor outlived its session (pids $(echo $leftover)), killed" >&2
  exit 1
fi

host_notifications_owner_after=$(host_notifications_owner)
if [ "$host_notifications_owner_before" != "$host_notifications_owner_after" ]; then
  echo "SMOKE_FAIL: host org.freedesktop.Notifications owner PID changed ($host_notifications_owner_before -> $host_notifications_owner_after), the session's NotificationServer touched the host bus" >&2
  exit 1
fi

# Read before the EXIT trap's restore could hide it: the value the live
# shell would have been restarted onto mid-run. A session that never got
# to its shot fails below on its own logs.
host_wayland_mid=$(sed -n 's/^WAYLAND_DISPLAY=//p' "$host_env_mid_path" 2>/dev/null || true)
if [ -s "$host_env_mid_path" ] && [ "$host_wayland_mid" != "$host_wayland_before" ]; then
  echo "SMOKE_FAIL: host systemd user WAYLAND_DISPLAY changed mid-run ($host_wayland_before -> $host_wayland_mid), something in the session wrote the host user environment" >&2
  exit 1
fi
[ -s "$sd_shim_log" ] && sed 's/^/host env write /' "$sd_shim_log"

# Both logs, not just the compositor's: a session that came up fine with a
# shell that died at startup looks identical from the outside.
fail() {
  echo "SMOKE_FAIL: $1" >&2
  echo "--- hyprland log tail ---" >&2
  tail -20 "$hypr_log_path" >&2 2>/dev/null || true
  echo "--- shell log tail ---" >&2
  tail -20 "$shell_log_path" >&2 2>/dev/null || true
  exit 1
}

if [ -n "$orphans" ]; then
  fail "the shell left children running after it was gone: $(echo "$orphans" | tr "\n" ";")"
fi

# Printed ahead of the asserts so a failing leg still reports what the
# shell weighed at the end of its run.
if [ -f "$shot_dir/mem.txt" ]; then
  cat "$shot_dir/mem.txt"
fi

# out_w and out_h are the output's size in the pixels grim captures,
# out_scale its scale; every crop and probe a leg takes is cut from them.
need_jq
if ! read -r out_w out_h out_scale < <("$jq_bin" -r '.[0] | "\(.width) \(.height) \(.scale)"' "$output_json_path" 2>/dev/null) \
  || ! [[ "$out_w" =~ ^[0-9]+$ && "$out_h" =~ ^[0-9]+$ ]]; then
  fail "could not read the session's output size: $(cat "$output_json_path" 2>/dev/null)"
fi
export out_w out_h out_scale
echo "SMOKE_OUTPUT ${out_w}x${out_h} scale=$out_scale"
output_end=$("$jq_bin" -r '.[0] | "\(.width) \(.height) \(.scale)"' "$output_end_json_path" 2>/dev/null || true)
if [ -n "$output_end" ] && [ "$output_end" != "$out_w $out_h $out_scale" ]; then
  fail "the output changed size mid-run: $out_w $out_h $out_scale at start, $output_end at teardown"
fi

for leg_name in ${active_legs[@]+"${active_legs[@]}"}; do
  if declare -F "leg_${leg_name}_assert" >/dev/null; then "leg_${leg_name}_assert"; fi
done

# The shell logs every commit it makes, which is the evidence that a
# strip at rest commits nothing.
grep -E '^(start|commit|exit|ipc|text:|hyprland:|event loop:) ' "$shell_log_path" 2>/dev/null || true
if [ ${#active_legs[@]} -eq 0 ]; then
  # `debug dump` and `debug query` grow with the shell, so they are read
  # as data: every dump key must be one of the contract's and the numbers
  # this rig pins must read back, and a query answers with the launcher's
  # rows. Every other reply is matched byte for byte.
  [ -s "$rs_ipc_path" ] || fail "the rust drive wrote no debug replies"
  need_jq
  dump_keys='["compositor","available","workspaces","windows","focusedWindowId","heldFocusedWindowId","focusedWorkspaceId","fullscreenOutputs","configLoaded","audio","brightness","location","lyrics","herdr","iphone","localsend","airplay","bar","join","modals","toasts","frame","theme"]'
  dumps=$(awk 'prev=="> debug dump"{print} {prev=$0}' "$rs_ipc_path")
  [ "$(printf '%s\n' "$dumps" | wc -l)" -eq 2 ] || fail "expected two debug dump replies in $rs_ipc_path"
  check_dump() {
    printf '%s' "$1" | "$jq_bin" -e --argjson allowed "$dump_keys" --argjson join "$2" '
      (keys - $allowed) == []
      and (["compositor","configLoaded","join","bar","frame","theme"] - keys) == []
      and .compositor == "hyprland"
      and (.bar | length) == 1
      and (.frame | keys) == ["enabled","habit","requested","thickness"]
      and (.theme | del(.edgeInset)) == {"radius":10,"radiusXl":14,"borderWidth":1,"barPosition":"top"}
      and (.theme.edgeInset | keys) == ["bottom","left","right","top"]
      and (if $join == null then .join == null
           else (.join | del(.screen)) == $join and (.join.screen | type) == "string" end)' >/dev/null
  }
  first_dump=$(printf '%s\n' "$dumps" | sed -n 1p)
  second_dump=$(printf '%s\n' "$dumps" | sed -n 2p)
  check_dump "$first_dump" null || fail "debug dump (no join) carries a key outside the contract or a wrong value: $first_dump"
  check_dump "$second_dump" '{"edge":"top","x":100,"width":200,"reach":14}' || fail "debug dump (joined) carries a key outside the contract or a wrong value: $second_dump"
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
fi
for frame in "$shot_dir"/rs-*.png; do
  [ -f "$frame" ] || continue
  name=$(basename "$frame" .png | tr 'a-z-' 'A-Z_')
  echo "SMOKE_${name} $frame"
done

if [ -f "$shot_path" ]; then
  echo "SMOKE_OK $shot_path"
else
  fail "no screenshot produced"
fi
