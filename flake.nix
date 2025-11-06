{
  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nix-community/naersk";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs =
    {
      self,
      flake-utils,
      naersk,
      nixpkgs,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = (import nixpkgs) {
          inherit system;
        };

        naersk' = pkgs.callPackage naersk { };

        runtimeLibs = with pkgs; [
          openssl
          libssh2
        ];

        pkgConfigDeps = with pkgs; [
          openssl.dev
          libssh2
        ];

        buildTools = with pkgs; [
          pkg-config
          rustc
          cargo
        ];

        cargoConfig = {
          PKG_CONFIG_PATH = pkgs.lib.makeSearchPath "lib/pkgconfig" pkgConfigDeps;
          OPENSSL_NO_VENDOR = 1;
          OPENSSL_DIR = "${pkgs.openssl.dev}";
          OPENSSL_LIB_DIR = "${pkgs.lib.getLib pkgs.openssl}/lib";
        };
      in
      {
        defaultPackage = naersk'.buildPackage (
          {
            src = ./.;

            buildInputs = runtimeLibs;
            nativeBuildInputs = [ pkgs.pkg-config ];
          }
          // cargoConfig
        );

        devShell = pkgs.mkShell {
          nativeBuildInputs = buildTools ++ runtimeLibs;
          env = cargoConfig;
        };
      }
    );
}
