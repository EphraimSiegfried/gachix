{ gachix }:
{
  pkgs,
  lib,
  config,
  ...
}:
let
  cfg = config.services.gachix;
  fmt = pkgs.formats.yaml { };
  finalPackage = pkgs.symlinkJoin {
    inherit (cfg.package) name;
    paths = [ cfg.package ];
    nativeBuildInputs = [ pkgs.makeWrapper ];
    postBuild = ''
      wrapProgram $out/bin/gachix \
        --add-flags '--config ${fmt.generate "gachix.yaml" cfg.settings}'
    '';
  };
in
{
  options.services.gachix = {
    enable = lib.mkEnableOption "Gachix distributed nix cache";

    package = (lib.mkPackageOption pkgs "gachix" { }) // {
      default = gachix;
    };

    finalPackage = lib.mkOption {
      type = lib.types.nullOr lib.types.package;
      visible = false;
      readOnly = true;
      description = "Resulting customized Gachix package.";
    };

    settings = lib.mkOption {
      inherit (fmt) type;
      description = "YAML config passed to Gachix via `-c`.";
      default = { };
      example = {
        store = {
          path = "/var/lib/gachix/cache";
          builders = [ ];
          sign_private_key_path = "/run/gachix/cache.secret";
        };
      };
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "gachix";
      description = "User account under which gachix runs.";
    };

    group = lib.mkOption {
      type = lib.types.str;
      default = "gachix";
      description = "Group account under which gachix runs.";
    };

    stateDir = lib.mkOption {
      description = "Directory under /var/lib/ where Gachix stores its internal state.";
      type = lib.types.str;
      default = "gachix";
      example = "gachix-2";
    };

    runDir = lib.mkOption {
      description = "Directory under /run/ where Gachix stores its temporary state.";
      type = lib.types.str;
      default = "gachix";
      example = "gachix-2";
    };

    exposeLocalNix = lib.mkOption {
      description = "Whether to expose the local nix-daemon to Gachix. Disable if pulling from elsewhere.";
      type = lib.types.bool;
      default = config.nix.enable;
    };

    port = lib.mkOption {
      description = "The port to open Gachix on.";
      type = lib.types.port;
      default = 8080;
      example = 9192;
    };

    openFirewall = lib.mkOption {
      description = "Whether to open the port in the firewall.";
      type = lib.types.bool;
      default = false;
    };
  };

  config = lib.mkIf cfg.enable {
    services.gachix = {
      inherit finalPackage;
      settings = {
        store = {
          path = lib.mkForce "/var/lib/${cfg.stateDir}/cache";
          use_local_nix_daemon = lib.mkIf (!cfg.exposeLocalNix) (lib.mkForce false);
        };
        server = {
          host = lib.mkForce "0.0.0.0";
          port = lib.mkForce cfg.port;
        };
      };
    };

    networking.firewall.allowedTCPPorts = lib.optional (cfg.openFirewall) cfg.port;

    users = {
      users.${cfg.user} = {
        isSystemUser = true;
        group = cfg.group;
      };
      groups.${cfg.group} = { };
    };

    systemd.services.gachix = {
      description = "Gachix nix cache";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];
      script = ''
        ${cfg.finalPackage}/bin/gachix serve
      '';

      serviceConfig = {
        User = cfg.user;
        Group = cfg.group;

        BindReadOnlyPaths = lib.optional (cfg.exposeLocalNix) "/nix";

        WorkingDirectory = "/var/lib/${cfg.stateDir}";
        StateDirectory = cfg.stateDir;
        RuntimeDirectory = cfg.runDir;

        # standard hardening options
        ProcSubset = "pid";
        ProtectProc = "invisible";
        AmbientCapabilities = [ "CAP_NET_BIND_SERVICE" ];
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        PrivateDevices = true;
        ProtectHostname = true;
        ProtectClock = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectKernelLogs = true;
        ProtectControlGroups = true;
        RestrictAddressFamilies = [
          "AF_UNIX"
          "AF_INET"
          "AF_INET6"
        ];
        RestrictNamespaces = true;
        LockPersonality = true;
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        RemoveIPC = true;
        PrivateMounts = true;
      };
    };

    environment.systemPackages = [ cfg.finalPackage ];
  };
}
