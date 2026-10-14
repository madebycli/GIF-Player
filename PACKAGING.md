# Installation and packaging

## NixOS flake

Add an input and module to the existing system flake:

```nix
inputs.gif-player.url = "github:madebycli/GIF-Player";
# Inside the existing nixosSystem modules list:
modules = [ inputs.gif-player.nixosModules.default {
  programs.noctalia-gif-player.enable = true;
} ];
```

The NixOS module installs the executable, desktop entries, completion and plugin payload. Run `gif-player install-plugin` as the desktop user. When Noctalia is available, this enables the local plugin and attaches the widget to an enabled bar; use `--bar flying` to select a named bar or `--no-bar` to install files only. It keeps a settings backup. It does not start a system-wide graphical daemon.

## Home Manager

In the user's existing Home Manager modules list:

```nix
modules = [ inputs.gif-player.homeManagerModules.default {
  services.noctalia-gif-player = {
    enable = true;
    installNoctaliaPlugin = true; # default: install local plugin via xdg.dataFile
    gifDirectory = "/home/me/Pictures/GIFs"; # optional
    systemdTarget = "graphical-session.target"; # compositor session target if different
  };
} ];
```

The module installs a user service `noctalia-gif-player`, started with `daemon --stay` after the selected graphical target. Your compositor session must export `WAYLAND_DISPLAY` and `XDG_RUNTIME_DIR` to the systemd user environment and start that target. A typical compositor startup uses `systemctl --user import-environment WAYLAND_DISPLAY XDG_RUNTIME_DIR` and `systemctl --user start graphical-session.target`. Noctalia remains optional; set `installNoctaliaPlugin = false` for standalone use.

```sh
systemctl --user status noctalia-gif-player
journalctl --user -u noctalia-gif-player
```

For manual installation: `nix profile add github:madebycli/GIF-Player#gif-player`. `nix build .#noctalia-plugin` produces just the plugin payload. The player closure excludes GTK, Python, Qt, Quickshell, Electron and Chromium. Nix links libxkbcommon and supplies a DejaVu font path at build time.

## Arch Linux

```sh
sudo pacman -S --needed base-devel rust cargo pkgconf libxkbcommon ttf-dejavu
git clone https://github.com/madebycli/GIF-Player.git
cd GIF-Player/packaging/arch
makepkg -si
gif-player install-plugin
```

The recipe builds from the containing checkout with `Cargo.lock`, runs Rust tests and installs aliases `gif-picker`, `gif-control` and `noctalia-gif-player`. It is a local-checkout recipe; no AUR publication or binary repository is implied. Noctalia and fontconfig are optional. The package keeps the repository's unlicensed copyright status.

## Cargo / Make

Rust 1.86+, pkg-config, libxkbcommon headers/library and a TTF/OTF font are required. Debian/Ubuntu build packages are `build-essential pkg-config libxkbcommon-dev fonts-dejavu-core`; Wayland is loaded dynamically, so no GTK development packages are needed.

```sh
cargo install --path . --locked
# Or system integration:
cargo build --release --locked
sudo make PREFIX=/usr/local install
gif-player install-plugin
```

`make DESTDIR=/tmp/gif-package PREFIX=/usr install` stages all files without modifying the system. `make uninstall` removes that installation. `cargo install` installs the binary only, but it embeds the complete Noctalia plugin and its notice. Fonts are selected through optional fontconfig, common DejaVu paths, a Nix font fallback, or `settings.json`'s `font_file` setting. `gif-player app` opens a normal compositor-managed application window; floating picker, control and settings remain separately callable.

Do not run the player as root. Start it inside the target user's Wayland session. Automatic startup from a compositor can simply execute `gif-player daemon --stay`; Home Manager provides supervised startup and restart.

## Validation

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --release --locked --examples
python3 tools/headless_acceptance.py
noctalia plugins lint plugins/gif-player
nix flake check --no-write-lock-file
nix build .#gif-player .#noctalia-plugin --no-write-lock-file
```

The headless acceptance script requires Sway, grim and a non-root Linux user. Python is used only by that development test runner and measurement script, never by the installed player. x86_64 Linux builds were executed here; aarch64 outputs are defined/evaluated and require an aarch64 builder for compilation. See [acceptance evidence](docs/acceptance.md).
