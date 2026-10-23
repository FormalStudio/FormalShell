{
  description = "FormalShell, a Hyprland desktop shell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # Only the nixos-module-eval check uses it.
    home-manager = {
      url = "github:nix-community/home-manager";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, home-manager, crane }:
    let
      # One dependency build per system, shared by every Rust derivation.
      rustCommonFor = pkgs: pkgs.callPackage ./nix/rust-common.nix { inherit crane; };
      systems = [ "x86_64-linux" "aarch64-linux" ];
      # darwin gets no shell package but runs the pure crates' tests and
      # hosts the dev loop driving a linux VM for e2e (see
      # docs/superpowers/plans/2026-07-28-mac-e2e-rig.md).
      darwinSystems = [ "aarch64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system:
        f system nixpkgs.legacyPackages.${system});
      forDarwin = f: nixpkgs.lib.genAttrs darwinSystems (system:
        f system nixpkgs.legacyPackages.${system});
      # Evaluates (never builds) the NixOS config README.md documents, with
      # no explicit `package` anywhere, so a module default that stops
      # resolving fails CI. The drvPath is embedded as a string without its
      # context, which keeps the check from building the closure.
      nixosModuleEval = system: pkgs:
        let
          os = nixpkgs.lib.nixosSystem {
            inherit system;
            modules = [
              self.nixosModules.formalshell
              self.nixosModules.formalshell-greeter
              home-manager.nixosModules.home-manager
              ({ ... }: {
                services.formalshell.enable = true;
                services.formalshell-greeter = {
                  enable = true;
                  sessionCommand = [ "Hyprland" ];
                };
                programs.hyprland.enable = true;
                users.users.me = { isNormalUser = true; };
                home-manager.users.me = {
                  imports = [ self.homeModules.default ];
                  programs.formalshell.enable = true;
                  home.stateVersion = "25.05";
                };
                fileSystems."/" = { device = "none"; fsType = "tmpfs"; };
                boot.loader.grub.enable = false;
                system.stateVersion = "25.05";
              })
            ];
          };
        in
        pkgs.writeText "formalshell-nixos-module-eval"
          (builtins.unsafeDiscardStringContext os.config.system.build.toplevel.drvPath);
    in
    {
      packages = nixpkgs.lib.recursiveUpdate
        (forDarwin (system: pkgs: {
          # The aarch64-linux build capability the mac's nix daemon offloads to —
          # nixpkgs' own macOS remote-builder VM, pinned here rather than taken
          # from the flake registry so the rig is reproducible with the repo.
          # The override raises the guest off its 1-core/3G/20G defaults (a Qt
          # build and a store image do not fit those); it only rebuilds the four
          # darwin-side runner derivations, everything else still substitutes.
          # Driven by dev/linux-builder.sh.
          linux-builder = pkgs.darwin.linux-builder.override {
            modules = [
              ./nix/vm-discard.nix
              {
                virtualisation.cores = 6;
                virtualisation.darwin-builder = { memorySize = 10240; diskSize = 61440; };
                # Store paths are copied back to the mac as they finish, so
                # the builder only needs the last day of them.
                nix.gc = {
                  automatic = true;
                  dates = "daily";
                  options = "--delete-older-than 1d";
                };
              }
            ];
          };

          # The runtime layer: darwin-runnable headless aarch64-linux test VM
          # (nix/testvm.nix). Driven by dev/vm.sh.
          testvm = self.nixosConfigurations.testvm.config.system.build.vm;
        }))
        (forAllSystems (system: pkgs: rec {
          formalshell-eds = pkgs.callPackage ./nix/eds-package.nix { };
          tensaku = pkgs.callPackage ./nix/tensaku-package.nix { };
          ttfx = pkgs.callPackage ./nix/ttfx-package.nix { };
          clipssh = pkgs.callPackage ./nix/clipssh-package.nix { };
          lucide-font = pkgs.callPackage ./nix/lucide-font.nix { };
          blue-marble = pkgs.callPackage ./nix/blue-marble.nix { };
          earbuds-art = pkgs.callPackage ./nix/earbuds-art.nix { };
          ancs4linux = pkgs.callPackage ./nix/ancs4linux.nix { };
          iphone-bridge = pkgs.callPackage ./nix/iphone-bridge.nix { };
          localsend-cli = pkgs.callPackage ./nix/localsend-cli.nix { };
          openscq30 = pkgs.callPackage ./nix/openscq30.nix { };
          nothingctl = pkgs.callPackage ./nix/nothingctl.nix { };
          formalshell = pkgs.callPackage ./nix/package.nix {
            rustCommon = rustCommonFor pkgs;
            inherit lucide-font blue-marble earbuds-art iphone-bridge openscq30 nothingctl formalshell-eds tensaku ttfx clipssh localsend-cli;
            inherit (pkgs) uxplay earbuds;
          };
          formalshell-greeter = pkgs.writeShellScriptBin "formalshell-greeter" ''
            exec ${pkgs.lib.getExe formalshell} greeter "$@"
          '';
          default = formalshell;
        }));

      homeModules = { formalshell = import ./nix/hm-module.nix self; default = import ./nix/hm-module.nix self; };

      nixosModules = {
        formalshell = ./nix/nixos-module.nix;
        formalshell-greeter = import ./nix/nixos-greeter-module.nix self;
        default = ./nix/nixos-module.nix;
      };

      nixosConfigurations = {
        testvm = import ./nix/testvm.nix { inherit self nixpkgs; };
      };

      checks = nixpkgs.lib.recursiveUpdate
        (forDarwin (system: pkgs: { rust-tests = pkgs.callPackage ./nix/rust-tests.nix { rustCommon = rustCommonFor pkgs; }; fs-auth = pkgs.callPackage ./nix/fs-auth-test.nix { inherit crane; }; }))
        (forAllSystems (system: pkgs: {
        rust-tests = pkgs.callPackage ./nix/rust-tests.nix { rustCommon = rustCommonFor pkgs; };
        fs-tray = pkgs.callPackage ./nix/fs-tray.nix { rustCommon = rustCommonFor pkgs; };
        fs-notifd = pkgs.callPackage ./nix/fs-notifd-check.nix { rustCommon = rustCommonFor pkgs; };
        fs-bluez = pkgs.callPackage ./nix/fs-bluez-test.nix { rustCommon = rustCommonFor pkgs; };
        fs-auth = pkgs.callPackage ./nix/fs-auth-test.nix { inherit crane; };
        fs-audio = pkgs.callPackage ./nix/fs-audio.nix { rustCommon = rustCommonFor pkgs; };
        fs-upower = pkgs.callPackage ./nix/fs-upower.nix { rustCommon = rustCommonFor pkgs; };
        fs-network = pkgs.callPackage ./nix/fs-network.nix { rustCommon = rustCommonFor pkgs; };
        nixos-module-eval = nixosModuleEval system pkgs;
        fs-mpris = pkgs.callPackage ./nix/fs-mpris.nix { rustCommon = rustCommonFor pkgs; };
      }));

      devShells = nixpkgs.lib.recursiveUpdate
        (forDarwin (system: pkgs: {
          default = pkgs.mkShell {
            packages = [ pkgs.just ];
          };
        }))
        (forAllSystems (system: pkgs: {
        default = pkgs.mkShell {
          packages = [
            pkgs.matugen
            pkgs.just
          ];
        };
      }));
    };
}
