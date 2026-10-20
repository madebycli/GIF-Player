{ lib, rustPlatform, pkg-config, libxkbcommon, dejavu_fonts }:
rustPlatform.buildRustPackage {
  pname = "noctalia-gif-player";
  version = "1.5.0";
  src = lib.cleanSource ../.;
  cargoLock.lockFile = ../Cargo.lock;
  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ libxkbcommon ];
  GIF_PLAYER_DEFAULT_FONT = "${dejavu_fonts}/share/fonts/truetype/DejaVuSans.ttf";
  doCheck = true;
  postInstall = ''
    ln -s gif-player "$out/bin/noctalia-gif-player"
    ln -s gif-player "$out/bin/gif-picker"
    ln -s gif-player "$out/bin/gif-control"
    install -Dm644 data/gif-player-picker.desktop "$out/share/applications/gif-player-picker.desktop"
    install -Dm644 data/noctalia-gif-player.desktop "$out/share/applications/noctalia-gif-player.desktop"
    install -Dm644 data/gif-player-control.desktop "$out/share/applications/gif-player-control.desktop"
    install -Dm644 completions/gif-player.fish "$out/share/fish/vendor_completions.d/gif-player.fish"
    mkdir -p "$out/share/noctalia-gif-player/plugins"
    cp -r plugins/gif-player "$out/share/noctalia-gif-player/plugins/"
    cp NOTICE.md "$out/share/noctalia-gif-player/plugins/gif-player/NOTICE.md"
    mkdir -p "$out/share/doc/noctalia-gif-player"
    cp README.md NOTICE.md ARCHITECTURE.md PACKAGING.md "$out/share/doc/noctalia-gif-player/"
    cp -r docs "$out/share/doc/noctalia-gif-player/"
  '';
  meta = {
    description = "Noctalia GIF Player: native Rust Wayland overlays and optional Noctalia 5 UI";
    homepage = "https://github.com/madebycli/GIF-Player";
    mainProgram = "gif-player";
    platforms = lib.platforms.linux;
    # Preserve NOTICE.md. No new license is assigned by the packaging metadata.
  };
}
