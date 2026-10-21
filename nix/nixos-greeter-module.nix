# Greetd wiring (spec §Nix / §Surfaces 9: nixosModules.formalshell-greeter —
# "greetd wiring, system-side, can't be done from home-manager"). Generalizes
# the hand-rolled rig M8 Task 2 built directly in nix/testvm.nix into a
# module any consumer can enable; nix/testvm.nix now consumes this module
# itself rather than duplicating the declarations (same proof strategy as
# nix/nixos-module.nix).
#
# greetd's session/worker.rs execs default_session.command through
# `/bin/sh -c` with the environment reset to PAM's own envlist (confirmed by
# reading greetd's own source, not the nixpkgs module) — nothing this
# session's own systemd units would otherwise export reaches the script for
# free, so every env var the compositor or the greeter needs is set
# explicitly below. The `greeter` system user's real passwd entry has
# HOME=/var/empty (unwritable), hence stateDir/runtimeDir below and the
# tmpfiles rules that own them.
#
# The greeter binary runs in the script's *foreground*, not via the
# compositor's own `exec` line: greetd-ipc(7) only starts the authenticated
# session once this whole default_session command terminates, and running
# the greeter directly is what lets this script know the exact moment that
# happens (needed by postGreeterCommand below, and by M8 Task 2's smoke rig
# specifically) rather than racing an async compositor `exec`.
self: { config, lib, pkgs, ... }:
let
  cfg = config.services.formalshell-greeter;

  # greeter.sessionCommand (the argv the greeter starts on successful auth),
  # a real deployment's version of
  # the settings.json key the shell would otherwise read from a real user's
  # $HOME the `greeter` account doesn't have. Written as a static store path
  # and pointed at via XDG_CONFIG_HOME rather than any settings.json write:
  # this is a NixOS module generating a file at build time, not the shell
  # itself writing to its own config at runtime (CLAUDE.md's read-only rule
  # is about the latter).
  settingsDir = pkgs.writeTextDir "formalshell/settings.json" (builtins.toJSON (lib.recursiveUpdate cfg.settings {
    greeter = {
      inherit (cfg) sessionCommand sessionDirs;
    } // lib.optionalAttrs (cfg.wallpaper != null) { wallpaper = toString cfg.wallpaper; };
  }));

  # The greeter's own state dir, where it reads the mirrored wallpaper and
  # theme.json and keeps the session each user last started.
  greeterState = "${cfg.stateDir}/.local/state/formalshell";
  mirrorHome = config.users.users.${cfg.mirrorUser}.home;
  mirrorState = "${mirrorHome}/.local/state/formalshell";

  # The wallpaper is read as the user, so a path in state.json never reaches
  # a file only root can read.
  mirrorScript = pkgs.writeShellScript "formalshell-greeter-mirror" ''
    set -eu
    PATH=${lib.makeBinPath [ pkgs.coreutils pkgs.jq pkgs.util-linux pkgs.diffutils ]}
    install -d -o greeter -g greeter -m 0700 ${cfg.stateDir}/.local ${cfg.stateDir}/.local/state ${greeterState} ${greeterState}/avatars
    copy() {
      tmp="${greeterState}/$2.tmp"
      if runuser -u ${cfg.mirrorUser} -- cat -- "$1" > "$tmp" 2>/dev/null && [ -s "$tmp" ]; then
        if cmp -s "$tmp" "${greeterState}/$2"; then rm -f "$tmp"; return; fi
        chown greeter:greeter "$tmp"
        chmod 0600 "$tmp"
        mv -f "$tmp" "${greeterState}/$2"
      else
        rm -f "$tmp" "${greeterState}/$2"
      fi
    }
    wallpaper=$(runuser -u ${cfg.mirrorUser} -- jq -r '.wallpaper // empty' ${mirrorState}/state.json 2>/dev/null || true)
    copy "''${wallpaper:-/nonexistent}" wallpaper
    copy ${mirrorState}/theme.json theme.json
    copy ${mirrorHome}/.face avatars/${cfg.mirrorUser}
  '';

  compositorConfigFile = pkgs.writeText "formalshell-greeter-compositor.conf" cfg.compositorConfigText;

  sessionScript = pkgs.writeShellScript "formalshell-greeter-session" ''
    ${lib.optionalString (cfg.sessionLogFile != null) ''
      exec >>${lib.escapeShellArg cfg.sessionLogFile} 2>&1
      set -x
    ''}
    export XDG_RUNTIME_DIR=${lib.escapeShellArg cfg.runtimeDir}
    export HOME=${lib.escapeShellArg cfg.stateDir}
    export XDG_CONFIG_HOME=${settingsDir}
    ${lib.concatStringsSep "\n" (lib.mapAttrsToList
      (name: value: "export ${name}=${lib.escapeShellArg value}")
      cfg.extraEnvironment)}

    # WAYLAND_DISPLAY must stay unset going into this exec: wlr_backend_autocreate
    # checks getenv("WAYLAND_DISPLAY")/WAYLAND_SOCKET *before* trying DRM
    # (wlroots backend/backend.c) and, on a real seat with no WLR_BACKENDS
    # override, treats its mere presence as "we're nested inside another
    # Wayland session" — it then fails to connect to a socket nothing has
    # created yet and errors out instead of falling through to DRM
    # (confirmed by reproducing that exact failure directly). The compositor
    # picks its own socket name (wl_display_add_socket_auto), so discover
    # whichever one actually got created rather than assuming a fixed name.
    # A compositor killed on the last restart leaves its socket file behind,
    # and the search below would hand the greeter that dead one.
    rm -f "$XDG_RUNTIME_DIR"/wayland-*
    ${lib.getExe cfg.compositorPackage} --config ${compositorConfigFile} &
    compositor_pid=$!
    wayland_socket=""
    for _ in $(seq 1 100); do
      wayland_socket=$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -name 'wayland-*' ! -name '*.lock' -print -quit 2>/dev/null)
      [ -n "$wayland_socket" ] && break
      sleep 0.1
    done
    export WAYLAND_DISPLAY=$(basename "$wayland_socket")

    ${lib.getExe cfg.package}
    ${cfg.postGreeterCommand}

    kill "$compositor_pid" 2>/dev/null || true
    wait "$compositor_pid" 2>/dev/null || true
  '';
in
{
  options.services.formalshell-greeter = {
    enable = lib.mkEnableOption "the FormalShell greetd greeter";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.formalshell-greeter;
      defaultText = lib.literalExpression "formalshell.packages.\${pkgs.stdenv.hostPlatform.system}.formalshell-greeter";
      description = "The greeter package. Defaults to this flake's build for the host system.";
    };

    compositorPackage = lib.mkOption {
      type = lib.types.package;
      default = pkgs.sway;
      defaultText = lib.literalExpression "pkgs.sway";
      description = "Wlroots compositor the greeter surface runs under.";
    };

    compositorConfigText = lib.mkOption {
      type = lib.types.lines;
      default = "";
      description = ''
        Extra config text for compositorPackage. Empty by default: the
        greeter binary is launched directly by this module's session
        script (see the header comment), not via the compositor's own
        `exec`, so a real deployment needs no config here at all — this
        exists for output/input quirks a particular seat might need.
      '';
    };

    sessionCommand = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "Hyprland" ];
      description = ''
        Argv a successful login launches (greetd-ipc(7)'s start_session),
        written into a static settings.json for the `greeter` system
        account so a consumer who is not this repo's owner gets a proper
        option instead of hand-editing that key.
      '';
    };

    sessionDirs = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "${config.services.displayManager.sessionData.desktops}/share/wayland-sessions" ];
      defaultText = lib.literalExpression ''[ "''${config.services.displayManager.sessionData.desktops}/share/wayland-sessions" ]'';
      description = ''
        Directories of Wayland session `.desktop` files the greeter offers
        under its field. The default is every session the NixOS display
        manager options register (`services.displayManager.sessionPackages`,
        Plasma, Hyprland). With none found, a login runs sessionCommand.
      '';
    };

    settings = lib.mkOption {
      type = (pkgs.formats.json { }).type;
      default = { };
      example = { theme.preset = "pantheon"; };
      description = "More settings.json keys for the greeter, such as the desktop's `theme.preset`.";
    };

    wallpaper = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = "Backdrop image, under the lock screen's scrim. A wallpaper mirrored from mirrorUser wins over it.";
    };

    mirrorUser = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "alice";
      description = ''
        A user whose FormalShell wallpaper, theme.json and `~/.face` the
        greeter shows, copied into stateDir whenever one of them changes,
        so the login screen matches the desktop.
      '';
    };

    runtimeDir = lib.mkOption {
      type = lib.types.str;
      default = "/run/formalshell-greeter";
      description = "Writable XDG_RUNTIME_DIR for the greeter's compositor and Qt process.";
    };

    stateDir = lib.mkOption {
      type = lib.types.str;
      default = "/var/lib/formalshell-greeter";
      description = "Writable HOME for the greeter session (the `greeter` user's real passwd HOME is unwritable).";
    };

    extraEnvironment = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      description = ''
        Additional environment variables exported before the compositor
        starts — e.g. WLR_BACKENDS/WLR_RENDERER overrides for a headless or
        otherwise nonstandard seat.
      '';
    };

    sessionLogFile = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "If set, the session script appends its own `set -x` trace (and both processes' stdout/stderr) here.";
    };

    postGreeterCommand = lib.mkOption {
      type = lib.types.lines;
      default = "";
      description = ''
        Extra shell run after the greeter process exits but before the
        compositor is torn down — the only moment the compositor is up
        with no client attached. Ignored by a normal login; verification
        tooling (e.g. a post-auth screenshot) is the intended use.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    services.greetd = {
      enable = true;
      settings.default_session.command = "${sessionScript}";
    };

    systemd.tmpfiles.rules = [
      "d ${cfg.runtimeDir} 0700 greeter greeter -"
      "d ${cfg.stateDir} 0700 greeter greeter -"
    ];

    systemd.services.formalshell-greeter-mirror = lib.mkIf (cfg.mirrorUser != null) {
      description = "Copy ${cfg.mirrorUser}'s FormalShell wallpaper and theme to the greeter";
      wantedBy = [ "multi-user.target" ];
      after = [ "systemd-tmpfiles-setup.service" ];
      serviceConfig = {
        Type = "oneshot";
        ExecStart = mirrorScript;
      };
    };

    systemd.paths.formalshell-greeter-mirror = lib.mkIf (cfg.mirrorUser != null) {
      wantedBy = [ "multi-user.target" ];
      pathConfig.PathChanged = [ "${mirrorState}/state.json" "${mirrorState}/theme.json" "${mirrorHome}/.face" ];
    };
  };
}
