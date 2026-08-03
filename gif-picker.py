#!/usr/bin/env python3
"""
gif-picker.py - Schneller, breiter GIF-Picker

Wesentliche Aenderungen gegenueber der alten Version:
  * Das Fenster erscheint SOFORT: Thumbnails werden im Thread-Pool geladen,
    bis dahin zeigen die Kacheln einen pulsierenden Skeleton-Platzhalter.
  * Disk-Cache fuer das erste Frame (~/.cache/gif-overlay/thumbs)
    -> ab dem zweiten Oeffnen sind alle Vorschauen quasi instant da.
  * Animationen starten erst bei Hover; die Animations-Frames werden auch
    erst beim ersten Hover dekodiert -> minimale CPU/RAM-Last im Leerlauf.
  * Kategorien: Unterordner von Gifs/ erscheinen als Filter-Chips
    (Zeile wird nur angezeigt, wenn es Unterordner gibt).
  * Bereits laufende Widgets sind markiert; Klick darauf loest einen Hop
    aus statt eines stillen Fehlstarts.
  * Pfeil hoch/runter + Enter zur Auswahl, Esc schliesst.
"""

import getpass
import hashlib
import json
import os
import socket
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import gi
gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
gi.require_version("GtkLayerShell", "0.1")
gi.require_version("GdkPixbuf", "2.0")

from gi.repository import Gtk, Gdk, GLib, GtkLayerShell, GdkPixbuf
from PIL import Image, ImageSequence

USER = getpass.getuser()
RUNTIME_DIR = Path(f"/tmp/gif-widget-{USER}")
APP_DIR = Path.home() / "Scripts" / "Gif-Overlay"
GIF_DIR = APP_DIR / "Gifs"
DAEMON_SCRIPT = APP_DIR / "gif-script.py"
CACHE_DIR = Path.home() / ".cache" / "gif-overlay" / "thumbs"
PROFILE_FILE = Path.home() / ".config" / "gif-widget" / "profiles.json"

THUMB_SIZE = 124             # logische Pixel
HOVER_FRAMES = 14            # Frames fuer die Hover-Animation
EXECUTOR = ThreadPoolExecutor(max_workers=max(2, (os.cpu_count() or 2)))

# HiDPI: wird in main() gesetzt; Thumbnails werden mit UI_SCALE dekodiert
# und als Cairo-Surface mit Device-Scale angezeigt -> scharf auf 2x-Monitoren.
UI_SCALE = 1
THUMB_PX = THUMB_SIZE


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
# Hilfen
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


def daemon_alive():
    return bool(daemon_send({"action": "ping"}, timeout=0.6).get("ok"))


def ensure_daemon(timeout=6.0):
    """Daemon bei Bedarf starten und auf Bereitschaft warten."""
    if daemon_alive():
        return True
    RUNTIME_DIR.mkdir(parents=True, exist_ok=True)
    try:
        subprocess.Popen([sys.executable, str(DAEMON_SCRIPT), "daemon"],
                         stdout=subprocess.DEVNULL,
                         stderr=subprocess.DEVNULL,
                         start_new_session=True)
    except Exception as e:
        print(f"Daemon-Start fehlgeschlagen: {e}", file=sys.stderr)
        return False
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if daemon_alive():
            return True
        time.sleep(0.1)
    return False


def list_statuses():
    resp = daemon_send({"action": "list"})
    if isinstance(resp, dict) and resp.get("ok"):
        return resp.get("widgets", [])
    return []


def running_ids():
    return {st.get("id") for st in list_statuses() if st.get("id")}


# ---------------------------------------------------------------------------
# Setups / Profile
# ---------------------------------------------------------------------------

PROFILE_KEYS = ("x", "y", "scale", "opacity", "flip_h", "flip_v",
                "speed", "bouncing", "jumping", "jump_rate")


def load_profiles() -> dict:
    try:
        if PROFILE_FILE.exists():
            data = json.loads(PROFILE_FILE.read_text())
            if isinstance(data, dict):
                return data
    except Exception as e:
        print(f"Profiles load failed: {e}", file=sys.stderr)
    return {}


def save_profiles(profiles: dict):
    try:
        PROFILE_FILE.parent.mkdir(parents=True, exist_ok=True)
        tmp = PROFILE_FILE.with_name(PROFILE_FILE.name + ".tmp")
        tmp.write_text(json.dumps(profiles, indent=2))
        os.replace(tmp, PROFILE_FILE)
    except Exception as e:
        print(f"Profiles save failed: {e}", file=sys.stderr)


def snapshot_setup() -> list:
    """Aktuelles Setup (alle laufenden Widgets) als Profil-Eintraege -
    EIN list-Aufruf liefert alle Status-Dicts."""
    widgets = []
    for st in list_statuses():
        if isinstance(st, dict) and st.get("ok") and st.get("file"):
            entry = {"gif": st["file"]}
            for key in PROFILE_KEYS:
                if key in st:
                    entry[key] = st[key]
            widgets.append(entry)
    return widgets


def _cache_key(path: Path) -> str:
    st = path.stat()
    raw = f"{path}|{st.st_mtime_ns}|{st.st_size}|{THUMB_PX}"
    return hashlib.sha1(raw.encode()).hexdigest()


def _square_pixbuf(rgba: Image.Image) -> GdkPixbuf.Pixbuf:
    rgba.thumbnail((THUMB_PX, THUMB_PX), RESAMPLE)
    square = Image.new("RGBA", (THUMB_PX, THUMB_PX), (0, 0, 0, 0))
    square.paste(rgba, ((THUMB_PX - rgba.width) // 2,
                        (THUMB_PX - rgba.height) // 2), rgba)
    return GdkPixbuf.Pixbuf.new_from_bytes(
        GLib.Bytes.new(square.tobytes()),
        GdkPixbuf.Colorspace.RGB, True, 8,
        THUMB_PX, THUMB_PX, THUMB_PX * 4,
    )


def _pb_to_surface(pb: GdkPixbuf.Pixbuf):
    """Pixbuf -> Cairo-Surface mit Device-Scale (nur im Main-Thread rufen)."""
    return Gdk.cairo_surface_create_from_pixbuf(pb, UI_SCALE, None)


def load_first_frame(path: Path):
    """Erstes Frame, mit Disk-Cache. Laeuft im Worker-Thread."""
    try:
        CACHE_DIR.mkdir(parents=True, exist_ok=True)
        cache_file = CACHE_DIR / (_cache_key(path) + ".png")
        if cache_file.exists():
            return GdkPixbuf.Pixbuf.new_from_file(str(cache_file))
        with Image.open(str(path)) as img:
            pb = _square_pixbuf(img.convert("RGBA"))
        try:
            pb.savev(str(cache_file), "png", [], [])
        except Exception:
            pass
        return pb
    except Exception as e:
        print(f"Thumb load failed for {path}: {e}", file=sys.stderr)
        return None


def load_hover_frames(path: Path):
    """Bis zu HOVER_FRAMES animierte Frames. Laeuft im Worker-Thread."""
    frames, durations = [], []
    try:
        with Image.open(str(path)) as img:
            total = getattr(img, "n_frames", 1)
            step = max(1, total // HOVER_FRAMES) if total > HOVER_FRAMES else 1
            for frame_no, frame in enumerate(ImageSequence.Iterator(img)):
                if frame_no % step != 0:
                    continue
                frames.append(_square_pixbuf(frame.convert("RGBA")))
                d = frame.info.get("duration", 100)
                durations.append(80 if d < 20 else d)
                if len(frames) >= HOVER_FRAMES:
                    break
    except Exception as e:
        print(f"Hover frames failed for {path}: {e}", file=sys.stderr)
    return frames, durations


def prune_thumb_cache(valid_keys: set):
    """Loescht Cache-Eintraege, zu denen es kein aktuelles GIF (mehr) gibt.

    Laeuft im Worker-Thread im Hintergrund; haelt den Cache klein, wenn
    GIFs geloescht/geaendert werden oder sich die Thumbnail-Groesse aendert.
    """
    try:
        if not CACHE_DIR.exists():
            return
        for f in CACHE_DIR.glob("*.png"):
            if f.stem not in valid_keys:
                try:
                    f.unlink()
                except Exception:
                    pass
    except Exception:
        pass


# ---------------------------------------------------------------------------
# Kachel
# ---------------------------------------------------------------------------

class GifTile(Gtk.Box):
    def __init__(self, gif_path: Path, category: str, on_activate, running: bool):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        self.gif_path = gif_path
        self.category = category
        self.on_activate = on_activate
        self.running = running

        self._frames = None
        self._durations = None
        self._loading_anim = False
        self._hovered = False
        self._anim_token = 0
        self._frame_index = 0
        self._first_surface = None
        self._alive = True

        self.get_style_context().add_class("thumb-tile")

        placeholder = GdkPixbuf.Pixbuf.new(GdkPixbuf.Colorspace.RGB, True, 8,
                                           THUMB_SIZE, THUMB_SIZE)
        placeholder.fill(0xffffff10)
        self.image = Gtk.Image.new_from_pixbuf(placeholder)
        self.image.get_style_context().add_class("thumb-skel")

        self.label = Gtk.Label()
        name = GLib.markup_escape_text(gif_path.stem)
        if running:
            self.label.set_markup(f'<span foreground="#8ee6a1">\u25CF</span> {name}')
        else:
            self.label.set_text(gif_path.stem)
        self.label.set_max_width_chars(16)
        self.label.set_ellipsize(3)  # Pango.EllipsizeMode.END
        self.label.set_xalign(0.5)
        self.label.get_style_context().add_class("thumb-label")

        inner = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        for m in ("top", "bottom", "start", "end"):
            getattr(inner, f"set_margin_{m}")(10)
        inner.pack_start(self.image, False, False, 0)
        inner.pack_start(self.label, False, False, 0)

        self.event_box = Gtk.EventBox()
        self.event_box.add_events(
            Gdk.EventMask.BUTTON_PRESS_MASK
            | Gdk.EventMask.ENTER_NOTIFY_MASK
            | Gdk.EventMask.LEAVE_NOTIFY_MASK
        )
        self.event_box.add(inner)
        if running:
            self.event_box.set_tooltip_text(
                "Laeuft bereits \u2013 Klick startet eine weitere Instanz")
        self.event_box.connect("button-press-event", self._on_click)
        self.event_box.connect("enter-notify-event", self._on_enter)
        self.event_box.connect("leave-notify-event", self._on_leave)
        self.pack_start(self.event_box, True, True, 0)

        self.connect("destroy", self._on_destroy)
        EXECUTOR.submit(self._load_first)

    def _on_destroy(self, *_):
        self._alive = False

    # -- Erstes Frame (mit Cache) --
    def _load_first(self):
        pb = load_first_frame(self.gif_path)
        GLib.idle_add(self._set_first, pb)

    def _set_first(self, pb):
        if not self._alive:
            return False
        self.image.get_style_context().remove_class("thumb-skel")
        if pb is not None:
            self._first_surface = _pb_to_surface(pb)
            if not self._hovered or self._frames is None:
                self.image.set_from_surface(self._first_surface)
        return False

    # -- Hover-Animation (lazy) --
    def _on_enter(self, *_):
        self.get_style_context().add_class("hover")
        self._hovered = True
        if self._frames:
            self._start_anim()
        elif not self._loading_anim:
            self._loading_anim = True
            EXECUTOR.submit(self._load_anim)
        return False

    def _on_leave(self, *_):
        self.get_style_context().remove_class("hover")
        self._hovered = False
        self._anim_token += 1  # laufende Timer-Kette beenden
        if self._first_surface is not None:
            self.image.set_from_surface(self._first_surface)
        return False

    def _load_anim(self):
        frames, durations = load_hover_frames(self.gif_path)
        GLib.idle_add(self._anim_ready, frames, durations)

    def _anim_ready(self, frames, durations):
        if not self._alive:
            return False
        if frames:
            self._frames = [_pb_to_surface(pb) for pb in frames]
            self._durations = durations
            if self._hovered:
                self._start_anim()
        return False

    def _start_anim(self):
        if not self._frames or len(self._frames) < 2:
            return
        self._anim_token += 1
        token = self._anim_token
        self._frame_index = 0
        self.image.set_from_surface(self._frames[0])
        GLib.timeout_add(self._durations[0], self._anim_tick, token)

    def _anim_tick(self, token):
        if not self._alive or token != self._anim_token or not self._hovered:
            return False
        self._frame_index = (self._frame_index + 1) % len(self._frames)
        self.image.set_from_surface(self._frames[self._frame_index])
        GLib.timeout_add(self._durations[self._frame_index], self._anim_tick, token)
        return False

    def _on_click(self, widget, event):
        if event.button == 1:
            self.on_activate(self)
            return True
        return False

    def set_selected(self, selected):
        ctx = self.get_style_context()
        if selected:
            ctx.add_class("selected")
        else:
            ctx.remove_class("selected")


class NamePrompt(Gtk.Window):
    """Kleiner Layer-Shell-Dialog fuer Namenseingabe (Speichern/Umbenennen)."""

    def __init__(self, title: str, initial: str, on_done):
        super().__init__(type=Gtk.WindowType.TOPLEVEL)
        self._on_done = on_done
        self.set_app_paintable(True)
        self.set_decorated(False)
        rgba = self.get_screen().get_rgba_visual()
        if rgba:
            self.set_visual(rgba)

        GtkLayerShell.init_for_window(self)
        GtkLayerShell.set_layer(self, GtkLayerShell.Layer.OVERLAY)
        GtkLayerShell.set_keyboard_mode(self, GtkLayerShell.KeyboardMode.EXCLUSIVE)
        GtkLayerShell.set_namespace(self, "gif-prompt")
        self.set_size_request(420, -1)

        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        box.get_style_context().add_class("picker-root")
        for m in ("top", "bottom", "start", "end"):
            getattr(box, f"set_margin_{m}")(16)

        lab = Gtk.Label(label=title)
        lab.get_style_context().add_class("thumb-label")
        lab.set_xalign(0.0)
        box.pack_start(lab, False, False, 0)

        self.entry = Gtk.Entry()
        self.entry.set_text(initial)
        self.entry.connect("activate", lambda *_: self._finish(True))
        box.pack_start(self.entry, False, False, 0)

        btns = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        ok = Gtk.Button(label="OK")
        ok.connect("clicked", lambda *_: self._finish(True))
        cancel = Gtk.Button(label="Abbrechen")
        cancel.connect("clicked", lambda *_: self._finish(False))
        btns.pack_end(ok, False, False, 0)
        btns.pack_end(cancel, False, False, 0)
        box.pack_start(btns, False, False, 0)

        self.add(box)
        self.connect("key-press-event", self._on_key)
        self.show_all()
        self.entry.grab_focus()
        self.entry.select_region(0, -1)

    def _on_key(self, widget, event):
        if event.keyval == Gdk.KEY_Escape:
            self._finish(False)
            return True
        return False

    def _finish(self, ok):
        name = self.entry.get_text().strip()
        self.destroy()
        if ok and name:
            self._on_done(name)


# ---------------------------------------------------------------------------
# Hauptfenster
# ---------------------------------------------------------------------------

class PickerWindow(Gtk.Window):
    def __init__(self, gif_dir: Path):
        super().__init__(type=Gtk.WindowType.TOPLEVEL)
        self.gif_dir = gif_dir
        self._filter_query = ""
        self._active_category = None
        self._selected = None
        self._running = running_ids()
        self._profiles = load_profiles()

        self.set_app_paintable(True)
        self.set_decorated(False)

        # Kompakte Breite: ~8 Spalten (wie das urspruengliche Layout),
        # nie breiter, als der Monitor erlaubt. Hoehe monitor-adaptiv.
        target_w = 8 * (THUMB_SIZE + 44) + 72
        win_w, win_h = target_w, 980
        display = Gdk.Display.get_default()
        if display:
            monitor = display.get_primary_monitor()
            if monitor is None and display.get_n_monitors() > 0:
                monitor = display.get_monitor(0)
            if monitor is not None:
                geom = monitor.get_geometry()
                win_w = min(target_w, max(720, geom.width - 120))
                win_h = min(980, max(560, geom.height - 160))
        # WICHTIG: GtkLayerShell ignoriert set_default_size - die Surface-
        # Groesse kommt aus der ANGEFORDERTEN Widget-Groesse. Ohne explizite
        # Anforderung kollabiert die Hoehe zu einem schmalen Balken.
        self.set_size_request(win_w, win_h)
        self._max_cols = max(4, min(10, (win_w - 80) // (THUMB_SIZE + 44)))

        screen = self.get_screen()
        rgba = screen.get_rgba_visual()
        if rgba:
            self.set_visual(rgba)

        GtkLayerShell.init_for_window(self)
        GtkLayerShell.set_layer(self, GtkLayerShell.Layer.OVERLAY)
        GtkLayerShell.set_keyboard_mode(self, GtkLayerShell.KeyboardMode.EXCLUSIVE)
        GtkLayerShell.set_namespace(self, "gif-picker")

        self._setup_css()
        self._build_ui()

        self.connect("key-press-event", self._on_key)
        self.show_all()
        self.search.grab_focus()

    # ------------------------------------------------------------------
    def _setup_css(self):
        css = b"""
        .picker-root {
            background: rgba(14, 14, 14, 0.97);
            border: 1px solid rgba(255, 255, 255, 0.12);
            border-radius: 12px;
        }
        .picker-hint {
            color: rgba(255, 255, 255, 0.30);
            font-size: 11px;
            font-family: monospace;
        }
        .picker-empty {
            color: rgba(255, 255, 255, 0.45);
            font-style: italic;
            font-size: 13px;
            font-family: monospace;
        }
        .thumb-tile {
            background: rgba(255, 255, 255, 0.03);
            border-radius: 10px;
            border: 1px solid rgba(255, 255, 255, 0.07);
            transition: all 120ms;
        }
        .thumb-tile.hover {
            background: rgba(255, 255, 255, 0.08);
            border-color: rgba(255, 255, 255, 0.20);
        }
        .thumb-tile.selected {
            border-color: rgba(255, 255, 255, 0.45);
            background: rgba(255, 255, 255, 0.06);
        }
        .thumb-label {
            color: rgba(255, 255, 255, 0.86);
            font-size: 12px;
            font-family: monospace;
        }
        @keyframes skel-pulse {
            0%   { opacity: 0.35; }
            50%  { opacity: 0.9; }
            100% { opacity: 0.35; }
        }
        .thumb-skel {
            animation: skel-pulse 1.2s ease-in-out infinite;
        }
        .x-btn {
            background: rgba(255, 255, 255, 0.05);
            color: rgba(255, 255, 255, 0.9);
            border: 1px solid rgba(255, 255, 255, 0.10);
            border-radius: 999px;
            min-width: 30px;
            min-height: 30px;
            padding: 0;
            font-family: monospace;
        }
        .x-btn:hover {
            background: rgba(255, 255, 255, 0.10);
            border-color: rgba(255, 255, 255, 0.24);
        }
        .chip {
            background: rgba(255, 255, 255, 0.04);
            color: rgba(255, 255, 255, 0.65);
            border: 1px solid rgba(255, 255, 255, 0.10);
            border-radius: 999px;
            padding: 2px 14px;
            font-size: 12px;
            font-family: monospace;
        }
        .chip:checked {
            background: rgba(255, 255, 255, 0.14);
            color: rgba(255, 255, 255, 0.95);
            border-color: rgba(255, 255, 255, 0.30);
        }
        entry {
            background: rgba(255, 255, 255, 0.05);
            color: rgba(255, 255, 255, 0.88);
            border: 1px solid rgba(255, 255, 255, 0.12);
            border-radius: 6px;
            padding: 8px 12px;
            font-size: 13px;
            font-family: monospace;
        }
        entry:focus {
            border-color: rgba(255, 255, 255, 0.40);
        }
        scrolledwindow undershoot,
        scrolledwindow overshoot { background: none; }
        """
        provider = Gtk.CssProvider()
        provider.load_from_data(css)
        Gtk.StyleContext.add_provider_for_screen(
            self.get_screen(), provider,
            Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION,
        )

    # ------------------------------------------------------------------
    def _scan_gifs(self):
        """[(pfad, kategorie)] - Kategorie = Unterordner relativ zu Gifs/."""
        items = []
        for p in sorted(self.gif_dir.rglob("*.gif")):
            try:
                rel = p.relative_to(self.gif_dir)
            except ValueError:
                continue
            category = rel.parts[0] if len(rel.parts) > 1 else None
            items.append((p, category))
        return items

    def _build_ui(self):
        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        outer.get_style_context().add_class("picker-root")

        header = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        header.set_margin_top(14)
        header.set_margin_bottom(10)
        header.set_margin_start(14)
        header.set_margin_end(14)

        close_btn = Gtk.Button(label="\u2715")
        close_btn.get_style_context().add_class("x-btn")
        close_btn.connect("clicked", lambda *_: Gtk.main_quit())
        header.pack_start(close_btn, False, False, 0)

        self.search = Gtk.Entry()
        self.search.set_placeholder_text("Suchen\u2026")
        self.search.set_hexpand(True)
        self.search.connect("changed", self._on_search_changed)
        self.search.connect("activate", self._on_search_activate)
        header.pack_start(self.search, True, True, 0)

        self.count_label = Gtk.Label(label="")
        self.count_label.get_style_context().add_class("picker-hint")
        header.pack_start(self.count_label, False, False, 0)

        outer.pack_start(header, False, False, 0)

        # ---- Setups / Profile ----
        setups = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        setups.set_margin_start(16)
        setups.set_margin_end(16)
        setups.set_margin_bottom(10)
        setups_label = Gtk.Label(label="Setups")
        setups_label.get_style_context().add_class("picker-hint")
        setups.pack_start(setups_label, False, False, 0)

        self.profile_chip_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        setups.pack_start(self.profile_chip_box, False, False, 0)

        save_btn = Gtk.Button(label="\uFF0B")
        save_btn.get_style_context().add_class("chip")
        save_btn.set_tooltip_text("Aktuelles Setup als Profil speichern")
        save_btn.connect("clicked", self._on_save_profile)
        setups.pack_start(save_btn, False, False, 0)

        outer.pack_start(setups, False, False, 0)
        self._rebuild_profile_chips()

        items = self._scan_gifs()
        categories = sorted({c for _p, c in items if c})

        # Kategorie-Chips nur anzeigen, wenn es Unterordner gibt
        if categories:
            chips = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            chips.set_margin_start(16)
            chips.set_margin_end(16)
            chips.set_margin_bottom(10)
            chips.set_halign(Gtk.Align.CENTER)
            group = None
            for label, cat in [("Alle", None)] + [(c, c) for c in categories]:
                btn = Gtk.RadioButton.new_with_label_from_widget(group, label)
                btn.set_mode(False)  # als Button darstellen
                btn.get_style_context().add_class("chip")
                btn.connect("toggled", self._on_chip, cat)
                chips.pack_start(btn, False, False, 0)
                if group is None:
                    group = btn
            outer.pack_start(chips, False, False, 0)

        hint = Gtk.Label(
            label="Klick = Starten  \u2022  Tippen = Filtern  \u2022  "
                  "\u2191\u2193 + Enter = Auswahl  \u2022  Esc = Schliessen"
        )
        hint.get_style_context().add_class("picker-hint")
        hint.set_xalign(0.5)
        hint.set_margin_bottom(8)
        outer.pack_start(hint, False, False, 0)

        outer.pack_start(Gtk.Separator(), False, False, 0)

        self.scroll = Gtk.ScrolledWindow()
        self.scroll.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)

        self.flow = Gtk.FlowBox()
        self.flow.set_valign(Gtk.Align.START)
        self.flow.set_max_children_per_line(self._max_cols)
        self.flow.set_min_children_per_line(min(4, self._max_cols))
        self.flow.set_selection_mode(Gtk.SelectionMode.NONE)
        self.flow.set_homogeneous(True)
        self.flow.set_row_spacing(12)
        self.flow.set_column_spacing(12)
        self.flow.set_margin_start(16)
        self.flow.set_margin_end(16)
        self.flow.set_margin_top(12)
        self.flow.set_margin_bottom(16)
        self.flow.set_filter_func(self._filter_func, None)

        self.tiles = []
        if not items:
            empty = Gtk.Label(
                label=f"Keine GIFs gefunden in:\n{self.gif_dir}\n\n"
                      "Einfach .gif-Dateien dort ablegen!"
            )
            empty.get_style_context().add_class("picker-empty")
            empty.set_margin_top(60)
            empty.set_margin_bottom(60)
            empty.set_justify(Gtk.Justification.CENTER)
            self.flow.add(empty)
        else:
            for gif_path, category in items:
                tile = GifTile(gif_path, category, self._activate_tile,
                               running=(gif_path.stem in self._running))
                self.tiles.append(tile)
                self.flow.add(tile)

            # Verwaiste Cache-Eintraege im Hintergrund aufraeumen
            valid_keys = set()
            for gif_path, _category in items:
                try:
                    valid_keys.add(_cache_key(gif_path))
                except Exception:
                    pass
            EXECUTOR.submit(prune_thumb_cache, valid_keys)

        self.scroll.add(self.flow)
        outer.pack_start(self.scroll, True, True, 0)
        self.add(outer)
        self._refresh_count()

    # ------------------------------------------------------------------
    # Setups / Profile
    # ------------------------------------------------------------------

    def _flash(self, text, restore_ms=2500):
        """Kurze Statusmeldung im Header anzeigen, danach Zaehler wieder."""
        self.count_label.set_text(text)

        def _restore():
            self._refresh_count()
            return False
        GLib.timeout_add(restore_ms, _restore)

    def _rebuild_profile_chips(self):
        for child in self.profile_chip_box.get_children():
            child.destroy()
        for name in sorted(self._profiles.keys(), key=str.lower):
            btn = Gtk.Button(label=name)
            btn.get_style_context().add_class("chip")
            n = len(self._profiles[name].get("widgets", []))
            btn.set_tooltip_text(
                f"{n} GIF(s)  \u2022  Klick: anwenden  \u2022  Rechtsklick: bearbeiten")
            btn.connect("clicked", self._on_profile_clicked, name)
            btn.connect("button-press-event", self._on_profile_button, name)
            self.profile_chip_box.pack_start(btn, False, False, 0)
        self.profile_chip_box.show_all()

    def _on_save_profile(self, *_):
        if not running_ids():
            self._flash("Keine GIFs aktiv \u2013 erst Widgets starten")
            return
        NamePrompt("Setup speichern als \u2026", "", self._do_save_profile)

    def _do_save_profile(self, name):
        widgets = snapshot_setup()
        if not widgets:
            self._flash("Kein Setup erfasst")
            return
        self._profiles[name] = {"widgets": widgets}
        save_profiles(self._profiles)
        self._rebuild_profile_chips()
        self._flash(f"Setup '{name}' gespeichert ({len(widgets)} GIFs)")

    def _on_profile_clicked(self, button, name):
        self._apply_profile(name)

    def _apply_profile(self, name):
        prof = self._profiles.get(name)
        if not prof:
            return
        self._flash(f"Setup '{name}' wird angewendet \u2026", restore_ms=10000)
        EXECUTOR.submit(self._apply_profile_worker, json.loads(json.dumps(prof)))

    def _apply_profile_worker(self, prof):
        try:
            if not ensure_daemon():
                print("Setup: Daemon nicht erreichbar", file=sys.stderr)
                return
            # Atomar im Daemon: schliesst alles und startet die Eintraege
            # mit exakten Werten - keine Spawn-/Warte-Races mehr.
            result = daemon_send(
                {"action": "apply-setup", "widgets": prof.get("widgets", [])},
                timeout=15.0)
            if isinstance(result, dict) and "error" in result:
                print(f"Setup fehlgeschlagen: {result['error']}", file=sys.stderr)
        finally:
            GLib.idle_add(Gtk.main_quit)

    def _on_profile_button(self, button, event, name):
        if event.button != 3:
            return False
        menu = Gtk.Menu()
        actions = [
            ("Anwenden", lambda: self._apply_profile(name)),
            ("Mit aktuellem Setup ueberschreiben",
             lambda: self._overwrite_profile(name)),
            ("Umbenennen \u2026", lambda: self._rename_profile(name)),
            ("Loeschen", lambda: self._delete_profile(name)),
        ]
        for label, cb in actions:
            it = Gtk.MenuItem(label=label)
            it.connect("activate", lambda _w, c=cb: c())
            menu.append(it)
        menu.show_all()
        menu.popup_at_pointer(event)
        return True

    def _overwrite_profile(self, name):
        widgets = snapshot_setup()
        if not widgets:
            self._flash("Keine GIFs aktiv \u2013 nichts zu ueberschreiben")
            return
        self._profiles[name] = {"widgets": widgets}
        save_profiles(self._profiles)
        self._rebuild_profile_chips()
        self._flash(f"Setup '{name}' aktualisiert ({len(widgets)} GIFs)")

    def _rename_profile(self, name):
        def done(new_name):
            if not new_name or new_name == name:
                return
            if new_name in self._profiles:
                self._flash(f"'{new_name}' existiert bereits")
                return
            self._profiles[new_name] = self._profiles.pop(name)
            save_profiles(self._profiles)
            self._rebuild_profile_chips()
        NamePrompt("Setup umbenennen", name, done)

    def _delete_profile(self, name):
        self._profiles.pop(name, None)
        save_profiles(self._profiles)
        self._rebuild_profile_chips()
        self._flash(f"Setup '{name}' geloescht")

    # ------------------------------------------------------------------
    # Filter / Auswahl
    # ------------------------------------------------------------------

    def _on_chip(self, button, category):
        if button.get_active():
            self._active_category = category
            self._set_selected(None)
            self.flow.invalidate_filter()
            self._refresh_count()

    def _on_search_changed(self, entry):
        self._filter_query = entry.get_text().lower().strip()
        self._set_selected(None)
        self.flow.invalidate_filter()
        self._refresh_count()

    def _matches(self, tile) -> bool:
        """Gemeinsames Filter-Praedikat fuer FlowBox, Zaehler und Auswahl."""
        if self._active_category is not None and tile.category != self._active_category:
            return False
        if not self._filter_query:
            return True
        hay = tile.gif_path.stem.lower()
        if tile.category:
            hay += " " + tile.category.lower()
        return self._filter_query in hay

    def _filter_func(self, child, _data):
        tile = child.get_child()
        if not isinstance(tile, GifTile):
            return True
        return self._matches(tile)

    def _visible_tiles(self):
        return [t for t in self.tiles if self._matches(t)]

    def _refresh_count(self):
        if not self.tiles:
            self.count_label.set_text("")
            return
        self.count_label.set_text(f"{len(self._visible_tiles())}/{len(self.tiles)}")

    def _set_selected(self, tile):
        if self._selected is not None:
            self._selected.set_selected(False)
        self._selected = tile
        if tile is not None:
            tile.set_selected(True)
            self._scroll_to(tile)

    def _scroll_to(self, tile):
        parent = tile.get_parent()
        if parent is None:
            return
        alloc = parent.get_allocation()
        adj = self.scroll.get_vadjustment()
        if alloc.y < adj.get_value():
            adj.set_value(max(adj.get_lower(), alloc.y - 12))
        elif alloc.y + alloc.height > adj.get_value() + adj.get_page_size():
            adj.set_value(min(adj.get_upper(),
                              alloc.y + alloc.height - adj.get_page_size() + 12))

    def _move_selection(self, delta):
        visible = self._visible_tiles()
        if not visible:
            return
        if self._selected not in visible:
            self._set_selected(visible[0])
            return
        idx = visible.index(self._selected) + delta
        idx = max(0, min(idx, len(visible) - 1))
        self._set_selected(visible[idx])

    def _on_search_activate(self, entry):
        if self._selected is not None:
            self._activate_tile(self._selected)
            return
        visible = self._visible_tiles()
        if visible:
            self._activate_tile(visible[0])

    # ------------------------------------------------------------------
    # Start
    # ------------------------------------------------------------------

    def _activate_tile(self, tile: GifTile):
        # Immer eine (weitere) Instanz starten - mehrere Kopien desselben
        # GIFs sind erlaubt; die Instanz-ID vergibt das Script automatisch
        # (name, name-2, name-3, ...).
        if ensure_daemon():
            result = daemon_send({"action": "spawn", "gif": str(tile.gif_path)},
                                 timeout=5.0)
            if isinstance(result, dict) and "error" in result:
                print(f"Launch failed: {result['error']}", file=sys.stderr)
        else:
            print("Launch failed: Daemon nicht erreichbar", file=sys.stderr)
        Gtk.main_quit()

    def _on_key(self, widget, event):
        if event.keyval == Gdk.KEY_Escape:
            Gtk.main_quit()
            return True
        if event.keyval == Gdk.KEY_Down:
            self._move_selection(1)
            return True
        if event.keyval == Gdk.KEY_Up:
            self._move_selection(-1)
            return True
        return False


def main():
    global UI_SCALE, THUMB_PX
    GIF_DIR.mkdir(parents=True, exist_ok=True)
    UI_SCALE = _detect_ui_scale()
    THUMB_PX = THUMB_SIZE * UI_SCALE
    PickerWindow(GIF_DIR)
    try:
        Gtk.main()
    except KeyboardInterrupt:
        pass
    finally:
        # Ausstehende Thumbnail-Decodes verwerfen, sonst haengt der Prozess
        # beim Beenden, bis der Pool alles abgearbeitet hat.
        try:
            EXECUTOR.shutdown(wait=False, cancel_futures=True)
        except TypeError:  # Python < 3.9
            EXECUTOR.shutdown(wait=False)


if __name__ == "__main__":
    main()
