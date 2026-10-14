# Noctalia GIF Player

Native Noctalia 5 plugin, API 30 or newer. All visible controls use the host's
`ui.*`, `panel.*`, bar widget and shortcut APIs; Noctalia owns fonts, palettes,
roundings, transitions, focus and panel placement. Playback runs in the Rust
`gif-player` daemon, independently of Noctalia.

Install the engine, then run `gif-player install-plugin`. Enable
`madebycli/gif-player` under Settings → Plugins, and add
`madebycli/gif-player:player` through the bar widget manager. The control-center
shortcut is `madebycli/gif-player:launcher`.

Left click opens controls; right click opens the floating picker. Both panels
have Collection, Active, Profiles and Settings pages and consume one shared
daemon snapshot. Open the floating picker without a bar widget:

```sh
noctalia msg panel-toggle madebycli/gif-player:picker
gif-player picker
```

The service uses one Rust `watch --reconnect` client because the plugin sandbox
provides process streams but no Unix-socket binding. Changes arrive as events;
the service does not run an IPC subprocess on every update tick. Commands use
argv-based `runAsync`, preserving spaces, quotes and Unicode in paths. Slider
commands are coalesced. The bar alone does not start an empty playback daemon.

The plugin is installed locally. No license is granted by its manifest;
`LicenseRef-All-Rights-Reserved` avoids Noctalia's implicit MIT default.
Community catalog publication requires the rights holder's license decision.
See the preserved NOTICE.md and ../../docs/acceptance.md.
