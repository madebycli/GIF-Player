self: { config, lib, pkgs, ... }:
let cfg = config.programs.noctalia-gif-player; in {
  options.programs.noctalia-gif-player = {
    enable = lib.mkEnableOption "Noctalia GIF Player";
    package = lib.mkOption { type = lib.types.package; default = self.packages.${pkgs.stdenv.hostPlatform.system}.gif-player; };
  };
  config = lib.mkIf cfg.enable { environment.systemPackages = [ cfg.package ]; };
}
