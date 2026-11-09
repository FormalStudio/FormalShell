#!/usr/bin/env bash
# clippy over the workspace (its deny-level lints fail the run, warnings
# print), then the tests of every crate that needs no daemon and the shell's own (none of which
# need a session), on a Linux host
# (dev/remote.sh lint runs it on g815, ci.yml on a GitHub runner). Release
# profile throughout, so the tests share dev/build.sh's dependency builds.
# cargo runs in dev.star's environment where fspec is installed; CI sets up
# the same toolchain and libraries itself.
#
# The service crates' tests are `dev/check.sh <crate>`: they need a private
# bus or a VM.
set -euo pipefail
. "$(dirname "$0")/rig.sh"
cd "$rig_root/crates"

cargo() {
  if command -v fspec >/dev/null 2>&1; then fspec dev -- cargo "$@"; else command cargo "$@"; fi
}

# Every crate but the service crates, whose tests are dev/check.sh's.
# fs-audio stays: its unit tests need no daemon, and PipeWire is reached
# only by dev/check.sh's probe.
exclude=()
for crate in $rig_session_checks $rig_vm_checks; do
  [ "$crate" = fs-audio ] || exclude+=(--exclude "$crate")
done

cargo clippy --locked --release --workspace --all-targets
cargo test --locked --release --workspace "${exclude[@]}"
