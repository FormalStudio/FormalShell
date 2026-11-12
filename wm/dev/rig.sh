#!/bin/bash
# Builds and tests formalwm on the rig's Shoji slot, where its headers are:
# copies wm/ and crates/fs-wm-proto to ~/fwm on the guest, laid out as here
# so the path between them holds, and runs cargo there in the `shoji-build`
# image with ~/fwm-build for a target directory. The ShojiWM worktree goes
# along for its own tests (--manifest-path /src/Developer/ShojiWM-wt/rs-sdk/Cargo.toml);
# formalwm builds against the rev its manifest names. ~/fwm-target/release-fast/formalwm is the build others run;
# `promote` puts the last build there, after it has been run.
#   wm/dev/rig.sh build --profile release-fast -p formalwm
#   wm/dev/rig.sh test -p formalwm-core
#   wm/dev/rig.sh promote
set -euo pipefail
guest=${FWM_GUEST:-fstest3@fstestmac}
here=$(cd "$(dirname "$0")/.." && pwd)
shell=$(dirname "$here")
sdk=${FWM_SHOJIWM:-$HOME/Developer/ShojiWM-wt/rs-sdk}
base=fwm/Developer
if [ "${1:-}" = promote ]; then
  exec ssh "$guest" 'set -e; for bin in formalwm fwm-hand fwm-layer fwm-drag; do
    cp "$HOME/fwm-build/release-fast/$bin" "$HOME/fwm-target/release-fast/$bin.new"
    mv -f "$HOME/fwm-target/release-fast/$bin.new" "$HOME/fwm-target/release-fast/$bin"
  done; ls -la "$HOME/fwm-target/release-fast/formalwm"'
fi
ssh "$guest" "mkdir -p $base/ecosystem/FormalShell-wt/shoji/crates $base/ShojiWM-wt/rs-sdk fwm-build"
rsync -a --delete --exclude target "$here/" "$guest:$base/ecosystem/FormalShell-wt/shoji/wm/"
rsync -a --delete --exclude target "$shell/crates/fs-wm-proto/" "$guest:$base/ecosystem/FormalShell-wt/shoji/crates/fs-wm-proto/"
rsync -a --delete --exclude target --exclude .git --exclude node_modules --exclude /docs --exclude /dist \
  "$sdk/" "$guest:$base/ShojiWM-wt/rs-sdk/"
# Two jobs at a time: the guest has 16G and no swap, and hangs when a build
# runs out of memory.
ssh "$guest" "bash -s -- $(printf '%q ' "$@")" <<'REMOTE'
exec podman run --rm --userns=keep-id --security-opt label=disable \
  -v "$HOME/fwm:/src" -v "$HOME/fwm-build:/target" \
  -v "$HOME/shoji-cache/cargo:/cargo" -v "$HOME/shoji-cache/rustup:/rustup" \
  -e CARGO_HOME=/cargo -e RUSTUP_HOME=/rustup -e CARGO_TARGET_DIR=/target -e HOME=/tmp \
  -e CARGO_BUILD_JOBS=2 \
  -w /src/Developer/ecosystem/FormalShell-wt/shoji/wm shoji-build cargo "$@"
REMOTE
