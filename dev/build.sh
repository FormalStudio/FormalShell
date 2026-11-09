#!/usr/bin/env bash
# Builds the shell with cargo in dev.star's environment and lays out
# ./result the way the release tarball unpacks (dev/tarball.sh), which is
# what dev/smoke.sh runs:
#
#   result/bin/formalshell              the launcher, the way the tarball's is
#   result/bin/formalshell-ipc
#   result/lib/formalshell/formalshell-rs
#   result/lib/formalshell/bin          the helpers no distro carries
#   result/share/formalshell            fonts, globe tiles, earbuds art
#
# The binaries, templates, branding and examples come from this tree. The
# rest (the icon fonts, the Blue Marble tiles, ttfx,
# localsend-cli, tensaku, clipssh, the iPhone bridge, the earbuds CLIs) is
# linked out of the host's installed formalshell package, the same tarball
# at an older rev: none of it moves with the shell's code, and the tarball is
# the one place it is built.
#
#   dev/build.sh
#
# FS_CARGO_PROFILE=ship builds with the tarball's profile (crates/Cargo.toml)
# instead of release, at ~4 min a build.
set -euo pipefail
cd "$(dirname "$0")/.."
repo=$(pwd)

. "$repo/dev/rig.sh"

fail() {
  echo "build: $1" >&2
  exit 1
}

command -v fspec >/dev/null 2>&1 || fail "fspec is not on PATH, and the build runs in dev.star's environment"
installed=$(command -v formalshell 2>/dev/null) \
  || fail "no installed formalshell to take fonts, tiles and helpers from: add pkg(\"formalshell\") to this host's fspec spec"
pkg=$(cd "$(dirname "$(readlink -f "$installed")")/.." && pwd)
for need in lib/formalshell/bin share/formalshell/fonts share/formalshell/tiles; do
  [ -d "$pkg/$need" ] || fail "$pkg is not a release tarball's layout: no $need"
done

profile=${FS_CARGO_PROFILE:-release}
(cd crates && fspec dev -- cargo build --locked --profile "$profile" --package formalshell-rs)
release=$repo/crates/target/$profile

rm -rf result
mkdir -p result/bin result/lib/formalshell/first result/share/formalshell
ln -s "$release/formalshell-ipc" result/bin/formalshell-ipc
ln -s "$release/formalshell-rs" result/lib/formalshell/formalshell-rs
ln -s "$pkg/lib/formalshell/bin" result/lib/formalshell/bin
for entry in "$pkg"/share/formalshell/*; do
  case $(basename "$entry") in
    templates|branding|examples|MANIFEST|VERSION) ;;
    *) ln -s "$entry" "result/share/formalshell/$(basename "$entry")" ;;
  esac
done
ln -s "$repo/crates/fs-theme/templates" result/share/formalshell/templates
ln -s "$repo/branding" result/share/formalshell/branding
ln -s "$repo/docs/examples" result/share/formalshell/examples

# The tarball's launcher puts every helper ahead of PATH. Here only the ones
# whose version the shell's output depends on go first; the rest go last, so
# a leg's PATH shim (clipssh, localsend-cli, the iPhone bridge) is the one
# the shell runs.
for helper in matugen ttfx formalshell-eds; do
  [ -x "$pkg/lib/formalshell/bin/$helper" ] && ln -s "$pkg/lib/formalshell/bin/$helper" "result/lib/formalshell/first/$helper"
done
rig_launcher '$prefix/lib/formalshell/first:$prefix/bin:$PATH:$prefix/lib/formalshell/bin' > result/bin/formalshell
chmod 755 result/bin/formalshell
echo "result: $repo/result (helpers and art from $pkg)"
