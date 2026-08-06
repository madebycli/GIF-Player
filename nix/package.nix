{
  lib,
  stdenvNoCC,
  closureInfo,
  python3,
  gtk3,
  gtk-layer-shell,
  glib,
  gdk-pixbuf,
  pango,
  at-spi2-core,
  fontconfig,
  gsettings-desktop-schemas,
  shared-mime-info,
  hicolor-icon-theme,
  gobject-introspection,
}:

let
  versionLines = lib.splitString "\n" (builtins.readFile ../pyproject.toml);
  versionLine = lib.findFirst (
    line: builtins.match "version = \"([^\"]+)\"" line != null
  ) null versionLines;
  versionMatch =
    if versionLine == null then
      throw "Unable to read GIF Player version from pyproject.toml"
    else
      builtins.match "version = \"([^\"]+)\"" versionLine;
  packageVersion = builtins.elemAt versionMatch 0;
  python = python3.withPackages (
    pythonPackages: with pythonPackages; [
      pygobject3
      pycairo
      pillow
    ]
  );
  typelibSourceClosure = closureInfo {
    rootPaths = [
      python
      glib
      gdk-pixbuf
      pango
      at-spi2-core
      gtk3
      gtk-layer-shell
      gobject-introspection
    ];
  };
  runtimeTypelibs = stdenvNoCC.mkDerivation {
    pname = "gif-player-runtime-typelibs";
    version = "1";
    dontUnpack = true;
    installPhase = ''
      destination="$out/lib/girepository-1.0"
      mkdir -p "$destination"

      while IFS= read -r source; do
        [ -e "$source" ] || continue
        while IFS= read -r typelib; do
          install -m644 "$typelib" "$destination/$(basename "$typelib")"
        done < <(find -L "$source" -type f -name '*.typelib' -print)
      done < ${typelibSourceClosure}/store-paths

      for required in \
        cairo-1.0 \
        Pango-1.0 \
        PangoCairo-1.0 \
        Atk-1.0 \
        HarfBuzz-0.0; do
        if [ ! -f "$destination/$required.typelib" ]; then
          echo "missing runtime typelib: $required" >&2
          find "$destination" -maxdepth 1 -name '*.typelib' -printf '%f\n' | sort >&2
          exit 1
        fi
      done
    '';
  };
  typelibPath = lib.makeSearchPath "lib/girepository-1.0" [
    runtimeTypelibs
    glib
    gtk3
    gtk-layer-shell
    gdk-pixbuf
  ];
  dataPath = lib.makeSearchPath "share" [
    glib
    gtk3
    gsettings-desktop-schemas
    shared-mime-info
    hicolor-icon-theme
  ];
  pixbufLoaders = "${gdk-pixbuf}/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache";
  fontconfigFile = "${fontconfig.out}/etc/fonts/fonts.conf";
in
stdenvNoCC.mkDerivation {
  pname = "gif-player";
  version = packageVersion;

  src = lib.cleanSource ../.;
  strictDeps = true;
  dontBuild = true;

  installPhase = ''
    runHook preInstall

    libexec="$out/libexec/gif-player"
    mkdir -p "$libexec" "$out/bin" "$out/share/applications" \
      "$out/share/fish/vendor_functions.d" "$out/share/fish/vendor_completions.d" \
      "$out/share/doc/gif-player"

    install -m644 \
      gif-script.py \
      gif-picker.py \
      gif-control.py \
      gif_player_paths.py \
      gif_player_ipc.py \
      gif_player_runtime.py \
      gif_player_runtime_patch.py \
      gif_player_runtime_guard.py \
      gif_player_bootstrap.py \
      gif_player_cli.py \
      gif_picker_entry.py \
      gif_control_entry.py \
      "$libexec/"
    find "$libexec" -type f -name '*.py' -exec sed -i '1{/^#!/d;}' {} +

    for spec in \
      "gif-player:gif_player_cli.py" \
      "gif-picker:gif_picker_entry.py" \
      "gif-control:gif_control_entry.py"; do
      name="''${spec%%:*}"
      script="''${spec#*:}"
      cat > "$out/bin/$name" <<PY
#!${python.interpreter}
import os
import runpy
import sys

sys.dont_write_bytecode = True


def prepend(name: str, value: str) -> None:
    current = os.environ.get(name)
    os.environ[name] = value if not current else f"{value}:{current}"


prepend("GI_TYPELIB_PATH", "${typelibPath}")
prepend("XDG_DATA_DIRS", "${dataPath}")
os.environ.setdefault("GDK_PIXBUF_MODULE_FILE", "${pixbufLoaders}")
os.environ.setdefault("FONTCONFIG_FILE", "${fontconfigFile}")
sys.path.insert(0, "$libexec")
runpy.run_path("$libexec/$script", run_name="__main__")
PY
      chmod +x "$out/bin/$name"
    done

    install -m644 gif.fish "$out/share/fish/vendor_functions.d/gif.fish"
    install -m644 completions/gif-player.fish \
      "$out/share/fish/vendor_completions.d/gif-player.fish"
    install -m644 data/*.desktop "$out/share/applications/"
    install -m644 NOTICE.md "$out/share/doc/gif-player/NOTICE.md"

    runHook postInstall
  '';

  doInstallCheck = true;
  installCheckPhase = ''
    runHook preInstallCheck

    export HOME="$TMPDIR/home"
    export XDG_RUNTIME_DIR="$TMPDIR/runtime"
    export XDG_CONFIG_HOME="$TMPDIR/config"
    export XDG_CACHE_HOME="$TMPDIR/cache"
    export XDG_DATA_HOME="$TMPDIR/data"
    mkdir -p "$HOME" "$XDG_RUNTIME_DIR"
    chmod 700 "$XDG_RUNTIME_DIR"

    test -f "${pixbufLoaders}"
    test -f "${fontconfigFile}"
    for required in \
      cairo-1.0 \
      Pango-1.0 \
      PangoCairo-1.0 \
      Atk-1.0 \
      HarfBuzz-0.0; do
      test -f "${runtimeTypelibs}/lib/girepository-1.0/$required.typelib"
    done
    find "$out" -type f -exec sha256sum {} + | sort > "$TMPDIR/out-before"
    "$out/bin/gif-player" --help >/dev/null
    "$out/bin/gif-player" doctor | grep -q 'GTK typelibs: OK'
    "$out/bin/gif-player" self-test | grep -q '"protocol": 2'
    test "$(stat -c %a "$XDG_RUNTIME_DIR/gif-player")" = 700

    if grep -R -E '/usr/bin/python3|/usr/bin/env|~/Scripts/Gif-Overlay|/nix/store/[^/]+-(glib|gobject-introspection|pygobject)-[^/]+-dev' \
      "$out/bin" "$out/libexec/gif-player"; then
      echo "non-hermetic or development path found in GIF Player output" >&2
      exit 1
    fi

    test ! -e "$out/libexec/gif-player/Gifs"
    test -f "$out/share/doc/gif-player/NOTICE.md"
    wayland_error="$(env -u WAYLAND_DISPLAY "$out/bin/gif-player" picker 2>&1 || true)"
    printf '%s\n' "$wayland_error" | grep -q 'WAYLAND_DISPLAY ist nicht gesetzt'
    find "$out" -type f -exec sha256sum {} + | sort > "$TMPDIR/out-after"
    cmp "$TMPDIR/out-before" "$TMPDIR/out-after"

    runHook postInstallCheck
  '';

  meta = {
    description = "GTK3 layer-shell GIF overlay supervisor for Wayland";
    homepage = "https://github.com/madebycli/GIF-Player";
    mainProgram = "gif-player";
    platforms = lib.platforms.linux;
  };
}
