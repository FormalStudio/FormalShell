#!/usr/bin/env bash
# clippy over the workspace (its deny-level lints fail the run, warnings
# print), then the pure crates' tests, on a Linux host
# (dev/remote.sh lint runs it on g815, ci.yml on a GitHub runner). Release
# profile throughout, so the tests share dev/build.sh's dependency builds.
# cargo runs in dev.star's environment where fspec is installed; CI sets up
# the same toolchain and libraries itself.
#
# The service crates' tests are `dev/check.sh <crate>`: they need a private
# bus or a VM.
set -euo pipefail
cd "$(dirname "$0")/../crates"

cargo() {
  if command -v fspec >/dev/null 2>&1; then fspec dev -- cargo "$@"; else command cargo "$@"; fi
}

pure=()
for crate in fs-js fs-chrome fs-info fs-system fs-devices fs-media fs-menu fs-screensaver fs-theme fs-audio; do
  pure+=(--package "$crate")
done

cargo clippy --locked --release --workspace --all-targets
cargo test --locked --release "${pure[@]}"
