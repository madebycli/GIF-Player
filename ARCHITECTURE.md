# GIF Player – Architektur (GTK3, Supervisor, IPC v2)

## Komponenten

```text
gif-player / gif-picker / gif-control
                  │
                  ├── XDG- und Bootstrap-Schicht
                  │   (gif_player_paths.py, gif_player_bootstrap.py)
                  │
                  ├── testbare Player-Logik
                  │   (gif_player_runtime.py)
                  │
                  └── gif-script.py daemon
                        ├── Gio.SocketService, JSON-Protokoll v2
                        ├── WidgetManager
                        ├── WidgetWindow × N (GTK3 + GtkLayerShell)
                        └── FrameStoreRegistry (Pillow → Cairo)
```

Die Rendering- und Animationsimplementierung bleibt GTK3-basiert. Ein einziger
Daemon verwaltet alle Layer-Shell-Fenster. Mehrere Instanzen desselben GIFs
teilen weiterhin einen dekodierten Frame-Satz.

`gif_player_runtime.py` enthält reine, displayfreie Logik für GIF-Dauern,
Disposal-sichere RGBA-Kopien, Cairo-Premultiplication, absolute Frame-Deadlines,
Jump-Kurven, freie Positionen und Bounce-Reflexion. Die Bootstrap-Schicht
installiert gezielte Laufzeitkorrekturen auf der geladenen GTK-Implementierung.
Die Supervisor- und IPC-Architektur wird dadurch nicht ersetzt.

## Installierte Programme

- `gif-player`: shellunabhängiges Haupt-CLI und Daemon-Bootstrap.
- `gif-picker`: grafischer Picker.
- `gif-control`: Live-Control-Panel.
- `gif`: optionale Fish-Funktion, die nur `gif-player` aufruft.

Die Python-Dateien liegen im Nix-Paket unter `libexec/gif-player`. Interne
Starts verwenden bevorzugt die verpackten Programme unter `$out/bin`, damit
die GTK-/GI-Wrapper-Umgebung erhalten bleibt. Das aktuelle Arbeitsverzeichnis
und globale Python-Installationen sind irrelevant.

## Player-Pipeline

```text
GIF path
  → FrameStoreRegistry.acquire(realpath)
  → Pillow decode thread
  → seek + composited RGBA copy
  → optional one-time resize
  → premultiplied BGRA bytearray
  → Cairo ImageSurface
  → shared FrameStore
  → monotonic absolute frame deadline
  → one queued GTK draw for newest due frame
  → Cairo clear + transform + paint in one callback
  → GtkLayerShell surface commit
```

Der Buffer jeder Cairo-Surface bleibt so lange wie die Surface in der Registry
gespeichert. Append und Read von Surface und Dauer verwenden denselben Lock.
Das erste Frame darf bereits laufen, während spätere Frames dekodiert werden.
Erreicht die Wiedergabe das Decoder-Ende, wird das letzte gültige Frame gehalten.

## Timing

GIF-Frames verwenden absolute monotone Deadlines. Unter normaler Last wird genau
ein Frame pro Deadline weitergeschaltet. Bei einer blockierten Main Loop werden
überfällige Dauern auf der Zeitachse berücksichtigt, aber nur das neueste fällige
Frame gezeichnet. Der Catch-up ist begrenzt; extreme Verzögerungen werden einmal
neu basiert, statt eine Folge von 0-ms-Callbacks zu erzeugen.

Jump und Bounce verwenden die GTK-Frame-Clock. Jump-Fortschritt wird aus der
absoluten Frame-Clock-Zeit berechnet. Bounce bleibt `dt`-basiert und reflektiert
mit einer robusten modulo-basierten Dreiecksfunktion.

## Koordinaten und Surface-Modi

Positionen sind monitorlokale logische GTK-Koordinaten.

```text
base_x/base_y       gespeicherte freie Position
bounce_x/bounce_y   temporäre sichtbare Bounce-Position
jump_offset_y       temporärer Jump-Offset
draw_x              aktive Basis-X
draw_y              aktive Basis-Y - jump_offset_y
```

Manuelle Positionen werden nicht an Monitorgrenzen geklemmt. Negative Werte und
vollständig außerhalb liegende Positionen sind erlaubt.

Surface-Zustände:

- `compact`: GIF-große Surface; Layer-Shell-Margins tragen die Position.
- `to-canvas`: Surface wächst vom bisherigen top-left Punkt; das GIF bleibt bei
  `(0, 0)` und damit auf exakt denselben globalen Pixeln.
- `canvas`: monitorfüllende Surface; die GIF-Basisposition ist der Draw-Offset.
- `to-compact`: umgekehrter positionsgleicher Übergang.

Ein gesperrtes GIF darf nur dann kompakt werden, wenn sein komplettes skaliertes
Rechteck im Monitor liegt. Teilweise oder vollständig außerhalb liegende
Widgets bleiben als durchklickbare Canvas-Surface erhalten, weil negative oder
außerhalb liegende Layer-Shell-Margins compositorübergreifend nicht zuverlässig
sind.

Dragging verändert weiterhin nur den Draw-Offset innerhalb der stabilen Canvas-
Surface. Die Wayland-Surface wird während des Pointer-Drags nicht verschoben.

Bounce besitzt eigene Bounds. Bei einem auf einer Achse zu großen GIF wird diese
Achse zentriert und ihre Geschwindigkeit auf null gesetzt; die andere Achse kann
weiter bouncen.

## XDG-Pfade

| Inhalt | Pfad |
|---|---|
| Socket, Lock, Log | `$XDG_RUNTIME_DIR/gif-player`, Fallback `/tmp/gif-player-$UID` |
| Zustand und Profile | `$XDG_CONFIG_HOME/gif-player` |
| Thumbnail-Cache | `$XDG_CACHE_HOME/gif-player/thumbs` |
| GIFs | `$XDG_DATA_HOME/gif-player/gifs` |

Das Runtime-Verzeichnis wird auf `0700`, der Socket auf höchstens `0600`
gesetzt. Der alte Ordner `~/Scripts/Gif-Overlay/Gifs` wird nur erkannt, wenn er
bereits existiert und der XDG-GIF-Ordner noch nicht vorhanden ist; er wird nie
angelegt.

## Protokoll v2

Eine Anfrage und eine Antwort bestehen jeweils aus genau einer JSON-Zeile.

```text
Daemon: ping, list, spawn, stop-all, apply-setup, quit-daemon
Widget: status, lock, unlock, toggle, pause, play, move, move-by, scale,
        corner, opacity, flip, speed, bounce, stop-bounce, hop, jump,
        jump-rate, reset, quit
```

Statusantworten dürfen zusätzliche Felder wie `base_position` und
`surface_mode` enthalten. Vorhandene Felder und Aktionen bleiben kompatibel.

## Diagnose

```console
GIF_PLAYER_DEBUG_TIMING=1 gif-player daemon
```

Aktiviert monotone JSON-Ereignisse für Surface-Übergänge, `size-allocate`,
Jump-Fortschritt, Draws, Damage, Frame-Catch-up und Bounce. Ohne die Variable ist
die detaillierte Instrumentierung deaktiviert.

## Display-Anforderung

Overlay, Picker und Control benötigen eine Wayland-Sitzung und einen Compositor
mit Layer-Shell-Unterstützung, beispielsweise Niri, Sway, Hyprland oder Wayfire.
CLI-Hilfe, Pfadtests, Syntaxprüfungen, GIF-Pixeltests und IPC-Unit-Tests benötigen
kein Display.
