{ inputs, ... }:
{
  perSystem =
    { pkgs, self', ... }:
    let
      inherit (pkgs.lib.fileset) toSource unions;

      src = toSource {
        root = ../.;
        fileset = unions [
          ../src
          ../tests
          ../Cargo.toml
          ../Cargo.lock
        ];
      };

      naersk' = pkgs.callPackage inputs.naersk { };

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
      packages.default = naersk'.buildPackage (
        {
          inherit src;

          buildInputs = runtimeLibs;
          nativeBuildInputs = [ pkgs.pkg-config ];
        }
        // cargoConfig
      );

      checks.build-check = self'.packages.default;

      devShells.default = pkgs.mkShell {
        nativeBuildInputs = buildTools ++ runtimeLibs;
        env = cargoConfig;
      };
    };
}
