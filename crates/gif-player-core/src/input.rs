use crate::model::PlayerState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputRegionMode {
    Empty,
    Gif(Rect),
    FullSurface { width: u32, height: u32 },
}

pub fn input_region_mode(
    state: &PlayerState,
    canvas_mode: bool,
    dragging: bool,
    gif_width: u32,
    gif_height: u32,
    surface_width: u32,
    surface_height: u32,
    hop_offset: f64,
    padding: i32,
) -> InputRegionMode {
    if state.locked {
        return InputRegionMode::Empty;
    }
    if dragging {
        return InputRegionMode::FullSurface {
            width: surface_width.max(1),
            height: surface_height.max(1),
        };
    }

    let (origin_x, origin_y) = if canvas_mode {
        (
            state.x.floor() as i32 - padding,
            (state.y - hop_offset).floor() as i32 - padding,
        )
    } else {
        (-padding, -padding)
    };
    let pad = padding.max(0) as u32;
    InputRegionMode::Gif(Rect {
        x: origin_x,
        y: origin_y,
        width: gif_width.saturating_add(pad.saturating_mul(2)).max(1),
        height: gif_height.saturating_add(pad.saturating_mul(2)).max(1),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_is_always_empty_even_while_drag_flag_is_set() {
        let state = PlayerState::default();
        assert_eq!(
            input_region_mode(&state, true, true, 100, 100, 1920, 1080, 0.0, 8),
            InputRegionMode::Empty
        );
    }

    #[test]
    fn unlocked_drag_uses_full_surface() {
        let state = PlayerState { locked: false, ..PlayerState::default() };
        assert_eq!(
            input_region_mode(&state, true, true, 100, 100, 1920, 1080, 0.0, 8),
            InputRegionMode::FullSurface { width: 1920, height: 1080 }
        );
    }

    #[test]
    fn unlocked_canvas_exposes_only_gif_rect() {
        let state = PlayerState { x: 100.0, y: 200.0, locked: false, ..PlayerState::default() };
        assert_eq!(
            input_region_mode(&state, true, false, 80, 40, 1920, 1080, 10.0, 8),
            InputRegionMode::Gif(Rect { x: 92, y: 182, width: 96, height: 56 })
        );
    }
}
