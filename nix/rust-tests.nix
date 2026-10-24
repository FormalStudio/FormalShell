{ lib, rustCommon }:

# The pure crates' tests, which nix/package.nix leaves out. Some read
# fixtures relative to the repo root, so the source keeps the repo layout and
# cargo runs from crates/. Service crates (fs-mpris,
# fs-tray, ...) need a bus or a VM and carry checks of their own.
rustCommon.craneLib.cargoTest (rustCommon.checkArgs // {
  pname = "formalshell-rs-tests";
  src = lib.fileset.toSource {
    root = ../.;
    fileset = ../crates;
  };
  postUnpack = ''
    cd $sourceRoot/crates
    sourceRoot="."
  '';
  cargoExtraArgs = "--locked -p fs-js -p fs-chrome -p fs-info -p fs-system -p fs-devices -p fs-media -p fs-menu -p fs-screensaver -p fs-theme -p fs-audio";
})
