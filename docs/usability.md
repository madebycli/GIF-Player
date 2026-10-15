# Using the controls

The standalone Rust interface (`gif-player app`, `picker`, `control`, `settings --standalone`) is drawn with Noctalia 5.2.1's own design tokens and colour roles, so it looks like a native Noctalia surface — also when Noctalia is not running. The native panels of the Noctalia plugin use the host's real `ui.*` components; both frontends talk to the same daemon.

## Choose GIFs

Open `gif-player picker`, or Collection in `gif-player app`. Click an image to start it; click again to stop its instances. The gallery scrolls continuously and has no per-image Add/Stop buttons. Active images have a theme-colored outline and a small badge (a number when the same GIF runs more than once). Hovering — or focusing with the keyboard — shows "Click to play/stop" on the image. Hovering animates the image; thumbnails decode only their first frame.

Search filters while you type. Every word must occur in the file's path *below the collection folder* — folder names count, parent directories outside the collection never do. Case and common accents are ignored ("cafe" finds "Café", "uber" finds "Über"). Enter starts or stops the first match; the × inside the field and Refresh reset or reload the list. The list reloads on its own when the window regains focus, so GIFs added meanwhile appear. Names sort case-insensitively.

An empty collection links to Settings. Enter the folder containing your GIFs and choose Save; subfolders are included and an unreadable subfolder no longer hides the others. Changing the folder updates the native Noctalia frontend and the Rust app through the shared daemon. Existing running GIFs keep their source paths.

## Control playback

Choose an active GIF (a list with thumbnails; narrow windows use a collapsible list), then use the main actions:

- Pause/Play changes playback without removing the GIF.
- Edit lets you drag the GIF and scroll over it to resize. Done editing locks it again and restores click-through.
- Hide/Show changes visibility while preserving the instance.
- Stop removes that instance; its saved settings remain available.
- Duplicate and Reset sit beside the GIF's name.

Size and opacity use percentages and speed a multiplier; sliders snap to 1 % (speed 0.05×). Position & monitor contains monitor choice, X/Y fields and corner/center buttons. Motion effects contains bounce, hop and automatic jumping; the jump interval appears when automatic jumping is enabled. Advanced contains mirroring, layer and canvas choices, the source path and any load error. With several instances, All active GIFs expands the global actions.

## Save layouts

Use Layouts in the app, or the expandable Layouts section of native Settings. Enter a name and choose Save; an existing name is shown as Overwrite. Each layout shows how many GIFs it holds. Apply restores it; the icon buttons overwrite, rename (inline, prefilled) or delete (confirmed with a second action and Cancel).

## Settings and diagnostics

The collection folder (with the number of GIFs found) and the restore switch are visible first. Diagnostics and Application expand separately. Logging levels are named choices. After exporting a report, Open report folder asks the system file manager to open it through optional `xdg-open`. The engine and playback do not depend on a file manager.

Errors and confirmations appear in a banner under the page title; the × dismisses an error, a confirmation disappears with your next interaction.

## Mouse

Buttons act when the mouse button is released over the same control, so a press can be cancelled by moving away. Sliders follow the pointer while dragged. The first click in a text field selects its whole value (typing replaces it); a further click places the caret and dragging selects. The overlay scrollbar grows when hovered and can be dragged; the wheel scrolls the list under the pointer first (instance list), otherwise the page.

## Keyboard

Native Noctalia controls use the host's focus and editing behavior. In the independent Rust UI:

| Key | Action |
| --- | --- |
| Tab / Shift+Tab | Move between visible controls; at the end of a long list the page scrolls on and focus continues |
| Arrow keys | Move between tiles, list rows and buttons; Left/Right change a focused slider |
| Enter / Space | Activate the focused control (Space types in a text field); Enter in a field saves a directory/name/coordinate or toggles the first search result |
| Ctrl+A | Select the whole field |
| Left / Right / Home / End | Move the caret; with Shift extend the selection |
| Backspace / Delete | Delete a character or the selection; with Ctrl a whole word; holding the key repeats |
| Ctrl+C / Ctrl+X / Ctrl+V | Copy, cut and paste through the Wayland clipboard (text only) |
| Page Up / Page Down | Scroll one page |
| Ctrl+F | Open Collection and focus search |
| Ctrl+W | Close the app/panel |
| Escape | Leave the field, then drop focus; close a floating panel |

Focused controls show Noctalia's 2 px focus ring and scroll into view. Fields show a static caret and selection without a blinking redraw timer; an idle open window issues no redraws. Labels use German for a German session locale (LC_ALL, LC_MESSAGES, LANG) and English otherwise. Native panels use Noctalia's language. Colors and font follow the real Noctalia appearance — all fourteen roles including `hover`/`on_hover`, `secondary` (focus ring) and `error` — and are retained for use without the host.

## Verification

See acceptance.md for what was executed for this version: Rust tests for editing, selection, paste, search, release-activation, slider snapping, scrollbar dragging, focus order/scroll-into-view, arrow navigation, rounded panels and the CLI; real Sway assertions for typed search plus Enter, key repeat and clipboard paste/copy; and an unmodified Noctalia 5.2.1 session in which native tiles started GIFs by real clicks while the Rust app received the host's palette, including a live scheme change.

These checks do not claim GPU-backed MangoWM acceptance. See acceptance.md for compositor/hardware limits.
