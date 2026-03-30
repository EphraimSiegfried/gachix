{ self, ... }:
{
  flake.nixosModules.default =
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
      exposeLocalNix = cfg.settings.store.use_local_nix_daemon or config.nix.enable;
    in
    {
      options.services.gachix = {
        enable = lib.mkEnableOption "Gachix distributed nix cache";

        package = (lib.mkPackageOption pkgs "gachix" { }) // {
          default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
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

        exposeRepository = {
          enable = lib.mkEnableOption "Serve Git repository via SSH";
          authorizedKeys = lib.mkOption {
            type = lib.types.listOf lib.types.str;
            default = [ ];
            description = "A list of OpenSSH public keys from peers which are allowed to pull from the repository.";
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

      config = lib.mkIf cfg.enable (
        lib.mkMerge [
          {
            services.gachix = {
              inherit finalPackage;
              settings = {
                store.path = lib.mkForce "/var/lib/${cfg.stateDir}/cache";
                server.host = lib.mkForce "0.0.0.0";
                server.port = lib.mkForce cfg.port;
              };
            };

            networking.firewall.allowedTCPPorts = lib.optional cfg.openFirewall cfg.port;

            users.users.${cfg.user} = {
              isSystemUser = true;
              group = cfg.group;
              createHome = false;
            };
            users.groups.${cfg.group} = { };

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

                BindPaths = lib.optional exposeLocalNix "/nix/var/nix/daemon-socket/socket";
                BindReadOnlyPaths = lib.optional exposeLocalNix "/nix";

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
          }

          # Git Server Configuration
          # Mostly copied from https://wiki.nixos.org/wiki/Git

          (lib.mkIf cfg.exposeRepository.enable {
            users.users.gachix_peer = {
              isSystemUser = true;
              group = "git";
              home = "/var/lib/${cfg.stateDir}";
              createHome = true;
              shell = "${pkgs.git}/bin/git-shell";
              openssh.authorizedKeys.keys = cfg.exposeRepository.authorizedKeys;
            };

            users.groups.git = { };

            services.openssh = {
              enable = true;
              extraConfig = ''
                Match User gachix_peer
                  AllowTcpForwarding no
                  AllowAgentForwarding no
                  PasswordAuthentication no
                  PermitTTY no
                  X11Forwarding no
              '';
            };
          })
        ]
      );

    };
}
