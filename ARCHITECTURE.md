# Architecture

The application is one Rust executable. A daemon owns the Wayland connection, widgets, playback clocks, configuration and Unix socket. A single bounded decoder worker handles media loading; playback does not start a process per GIF. The optional Noctalia frontend is a thin Luau plugin. The core has no dependency on a Noctalia runtime or design state.

## Core and memory

`decode.rs` materializes GIF delta frames using palettes, transparency and disposal before resampling and premultiplying RGBA. Logical media dimensions are capped at 512 pixels on the longest axis, matching the old player; budget resampling preserves that logical geometry and every frame's duration. The clock uses absolute elapsed time and binary-searches a cumulative timeline, so late presentation skips directly to the current frame rather than accumulating drift. Zero delays become 100 ms; nonzero delays are at least 20 ms.

Decoded frames are shared by canonical source path with `Arc`. The global charged playback cache is bounded at 240 MiB, with 16 MiB reserved for first-frame thumbnails; per-asset playback reservations are at most 32 MiB. Those limits cover decoded cache data, not total process RSS: source/disposal canvases, fonts, compositor buffers and temporary resampling allocations are additional. Inputs are limited to 16 megapixel canvases, 256 MiB files, 100,000 frames and 256 active widgets. A bounded 32-job channel rejects overload cleanly. Stopped assets are cancelled, generation tokens reject obsolete decode results, and unused caches are freed. Frames are never silently dropped to fit a budget.

## Rendering

`wayland.rs` uses SCTK and wayland-client directly. Pixels are premultiplied native-endian ARGB8888 in `wl_shm`. Locked stationary widgets use compact, clipped layer surfaces. Editing, bounce, hop and explicitly selected fullscreen canvas use one transparent output canvas per layer. Each surface has two reusable SHM slots. Damage is the union of changed old/new rectangles; only damaged rows are cleared, then intersecting items are composited. Integer nearest-neighbor sampling avoids division in the hot pixel loop. No GPU runtime is required by the player.

Surfaces have exclusive zone -1, default Overlay layer and keyboard interactivity None. Locked input regions are empty. Editable canvases contain the union of unlocked GIF rectangles, never the entire transparent screen. Input-region commits are independent of frame callbacks, so locking does not wait for a visible animation frame. Replacement surfaces retain the preceding visible surface until their first buffer commits. Position, buffer, viewport and scale changes commit together. Output names, logical geometry, integer scale and optional fractional-scale/viewporter are tracked; the compositor retains ultimate stacking and session-lock authority.

A single poll loop combines Wayland, IPC, decoder notifications, precise frame/effect deadlines, persistence deadlines and peer timeouts. Frame callbacks gate presentation. Hidden, paused and offscreen animations do not cause continuous redraws. Bounce and hop are elapsed-time effects; paused media can still move. A four-Hz status notification ceiling avoids flooding frontends during motion.

## Frontends

`ui.rs` renders the independent Rust control surface through tiny-skia/fontdue and system fonts. It loads and retains fonts only while open, provides collection, active widgets, profiles and settings, and handles bounded thumbnails and on-demand hover animation. It uses its own carefully designed surface; it is not represented as a native Noctalia component.

`plugins/gif-player` uses the actual Noctalia 5 API 30: native `ui.*` widgets, host panels, translations, color roles and settings. One reconnecting `gif-player watch --frontend` process streams daemon snapshots. User actions use bounded argv-based `runAsync` requests. Slider commands coalesce; plugin snapshots share selection and state between the attached control panel and independent floating picker. Preview PNGs are produced by Rust and displayed by the host. Frame ticks run only during hover animation. Right-click edit requests use a frontend-neutral event/ack contract; when no managed frontend is connected, the Rust UI opens.

The CLI prefers an installed, running Noctalia panel and falls back to the standalone surface. Renderer code does not spawn Noctalia. The plugin is embedded in the executable for installation and is also distributed separately.

## Persistence and IPC

State and profile object shapes and IPC protocol 2 remain compatible with the old implementation. Unknown state fields are flattened and retained. Original migration files remain unchanged and are backed up before importing. JSON writes use exclusive temporary files, file fsync, rename and parent-directory fsync. Session data records every active ID and output; profiles validate their entire layout before replacing it.

IPC is newline-delimited JSON, with up to 128 nonblocking peers, a 1 MiB request bound, 5-second incomplete-request timeout and 10-second backpressure timeout. Snapshot streams coalesce pending updates. Runtime paths reject unsafe ownership/symlinks, daemon exclusivity uses flock and sockets are user-only. No TCP endpoint or shell interpolation is used.

The preserved Python branch permits regression comparison and rollback. Technical evidence lives in this repository; project context and handoff records live exclusively in `madebycli/master-context`.
