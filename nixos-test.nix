{ gachix }:
{ pkgs, ... }:
let
  port = 9192;
  # copypasted from nixpkgs for better eval time
  snakeOilEd25519PrivateKey = pkgs.writeText "privkey.snakeoil" ''
    -----BEGIN OPENSSH PRIVATE KEY-----
    b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
    QyNTUxOQAAACAz0F5hFTFS5nhUcmnyjFVoDw5L/P7kQU8JnBA2rWczAwAAAIhWlP99VpT/
    fQAAAAtzc2gtZWQyNTUxOQAAACAz0F5hFTFS5nhUcmnyjFVoDw5L/P7kQU8JnBA2rWczAw
    AAAEDE1rlcMC0s0X3TKVZAOVavZOywwkXw8tO5dLObxaCMEDPQXmEVMVLmeFRyafKMVWgP
    Dkv8/uRBTwmcEDatZzMDAAAAAAECAwQF
    -----END OPENSSH PRIVATE KEY-----
  '';

  snakeOilEd25519PublicKey = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIDPQXmEVMVLmeFRyafKMVWgPDkv8/uRBTwmcEDatZzMD snakeoil";
in
{
  name = "gachix";
  nodes = {
    server = {
      imports = [ (import ./nixos-module.nix { inherit gachix; }) ];

      nix.enable = false; # gachix should be able to run without nix available

      services.gachix = {
        enable = true;
        openFirewall = true;
        inherit port;
        settings.store = {
          ssh_private_key_path = snakeOilEd25519PrivateKey;
          builders = [ "ssh://client" ];
        };
      };
    };
    client = {
      environment.systemPackages = with pkgs; [
        curl
        gawk
        hello
      ];
      nix.sshServe = {
        enable = true;
        keys = [ snakeOilEd25519PublicKey ];
      };
    };
  };
  testScript = ''
    start_all()

    server.wait_for_unit("gachix.service")
    client.wait_for_unit("sshd.service")

    server.wait_for_open_port(${toString port})
    client.wait_for_open_port(22)

    server.succeed("sudo -u gachix gachix add ${pkgs.hello}")
    client.succeed("curl -f \"http://server:${toString port}$(echo ''${${pkgs.hello}#/nix/store/} | awk -F- '{print $1}').narinfo\"")
  '';
}
