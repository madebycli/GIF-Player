# GIF Player for Noctalia

This directory contains the Noctalia v5 frontend for GIF Player.

Noctalia is intentionally an additional UI, not the renderer. The GIF Player daemon remains the single owner of overlay state and Wayland surfaces. That preserves the same IPC contract for the standard frontend, CLI and Noctalia.

## Noctalia structure

The plugin follows Noctalia's own service/picker/widget pattern rather than reproducing the old GTK visual style.

- `service.luau`: one bridge from GIF Player daemon status into `noctalia.state`
- `picker.luau`: GIF browser with search, categories, setups/profiles and running-instance markers
- `controls.luau`: runtime controls for active overlays
- `widget.luau`: minimal bar launcher
- `Settings -> Plugins`: static GIF directory and launch defaults

The bar widget uses:

- left click: GIF picker
- right click: active overlay controls
- middle click: plugin settings

## Picker behavior

The picker carries forward the original picker model while using Noctalia-native controls, palette tokens, spacing, glyphs and panel behavior:

- local GIF thumbnail grid
- fuzzy search
- category filtering from GIF subdirectories
- duplicate launches of an already-running GIF
- active-instance count on thumbnails
- setup/profile save, apply, overwrite, rename and delete
- launch defaults from Noctalia plugin settings

Lazy animated hover previews are part of the target design but depend on the native core preview-cache command and are not considered complete until that command lands.

## Runtime controls

Each active overlay exposes lock/unlock, pause/play, scale, opacity, speed, jump rate, bounce, hop, auto-jump, horizontal/vertical flip, snap, exact position, reset and close. Global edit, lock and stop actions remain available.

## Runtime contract

The plugin invokes `gif-player` through Noctalia's argv-based process API for dynamic arguments. A single service owns status integration so the bar and panels do not each poll the daemon independently.

During the migration branch, `gif-player watch` is a long-lived compatibility process that emits JSONL snapshots and internally polls the legacy daemon. The final native Rust daemon will keep the same `watch` CLI surface but publish events directly without polling.

Locked overlays must remain true pointer pass-through surfaces. The native target implements this with an empty Wayland input region on an overlay-layer `wlr-layer-shell` surface. Noctalia never owns that rendering surface.

## Compatibility target

`plugin_api = 28` is deliberate:

- API 24 provides argv-based `runAsync` for safe dynamic paths and values.
- API 28 provides native Noctalia context menus.
- API 30 is currently unreleased and is not required.

The native renderer targets the `wlr-layer-shell` protocol directly. MangoWM is a primary test compositor, while Niri, Sway, Hyprland and other compatible compositors remain supported targets.
