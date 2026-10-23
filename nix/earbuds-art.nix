# Product pictures for the earbuds panel: Nothing and CMF from ear-web
# (radiance-project/ear-web, the pictures its device pages show), AirPods
# from librepods' Android app (kavishdevar/librepods, res-apple/drawable).
# The originals run to ~250 KB each; nix/earbuds-art.py cuts them down to
# 256 px webps. dev/tarball.sh runs the same script.
{ stdenvNoCC, fetchFromGitHub, python3 }:
let
  ear-web = fetchFromGitHub {
    owner = "radiance-project";
    repo = "ear-web";
    rev = "d076e1371572fbf0a14b441f9b3cafbf9a237e04";
    hash = "sha256-Zz4C7/pOTPgrg9II826kEj3I1GZQeuZ6whq6uQd3PF8=";
  };
  librepods = fetchFromGitHub {
    owner = "kavishdevar";
    repo = "librepods";
    rev = "89356c08cea3db0b6b54792d9329d09a87cc85fa";
    hash = "sha256-R84mSviNoDAh5uQhUKBnsOf850E8fCHvDlWKWCBtZF4=";
  };
in
stdenvNoCC.mkDerivation {
  pname = "earbuds-art";
  version = "0-unstable-2026-10-21";

  dontUnpack = true;
  nativeBuildInputs = [ (python3.withPackages (p: [ p.pillow ])) ];

  installPhase = ''
    runHook preInstall
    python3 ${./earbuds-art.py} ${ear-web} ${librepods} $out/share/formalshell
    runHook postInstall
  '';
}
