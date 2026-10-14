# CLI and Unix socket contract

Protocol 2 uses UTF-8 newline-delimited JSON on `$XDG_RUNTIME_DIR/gif-player/daemon.sock`. A normal connection sends one object and receives one line. `gif-player request '{"action":"snapshot"}'` is an equivalent CLI transport. No shell is involved in plugin command arguments.

```json
{"action":"spawn","gif":"/home/me/GIFs/bird.gif","id":"bird","state":{"x":100,"y":200,"locked":true}}
{"action":"move","id":"bird","x":-100,"y":500}
{"action":"pause","id":"*"}
{"action":"profile-save","name":"desk"}
```

`spawn` accepts a path or collection name, optional ID, legacy monitor index, output name and state object. IDs are unique. Missing IDs use the filename stem and numbered suffixes. Settings include x, y, scale, opacity, speed, locked, visible, paused, flip_h, flip_v, bouncing, jumping, jump_rate, output, layer and canvas. Unknown persisted fields are retained. Per-widget results retain the legacy status keys, including actual position, base_position, size, screen, source_size, frame_storage and frames; `frames` is `[decoded_count,total_count]`.

| Action | Additional fields |
|---|---|
| ping, list, snapshot, library, profiles, settings, stop-all, quit-daemon | none |
| move / move-by | id, x/y or dx/dy |
| scale, opacity, speed | id and field named as action |
| corner | id, position: tl/tr/bl/br/center |
| flip | id, mode: none/h/v/hv/toggle-h/toggle-v |
| jump-rate | id, seconds |
| layer / output | id, layer or output |
| canvas | id, optional enabled boolean (otherwise toggles) |
| status, lock, unlock, toggle, pause, play, resume, bounce, stop-bounce, hop, jump, reset, center, quit, stop, show, hide, visibility | id |
| profile-save, profile-apply, profile-delete | name |
| profile-rename | name, new_name |
| library-directory | path |
| settings | optional settings object (including restore boolean) |
| picker, control, app, settings-ui, ui-close | none |
| appearance | palette object with hex color roles, font_family |

`id:"*"` selects all active widgets for legacy global actions. Use CLI `all ACTION`, `edit`, `lock`, `stop-all` or `kill-all` as shortcuts. Profile save replaces an existing profile explicitly; rename refuses collisions. Profile apply validates paths and settings before replacing active widgets.

`{"action":"watch"}` keeps the socket open for full snapshots. Snapshots include connected state, revision, widgets, outputs, settings, profiles, engine errors and decoder warnings. Updates coalesce under backpressure. `gif-player watch --reconnect` supplies a newline stream and reports disconnection while reconnecting without launching a daemon.

Managed frontends register with `{"action":"watch","frontend":true}` (CLI `watch --frontend`). An edit-context request arrives as `frontend_request` with kind, widget ID and nonce. The frontend opens its own native control panel and acknowledges via `{"action":"frontend-ack","nonce":N}`. With no managed frontend, the daemon opens the standalone Rust control surface. This contract does not name a particular shell.

Request size is bounded at 1 MiB. Malformed or unsupported commands return `ok:false` and an error. The CLI returns a nonzero exit status for failures. Socket access is restricted to the current user. Partial requests and blocked readers expire without blocking other clients.

## Diagnostics

`logs-level` takes `level` (`off`, `error`, `warn`, `info`, `debug`, `trace`), validates it and persists it. Snapshots expose a `logging` object with directory, effective level, queue-loss count, writer error, rotation limits and latest export status.

`logs-export` accepts an optional `directory`. The directory must be new. It returns `{ok:true,pending:true,directory:...}` immediately and completes in a temporary worker. Query `logs-export-status` with that returned directory until `pending` is absent/false. Success returns `{ok:true,directory:...,warnings:[...]}`; failure includes `ok:false,error`. Unknown jobs return an error; only eight recent job results are retained. One export runs at a time. Completion wakes the Wayland loop and updates the streamed snapshot. Rendering and IPC do not wait for report disk writes.

The CLI `logs export [NEW_DIRECTORY]` resolves relative paths in the caller's directory and waits up to 30 seconds for completion. A stopped daemon is handled through offline export rather than autostart. Live IPC errors are reported rather than silently substituted with an offline report.
