use crate::model::PlayerState;
use anyhow::{anyhow, Result};
use smithay_client_toolkit::compositor::{CompositorState, Region};
use wayland_client::protocol::wl_surface::WlSurface;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputGeometry {
    pub canvas_mode: bool,
    pub dragging: bool,
    pub gif_width: u32,
    pub gif_height: u32,
    pub surface_width: u32,
    pub surface_height: u32,
    pub hop_offset: f64,
    pub padding: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputRegionMode {
    Empty,
    Gif(Rect),
    FullSurface { width: u32, height: u32 },
}

pub fn input_region_mode(state: &PlayerState, geometry: InputGeometry) -> InputRegionMode {
    if state.locked {
        return InputRegionMode::Empty;
    }
    if geometry.dragging {
        return InputRegionMode::FullSurface {
            width: geometry.surface_width.max(1),
            height: geometry.surface_height.max(1),
        };
    }

    let (origin_x, origin_y) = if geometry.canvas_mode {
        (
            state.x.floor() as i32 - geometry.padding,
            (state.y - geometry.hop_offset).floor() as i32 - geometry.padding,
        )
    } else {
        (-geometry.padding, -geometry.padding)
    };
    let pad = geometry.padding.max(0) as u32;
    InputRegionMode::Gif(Rect {
        x: origin_x,
        y: origin_y,
        width: geometry
            .gif_width
            .saturating_add(pad.saturating_mul(2))
            .max(1),
        height: geometry
            .gif_height
            .saturating_add(pad.saturating_mul(2))
            .max(1),
    })
}

/// Apply a wl_surface input region without requiring application code to dispatch
/// wl_region objects itself. `InputRegionMode::Empty` intentionally adds no
/// rectangles, which makes the committed surface fully pointer/touch transparent.
pub fn apply_input_region(
    compositor: &CompositorState,
    surface: &WlSurface,
    mode: InputRegionMode,
) -> Result<()> {
    let region = Region::new(compositor)
        .map_err(|error| anyhow!("create Wayland input region: {error:?}"))?;
    match mode {
        InputRegionMode::Empty => {}
        InputRegionMode::Gif(rect) => region.add(
            rect.x,
            rect.y,
            rect.width.min(i32::MAX as u32) as i32,
            rect.height.min(i32::MAX as u32) as i32,
        ),
        InputRegionMode::FullSurface { width, height } => region.add(
            0,
            0,
            width.min(i32::MAX as u32) as i32,
            height.min(i32::MAX as u32) as i32,
        ),
    }
    surface.set_input_region(Some(region.wl_region()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geometry() -> InputGeometry {
        InputGeometry {
            canvas_mode: true,
            dragging: false,
            gif_width: 100,
            gif_height: 100,
            surface_width: 1920,
            surface_height: 1080,
            hop_offset: 0.0,
            padding: 8,
        }
    }

    #[test]
    fn locked_is_always_empty_even_while_drag_flag_is_set() {
        let state = PlayerState::default();
        let mut geometry = geometry();
        geometry.dragging = true;
        assert_eq!(input_region_mode(&state, geometry), InputRegionMode::Empty);
    }

    #[test]
    fn unlocked_drag_uses_full_surface() {
        let state = PlayerState {
            locked: false,
            ..PlayerState::default()
        };
        let mut geometry = geometry();
        geometry.dragging = true;
        assert_eq!(
            input_region_mode(&state, geometry),
            InputRegionMode::FullSurface {
                width: 1920,
                height: 1080
            }
        );
    }

    #[test]
    fn unlocked_canvas_exposes_only_gif_rect() {
        let state = PlayerState {
            x: 100.0,
            y: 200.0,
            locked: false,
            ..PlayerState::default()
        };
        let geometry = InputGeometry {
            gif_width: 80,
            gif_height: 40,
            hop_offset: 10.0,
            ..geometry()
        };
        assert_eq!(
            input_region_mode(&state, geometry),
            InputRegionMode::Gif(Rect {
                x: 92,
                y: 182,
                width: 96,
                height: 56
            })
        );
    }
}
