# Noctalia GIF Player

A native Rust GIF player for transparent Wayland overlays, with a real Noctalia 5 plugin and an independent Rust control surface. MangoWM is the primary target; the engine requires `wlr-layer-shell`, not Noctalia, GTK, Python, Qt or Quickshell.

The Python implementation is preserved on [`legacy/python-before-rust`](https://github.com/madebycli/GIF-Player/tree/legacy/python-before-rust). The Rust implementation replaces it on `main`. Existing IPC v2 commands and configuration paths are retained.

## Install

### Nix / NixOS

```sh
nix profile add github:madebycli/GIF-Player#gif-player
gif-player install-plugin
```

For NixOS and Home Manager modules, autostart and plugin installation, see [PACKAGING.md](PACKAGING.md). The flake provides `gif-player`, `noctalia-plugin`, NixOS and Home Manager modules for x86_64 and aarch64 Linux.

### Cargo

Install Rust 1.86 or newer, pkg-config, libxkbcommon development files and a TrueType/OpenType system font. On Arch: `sudo pacman -S rust cargo pkgconf libxkbcommon ttf-dejavu`.

```sh
git clone https://github.com/madebycli/GIF-Player.git
cd GIF-Player
cargo install --path . --locked
gif-player install-plugin
```

For desktop launchers, aliases, Fish completion and documentation: `cargo build --release --locked && sudo make install`. Arch packaging is available in `packaging/arch`; run `makepkg -si` there in a full checkout.

### Native Noctalia 5 UI

Requires Noctalia 5 with plugin API **30 or later** (tested with 5.2.1). Enable `madebycli/gif-player` under Settings → Plugins. Add its **player** widget through Noctalia's normal bar widget manager. Left click opens the native control panel; right click opens the floating picker. Both use Noctalia's own controls, typography, icons, color roles, spacing and panels. No external imitation is embedded.

Open the floating picker independently:

```sh
gif-player picker
# or: noctalia msg panel-toggle madebycli/gif-player:picker
```

The plugin reads the same daemon state as the CLI and standalone UI. Its executable and collection directory can be configured through Noctalia's native plugin settings. English and German translations are included. See [plugin documentation](plugins/gif-player/README.md).

Without Noctalia, `gif-player picker` and `gif-player control` open the Rust UI automatically. `--standalone` explicitly selects it.

## Use

```sh
gif-player --gif-dir ~/Pictures/GIFs run bird --id bird-left --output DP-1
gif-player --gif-dir ~/Pictures/GIFs run bird --id bird-right --state '{"x":600,"y":200,"scale":0.8}'
gif-player ipc bird-left move 120 240
gif-player ipc bird-left opacity 0.6
gif-player ipc bird-left flip h
gif-player ipc bird-left speed 1.5
gif-player ipc bird-left bounce
gif-player ipc bird-left hop
gif-player all pause
gif-player edit
gif-player lock
gif-player profile save desk
gif-player profile apply desk
gif-player stop-all
```

GIF files are user supplied. The collection is searched recursively, with case-insensitive names; ambiguous names require a path. Directory precedence: `--gif-dir`, `GIF_PLAYER_GIF_DIR`, saved collection setting, an existing `~/Scripts/Gif-Overlay/Gifs`, then `$XDG_DATA_HOME/gif-player/gifs`.

All legacy entry points remain: `run`, `ipc`, `all`, `list`, `edit`, `lock`, `stop-all`, `kill-all`, `picker`, `control`, `daemon`, `doctor`, `self-test`. Short forms `gif-player bird` and `gif-player bird pause` work. `kill-all` stops all widgets safely; `quit-daemon` stops the supervisor. Multiple instances have separate IDs and share decoded frames.

Widget actions include status, lock/unlock/toggle, pause/play/resume, scale (0.1–5), opacity (0–1), speed (0.1–10), move/move-by, corners/center, horizontal/vertical mirroring, bounce/stop-bounce, hop, automatic jump/jump-rate, reset and quit. Additional actions control visibility, output, layer and fullscreen canvas. See `gif-player --help` and [IPC reference](docs/ipc.md).

Locked GIFs use an **empty input region** and never request keyboard focus. In edit mode, only GIF rectangles capture pointer input, including on a transparent fullscreen canvas. Drag with the left button, resize with the wheel, double click to lock, and right click for controls. Edge and center snapping uses a 14 logical pixel threshold. Coordinates are output-relative logical pixels; negative and offscreen positions are valid. The default layer is `overlay`; background, bottom and top are also available.

The collection, active-widget, profile and settings pages are available in both native Noctalia panels and the standalone UI. Thumbnails decode only the first frame; hover previews animate on demand. Profiles save, replace, rename, delete and restore layouts. Global controls affect all active widgets.

## Configuration and autostart

Configuration stays under `$XDG_CONFIG_HOME/gif-player`: `state.json`, `profiles.json`, `settings.json`, and `session.json`. Unknown legacy fields survive updates. Missing state/profile files are imported from `~/.config/gif-widget` with byte-preserving `.legacy-backup` files. Existing current files are never silently replaced by that migration. Invalid JSON produces an actionable error instead of erasing it.

The default session restore setting is enabled. `gif-player daemon --stay` runs in a graphical session and restores active widgets; `--no-restore` suppresses restoration. On-demand daemons exit when empty; `--stay` is for an autostart/user service. The Home Manager module configures that service. The socket remains `$XDG_RUNTIME_DIR/gif-player/daemon.sock` with mode 0600; the runtime directory is 0700.

## Verification and performance

On an unmodified headless Sway session, the automated suite verifies fullscreen overlay pixels, click-through, keyboard/scroll pass-through, edit-region isolation, dragging, offscreen positioning, two outputs, fractional scale and output removal. The real Noctalia 5.2.1 host has also loaded the native service, bar, picker and control panel. Cargo checks and Nix checks run in CI.

A 60-second comparison of three 512×512 GIFs at 20 FPS on the same Sway setup measured **4.48% CPU / 82.4 MiB RSS** for Rust versus **27.18% CPU / 129.4 MiB peak RSS** for the preserved Python version. Paused Rust playback measured 0% CPU; after stopping all GIFs, RSS fell to 4.6 MiB. These measurements apply to that workload, not every GIF or compositor.

**MangoWM hardware acceptance remains unverified:** this environment has no `/dev/dri`; unchanged MangoWM 0.18.0 cannot initialize its required GLES/scenefx renderer. Noctalia integration and Sway input tests do not establish Mango fullscreen/game behavior. Niri and Hyprland have not been run here. See [acceptance evidence and reproduction](docs/acceptance.md) for exact limits and commands.

## Copyright

[NOTICE.md](NOTICE.md) is preserved. No new license is granted for this repository or plugin, and no third-party artwork or GIFs are distributed. [Dependency license texts](docs/third-party-licenses.md) and [reference provenance](docs/provenance.md) retain third-party notices. Local plugin installation does not imply publication in Noctalia's official catalog.
