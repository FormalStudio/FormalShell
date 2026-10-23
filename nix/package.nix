{ lib, stdenvNoCC, rustCommon, makeBinaryWrapper, lucide-font, cartridge-fonts, blue-marble, earbuds-art, nerd-fonts, noto-fonts-color-emoji, matugen, brightnessctl, ddcutil, wlsunset
, wireplumber, cava, mpv, curl, util-linux, coreutils, procps, systemd, glib, pipewire, asusctl, uxplay, iphone-bridge, openscq30, nothingctl, earbuds
, formalshell-eds, git, qrencode, networkmanager, wl-clipboard, grim, slurp, wf-recorder, tesseract, ffmpeg-headless, pulseaudio, xdg-utils
, tensaku, ttfx, clipssh, localsend-cli, wtype, openssh }:

rustCommon.craneLib.buildPackage (rustCommon.commonArgs // {
  inherit (rustCommon) cargoArtifacts cargoVendorDir;
  pname = "formalshell";
  version = "0.2.0";

  src = lib.fileset.toSource {
    root = ../.;
    fileset = ../crates;
  };
  postUnpack = ''
    cd $sourceRoot/crates
    sourceRoot="."
  '';

  # The runtime package builds and tests itself alone; the pure crates'
  # tests are nix/rust-tests.nix.
  cargoExtraArgs = "--locked --package formalshell-rs";

  nativeBuildInputs = rustCommon.commonArgs.nativeBuildInputs ++ [ makeBinaryWrapper ];

  # text.rs's tests shape the cartridge faces, so the test run registers
  # them the way the wrapper below does at runtime.
  FS_RS_FONT_DIRS = "${cartridge-fonts}/share/fonts";

  # The icon fonts by path, registered with parley at startup.
  #
  # Suffixed rather than prefixed: anything a host install or the smoke rig's
  # PATH shims must be able to shadow (wtype, ssh and clipssh, which the
  # --spaces and --clipssh legs replace; uxplay, localsend-cli and the iPhone
  # bridge, every caller guarding with `command -v`), and the clients of a
  # daemon the host runs, which have to match that daemon's own build
  # (nmcli, pw-dump, busctl and systemctl, earbuds, asusctl).
  # util-linux is here for setpriv: every long-lived child is started under
  # PR_SET_PDEATHSIG, with no fallback when it is missing.
  #
  # The examples directory carries the Hyprland and mango configs the
  # home-manager module links (binds and layer rules); nothing in the shell
  # reads them.
  postInstall = ''
    mkdir -p $out/share/formalshell
    cp -r --no-preserve=mode ${../crates/fs-theme/templates} $out/share/formalshell/templates
    cp -r --no-preserve=mode ${../branding} $out/share/formalshell/branding
    cp -r --no-preserve=mode ${../docs/examples} $out/share/formalshell/examples
    wrapProgram $out/bin/formalshell-rs \
      --set-default FS_RS_ICON_FONT ${lucide-font}/share/fonts/truetype/lucide.ttf \
      --set-default FS_RS_FONT_DIRS ${nerd-fonts.symbols-only}/share/fonts:${noto-fonts-color-emoji}/share/fonts:${cartridge-fonts}/share/fonts \
      --set-default FS_TEMPLATE_DIR $out/share/formalshell/templates \
      --set-default FS_BRANDING_DIR $out/share/formalshell/branding \
      --set-default FS_EARTH_IMAGE ${blue-marble}/share/formalshell/earth.jpg \
      --set-default FS_EARTH_TILES ${blue-marble}/share/formalshell/tiles \
      --set-default FS_EARBUDS_ART ${earbuds-art}/share/formalshell/earbuds \
      --prefix PATH : ${lib.makeBinPath [ matugen brightnessctl ddcutil wlsunset wireplumber cava mpv curl util-linux procps git formalshell-eds qrencode wl-clipboard grim slurp wf-recorder tesseract ffmpeg-headless pulseaudio xdg-utils ttfx ]} \
      --suffix PATH : ${lib.makeBinPath ([ tensaku wtype openssh clipssh localsend-cli uxplay iphone-bridge networkmanager pipewire systemd glib ]
        ++ lib.optional (lib.meta.availableOn stdenvNoCC.hostPlatform asusctl) asusctl
        ++ lib.optionals (lib.meta.availableOn stdenvNoCC.hostPlatform earbuds) [ earbuds openscq30 ]
        ++ lib.optional (lib.meta.availableOn stdenvNoCC.hostPlatform nothingctl) nothingctl)}
    ln -s formalshell-rs $out/bin/formalshell

    # Liveness probe for the home-manager module's formalshell-watchdog timer
    # (nix/formalshell-watchdog.in has the why).
    substitute ${./formalshell-watchdog.in} $out/bin/formalshell-watchdog \
      --subst-var-by IPC $out/bin/formalshell-ipc \
      --subst-var-by TIMEOUT ${lib.getExe' coreutils "timeout"} \
      --subst-var-by SYSTEMCTL ${lib.getExe' systemd "systemctl"}
    chmod +x $out/bin/formalshell-watchdog
  '';

  meta = {
    description = "FormalShell, a Hyprland desktop shell";
    license = lib.licenses.mit;
    mainProgram = "formalshell";
    platforms = lib.platforms.linux;
  };
})
