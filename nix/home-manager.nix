self: { config, lib, pkgs, ... }:
let
  cfg = config.services.noctalia-gif-player;
  package = cfg.package;
in {
  options.services.noctalia-gif-player = {
    enable = lib.mkEnableOption "Noctalia GIF Player user service";
    package = lib.mkOption { type = lib.types.package; default = self.packages.${pkgs.stdenv.hostPlatform.system}.gif-player; };
    installNoctaliaPlugin = lib.mkOption { type = lib.types.bool; default = true; };
    systemdTarget = lib.mkOption { type = lib.types.str; default = "graphical-session.target"; };
    gifDirectory = lib.mkOption { type = lib.types.nullOr lib.types.str; default = null; };
  };
  config = lib.mkIf cfg.enable {
    home.packages = [ package ];
    xdg.dataFile = lib.mkIf cfg.installNoctaliaPlugin {
      "noctalia/plugins/gif-player".source = "${package}/share/noctalia-gif-player/plugins/gif-player";
    };
    systemd.user.services.noctalia-gif-player = {
      Unit = { Description = "Noctalia GIF Player Wayland daemon"; PartOf = [ cfg.systemdTarget ]; After = [ cfg.systemdTarget ]; };
      Service = {
        ExecStart = "${package}/bin/gif-player daemon --stay";
        Restart = "on-failure";
        RestartSec = 2;
        Environment = lib.optional (cfg.gifDirectory != null) (lib.escapeShellArg "GIF_PLAYER_GIF_DIR=${cfg.gifDirectory}");
      };
      Install.WantedBy = [ cfg.systemdTarget ];
    };
  };
}
