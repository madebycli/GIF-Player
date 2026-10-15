# Using the redesigned controls

## Choose GIFs

Open `gif-player picker`, or Collection in `gif-player app`. Click an image to start it; click again to stop its instances. The gallery scrolls continuously and has no per-image Add/Stop buttons. Active images have a theme-colored outline. Search filters the collection; Clear removes the query. Native panels also offer folder filtering when there are subfolders.

An empty collection links to Settings. Enter the folder containing your GIFs and choose Save; subfolders are included. Changing the folder updates the native Noctalia frontend and the Rust app through the shared daemon. Existing running GIFs keep their source paths.

## Control playback

Choose an active GIF, then use the main actions:

- Play/Pause changes playback without removing the GIF.
- Edit lets you drag the GIF and scroll over it to resize. Done editing locks it again and restores click-through.
- Show/Hide changes visibility while preserving the instance.
- Stop removes that instance; its saved settings remain available.

Size and opacity use percentages; speed uses a multiplier. Position & monitor contains coordinates, monitor selection and corner/center actions. Motion effects contains bounce, hop and automatic jumping; the jump interval appears when automatic jumping is enabled. Advanced contains mirroring, layer/canvas selection, reset and duplication. Source paths are kept out of the main native control view.

With multiple instances, choose the instance by ID. All active GIFs expands the global actions. The narrow Rust selector limits its visible list and offers previous/next controls for additional instances; the GIF collection itself always scrolls continuously.

## Save layouts

Use Layouts in the app, or the expandable Layouts section of native Settings. Enter a name and choose Save. An existing name is shown as Overwrite. Apply restores that layout's GIFs and settings. Rename opens a prefilled editor; Save or Enter commits it. Delete requires an explicit second action and offers Cancel.

## Settings and diagnostics

The collection folder and compact restore switch are visible first. Diagnostic and application tools expand separately. Logging levels have named choices instead of a cycling button. After exporting a report, Open report folder asks the system file manager to open it through optional `xdg-open`. The engine and playback do not depend on a file manager.

The picker, playback controls and settings remain independently callable. `gif-player app` combines them with layouts in a normal compositor-managed xdg-shell window; it does not request a floating role.

## Keyboard

Native Noctalia controls use the host's focus and editing behavior. In the independent Rust UI:

| Key | Action |
| --- | --- |
| Tab / Shift+Tab | Move between visible controls |
| Enter / Space | Activate a focused button; Space still types in a text field |
| Enter in a field | Save a directory/name/coordinate, or toggle the first search result |
| Ctrl+A | Select the whole input for replacement |
| Ctrl+F | Open Collection and focus search |
| Left / Right on a slider | Change its value |
| Ctrl+W | Close the app/menu |
| Escape | Leave the app's input; close a floating menu |

Text fields show focus/selection and a static caret without a blinking redraw timer. Labels use German for a German session locale (LC_ALL, LC_MESSAGES, LANG) and English otherwise. Native panels use Noctalia's language. Colors/font continue to follow the real Noctalia appearance; the standalone app retains the last appearance when the host is absent.

## Verification

Real Noctalia 5.2.1 interactions cover image-click playback, collection-folder Save and layout Save. A real Rust xdg window accepts text replacement and Tab/Enter saving; small-window rendering was inspected at a 720×480 output. Tests cover field replacement, narrow scrolled layout controls and a bounded selector with 100 real daemon instances. The headless Sway runner verifies xdg keyboard focus/close in addition to overlay, click-through, drag, native-resolution and export checks.

These checks do not claim GPU-backed MangoWM acceptance. See acceptance.md for compositor/hardware limits.
