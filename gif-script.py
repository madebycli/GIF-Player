#!/usr/bin/env python3
"""
gif-script.py - GIF-Overlay-System (Supervisor-Architektur, Protokoll v2)

ARCHITEKTUR-ENTSCHEIDUNGEN (Lead-Review)
=========================================
* EIN Daemon-Prozess verwaltet ALLE Widgets als Fenster (statt ein Prozess
  pro GIF). Gewinn: ~60-80 MB RAM pro weiterem GIF gespart, ein einziger
  Socket statt PID-Datei-Discovery, atomares Setup-Anwenden ohne
  Prozess-Races, und mehrere Instanzen desselben GIFs teilen sich EINEN
  dekodierten Frame-Satz (FrameStoreRegistry).
* IPC laeuft ueber Gio.SocketService direkt auf dem GLib-Main-Loop:
  kein Server-Thread, kein idle_add/Event-Handshake, Dispatch ist ein
  normaler Funktionsaufruf im Main-Thread.
* Der Daemon startet bei Bedarf automatisch (jeder `run`/Picker-Klick)
  und beendet sich selbst, wenn das letzte Widget geschlossen wurde.
* GTK3 + Cairo + PIL-Decode-Thread bleiben bewusst: Das Canvas/Kompakt-
  Modell, Input-Regionen und der Drag sind auf Niri/GTK3 verifiziert;
  GTK4 braechte fuer ein kleines Overlay keinen messbaren Gewinn bei
  hohem, hier nicht testbarem Migrationsrisiko. Details: ARCHITECTURE.md.

SURFACE-MODELL (unveraendert, verifiziert)
==========================================
* KOMPAKT (gesperrt + ruhend): Surface = GIF-Groesse, Position via Margins,
  Input-Region leer (durchklickbar).
* CANVAS (Edit/Drag/Bounce/Hop): Surface fuellt die Arbeitsflaeche, das GIF
  wird per Cairo an einem internen Offset gezeichnet. Die Surface bewegt
  sich nie -> Drag ist exakt 1:1 und latenzimmun.

PROTOKOLL v2 (ein Socket: /tmp/gif-widget-<user>/daemon.sock)
=============================================================
Anfrage  : {"action": <str>, "id": <widget-id|"*">, ...}  (eine JSON-Zeile)
Antwort  : {"ok": true, ...} | {"error": "..."}
Daemon   : ping, list, spawn{gif,id?,state?,monitor?}, stop-all,
           apply-setup{widgets:[...]}, quit-daemon
Widget   : status, lock, unlock, toggle, pause, play, move, move-by,
           scale, corner, opacity, flip, speed, bounce, stop-bounce,
           hop, jump, jump-rate, reset, quit          (id noetig, "*" = alle)
WICHTIG  : Picker, Control-Panel und gif.fish muessen zusammen mit dieser
           Datei aktualisiert werden (altes Pro-Widget-Socket-Protokoll
           entfaellt). Alte Einzel-Daemons vorher beenden.
"""

import argparse
import cairo
import fcntl
import getpass
import gi
import json
import math
import os
import random
import signal
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path

gi.require_version("Gtk", "3.0")
gi.require_version("GtkLayerShell", "0.1")
gi.require_version("Gdk", "3.0")

from gi.repository import Gdk, Gio, GLib, Gtk, GtkLayerShell
from PIL import Image, ImageChops, ImageSequence

USER = getpass.getuser()
RUNTIME_DIR = Path(f"/tmp/gif-widget-{USER}")
DAEMON_SOCK = RUNTIME_DIR / "daemon.sock"
DAEMON_LOCK = RUNTIME_DIR / "daemon.lock"
CONFIG_DIR = Path.home() / ".config" / "gif-widget"
STATE_FILE = CONFIG_DIR / "state.json"

MAX_SOURCE_DIM = 512          # native Decodier-Aufloesung (laengste Kante)
MEMORY_BUDGET_MB = 256        # Obergrenze fuer dekodierte Frames (RGBA)
DEFAULT_X = 100.0
DEFAULT_Y = 100.0
DEFAULT_SCALE = 0.7
DEFAULT_OPACITY = 1.0
DEFAULT_SPEED = 1.0

DRAG_THRESHOLD = 4            # px Bewegung, ab der ein Klick zum Drag wird
EDGE_SNAP = 14                # px sanftes Einrasten an Kanten/Mitte beim Drag
DOUBLE_CLICK_MS = 400
BOUNCE_SPEED = 360.0          # px/s
HOP_HEIGHT = 60.0
HOP_DURATION = 0.65           # Sekunden
JUMP_RATE_DEFAULT = 6.0       # mittlerer Abstand zwischen Auto-Jumps (s)
JUMP_MIN_DELAY = 0.7
JUMP_MAX_FACTOR = 6.0
GIF_PAD = 4                   # Zeichen-/Input-Polster um das GIF
EMPTY_EXIT_SECONDS = 2        # Daemon beendet sich, wenn so lange leer
DAEMON_BOOT_TIMEOUT = 6.0

PROFILE_KEYS = ("x", "y", "scale", "opacity", "flip_h", "flip_v",
                "speed", "bouncing", "jumping", "jump_rate")


LOG_FILE = RUNTIME_DIR / "daemon.log"
_LOG_MAX = 512 * 1024


def log(msg):
    line = f"[gif-daemon] {msg}"
    print(line, file=sys.stderr)
    # Der auto-gestartete Daemon laeuft mit stderr=DEVNULL -> Logdatei
    # ist die einzige Diagnose-Quelle. Best-effort, mit Groessen-Deckel.
    try:
        RUNTIME_DIR.mkdir(parents=True, exist_ok=True)
        if LOG_FILE.exists() and LOG_FILE.stat().st_size > _LOG_MAX:
            LOG_FILE.write_text("")
        with open(LOG_FILE, "a") as f:
            f.write(time.strftime("%H:%M:%S ") + line + "\n")
    except Exception:
        pass


def ensure_dirs():
    RUNTIME_DIR.mkdir(parents=True, exist_ok=True)
    CONFIG_DIR.mkdir(parents=True, exist_ok=True)


# ---------------------------------------------------------------------------
# Persistenter Zustand (pro GIF-Dateistamm)
# ---------------------------------------------------------------------------

class StateStore:
    """state.json mit flock + atomarem Replace.

    Der Daemon ist normalerweise der einzige Schreiber; das Lock schuetzt
    trotzdem gegen parallel laufende Alt-Versionen oder manuelle Edits.
    """

    def __init__(self, path: Path):
        self.path = path

    def load_all(self) -> dict:
        try:
            if self.path.exists():
                data = json.loads(self.path.read_text())
                if isinstance(data, dict):
                    return data
        except Exception as e:
            log(f"State load failed: {e}")
        return {}

    def get(self, key: str) -> dict:
        entry = self.load_all().get(key, {})
        return entry if isinstance(entry, dict) else {}

    def put(self, key: str, data: dict):
        try:
            ensure_dirs()
            lock_path = self.path.with_name(".state.lock")
            with open(lock_path, "w") as lf:
                fcntl.flock(lf, fcntl.LOCK_EX)
                state = self.load_all()
                state[key] = data
                tmp = self.path.with_name(self.path.name + ".tmp")
                tmp.write_text(json.dumps(state, indent=2))
                os.replace(tmp, self.path)
        except Exception as e:
            log(f"State save failed: {e}")


STATE = StateStore(STATE_FILE)


# ---------------------------------------------------------------------------
# Frame-Decoding: Hintergrund-Thread + geteilte Frame-Saetze
# ---------------------------------------------------------------------------

def _pil_to_cairo(rgba: Image.Image):
    """RGBA-PIL-Frame -> (cairo.ImageSurface, buffer).

    Cairo ARGB32 erwartet auf Little-Endian-Systemen (x86/ARM) die
    Byte-Reihenfolge B,G,R,A mit vorgemultipliziertem Alpha.
    """
    try:
        pre = rgba.convert("RGBa")           # premultiplied
        r, g, b, a = pre.split()
        data = Image.merge("RGBa", (b, g, r, a)).tobytes()
    except Exception:
        rgb = rgba.convert("RGB")
        a = rgba.getchannel("A")
        pre = ImageChops.multiply(rgb, Image.merge("RGB", (a, a, a)))
        r, g, b = pre.split()
        data = Image.merge("RGBA", (b, g, r, a)).tobytes()

    buf = bytearray(data)
    surf = cairo.ImageSurface.create_for_data(
        buf, cairo.FORMAT_ARGB32, rgba.width, rgba.height, rgba.width * 4
    )
    return surf, buf


class FrameStore:
    """Dekodiert ein GIF im Hintergrund in Cairo-Surfaces.

    * Alle Frames werden behalten (keine Reduzierung, kein Frame-Verlust).
    * Memory-Budget: Decodier-Aufloesung wird einmalig so gewaehlt, dass
      frames * w * h * 4 <= Budget bleibt.
    * Mehrere Widgets koennen denselben Store nutzen (Registry); alle
      Listener werden im Main-Thread benachrichtigt, sobald Frame 0 da ist.
    """

    def __init__(self, path: str):
        self.path = path
        self._lock = threading.Lock()
        self._frames = []
        self._buffers = []                   # haelt die Pixel-Daten am Leben
        self.durations = []
        self.total = 0
        self.width = 1
        self.height = 1
        self.complete = False
        self.error = None
        self._abort = False
        self._listeners = []                 # nur im Main-Thread benutzt
        self._notified = False

    def start(self):
        threading.Thread(target=self._decode, daemon=True).start()

    def abort(self):
        """Decode stoppen und Speicher freigeben (letzte Referenz weg)."""
        self._abort = True
        with self._lock:
            self._frames = []
            self._buffers = []
            self.durations = []

    def add_listener(self, callback):
        """callback() laeuft im Main-Thread, sobald Frame 0 oder ein
        Fehler vorliegt - sofort, falls schon geschehen."""
        if self._notified:
            GLib.idle_add(lambda: (callback(), False)[1])
        else:
            self._listeners.append(callback)

    def _notify_first(self):
        self._notified = True
        listeners, self._listeners = self._listeners, []
        for cb in listeners:
            try:
                cb()
            except Exception as e:
                log(f"FrameStore listener failed: {e}")
        return False

    def count(self) -> int:
        with self._lock:
            return len(self._frames)

    def get(self, index: int):
        with self._lock:
            if 0 <= index < len(self._frames):
                return self._frames[index], self.durations[index]
        return None, 80

    def _decode(self):
        try:
            with Image.open(self.path) as gif:
                self.total = max(1, int(getattr(gif, "n_frames", 1)))
                src_w, src_h = gif.size

                factor = min(1.0, MAX_SOURCE_DIM / float(max(src_w, src_h)))
                budget = MEMORY_BUDGET_MB * 1024 * 1024
                est = self.total * (src_w * factor) * (src_h * factor) * 4
                if est > budget:
                    factor *= math.sqrt(budget / est)
                tw = max(1, int(round(src_w * factor)))
                th = max(1, int(round(src_h * factor)))
                self.width, self.height = tw, th

                first_sent = False
                for frame in ImageSequence.Iterator(gif):
                    if self._abort:
                        return
                    rgba = frame.convert("RGBA")
                    if rgba.size != (tw, th):
                        rgba = rgba.resize((tw, th), Image.LANCZOS)
                    surf, buf = _pil_to_cairo(rgba)

                    duration = frame.info.get("duration", 80)
                    duration = 80 if duration < 20 else int(duration)

                    with self._lock:
                        if self._abort:
                            return
                        self._frames.append(surf)
                        self._buffers.append(buf)
                        self.durations.append(duration)

                    if not first_sent:
                        first_sent = True
                        GLib.idle_add(self._notify_first)

            with self._lock:
                self.total = len(self._frames)
            if self.total == 0:
                raise RuntimeError("No frames in GIF")
            self.complete = True
        except Exception as e:
            if self._abort:
                return
            with self._lock:
                have = len(self._frames)
                self.total = have
            if have:
                log(f"Partial decode ({have} frames) fuer {self.path}: {e}")
                self.complete = True
            else:
                self.error = str(e)
                GLib.idle_add(self._notify_first)


class FrameStoreRegistry:
    """Mehrere Instanzen desselben GIFs teilen sich EINEN Frame-Satz.

    Das ist der grosse Gewinn der Supervisor-Architektur: 5x dasselbe GIF
    kostet einmal Decode-Zeit und einmal Frame-Speicher.
    """

    def __init__(self):
        self._stores = {}
        self._refs = {}

    def acquire(self, path: str):
        key = os.path.realpath(path)
        store = self._stores.get(key)
        if store is None or (store.error and store.count() == 0):
            store = FrameStore(path)
            store.start()
            self._stores[key] = store
            self._refs[key] = 0
        self._refs[key] += 1
        return key, store

    def release(self, key: str):
        if key not in self._refs:
            return
        self._refs[key] -= 1
        if self._refs[key] <= 0:
            self._refs.pop(key, None)
            store = self._stores.pop(key, None)
            if store is not None:
                store.abort()


# ---------------------------------------------------------------------------
# Ein Widget-Fenster (vom Manager verwaltet)
# ---------------------------------------------------------------------------

class WidgetWindow(Gtk.Window):
    def __init__(self, manager, gif_path: str, widget_id: str,
                 monitor_index=None, state_override=None):
        super().__init__(type=Gtk.WindowType.TOPLEVEL)
        self.manager = manager
        self.widget_id = widget_id
        self.gif_path = gif_path
        # Einstellungen sind pro GIF (Dateistamm) gespeichert; bei mehreren
        # Instanzen desselben GIFs speichert nur die erste ("primaere").
        self.state_key = Path(gif_path).stem
        self.is_primary = (widget_id == self.state_key)
        self._closed = False

        self.set_app_paintable(True)
        self.set_decorated(False)
        self.set_accept_focus(False)

        screen = self.get_screen()
        rgba_visual = screen.get_rgba_visual()
        if rgba_visual:
            self.set_visual(rgba_visual)

        # Monitor-Auswahl (optional)
        display = Gdk.Display.get_default()
        monitor = None
        if display:
            if monitor_index is not None and 0 <= monitor_index < display.get_n_monitors():
                monitor = display.get_monitor(monitor_index)
            if monitor is None:
                monitor = display.get_primary_monitor()
            if monitor is None and display.get_n_monitors() > 0:
                monitor = display.get_monitor(0)
        self.monitor = monitor
        if monitor is not None:
            geom = monitor.get_geometry()
            self.bounds_w = geom.width
            self.bounds_h = geom.height
        else:
            self.bounds_w, self.bounds_h = 1920, 1080
        # Referenz fuer Plausibilitaetspruefungen (aendert sich nie)
        self.mon_w, self.mon_h = self.bounds_w, self.bounds_h

        GtkLayerShell.init_for_window(self)
        if monitor is not None:
            GtkLayerShell.set_monitor(self, monitor)
        GtkLayerShell.set_layer(self, GtkLayerShell.Layer.OVERLAY)
        GtkLayerShell.set_keyboard_mode(self, GtkLayerShell.KeyboardMode.NONE)
        GtkLayerShell.set_namespace(self, f"gif-widget-{widget_id}")
        GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.TOP, True)
        GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.LEFT, True)

        # ---- Zustand ----
        prev = STATE.get(self.state_key)
        if state_override:
            prev = {**prev, **state_override}
        self.window_x = float(prev.get("x", DEFAULT_X))
        self.window_y = float(prev.get("y", DEFAULT_Y))
        self.scale = float(prev.get("scale", DEFAULT_SCALE))
        self.opacity = float(prev.get("opacity", DEFAULT_OPACITY))
        self.flip_h = bool(prev.get("flip_h", False))
        self.flip_v = bool(prev.get("flip_v", False))
        self.speed = float(prev.get("speed", DEFAULT_SPEED))
        self.jumping = bool(prev.get("jumping", False))
        self.jump_rate = float(prev.get("jump_rate", JUMP_RATE_DEFAULT))
        self._pending_bounce = bool(prev.get("bouncing", False))

        if not self.is_primary and not state_override:
            # Weitere Instanzen leicht versetzen, damit Kopien nicht
            # unsichtbar exakt uebereinander liegen.
            k = 1
            tail = widget_id.rsplit("-", 1)
            if len(tail) == 2 and tail[1].isdigit():
                k = max(1, int(tail[1]) - 1)
            self.window_x = min(self.window_x + 36.0 * k, float(self.bounds_w - 64))
            self.window_y = min(self.window_y + 36.0 * k, float(self.bounds_h - 64))

        self.locked = True
        self.paused = False
        self.frame_index = 0

        self.bouncing = False
        self.bounce_vx = 0.0
        self.bounce_vy = 0.0
        self.hop_active = False
        self.hop_t = 0.0
        self.hop_offset_y = 0.0

        self.dragging = False
        self._drag_pending = False
        self._drag_press = (0.0, 0.0)
        self._drag_origin = (0.0, 0.0)
        self._drag_was_bouncing = False
        self._last_click_time = 0.0

        self._canvas_mode = None
        self._state_dirty = False
        self._frame_timer_id = None
        self._frame_deadline = 0.0
        self._bounce_tick_id = None
        self._bounce_last_us = None
        self._hop_tick_id = None
        self._hop_last_us = None
        self._jump_timer_id = None
        self._scroll_accum = 0.0
        self._last_draw_rect = None
        self._first_frame_ready = False

        # ---- Inhalt: eine DrawingArea, alles per Cairo ----
        self.area = Gtk.DrawingArea()
        self.area.connect("draw", self._on_draw)
        self.area.add_events(
            Gdk.EventMask.BUTTON_PRESS_MASK
            | Gdk.EventMask.POINTER_MOTION_MASK
            | Gdk.EventMask.BUTTON_RELEASE_MASK
            | Gdk.EventMask.SCROLL_MASK
            | Gdk.EventMask.SMOOTH_SCROLL_MASK
        )
        self.area.connect("button-press-event", self.on_press)
        self.area.connect("motion-notify-event", self.on_motion)
        self.area.connect("button-release-event", self.on_release)
        self.area.connect("scroll-event", self.on_scroll)
        self.add(self.area)

        self.connect("map-event", self._on_map)
        self.connect("size-allocate", self._on_size_allocate)

        # Geteilter Frame-Satz aus der Registry (dekodiert genau einmal)
        self._store_key, self.store = manager.registry.acquire(gif_path)
        self.store.add_listener(self._on_first_frame)

        self._apply_surface_mode(force=True)
        GLib.timeout_add_seconds(3, self._auto_save)
        self.show_all()

    # ------------------------------------------------------------------
    # Lebenszyklus
    # ------------------------------------------------------------------

    def request_close(self):
        """IPC-'quit': Antwort zuerst, Abbau danach (idle)."""
        GLib.idle_add(self._close_idle)
        return {"ok": True, "shutdown": True}

    def _close_idle(self):
        self.close()
        return False

    def close(self):
        if self._closed:
            return
        self._closed = True
        self._save_now()
        self._cancel_frame_timer()
        self._cancel_jump_timer()
        if self._bounce_tick_id is not None:
            try:
                self.area.remove_tick_callback(self._bounce_tick_id)
            except Exception:
                pass
            self._bounce_tick_id = None
        if self._hop_tick_id is not None:
            try:
                self.area.remove_tick_callback(self._hop_tick_id)
            except Exception:
                pass
            self._hop_tick_id = None
        self.manager.registry.release(self._store_key)
        try:
            self.destroy()
        except Exception:
            pass
        self.manager.on_widget_closed(self.widget_id)

    # ------------------------------------------------------------------
    # Geometrie-Grundlagen
    # ------------------------------------------------------------------

    def gif_size(self):
        w = max(1, int(round(self.store.width * self.scale)))
        h = max(1, int(round(self.store.height * self.scale)))
        return w, h

    def _draw_origin(self):
        """Zeichen-Ursprung, an der tatsaechlichen Allokation festgemacht:
        ist die Surface groesser als das GIF, wird am Offset gezeichnet,
        sonst bei (0,0) -> Uebergaenge bleiben glitchfrei."""
        alloc = self.get_allocation()
        w, h = self.gif_size()
        if alloc.width > w + 2 * GIF_PAD + 8 or alloc.height > h + 2 * GIF_PAD + 8:
            return self.window_x, self.window_y - self.hop_offset_y
        return 0.0, 0.0

    def _gif_rect_padded(self):
        ox, oy = self._draw_origin()
        w, h = self.gif_size()
        return (
            int(math.floor(ox)) - GIF_PAD,
            int(math.floor(oy)) - GIF_PAD,
            w + 2 * GIF_PAD,
            h + 2 * GIF_PAD,
        )

    def _clamp_position(self):
        w, h = self.gif_size()
        self.window_x = max(0.0, min(self.window_x, float(max(0, self.bounds_w - w))))
        self.window_y = max(0.0, min(self.window_y, float(max(0, self.bounds_h - h))))

    # ------------------------------------------------------------------
    # Die EINZIGEN Schreiber von Compositor-Geometrie
    # ------------------------------------------------------------------

    def _wanted_canvas(self) -> bool:
        return (not self.locked) or self.bouncing or self.hop_active or self.dragging

    def _apply_surface_mode(self, force=False):
        want = self._wanted_canvas()
        if want == self._canvas_mode and not force:
            return
        self._canvas_mode = want

        if want:
            GtkLayerShell.set_margin(self, GtkLayerShell.Edge.LEFT, 0)
            GtkLayerShell.set_margin(self, GtkLayerShell.Edge.TOP, 0)
            GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.RIGHT, True)
            GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.BOTTOM, True)
            self.set_size_request(-1, -1)
        else:
            GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.RIGHT, False)
            GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.BOTTOM, False)
            self._sync_compact_size()
            self._sync_compact_margins()

        self._update_input_region()
        self.area.queue_draw()

    def _sync_compact_size(self):
        if self._canvas_mode:
            return
        if self._first_frame_ready:
            w, h = self.gif_size()
        else:
            w, h = 8, 8
        self.set_size_request(w, h)
        try:
            self.resize(w, h)
        except Exception:
            pass

    def _sync_compact_margins(self):
        if self._canvas_mode:
            return
        GtkLayerShell.set_margin(self, GtkLayerShell.Edge.LEFT, int(round(self.window_x)))
        GtkLayerShell.set_margin(self, GtkLayerShell.Edge.TOP, int(round(self.window_y)))

    def _update_input_region(self):
        win = self.get_window()
        if win is None:
            return
        if self.dragging:
            alloc = self.get_allocation()
            region = cairo.Region(cairo.RectangleInt(0, 0, alloc.width, alloc.height))
        elif self.locked:
            region = cairo.Region()
        else:
            x, y, w, h = self._gif_rect_padded()
            region = cairo.Region(cairo.RectangleInt(x, y, w, h))
        win.input_shape_combine_region(region, 0, 0)

    # ------------------------------------------------------------------
    # Positions-API: der einzige Pfad, der window_x/window_y aendert
    # ------------------------------------------------------------------

    def _set_pos(self, x, y, clamp=True):
        old = self._gif_rect_padded()
        self.window_x = float(x)
        self.window_y = float(y)
        if clamp:
            self._clamp_position()
        self._state_dirty = True

        if self._canvas_mode:
            self._queue_redraw(old)
            if not self.locked:
                self._update_input_region()
        else:
            self._sync_compact_margins()

    def _queue_redraw(self, old_rect=None):
        new = self._gif_rect_padded()
        rects = [new]
        if old_rect is not None and old_rect != new:
            rects.append(old_rect)
        if self._last_draw_rect is not None and self._last_draw_rect not in rects:
            rects.append(self._last_draw_rect)
        self._last_draw_rect = new
        for (x, y, w, h) in rects:
            self.area.queue_draw_area(x, y, w, h)

    # ------------------------------------------------------------------
    # Zeichnen
    # ------------------------------------------------------------------

    def _on_draw(self, area, cr):
        cr.save()
        cr.set_operator(cairo.OPERATOR_SOURCE)
        cr.set_source_rgba(0, 0, 0, 0)
        cr.paint()
        cr.restore()

        surf, _dur = self.store.get(self.frame_index)
        if surf is None:
            return False

        ox, oy = self._draw_origin()
        # Uebergang Kompakt->Canvas: Allokation noch klein -> lieber einen
        # Frame nichts zeichnen als das GIF kurz falsch zu platzieren.
        if self._canvas_mode and (ox, oy) == (0.0, 0.0) and (
                self.window_x > 1 or self.window_y > 1):
            alloc = area.get_allocation()
            w0, h0 = self.gif_size()
            if alloc.width <= w0 + 2 * GIF_PAD + 8 and alloc.height <= h0 + 2 * GIF_PAD + 8:
                return False

        w, h = self.gif_size()
        sw = max(1, self.store.width)
        sh = max(1, self.store.height)

        cr.save()
        cr.translate(ox, oy)
        if self.flip_h:
            cr.translate(w, 0)
            cr.scale(-1, 1)
        if self.flip_v:
            cr.translate(0, h)
            cr.scale(1, -1)
        cr.scale(w / sw, h / sh)
        cr.set_source_surface(surf, 0, 0)
        cr.get_source().set_filter(cairo.FILTER_GOOD)
        if self.opacity < 0.999:
            cr.paint_with_alpha(self.opacity)
        else:
            cr.paint()
        cr.restore()

        # Rahmen NUR im Edit-Modus (nicht bei Bounce/Bildschirmschoner)
        if not self.locked:
            cr.save()
            r = 8.0
            x0, y0 = ox - 2.5, oy - 2.5
            x1, y1 = ox + w + 2.5, oy + h + 2.5
            cr.new_sub_path()
            cr.arc(x1 - r, y0 + r, r, -math.pi / 2, 0)
            cr.arc(x1 - r, y1 - r, r, 0, math.pi / 2)
            cr.arc(x0 + r, y1 - r, r, math.pi / 2, math.pi)
            cr.arc(x0 + r, y0 + r, r, math.pi, 3 * math.pi / 2)
            cr.close_path()
            cr.set_source_rgba(1, 1, 1, 0.20)
            cr.set_line_width(1.0)
            cr.stroke()
            cr.restore()

        return False

    # ------------------------------------------------------------------
    # Animation (driftfrei, kein Frame wird uebersprungen)
    # ------------------------------------------------------------------

    def _on_first_frame(self):
        if self._closed:
            return False
        if self.store.error and self.store.count() == 0:
            log(f"Failed to load GIF '{self.gif_path}': {self.store.error}")
            self.close()
            return False
        if self._first_frame_ready:
            return False
        self._first_frame_ready = True
        self._sync_compact_size()
        self._sync_compact_margins()
        self._update_input_region()
        self._queue_redraw()
        self._start_animation()
        # Gespeicherte Modi fortsetzen (brauchen die GIF-Groesse)
        if self._pending_bounce:
            self._pending_bounce = False
            self.start_bounce()
        if self.jumping and self._jump_timer_id is None:
            self._schedule_jump(first=True)
        return False

    def _cancel_frame_timer(self):
        if self._frame_timer_id is not None:
            try:
                GLib.source_remove(self._frame_timer_id)
            except Exception:
                pass
            self._frame_timer_id = None

    def _frame_delay(self, duration_ms):
        if self.speed <= 0:
            return 9.999
        return max(0.001, (duration_ms / 1000.0) / self.speed)

    def _start_animation(self):
        self._cancel_frame_timer()
        if self.paused or not self._first_frame_ready:
            return
        _surf, dur = self.store.get(self.frame_index)
        self._frame_deadline = time.monotonic() + self._frame_delay(dur)
        self._arm_frame_timer()

    def _arm_frame_timer(self):
        delay = self._frame_deadline - time.monotonic()
        if delay < -0.25:
            self._frame_deadline = time.monotonic()
            delay = 0.0
        self._frame_timer_id = GLib.timeout_add(max(0, int(delay * 1000)), self._advance_frame)

    def _advance_frame(self):
        self._frame_timer_id = None
        if self._closed or self.paused or not self._first_frame_ready:
            return False

        nxt = self.frame_index + 1
        count = self.store.count()

        if self.store.complete:
            if count <= 1:
                return False
            nxt %= count
        elif nxt >= count:
            # Decoder haengt hinterher: Frame halten, NICHT ueberspringen
            self._frame_timer_id = GLib.timeout_add(15, self._advance_frame)
            return False

        self.frame_index = nxt
        self._queue_redraw()
        _surf, dur = self.store.get(nxt)
        self._frame_deadline += self._frame_delay(dur)
        self._arm_frame_timer()
        return False

    # ------------------------------------------------------------------
    # Bounce / Hop (Frame-Clock, dt-basiert)
    # ------------------------------------------------------------------

    def start_bounce(self):
        if self.bouncing:
            return
        quadrant = [0.5, math.pi - 0.5, math.pi + 0.5, 2 * math.pi - 0.5]
        angle = random.choice(quadrant) + random.uniform(-0.3, 0.3)
        self.bounce_vx = math.cos(angle) * BOUNCE_SPEED
        self.bounce_vy = math.sin(angle) * BOUNCE_SPEED
        self.bouncing = True
        self._state_dirty = True
        self._apply_surface_mode()
        self._bounce_last_us = None
        if self._bounce_tick_id is None:
            self._bounce_tick_id = self.area.add_tick_callback(self._bounce_tick)

    def stop_bounce(self):
        self.bouncing = False
        self._state_dirty = True
        if self._bounce_tick_id is not None:
            try:
                self.area.remove_tick_callback(self._bounce_tick_id)
            except Exception:
                pass
            self._bounce_tick_id = None
        self._apply_surface_mode()
        self._queue_redraw()

    def toggle_bounce(self):
        if self.bouncing:
            self.stop_bounce()
        else:
            self.start_bounce()

    def _bounce_tick(self, widget, frame_clock):
        if not self.bouncing:
            self._bounce_tick_id = None
            return GLib.SOURCE_REMOVE

        now_us = frame_clock.get_frame_time()
        if self._bounce_last_us is None:
            self._bounce_last_us = now_us
            return GLib.SOURCE_CONTINUE
        dt = min(0.05, max(0.0, (now_us - self._bounce_last_us) / 1_000_000.0))
        self._bounce_last_us = now_us

        w, h = self.gif_size()
        max_x = float(max(0, self.bounds_w - w))
        max_y = float(max(0, self.bounds_h - h))

        nx = self.window_x + self.bounce_vx * dt
        ny = self.window_y + self.bounce_vy * dt

        if nx <= 0.0:
            nx = 0.0
            self.bounce_vx = abs(self.bounce_vx)
        elif nx >= max_x:
            nx = max_x
            self.bounce_vx = -abs(self.bounce_vx)
        if ny <= 0.0:
            ny = 0.0
            self.bounce_vy = abs(self.bounce_vy)
        elif ny >= max_y:
            ny = max_y
            self.bounce_vy = -abs(self.bounce_vy)

        self._set_pos(nx, ny, clamp=False)
        return GLib.SOURCE_CONTINUE

    def hop(self):
        if self.hop_active:
            return
        self.hop_active = True
        self.hop_t = 0.0
        self._apply_surface_mode()
        self._hop_last_us = None
        if self._hop_tick_id is None:
            self._hop_tick_id = self.area.add_tick_callback(self._hop_tick)

    def _cancel_hop(self):
        if self._hop_tick_id is not None:
            try:
                self.area.remove_tick_callback(self._hop_tick_id)
            except Exception:
                pass
            self._hop_tick_id = None
        self.hop_active = False
        self.hop_offset_y = 0.0

    def _hop_tick(self, widget, frame_clock):
        if not self.hop_active:
            self._hop_tick_id = None
            return GLib.SOURCE_REMOVE

        now_us = frame_clock.get_frame_time()
        if self._hop_last_us is None:
            self._hop_last_us = now_us
            return GLib.SOURCE_CONTINUE
        dt = min(0.05, max(0.0, (now_us - self._hop_last_us) / 1_000_000.0))
        self._hop_last_us = now_us

        old = self._gif_rect_padded()
        self.hop_t += dt * (math.pi / HOP_DURATION)
        if self.hop_t >= math.pi:
            self._hop_tick_id = None
            self.hop_active = False
            self.hop_offset_y = 0.0
            self._queue_redraw(old)
            self._apply_surface_mode()
            return GLib.SOURCE_REMOVE
        self.hop_offset_y = math.sin(self.hop_t) * HOP_HEIGHT
        self._queue_redraw(old)
        return GLib.SOURCE_CONTINUE

    # ------------------------------------------------------------------
    # Auto-Jump: zufaellige Hops (Poisson-Prozess)
    # ------------------------------------------------------------------
    #
    # Exponentialverteilte Wartezeiten: mal zwei Jumps kurz hintereinander,
    # dann laengere Pausen. jump_rate = MITTLERER Abstand in Sekunden.

    def _cancel_jump_timer(self):
        if self._jump_timer_id is not None:
            try:
                GLib.source_remove(self._jump_timer_id)
            except Exception:
                pass
            self._jump_timer_id = None

    def _schedule_jump(self, first=False):
        self._cancel_jump_timer()
        mean = max(0.5, self.jump_rate)
        delay = random.expovariate(1.0 / mean)
        delay = max(JUMP_MIN_DELAY, min(delay, mean * JUMP_MAX_FACTOR))
        if first:
            delay = min(delay, mean)
        self._jump_timer_id = GLib.timeout_add(int(delay * 1000), self._jump_fire)

    def _jump_fire(self):
        self._jump_timer_id = None
        if self._closed or not self.jumping:
            return False
        if not self.dragging:
            self.hop()
        self._schedule_jump()
        return False

    def set_jumping(self, on):
        on = bool(on)
        if on != self.jumping:
            self.jumping = on
            self._state_dirty = True
            if on:
                self._schedule_jump(first=True)
            else:
                self._cancel_jump_timer()
        return self.status()

    def toggle_jump(self):
        return self.set_jumping(not self.jumping)

    def set_jump_rate(self, seconds):
        self.jump_rate = max(0.5, min(float(seconds), 60.0))
        self._state_dirty = True
        if self.jumping:
            self._schedule_jump()
        return self.status()

    # ------------------------------------------------------------------
    # Maus-Eingabe (im Canvas-Modus; Koordinatensystem steht still)
    # ------------------------------------------------------------------

    def _point_in_gif(self, x, y):
        rx, ry, rw, rh = self._gif_rect_padded()
        return rx <= x <= rx + rw and ry <= y <= ry + rh

    def on_press(self, widget, event):
        if self.locked:
            return False

        if event.button == 1:
            if not self._point_in_gif(event.x, event.y):
                return False
            now = time.time() * 1000.0
            if now - self._last_click_time < DOUBLE_CLICK_MS:
                self._last_click_time = 0.0
                self._drag_pending = False
                self._show_settings_menu(event)
                return True
            self._last_click_time = now

            self._drag_was_bouncing = self.bouncing
            if self.bouncing:
                self.stop_bounce()
            if self.hop_active:
                old = self._gif_rect_padded()
                self._cancel_hop()
                self._queue_redraw(old)

            self._drag_pending = True
            self._drag_press = (float(event.x), float(event.y))
            self._drag_origin = (self.window_x, self.window_y)
            return True

        if event.button == 3:
            self.set_locked(True)
            return True

        return False

    @staticmethod
    def _snap_axis(v, size, bound):
        for target in (0.0, float(bound - size), (bound - size) / 2.0):
            if abs(v - target) <= EDGE_SNAP:
                return max(0.0, target)
        return v

    def on_motion(self, widget, event):
        if self._drag_pending and not self.dragging:
            dx = event.x - self._drag_press[0]
            dy = event.y - self._drag_press[1]
            if (dx * dx + dy * dy) >= DRAG_THRESHOLD * DRAG_THRESHOLD:
                self.dragging = True
                self._last_click_time = 0.0
                self._update_input_region()

        if not self.dragging:
            return False

        # Canvas steht still -> event.x/y sind absolute Arbeitsflaechen-
        # Koordinaten. Position = Ursprung + (Maus jetzt - Maus beim Press).
        nx = self._drag_origin[0] + (event.x - self._drag_press[0])
        ny = self._drag_origin[1] + (event.y - self._drag_press[1])

        w, h = self.gif_size()
        nx = self._snap_axis(nx, w, self.bounds_w)
        ny = self._snap_axis(ny, h, self.bounds_h)

        self._set_pos(nx, ny)
        return True

    def on_release(self, widget, event):
        if event.button != 1:
            return False
        self._drag_pending = False
        if self.dragging:
            self.dragging = False
            self._update_input_region()
            self._state_dirty = True
        if self._drag_was_bouncing:
            self._drag_was_bouncing = False
            self.start_bounce()
        return True

    def on_scroll(self, widget, event):
        if self.locked:
            return False
        if not self._point_in_gif(event.x, event.y):
            return False
        factor = 1.0
        if event.direction == Gdk.ScrollDirection.UP:
            factor = 1.05
        elif event.direction == Gdk.ScrollDirection.DOWN:
            factor = 1.0 / 1.05
        elif event.direction == Gdk.ScrollDirection.SMOOTH:
            ok, _dx, dy = event.get_scroll_deltas()
            if ok and dy:
                self._scroll_accum += dy
                steps = int(self._scroll_accum)
                if steps != 0:
                    self._scroll_accum -= steps
                    factor = (1.0 / 1.05) ** steps
        if factor != 1.0:
            self.set_scale(self.scale * factor)
        return True

    # ------------------------------------------------------------------
    # Kontextmenue (Doppelklick im Edit-Modus)
    # ------------------------------------------------------------------

    def _show_settings_menu(self, event):
        menu = Gtk.Menu()

        def add_item(label, callback):
            it = Gtk.MenuItem(label=label)
            it.connect("activate", lambda *_: callback())
            menu.append(it)

        def add_submenu(label, items):
            it = Gtk.MenuItem(label=label)
            sub = Gtk.Menu()
            for sub_label, cb in items:
                sub_it = Gtk.MenuItem(label=sub_label)
                sub_it.connect("activate", lambda w, c=cb: c())
                sub.append(sub_it)
            it.set_submenu(sub)
            menu.append(it)

        add_submenu("\U0001F4D0 Size", [
            ("Tiny  (0.3x)", lambda: self.set_scale(0.3)),
            ("Small (0.5x)", lambda: self.set_scale(0.5)),
            ("Normal (0.7x)", lambda: self.set_scale(0.7)),
            ("Big   (1.0x)", lambda: self.set_scale(1.0)),
            ("Huge  (1.5x)", lambda: self.set_scale(1.5)),
        ])
        add_submenu("\U0001F47B Opacity", [
            ("100%", lambda: self.set_opacity(1.0)),
            ("75%", lambda: self.set_opacity(0.75)),
            ("50%", lambda: self.set_opacity(0.5)),
            ("25%", lambda: self.set_opacity(0.25)),
        ])
        add_submenu("\U0001F504 Flip", [
            ("None", lambda: self.set_flip("none")),
            ("Horizontal", lambda: self.set_flip("h")),
            ("Vertical", lambda: self.set_flip("v")),
            ("Both", lambda: self.set_flip("hv")),
        ])
        add_submenu("\u26A1 Speed", [
            ("0.25x (slow-mo)", lambda: self.set_speed(0.25)),
            ("0.5x", lambda: self.set_speed(0.5)),
            ("1x (normal)", lambda: self.set_speed(1.0)),
            ("2x", lambda: self.set_speed(2.0)),
            ("4x (turbo)", lambda: self.set_speed(4.0)),
        ])
        add_submenu("\U0001F4CD Snap", [
            ("Top Left", lambda: self.go_corner("tl")),
            ("Top Right", lambda: self.go_corner("tr")),
            ("Bottom Left", lambda: self.go_corner("bl")),
            ("Bottom Right", lambda: self.go_corner("br")),
            ("Center", lambda: self.go_corner("center")),
        ])
        menu.append(Gtk.SeparatorMenuItem())
        add_submenu("\U0001F998 Jump", [
            ("Einmal huepfen", self.hop),
            ("Auto-Jump " + ("ausschalten" if self.jumping
                             else "einschalten (zufaellig)"), self.toggle_jump),
            ("Rate: ~2 s", lambda: self.set_jump_rate(2)),
            ("Rate: ~5 s", lambda: self.set_jump_rate(5)),
            ("Rate: ~10 s", lambda: self.set_jump_rate(10)),
            ("Rate: ~20 s", lambda: self.set_jump_rate(20)),
        ])
        add_item("\U0001F3D0 " + ("Stop bounce" if self.bouncing else "Start bounce"),
                 self.toggle_bounce)
        add_item("\u267B Reset to defaults", self.reset)
        add_item("\u23F8\uFE0F  Pause" if not self.paused else "\u25B6\uFE0F  Play",
                 lambda: self.set_paused(not self.paused))
        menu.append(Gtk.SeparatorMenuItem())
        add_item("\U0001F512 Lock", lambda: self.set_locked(True))
        add_item("\u274C Close widget", self.request_close)

        menu.show_all()
        menu.popup_at_pointer(event)

    # ------------------------------------------------------------------
    # Persistenz
    # ------------------------------------------------------------------

    def _auto_save(self):
        if self._closed:
            return False
        if self._state_dirty:
            self._save_now()
            self._state_dirty = False
        return True

    def _save_now(self):
        if not self.is_primary:
            return  # nur die erste Instanz eines GIFs speichert ihre Werte
        STATE.put(self.state_key, {
            "x": self.window_x,
            "y": self.window_y,
            "scale": self.scale,
            "opacity": self.opacity,
            "flip_h": self.flip_h,
            "flip_v": self.flip_v,
            "speed": self.speed,
            "bouncing": self.bouncing,
            "jumping": self.jumping,
            "jump_rate": self.jump_rate,
        })

    # ------------------------------------------------------------------
    # Setter (IPC + Menue) - Antworten sind status()-Dicts
    # ------------------------------------------------------------------

    def set_locked(self, locked):
        self.locked = bool(locked)
        if self.locked:
            self._drag_pending = False
            self.dragging = False
        self._apply_surface_mode()
        self._update_input_region()
        self._queue_redraw()
        return self.status()

    def toggle_locked(self):
        return self.set_locked(not self.locked)

    def set_position(self, x, y):
        if self.dragging:
            return self.status()
        if self.bouncing:
            self.stop_bounce()
        self._set_pos(float(x), float(y))
        return self.status()

    def move_by(self, dx, dy):
        if self.dragging:
            return self.status()
        self._set_pos(self.window_x + float(dx), self.window_y + float(dy))
        return self.status()

    def set_scale(self, s):
        s = max(0.1, min(float(s), 5.0))
        if abs(s - self.scale) < 1e-6:
            return self.status()
        old_rect = self._gif_rect_padded()
        w0, h0 = self.gif_size()
        cx = self.window_x + w0 / 2.0
        cy = self.window_y + h0 / 2.0
        self.scale = s
        w1, h1 = self.gif_size()
        self.window_x = cx - w1 / 2.0
        self.window_y = cy - h1 / 2.0
        self._clamp_position()
        self._state_dirty = True
        if self._canvas_mode:
            self._queue_redraw(old_rect)
            if not self.locked:
                self._update_input_region()
        else:
            self._sync_compact_size()
            self._sync_compact_margins()
            self.area.queue_draw()
        return self.status()

    def set_opacity(self, o):
        self.opacity = max(0.05, min(float(o), 1.0))
        self._state_dirty = True
        self._queue_redraw()
        return self.status()

    def set_flip(self, mode):
        mode = str(mode).lower()
        if mode == "none":
            self.flip_h = False
            self.flip_v = False
        elif mode == "h":
            self.flip_h = True
            self.flip_v = False
        elif mode == "v":
            self.flip_v = True
            self.flip_h = False
        elif mode in ("hv", "vh", "both"):
            self.flip_h = True
            self.flip_v = True
        elif mode == "toggle-h":
            self.flip_h = not self.flip_h
        elif mode == "toggle-v":
            self.flip_v = not self.flip_v
        else:
            return {"error": f"Unknown flip mode: {mode}"}
        self._state_dirty = True
        self._queue_redraw()
        return self.status()

    def set_speed(self, s):
        self.speed = max(0.1, min(float(s), 10.0))
        self._state_dirty = True
        self._start_animation()
        return self.status()

    def go_corner(self, pos):
        margin = 20
        w, h = self.gif_size()
        positions = {
            "tl": (margin, margin),
            "tr": (self.bounds_w - w - margin, margin),
            "bl": (margin, self.bounds_h - h - margin),
            "br": (self.bounds_w - w - margin, self.bounds_h - h - margin),
            "center": ((self.bounds_w - w) // 2, (self.bounds_h - h) // 2),
        }
        if pos not in positions:
            return {"error": f"Unknown corner: {pos} (use tl/tr/bl/br/center)"}
        x, y = positions[pos]
        return self.set_position(x, y)

    def reset(self):
        if self.bouncing:
            self.stop_bounce()
        if self.hop_active:
            old = self._gif_rect_padded()
            self._cancel_hop()
            self._queue_redraw(old)
            self._apply_surface_mode()
        old_rect = self._gif_rect_padded()
        self.paused = False
        self.flip_h = False
        self.flip_v = False
        self.scale = DEFAULT_SCALE
        self.opacity = DEFAULT_OPACITY
        self.speed = DEFAULT_SPEED
        if self.jumping:
            self.jumping = False
            self._cancel_jump_timer()
        self.jump_rate = JUMP_RATE_DEFAULT
        self.frame_index = 0
        self._set_pos(DEFAULT_X, DEFAULT_Y)
        if not self._canvas_mode:
            self._sync_compact_size()
            self.area.queue_draw()
        else:
            self._queue_redraw(old_rect)
        self._state_dirty = True
        self._start_animation()
        return self.status()

    def set_paused(self, paused):
        self.paused = bool(paused)
        if self.paused:
            self._cancel_frame_timer()
        else:
            self._start_animation()
        return self.status()

    def status(self):
        w, h = self.gif_size()
        return {
            "ok": True,
            "id": self.widget_id,
            "x": int(self.window_x),
            "y": int(self.window_y),
            "scale": round(self.scale, 3),
            "locked": self.locked,
            "paused": self.paused,
            "opacity": round(self.opacity, 3),
            "flip_h": self.flip_h,
            "flip_v": self.flip_v,
            "speed": round(self.speed, 3),
            "bouncing": self.bouncing,
            "jumping": self.jumping,
            "jump_rate": round(self.jump_rate, 2),
            "size": [w, h],
            "screen": [self.bounds_w, self.bounds_h],
            "frames": [self.store.count(), self.store.total],
            "loading": not self.store.complete,
            "file": self.gif_path,
        }

    # ------------------------------------------------------------------
    # Fenster-Ereignisse
    # ------------------------------------------------------------------

    def _on_map(self, *_):
        self._update_input_region()
        return False

    def _on_size_allocate(self, widget, alloc):
        # Nur plausibel monitor-grosse Allokationen als Arbeitsflaeche
        # uebernehmen - das size-allocate mit der alten, GIF-grossen
        # Allokation beim Moduswechsel darf NIE die Bounds setzen, sonst
        # clampt _clamp_position() alle Positionen auf (0,0).
        if (self._canvas_mode
                and alloc.width >= int(self.mon_w * 0.7)
                and alloc.height >= int(self.mon_h * 0.7)):
            if (alloc.width, alloc.height) != (self.bounds_w, self.bounds_h):
                self.bounds_w = alloc.width
                self.bounds_h = alloc.height
                self._clamp_position()
        self._update_input_region()


# ---------------------------------------------------------------------------
# WidgetManager: Lebenszyklus + Befehls-Dispatch (laeuft im Main-Thread)
# ---------------------------------------------------------------------------

class WidgetManager:
    def __init__(self, registry, widget_factory=None, quit_func=None):
        self.registry = registry
        self.widgets = {}
        self._factory = widget_factory or WidgetWindow
        self._quit = quit_func or Gtk.main_quit
        self._exit_timer = None

    # ---- Lebenszyklus ----

    def allocate_id(self, base: str) -> str:
        if base not in self.widgets:
            return base
        n = 2
        while f"{base}-{n}" in self.widgets:
            n += 1
        return f"{base}-{n}"

    def spawn(self, gif, wid=None, state=None, monitor=None):
        if not gif:
            return {"error": "spawn: Parameter 'gif' fehlt"}
        gif = os.path.abspath(os.path.expanduser(str(gif)))
        if not os.path.exists(gif):
            return {"error": f"GIF not found: {gif}"}
        if wid:
            if wid in self.widgets:
                return {"error": f"'{wid}' laeuft bereits"}
        else:
            wid = self.allocate_id(Path(gif).stem)
        try:
            widget = self._factory(self, gif, wid, monitor, state)
        except Exception as e:
            return {"error": f"spawn failed: {e}"}
        self.widgets[wid] = widget
        self._cancel_exit_timer()
        return widget.status()

    def on_widget_closed(self, wid: str):
        self.widgets.pop(wid, None)
        if not self.widgets:
            self._schedule_exit()

    def close_all(self):
        for wid in list(self.widgets):
            widget = self.widgets.get(wid)
            if widget is not None:
                widget.close()

    def apply_setup(self, entries):
        """Setup atomar anwenden: alles schliessen, dann mit exakten
        Werten neu starten - in-process, ohne Spawn-/Warte-Races."""
        self.close_all()
        counts = {}
        results = []
        for entry in entries or []:
            gif = str(entry.get("gif", ""))
            if not gif or not os.path.exists(os.path.expanduser(gif)):
                results.append({"error": f"GIF fehlt: {gif}"})
                continue
            stem = Path(gif).stem
            counts[stem] = counts.get(stem, 0) + 1
            wid = stem if counts[stem] == 1 else f"{stem}-{counts[stem]}"
            state = {k: entry[k] for k in PROFILE_KEYS if k in entry}
            results.append(self.spawn(gif, wid, state))
        ok_count = sum(1 for r in results if r.get("ok"))
        return {"ok": True, "applied": ok_count, "results": results}

    # ---- Leerlauf-Exit ----

    def _cancel_exit_timer(self):
        if self._exit_timer is not None:
            try:
                GLib.source_remove(self._exit_timer)
            except Exception:
                pass
            self._exit_timer = None

    def _schedule_exit(self):
        self._cancel_exit_timer()
        self._exit_timer = GLib.timeout_add_seconds(
            EMPTY_EXIT_SECONDS, self._exit_if_empty)

    def _exit_if_empty(self):
        self._exit_timer = None
        if not self.widgets:
            log("Keine Widgets mehr - Daemon beendet sich")
            self._quit()
        return False

    # ---- Dispatch ----

    def dispatch(self, cmd: dict) -> dict:
        action = str(cmd.get("action", ""))

        if action == "ping":
            return {"ok": True, "daemon": True, "widgets": len(self.widgets)}
        if action == "list":
            return {"ok": True,
                    "widgets": [self.widgets[w].status()
                                for w in sorted(self.widgets)]}
        if action == "spawn":
            return self.spawn(cmd.get("gif"), cmd.get("id"),
                              cmd.get("state"), cmd.get("monitor"))
        if action == "stop-all":
            n = len(self.widgets)
            self.close_all()
            return {"ok": True, "stopped": n}
        if action == "apply-setup":
            return self.apply_setup(cmd.get("widgets", []))
        if action == "quit-daemon":
            self.close_all()
            GLib.idle_add(self._quit)
            return {"ok": True, "daemon": False}

        wid = cmd.get("id")
        if wid == "*":
            results = {}
            for each in sorted(list(self.widgets)):
                widget = self.widgets.get(each)
                if widget is not None:
                    results[each] = self._widget_action(widget, action, cmd)
            return {"ok": True, "results": results}
        if not wid:
            return {"error": f"Aktion '{action}' braucht eine Widget-'id'"}
        widget = self.widgets.get(wid)
        if widget is None:
            return {"error": f"No widget '{wid}' running"}
        return self._widget_action(widget, action, cmd)

    def _widget_action(self, w, action, cmd):
        handlers = {
            "status":      lambda: w.status(),
            "quit":        lambda: w.request_close(),
            "lock":        lambda: w.set_locked(True),
            "unlock":      lambda: w.set_locked(False),
            "toggle":      lambda: w.toggle_locked(),
            "pause":       lambda: w.set_paused(True),
            "play":        lambda: w.set_paused(False),
            "move":        lambda: w.set_position(cmd["x"], cmd["y"]),
            "move-by":     lambda: w.move_by(cmd["dx"], cmd["dy"]),
            "scale":       lambda: w.set_scale(cmd["scale"]),
            "corner":      lambda: w.go_corner(cmd["position"]),
            "opacity":     lambda: w.set_opacity(cmd["opacity"]),
            "flip":        lambda: w.set_flip(cmd["mode"]),
            "speed":       lambda: w.set_speed(cmd["speed"]),
            "bounce":      lambda: w.toggle_bounce() or w.status(),
            "stop-bounce": lambda: w.stop_bounce() or w.status(),
            "hop":         lambda: w.hop() or w.status(),
            "jump":        lambda: w.toggle_jump(),
            "jump-rate":   lambda: w.set_jump_rate(cmd["seconds"]),
            "reset":       lambda: w.reset(),
        }
        if action not in handlers:
            return {"error": f"Unknown action: {action}"}
        try:
            result = handlers[action]()
        except KeyError as e:
            return {"error": f"Missing parameter: {e}"}
        except Exception as e:
            return {"error": str(e)}
        return result if isinstance(result, dict) else {"ok": True}


# ---------------------------------------------------------------------------
# IPC: Gio.SocketService auf dem Main-Loop (kein Thread, kein Handshake)
# ---------------------------------------------------------------------------

class IpcService:
    def __init__(self, manager, sock_path: Path):
        self.manager = manager
        self.sock_path = sock_path
        self.service = Gio.SocketService.new()
        addr = Gio.UnixSocketAddress.new(str(sock_path))
        self.service.add_address(addr, Gio.SocketType.STREAM,
                                 Gio.SocketProtocol.DEFAULT, None)
        try:
            os.chmod(sock_path, 0o600)
        except Exception:
            pass
        self.service.connect("incoming", self._on_incoming)
        self.service.start()

    def _on_incoming(self, service, conn, source):
        din = Gio.DataInputStream.new(conn.get_input_stream())
        din.read_line_async(GLib.PRIORITY_DEFAULT, None, self._on_line, conn)
        return True

    def _on_line(self, din, res, conn):
        try:
            line, _length = din.read_line_finish(res)
            cmd = json.loads(line.decode()) if line else {}
            if not isinstance(cmd, dict):
                raise ValueError("request must be a JSON object")
            try:
                result = self.manager.dispatch(cmd)
            except Exception as e:
                result = {"error": str(e)}
        except Exception as e:
            result = {"error": f"bad request: {e}"}
        try:
            conn.get_output_stream().write_all(
                (json.dumps(result) + "\n").encode(), None)
        except Exception:
            pass
        try:
            conn.close(None)
        except Exception:
            pass

    def stop(self):
        try:
            self.service.stop()
            self.service.close()
        except Exception:
            pass
        try:
            if self.sock_path.exists():
                self.sock_path.unlink()
        except Exception:
            pass


# ---------------------------------------------------------------------------
# Daemon-Bootstrap
# ---------------------------------------------------------------------------

def cleanup_legacy():
    """Migration: Alt-Versionen (ein Prozess pro GIF, PID-Dateien)
    sauber beenden und ihre Laufzeit-Dateien entfernen."""
    if not RUNTIME_DIR.exists():
        return
    for pf in RUNTIME_DIR.glob("*.pid"):
        try:
            pid = int(pf.read_text().strip())
            os.kill(pid, signal.SIGTERM)
            log(f"Alt-Widget-Prozess beendet: {pf.stem} (pid {pid})")
        except Exception:
            pass
        try:
            pf.unlink()
        except Exception:
            pass
    for sf in RUNTIME_DIR.glob("*.sock"):
        if sf.name != DAEMON_SOCK.name:
            try:
                sf.unlink()
            except Exception:
                pass


def run_daemon() -> int:
    ensure_dirs()
    cleanup_legacy()

    # Single-Instance-Garantie ueber flock (haelt fuer die Lebenszeit)
    lock_file = open(DAEMON_LOCK, "w")
    try:
        fcntl.flock(lock_file, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError:
        log("Daemon laeuft bereits")
        return 0
    lock_file.write(str(os.getpid()))
    lock_file.flush()

    # Lock garantiert Exklusivitaet -> ein vorhandener Socket ist eine Leiche
    try:
        if DAEMON_SOCK.exists():
            DAEMON_SOCK.unlink()
    except Exception:
        pass

    registry = FrameStoreRegistry()
    manager = WidgetManager(registry)
    try:
        service = IpcService(manager, DAEMON_SOCK)
    except Exception as e:
        log(f"Socket-Start fehlgeschlagen: {e}")
        return 1

    def _quit_signal(*_):
        manager.close_all()
        Gtk.main_quit()
        return GLib.SOURCE_REMOVE

    for sig in (signal.SIGINT, signal.SIGTERM):
        try:
            GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, sig, _quit_signal)
        except Exception:
            signal.signal(sig, lambda *_a: Gtk.main_quit())

    # Falls nach dem Start nichts gespawnt wird: nicht als Leiche haengen
    GLib.timeout_add_seconds(10, manager._exit_if_empty)

    log(f"bereit ({DAEMON_SOCK})")
    try:
        Gtk.main()
    except KeyboardInterrupt:
        pass
    finally:
        manager.close_all()
        service.stop()
    return 0


# ---------------------------------------------------------------------------
# Client-Seite (CLI, Picker, Control-Panel nutzen dieselben Funktionen)
# ---------------------------------------------------------------------------

def daemon_send(command: dict, timeout: float = 2.0) -> dict:
    if not DAEMON_SOCK.exists():
        return {"error": "Daemon laeuft nicht"}
    try:
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.settimeout(timeout)
        s.connect(str(DAEMON_SOCK))
        s.sendall((json.dumps(command) + "\n").encode())
        data = b""
        while True:
            chunk = s.recv(65536)
            if not chunk:
                break
            data += chunk
            if data.endswith(b"\n"):
                break
        s.close()
        return json.loads(data.decode().strip()) if data else {"ok": True}
    except Exception as e:
        return {"error": str(e)}


def daemon_alive() -> bool:
    return bool(daemon_send({"action": "ping"}, timeout=0.6).get("ok"))


def ensure_daemon(timeout: float = DAEMON_BOOT_TIMEOUT) -> bool:
    """Daemon bei Bedarf starten und auf Bereitschaft warten."""
    if daemon_alive():
        return True
    ensure_dirs()
    try:
        subprocess.Popen(
            [sys.executable, os.path.abspath(__file__), "daemon"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
    except Exception as e:
        print(f"Daemon-Start fehlgeschlagen: {e}", file=sys.stderr)
        return False
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if daemon_alive():
            return True
        time.sleep(0.1)
    return False


def list_statuses() -> list:
    resp = daemon_send({"action": "list"})
    if isinstance(resp, dict) and resp.get("ok"):
        return resp.get("widgets", [])
    return []


def build_widget_cmd(widget_id, action_args):
    """CLI-Argumente -> Protokoll-Nachricht (mit Validierung)."""
    if not action_args:
        return {"error": "No action specified"}
    action = action_args[0]
    args = action_args[1:]
    cmd = {"action": action, "id": widget_id}
    try:
        if action == "move":
            cmd["x"], cmd["y"] = float(args[0]), float(args[1])
        elif action == "move-by":
            cmd["dx"], cmd["dy"] = float(args[0]), float(args[1])
        elif action == "scale":
            cmd["scale"] = float(args[0])
        elif action == "corner":
            cmd["position"] = args[0]
        elif action == "opacity":
            cmd["opacity"] = float(args[0])
        elif action == "flip":
            cmd["mode"] = args[0]
        elif action == "speed":
            cmd["speed"] = float(args[0])
        elif action == "jump-rate":
            cmd["seconds"] = float(args[0])
    except (IndexError, ValueError) as e:
        return {"error": f"Bad args for {action}: {e}"}
    return cmd


def _launch_sibling(name: str):
    script = Path(__file__).resolve().parent / name
    try:
        subprocess.Popen([sys.executable, str(script)],
                         stdout=subprocess.DEVNULL,
                         stderr=subprocess.DEVNULL,
                         start_new_session=True)
    except Exception as e:
        print(f"Start von {name} fehlgeschlagen: {e}", file=sys.stderr)


def main():
    parser = argparse.ArgumentParser(
        description="GIF-Overlay (Supervisor-Daemon + CLI)")
    sub = parser.add_subparsers(dest="mode")

    p_run = sub.add_parser("run", help="GIF starten (Daemon startet automatisch)")
    p_run.add_argument("gif_path")
    p_run.add_argument("--id", default=None)
    p_run.add_argument("--monitor", type=int, default=None)
    p_run.add_argument("--state", default=None,
                       help="JSON mit Startwerten (ueberschreibt gespeicherten Zustand)")

    p_ipc = sub.add_parser("ipc", help="Befehl an ein Widget")
    p_ipc.add_argument("widget_id")
    p_ipc.add_argument("action_args", nargs="+")

    p_all = sub.add_parser("all", help="Befehl an ALLE Widgets")
    p_all.add_argument("action_args", nargs="+")

    sub.add_parser("list", help="Laufende Widgets")
    sub.add_parser("edit", help="Edit-Modus fuer alle (unlock)")
    sub.add_parser("lock", help="Alle sperren")
    sub.add_parser("stop-all", aliases=["kill-all"], help="Alle Widgets beenden")
    sub.add_parser("daemon", help="Supervisor-Daemon (intern/manuell)")
    sub.add_parser("picker", help="GIF-Picker oeffnen")
    sub.add_parser("control", help="Control-Panel oeffnen")

    args = parser.parse_args()

    if args.mode == "run":
        gif = os.path.abspath(os.path.expanduser(args.gif_path))
        if not os.path.exists(gif):
            print(f"Error: GIF not found: {gif}", file=sys.stderr)
            sys.exit(1)
        if not ensure_daemon():
            print("Error: Daemon nicht erreichbar", file=sys.stderr)
            sys.exit(1)
        cmd = {"action": "spawn", "gif": gif}
        if args.id:
            cmd["id"] = args.id
        if args.monitor is not None:
            cmd["monitor"] = args.monitor
        if args.state:
            try:
                cmd["state"] = json.loads(args.state)
            except Exception as e:
                print(f"--state ignoriert (kein gueltiges JSON): {e}", file=sys.stderr)
        result = daemon_send(cmd, timeout=5.0)
        print(json.dumps(result))
        if "error" in result:
            sys.exit(1)

    elif args.mode == "ipc":
        cmd = build_widget_cmd(args.widget_id, args.action_args)
        result = cmd if "error" in cmd else daemon_send(cmd)
        print(json.dumps(result))
        if "error" in result:
            sys.exit(1)

    elif args.mode == "all":
        cmd = build_widget_cmd("*", args.action_args)
        if "error" in cmd:
            print(json.dumps(cmd))
            sys.exit(1)
        result = daemon_send(cmd)
        if result.get("ok") and not result.get("results"):
            print("Keine Widgets aktiv")
        else:
            print(json.dumps(result))
        if "error" in result:
            sys.exit(1)

    elif args.mode in ("edit", "lock", "stop-all", "kill-all"):
        if args.mode in ("stop-all", "kill-all"):
            result = daemon_send({"action": "stop-all"})
            if result.get("ok"):
                n = result.get("stopped", 0)
                print(f"{n} Widget(s) beendet" if n else "Keine Widgets aktiv")
            else:
                print("Keine Widgets aktiv")
        else:
            sub_action = "unlock" if args.mode == "edit" else "lock"
            verb = "edit" if args.mode == "edit" else "gesperrt"
            result = daemon_send({"action": sub_action, "id": "*"})
            results = result.get("results", {}) if result.get("ok") else {}
            if not results:
                print("Keine Widgets aktiv")
            for wid, r in sorted(results.items()):
                if isinstance(r, dict) and "error" in r:
                    print(f"{wid}: {r['error']}")
                else:
                    print(f"{wid}: {verb}")

    elif args.mode == "list":
        for st in list_statuses():
            print(st.get("id", "?"))

    elif args.mode == "daemon":
        sys.exit(run_daemon())

    elif args.mode == "picker":
        _launch_sibling("gif-picker.py")

    elif args.mode == "control":
        _launch_sibling("gif-control.py")

    else:
        parser.print_help()


if __name__ == "__main__":
    main()
