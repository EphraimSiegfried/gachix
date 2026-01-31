{
  self,
  lib,
  inputs,
  ...
}:
{
  flake.githubActions = inputs.nix-github-actions.lib.mkGithubMatrix {
    checks = {
      inherit (self.checks) x86_64-linux;
    }
    // lib.genAttrs [ "aarch64-darwin" "aarch64-linux" "x86_64-darwin" ] (system: {
      inherit (self.checks.${system}) build-check;
    });
  };
}
