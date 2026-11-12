#!/bin/bash
# Builds, tests and lints the committed branch from a clone of it, on the
# rig, with nothing beside it: no ShojiWM checkout, an empty target
# directory and an empty cargo home, so every crate comes from Cargo.lock
# over the network. Leaves nothing behind on either side.
#   wm/dev/fresh.sh
set -euo pipefail
guest=${FWM_GUEST:-fstest3@fstestmac}
here=$(cd "$(dirname "$0")/.." && pwd)
shell=$(dirname "$here")
clone="$shell-fresh-$$"
trap 'rm -rf "$clone"; ssh "$guest" "rm -rf fwm-build/fresh"' EXIT
git clone --quiet --depth 1 --single-branch --branch "$(git -C "$shell" branch --show-current)" "file://$shell" "$clone"
echo "clone at $(git -C "$clone" log -1 --format='%h %s')"
ssh "$guest" "rm -rf fwm-build/fresh && mkdir -p fwm-build/fresh/src fwm-build/fresh/target"
rsync -a --exclude .git "$clone/wm" "$clone/crates" "$guest:fwm-build/fresh/src/"
ssh "$guest" bash -s <<'REMOTE'
set -euo pipefail
run() {
  local status=0
  podman run --rm --userns=keep-id --security-opt label=disable \
    -v "$HOME/fwm-build/fresh/src:/src" -v "$HOME/fwm-build/fresh/target:/target" \
    -v "$HOME/shoji-cache/rustup:/rustup" \
    -e CARGO_HOME=/target/cargo -e RUSTUP_HOME=/rustup -e CARGO_TARGET_DIR=/target -e HOME=/tmp \
    -e CARGO_BUILD_JOBS=2 \
    -w /src/wm shoji-build cargo "$@" 2>&1 | grep -aE "^(warning|error)|test result|Finished|panicked" \
    || status=${PIPESTATUS[0]}
  echo "cargo $1: exit $status"
  return "$status"
}
echo "== build"; run build --locked --profile release-fast -p formalwm
echo "== test"; run test --locked --profile release-fast --workspace
echo "== clippy"; run clippy --locked --profile release-fast --workspace --all-targets -- -D warnings
ls -la "$HOME/fwm-build/fresh/target/release-fast/formalwm"
REMOTE
