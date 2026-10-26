# System-side prerequisites for FormalShell that home-manager cannot provide
# (spec docs/superpowers/specs/2026-07-27-formalshell-design.md §Nix): a
# dedicated PAM service for the lock screen, geoclue2 (+ its agent) for the
# default position source, and the system D-Bus
# services the bar/panels bind directly and have no other way to acquire.
#
# Each toggle below is a system service the shell talks to directly,
# audited from nix/testvm.nix's own hand-added services (M8 Task 3):
#   - NetworkManager: the network panel's only backend.
#   - bluez: the bluetooth panel's backend.
#   - UPower: the battery bar cell and the power panel.
#   - power-profiles-daemon: the net.hadess.PowerProfiles D-Bus service
#     behind the power panel's profile picker.
#   - pipewire: the audio bar cell, audio panel and volume OSD.
#   - polkit: the shell registers as the session's authentication agent,
#     which needs polkitd to register with. The setuid pkexec wrapper is its
#     own opt-in on current nixpkgs; without it pkexec refuses to run
#     ("pkexec must be setuid root").
# All six are genuine FormalShell prerequisites, not test-rig artifacts —
# nix/testvm.nix's *other* hand-added bits (the null-audio-sink virtual
# node, wtype/grim/mpv, getty autologin, …) stay in the VM config because
# they only exist to give the smoke rig something to screenshot, not
# something a real install needs.
#
# Every option defaults to true but is set with lib.mkDefault, so a consumer
# who already manages one of these services their own way (a different
# network stack, an existing pipewire config, …) can still override it
# without a definition conflict.
{ config, lib, pkgs, ... }:
let
  cfg = config.services.formalshell;
  ancs4linux = pkgs.callPackage ./ancs4linux.nix { };

  # 0x12 is GET_CONNECTOR_STATUS, the connector number goes in bits 16 and
  # up; it is the only command written. fs-system's power::flow decodes the
  # `response` lines. RAPL is published as a milliwatt average over the whole
  # poll interval rather than by opening energy_uj to everyone: the counter
  # is root-only because of the PLATYPUS side channel, which needs
  # fine-grained reads a three-second average does not give.
  powerPoll = pkgs.writeShellScript "formalshell-power-poll" (builtins.readFile ./formalshell-power-poll.sh);
in
{
  options.services.formalshell = {
    enable = lib.mkEnableOption "FormalShell system-side prerequisites";

    pam.enable = lib.mkEnableOption ''
      the "formalshell-lock" PAM service the lock screen authenticates
      against (the name is fixed in the shell, so this only toggles whether
      the service exists)
    '' // { default = true; };

    geoclue.enable = lib.mkEnableOption ''
      geoclue2 (+ its demo agent) for the shell's default position source.
      FormalShell ships no agent of its own; the upstream demo agent is what
      authorizes the request, the role services.geoclue2's own
      enableDemoAgent default already plays
    '' // { default = true; };

    networkmanager.enable = lib.mkEnableOption "NetworkManager, the network panel's only backend" // { default = true; };
    bluetooth.enable = lib.mkEnableOption "bluez, the bluetooth panel's backend" // { default = true; };
    upower.enable = lib.mkEnableOption "UPower, backing the battery bar cell and power panel" // { default = true; };
    powerProfiles.enable = lib.mkEnableOption "power-profiles-daemon, backing the power panel's profile picker" // { default = true; };
    pipewire.enable = lib.mkEnableOption "pipewire, backing the audio bar cell, audio panel, and volume OSD" // { default = true; };
    polkit.enable = lib.mkEnableOption "polkit and the pkexec wrapper, backing the shell's authentication agent" // { default = true; };

    power.poller.enable = lib.mkEnableOption ''
      a root service feeding the Power panel's flow diagram two things the
      kernel keeps from a user session, every three seconds: each USB-C
      connector's status (the UCSI GET_CONNECTOR_STATUS command through
      debugfs, nothing else) to /run/formalshell/ucsi, and the CPU package
      draw averaged over the interval to /run/formalshell/rapl. Without it
      the USB-C rows say "Powering a device" with no contract wattage and the
      CPU draw shows as unavailable
    '' // { default = true; };

    # M75: the iPhone ANCS/AMS bridge. Off by default and not folded into the
    # `enable` toggle above: ancs4linux.Observer and ancs4linux.Advertising
    # run as root on the system bus and start advertising over BLE, which is
    # not something a shell that merely wants a battery cell should turn on
    # by accident.
    iphone.enable = lib.mkEnableOption ''
      the iPhone integration's system side: ancs4linux's Observer and
      Advertising daemons (nix/ancs4linux.nix, pzmarzly/ancs4linux with the
      omarchy-iphone metadata patch), the D-Bus policy letting the
      "ancs4linux" group talk to them, that group itself, and Bluetooth
    '';

    # M75 Task 5/6: LocalsendService's receiver binds 0.0.0.0:53317 (both
    # TCP for the HTTP(S) transfer and UDP for the peer-discovery multicast
    # announcement, internal/localsend/scan.go's 224.0.0.167:53317 group),
    # on every interface so it/tailscale0 both work -- not folded into the
    # top-level `enable` toggle since the port only needs opening when
    # localsend.receive is actually turned on.
    localsend.enable = lib.mkEnableOption ''
      the firewall opening for LocalsendService's receiver: TCP+UDP 53317
    '';

    # M75 Task 6: AirplayService's `uxplay` child needs both mDNS
    # advertising (so an iPhone's control centre actually lists it) and its
    # fixed legacy ports open. Matches what the owner's hand-rolled
    # `kyan.airplay`/avahi setup on g815 did today, folded into this module
    # so a consumer needs no separate avahi config of their own.
    airplay.enable = lib.mkEnableOption ''
      avahi mDNS publishing and UxPlay's legacy fixed ports (TCP
      7000/7001/7100, UDP 6000/6001/7011, the set `-p` with no argument
      opens) for AirplayService's receiver
    '';

    # LightsService drives keyboard RGB through asusctl, which is only a
    # client: the Aura object it talks to belongs to the asusd system daemon.
    lights.asus.enable = lib.mkEnableOption ''
      asusd, the daemon behind the keyboard lights route on ASUS ROG and TUF
      laptops
    '';

    gaming.enable = lib.mkEnableOption ''
      Steam with its gamescope session (a "Steam" entry in the greeter's
      session list), gamescope with cap_sys_nice, GameMode, the udev rules
      for Steam controllers, and the hyprland and gtk portals (left to
      programs.hyprland when that is on, which adds both). x86_64-linux
      only, as Steam is. Steam is unfree: this adds "steam",
      "steam-unwrapped" and "steam-run" to nixpkgs.config.allowUnfreePackages,
      which merges with the system's own list and predicate. It has no
      effect where nixpkgs.pkgs is passed in from outside the system, whose
      config then has to allow those names itself
    '';
  };

  config = lib.mkMerge [
    (lib.mkIf cfg.enable {
      security.pam.services."formalshell-lock" = lib.mkIf cfg.pam.enable (lib.mkDefault { });

      services.geoclue2 = lib.mkIf cfg.geoclue.enable {
        enable = lib.mkDefault true;
        enableDemoAgent = lib.mkDefault true;
        # LocationService's PositionSource asks under this desktop id. A
        # system client skips the agent, whose prompt has no one to answer
        # it once the shell owns the notification server.
        appConfig.formalshell = {
          isAllowed = true;
          isSystem = true;
        };
      };

      networking.networkmanager.enable = lib.mkIf cfg.networkmanager.enable (lib.mkDefault true);
      hardware.bluetooth.enable = lib.mkIf cfg.bluetooth.enable (lib.mkDefault true);
      services.upower.enable = lib.mkIf cfg.upower.enable (lib.mkDefault true);
      services.power-profiles-daemon.enable = lib.mkIf cfg.powerProfiles.enable (lib.mkDefault true);
      services.pipewire.enable = lib.mkIf cfg.pipewire.enable (lib.mkDefault true);
      security.polkit = lib.mkIf cfg.polkit.enable {
        enable = lib.mkDefault true;
        enablePkexecWrapper = lib.mkDefault true;
      };
    })

    (lib.mkIf (cfg.enable && cfg.power.poller.enable) {
      systemd.services.formalshell-power-poll = {
        description = "FormalShell USB-C and CPU power readings for the Power panel";
        wantedBy = [ "multi-user.target" ];
        path = [ pkgs.coreutils ];
        serviceConfig = {
          ExecStart = powerPoll;
          RuntimeDirectory = "formalshell";
          RuntimeDirectoryMode = "0755";
          Restart = "on-failure";
        };
      };
    })

    (lib.mkIf cfg.iphone.enable {
      hardware.bluetooth.enable = lib.mkDefault true;

      users.groups.ancs4linux = { };

      # D-Bus policy copied from ancs4linux's own autorun/ dir (pzmarzly/ancs4linux
      # @ 1384517, autorun/ancs4linux-{observer,advertising}.xml): root owns each
      # bus name outright, the ancs4linux group gets send/receive. It goes in
      # through services.dbus.packages because NixOS links /etc/dbus-1 whole
      # from the store, so environment.etc cannot add a file under it.
      services.dbus.packages = [
        (pkgs.writeTextDir "share/dbus-1/system.d/ancs4linux.conf" ''
          <!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
           "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
          <busconfig>
            <policy user="root">
              <allow own="ancs4linux.Observer"/>
              <allow send_destination="ancs4linux.Observer"/>
              <allow receive_sender="ancs4linux.Observer"/>
              <allow own="ancs4linux.Advertising"/>
              <allow send_destination="ancs4linux.Advertising"/>
              <allow receive_sender="ancs4linux.Advertising"/>
            </policy>
            <policy group="ancs4linux">
              <allow send_destination="ancs4linux.Observer"/>
              <allow receive_sender="ancs4linux.Observer"/>
              <allow send_destination="ancs4linux.Advertising"/>
              <allow receive_sender="ancs4linux.Advertising"/>
            </policy>
          </busconfig>
        '')
      ];

      # Type=dbus units, same as upstream: systemd waits for the BusName to
      # appear on the system bus before treating the unit as started, which is
      # what lets ExecStartPost-less dependents (IphoneService's bridge child)
      # find the service already registered rather than racing its startup.
      systemd.services.ancs4linux-observer = {
        description = "ancs4linux Observer daemon (ANCS notifications over BLE)";
        requires = [ "bluetooth.service" ];
        after = [ "bluetooth.service" ];
        wantedBy = [ "multi-user.target" ];
        serviceConfig = {
          Type = "dbus";
          BusName = "ancs4linux.Observer";
          ExecStart = "${ancs4linux}/bin/ancs4linux-observer";
        };
      };

      systemd.services.ancs4linux-advertising = {
        description = "ancs4linux Advertising daemon (BLE pairing)";
        requires = [ "bluetooth.service" ];
        after = [ "bluetooth.service" ];
        wantedBy = [ "multi-user.target" ];
        serviceConfig = {
          Type = "dbus";
          BusName = "ancs4linux.Advertising";
          ExecStart = "${ancs4linux}/bin/ancs4linux-advertising";
        };
      };
    })

    (lib.mkIf cfg.lights.asus.enable {
      services.asusd.enable = lib.mkDefault true;
    })

    (lib.mkIf cfg.gaming.enable {
      nixpkgs.config.allowUnfreePackages = [ "steam" "steam-unwrapped" "steam-run" ];

      programs.steam = {
        enable = lib.mkDefault true;
        gamescopeSession.enable = lib.mkDefault true;
      };
      programs.gamescope = {
        enable = lib.mkDefault true;
        capSysNice = lib.mkDefault true;
      };
      programs.gamemode.enable = lib.mkDefault true;
      hardware.steam-hardware.enable = lib.mkDefault true;

      # programs.hyprland already adds both portals and Hyprland's own
      # portals.conf.
      xdg.portal = lib.mkIf (!config.programs.hyprland.enable) {
        enable = lib.mkDefault true;
        extraPortals = [ pkgs.xdg-desktop-portal-hyprland pkgs.xdg-desktop-portal-gtk ];
        config.hyprland.default = lib.mkDefault [ "hyprland" "gtk" ];
      };
    })

    (lib.mkIf cfg.localsend.enable {
      networking.firewall.allowedTCPPorts = [ 53317 ];
      networking.firewall.allowedUDPPorts = [ 53317 ];
    })

    (lib.mkIf cfg.airplay.enable {
      services.avahi = {
        enable = lib.mkDefault true;
        nssmdns4 = lib.mkDefault true;
        publish = {
          enable = lib.mkDefault true;
          userServices = lib.mkDefault true;
        };
      };
      # UxPlay's `-p`: legacy fixed ports (uxplay.cpp's own argument
      # parsing, checked against its source at HEAD), the set the owner's
      # replaced `kyan.airplay` unit already ran with.
      networking.firewall.allowedTCPPorts = [ 7000 7001 7100 ];
      networking.firewall.allowedUDPPorts = [ 6000 6001 7011 ];
    })
  ];
}
