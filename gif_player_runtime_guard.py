#!/usr/bin/env python3
"""Final allocation guards layered over the compositor runtime patches."""

from __future__ import annotations

from typing import Any


def install_transition_guards(module: Any) -> None:
    """Prevent stale compact allocations from completing a canvas transition."""

    cls = module.WidgetWindow
    if getattr(cls, "_gif_player_transition_guarded", False):
        return

    patched_size_allocate = cls._on_size_allocate
    original_first_frame = cls._on_first_frame

    def guarded_size_allocate(self: Any, widget: Any, allocation: Any) -> None:
        if self._surface_phase == "to-canvas":
            canvas_ready = (
                allocation.width >= int(self.mon_w * 0.7)
                and allocation.height >= int(self.mon_h * 0.7)
            )
            if not canvas_ready:
                # GTK can emit the old GIF-sized allocation after anchors have
                # changed. Completing on that notification would draw the GIF
                # at its canvas offset into the old tiny buffer and recreate the
                # blank transition frame that this work removes.
                self._update_input_region()
                return
        patched_size_allocate(self, widget, allocation)

    def guarded_first_frame(self: Any) -> bool:
        result = original_first_frame(self)
        if not self._closed and self._first_frame_ready:
            # Before decoding, the provisional store size is 1×1. Re-evaluate
            # after the real dimensions are known so an image whose top-left is
            # inside but whose rectangle crosses an edge uses locked canvas.
            self._apply_surface_mode()
        return result

    cls._on_size_allocate = guarded_size_allocate
    cls._on_first_frame = guarded_first_frame
    cls._gif_player_transition_guarded = True
