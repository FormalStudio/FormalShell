# Depth Anything V2 Small (onnx-community's export) behind nix/formalshell-depth.py,
# the wallpaper depth map writer. The model is pinned by hash and never fetched
# at runtime.
{ lib, fetchurl, python3, makeBinaryWrapper, stdenvNoCC }:
let
  model = fetchurl {
    url = "https://huggingface.co/onnx-community/depth-anything-v2-small/resolve/4472b7362082ad9968fee890ca0f1e5aca36b93d/onnx/model.onnx";
    sha256 = "afb6a5c28f3b6bf1618c6e43f02073ef9dfdc70e937502d51603e57b0a1df10c";
  };
  python = python3.withPackages (ps: [ ps.numpy ps.onnxruntime ps.pillow ]);
in
stdenvNoCC.mkDerivation {
  pname = "formalshell-depth";
  version = "0-unstable-2026-10-26";

  dontUnpack = true;
  nativeBuildInputs = [ makeBinaryWrapper ];

  installPhase = ''
    runHook preInstall
    install -Dm444 ${./formalshell-depth.py} $out/share/formalshell/formalshell-depth.py
    makeBinaryWrapper ${python}/bin/python3 $out/bin/formalshell-depth \
      --add-flags "-I $out/share/formalshell/formalshell-depth.py" \
      --set FORMALSHELL_DEPTH_MODEL ${model}
    runHook postInstall
  '';

  meta = {
    description = "Writes a depth map per wallpaper for the lock screen";
    mainProgram = "formalshell-depth";
    license = lib.licenses.mit;
  };
}
