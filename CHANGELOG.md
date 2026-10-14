# Changelog

## 1.2.0 — Detailed diagnostics and folder export

- Add timestamped JSONL lifecycle, IPC, decode, playback, cache, surface, buffer, input and persistence events, including frame/render/decompression timing in trace mode.
- Keep disk writes asynchronous, bound the queue and rotate five 8 MiB log files; report queue loss and writer failures.
- Export consistent log copies, sanitized runtime state, asset timing/storage, monitor/DPI details, system/process metrics and configuration validation into a new private folder, including after daemon shutdown.
- Run exports in a temporary worker so rendering/input keep running. Add persisted logging levels and export actions to CLI, native Noctalia settings and the standalone app.
- Extend real Wayland acceptance to assert exported frame/callback/input events.

## 1.1.0 — Quality, interaction and frontend corrections

- Preserve native GIF pixels under cache pressure through lossless anonymous frame backing; improve scaling with bilinear filtering and higher-resolution previews.
- Move drag input off the JSON dispatch path, apply snapping on release and coalesce frontend updates; repair both buffer history and last presented image to prevent movement trails.
- Replace picker pagination and per-tile buttons with continuous virtualized scrolling and image-click start/stop.
- Separate native picker, playback starter and settings; compact switches/actions and remove the repeated top toolbar.
- Add a normal, resizable xdg-shell application window and live palette/font synchronization with the actual Noctalia host.
- Install the visible native bar widget on an enabled bar, retain settings backups and support explicit bar selection and file-only installation.

## 1.0.0 — Noctalia GIF Player

- Replace Python/GTK playback and menus with an independently written Rust daemon, layer-shell renderer, decoder, CLI, IPC, persistence and standalone control surface.
- Add a genuine Noctalia 5 API 30 plugin with native bar widget, attached controls, independent floating collection picker, shared streamed state and English/German translations.
- Preserve IPC v2, legacy commands, multi-instance IDs, motion effects, mirroring, opacity, speed, offscreen placement, layouts and configuration migration.
- Add output selection, fractional scaling, explicit fullscreen canvas, layer and visibility control, automatic session restoration and bounded shared caching.
- Package for Cargo/Make, Arch, NixOS flakes and Home Manager; add Rust, real Wayland input and native Noctalia validation.
- Preserve the previous implementation on `legacy/python-before-rust` and retain NOTICE.md without assigning a new license.

Measured performance and unverified MangoWM hardware behavior are documented in `docs/acceptance.md`.
