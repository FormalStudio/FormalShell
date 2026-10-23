# The cartridge theme's faces: Archivo (variable, wdth 62..125 and wght
# 100..900), Archivo Narrow, BIZ UD Mincho and DotGothic16, each the
# upright file and its licence from google/fonts at one pinned commit.
# dev/tarball.sh fetches the same files with the same hashes.
#
# OFL 1.1: shipped as plain font files beside the shell, never linked into
# or vendored by its source, so FormalShell stays MIT.
{ lib, stdenvNoCC, fetchurl }:
let
  rev = "e7db6ca603fd4862010383ff17ffd6cc4ffac1c0";
  # A store path name cannot carry the brackets and comma of a variable
  # font's file name.
  file = dir: name: hash: fetchurl {
    name = "${dir}-${builtins.replaceStrings [ "[" "]" "," ] [ "-" "" "-" ] name}";
    url = "https://raw.githubusercontent.com/google/fonts/${rev}/ofl/${dir}/${lib.escapeURL name}";
    inherit hash;
  };
  fonts = [
    { dir = "archivo"; name = "Archivo[wdth,wght].ttf"; hash = "sha256-DglKfTx8TCXPExDEswAU8drpMyIgscLIj0+plvCwUFM="; }
    { dir = "archivonarrow"; name = "ArchivoNarrow[wght].ttf"; hash = "sha256-rb4Cf2Jcg5OuD24XTjLiM92khbw+2lFTzkKCdTlO+X8="; }
    { dir = "bizudmincho"; name = "BIZUDMincho-Regular.ttf"; hash = "sha256-Ro7m2bFJyhRICeA4Qb8YdA7PAU4FWgDabsrxqvQWWvI="; }
    { dir = "bizudmincho"; name = "BIZUDMincho-Bold.ttf"; hash = "sha256-Hwd/j4TB4J1cSs3WgoBIGAwvczrlrhMnH0jPAb7kroM="; }
    { dir = "dotgothic16"; name = "DotGothic16-Regular.ttf"; hash = "sha256-OtmviHJtQrQPfzZfDcrHha9zzyDqbx1bROV8whFQuPE="; }
  ];
  licences = [
    { dir = "archivo"; hash = "sha256-EItOV8nHltPTjQQoyn7jneR62TGHMCcY2bLYhkubcWs="; }
    { dir = "archivonarrow"; hash = "sha256-sgh+8/uRJI40ZgDRkCEkkVKpaI7+pykFqhmWuxakyN4="; }
    { dir = "bizudmincho"; hash = "sha256-7w7VJLO9AS83AV6YaAnu1vZ5BUVuv8Hr1/zCdlKC7gE="; }
    { dir = "dotgothic16"; hash = "sha256-tmMMYeoHjKzX+r430U/+VXoLRbBmgzdKmqniQmKZPjM="; }
  ];
in
stdenvNoCC.mkDerivation {
  pname = "cartridge-fonts";
  version = "0-unstable-2026-10-23";

  dontUnpack = true;
  dontBuild = true;

  installPhase = ''
    runHook preInstall
    ${lib.concatMapStrings (f: ''
      install -Dm444 ${file f.dir f.name f.hash} "$out/share/fonts/truetype/${f.name}"
    '') fonts}
    ${lib.concatMapStrings (l: ''
      install -Dm444 ${file l.dir "OFL.txt" l.hash} "$out/share/licenses/cartridge-fonts/${l.dir}/OFL.txt"
    '') licences}
    runHook postInstall
  '';

  meta = {
    description = "Archivo, Archivo Narrow, BIZ UD Mincho and DotGothic16 for the cartridge theme";
    homepage = "https://github.com/google/fonts";
    license = lib.licenses.ofl;
    platforms = lib.platforms.all;
  };
}
