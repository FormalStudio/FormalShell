#!/bin/bash
# Builds and tests formalwm on the rig's Shoji slot, where its headers are:
# copies wm/, crates/fs-wm-proto and the ShojiWM worktree it depends on to
# ~/fwm on the guest, laid out as here so the path dependencies hold, and
# runs cargo there in the `shoji-build` image with a target directory of its
# own.
#   wm/dev/rig.sh build --profile release-fast -p formalwm
#   wm/dev/rig.sh test -p formalwm-core
set -euo pipefail
guest=${FWM_GUEST:-fstest3@fstestmac}
here=$(cd "$(dirname "$0")/.." && pwd)
shell=$(dirname "$here")
sdk=${FWM_SHOJIWM:-$HOME/Developer/ShojiWM-wt/rs-sdk}
base=fwm/Developer
ssh "$guest" "mkdir -p $base/ecosystem/FormalShell-wt/shoji/crates $base/ShojiWM-wt/rs-sdk fwm-target"
rsync -a --delete --exclude target "$here/" "$guest:$base/ecosystem/FormalShell-wt/shoji/wm/"
rsync -a --delete --exclude target "$shell/crates/fs-wm-proto/" "$guest:$base/ecosystem/FormalShell-wt/shoji/crates/fs-wm-proto/"
rsync -a --delete --exclude target --exclude .git --exclude node_modules --exclude /docs --exclude /dist \
  "$sdk/" "$guest:$base/ShojiWM-wt/rs-sdk/"
ssh "$guest" "bash -s -- $(printf '%q ' "$@")" <<'REMOTE'
exec podman run --rm --userns=keep-id --security-opt label=disable \
  -v "$HOME/fwm:/src" -v "$HOME/fwm-target:/target" \
  -v "$HOME/shoji-cache/cargo:/cargo" -v "$HOME/shoji-cache/rustup:/rustup" \
  -e CARGO_HOME=/cargo -e RUSTUP_HOME=/rustup -e CARGO_TARGET_DIR=/target -e HOME=/tmp \
  -w /src/Developer/ecosystem/FormalShell-wt/shoji/wm shoji-build cargo "$@"
REMOTE
