# GIF Player for Noctalia

This directory contains the Noctalia v5 frontend for GIF Player.

The plugin intentionally does not render the desktop overlays itself. Noctalia's current plugin surface API does not expose the Wayland input-region control required for a true click-through overlay. The plugin therefore owns the user interface while the GIF Player runtime owns the Wayland surfaces.

## Included UI

- Bar widget with active-overlay count
- Attached manager panel
- Searchable GIF collection with previews
- Launch defaults in Noctalia settings
- Per-overlay lock, pause, bounce, hop, auto-jump, reset, flip, scale, opacity, speed, jump-rate, snap and close controls
- Global edit, lock and stop controls

## Runtime contract

The plugin invokes the `gif-player` executable through Noctalia's argv-based process API and consumes protocol-v2 JSON output. It requires the matching GIF Player runtime from this repository.

Locked overlays must remain true pointer pass-through surfaces: clicks over transparent or visible GIF pixels must continue to the next Wayland surface underneath. This invariant belongs to the native renderer and must be verified on a real Wayland compositor before the GTK renderer can be removed.

## Local development

Point a Noctalia local plugin source at this directory, then add the `GIF Player` bar widget. Left-click opens the manager, middle-click toggles edit/lock for all overlays, and right-click opens plugin settings.
