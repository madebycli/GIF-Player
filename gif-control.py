#!/usr/bin/env python3
"""
gif-control.py - Dashboard fuer alle laufenden GIF-Widgets

Wesentliche Aenderungen gegenueber der alten Version:
  * IPC laeuft DIREKT ueber den Unix-Socket des Daemons. Vorher wurde fuer
    jeden Slider-Tick ein kompletter Python-Prozess (inkl. GTK/PIL-Import)
    gestartet UND danach noch einer fuer den Status - blockierend im
    UI-Thread. Das war die Ursache fuer laggende Slider.
  * Alle IPC-Aufrufe sind asynchron (Thread-Pool); pro Karte ist genau eine
    Anfrage unterwegs, neue Werte werden zusammengefasst -> fluessig.
  * Kein Feedback-Loop mehr: Status-Updates setzen keine Slider, die der
    Nutzer gerade anfasst, und loesen keine erneuten Sends aus.
  * Sliderwerte stehen in Labels fester Breite (draw_value aus) und die
    Karten haben feste Breite -> keine Layout-Spruenge beim Ziehen.
  * Mausrad ueber Slidern/Spinnern ist blockiert -> Scrollen verstellt
    keine Werte mehr.
  * Kein globales show_all() im Poll, Filter via FlowBox-Filter
    -> kein Flackern, keine UI-Verschiebung beim Aktualisieren.
  * Thumbnails laden im Hintergrund.
  * Neu: X/Y-Position direkt einstellbar; Picker aus dem Panel startbar.
"""

import getpass
import json
import socket
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import gi
gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
gi.require_version("GtkLayerShell", "0.1")
gi.require_version("GdkPixbuf", "2.0")

from gi.repository import Gdk, GdkPixbuf, GLib, Gtk, GtkLayerShell
from PIL import Image, ImageSequence

USER = getpass.getuser()
RUNTIME_DIR = Path(f"/tmp/gif-widget-{USER}")
APP_DIR = Path.home() / "Scripts" / "Gif-Overlay"
PICKER_SCRIPT = APP_DIR / "gif-picker.py"

THUMB_SIZE = 88
THUMB_FRAMES = 10
SEND_MIN_INTERVAL_MS = 33      # max. ~30 IPC-Updates/s pro Karte
CARD_WIDTH = 380
POLL_SECONDS = 2

EXECUTOR = ThreadPoolExecutor(max_workers=4)

# HiDPI: wird in main() gesetzt; Thumbnails werden mit UI_SCALE dekodiert
# und als Cairo-Surface mit Device-Scale angezeigt -> scharf auf 2x-Monitoren.
UI_SCALE = 1


def _detect_ui_scale() -> int:
    try:
        display = Gdk.Display.get_default()
        if display:
            monitor = display.get_primary_monitor()
            if monitor is None and display.get_n_monitors() > 0:
                monitor = display.get_monitor(0)
            if monitor is not None:
                return max(1, int(monitor.get_scale_factor()))
    except Exception:
        pass
    return 1

try:
    RESAMPLE = Image.Resampling.LANCZOS
except AttributeError:
    RESAMPLE = Image.LANCZOS


# ---------------------------------------------------------------------------
# IPC - direkt ueber den Socket, ohne Subprozesse
# ---------------------------------------------------------------------------

DAEMON_SOCK = RUNTIME_DIR / "daemon.sock"


def daemon_send(command, timeout=2.0):
    """Eine Anfrage an den Supervisor-Daemon (Protokoll v2)."""
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


def ipc_async(command, callback):
    """Daemon-Anfrage im Thread-Pool; callback(result) im GTK-Main-Thread."""
    def work():
        result = daemon_send(command)
        GLib.idle_add(callback, result)
    EXECUTOR.submit(work)


# ---------------------------------------------------------------------------
# Asynchrones Thumbnail
# ---------------------------------------------------------------------------

class AsyncThumb(Gtk.Box):
    def __init__(self):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        self.get_style_context().add_class("ctrl-thumb")
        self.frames = []
        self.durations = []
        self.frame_index = 0
        self._alive = True
        self._started = False

        placeholder = GdkPixbuf.Pixbuf.new(GdkPixbuf.Colorspace.RGB, True, 8,
                                           THUMB_SIZE, THUMB_SIZE)
        placeholder.fill(0x2b2b2b40)
        self.image = Gtk.Image.new_from_pixbuf(placeholder)
        self.pack_start(self.image, False, False, 0)

        self.connect("destroy", self._on_destroy)

    def ensure(self, gif_path):
        """Laedt das Thumbnail, sobald der echte GIF-Pfad (aus dem
        Widget-Status) bekannt ist - funktioniert damit auch fuer
        Instanzen (name-2) und GIFs in Kategorie-Unterordnern."""
        if self._started or not gif_path:
            return
        self._started = True
        EXECUTOR.submit(self._load, str(gif_path))

    def _on_destroy(self, *_):
        self._alive = False

    def _load(self, gif_path):
        frames, durations = [], []
        px = THUMB_SIZE * UI_SCALE
        try:
            with Image.open(str(gif_path)) as img:
                for i, frame in enumerate(ImageSequence.Iterator(img)):
                    if i >= THUMB_FRAMES:
                        break
                    rgba = frame.convert("RGBA")
                    rgba.thumbnail((px, px), RESAMPLE)
                    square = Image.new("RGBA", (px, px), (0, 0, 0, 0))
                    square.paste(rgba, ((px - rgba.width) // 2,
                                        (px - rgba.height) // 2), rgba)
                    pb = GdkPixbuf.Pixbuf.new_from_bytes(
                        GLib.Bytes.new(square.tobytes()),
                        GdkPixbuf.Colorspace.RGB, True, 8,
                        px, px, px * 4)
                    frames.append(pb)
                    d = frame.info.get("duration", 100)
                    durations.append(80 if d < 20 else d)
        except Exception as e:
            print(f"Thumb load failed for {gif_path}: {e}", file=sys.stderr)
        GLib.idle_add(self._apply, frames, durations)

    def _apply(self, frames, durations):
        if not self._alive or not frames:
            return False
        # Pixbuf -> Cairo-Surface mit Device-Scale (Main-Thread)
        self.frames = [Gdk.cairo_surface_create_from_pixbuf(pb, UI_SCALE, None)
                       for pb in frames]
        self.durations = durations
        self.image.set_from_surface(self.frames[0])
        if len(self.frames) > 1:
            GLib.timeout_add(durations[0], self._tick)
        return False

    def _tick(self):
        if not self._alive or not self.frames:
            return False
        self.frame_index = (self.frame_index + 1) % len(self.frames)
        self.image.set_from_surface(self.frames[self.frame_index])
        GLib.timeout_add(self.durations[self.frame_index], self._tick)
        return False


# ---------------------------------------------------------------------------
# Karte pro Widget
# ---------------------------------------------------------------------------

class WidgetCard(Gtk.Box):
    def __init__(self, widget_id):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        self.widget_id = widget_id
        self._current = {}
        self._updating = False        # True, waehrend Status -> UI geschrieben wird
        self._inflight = False        # genau eine IPC-Anfrage gleichzeitig
        self._pending = {}            # zusammengefasste, noch zu sendende Befehle
        self._hot = set()             # Slider, die der Nutzer gerade anfasst
        self._last_send = 0.0
        self._send_timer = None
        self._alive = True

        self.get_style_context().add_class("card")
        self.set_size_request(CARD_WIDTH, -1)  # feste Breite -> kein Springen
        self.connect("destroy", self._on_destroy)

        # ---- Kopfzeile ----
        header = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        self.thumb = AsyncThumb()
        header.pack_start(self.thumb, False, False, 0)

        text = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
        self.title = Gtk.Label(label=widget_id)
        self.title.set_xalign(0.0)
        self.title.set_ellipsize(3)  # Pango.EllipsizeMode.END
        self.title.get_style_context().add_class("card-title")
        self.status = Gtk.Label(label="laedt\u2026")
        self.status.set_xalign(0.0)
        self.status.set_ellipsize(3)
        self.status.get_style_context().add_class("card-status")
        text.pack_start(self.title, False, False, 0)
        text.pack_start(self.status, False, False, 0)
        header.pack_start(text, True, True, 0)

        self.reset_btn = Gtk.Button(label="\u21BA")
        self.reset_btn.set_tooltip_text("Auf Standardwerte zuruecksetzen")
        self.reset_btn.connect("clicked", lambda *_: self._queue_command("reset"))
        header.pack_start(self.reset_btn, False, False, 0)

        self.stop_btn = Gtk.Button(label="\u2715")
        self.stop_btn.set_tooltip_text("Widget beenden")
        self.stop_btn.connect("clicked", lambda *_: self._queue_command("quit"))
        header.pack_start(self.stop_btn, False, False, 0)

        self.pack_start(header, False, False, 0)

        # ---- Slider ----
        self.scale_scale = self._make_scale(0.1, 5.0, 0.05, "Scale", "%.2f")
        self.opacity_scale = self._make_scale(0.05, 1.0, 0.05, "Opacity", "%.2f")
        self.speed_scale = self._make_scale(0.1, 10.0, 0.1, "Speed", "%.2f")
        self.jump_scale = self._make_scale(1.0, 30.0, 0.5, "Jump \u00D8s", "%.1f")
        self._wire_scale(self.scale_scale, "scale", "scale", "scale")
        self._wire_scale(self.opacity_scale, "opacity", "opacity", "opacity")
        self._wire_scale(self.speed_scale, "speed", "speed", "speed")
        self._wire_scale(self.jump_scale, "jump-rate", "seconds", "jump_rate")
        self._all_scales = (self.scale_scale, self.opacity_scale,
                            self.speed_scale, self.jump_scale)

        # ---- Position ----
        posrow = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        poslab = Gtk.Label(label="Position")
        poslab.set_xalign(0.0)
        poslab.set_width_chars(8)
        poslab.get_style_context().add_class("slider-label")
        posrow.pack_start(poslab, False, False, 0)

        self.spin_x = Gtk.SpinButton.new_with_range(0, 16384, 10)
        self.spin_y = Gtk.SpinButton.new_with_range(0, 16384, 10)
        for sp in (self.spin_x, self.spin_y):
            sp.set_digits(0)
            sp.set_width_chars(5)
            sp.connect("value-changed", self._on_pos_changed)
            sp.connect("scroll-event", lambda *_: True)
        posrow.pack_start(self.spin_x, True, True, 0)
        posrow.pack_start(self.spin_y, True, True, 0)

        self.snap_btn = Gtk.Button(label="\U0001F4CD")
        self.snap_btn.set_tooltip_text("An Ecke/Mitte ausrichten")
        self.snap_btn.connect("clicked", self._show_snap_menu)
        posrow.pack_start(self.snap_btn, False, False, 0)
        self.pack_start(posrow, False, False, 0)

        # ---- Aktionen ----
        row1 = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        self.lock_btn = Gtk.Button(label="\U0001F512")
        self.lock_btn.set_tooltip_text("Edit-Modus umschalten")
        self.pause_btn = Gtk.Button(label="\u23F8")
        self.pause_btn.set_tooltip_text("Pause/Play")
        self.bounce_btn = Gtk.Button(label="\U0001F3D0")
        self.bounce_btn.set_tooltip_text("Bounce umschalten")
        self.jump_btn = Gtk.ToggleButton(label="\U0001F998")
        self.jump_btn.set_tooltip_text(
            "Auto-Jump: springt in zufaelligen Abstaenden (Rate: Slider 'Jump \u00D8s')")
        self.flip_btn = Gtk.Button(label="\u2194")
        self.flip_btn.set_tooltip_text("Spiegeln")

        self.lock_btn.connect("clicked", self._toggle_lock)
        self.pause_btn.connect("clicked", self._toggle_pause)
        self.bounce_btn.connect("clicked", lambda *_: self._queue_command("bounce"))
        self.jump_btn.connect("toggled", self._on_jump_toggled)
        self.flip_btn.connect("clicked", self._show_flip_menu)

        for btn in (self.lock_btn, self.pause_btn, self.bounce_btn,
                    self.jump_btn, self.flip_btn):
            row1.pack_start(btn, True, True, 0)
        self.pack_start(row1, False, False, 0)

        self.refresh()

    def _on_destroy(self, *_):
        self._alive = False

    # ---- Slider-Bau: fester Wert rechts, kein draw_value ----
    def _make_scale(self, lower, upper, step, title, fmt):
        row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        lab = Gtk.Label(label=title)
        lab.set_xalign(0.0)
        lab.set_width_chars(8)
        lab.get_style_context().add_class("slider-label")

        scale = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, lower, upper, step)
        scale.set_draw_value(False)
        scale.set_hexpand(True)
        scale.connect("scroll-event", lambda *_: True)  # Mausrad blockieren

        value = Gtk.Label(label="\u2013")
        value.set_width_chars(5)
        value.set_xalign(1.0)
        value.get_style_context().add_class("slider-value")

        row.pack_start(lab, False, False, 0)
        row.pack_start(scale, True, True, 0)
        row.pack_start(value, False, False, 0)
        self.pack_start(row, False, False, 0)
        scale.value_label = value
        scale.value_fmt = fmt
        return scale

    def _wire_scale(self, scale, action, param_key, status_key):
        scale.ipc_action = action
        scale.ipc_param = param_key
        scale.status_key = status_key
        scale.connect("value-changed", self._on_scale_changed)
        scale.connect("button-press-event", self._on_scale_grab)
        scale.connect("button-release-event", self._on_scale_release)

    def _on_scale_grab(self, scale, event):
        self._hot.add(scale.ipc_action)
        return False

    def _on_scale_release(self, scale, event):
        self._hot.discard(scale.ipc_action)
        return False

    def _on_scale_changed(self, scale):
        scale.value_label.set_text(scale.value_fmt % scale.get_value())
        if self._updating:
            return
        self._queue_command(scale.ipc_action,
                            {scale.ipc_param: round(scale.get_value(), 3)})

    def _on_jump_toggled(self, btn):
        if self._updating:
            return
        self._queue_command("jump")

    def _on_pos_changed(self, *_):
        if self._updating:
            return
        self._queue_command("move", {
            "x": int(self.spin_x.get_value()),
            "y": int(self.spin_y.get_value()),
        })

    # ---- Befehls-Pipeline: coalescing + genau 1 Anfrage in flight ----
    def _queue_command(self, action, params=None):
        self._pending[action] = params or {}
        self._pump()

    def _pump(self):
        if not self._alive or self._inflight or not self._pending:
            return
        now = GLib.get_monotonic_time() / 1000.0  # ms
        wait = SEND_MIN_INTERVAL_MS - (now - self._last_send)
        if wait > 0:
            if self._send_timer is None:
                self._send_timer = GLib.timeout_add(int(wait) + 1, self._pump_timer)
            return
        action, params = next(iter(self._pending.items()))
        del self._pending[action]
        cmd = {"action": action, "id": self.widget_id}
        cmd.update(params)
        self._inflight = True
        self._last_send = now
        ipc_async(cmd, self._on_reply)

    def _pump_timer(self):
        self._send_timer = None
        self._pump()
        return False

    def _on_reply(self, result):
        self._inflight = False
        if not self._alive:
            return False
        if isinstance(result, dict):
            if "error" in result:
                self.status.set_text(result["error"])
            elif result.get("ok") and "id" in result:
                self._apply_status(result)
        self._pump()
        return False

    def refresh(self):
        # Antworten der Setter sind bereits Status-Dicts; extra Status nur,
        # wenn gerade nichts unterwegs ist.
        if self._alive and not self._inflight and not self._pending:
            self._queue_command("status")

    def feed_status(self, st):
        """Vom Sammel-Poll gepusht; greift nicht in laufende Sends ein."""
        if not self._alive or self._inflight or self._pending:
            return
        if isinstance(st, dict) and st.get("ok"):
            self._apply_status(st)

    # ---- Status -> UI (ohne Feedback-Loop) ----
    def _apply_status(self, st):
        self._current = st
        self.thumb.ensure(st.get("file"))
        self._updating = True
        try:
            for scale in self._all_scales:
                action = scale.ipc_action
                if action in self._hot or action in self._pending:
                    continue
                val = float(st.get(scale.status_key, scale.get_value()))
                scale.set_value(val)
                scale.value_label.set_text(scale.value_fmt % val)

            if "jump" not in self._pending:
                self.jump_btn.set_active(bool(st.get("jumping", False)))

            if "move" not in self._pending and not (
                    self.spin_x.has_focus() or self.spin_y.has_focus()):
                self.spin_x.set_value(int(st.get("x", 0)))
                self.spin_y.set_value(int(st.get("y", 0)))

            locked = st.get("locked", True)
            self.lock_btn.set_label("\U0001F512" if locked else "\U0001F513")
            self.pause_btn.set_label("\u25B6" if st.get("paused", False) else "\u23F8")
            size = st.get("size", ["?", "?"])
            extra = "  \u2022  laedt\u2026" if st.get("loading") else ""
            self.status.set_text(
                f"{size[0]}\u00D7{size[1]}  \u2022  {st.get('x', '?')},{st.get('y', '?')}"
                f"  \u2022  {'gesperrt' if locked else 'edit'}{extra}"
            )
        finally:
            self._updating = False

    def _toggle_lock(self, *_):
        locked = self._current.get("locked", True)
        self._queue_command("unlock" if locked else "lock")

    def _toggle_pause(self, *_):
        paused = self._current.get("paused", False)
        self._queue_command("play" if paused else "pause")

    def _show_flip_menu(self, button):
        menu = Gtk.Menu()
        for label, mode in [("None", "none"), ("Horizontal", "h"),
                            ("Vertical", "v"), ("Both", "hv")]:
            item = Gtk.MenuItem(label=label)
            item.connect("activate", lambda *_, m=mode: self._queue_command("flip", {"mode": m}))
            menu.append(item)
        menu.show_all()
        menu.popup_at_widget(button, Gdk.Gravity.SOUTH, Gdk.Gravity.NORTH, None)

    def _show_snap_menu(self, button):
        menu = Gtk.Menu()
        for label, pos in [("Top Left", "tl"), ("Top Right", "tr"),
                           ("Bottom Left", "bl"), ("Bottom Right", "br"),
                           ("Center", "center")]:
            item = Gtk.MenuItem(label=label)
            item.connect("activate", lambda *_, p=pos: self._queue_command("corner", {"position": p}))
            menu.append(item)
        menu.show_all()
        menu.popup_at_widget(button, Gdk.Gravity.SOUTH, Gdk.Gravity.NORTH, None)


# ---------------------------------------------------------------------------
# Hauptfenster
# ---------------------------------------------------------------------------

class ControlWindow(Gtk.Window):
    def __init__(self):
        super().__init__(type=Gtk.WindowType.TOPLEVEL)
        self.set_app_paintable(True)
        self.set_decorated(False)

        # Kompakte Breite (3 Karten-Spalten), monitor-adaptiv
        win_w, win_h = 1220, 860
        display = Gdk.Display.get_default()
        if display:
            monitor = display.get_primary_monitor()
            if monitor is None and display.get_n_monitors() > 0:
                monitor = display.get_monitor(0)
            if monitor is not None:
                geom = monitor.get_geometry()
                win_w = min(1220, max(720, geom.width - 120))
                win_h = min(860, max(520, geom.height - 160))
        # GtkLayerShell ignoriert set_default_size -> Groesse explizit
        # anfordern, sonst kollabiert das Fenster zu einem Balken.
        self.set_size_request(win_w, win_h)

        rgba = self.get_screen().get_rgba_visual()
        if rgba:
            self.set_visual(rgba)

        GtkLayerShell.init_for_window(self)
        GtkLayerShell.set_layer(self, GtkLayerShell.Layer.OVERLAY)
        GtkLayerShell.set_keyboard_mode(self, GtkLayerShell.KeyboardMode.EXCLUSIVE)
        GtkLayerShell.set_namespace(self, "gif-control")

        self.cards = {}
        self._query = ""
        self._setup_css()
        self._build_ui()

        self.connect("key-press-event", self._on_key)
        self.show_all()

        self._poll()
        GLib.timeout_add_seconds(POLL_SECONDS, self._poll)

    def _setup_css(self):
        css = b"""
        .root {
            background: rgba(14, 14, 14, 0.97);
            border: 1px solid rgba(255, 255, 255, 0.12);
            border-radius: 12px;
        }
        .top-hint {
            color: rgba(255, 255, 255, 0.30);
            font-size: 11px;
            font-family: monospace;
        }
        .empty-state {
            color: rgba(255, 255, 255, 0.45);
            font-style: italic;
            font-size: 13px;
            font-family: monospace;
        }
        .card {
            background: rgba(255, 255, 255, 0.03);
            border: 1px solid rgba(255, 255, 255, 0.07);
            border-radius: 10px;
            padding: 10px;
        }
        .card-title {
            color: rgba(255, 255, 255, 0.88);
            font-size: 13px;
            font-family: monospace;
            font-weight: bold;
        }
        .card-status {
            color: rgba(255, 255, 255, 0.45);
            font-size: 11px;
            font-family: monospace;
        }
        .slider-label {
            color: rgba(255, 255, 255, 0.40);
            font-size: 11px;
            font-family: monospace;
        }
        .slider-value {
            color: rgba(255, 255, 255, 0.70);
            font-size: 11px;
            font-family: monospace;
        }
        button {
            background: rgba(255, 255, 255, 0.05);
            color: rgba(255, 255, 255, 0.88);
            border: 1px solid rgba(255, 255, 255, 0.10);
            border-radius: 6px;
            font-family: monospace;
        }
        button:hover {
            background: rgba(255, 255, 255, 0.09);
            border-color: rgba(255, 255, 255, 0.20);
        }
        button:checked {
            background: rgba(255, 255, 255, 0.16);
            border-color: rgba(255, 255, 255, 0.35);
        }
        entry, spinbutton {
            background: rgba(255, 255, 255, 0.05);
            color: rgba(255, 255, 255, 0.88);
            border: 1px solid rgba(255, 255, 255, 0.12);
            border-radius: 6px;
            padding: 6px 10px;
            font-size: 13px;
            font-family: monospace;
        }
        entry:focus, spinbutton:focus {
            border-color: rgba(255, 255, 255, 0.40);
        }
        spinbutton button {
            border: none;
            background: transparent;
        }
        scale trough { min-height: 6px; }
        scrolledwindow undershoot, scrolledwindow overshoot { background: none; }
        """
        provider = Gtk.CssProvider()
        provider.load_from_data(css)
        Gtk.StyleContext.add_provider_for_screen(
            self.get_screen(), provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
        )

    def _build_ui(self):
        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        outer.get_style_context().add_class("root")

        header = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        header.set_margin_top(16)
        header.set_margin_bottom(10)
        header.set_margin_start(18)
        header.set_margin_end(18)

        self.search = Gtk.Entry()
        self.search.set_placeholder_text("Laufende Widgets suchen\u2026")
        self.search.set_hexpand(True)
        self.search.connect("changed", self._on_search)
        header.pack_start(self.search, True, True, 0)

        picker_btn = Gtk.Button(label="\uFF0B GIF")
        picker_btn.set_tooltip_text("Picker oeffnen")
        picker_btn.connect("clicked", self._open_picker)
        header.pack_start(picker_btn, False, False, 0)

        refresh = Gtk.Button(label="\u21BB")
        refresh.set_tooltip_text("Aktualisieren")
        refresh.connect("clicked", lambda *_: self._poll())
        header.pack_start(refresh, False, False, 0)

        outer.pack_start(header, False, False, 0)

        hint = Gtk.Label(label="Alle Regler wirken live  \u2022  Esc schliesst")
        hint.get_style_context().add_class("top-hint")
        hint.set_margin_bottom(10)
        hint.set_xalign(0.5)
        outer.pack_start(hint, False, False, 0)

        outer.pack_start(Gtk.Separator(), False, False, 0)

        scroll = Gtk.ScrolledWindow()
        scroll.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)

        self.flow = Gtk.FlowBox()
        self.flow.set_valign(Gtk.Align.START)
        self.flow.set_selection_mode(Gtk.SelectionMode.NONE)
        self.flow.set_homogeneous(True)
        self.flow.set_row_spacing(12)
        self.flow.set_column_spacing(12)
        self.flow.set_margin_top(18)
        self.flow.set_margin_bottom(18)
        self.flow.set_margin_start(18)
        self.flow.set_margin_end(18)
        self.flow.set_max_children_per_line(3)
        self.flow.set_min_children_per_line(1)
        self.flow.set_filter_func(self._filter_func, None)

        scroll.add(self.flow)
        outer.pack_start(scroll, True, True, 0)

        self.empty = Gtk.Label(label="Keine GIFs aktiv.\n\u00DCber \uFF0B GIF den Picker oeffnen.")
        self.empty.get_style_context().add_class("empty-state")
        self.empty.set_justify(Gtk.Justification.CENTER)
        self.empty.set_margin_bottom(24)
        self.empty.set_no_show_all(True)
        outer.pack_start(self.empty, False, False, 0)

        self.add(outer)

    def _open_picker(self, *_):
        try:
            subprocess.Popen(["python3", str(PICKER_SCRIPT)],
                             stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL,
                             start_new_session=True)
        except Exception as e:
            print(f"Picker launch failed: {e}", file=sys.stderr)
        Gtk.main_quit()

    def _on_search(self, entry):
        self._query = entry.get_text().strip().lower()
        self.flow.invalidate_filter()

    def _filter_func(self, child, _data):
        if not self._query:
            return True
        card = child.get_child()
        return isinstance(card, WidgetCard) and self._query in card.widget_id.lower()

    def _sync_widgets(self, running):

        for wid in running:
            if wid not in self.cards:
                card = WidgetCard(wid)
                self.cards[wid] = card
                card.show_all()
                self.flow.add(card)
                # FlowBox erzeugt einen Child-Wrapper; bei Adds nach show_all()
                # muss der explizit sichtbar gemacht werden.
                parent = card.get_parent()
                if parent is not None:
                    parent.show()

        for wid in list(self.cards.keys()):
            if wid not in running:
                card = self.cards.pop(wid)
                parent = card.get_parent()
                if parent is not None:
                    parent.destroy()
                else:
                    card.destroy()

        has_cards = bool(self.cards)
        self.empty.set_visible(not has_cards)
        self.flow.invalidate_filter()

    def _poll(self):
        ipc_async({"action": "list"}, self._on_list)
        return True

    def _on_list(self, resp):
        statuses = []
        if isinstance(resp, dict) and resp.get("ok"):
            statuses = resp.get("widgets", [])
        self._sync_widgets([st.get("id") for st in statuses if st.get("id")])
        for st in statuses:
            card = self.cards.get(st.get("id"))
            if card is not None:
                card.feed_status(st)
        return False

    def _on_key(self, widget, event):
        if event.keyval == Gdk.KEY_Escape:
            Gtk.main_quit()
            return True
        return False


def main():
    global UI_SCALE
    UI_SCALE = _detect_ui_scale()
    ControlWindow()
    try:
        Gtk.main()
    except KeyboardInterrupt:
        pass
    finally:
        # Ausstehende Jobs verwerfen, sonst haengt der Prozess beim Beenden.
        try:
            EXECUTOR.shutdown(wait=False, cancel_futures=True)
        except TypeError:  # Python < 3.9
            EXECUTOR.shutdown(wait=False)


if __name__ == "__main__":
    main()
