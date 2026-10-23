{
  description = "Xavier: headless server package and NixOS module";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
      version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;
    in
    {
      packages = forAll (pkgs: rec {
        xavier-server = pkgs.callPackage ./packaging/nix/package.nix { inherit version; };
        default = xavier-server;
      });

      nixosModules.xavier = import ./packaging/nix/module.nix self;
      nixosModules.default = self.nixosModules.xavier;

      # Evaluates the module without building a system (`nix flake check --no-build`).
      checks = forAll (
        pkgs:
        let
          system = nixpkgs.lib.nixosSystem {
            inherit (pkgs.stdenv.hostPlatform) system;
            modules = [
              self.nixosModules.xavier
              {
                services.xavier.enable = true;
                services.xavier.environmentFile = "/run/secrets/xavier.env";
                boot.loader.grub.enable = false;
                fileSystems."/" = {
                  device = "nodev";
                  fsType = "tmpfs";
                };
                system.stateVersion = "25.11";
              }
            ];
          };
          unit = system.config.systemd.services.xavier;
        in
        {
          # unsafeDiscardStringContext: check the unit text without building the package.
          module-eval = pkgs.writeText "xavier-module-eval" (
            builtins.unsafeDiscardStringContext ''
              ${unit.serviceConfig.ExecStart}
              ${unit.serviceConfig.User}
              ${unit.environment.HOME}
            ''
          );
        }
      );
    };
}
