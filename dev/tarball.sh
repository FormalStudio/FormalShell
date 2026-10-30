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
  "cpython https://www.python.org/ftp/python/3.12.16/Python-3.12.16.tar.xz b5796915dc45ff745cfb4e990a7e7c93d6f546328a102ce31448d69ebc37575c 1"
)
case $arch in
  x86_64)
    sources+=("zig https://ziglang.org/download/0.16.0/zig-x86_64-linux-0.16.0.tar.xz 70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00 1")
    sources+=("wheel-numpy https://files.pythonhosted.org/packages/43/a3/c799c62e19c337e6d3770b08e475887fb30ce8477d3c09efca6b2f0228a6/numpy-2.5.4-cp312-cp312-manylinux_2_27_x86_64.manylinux_2_28_x86_64.whl fbde6962867ee75b48b0ee29b2b9372ec5d617799dbaf38e82dc0596f2f7738a 0")
    sources+=("wheel-pillow https://files.pythonhosted.org/packages/e6/99/fdf65cc539e266ab93546c261bdf98411fe165c6e4e13f9dbf597097192b/pillow-13.0.0-cp312-cp312-manylinux_2_27_x86_64.manylinux_2_28_x86_64.whl 42d63be3af4f9b951b199b0a9d23365d37ab14687e6d77b74940432c8515c4ca 0")
    sources+=("wheel-onnxruntime https://files.pythonhosted.org/packages/eb/27/c6a823bac55b2977bea2027c3ec9393a2c1002cb9a09eb40354563e02730/onnxruntime-1.31.1-cp312-cp312-manylinux_2_28_x86_64.whl 1f3dea47f2f0e5117c2404cea7d21fd253ae7836a3bf167a501cdfbb2600d536 0")
    sources+=("go https://go.dev/dl/go1.27.1.linux-amd64.tar.gz 63d339f0da5ab53635a56f2490a7984dfe12dfcff22ad749f63edaf590168445 1") ;;
  aarch64)
    sources+=("zig https://ziglang.org/download/0.16.0/zig-aarch64-linux-0.16.0.tar.xz ea4b09bfb22ec6f6c6ceac57ab63efb6b46e17ab08d21f69f3a48b38e1534f17 1")
    sources+=("wheel-numpy https://files.pythonhosted.org/packages/dc/96/8319e2457ae4333c62c815c7006b869a4f60985c1e01024c2f8c6c040fe5/numpy-2.5.4-cp312-cp312-manylinux_2_27_aarch64.manylinux_2_28_aarch64.whl fe4d21ab149f15e4e6043dfb0de87e6e5f34ac176cde83060e9802981fca2ac2 0")
    sources+=("wheel-pillow https://files.pythonhosted.org/packages/3b/bb/9e30e8ed18c20800ee327a12107d5249ad8dc444867b3adfcc21654e3c71/pillow-13.0.0-cp312-cp312-manylinux_2_27_aarch64.manylinux_2_28_aarch64.whl 8bf295fe459343bc6093c8e5e0df60bfa189e0fcb775a22e2c2488fdeccc6c0d 0")
    sources+=("wheel-onnxruntime https://files.pythonhosted.org/packages/ba/e4/1ed1a7a5f4702d899091fa2b976d377995250c307326ec8dc9ae6378aa13/onnxruntime-1.31.1-cp312-cp312-manylinux_2_28_aarch64.whl 0b9a492176886992e0e875f06bc0367e333f5fff11711a2d642e18b02f3009cc 0")
    sources+=("go https://go.dev/dl/go1.27.1.linux-arm64.tar.gz 3450b45a3f9ee8568792736a5c5e70a1f2e9b36c35a8f74958c03e51d7d92bec 1") ;;
  *) echo "no tarball build for $arch" >&2; exit 1 ;;
esac

for entry in "${sources[@]}"; do
  read -r name url sum strip <<<"$entry"
  file=$dl/$name-${url##*/}
  if ! { [ -f "$file" ] && echo "$sum  $file" | sha256sum -c --quiet 2>/dev/null; }; then
    curl -fsSL --retry 5 --retry-all-errors -o "$file" "$url"
    echo "$sum  $file" | sha256sum -c --quiet
  fi
  rm -rf "${tp:?}/$name"; mkdir -p "$tp/$name"
  case $file in
    *.zip|*.whl) (cd "$tp/$name" && unzip -q "$file" && if [ "$strip" = 1 ]; then d=$(ls); mv "$d"/* . && rmdir "$d"; fi) ;;
    *) tar -xf "$file" -C "$tp/$name" --strip-components="$strip" ;;
  esac
done

# Radio Atlas's satellite globe, cut into tiles by packaging/earth-tiles.py.
earth_url=https://eoimages.gsfc.nasa.gov/images/imagerecords/73000/73751/world.topo.bathy.200407.3x21600x10800.jpg
earth_sum=d225f1f35a6448a4d1d8f6de6e48f3433e470085b70a35800e64f384f269a7b0
earth=$dl/blue-marble-${earth_url##*/}
if ! { [ -f "$earth" ] && echo "$earth_sum  $earth" | sha256sum -c --quiet 2>/dev/null; }; then
  curl -fsSL --retry 5 --retry-all-errors -o "$earth" "$earth_url"
  echo "$earth_sum  $earth" | sha256sum -c --quiet
fi

# formalshell-depth's model: Depth Anything V2 Small (Apache-2.0), the int8
# export, 27 MB where the fp32 one is 99. On a photograph its map is 0.0033
# off the fp32 map on average (0..1 scale), 0.043 at worst.
depth_url=https://huggingface.co/onnx-community/depth-anything-v2-small/resolve/4472b7362082ad9968fee890ca0f1e5aca36b93d/onnx/model_quantized.onnx
depth_sum=fcf51f1b230362b28690bb9d1809bf0431f29cad20534e3f589bd7285547f20d
depth_model=$dl/depth-anything-v2-small-model_quantized.onnx
if ! { [ -f "$depth_model" ] && echo "$depth_sum  $depth_model" | sha256sum -c --quiet 2>/dev/null; }; then
  curl -fsSL --retry 5 --retry-all-errors -o "$depth_model" "$depth_url"
  echo "$depth_sum  $depth_model" | sha256sum -c --quiet
fi

export CARGO_HOME=$work/cargo RUSTUP_HOME=$work/rustup
export PATH=$CARGO_HOME/bin:$tp/go/bin:$tp/zig:$PATH
if ! command -v cargo >/dev/null; then
  curl -fsSL --retry 5 --retry-all-errors https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable --no-modify-path
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
# PIE link rejects. An explicit --target keeps the
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

# Without a Content-Type every send is rejected by a fiber receiver, this
# CLI's own `recv` included.
(cd "$tp/localsend-cli" \
  && sed -i 's/req.SetBodyRaw(metaJson)/req.Header.SetContentType("application\/json"); req.SetBodyRaw(metaJson)/' \
    internal/localsend/send/fwdsend.go \
  && grep -q 'SetContentType("application/json")' internal/localsend/send/fwdsend.go \
  && CGO_ENABLED=0 GOTOOLCHAIN=local GOPATH=$work/gopath GOFLAGS=-modcacherw \
    go build -trimpath -buildvcs=false -o localsend-cli .)
(cd "$tp/omarchy-iphone" && patch -Np1 -i "$repo/packaging/iphone-ams-subscribe.patch" \
  && patch -Np1 -i "$repo/packaging/iphone-bridge-bond.patch")

# formalshell-depth's interpreter, built here rather than taken from
# python-build-standalone: those builds link libutil.so.1, which Arch's glibc
# no longer ships. Bookworm's glibc binds openpty to libc, so NEEDED stays
# libc and libm. Static libpython, no ensurepip, no tests.
py=$work/cpython-3.12.16-1
if [ ! -x "$py/bin/python3" ]; then
  rm -rf "$py"
  (cd "$tp/cpython" && ./configure --prefix="$py" --without-ensurepip --disable-test-modules --disable-shared >/dev/null \
    && make -j"$(nproc)" >/dev/null && make install >/dev/null)
fi

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

# formalshell-depth runs on that CPython with numpy, pillow and onnxruntime
# unpacked from hash-pinned manylinux wheels, so no distro's python version
# or packages matter. Nothing in the shell itself links any of it.
mkdir -p "$lib/depth" "$share/depth"
cp -R "$py" "$lib/depth/python"
site=$lib/depth/python/lib/python3.12/site-packages
for wheel in numpy pillow onnxruntime; do cp -R "$tp/wheel-$wheel"/. "$site/"; done
(cd "$lib/depth/python" \
  && rm -rf include share lib/pkgconfig lib/libpython3.12.a lib/python3.12/config-3.12-* lib/python3.12/ensurepip lib/python3.12/pydoc_data \
    bin/idle* bin/pydoc* bin/2to3* bin/python3-config bin/python3.*-config \
    lib/python3.12/idlelib lib/python3.12/tkinter lib/python3.12/turtledemo lib/python3.12/lib2to3 \
    lib/python3.12/lib-dynload/_tkinter* lib/python3.12/lib-dynload/_curses* lib/python3.12/lib-dynload/_dbm* \
    lib/python3.12/lib-dynload/_gdbm* lib/python3.12/lib-dynload/_ctypes* lib/python3.12/lib-dynload/_crypt* \
    lib/python3.12/lib-dynload/_lzma* lib/python3.12/lib-dynload/_uuid* lib/python3.12/lib-dynload/nis* \
  && strip --strip-unneeded bin/python3.12 lib/python3.12/lib-dynload/*.so \
  && find . -name __pycache__ -prune -exec rm -rf {} + \
  && find lib/python3.12/site-packages \( -name tests -o -name test \) -type d -prune -exec rm -rf {} +)
install -m644 "$repo/packaging/formalshell-depth.py" "$lib/depth/formalshell-depth.py"
install -m644 "$depth_model" "$share/depth/model.onnx"
cat > "$lib/bin/formalshell-depth" <<'EOF'
#!/bin/sh
here=$(dirname "$(readlink -f "$0")")
prefix=$(cd "$here/../../.." && pwd)
export FORMALSHELL_DEPTH_MODEL="${FORMALSHELL_DEPTH_MODEL-$prefix/share/formalshell/depth/model.onnx}"
exec "$prefix/lib/formalshell/depth/python/bin/python3" -I "$prefix/lib/formalshell/depth/formalshell-depth.py" "$@"
EOF
chmod 755 "$lib/bin/formalshell-depth"

cp -R "$repo/crates/fs-theme/templates" "$share/templates"
cp -R "$repo/branding" "$share/branding"
cp -R "$repo/docs/examples" "$share/examples"
install -m644 "$tp/lucide-font/lucide.ttf" "$share/fonts/lucide.ttf"
install -m644 "$tp/nerd-fonts-symbols/SymbolsNerdFont-Regular.ttf" "$tp/nerd-fonts-symbols/SymbolsNerdFontMono-Regular.ttf" "$share/fonts/"
# Both image passes run on one core for minutes, and their output only moves
# with the pinned sources and the scripts, so a WORK kept between builds
# reuses it.
art_key=$({ printf '%s\n' "${sources[@]}" "$earth_sum"; cat "$repo/packaging/earth-tiles.py" "$repo/packaging/earbuds-art.py"; } | sha256sum | cut -c1-16)
art=$work/art/$art_key
if [ ! -f "$art/done" ]; then
  rm -rf "$work/art"; mkdir -p "$art"
  python3 "$repo/packaging/earth-tiles.py" "$earth" "$art"
  python3 "$repo/packaging/earbuds-art.py" "$tp/ear-web" "$tp/librepods" "$art"
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
  "$repo/packaging/formalshell-watchdog.in" > "$stage/bin/formalshell-watchdog"
chmod 755 "$stage/bin/formalshell" "$stage/bin/formalshell-greeter" "$stage/bin/formalshell-watchdog"

# Every soname here is in the base install or a package install.sh's table
# pulls in, on Arch, Debian and Fedora alike.
allowed='^(ld-linux-[a-z0-9_-]+\.so\.[0-9]+|libc\.so\.6|libm\.so\.6|libdl\.so\.2|librt\.so\.1|libpthread\.so\.0|libgcc_s\.so\.1|libstdc\+\+\.so\.6|libpam\.so\.0|libpipewire-0\.3\.so\.0|libfontconfig\.so\.1|libfreetype\.so\.6|libxkbcommon\.so\.0|libdbus-1\.so\.3|libsystemd\.so\.0|libpulse\.so\.0|libbluetooth\.so\.3|libsqlite3\.so\.0|libwayland-client\.so\.0|libgtk-4\.so\.1|libadwaita-1\.so\.0|libgio-2\.0\.so\.0|libglib-2\.0\.so\.0|libgobject-2\.0\.so\.0|libcairo\.so\.2|libcairo-gobject\.so\.2|libpango-1\.0\.so\.0|libpangocairo-1\.0\.so\.0|libgdk_pixbuf-2\.0\.so\.0|libgraphene-1\.0\.so\.0|libepoxy\.so\.0|libharfbuzz\.so\.0|libz\.so\.1)$'
bad=0
# The depth tree's wheels vendor their own libraries; those are fine to
# need, and are held to the same glibc floor.
for f in "$stage"/bin/* "$lib"/formalshell-rs "$lib"/bin/* $(find "$lib/depth" -type f \( -name '*.so' -o -name '*.so.*' -o -name 'python3*' \) -not -name '*.py' | sort); do
  file -b "$f" | grep -q '^ELF' || continue
  needed=$(objdump -p "$f" | awk '/NEEDED/ { print $2 }')
  top=$({ objdump -T "$f" 2>/dev/null || true; } | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -uV | tail -1 || true)
  echo "linkage ${f#"$stage"/}: glibc ${top:-none}; $(echo "$needed" | tr '\n' ' ')"
  for n in $needed; do
    echo "$n" | grep -Eq "$allowed" && continue
    case $f in "$lib"/depth/*) [ -n "$(find "$lib/depth" -name "${n##*/}" -print -quit)" ] && continue ;; esac
    echo "  unexpected NEEDED: $n" >&2; bad=1
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
