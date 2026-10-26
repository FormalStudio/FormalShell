#!/usr/bin/env bash
# Builds the release tarball install.sh unpacks: the shell, formalshell-ipc
# and the helpers no distro carries, for this machine's arch. Runs as root in
# a debian:bookworm container (`just tarball` on the mac, release.yml in CI):
# bookworm's glibc 2.36 is the floor every binary in the tarball is held to,
# and NEEDED may only name libraries every supported distro ships under the
# same soname.
#
#   dev/tarball.sh [OUTDIR]
#
# OUTDIR (default artifacts/tarball) gets formalshell-<arch>-linux.tar.gz.
# A tagged build sets VERSION; otherwise it is the crate version plus the
# commit.
set -euo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd)
out=${1:-$repo/artifacts/tarball}
work=${WORK:-/build}
arch=$(uname -m)
glibc_max=2.36
export DEBIAN_FRONTEND=noninteractive

apt-get update
apt-get install -y --no-install-recommends \
  ca-certificates curl git xz-utils unzip patch file binutils python3-pil \
  build-essential pkg-config clang libclang-dev nasm protobuf-compiler meson ninja-build \
  libpam0g-dev libpipewire-0.3-dev libfontconfig-dev libxkbcommon-dev libsystemd-dev \
  libdbus-1-dev libbluetooth-dev libpulse-dev libsqlite3-dev \
  libgtk-4-dev libadwaita-1-dev libepoxy-dev libwayland-dev wayland-protocols \
  gobject-introspection libgirepository1.0-dev valac

if [ -z "${VERSION:-}" ]; then
  VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo/crates/formalshell-rs/Cargo.toml" | head -1)
  rev=$(git -c safe.directory='*' -C "$repo" rev-parse --short HEAD 2>/dev/null || true)
  [ -n "$rev" ] && VERSION=$VERSION+$rev
fi

dl=$work/downloads
tp=$work/third_party
mkdir -p "$dl" "$tp"

# name, url, sha256, leading path components to strip
sources=(
  "lucide-font https://github.com/lucide-icons/lucide/releases/download/1.34.0/lucide-font-1.34.0.zip 1606dba9b2abe06168ee818bea7e66e3c5b881b56ee73e6331cee1cb4e096756 1"
  "nerd-fonts-symbols https://github.com/ryanoasis/nerd-fonts/releases/download/v3.4.0/NerdFontsSymbolsOnly.tar.xz 7f8c090da3b0eaa7108646bf34cbbb6ed13d5358a72460522108b06c7ecd716a 0"
  "ttfx https://github.com/omacom/ttfx/archive/refs/tags/v0.4.0.tar.gz 90a057971973917a45ae1cb2fc795cfaea33afc265180ba4a963172daf9f64ee 1"
  "tensaku https://github.com/jondkinney/tensaku/archive/refs/tags/v0.26.6.tar.gz 2387b38b5ac65fa61e3557fcf568b83585b15449e83d027bf860d4a0593fa17d 1"
  "gtk4-layer-shell https://github.com/wmww/gtk4-layer-shell/archive/refs/tags/v1.3.0.tar.gz 1ebb01ab14e98afd1727f68f64981c37bd23305b1f131f5667c02b94cf593192 1"
  "matugen https://github.com/InioX/matugen/archive/refs/tags/v4.1.0.tar.gz b46507be24d01a6233597077512501f21075fe4ce19b60b410354f439f569ddf 1"
  "clipssh https://github.com/samuellawrentz/clipssh/archive/c7f4e8ddcf102302c6375ba51534c7505ddc2616.tar.gz f4c388645830e3afd9adeae2b71173441ce3b3b97a852915457c043e9cc99afd 1"
  "localsend-cli https://github.com/0w0mewo/localsend-cli/archive/7865fb1cf26e4f782c6400167a7d218d69313cff.tar.gz 0661e232dd939ad3c36c29025aac75db212fbf909f2701ded69bbc590bc96ab3 1"
  "omarchy-iphone https://github.com/kbbahaPro/omarchy-iphone/archive/586f37dce6aceef72376afb8be8bcc8a04de41fe.tar.gz b26cf068e47705ab6046b14c56e08c716afbad07f4b2d2a685785ee71c137ecb 1"
  "nothingctl https://github.com/FormalSnake/nothingctl/archive/refs/tags/v0.2.0.tar.gz fc9e4219d3996ed7546ddc6b80f4b3595dda5416d695095ada7aa88a5f62a877 1"
  "openscq30 https://github.com/Oppzippy/OpenSCQ30/archive/refs/tags/v2.12.0.tar.gz 5c2509ea0dd71ab0b6b2d81948ec2850fc4d72a1239e4cda79dac2dbad7f3860 1"
  "earbuds https://github.com/JojiiOfficial/LiveBudsCli/archive/refs/tags/v0.2.0.tar.gz b0174d4207312bdf54f5cb586fff22984ecd5be54bd0205c4debf7927878ea70 1"
  "ear-web https://github.com/radiance-project/ear-web/archive/d076e1371572fbf0a14b441f9b3cafbf9a237e04.tar.gz 9c08f9ecbdc0924eb9e01416c17f01a02f363686b84b279cc6759f30b021d910 1"
  "librepods https://github.com/kavishdevar/librepods/archive/89356c08cea3db0b6b54792d9329d09a87cc85fa.tar.gz 508c24bc537602f6062778771d871033d43aee9ea101e011f2ec5ef1a366ca95 1"
)
case $arch in
  x86_64)
    sources+=("zig https://ziglang.org/download/0.16.0/zig-x86_64-linux-0.16.0.tar.xz 70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00 1")
    sources+=("go https://go.dev/dl/go1.27.1.linux-amd64.tar.gz 63d339f0da5ab53635a56f2490a7984dfe12dfcff22ad749f63edaf590168445 1") ;;
  aarch64)
    sources+=("zig https://ziglang.org/download/0.16.0/zig-aarch64-linux-0.16.0.tar.xz ea4b09bfb22ec6f6c6ceac57ab63efb6b46e17ab08d21f69f3a48b38e1534f17 1")
    sources+=("go https://go.dev/dl/go1.27.1.linux-arm64.tar.gz 3450b45a3f9ee8568792736a5c5e70a1f2e9b36c35a8f74958c03e51d7d92bec 1") ;;
  *) echo "no tarball build for $arch" >&2; exit 1 ;;
esac

for entry in "${sources[@]}"; do
  read -r name url sum strip <<<"$entry"
  file=$dl/$name-${url##*/}
  if ! { [ -f "$file" ] && echo "$sum  $file" | sha256sum -c --quiet 2>/dev/null; }; then
    curl -fsSL -o "$file" "$url"
    echo "$sum  $file" | sha256sum -c --quiet
  fi
  rm -rf "${tp:?}/$name"; mkdir -p "$tp/$name"
  case $file in
    *.zip) (cd "$tp/$name" && unzip -q "$file" && if [ "$strip" = 1 ]; then d=$(ls); mv "$d"/* . && rmdir "$d"; fi) ;;
    *) tar -xf "$file" -C "$tp/$name" --strip-components="$strip" ;;
  esac
done

# Radio Atlas's satellite globe, cut into tiles the way nix/blue-marble.nix does.
earth_url=https://eoimages.gsfc.nasa.gov/images/imagerecords/73000/73751/world.topo.bathy.200407.3x21600x10800.jpg
earth_sum=d225f1f35a6448a4d1d8f6de6e48f3433e470085b70a35800e64f384f269a7b0
earth=$dl/blue-marble-${earth_url##*/}
if ! { [ -f "$earth" ] && echo "$earth_sum  $earth" | sha256sum -c --quiet 2>/dev/null; }; then
  curl -fsSL -o "$earth" "$earth_url"
  echo "$earth_sum  $earth" | sha256sum -c --quiet
fi

# The cartridge theme's faces and their licences: the files and hashes
# nix/cartridge-fonts.nix pins, as google/fonts path, file name, sha256.
fonts_rev=e7db6ca603fd4862010383ff17ffd6cc4ffac1c0
fonts=(
  "archivo/Archivo%5Bwdth,wght%5D.ttf Archivo[wdth,wght].ttf 0e094a7d3c7c4c25cf1310c4b30014f1dae9332220b1c2c88f4fa996f0b05053"
  "archivo/OFL.txt archivo-OFL.txt 108b4e57c9c796d3d38d0428ca7ee39de47ad93187302718d9b2d8864b9b716b"
  "archivonarrow/ArchivoNarrow%5Bwght%5D.ttf ArchivoNarrow[wght].ttf adbe027f625c8393ae0f6e174e32e233dda485bc3eda5153ce428275394ef97f"
  "archivonarrow/OFL.txt archivonarrow-OFL.txt b2087ef3fb91248e346600d19021249152a9688efea72905aa1996bb16a4c8de"
  "bizudmincho/BIZUDMincho-Regular.ttf BIZUDMincho-Regular.ttf 468ee6d9b149ca144809e03841bf18740ecf014e055a00da6ecaf1aaf4165af2"
  "bizudmincho/BIZUDMincho-Bold.ttf BIZUDMincho-Bold.ttf 1f077f8f84c1e09d5c4acdd6828048180c2f733ae5ae13271f48cf01bee4ae83"
  "bizudmincho/OFL.txt bizudmincho-OFL.txt ef0ed524b3bd012f37015e986809eed6f67905456ebfc1ebd7fcc2765282ee01"
  "dotgothic16/DotGothic16-Regular.ttf DotGothic16-Regular.ttf 3ad9af88726d42b40f7f365f0dcac785af73cf20ea6f1d5b44e57cc21150b8f1"
  "dotgothic16/OFL.txt dotgothic16-OFL.txt b6630c61ea078cacd7fabe37d14ffe557a0b45b06683374a9aa9e24262993e33"
)
mkdir -p "$tp/cartridge-fonts"
for entry in "${fonts[@]}"; do
  read -r path name sum <<<"$entry"
  file=$tp/cartridge-fonts/$name
  if ! { [ -f "$file" ] && echo "$sum  $file" | sha256sum -c --quiet 2>/dev/null; }; then
    curl -fsSL -o "$file" "https://raw.githubusercontent.com/google/fonts/$fonts_rev/ofl/$path"
    echo "$sum  $file" | sha256sum -c --quiet
  fi
done

export CARGO_HOME=$work/cargo RUSTUP_HOME=$work/rustup
export PATH=$CARGO_HOME/bin:$tp/go/bin:$tp/zig:$PATH
if ! command -v cargo >/dev/null; then
  curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable --no-modify-path
fi
host=$(rustc -vV | sed -n 's/^host: //p')
export CARGO_TARGET_DIR=$work/target

# tensaku's gtk4-layer-shell crate wants the C library at 1.3, which bookworm
# does not carry; built static, it adds nothing to NEEDED.
gls=$work/gls
if [ ! -f "$gls/lib/pkgconfig/gtk4-layer-shell-0.pc" ] && [ ! -f "$gls/lib/$host/pkgconfig/gtk4-layer-shell-0.pc" ]; then
  (cd "$tp/gtk4-layer-shell" && rm -rf build \
    && meson setup build --prefix="$gls" --libdir=lib -Ddefault_library=static \
      -Dexamples=false -Ddocs=false -Dtests=false -Dintrospection=false -Dvapi=false \
    && ninja -C build install)
fi
export PKG_CONFIG_PATH=$gls/lib/pkgconfig

(cd "$repo/crates" && cargo build --locked --release --package formalshell-rs)
release=$CARGO_TARGET_DIR/release

# The x86-64 engine's asm objects call libc with PC32 relocations, which a
# PIE link rejects (nix/ttfx-package.nix). An explicit --target keeps the
# static model off the proc-macros.
(cd "$tp/ttfx" && RUSTFLAGS="-C relocation-model=static" \
  cargo build --locked --release --target "$host" --target-dir "$work/target-ttfx")
rm -f "$tp/tensaku/rust-toolchain.toml"
(cd "$tp/tensaku" && cargo build --locked --release --features ci-release)
(cd "$tp/matugen" && cargo build --locked --release)
(cd "$tp/nothingctl" && cargo build --locked --release)
(cd "$tp/openscq30" && cargo build --locked --release --package openscq30-cli)
(cd "$tp/earbuds" && cargo build --locked --release)
(cd "$repo/tools/eds" && zig build --global-cache-dir "$work/zig-cache" --release=safe -Dcpu=baseline --prefix "$work/eds")

# Same fix as nix/localsend-cli.nix: without a Content-Type every send is
# rejected by a fiber receiver, this CLI's own `recv` included.
(cd "$tp/localsend-cli" \
  && sed -i 's/req.SetBodyRaw(metaJson)/req.Header.SetContentType("application\/json"); req.SetBodyRaw(metaJson)/' \
    internal/localsend/send/fwdsend.go \
  && grep -q 'SetContentType("application/json")' internal/localsend/send/fwdsend.go \
  && CGO_ENABLED=0 GOTOOLCHAIN=local GOPATH=$work/gopath GOFLAGS=-modcacherw \
    go build -trimpath -buildvcs=false -o localsend-cli .)
(cd "$tp/omarchy-iphone" && patch -Np1 -i "$repo/nix/iphone-ams-subscribe.patch" \
  && patch -Np1 -i "$repo/nix/iphone-bridge-bond.patch")

stage=$work/stage/formalshell
rm -rf "$work/stage"
mkdir -p "$stage/bin" "$stage/lib/formalshell/bin" "$stage/share/formalshell/fonts"
lib=$stage/lib/formalshell
share=$stage/share/formalshell

install -m755 "$release/formalshell-rs" "$lib/formalshell-rs"
install -m755 "$release/formalshell-ipc" "$stage/bin/formalshell-ipc"
install -m755 "$work/target-ttfx/$host/release/ttfx" "$lib/bin/ttfx"
install -m755 "$CARGO_TARGET_DIR/release/tensaku" "$lib/bin/tensaku"
install -m755 "$tp/tensaku/assets/tensaku-edit" "$lib/bin/tensaku-edit"
install -m755 "$CARGO_TARGET_DIR/release/matugen" "$lib/bin/matugen"
install -m755 "$CARGO_TARGET_DIR/release/nothingctl" "$lib/bin/nothingctl"
install -m755 "$CARGO_TARGET_DIR/release/openscq30" "$lib/bin/openscq30"
install -m755 "$CARGO_TARGET_DIR/release/earbuds" "$lib/bin/earbuds"
install -m755 "$work/eds/bin/formalshell-eds" "$lib/bin/formalshell-eds"
install -m755 "$tp/localsend-cli/localsend-cli" "$lib/bin/localsend-cli"
install -m755 "$tp/clipssh/clipssh" "$lib/bin/clipssh"
install -m755 "$tp/omarchy-iphone/bin/omarchy-iphone-bridge" "$tp/omarchy-iphone/bin/omarchy-iphone-ams" "$lib/bin/"

cp -R "$repo/crates/fs-theme/templates" "$share/templates"
cp -R "$repo/branding" "$share/branding"
cp -R "$repo/docs/examples" "$share/examples"
install -m644 "$tp/lucide-font/lucide.ttf" "$share/fonts/lucide.ttf"
install -m644 "$tp/nerd-fonts-symbols/SymbolsNerdFont-Regular.ttf" "$tp/nerd-fonts-symbols/SymbolsNerdFontMono-Regular.ttf" "$share/fonts/"
install -m644 "$tp/cartridge-fonts/"*.ttf "$share/fonts/"
mkdir -p "$share/licenses"
install -m644 "$tp/cartridge-fonts/"*-OFL.txt "$share/licenses/"
# Both image passes run on one core for minutes, and their output only moves
# with the pinned sources and the scripts, so a WORK kept between builds
# reuses it.
art_key=$({ printf '%s\n' "${sources[@]}" "$earth_sum"; cat "$repo/nix/earth-tiles.py" "$repo/nix/earbuds-art.py"; } | sha256sum | cut -c1-16)
art=$work/art/$art_key
if [ ! -f "$art/done" ]; then
  rm -rf "$work/art"; mkdir -p "$art"
  python3 "$repo/nix/earth-tiles.py" "$earth" "$art"
  python3 "$repo/nix/earbuds-art.py" "$tp/ear-web" "$tp/librepods" "$art"
  touch "$art/done"
fi
cp -R "$art/earth.jpg" "$art/tiles" "$art/earbuds" "$share/"
install -m644 "$repo/LICENSE" "$share/LICENSE"
echo "$VERSION" > "$share/VERSION"

# The helpers come first on PATH so a distro's own matugen or ttfx, at
# another version, never renders the shell's templates.
cat > "$stage/bin/formalshell" <<'EOF'
#!/bin/sh
prefix=$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)
export FS_RS_ICON_FONT="${FS_RS_ICON_FONT-$prefix/share/formalshell/fonts/lucide.ttf}"
export FS_RS_FONT_DIRS="${FS_RS_FONT_DIRS-$prefix/share/formalshell/fonts}"
export FS_TEMPLATE_DIR="${FS_TEMPLATE_DIR-$prefix/share/formalshell/templates}"
export FS_BRANDING_DIR="${FS_BRANDING_DIR-$prefix/share/formalshell/branding}"
export FS_EARTH_IMAGE="${FS_EARTH_IMAGE-$prefix/share/formalshell/earth.jpg}"
export FS_EARTH_TILES="${FS_EARTH_TILES-$prefix/share/formalshell/tiles}"
export FS_EARBUDS_ART="${FS_EARBUDS_ART-$prefix/share/formalshell/earbuds}"
export PATH="$prefix/lib/formalshell/bin:$prefix/bin:$PATH"
exec "$prefix/lib/formalshell/formalshell-rs" "$@"
EOF
cat > "$stage/bin/formalshell-greeter" <<'EOF'
#!/bin/sh
exec "$(dirname "$(readlink -f "$0")")/formalshell" greeter "$@"
EOF
sed -e 's|@IPC@|"$(dirname "$(readlink -f "$0")")/formalshell-ipc"|' \
  -e 's|@TIMEOUT@|timeout|' -e 's|@SYSTEMCTL@|systemctl|' \
  "$repo/nix/formalshell-watchdog.in" > "$stage/bin/formalshell-watchdog"
chmod 755 "$stage/bin/formalshell" "$stage/bin/formalshell-greeter" "$stage/bin/formalshell-watchdog"

# Every soname here is in the base install or a package install.sh's table
# pulls in, on Arch, Debian and Fedora alike.
allowed='^(ld-linux-[a-z0-9_-]+\.so\.[0-9]+|libc\.so\.6|libm\.so\.6|libdl\.so\.2|librt\.so\.1|libpthread\.so\.0|libgcc_s\.so\.1|libstdc\+\+\.so\.6|libpam\.so\.0|libpipewire-0\.3\.so\.0|libfontconfig\.so\.1|libfreetype\.so\.6|libxkbcommon\.so\.0|libdbus-1\.so\.3|libsystemd\.so\.0|libpulse\.so\.0|libbluetooth\.so\.3|libsqlite3\.so\.0|libwayland-client\.so\.0|libgtk-4\.so\.1|libadwaita-1\.so\.0|libgio-2\.0\.so\.0|libglib-2\.0\.so\.0|libgobject-2\.0\.so\.0|libcairo\.so\.2|libcairo-gobject\.so\.2|libpango-1\.0\.so\.0|libpangocairo-1\.0\.so\.0|libgdk_pixbuf-2\.0\.so\.0|libgraphene-1\.0\.so\.0|libepoxy\.so\.0|libharfbuzz\.so\.0)$'
bad=0
for f in "$stage"/bin/* "$lib"/formalshell-rs "$lib"/bin/*; do
  file -b "$f" | grep -q '^ELF' || continue
  needed=$(objdump -p "$f" | awk '/NEEDED/ { print $2 }')
  top=$({ objdump -T "$f" 2>/dev/null || true; } | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -uV | tail -1 || true)
  echo "linkage ${f#"$stage"/}: glibc ${top:-none}; $(echo "$needed" | tr '\n' ' ')"
  for n in $needed; do
    echo "$n" | grep -Eq "$allowed" || { echo "  unexpected NEEDED: $n" >&2; bad=1; }
  done
  if [ -n "$top" ] && [ "$(printf '%s\n%s\n' "$top" "$glibc_max" | sort -V | tail -1)" != "$glibc_max" ]; then
    echo "  needs glibc $top, newer than $glibc_max" >&2; bad=1
  fi
done
[ "$bad" = 0 ] || { echo "linkage check failed" >&2; exit 1; }

(cd "$stage" && find . -type f -o -type l | sed 's|^\./||' | sort > share/formalshell/MANIFEST && echo share/formalshell/MANIFEST >> share/formalshell/MANIFEST)
mkdir -p "$out"
tar -C "$work/stage" --owner=0 --group=0 -czf "$out/formalshell-$arch-linux.tar.gz" formalshell
echo "TARBALL $out/formalshell-$arch-linux.tar.gz ($VERSION)"
