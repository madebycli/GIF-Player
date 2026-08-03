from __future__ import annotations

import math
import unittest

from gif_player_runtime import (
    advance_frame_timeline,
    bounce_axis,
    bounce_start_position,
    bounce_step,
    is_fully_inside,
    jump_offset,
    manual_position,
    normalize_frame_duration,
)


class RuntimeLogicTests(unittest.TestCase):
    def test_jump_curve_has_exact_endpoints_and_peak(self):
        self.assertEqual(jump_offset(0.0, 60.0), 0.0)
        self.assertEqual(jump_offset(1.0, 60.0), 0.0)
        self.assertAlmostEqual(jump_offset(0.5, 60.0), 60.0)

    def test_jump_curve_is_continuous_at_first_tick(self):
        first = jump_offset(0.0, 60.0)
        second = jump_offset(1.0 / 60.0 / 0.65, 60.0)
        self.assertGreater(second, first)
        self.assertLess(second, 6.0)

    def test_manual_positions_are_not_clamped(self):
        self.assertEqual(manual_position(-400, 2200), (-400.0, 2200.0))
        self.assertEqual(manual_position(5000, -900), (5000.0, -900.0))

    def test_inside_detection_drives_locked_canvas_fallback(self):
        self.assertTrue(is_fully_inside(10, 10, 100, 100, 1920, 1080))
        self.assertFalse(is_fully_inside(-1, 10, 100, 100, 1920, 1080))
        self.assertFalse(is_fully_inside(1900, 10, 100, 100, 1920, 1080))
        self.assertFalse(is_fully_inside(0, 0, 2200, 100, 1920, 1080))

    def test_bounce_reflects_at_both_edges(self):
        left = bounce_axis(5, -100, 0.1, 100, 10)
        self.assertAlmostEqual(left.position, 5.0)
        self.assertGreater(left.velocity, 0.0)
        right = bounce_axis(85, 100, 0.1, 100, 10)
        self.assertAlmostEqual(right.position, 85.0)
        self.assertLess(right.velocity, 0.0)

    def test_bounce_handles_large_dt_without_escape(self):
        axis = bounce_axis(20, 360, 10.0, 1920, 200)
        self.assertGreaterEqual(axis.position, 0.0)
        self.assertLessEqual(axis.position, 1720.0)
        self.assertTrue(math.isfinite(axis.velocity))

    def test_oversized_bounce_axis_centers_and_freezes(self):
        axis = bounce_axis(500, 360, 0.016, 100, 180)
        self.assertEqual(axis.position, -40.0)
        self.assertEqual(axis.velocity, 0.0)
        self.assertTrue(axis.oversized)

    def test_oversized_one_axis_keeps_other_axis_moving(self):
        step = bounce_step(0, 10, 100, 100, 0.1, 100, 500, 180, 50)
        self.assertTrue(step.oversized_x)
        self.assertFalse(step.oversized_y)
        self.assertEqual(step.vx, 0.0)
        self.assertNotEqual(step.vy, 0.0)

    def test_bounce_start_clamps_offscreen_without_affecting_manual_helper(self):
        x, y, over_x, over_y = bounce_start_position(-500, 5000, 1920, 1080, 200, 200)
        self.assertEqual((x, y), (0.0, 880.0))
        self.assertFalse(over_x)
        self.assertFalse(over_y)
        self.assertEqual(manual_position(-500, 5000), (-500.0, 5000.0))

    def test_frame_duration_preserves_fast_valid_delays(self):
        self.assertEqual(normalize_frame_duration(None), 100)
        self.assertEqual(normalize_frame_duration(0), 100)
        self.assertEqual(normalize_frame_duration(10), 20)
        self.assertEqual(normalize_frame_duration(20), 20)
        self.assertEqual(normalize_frame_duration(80), 80)

    def test_absolute_timeline_has_no_drift_under_normal_load(self):
        result = advance_frame_timeline(0, 10.100, 10.100, [100, 100, 100], 1.0)
        self.assertEqual(result.index, 1)
        self.assertAlmostEqual(result.deadline, 10.200)
        self.assertEqual(result.skipped, 0)
        self.assertFalse(result.rebased)

    def test_late_timeline_skips_overdue_frames_without_burst_draws(self):
        result = advance_frame_timeline(0, 10.100, 10.350, [100, 100, 100, 100], 1.0)
        self.assertEqual(result.index, 3)
        self.assertEqual(result.advanced, 3)
        self.assertEqual(result.skipped, 2)
        self.assertGreater(result.deadline, 10.350)

    def test_extreme_stall_rebases_after_catchup_cap(self):
        result = advance_frame_timeline(
            0,
            1.0,
            100.0,
            [20, 20, 20],
            1.0,
            max_catchup=4,
        )
        self.assertTrue(result.rebased)
        self.assertGreater(result.deadline, 100.0)


if __name__ == "__main__":
    unittest.main()
