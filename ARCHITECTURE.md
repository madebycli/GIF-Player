# Architecture

The application is one Rust executable. A daemon owns the Wayland connection, widgets, playback clocks, configuration and Unix socket. A single bounded decoder worker handles media loading; playback does not start a process per GIF. The optional Noctalia frontend is a thin Luau plugin. The core has no dependency on a Noctalia runtime or design state.

## Core and memory

`decode.rs` materializes GIF delta frames using palettes, transparency and disposal before resampling and premultiplying RGBA. Legacy layout geometry remains capped at 512 logical pixels, but decoded playback pixels keep the original GIF canvas dimensions. Cache pressure spills frames into lossless zlib-compressed anonymous disk storage; a bounded resident LRU supplies current frames. Cache limits never silently reduce playback resolution or discard durations. The clock uses absolute elapsed time and binary-searches a cumulative timeline, so late presentation skips directly to the current frame rather than accumulating drift. Zero delays become 100 ms; nonzero delays are at least 20 ms.

Decoded frames are shared by canonical source path with `Arc`. The global charged playback cache is bounded at 240 MiB, with 16 MiB reserved for first-frame thumbnails; per-asset reservations normally use at most 32 MiB, increased when needed for one larger native frame within the global budget. Anonymous backing is limited to 1 GiB compressed data per asset and is freed on last handle close. Those limits cover decoded cache data, not total process RSS: source/disposal canvases, fonts, compositor buffers and temporary resampling allocations are additional. Inputs are limited to 16 megapixel canvases, 256 MiB files, 100,000 frames and 256 active widgets. A bounded 32-job channel rejects overload cleanly. Stopped assets are cancelled, generation tokens reject obsolete decode results, and unused caches are freed. Frames are never silently dropped to fit a budget.

## Rendering

`wayland.rs` uses SCTK and wayland-client directly. Pixels are premultiplied native-endian ARGB8888 in `wl_shm`. Locked stationary widgets use compact, clipped layer surfaces. Editing, bounce, hop and explicitly selected fullscreen canvas use one transparent output canvas per layer. Each surface has two reusable SHM slots. Damage is the union of changed old/new rectangles; only damaged rows are cleared, then intersecting items are composited. Bilinear resampling uses precomputed horizontal sampling coordinates; unscaled pixels use the direct fast path. Damage combines both recycled-buffer repair and change from the last presented scene so partial SHM uploads cannot leave trails. No GPU runtime is required by the player.

Surfaces have exclusive zone -1, default Overlay layer and keyboard interactivity None. Locked input regions are empty. Editable canvases contain the union of unlocked GIF rectangles, never the entire transparent screen. Input-region commits are independent of frame callbacks, so locking does not wait for a visible animation frame. Replacement surfaces retain the preceding visible surface until their first buffer commits. Position, buffer, viewport and scale changes commit together. Output names, logical geometry, integer scale and optional fractional-scale/viewporter are tracked; the compositor retains ultimate stacking and session-lock authority.

A single poll loop combines Wayland, IPC, decoder notifications, precise frame/effect deadlines, persistence deadlines and peer timeouts. Frame callbacks gate presentation. Hidden, paused and offscreen animations do not cause continuous redraws. Bounce and hop are elapsed-time effects; paused media can still move. A four-Hz status notification ceiling avoids flooding frontends during motion.

## Frontends

`ui.rs` renders the independent Rust control surface through tiny-skia/fontdue and system fonts. It loads and retains fonts only while open, provides separated floating entry points plus a normal xdg-shell application window, collection, active widgets, profiles and settings, and handles bounded thumbnails and on-demand hover animation. It uses its own carefully designed surface; it is not represented as a native Noctalia component.

`plugins/gif-player` uses the actual Noctalia 5 API 30: native `ui.*` widgets, host panels, translations, color roles and settings. One reconnecting `gif-player watch --frontend` process streams daemon snapshots. User actions use bounded argv-based `runAsync` requests. Slider commands coalesce; plugin snapshots share selection and state between the attached control panel and independent floating picker. Preview PNGs are produced by Rust and displayed by the host. Frame ticks run only during hover animation. Right-click edit requests use a frontend-neutral event/ack contract; when no managed frontend is connected, the Rust UI opens.

The new install helper selects an enabled native bar, ignores bar-order metadata, preserves existing entries/settings, and backs up Noctalia settings. A host service sends actual color roles and font family through generic appearance IPC only when changed.

The CLI prefers an installed, running Noctalia panel and falls back to the standalone surface. Renderer code does not spawn Noctalia. The plugin is embedded in the executable for installation and is also distributed separately.

## Persistence and IPC

State and profile object shapes and IPC protocol 2 remain compatible with the old implementation. Unknown state fields are flattened and retained. Original migration files remain unchanged and are backed up before importing. JSON writes use exclusive temporary files, file fsync, rename and parent-directory fsync. Session data records every active ID and output; profiles validate their entire layout before replacing it.

IPC is newline-delimited JSON, with up to 128 nonblocking peers, a 1 MiB request bound, 5-second incomplete-request timeout and 10-second backpressure timeout. Snapshot streams coalesce pending updates. Runtime paths reject unsafe ownership/symlinks, daemon exclusivity uses flock and sockets are user-only. No TCP endpoint or shell interpolation is used.

The preserved Python branch permits regression comparison and rollback. Technical evidence lives in this repository; project context and handoff records live exclusively in `madebycli/master-context`.

## Diagnostics

A bounded nonblocking channel sends serialized JSONL records to one Rust writer thread. Severity checks happen before field construction. Each record is capped at 8 KiB; the 256-slot queue, 32 KiB write buffer and five 8 MiB rotating files bound logging resources. A temporary export worker asks the writer for consistent flushed copies, then writes private report files. Completion signals the existing Unix wake stream; snapshots/GUI receive the result without waiting for disk work in the Wayland loop. A separate advisory lock prevents competing writers. Logs expose failures/dropped events and preserve previous sessions. See docs/diagnostics.md for the public format and data scope.
