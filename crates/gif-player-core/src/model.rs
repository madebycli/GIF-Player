use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

pub const DEFAULT_X: f64 = 100.0;
pub const DEFAULT_Y: f64 = 100.0;
pub const DEFAULT_SCALE: f64 = 0.7;
pub const DEFAULT_OPACITY: f64 = 1.0;
pub const DEFAULT_SPEED: f64 = 1.0;
pub const DEFAULT_JUMP_RATE: f64 = 6.0;
pub const EDGE_SNAP: f64 = 20.0;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PlayerState {
    pub x: f64,
    pub y: f64,
    pub scale: f64,
    pub locked: bool,
    pub paused: bool,
    pub opacity: f64,
    pub flip_h: bool,
    pub flip_v: bool,
    pub speed: f64,
    pub bouncing: bool,
    pub jumping: bool,
    pub jump_rate: f64,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            x: DEFAULT_X,
            y: DEFAULT_Y,
            scale: DEFAULT_SCALE,
            locked: true,
            paused: false,
            opacity: DEFAULT_OPACITY,
            flip_h: false,
            flip_v: false,
            speed: DEFAULT_SPEED,
            bouncing: false,
            jumping: false,
            jump_rate: DEFAULT_JUMP_RATE,
        }
    }
}

impl PlayerState {
    pub fn normalize(&mut self) {
        if !self.x.is_finite() {
            self.x = DEFAULT_X;
        }
        if !self.y.is_finite() {
            self.y = DEFAULT_Y;
        }
        self.scale = finite_clamp(self.scale, 0.1, 5.0, DEFAULT_SCALE);
        self.opacity = finite_clamp(self.opacity, 0.05, 1.0, DEFAULT_OPACITY);
        self.speed = finite_clamp(self.speed, 0.1, 10.0, DEFAULT_SPEED);
        self.jump_rate = finite_clamp(self.jump_rate, 0.5, 60.0, DEFAULT_JUMP_RATE);
    }
}

pub fn finite_clamp(value: f64, min: f64, max: f64, fallback: f64) -> f64 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

pub fn scaled_size(source_width: u32, source_height: u32, scale: f64) -> (u32, u32) {
    let width = ((source_width as f64 * scale).round() as i64).max(1) as u32;
    let height = ((source_height as f64 * scale).round() as i64).max(1) as u32;
    (width, height)
}

pub fn fully_inside(x: f64, y: f64, width: f64, height: f64, bounds_w: f64, bounds_h: f64) -> bool {
    [x, y, width, height, bounds_w, bounds_h]
        .iter()
        .all(|value| value.is_finite())
        && x >= 0.0
        && y >= 0.0
        && width <= bounds_w
        && height <= bounds_h
        && x + width <= bounds_w
        && y + height <= bounds_h
}

pub fn snap_axis(value: f64, size: f64, bound: f64) -> f64 {
    let maximum = bound - size;
    if (0.0..=EDGE_SNAP).contains(&value) {
        return 0.0;
    }
    if maximum >= 0.0 && value >= maximum - EDGE_SNAP && value <= maximum {
        return maximum;
    }
    let center = maximum / 2.0;
    if value >= 0.0 && value <= maximum && (value - center).abs() <= EDGE_SNAP {
        return center;
    }
    value
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BounceAxis {
    pub position: f64,
    pub velocity: f64,
    pub oversized: bool,
}

pub fn bounce_axis(
    position: f64,
    velocity: f64,
    dt: f64,
    bound: f64,
    item_size: f64,
) -> BounceAxis {
    let position = if position.is_finite() { position } else { 0.0 };
    let velocity = if velocity.is_finite() { velocity } else { 0.0 };
    let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
    let bound = if bound.is_finite() {
        bound.max(0.0)
    } else {
        0.0
    };
    let item_size = if item_size.is_finite() {
        item_size.max(0.0)
    } else {
        0.0
    };
    let extent = bound - item_size;
    if extent <= 0.0 {
        return BounceAxis {
            position: extent / 2.0,
            velocity: 0.0,
            oversized: true,
        };
    }
    if velocity == 0.0 || dt == 0.0 {
        return BounceAxis {
            position: position.clamp(0.0, extent),
            velocity,
            oversized: false,
        };
    }

    let raw = position + velocity * dt;
    let period = 2.0 * extent;
    let mut phase = raw % period;
    if phase < 0.0 {
        phase += period;
    }
    if phase <= extent {
        BounceAxis {
            position: phase,
            velocity,
            oversized: false,
        }
    } else {
        BounceAxis {
            position: period - phase,
            velocity: -velocity,
            oversized: false,
        }
    }
}

pub fn jump_offset(progress: f64, height: f64) -> f64 {
    if !progress.is_finite() || !height.is_finite() || progress <= 0.0 || progress >= 1.0 {
        0.0
    } else {
        (PI * progress.clamp(0.0, 1.0)).sin() * height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_keeps_free_finite_positions() {
        let mut state = PlayerState {
            x: -900.0,
            y: 4000.0,
            ..PlayerState::default()
        };
        state.normalize();
        assert_eq!(state.x, -900.0);
        assert_eq!(state.y, 4000.0);
    }

    #[test]
    fn snap_keeps_true_offscreen_positions() {
        assert_eq!(snap_axis(-50.0, 100.0, 1920.0), -50.0);
        assert_eq!(snap_axis(2000.0, 100.0, 1920.0), 2000.0);
    }

    #[test]
    fn oversized_bounce_centers_axis() {
        let axis = bounce_axis(10.0, 200.0, 0.1, 100.0, 140.0);
        assert!(axis.oversized);
        assert_eq!(axis.position, -20.0);
        assert_eq!(axis.velocity, 0.0);
    }

    #[test]
    fn hop_returns_to_zero() {
        assert_eq!(jump_offset(0.0, 60.0), 0.0);
        assert_eq!(jump_offset(1.0, 60.0), 0.0);
        assert!(jump_offset(0.5, 60.0) > 59.0);
    }
}
