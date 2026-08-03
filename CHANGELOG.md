# Gif-Overlay — Canvas-Edition

Drop-in-Ersatz: gleiche Dateinamen, CLI und IPC-Protokoll sind 100 % kompatibel
(`gif.fish`, `gif <name> move x y` usw. laufen unverändert).

## gif-script.py — Drag-Bug behoben + neue Architektur

- **Root Cause:** Drag bewegte die Surface per Margins, maß die Maus aber in
  surface-relativen Koordinaten → Feedback-Schleife mit der asynchronen
  Compositor-Bewegung (≈ halbe Distanz + diagonales Doppelbild).
- **Fix:** Bei Edit/Drag/Bounce/Hop wird die Surface auf die Arbeitsfläche
  verankert (Canvas) und das GIF per Cairo an einem Offset gezeichnet.
  Die Surface bewegt sich nie → Drag ist exakt 1:1. Im gesperrten Ruhezustand
  schrumpft die Surface wieder auf GIF-Größe (kein RAM-/Compositing-Overhead).
- Decoding im Hintergrund (Fenster sofort da), alle Frames, Memory-Budget
  (256 MiB) über einmaliges Downscaling, native Auflösung jetzt bis 512 px.
- Frames als premultiplied Cairo-Surfaces: Scale/Opacity/Flip sind reine
  Zeichen-Transformationen — kein PIL-Resize pro Frame, kein Render-Cache.
- Driftfreies Frame-Timing (monotone Deadline), nie übersprungene Frames.
- Keepalive-/Drag-Timer entfernt → deutlich weniger Wakeups; Animation läuft
  beim Draggen weiter; sanftes Kanten-/Mitte-Snapping; Skalieren um die Mitte;
  Drag-Schwelle (4 px) trennt Klick und Drag; SIGTERM/SIGINT via GLib.
- Neu: `run --monitor N`.

## gif-control.py — Slider-Lag + Layout-Sprünge behoben

- **Root Cause Lag:** Jeder Slider-Tick startete einen kompletten
  Python-Prozess (inkl. GTK/PIL-Import) blockierend im UI-Thread — plus einen
  zweiten für den Status. Jetzt: direkter Unix-Socket, asynchron, pro Karte
  eine Anfrage in flight mit Coalescing.
- Feedback-Loop entfernt (Status-Refresh löste erneute Sends aus und
  verstellte Slider, die man gerade zog).
- Werte in Labels fester Breite (draw_value aus), feste Kartenbreite,
  Filter via FlowBox-Filter, kein globales show_all() im Poll → stabiles
  Layout, kein Flackern. Mausrad über Slidern/Spinnern blockiert.
- Neu: X/Y-Position direkt einstellbar, Picker-Button, Leerzustand, Tooltips.

## gif-picker.py — öffnet sofort

- Fenster erscheint instant; Thumbnails laden im Thread-Pool mit
  Skeleton-Puls; erstes Frame mit Disk-Cache (~/.cache/gif-overlay/thumbs).
- Animation startet erst bei Hover, Frames werden erst dann dekodiert.
- Kategorien = Unterordner von Gifs/ als Filter-Chips (nur wenn vorhanden).
- Laufende Widgets sind markiert (●); Klick darauf löst einen Hop aus.
- ↑/↓ + Enter zur Auswahl; Fenstergröße passt sich dem Monitor an.

-----

# Update 2 — Bugfixes + Feinschliff

## Bugfix: Picker/Control-Panel nur ein schmaler Balken

GtkLayerShell ignoriert `set_default_size` — die Surface-Größe kommt aus der
*angeforderten* Widget-Größe. Beide Fenster fordern ihre Größe jetzt explizit
per `set_size_request` an (monitor-adaptiv wie gehabt).

## Bugfix: `gif edit` / `kill-all` finden keine Widgets

Das Script-CLI kannte nie eigene Befehle dafür — beides lebte in der
fish-Funktion mit eigener (fragiler) Widget-Erkennung, die per Picker
gestartete Daemons (eigene Session, keine fish-Jobs) nicht sieht.
Jetzt fest eingebaut, Erkennung über die PID-Dateien:

- `gif-script.py edit` — Edit-Modus für alle (unlock)
- `gif-script.py lock` — alle sperren
- `gif-script.py stop-all` (Alias: `kill-all`) — alle beenden
- `gif-script.py all <befehl> [args]` — beliebiger IPC-Broadcast
  Dazu liegt eine Referenz-`gif.fish` bei, die ausschließlich dieses CLI nutzt
  (alte Funktion vorher sichern: `cp ~/.config/fish/functions/gif.fish{,.bak}`).
  Funktional getestet (list/edit/all/kill-all inkl. Stale-PID-Cleanup).

## gif-script.py

- Bounce & Hop laufen jetzt am Frame-Clock (`add_tick_callback`) statt auf
  16-ms-Timern → vsync-synchron, kein Beat-Muster gegen die Framerate.
- State-Speichern mit `flock` + atomarem Replace → kein Verlust mehr, wenn
  mehrere Widgets gleichzeitig speichern; kein kaputtes JSON bei Absturz.
- Teilweise defekte GIFs (kaputtes Dateiende) laufen mit den bereits
  dekodierten Frames weiter, statt das Widget zu beenden.
- Touchpad-Zoom: Smooth-Scroll-Deltas werden akkumuliert (vorher 5 %/Event).

## gif-picker.py / gif-control.py

- HiDPI: Thumbnails werden mit dem Monitor-Skalierungsfaktor dekodiert und
  als Cairo-Surface mit Device-Scale angezeigt → scharf auf 2×-Displays.
- Prozesse beenden sofort: ausstehende Thumbnail-Decodes werden beim
  Schließen verworfen (vorher joinete der ThreadPool beim Exit).
- Picker: Treffer-Zähler im Header, zuverlässige Auswahl-/Enter-Logik über
  gemeinsames Filter-Prädikat, sanftes Scroll-to-Selection, Hintergrund-
  Pruning verwaister Thumbnail-Cache-Einträge.
- Control-Panel: dynamisch hinzugefügte Karten werden garantiert sichtbar
  (FlowBoxChild-Wrapper wurde nicht angezeigt).

-----

# Update 3 — Positions-Reset-Fix, Setups, Auto-Jump, Mehrfachinstanzen

## BUGFIX: Edit-Modus setzte alle Positionen zurück

Beim Wechsel in den Canvas-Modus feuerte GTK zuerst noch ein size-allocate
mit der alten, GIF-großen Allokation. Der Handler übernahm sie als
„Arbeitsfläche” → `_clamp_position()` quetschte alle Positionen auf (0,0),
der Autosave machte das dauerhaft. Fix: Allokationen werden nur noch als
Arbeitsfläche akzeptiert, wenn sie plausibel monitor-groß sind (≥ 70 %).
Zusätzlich wird der Übergangs-Frame leer gelassen statt falsch platziert.

## BUGFIX: Fenster zu breit

Picker jetzt kompakt (~8 Spalten ≈ 1416 px statt fast bildschirmbreit),
Control-Panel 1220 px — beide weiterhin monitor-adaptiv nach unten.

## BUGFIX: Rahmen bei Bounce

Die Umrandung wird nur noch im Edit-Modus gezeichnet, nicht mehr bei
Bounce/Bildschirmschoner.

## Persistenz (Position + ALLE Einstellungen)

- Einstellungen sind jetzt pro GIF (Dateistamm) gespeichert und werden bei
  jedem Neustart des GIFs / des PCs wiederhergestellt: Position, Scale,
  Opacity, Flip, Speed — und neu auch Bounce- und Auto-Jump-Zustand
  inkl. Jump-Rate.
- Bei mehreren Instanzen desselben GIFs speichert nur die erste; weitere
  Instanzen übernehmen deren Werte (leicht versetzt, damit sie sichtbar sind).

## Mehrfachinstanzen

Dasselbe GIF kann beliebig oft laufen: Picker-Klick oder `gif <name>`
startet eine weitere Instanz, IDs werden automatisch vergeben
(name, name-2, name-3, …). Control-Panel zeigt jede Instanz als eigene
Karte (Thumbnail kommt jetzt aus dem Status-Pfad, funktioniert daher auch
für Instanzen und Unterordner-GIFs).

## Setups / Profile (im Picker)

Neue „Setups”-Zeile im Picker: ＋ speichert das aktuelle Arrangement
(alle laufenden GIFs mit sämtlichen Einstellungen) unter einem Namen.
Klick auf ein Setup wendet es an (laufende Widgets werden ersetzt;
Start mit exakten Werten über `run --state`). Rechtsklick: Anwenden /
Mit aktuellem Setup überschreiben / Umbenennen / Löschen.
Gespeichert in ~/.config/gif-widget/profiles.json.

## Auto-Jump (Toggle, echter Zufall)

Der Jump-Button im Control-Panel ist jetzt ein Toggle. Die Wartezeit
zwischen Jumps ist exponentialverteilt (Poisson-Prozess) — also mal zwei
Jumps direkt hintereinander, dann längere Pausen, kein starres Intervall.
Die mittlere Rate ist einstellbar: Slider „Jump Øs” im Panel,
`gif <name> jump-rate 5` im Terminal, Raten-Presets im Kontextmenü.
IPC/CLI: `jump` (Toggle), `jump-rate <s>`; Einzelhop bleibt als `hop`.

-----

# Update 4 — Finale Architektur (Supervisor, Protokoll v2)

**Breaking:** Alle vier Dateien zusammen austauschen. Der neue Daemon räumt
alte Einzel-Daemons (≤ v3) beim ersten Start automatisch ab.

- **Ein Supervisor-Daemon statt ein Prozess pro GIF:** ~60–80 MB RAM pro
  weiterem GIF gespart; 5× dasselbe GIF wird genau **einmal** dekodiert
  (geteilte Frame-Sätze). Startet automatisch, beendet sich, wenn leer;
  Single-Instance via flock; `daemon.log` zur Diagnose.
- **IPC neu:** Gio.SocketService auf dem Main-Loop (Server-Thread und
  idle/Event-Handshake entfernt). Ein Socket, ein Protokoll (v2) für CLI,
  Picker und Panel; `list` liefert alle Status in einem Round-Trip.
- **Setups werden atomar im Daemon angewendet** (`apply-setup`) — keine
  Stop/Warte/Spawn-Races mehr; Picker-Klick spawnt über den Daemon.
- **Control-Panel:** ein Sammel-Poll statt N Status-Anfragen.
- **CLI vollständig** (`run/ipc/all/list/edit/lock/stop-all/picker/control`),
  fish ist nur noch optionaler Komfort (gif.fish unverändert nutzbar).
- Funktional getestet: Manager (Auto-IDs, Wildcard, apply-setup,
  Fehlerfälle, Leerlauf-Exit), komplettes CLI und Picker/Panel-Clients
  gegen einen Protokoll-v2-Testserver. Architektur-Begründung und
  manuelle Smoke-Checkliste: ARCHITECTURE.md.
