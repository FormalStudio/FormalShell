# The same tools as flake.nix's devShell, plus what a non-nix `cargo build` of the shell needs:
#   cd crates && cargo build --release --package formalshell-rs
# The shell is Linux only; `just build` stays the nix build.

# rustup's proxies act on the name they run as and install the pinned toolchain on first use.
RUST = ["cargo", "rustc", "rustdoc", "rustfmt", "cargo-fmt", "cargo-clippy", "clippy-driver"]

# matugen publishes an x86_64 Linux build only.
MATUGEN = [pkg("matugen", source = github("InioX/matugen", asset = "matugen-*-x86_64.tar.gz"))] if platform.arch == "x86_64" else []

dev(
    platforms = ["x86_64-linux", "aarch64-linux"],
    packages = [pkg("just"), pkg("rustup", links = {name: "rustup" for name in RUST})] + MATUGEN,
    # fs-audio's PipeWire client binds libpipewire with bindgen, which loads libclang.
    # The lock screen authenticates through PAM.
    libs = [lib(n) for n in ["libpipewire-0.3", "libspa-0.2", "wayland-client", "xkbcommon", "fontconfig", "pam", "pam_misc"]],
    bins = [bin(n) for n in ["pkg-config", "gcc", "clang"]],
    env = {"RUSTUP_TOOLCHAIN": "1.99.1"},
)
