{
  rustPlatform,
  pkgs,

}:

rustPlatform.buildRustPackage (finalAttrs: {
  pname = "gachix";
  version = "0.1.0";
  src = ./.;
  nativeBuildInputs = [ pkgs.openssl ];
  env = {
    PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig";
    OPENSSL_NO_VENDOR = 1;
    OPENSSL_DIR = "${pkgs.openssl.dev}";
    OPENSSL_LIB_DIR = "${pkgs.lib.getLib pkgs.openssl}/lib";

  };
  cargoHash = "sha256-E/Pl2ffbMsbZFU9mpESMeqOmGHTd+U3oP44dG75G2hc=";
})
