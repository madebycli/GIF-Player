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

Requires Noctalia 5 with plugin API **30 or later** (tested with 5.2.1). `gif-player install-plugin` installs/enables the local plugin and adds its widget to an enabled bar, preserving the other widgets. It saves a backup of Noctalia's settings before editing them. With multiple bars, use `gif-player install-plugin --bar flying` to choose the target; `--no-bar` installs only the files. Widget placement remains editable through Noctalia's normal bar manager.

The **picker**, **playback starter** and **settings** are separate native host panels. The picker is a continuous, virtualized gallery: click an image to start its GIF; click again to stop its instances. It has no per-tile Add/Stop buttons, pagination or global action toolbar. Active images have a theme-colored outline. The control panel holds per-GIF/global playback tools; the settings panel holds the compact restore switch, layouts and application/daemon actions.

```sh
gif-player picker
gif-player control
gif-player settings
gif-player app
```

`app` opens a **normal xdg-shell application window** with sidebar navigation, all four pages and compositor-managed resizing/tiling. It does not request a floating layer-shell role. Its colors and font follow the current Noctalia host through the plugin's generic appearance IPC; changes are applied live. The last appearance is retained for use without Noctalia. Standalone controls remain Rust and do not require the host. `--standalone` explicitly selects the Rust picker/control/settings frontend.

All Noctalia panels use host `ui.*` components, typography, color roles and transitions. Left click on the bar widget opens playback controls, right click opens the picker; settings are available through the small gear action. The bar remains visible with zero active GIFs, including on vertical bars. See [plugin documentation](plugins/gif-player/README.md).

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

Locked GIFs use an **empty input region** and never request keyboard focus. In edit mode, only GIF rectangles capture pointer input, including on a transparent fullscreen canvas. Drag with the left button, resize with the wheel, double click to lock, and right click for controls. Dragging follows the pointer directly; edge/center snapping is applied on release within 14 logical pixels. Frontend updates during motion are coalesced instead of rebuilding controls for every mouse event. Coordinates are output-relative logical pixels; negative and offscreen positions are valid. The default layer is `overlay`; background, bottom and top are also available.

The collection, active-widget, profile and settings pages are available in both native Noctalia panels and the standalone UI. Playback retains original source pixels and timing. If decoded frames exceed the RAM budget, lossless anonymous disk backing replaces resolution reduction. Display geometry stays compatible with saved layouts. Thumbnails decode only the first frame; hover previews animate on demand. Profiles save, replace, rename, delete and restore layouts. Global controls affect all active widgets.

## Configuration and autostart

Configuration stays under `$XDG_CONFIG_HOME/gif-player`: `state.json`, `profiles.json`, `settings.json`, and `session.json`. Unknown legacy fields survive updates. Missing state/profile files are imported from `~/.config/gif-widget` with byte-preserving `.legacy-backup` files. Existing current files are never silently replaced by that migration. Invalid JSON produces an actionable error instead of erasing it.

The default session restore setting is enabled. `gif-player daemon --stay` runs in a graphical session and restores active widgets; `--no-restore` suppresses restoration. On-demand daemons exit when empty; `--stay` is for an autostart/user service. The Home Manager module configures that service. The socket remains `$XDG_RUNTIME_DIR/gif-player/daemon.sock` with mode 0600; the runtime directory is 0700.

## Verification and performance

On an unmodified headless Sway session, the automated suite verifies fullscreen overlay pixels, click-through, keyboard/scroll pass-through, edit-region isolation, dragging, offscreen positioning, two outputs, fractional scale, native-resolution cache pressure, trailing-buffer cleanup, normal application-window mapping and output removal. The real Noctalia 5.2.1 host has also loaded the native service, bar, picker and control panel. Cargo checks and Nix checks run in CI.

The historical 1.0.0 60-second comparison of three 512×512 GIFs at 20 FPS on the same Sway setup measured **4.48% CPU / 82.4 MiB RSS** for Rust versus **27.18% CPU / 129.4 MiB peak RSS** for the preserved Python version. Paused Rust playback measured 0% CPU; after stopping all GIFs, RSS fell to 4.6 MiB. Those historical measurements apply to the 1.0.0 workload; they are not a performance claim for the new native-resolution/lossless backing and bilinear renderer.

**MangoWM hardware acceptance remains unverified:** this environment has no `/dev/dri`; unchanged MangoWM 0.18.0 cannot initialize its required GLES/scenefx renderer. Noctalia integration and Sway input tests do not establish Mango fullscreen/game behavior. Niri and Hyprland have not been run here. See [acceptance evidence and reproduction](docs/acceptance.md) for exact limits and commands.

## Copyright

[NOTICE.md](NOTICE.md) is preserved. No new license is granted for this repository or plugin, and no third-party artwork or GIFs are distributed. [Dependency license texts](docs/third-party-licenses.md) and [reference provenance](docs/provenance.md) retain third-party notices. Local plugin installation does not imply publication in Noctalia's official catalog.

## Detailed logs

For a reproduction with individual frames, buffers and mouse events:

```sh
gif-player logs level trace
# Reproduce the problem, then export to a new directory:
gif-player logs export ~/gif-player-diagnostics
gif-player logs level info
```

The settings panels and standalone app also provide **Logs** and **Export logs** buttons. Without a directory, export creates a timestamped report under `$XDG_STATE_HOME/gif-player/exports` (default `~/.local/state/gif-player/exports`) and returns its path. `gif-player logs path` prints the continuous log directory. Reports also work after a daemon crash without starting another daemon.

Logs are asynchronous and rotate at 8 MiB per file (five files, approximately 40 MiB maximum). Trace mode intentionally records far more data and costs more CPU/disk bandwidth; use `info` for ordinary playback. Exports contain GIF paths and selected session details; unknown legacy/config fields and a full environment dump are excluded. See [diagnostic format, contents and troubleshooting](docs/diagnostics.md).
