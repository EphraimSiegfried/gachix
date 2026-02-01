{ inputs, ... }:
{
  imports = [ inputs.treefmt.flakeModule ];
  perSystem.treefmt.programs = {
    nixfmt.enable = true;
    rustfmt.enable = true;
  };
}
