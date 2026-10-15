# Changelog

## 1.4.0 — Native Noctalia look, steadier input, fewer logic gaps

- Rebuild the standalone Rust interface on Noctalia 5.2.1's design system: 38/32 px controls, 3/6/9/12 px radii, 1 px outlines, a 2 px `secondary` focus ring, `hover`/`on_hover` feedback, card/panel/tab/toggle/slider/scrollbar chrome, bold `primary` page titles and original vector icons. The Noctalia service now forwards all fourteen colour roles (was eight), so hover labels, destructive actions and focus rings follow the real host live. Floating panels are rounded Noctalia panels; the app window keeps its normal compositor-managed frame.
- Page layouts: sidebar tabs with a live instance badge, instance list with thumbnails instead of the previous/next pager, grouped cards, collapsible detail sections and aligned slider/value rows. The collection keeps its minimal, continuous image toggle: tiles gain rounded previews, a hover/keyboard "Click to play/stop" hint and an active badge, but no per-image buttons and no pagination.
- Pointer: buttons activate on release over the same control, sliders follow Noctalia's thumb geometry and snap to 1 %, the overlay scrollbar widens on hover and can be dragged, wheel scrolling targets the list under the pointer.
- Keyboard and text: caret placement by click, selection by drag/Shift, Left/Right/Home/End/Delete, Ctrl+Backspace/Delete word removal, Ctrl+C/X/V through the Wayland clipboard, client-side key repeat, Up/Down/Left/Right navigation between tiles and rows, Page Up/Down, Tab at the edge of a long list continues into the next rows, and focused controls scroll into view. Focus is tracked by control identity rather than draw order.
- Search matches every typed word against the relative path (folders included) without caring about case or common accents; the temporary parent path never matches. Collections sort case-insensitively, an unreadable subfolder no longer hides the rest, and the list refreshes when the window regains focus.
- The first click in a text field selects its value so typing replaces it; a second click places the caret.
- Fix: `gif-player COMMAND --help` ran the command (starting a daemon, opening panels or editing Noctalia's settings); misspelled `daemon`/`install-plugin` options are rejected instead of ignored; the "no widgets" message follows the session language.
- Performance: scaled thumbnails are cached per file/frame/size and clip masks are only used where a shape crosses its region. Measured on the same collection page (60 scroll redraws, pointer parked on empty space, software-rendered Sway sandbox), the median redraw fell from 21.6 ms to 15.1 ms (p90 30.2 ms to 22.6 ms); an idle open window issues no redraws.
- Tests: 38 Rust tests (was 19) and 20 real Sway assertions (was 16), including typed search plus Enter, client-side key repeat, clipboard paste/copy against `wl-clipboard` and an idle-daemon check after the window closes. Official Noctalia plugin lint reports 0 errors and 0 warnings.

## 1.3.0 — Clearer, more intuitive controls

- Put playback, edit/done, visibility and stop first; move position, motion and advanced tools into expandable sections.
- Keep the picker minimal and continuous, add image-action hints, clear search and useful empty states.
- Add explicit directory/layout Save, inline Rename and Delete confirmation, named logging-level choices and report-folder opening.
- Rework the normal Rust app with responsive image columns, compact restore switches, usable narrow layouts and shared English/German labels.
- Fix keyboard focus for normal xdg windows; support Tab/Shift+Tab, Enter/Space, Ctrl+A, Ctrl+F, Ctrl+W and keyboard slider changes.
- Synchronize collection-directory changes across frontends. Preserve playback, legacy commands, configuration and theme tracking.

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
