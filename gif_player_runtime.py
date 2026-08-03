#!/usr/bin/env python3
"""Pure player timing, motion, GIF decode and Cairo conversion helpers.

This module intentionally avoids importing GTK. Geometry, frame pacing, GIF
metadata normalization and pixel conversion therefore remain unit-testable
without a Wayland compositor.
"""

from __future__ import annotations

from dataclasses import dataclass
import math
from typing import Any, Iterator

DEFAULT_FRAME_DURATION_MS = 100
MIN_FRAME_DURATION_MS = 20
MAX_CATCHUP_FRAMES = 8
TRANSITION_FALLBACK_MS = 120


def normalize_frame_duration(value: Any) -> int:
    """Return a practical GIF frame duration while preserving fast animation."""

    try:
        duration = int(value)
    except (TypeError, ValueError, OverflowError):
        return DEFAULT_FRAME_DURATION_MS
    if duration <= 0:
        return DEFAULT_FRAME_DURATION_MS
    return max(MIN_FRAME_DURATION_MS, duration)


def iter_composited_frames(image: Any) -> Iterator[tuple[Any, int]]:
    """Yield independent, fully composited RGBA frames and durations.

    Pillow's GIF ``seek`` applies local palettes, transparency, delta-frame
    composition and disposal handling to the logical canvas. Copying the RGBA
    result prevents later seek operations from mutating an already stored frame.
    """

    total = max(1, int(getattr(image, "n_frames", 1)))
    for index in range(total):
        image.seek(index)
        rgba = image.convert("RGBA").copy()
        duration = normalize_frame_duration(image.info.get("duration"))
        yield rgba, duration


def premultiplied_bgra_bytes(rgba: Any) -> bytes:
    """Convert Pillow RGBA to Cairo ARGB32 bytes on little-endian Linux."""

    premultiplied = rgba.convert("RGBa")
    return premultiplied.tobytes("raw", "BGRa")


def cairo_surface_from_rgba(rgba: Any, cairo_module: Any) -> tuple[Any, bytearray]:
    """Create a Cairo surface and return its explicitly retained buffer."""

    buffer = bytearray(premultiplied_bgra_bytes(rgba))
    surface = cairo_module.ImageSurface.create_for_data(
        buffer,
        cairo_module.FORMAT_ARGB32,
        rgba.width,
        rgba.height,
        rgba.width * 4,
    )
    return surface, buffer


def jump_offset(progress: float, height: float) -> float:
    """Continuous half-sine hop with exact zero at both endpoints."""

    if not math.isfinite(progress) or not math.isfinite(height):
        return 0.0
    progress = min(1.0, max(0.0, float(progress)))
    if progress <= 0.0 or progress >= 1.0:
        return 0.0
    return math.sin(math.pi * progress) * float(height)


def is_fully_inside(
    x: float,
    y: float,
    width: float,
    height: float,
    bounds_width: float,
    bounds_height: float,
) -> bool:
    """Return whether a rectangle is completely inside monitor-local bounds."""

    values = (x, y, width, height, bounds_width, bounds_height)
    if not all(math.isfinite(float(value)) for value in values):
        return False
    return (
        x >= 0.0
        and y >= 0.0
        and width <= bounds_width
        and height <= bounds_height
        and x + width <= bounds_width
        and y + height <= bounds_height
    )


def manual_position(x: float, y: float) -> tuple[float, float]:
    """Normalize a manual position without applying monitor clamps."""

    x = float(x)
    y = float(y)
    if not math.isfinite(x) or not math.isfinite(y):
        raise ValueError("position must be finite")
    return x, y


@dataclass(frozen=True)
class BounceAxis:
    position: float
    velocity: float
    oversized: bool = False


def bounce_axis(
    position: float,
    velocity: float,
    dt: float,
    bound: float,
    item_size: float,
) -> BounceAxis:
    """Advance one bounce axis with reflection and an oversized fallback.

    If the item cannot fit on an axis, no valid reflection interval exists. The
    stable fallback centers the item on that axis and freezes that component of
    velocity. The other axis can continue bouncing normally.
    """

    position = float(position)
    velocity = float(velocity)
    dt = max(0.0, float(dt))
    bound = max(0.0, float(bound))
    item_size = max(0.0, float(item_size))
    values = (position, velocity, dt, bound, item_size)
    if not all(math.isfinite(value) for value in values):
        return BounceAxis(0.0, 0.0, True)

    extent = bound - item_size
    if extent <= 0.0:
        return BounceAxis(extent / 2.0, 0.0, True)
    if velocity == 0.0 or dt == 0.0:
        return BounceAxis(min(extent, max(0.0, position)), velocity, False)

    raw = position + velocity * dt
    period = 2.0 * extent
    phase = raw % period
    if phase <= extent:
        reflected = phase
        slope = 1.0
    else:
        reflected = period - phase
        slope = -1.0
    return BounceAxis(reflected, velocity * slope, False)


@dataclass(frozen=True)
class BounceStep:
    x: float
    y: float
    vx: float
    vy: float
    oversized_x: bool
    oversized_y: bool


def bounce_step(
    x: float,
    y: float,
    vx: float,
    vy: float,
    dt: float,
    bounds_width: float,
    bounds_height: float,
    item_width: float,
    item_height: float,
) -> BounceStep:
    horizontal = bounce_axis(x, vx, dt, bounds_width, item_width)
    vertical = bounce_axis(y, vy, dt, bounds_height, item_height)
    return BounceStep(
        horizontal.position,
        vertical.position,
        horizontal.velocity,
        vertical.velocity,
        horizontal.oversized,
        vertical.oversized,
    )


def bounce_start_position(
    x: float,
    y: float,
    bounds_width: float,
    bounds_height: float,
    item_width: float,
    item_height: float,
) -> tuple[float, float, bool, bool]:
    horizontal = bounce_axis(x, 0.0, 0.0, bounds_width, item_width)
    vertical = bounce_axis(y, 0.0, 0.0, bounds_height, item_height)
    return horizontal.position, vertical.position, horizontal.oversized, vertical.oversized


@dataclass(frozen=True)
class FrameAdvance:
    index: int
    deadline: float
    advanced: int
    skipped: int
    rebased: bool


def frame_delay_seconds(duration_ms: int, speed: float) -> float:
    speed = float(speed)
    if not math.isfinite(speed) or speed <= 0.0:
        return 9.999
    return max(0.001, normalize_frame_duration(duration_ms) / 1000.0 / speed)


def advance_frame_timeline(
    index: int,
    deadline: float,
    now: float,
    durations: list[int] | tuple[int, ...],
    speed: float,
    *,
    max_catchup: int = MAX_CATCHUP_FRAMES,
) -> FrameAdvance:
    """Advance an absolute GIF timeline and draw only the newest due frame.

    Under normal load exactly one frame advances. If the main loop was late,
    overdue frames remain accounted for against their original deadlines while
    only the final due frame is rendered. Extreme stalls rebase once instead of
    causing a burst of zero-delay callbacks.
    """

    count = len(durations)
    if count <= 1:
        return FrameAdvance(max(0, min(index, count - 1)), deadline, 0, 0, False)

    index = int(index) % count
    deadline = float(deadline)
    now = float(now)
    advanced = 0
    limit = max(1, int(max_catchup))
    while deadline <= now and advanced < limit:
        index = (index + 1) % count
        deadline += frame_delay_seconds(durations[index], speed)
        advanced += 1

    rebased = deadline <= now
    if rebased:
        deadline = now + frame_delay_seconds(durations[index], speed)
    return FrameAdvance(index, deadline, advanced, max(0, advanced - 1), rebased)
