# Noctalia GIF Player

Native Noctalia 5 plugin, API 30 or newer. All visible controls use the host's
`ui.*`, `panel.*`, bar widget and shortcut APIs; Noctalia owns fonts, palettes,
roundings, transitions, focus and panel placement. Playback runs in the Rust
`gif-player` daemon, independently of Noctalia.

Install the engine, then run `gif-player install-plugin`. The installer enables the local plugin and attaches its widget to an enabled bar. Use `--bar flying` for an explicit bar or `--no-bar` for file-only installation. Adjust placement through the native bar widget manager. The control-center
shortcut is `madebycli/gif-player:launcher`.

Left click opens controls; right click opens the floating picker. Picker, playback and settings are separate native panels sharing daemon state. The picker scrolls continuously and image clicks toggle playback; the settings switch has its natural compact width. `gif-player app` opens a normal Rust xdg-shell window with all pages. The service synchronizes actual host color roles/font with that window. Open the floating picker without a bar widget:

```sh
noctalia msg panel-toggle madebycli/gif-player:picker
gif-player picker
```

The service uses one Rust `watch --reconnect --frontend` client because the plugin sandbox
provides process streams but no Unix-socket binding. Changes arrive as events;
the service does not run an IPC subprocess on every update tick. Commands use
argv-based `runAsync`, preserving spaces, quotes and Unicode in paths. Slider
commands are coalesced. The bar alone does not start an empty playback daemon.

Settings provide a logging-level button (info/debug/trace/off) and **Export logs**.
Exports run asynchronously in the Rust engine; a native notification shows the
completed folder or an error. The same operation is available through
`gif-player logs export [NEW_DIRECTORY]`, including when the daemon has stopped.
Trace records individual playback/render/input events; switch back to info after
reproducing a problem. See ../../docs/diagnostics.md for contents, privacy and bounds.

The plugin is installed locally. No license is granted by its manifest;
`LicenseRef-All-Rights-Reserved` avoids Noctalia's implicit MIT default.
Community catalog publication requires the rights holder's license decision.
See the preserved NOTICE.md and ../../docs/acceptance.md.
