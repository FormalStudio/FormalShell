# nothingctl (github.com/FormalSnake/nothingctl, MIT), the one process that
# speaks the Nothing protocol for the earbuds panel. Built the way the
# repo's own nix/package.nix builds it: bluer links libdbus.
{ lib, rustPlatform, fetchFromGitHub, pkg-config, dbus }:

rustPlatform.buildRustPackage rec {
  pname = "nothingctl";
  version = "0.2.0";

  src = fetchFromGitHub {
    owner = "FormalSnake";
    repo = "nothingctl";
    rev = "v${version}";
    hash = "sha256-Iv9JQVmtg3DawNk1q98OnCkzjQ7WIlfL33XU5ZNgkeQ=";
  };

  cargoHash = "sha256-mQpK/BBrBKwzxdXzJ3YUj3ftDYJLJg1dLYcSSvy+veg=";

  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ dbus ];

  meta = {
    description = "Control Nothing and CMF audio devices over Bluetooth";
    homepage = "https://github.com/FormalSnake/nothingctl";
    license = lib.licenses.mit;
    mainProgram = "nothingctl";
    platforms = lib.platforms.linux;
  };
}
