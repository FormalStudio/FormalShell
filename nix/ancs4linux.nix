# ancs4linux (github.com/pzmarzly/ancs4linux): the ANCS/AMS bridge that
# turns an iPhone's Bluetooth LE notification and pairing traffic into two
# system D-Bus services, ancs4linux.Observer and ancs4linux.Advertising.
# GPL-2.0-or-later, not in nixpkgs as of 2026-09-28.
#
# The metadata patch (kbbahaPro/omarchy-iphone, MIT) adds the fields upstream
# drops on the floor: subtitle, category, the Silent/Important ANCS flags and
# the phone's own timestamp. the shell's iPhone service needs Silent for
# the Focus heuristic and category for routing, so this is applied
# unconditionally rather than left optional.
#
# The authorize patch lets the pairing agent accept AuthorizeService. BlueZ
# asks it for A2DP and AVRCP the moment a fresh bond lands, before anything
# can mark the phone trusted, and a rejection there fails the pairing on the
# phone's side. The agent is only registered while advertising, the window
# the user opened to pair.
#
# Busy retries, resubscribe handler cleanup and the reconnect advert are
# upstream as of pzmarzly/ancs4linux#21.
{ lib, python3, fetchFromGitHub, fetchpatch }:

python3.pkgs.buildPythonApplication rec {
  pname = "ancs4linux";
  version = "1.0.0-unstable-2026-10-23";
  pyproject = true;

  src = fetchFromGitHub {
    owner = "pzmarzly";
    repo = "ancs4linux";
    rev = "138451721ff10d3c8634b32656ae3d38a99b9163";
    hash = "sha256-m1OUrBCMgFRct7GDxrBHJE4Tu5Jdzh5W1AaboE/XcWI=";
  };

  patches = [
    (fetchpatch {
      url = "https://raw.githubusercontent.com/kbbahaPro/omarchy-iphone/586f37dce6aceef72376afb8be8bcc8a04de41fe/patches/ancs4linux-metadata.patch";
      hash = "sha256-PKHnQ9/412aMVdznqwQ2KDPgufDWLfbqFgexu4dvBwc=";
    })
    ./ancs4linux-authorize.patch
  ];

  build-system = [ python3.pkgs.hatchling ];

  dependencies = with python3.pkgs; [ dasbus pygobject3 typer ];

  # No tests shipped upstream; ci.sh is lint (black/isort/mypy) only.
  doCheck = false;

  pythonImportsCheck = [ "ancs4linux" ];

  meta = {
    description = "ANCS/AMS bridge exposing an iPhone's notifications and now-playing state as system D-Bus services";
    homepage = "https://github.com/pzmarzly/ancs4linux";
    license = lib.licenses.gpl2Plus;
    mainProgram = "ancs4linux-observer";
    platforms = lib.platforms.linux;
  };
}
