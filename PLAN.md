# GIF Player Beta Plan

Active branch: `beta/native-noctalia-v2`

The authoritative project plan lives in `madebycli/master-context` at `projects/gif-player/PLAN.md`. This repository copy is the implementation-facing summary and must stay compatible with that context.

## Goal

Replace the runtime renderer/supervisor path with one native Rust daemon that owns all Wayland layer surfaces, while preserving IPC v2 and all current behavior. Keep the standard frontend available and add Noctalia as a second optional frontend.

## Non-negotiable behavior

- true locked pointer click-through using an empty Wayland input region
- overlay layer above normal/fullscreen application surfaces
- manual finite positions remain unclamped
- duplicate instances supported
- scale, opacity, speed, flip, pause, snap, bounce, hop, auto-jump and profiles preserved
- multi-output support by connector name plus monitor-index compatibility
- core never invokes Noctalia or another frontend

## Implementation order

1. Establish a Rust workspace and frontend-neutral core modules with tests.
2. Port state/geometry/input-region state machine and IPC types.
3. Build one multi-widget daemon with one Wayland connection/event loop.
4. Add shared decoded GIF cache and global memory budget.
5. Port persistence/profiles/catalog/events.
6. Wire CLI to the native daemon.
7. Finish Noctalia picker/controls/output-context integration.
8. Build GTK-free Nix core package while keeping the legacy frontend optional.
9. Verify MangoWM and Niri fullscreen/click-through behavior.
10. Benchmark CPU/RSS and optimize based on measurements.

## Acceptance targets

Reference MangoWM workload: 3 active GIFs, up to 512x512, typical 15-30 FPS, 60 seconds after warm start.

- average GIF Player CPU <= 5%
- total native core RSS <= 300 MiB
- idle near 0% CPU
- repeated use of one GIF shares decoded data

Do not merge this branch into `main` until native CI, Nix packaging, real Wayland click-through/fullscreen tests and performance measurements pass.