//! Colour roles and spacing/typography tokens shared with the real Noctalia 5 design system.
//!
//! The role names and the numeric tokens mirror Noctalia 5.2.1 (`ui/palette.h`, `ui/style.h`), so
//! the standalone Rust interface renders with the same chrome as the host's native components:
//! 38 px controls, radii of 3/6/9/12 px, 1 px outlines, a 2 px `secondary` focus ring and
//! `hover`/`on_hover` for pointer feedback. Colours come from the live host palette.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tiny_skia::Color;

pub const RADIUS_SM: f32 = 3.0;
pub const RADIUS_MD: f32 = 6.0;
pub const RADIUS_LG: f32 = 9.0;
pub const RADIUS_XL: f32 = 12.0;
pub const SPACE_XS: i32 = 4;
pub const SPACE_SM: i32 = 8;
pub const SPACE_MD: i32 = 12;
pub const SPACE_LG: i32 = 16;
pub const CARD_PADDING: i32 = 14;
pub const CONTROL_SM: i32 = 32;
pub const CONTROL: i32 = 38;
pub const FONT_MINI: f32 = 11.0;
pub const FONT_CAPTION: f32 = 13.0;
pub const FONT_BODY: f32 = 14.0;
pub const FONT_TITLE: f32 = 16.0;
pub const FONT_HEADER: f32 = 20.0;
pub const FOCUS_RING: f32 = 2.0;
pub const DISABLED_ALPHA: f32 = 0.55;
pub const PLACEHOLDER_ALPHA: f32 = 0.68;
pub const SLIDER_TRACK: i32 = 8;
pub const SLIDER_THUMB: i32 = 18;
pub const TOGGLE_W: i32 = 40;
pub const TOGGLE_H: i32 = 24;

/// Effective settings supplied by Noctalia, retained with its palette for offline use.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HostStyle {
    pub corner_radius_scale: f32,
    pub button_borders: bool,
    pub input_borders: bool,
    pub panel_borders: bool,
    pub panel_border_width: f32,
    pub panel_opacity: f32,
    pub card_opacity: f32,
    pub window_translucent: bool,
    pub show_close_buttons: bool,
    pub ui_scale: f32,
}
impl Default for HostStyle {
    fn default() -> Self {
        Self {
            corner_radius_scale: 1.0,
            button_borders: true,
            input_borders: true,
            panel_borders: true,
            panel_border_width: 1.0,
            panel_opacity: 1.0,
            card_opacity: 1.0,
            window_translucent: false,
            show_close_buttons: true,
            ui_scale: 1.0,
        }
    }
}
impl HostStyle {
    pub fn parse(value: Option<&Value>) -> anyhow::Result<Self> {
        let style: Self = match value {
            Some(v) => serde_json::from_value(v.clone())?,
            None => Self::default(),
        };
        anyhow::ensure!(
            style.corner_radius_scale.is_finite()
                && (0.0..=2.0).contains(&style.corner_radius_scale),
            "Invalid corner radius scale"
        );
        anyhow::ensure!(
            style.ui_scale.is_finite() && (0.1..=4.0).contains(&style.ui_scale),
            "Invalid UI scale"
        );
        anyhow::ensure!(
            style.panel_border_width.is_finite()
                && (0.0..=20.0).contains(&style.panel_border_width),
            "Invalid panel border width"
        );
        anyhow::ensure!(
            [style.panel_opacity, style.card_opacity]
                .iter()
                .all(|n| n.is_finite() && (0.0..=1.0).contains(n)),
            "Invalid theme opacity"
        );
        Ok(style)
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct Theme {
    pub style: HostStyle,
    pub panel_border: Color,
    pub surface: Color,
    pub variant: Color,
    pub on_surface: Color,
    pub on_variant: Color,
    pub primary: Color,
    pub on_primary: Color,
    pub secondary: Color,
    pub on_secondary: Color,
    pub error: Color,
    pub on_error: Color,
    pub outline: Color,
    pub shadow: Color,
    pub hover: Color,
    pub on_hover: Color,
}
fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba8(r, g, b, 255)
}
impl Default for Theme {
    /// Dark lavender fallback used only before a Noctalia host has ever supplied a palette.
    fn default() -> Self {
        Self {
            style: HostStyle::default(),
            panel_border: rgb(72, 69, 80),
            surface: rgb(18, 18, 23),
            variant: rgb(32, 31, 36),
            on_surface: rgb(224, 222, 230),
            on_variant: rgb(189, 186, 198),
            primary: rgb(204, 185, 255),
            on_primary: rgb(46, 32, 77),
            secondary: rgb(190, 176, 235),
            on_secondary: rgb(42, 31, 69),
            error: rgb(255, 180, 171),
            on_error: rgb(105, 0, 5),
            outline: rgb(72, 69, 80),
            shadow: rgb(0, 0, 0),
            hover: rgb(190, 170, 240),
            on_hover: rgb(37, 25, 63),
        }
    }
}
pub fn parse_hex(value: &Value) -> Option<Color> {
    let digits = value.as_str()?.strip_prefix('#')?;
    if digits.len() != 6 {
        return None;
    }
    let value = u32::from_str_radix(digits, 16).ok()?;
    Some(rgb((value >> 16) as u8, (value >> 8) as u8, value as u8))
}
impl Theme {
    pub fn from_appearance(value: &Value) -> Self {
        let mut theme = Self::from_palette(&value["palette"]);
        theme.style = HostStyle::parse(value.get("style")).unwrap_or_default();
        theme.panel_border = parse_hex(&value["panel_border_color"]).unwrap_or(theme.outline);
        theme
    }
    pub fn from_palette(palette: &Value) -> Self {
        let mut theme = Self::default();
        for (name, target) in [
            ("surface", &mut theme.surface),
            ("surface_variant", &mut theme.variant),
            ("on_surface", &mut theme.on_surface),
            ("on_surface_variant", &mut theme.on_variant),
            ("primary", &mut theme.primary),
            ("on_primary", &mut theme.on_primary),
            ("secondary", &mut theme.secondary),
            ("on_secondary", &mut theme.on_secondary),
            ("error", &mut theme.error),
            ("on_error", &mut theme.on_error),
            ("outline", &mut theme.outline),
            ("shadow", &mut theme.shadow),
            ("hover", &mut theme.hover),
            ("on_hover", &mut theme.on_hover),
        ] {
            if let Some(color) = parse_hex(&palette[name]) {
                *target = color;
            }
        }
        // Hosts that predate the extra roles still get a readable, theme-derived hover label.
        if palette.get("on_hover").is_none() {
            theme.on_hover = readable_on(theme.hover, theme.on_surface, theme.surface);
        }
        theme
    }
}
pub fn with_alpha(color: Color, alpha: f32) -> Color {
    Color::from_rgba(
        color.red(),
        color.green(),
        color.blue(),
        (color.alpha() * alpha).clamp(0.0, 1.0),
    )
    .unwrap_or(color)
}
fn luminance(color: Color) -> f32 {
    let channel = |v: f32| {
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.red()) + 0.7152 * channel(color.green()) + 0.0722 * channel(color.blue())
}
pub fn contrast(a: Color, b: Color) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}
/// Pick whichever candidate reads better on `background`.
pub fn readable_on(background: Color, first: Color, second: Color) -> Color {
    if contrast(background, first) >= contrast(background, second) {
        first
    } else {
        second
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn every_noctalia_role_is_read_and_invalid_values_are_ignored() {
        let theme = Theme::from_palette(&json!({
            "surface":"#010203","hover":"#112233","on_hover":"#445566","secondary":"#778899",
            "error":"#aa0000","on_error":"#ffffff","primary":"zzz","shadow":"#000001"
        }));
        assert_eq!(theme.surface, Color::from_rgba8(1, 2, 3, 255));
        assert_eq!(theme.hover, Color::from_rgba8(0x11, 0x22, 0x33, 255));
        assert_eq!(theme.on_hover, Color::from_rgba8(0x44, 0x55, 0x66, 255));
        assert_eq!(theme.secondary, Color::from_rgba8(0x77, 0x88, 0x99, 255));
        assert_eq!(theme.error, Color::from_rgba8(0xaa, 0, 0, 255));
        assert_eq!(
            theme.primary,
            Theme::default().primary,
            "invalid hex keeps the previous value"
        );
    }
    #[test]
    fn missing_hover_label_is_derived_for_contrast() {
        let theme = Theme::from_palette(
            &json!({"hover":"#fff4d0","on_surface":"#f0f0f0","surface":"#101010"}),
        );
        assert!(contrast(theme.hover, theme.on_hover) > 7.0);
    }
}
