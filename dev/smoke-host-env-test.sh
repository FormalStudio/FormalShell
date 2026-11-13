#!/usr/bin/env bash
# dev/smoke.sh's host environment restore against a stand-in systemctl:
# the manager the snapshot came from gets it back, a swapped one or none
# gets no write at all.
set -euo pipefail
cd "$(dirname "$0")/.."

work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
mkdir "$work/bin"
cat > "$work/bin/systemctl" <<'EOF'
#!/usr/bin/env bash
# MANAGER names the manager answering: a, b, or none.
[ "$MANAGER" != none ] || exit 1
case "$*" in
  "--user show --value -p UserspaceTimestampMonotonic") [ "$MANAGER" = a ] && echo 1000 || echo 2000 ;;
  "--user show-environment") printf 'XDG_RUNTIME_DIR=/run/user/%s\nPATH=/usr/bin\nWAYLAND_DISPLAY=wayland-1\n' "$MANAGER" ;;
  *) echo "$MANAGER $*" >> "$WRITES" ;;
esac
EOF
chmod +x "$work/bin/systemctl"
export PATH="$work/bin:$PATH" WRITES="$work/writes"

eval "$(sed -n '/^host_manager_id() {/,/^}/p; /^restore_host_env() {/,/^}/p' dev/smoke.sh)"
host_env_names="WAYLAND_DISPLAY PATH HYPRLAND_INSTANCE_SIGNATURE"

fail=0
check() {
  local name=$1 before=$2 after=$3 want=$4 got
  : > "$WRITES"
  export MANAGER=$before
  host_manager_before=$(host_manager_id)
  host_env_before=$(systemctl --user show-environment 2>/dev/null || true)
  export MANAGER=$after
  restore_host_env 2> "$work/log"
  got=$(cat "$WRITES")
  if [ "$got" = "$want" ]; then
    echo "ok $name"
  else
    echo "FAIL $name: wrote [$got], wanted [$want]; $(cat "$work/log")"
    fail=1
  fi
}

check "same manager restores" a a "a --user set-environment WAYLAND_DISPLAY=wayland-1
a --user set-environment PATH=/usr/bin
a --user unset-environment HYPRLAND_INSTANCE_SIGNATURE"
check "swapped manager is left alone" a b ""
check "no manager at the start, one at exit" none b ""
check "manager gone at exit" a none ""
exit $fail
