<p align="center">
  <img src="assets/readme-banner.svg" alt="GIF Player — animated Wayland desktop overlays" width="100%">
</p>

<p align="center">
  <a href="https://github.com/madebycli/GIF-Player/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/madebycli/GIF-Player/actions/workflows/ci.yml/badge.svg?branch=main"></a>
  <img alt="Python 3.10+" src="https://img.shields.io/badge/Python-3.10%2B-3776AB?logo=python&logoColor=white">
  <img alt="Wayland" src="https://img.shields.io/badge/display-Wayland-43f1e8">
  <img alt="GTK3" src="https://img.shields.io/badge/UI-GTK3-7c8cff">
</p>

<p align="center">
  Lightweight animated GIF widgets for modern Wayland desktops.
</p>

GIF Player turns a local GIF collection into independent desktop overlays. A single supervisor manages every widget, while the picker, control panel, and command-line interface provide a simple way to launch and arrange them.

> GIF Player requires a Wayland compositor with layer-shell support. Niri, Sway, Hyprland, and Wayfire are suitable examples. X11 is not supported.

## Preview

<table>
  <tr>
    <td width="50%" align="center">
      <img src="Pics/GIF-picker.png" alt="GIF Player picker" width="100%">
      <br><sub><strong>Picker</strong> — browse and launch your local GIF collection.</sub>
    </td>
    <td width="50%" align="center">
      <img src="Pics/Gif%20in%20Action.png" alt="GIF overlays on a Wayland desktop" width="100%">
      <br><sub><strong>Desktop overlays</strong> — position and control multiple widgets independently.</sub>
    </td>
  </tr>
</table>

## Highlights

- Multiple independent widgets, including duplicate instances of the same GIF
- Graphical picker and live control panel
- Drag, lock, click-through, scale, opacity, speed, flip, bounce, and jump controls
- Saved positions, reusable profiles, and multi-monitor placement
- Shared frame decoding for efficient duplicate widgets
- Command-line control through a private Unix-socket protocol
- XDG-compliant configuration, data, cache, and runtime paths
- Nix, Arch Linux, and Fedora packaging

GIF files are never bundled with the project. Bring your own local collection.

## Install

### Nix

Run without installing:

```bash
nix run github:madebycli/GIF-Player
```

Install into the current profile:

```bash
nix profile add github:madebycli/GIF-Player#gif-player
gif-player doctor
```

The flake supports `x86_64-linux` and `aarch64-linux`.

### NixOS

```nix
{
  inputs.gif-player.url = "github:madebycli/GIF-Player";

  environment.systemPackages = [
    inputs.gif-player.packages.${pkgs.system}.gif-player
  ];
}
```

### Arch Linux

```bash
git clone https://github.com/madebycli/GIF-Player.git
cd GIF-Player/packaging/arch
makepkg --syncdeps --cleanbuild
sudo pacman -U ./gif-player-*.pkg.tar.zst
```

### Fedora

The RPM recipe lives in [`packaging/fedora/gif-player.spec`](packaging/fedora/gif-player.spec). See [`PACKAGING.md`](PACKAGING.md) for build requirements and packaging notes.

## Quick start

Create the default GIF directory and add a file:

```bash
mkdir -p ~/.local/share/gif-player/gifs
cp ~/Pictures/mascot.gif ~/.local/share/gif-player/gifs/
```

Open the picker:

```bash
gif-player
```

Launch directly by unique filename or path:

```bash
gif-player mascot
gif-player run ~/Pictures/mascot.gif
gif-player run ~/Pictures/mascot.gif --monitor 1
```

Starting the same GIF again creates another widget with its own ID.

## Everyday commands

| Command | Action |
|---|---|
| `gif-player` | Open the picker |
| `gif-player NAME` | Launch a GIF by unique filename stem |
| `gif-player list` | List active widget IDs |
| `gif-player control` | Open the control panel |
| `gif-player edit` | Unlock all widgets |
| `gif-player lock` | Lock all widgets and enable click-through |
| `gif-player ipc ID ACTION` | Control one widget |
| `gif-player all ACTION` | Control every widget |
| `gif-player stop-all` | Close all widgets |
| `gif-player doctor` | Check runtime dependencies |
| `gif-player self-test` | Show resolved paths and runtime status |

Common widget actions:

```text
status  lock  unlock  pause  play  reset  quit
move X Y        move-by DX DY       corner POSITION
scale N         opacity N           speed N
flip MODE       bounce              stop-bounce
hop             jump                jump-rate SECONDS
```

Example:

```bash
gif-player mascot
gif-player list
gif-player ipc mascot scale 1.4
gif-player ipc mascot opacity 0.8
gif-player ipc mascot bounce
gif-player all lock
```

## GIF collection

The collection directory is resolved in this order:

1. `--gif-dir DIR`
2. `GIF_PLAYER_GIF_DIR`
3. `$XDG_DATA_HOME/gif-player/gifs`
4. `~/.local/share/gif-player/gifs`

Use another collection for one command:

```bash
gif-player --gif-dir ~/Pictures/Gifs mascot
```

Subdirectories are supported. When several files share the same filename stem, GIF Player reports the ambiguity instead of choosing one silently.

## Data locations

| Data | Default path |
|---|---|
| GIF collection | `$XDG_DATA_HOME/gif-player/gifs/` |
| State and profiles | `$XDG_CONFIG_HOME/gif-player/` |
| Thumbnail cache | `$XDG_CACHE_HOME/gif-player/thumbs/` |
| Socket, lock, and daemon log | `$XDG_RUNTIME_DIR/gif-player/` |

The runtime directory is private to the current user. Configuration, media, cache files, and logs are never written into the Nix store.

## Development

```bash
nix develop
python3 -m unittest discover -s tests -v
ruff check .
nix flake check --print-build-logs
nix build .#gif-player --print-build-logs
./result/bin/gif-player doctor
```

The automated checks cover the CLI, isolated XDG paths, runtime permissions, protocol behavior, frame handling, animation timing, package closure, GTK typelibs, and reproducible Nix builds. A real Wayland session is still required for visual end-to-end testing.

## Project status and copyright

GIF Player is independently developed and is not affiliated with another GIF-overlay project. It does not include third-party GIFs, anime media, or artwork.

No general open-source license is currently granted for this repository. Default copyright law applies. See [`NOTICE.md`](NOTICE.md) for the complete notice.
