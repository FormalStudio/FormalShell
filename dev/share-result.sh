#!/usr/bin/env bash
# Copies ./result into a directory every rig slot user can read, for
# `dev/remote.sh smoke-all`: one slot builds, the others run the same
# binaries with FS_RESULT pointing here. Slot homes are 0700, so ./result's
# links into this checkout and its crates/target are copied; the links into
# the installed fspec package (fonts, tiles, helpers) stay links.
#
#   dev/share-result.sh /tmp/formalshell-rig-result-<id>
set -euo pipefail
cd "$(dirname "$0")/.."
dest=$1
case "$dest" in /tmp/formalshell-rig-result-*) ;; *) echo "share-result: $dest is not /tmp/formalshell-rig-result-*" >&2; exit 1 ;; esac
[ -x result/bin/formalshell ] || { echo "share-result: no ./result, run dev/build.sh first" >&2; exit 1; }

# A smoke-all that was killed leaves its copy behind; a day is past any run.
find /tmp -maxdepth 1 -name 'formalshell-rig-result-*' -user "$(id -u)" -mmin +1440 -exec rm -rf {} + 2>/dev/null || true

rm -rf "$dest"
mkdir -m 755 "$dest"
cp -a result/. "$dest/"
find "$dest" -type l | while IFS= read -r link; do
  real=$(readlink -f "$link")
  case "$real" in
    "$HOME"/*) rm "$link"; cp -rL "$real" "$link" ;;
  esac
done
chmod -R a+rX "$dest"
echo "shared: $dest"
