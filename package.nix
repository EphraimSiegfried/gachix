{
  rustPlatform,
}:

rustPlatform.buildRustPackage (finalAttrs: {
  pname = "gachix";
  version = "0.1.0";
  src = ./.;
  cargoHash = "sha256-9atn5qyBDy4P6iUoHFhg+TV6Ur71fiah4oTJbBMeEy4=";
})
