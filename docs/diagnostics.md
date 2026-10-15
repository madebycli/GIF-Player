# Detailed diagnostic logs

Version 1.2 adds native Rust diagnostics to the actual engine. No Python service, external logging runtime or process per GIF is installed.

## Record a problem

After updating the engine, restart its existing daemon once. Enable trace **before** loading the GIF if you need its complete decode/disposal history:

```sh
gif-player logs level trace
gif-player run bird
# Reproduce playback, drag, resize or click-through problems.
gif-player logs export ~/gif-player-diagnostics
gif-player logs level info
```

The target must not exist; existing reports are never overwritten. Paths with spaces and Unicode work. Omit the target to create a timestamped folder and get its absolute path in the JSON response. The standalone app and native Noctalia settings provide logging-level and export buttons; the Noctalia notification and app footer show the result path.

Live level changes are persisted in settings.json. Available levels are off, error, warn, info (default), debug and trace. The UI offers named choices for every level; there is no hidden cycling order. Diagnostics are grouped in an expandable settings section. To diagnose startup before IPC is available:

```sh
GIF_PLAYER_LOG=trace gif-player daemon --stay
```

The environment override applies at daemon startup; subsequent IPC level changes still take effect. With a user service, set that variable in its service environment or use the persisted level. A failed startup, caught fatal error or panic is recorded when the log directory is writable. Panic records include a captured backtrace; release stripping can limit symbol detail. SIGKILL/power loss cannot produce a shutdown event and may lose buffered records.

After a crash or shutdown, the same export command copies the previous files without restarting the daemon. Its snapshot marks the daemon disconnected; process metrics describe the export client. If logs were never created or recording was disabled, export still provides system/configuration checks and reports missing/unavailable sources. It cannot reconstruct events that were not recorded.

## Locations and bounds

- Continuous logs: `$XDG_STATE_HOME/gif-player/logs`, default `~/.local/state/gif-player/logs`. Print it with `gif-player logs path`.
- Default report root: `$XDG_STATE_HOME/gif-player/exports`.
- Current file: daemon.jsonl. Archives: daemon.1.jsonl (newest) through daemon.4.jsonl (oldest).
- Each log file is limited to 8 MiB; five files total, approximately 40 MiB. Older archives are removed as rotation proceeds. Exported reports are retained until you remove them.
- Log records are limited to 8 KiB. Oversized fields are replaced with a truncation marker and original size.
- A bounded 256-record queue holds at most approximately 2 MiB of serialized payload. Rendering/input use nonblocking enqueue, never filesystem writes. Queue overflow increments dropped_events and produces a queue_overflow record when the writer can run.
- The writer uses a 32 KiB buffer, flushes pending records on idle after one second, and sleeps without periodic wakeups when there are no pending writes. Export flushes/copies through that writer to avoid rotation races.
- Export runs in one temporary worker and does not hold the Wayland event loop while copying/fsyncing files. One report runs at a time. The CLI waits up to 30 seconds; a slow filesystem may leave the job running beyond that wait. Query its IPC status using the returned path.
- Writer failures are exposed in snapshot.logging.io_error and stderr. Recording failure does not terminate playback. A separate log-directory lock prevents competing writers from different daemon instances.

Trace mode allocates/serializes more events and generates substantially more disk traffic than info. Disable it after reproducing a problem; the ordinary performance figures must not be applied to trace mode. Log rotation and queue bounds do not promise a fixed total application RSS.

## Report contents

| File | Contents |
| --- | --- |
| manifest.json | Product/version, creation time, copied log names, warnings and scope limitations; written after the other report files |
| daemon*.jsonl | Consistent copies of engine records and available archives |
| daemon.log / daemon.log.1 | Available captured stdout/stderr from automatically launched daemon instances |
| snapshot.json | Sanitized active widget IDs, source paths, flags, positions, frame/storage status, outputs/DPI, engine cache reservations, uptime, asset dimensions/cycle duration and a 32-frame delay sample |
| system.json | Engine version, architecture, OS/kernel, XDG paths, selected session/language variables and logging state |
| configuration.json | Presence, size, object counts and JSON validity of state/profiles/settings/session documents; original unknown configuration fields are excluded |
| process-status.txt | Linux process memory, thread and state values |
| process-stat.txt | Linux process CPU/accounting counters |
| process-io.txt | Available Linux process I/O counters |
| process-limits.txt | Available process resource limits |

The engine includes known appearance/settings values and profile names in the sanitized snapshot. Preserved unknown widget/config fields are excluded. No GIF pixels/media, complete environment dump or complete arbitrary configuration documents are copied. Only explicitly selected session variables are collected. Error text, GIF paths, profile names and display details can still be private: inspect a report before sharing it. Directories use mode 0700 and files 0600. Symlink log sources are rejected.

Noctalia/compositor logs from another application's terminal are not intercepted. The engine records its actual frontend IPC requests, watch connections, palette changes and native Rust UI events. Collect the Noctalia terminal output separately for host-only plugin/load errors.

## Events and timing

Every engine record is one JSON object per line. Schema 1 supplies timestamp_unix_ms (UTC epoch), elapsed_us (monotonic daemon uptime), pid, thread, level, component, event and fields. Timestamps allow correlation with compositor/Noctalia logs; epoch adjustments do not change monotonic elapsed_us. A queue-overflow warning is generated by the writer and has its own timestamp.

| Component | Details |
| --- | --- |
| daemon | Startup version/paths/flags, logging setup, fatal errors, panic and shutdown |
| command / ipc | Selected command arguments, failures, execution duration, peer IDs, message/reply sizes, watch revisions, backpressure retirement and malformed/oversized input |
| decoder | Job path/budget, decode duration, native dimensions, frame count, storage, warnings/cancellation; trace adds frame rectangles, raw/normalized delays, disposal, transparency and palette type |
| playback | Trace frame selection, media clock, next-frame delay, speed and pause state |
| cache | Release/cancellation, lossless-backing transition, raw/compressed frame size, resident hits, decompression time and resident frame count |
| wayland / surface | Actual advertised protocols, output changes, role/layer/output, geometry/configure, input rectangles/click-through, creation/destruction and frame callbacks |
| renderer | SHM buffer allocation/size/slot, waiting for release, physical frame size, scale, damage, frame number, widget rectangles and render duration |
| input | Drag start/release/motion and trace pointer event kind/position |
| persistence / ui | Saves, restore failures, save errors and standalone panel/window opening |
| diagnostics | Export requests/results and queue-loss information |

render_us covers CPU work up to committing that surface, not compositor display latency. Frame callbacks indicate compositor scheduling readiness; compositor_time_ms has the compositor's wrapping clock domain. Neither is mouse-to-photon measurement. Source dimensions and frame_storage distinguish genuine native playback from previews/layout geometry.

To read older records first, concatenate archives from daemon.4.jsonl down to daemon.1.jsonl, then daemon.jsonl. All files are machine-readable JSONL, so ordinary jq/Rust tooling can filter by component, event, widget ID and timestamps.

## Verification

Rust tests cover bounded rotation, parseable records, private permissions, oversized-record truncation, unknown-field exclusion, live asynchronous export, continued IPC response, existing-directory rejection, level persistence and offline export. Real headless Sway acceptance asserts exported decoder frames, committed surfaces, frame callbacks, input regions, drag motions and outputs while the actual player runs. This verifies genuine engine events; it does not substitute for GPU-backed MangoWM hardware acceptance.
