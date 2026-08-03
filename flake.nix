{
  description = "GIF Player - GTK3 Wayland layer-shell GIF overlays";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          gif-player = pkgs.callPackage ./nix/package.nix { };
        in
        {
          inherit gif-player;
          default = gif-player;
        }
      );

      apps = forAllSystems (system: rec {
        gif-player = {
          type = "app";
          program = "${self.packages.${system}.gif-player}/bin/gif-player";
        };
        default = gif-player;
      });

      checks = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          package = self.packages.${system}.gif-player;
          source = nixpkgs.lib.cleanSource ./.;
          python = pkgs.python3.withPackages (pythonPackages: with pythonPackages; [
            pygobject3
            pycairo
            pillow
          ]);
          closure = pkgs.closureInfo { rootPaths = [ package ]; };
        in
        {
          inherit package;

          source-tests = pkgs.runCommand "gif-player-source-tests" { nativeBuildInputs = [ python ]; } ''
            cp -r ${source} source
            chmod -R u+w source
            cd source
            python -m compileall -q .
            PYTHONPATH=. python -m unittest discover -s tests -v
            touch "$out"
          '';

          cli-smoke = pkgs.runCommand "gif-player-cli-smoke" { nativeBuildInputs = [ package ]; } ''
            export HOME="$TMPDIR/home"
            export XDG_RUNTIME_DIR="$TMPDIR/runtime"
            export XDG_CONFIG_HOME="$TMPDIR/config"
            export XDG_CACHE_HOME="$TMPDIR/cache"
            export XDG_DATA_HOME="$TMPDIR/data"
            mkdir -p "$HOME" "$XDG_RUNTIME_DIR"
            chmod 700 "$XDG_RUNTIME_DIR"
            gif-player --help >/dev/null
            gif-player doctor | grep -q 'GTK typelibs: OK'
            gif-player self-test > report.json
            grep -q '"protocol": 2' report.json
            touch "$out"
          '';

          runtime-closure-policy = pkgs.runCommand "gif-player-runtime-closure-policy" { } ''
            if grep -E '/[^/]*(setuptools|wheel|linux-headers|gcc-wrapper|binutils-wrapper)-' \
              ${closure}/store-paths; then
              echo "forbidden build dependency in GIF Player runtime closure" >&2
              exit 1
            fi
            if grep -R -E '/nix/store/[^/]+-(glib|gobject-introspection|pygobject)-[^/]+-dev' \
              ${package}/bin ${package}/libexec; then
              echo "development output leaked into GIF Player launchers" >&2
              exit 1
            fi
            touch "$out"
          '';
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          python = pkgs.python3.withPackages (pythonPackages: with pythonPackages; [
            pygobject3
            pycairo
            pillow
          ]);
        in
        {
          default = pkgs.mkShell {
            packages = [
              python
              pkgs.gtk3
              pkgs.gtk-layer-shell
              pkgs.glib
              pkgs.gdk-pixbuf
              pkgs.pango
              pkgs.at-spi2-core
              pkgs.gobject-introspection
              pkgs.ruff
              pkgs.nixfmt-rfc-style
            ];
            shellHook = ''
              export PYTHONPATH="$PWD''${PYTHONPATH:+:$PYTHONPATH}"
              export GI_TYPELIB_PATH="${pkgs.lib.makeSearchPath "lib/girepository-1.0" [
                pkgs.gobject-introspection
                pkgs.glib
                pkgs.gtk3
                pkgs.gtk-layer-shell
                pkgs.gdk-pixbuf
                pkgs.pango
                pkgs.at-spi2-core
              ]}''${GI_TYPELIB_PATH:+:$GI_TYPELIB_PATH}"
              echo "GIF Player dev shell: python, GTK3, GtkLayerShell, Cairo, Pillow, ruff, nixfmt"
            '';
          };
        }
      );
    };
}
