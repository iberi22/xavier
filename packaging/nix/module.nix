# NixOS module: services.xavier runs the headless server as a hardened system service.
self:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.xavier;
in
{
  options.services.xavier = {
    enable = lib.mkEnableOption "the Xavier headless memory server";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.xavier-server;
      defaultText = lib.literalExpression "xavier.packages.\${system}.xavier-server";
      description = "Xavier server package.";
    };

    host = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1";
      description = "Bind address. 0.0.0.0 exposes the API.";
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 8006;
      description = "HTTP port.";
    };

    environmentFile = lib.mkOption {
      type = lib.types.path;
      example = "/run/secrets/xavier.env";
      description = ''
        File with at least `XAVIER_TOKEN=…`. Kept out of the Nix store on purpose.
      '';
    };

    stateDir = lib.mkOption {
      type = lib.types.str;
      default = "xavier";
      description = "Directory under /var/lib. It is HOME of the service and holds master.key.";
    };
  };

  config = lib.mkIf cfg.enable {
    users.users.xavier = {
      isSystemUser = true;
      group = "xavier";
      home = "/var/lib/${cfg.stateDir}";
    };
    users.groups.xavier = { };

    systemd.services.xavier = {
      description = "Xavier memory server (headless)";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      environment = {
        HOME = "/var/lib/${cfg.stateDir}";
        XAVIER_DATA_DIR = "/var/lib/${cfg.stateDir}/data";
        XAVIER_HOST = cfg.host;
        XAVIER_PORT = toString cfg.port;
      };
      serviceConfig = {
        ExecStart = "${lib.getExe cfg.package} http --no-ui";
        EnvironmentFile = cfg.environmentFile;
        User = "xavier";
        Group = "xavier";
        StateDirectory = cfg.stateDir;
        StateDirectoryMode = "0750";
        WorkingDirectory = "/var/lib/${cfg.stateDir}";
        Restart = "on-failure";
        RestartSec = 5;
        TimeoutStopSec = 60;
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictSUIDSGID = true;
        LockPersonality = true;
      };
    };
  };
}
