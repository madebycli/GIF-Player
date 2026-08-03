from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

import cairo
from PIL import Image, ImageDraw

from gif_player_runtime import (
    cairo_surface_from_rgba,
    iter_composited_frames,
    jump_offset,
    premultiplied_bgra_bytes,
)


class GifDecodeTests(unittest.TestCase):
    @staticmethod
    def _frame(color: str, box: tuple[int, int, int, int]) -> Image.Image:
        image = Image.new("RGBA", (10, 10), (0, 0, 0, 0))
        ImageDraw.Draw(image).rectangle(box, fill=color)
        return image

    def _write_disposal_gif(self, path: Path, disposal: int) -> None:
        frames = [
            self._frame("red", (0, 0, 3, 3)),
            self._frame("blue", (5, 0, 8, 3)),
            self._frame("green", (2, 5, 5, 8)),
        ]
        frames[0].save(
            path,
            save_all=True,
            append_images=frames[1:],
            duration=[10, 40, 250],
            loop=0,
            disposal=[1, disposal, 1],
            transparency=0,
            optimize=True,
        )

    def test_disposal_two_clears_previous_delta_region(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "dispose-2.gif"
            self._write_disposal_gif(path, 2)
            with Image.open(path) as image:
                frames = list(iter_composited_frames(image))
        self.assertEqual(len(frames), 3)
        self.assertEqual(frames[0][1], 20)
        self.assertEqual(frames[1][1], 40)
        self.assertEqual(frames[2][1], 250)
        third = frames[2][0]
        self.assertEqual(third.getpixel((1, 1))[3], 0)
        self.assertEqual(third.getpixel((6, 1))[3], 0)
        self.assertGreater(third.getpixel((3, 6))[3], 0)

    def test_disposal_three_restores_previous_composite(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "dispose-3.gif"
            self._write_disposal_gif(path, 3)
            with Image.open(path) as image:
                frames = list(iter_composited_frames(image))
        third = frames[2][0]
        self.assertGreater(third.getpixel((1, 1))[3], 0)
        self.assertEqual(third.getpixel((6, 1))[3], 0)
        self.assertGreater(third.getpixel((3, 6))[3], 0)

    def test_local_palette_and_transparency_are_materialized_per_frame(self):
        first = Image.new("P", (4, 4), 0)
        first.putpalette([0, 0, 0, 255, 0, 0] + [0, 0, 0] * 254)
        ImageDraw.Draw(first).rectangle((0, 0, 1, 1), fill=1)
        second = Image.new("P", (4, 4), 0)
        second.putpalette([0, 0, 0, 0, 255, 0] + [0, 0, 0] * 254)
        ImageDraw.Draw(second).rectangle((2, 2, 3, 3), fill=1)
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "palettes.gif"
            first.save(
                path,
                save_all=True,
                append_images=[second],
                duration=[100, 100],
                transparency=0,
                disposal=[1, 1],
                optimize=False,
            )
            with Image.open(path) as image:
                frames = list(iter_composited_frames(image))
        self.assertEqual(frames[0][0].getpixel((0, 0)), (255, 0, 0, 255))
        self.assertEqual(frames[1][0].getpixel((2, 2)), (0, 255, 0, 255))

    def test_corrupt_tail_keeps_decoded_prefix_available(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "truncated.gif"
            self._write_disposal_gif(path, 1)
            data = path.read_bytes()
            path.write_bytes(data[:-8])
            decoded = []
            with Image.open(path) as image:
                try:
                    for frame in iter_composited_frames(image):
                        decoded.append(frame)
                except Exception:
                    # Pillow releases differ in how eagerly they reject a
                    # missing trailer or truncated final data block. The player
                    # contract is that an already yielded valid prefix remains
                    # usable in either case.
                    pass
        self.assertGreaterEqual(len(decoded), 1)

    def test_cairo_bytes_are_premultiplied_bgra(self):
        image = Image.new("RGBA", (1, 1), (200, 100, 50, 128))
        blue, green, red, alpha = premultiplied_bgra_bytes(image)
        self.assertEqual(alpha, 128)
        self.assertLessEqual(abs(red - 100), 1)
        self.assertLessEqual(abs(green - 50), 1)
        self.assertLessEqual(abs(blue - 25), 1)

    def test_first_jump_render_is_pixel_identical(self):
        source = Image.new("RGBA", (3, 3), (255, 0, 0, 255))
        surface, backing = cairo_surface_from_rgba(source, cairo)
        self.assertEqual(len(backing), 36)

        def render(offset: float) -> bytes:
            target = cairo.ImageSurface(cairo.FORMAT_ARGB32, 12, 12)
            context = cairo.Context(target)
            context.set_operator(cairo.OPERATOR_SOURCE)
            context.set_source_rgba(0, 0, 0, 0)
            context.paint()
            context.set_source_surface(surface, 4, 5 - offset)
            context.paint()
            target.flush()
            return bytes(target.get_data())

        before = render(0.0)
        first_jump = render(jump_offset(0.0, 60.0))
        self.assertEqual(before, first_jump)
        self.assertGreater(sum(before[3::4]), 0)


if __name__ == "__main__":
    unittest.main()
