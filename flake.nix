{
  description = "Noctalia GIF Player — Rust, native Wayland, Noctalia 5";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      each = nixpkgs.lib.genAttrs systems;
    in {
      packages = each (system: let
        pkgs = import nixpkgs { inherit system; };
        gif-player = pkgs.callPackage ./nix/package.nix {};
      in {
        inherit gif-player;
        default = gif-player;
        noctalia-plugin = pkgs.runCommand "noctalia-gif-player-plugin-1.5.0" {} ''
          mkdir -p "$out"
          cp -r ${./plugins/gif-player}/. "$out/"
          cp ${./NOTICE.md} "$out/NOTICE.md"
        '';
      });
      apps = each (system: rec {
        gif-player = { type = "app"; program = "${self.packages.${system}.gif-player}/bin/gif-player"; };
        default = gif-player;
      });
      nixosModules.default = import ./nix/nixos.nix self;
      homeManagerModules.default = import ./nix/home-manager.nix self;
      checks = each (system: let
        pkgs = import nixpkgs { inherit system; };
        package = self.packages.${system}.gif-player;
        closure = pkgs.closureInfo { rootPaths = [ package ]; };
      in {
        inherit package;
        cli = pkgs.runCommand "gif-player-cli-check" { nativeBuildInputs = [ package ]; } ''
          export HOME="$TMPDIR/home" XDG_RUNTIME_DIR="$TMPDIR/runtime"
          export XDG_CONFIG_HOME="$TMPDIR/config" XDG_DATA_HOME="$TMPDIR/data"
          mkdir -p "$HOME" "$XDG_RUNTIME_DIR"
          chmod 700 "$XDG_RUNTIME_DIR"
          gif-player --help >/dev/null
          gif-player doctor --offline >/dev/null
          gif-player self-test > "$out"
        '';
        native-closure = pkgs.runCommand "gif-player-native-closure-check" {} ''
          if grep -E '/[^/]*(gtk[34]|gtk-layer-shell|python3|pygobject|pillow|qtbase|quickshell|electron|chromium)-' ${closure}/store-paths; then
            exit 1
          fi
          cp ${closure}/store-paths "$out"
        '';
        plugin = pkgs.runCommand "gif-player-noctalia-plugin-check" { nativeBuildInputs = [ pkgs.noctalia ]; } ''
          export HOME="$TMPDIR/home" XDG_CONFIG_HOME="$TMPDIR/config" XDG_DATA_HOME="$TMPDIR/data"
          mkdir -p "$HOME"
          noctalia plugins lint ${./plugins/gif-player} > "$out"
        '';
      });
      devShells = each (system: let pkgs = import nixpkgs { inherit system; }; in {
        default = pkgs.mkShell {
          packages = with pkgs; [ cargo rustc rustfmt clippy pkg-config libxkbcommon fontconfig dejavu_fonts nixfmt sway grim wlrctl ];
        };
      });
    };
}
