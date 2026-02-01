{ self, ... }:
{
  perSystem =
    { pkgs, self', ... }:
    {
      packages.dockerImage = pkgs.dockerTools.streamLayeredImage {
        name = "gachix";
        tag =
          self.sourceInfo.shortRev or "${self.sourceInfo.dirtyShortRev}-${self.sourceInfo.lastModifiedDate}";
        contents = [
          pkgs.cacert
          self'.packages.default
        ];
        created = "now"; # otherwise all images look like 1s since UNIX epoch
        config.Entrypoint = [
          "gachix"
          "serve"
        ];
      };
    };
}
