//! Small font engine for the standalone interface: kerning-aware advances, a regular and a bold
//! face, measurement, ellipsis truncation and clipped glyph blitting onto premultiplied pixmaps.
use anyhow::{Context, Result};
use fontdue::{Font, FontSettings, Metrics};
use std::{borrow::Cow, collections::HashMap, path::PathBuf, process::Command};
use tiny_skia::{Color, Pixmap};

/// Device-pixel clip rectangle (x, y, w, h).
pub type Clip = (i32, i32, i32, i32);

pub struct Typeface {
    regular: Font,
    bold: Option<Font>,
    scale: f32,
    glyphs: HashMap<(char, u32, bool), (Metrics, Vec<u8>)>,
}
fn font_file(pattern: &str) -> Option<PathBuf> {
    Command::new("fc-match")
        .args(["-f", "%{file}", pattern])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
        .filter(|path| {
            path.extension().is_some_and(|ext| {
                ["ttf", "otf", "ttc"]
                    .iter()
                    .any(|kind| ext.eq_ignore_ascii_case(kind))
            })
        })
}
fn load(path: &PathBuf) -> Result<Font> {
    Font::from_bytes(std::fs::read(path)?, FontSettings::default()).map_err(|e| anyhow::anyhow!(e))
}
impl Typeface {
    /// `explicit` is the user's `font_file` setting; otherwise the host's font family is resolved
    /// through fontconfig, with a DejaVu fallback and an optional build-time default.
    pub fn resolve(family: &str, explicit: Option<PathBuf>) -> Result<Self> {
        let regular_path = explicit
            .or_else(|| font_file(family))
            .or_else(|| {
                [
                    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                    "/usr/share/fonts/TTF/DejaVuSans.ttf",
                ]
                .iter()
                .map(PathBuf::from)
                .find(|p| p.exists())
            })
            .or_else(|| option_env!("GIF_PLAYER_DEFAULT_FONT").map(PathBuf::from))
            .context("Standalone UI needs a system font; set settings.json font_file")?;
        let bold_path = font_file(&format!("{family}:bold")).filter(|p| *p != regular_path);
        Ok(Self {
            regular: load(&regular_path)?,
            bold: bold_path.and_then(|p| load(&p).ok()),
            scale: 1.0,
            glyphs: HashMap::new(),
        })
    }
    pub fn set_scale(&mut self, scale: f32) {
        if (self.scale - scale).abs() > f32::EPSILON {
            self.scale = scale;
            self.glyphs.clear();
        }
    }
    fn px(&self, size: f32) -> u32 {
        (size * self.scale).round().max(1.0) as u32
    }
    fn face(&self, bold: bool) -> &Font {
        if bold {
            self.bold.as_ref().unwrap_or(&self.regular)
        } else {
            &self.regular
        }
    }
    fn glyph(&mut self, ch: char, px: u32, bold: bool) -> &(Metrics, Vec<u8>) {
        if !self.glyphs.contains_key(&(ch, px, bold)) {
            if self.glyphs.len() > 4096 {
                self.glyphs.clear();
            }
            let value = self.face(bold).rasterize(ch, px as f32);
            self.glyphs.insert((ch, px, bold), value);
        }
        &self.glyphs[&(ch, px, bold)]
    }
    fn advance(&mut self, ch: char, next: Option<char>, px: u32, bold: bool) -> f32 {
        let kern = next
            .and_then(|n| self.face(bold).horizontal_kern(ch, n, px as f32))
            .unwrap_or(0.0);
        self.glyph(ch, px, bold).0.advance_width + kern
    }
    /// Logical width of `text`.
    pub fn measure(&mut self, text: &str, size: f32, bold: bool) -> f32 {
        let px = self.px(size);
        let mut chars = text.chars().peekable();
        let mut width = 0.0;
        while let Some(ch) = chars.next() {
            width += self.advance(ch, chars.peek().copied(), px, bold);
        }
        width / self.scale
    }
    /// Logical width of the first `chars` characters; used for caret placement.
    pub fn prefix_width(&mut self, text: &str, chars: usize, size: f32, bold: bool) -> f32 {
        let end = text
            .char_indices()
            .nth(chars)
            .map_or(text.len(), |(i, _)| i);
        self.measure(&text[..end], size, bold)
    }
    /// Index of the character boundary closest to logical offset `x`.
    pub fn index_at(&mut self, text: &str, x: f32, size: f32, bold: bool) -> usize {
        let count = text.chars().count();
        let mut best = (0, f32::MAX);
        for index in 0..=count {
            let distance = (self.prefix_width(text, index, size, bold) - x).abs();
            if distance < best.1 {
                best = (index, distance);
            }
            if distance > best.1 {
                break;
            }
        }
        best.0
    }
    /// (ascent, descent) in logical pixels; descent is positive.
    pub fn line(&self, size: f32, bold: bool) -> (f32, f32) {
        let px = self.px(size) as f32;
        self.face(bold)
            .horizontal_line_metrics(px)
            .map(|m| (m.ascent / self.scale, -m.descent / self.scale))
            .unwrap_or((size * 0.9, size * 0.25))
    }
    pub fn line_height(&self, size: f32, bold: bool) -> f32 {
        let (a, d) = self.line(size, bold);
        a + d
    }
    pub fn ellipsize<'a>(
        &mut self,
        text: &'a str,
        size: f32,
        bold: bool,
        max: f32,
    ) -> Cow<'a, str> {
        if max <= 0.0 || self.measure(text, size, bold) <= max {
            return Cow::Borrowed(text);
        }
        let dots = self.measure("…", size, bold);
        let mut kept = String::new();
        for ch in text.chars() {
            kept.push(ch);
            if self.measure(&kept, size, bold) + dots > max {
                kept.pop();
                break;
            }
        }
        let kept = kept.trim_end().to_owned();
        Cow::Owned(kept + "…")
    }
    /// Draw `text` with its box top-left at logical (x, y); returns the drawn logical width.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        pix: &mut Pixmap,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        bold: bool,
        color: Color,
        clip: Option<Clip>,
    ) -> f32 {
        let px = self.px(size);
        let (ascent, _) = self.line(size, bold);
        let baseline = ((y + ascent) * self.scale).round() as i32;
        let mut cursor = x * self.scale;
        let (cx, cy, cw, ch_) = clip.unwrap_or((0, 0, pix.width() as i32, pix.height() as i32));
        let (left, top) = (cx.max(0), cy.max(0));
        let right = (cx + cw).min(pix.width() as i32);
        let bottom = (cy + ch_).min(pix.height() as i32);
        let width = pix.width() as usize;
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            let advance = self.advance(ch, chars.peek().copied(), px, bold);
            let (metrics, bitmap) = self.glyph(ch, px, bold).clone();
            let gx = cursor.round() as i32 + metrics.xmin;
            let gy = baseline - metrics.height as i32 - metrics.ymin;
            if gx < right && gx + metrics.width as i32 > left {
                let bytes = pix.data_mut();
                for row in 0..metrics.height {
                    let py = gy + row as i32;
                    if py < top || py >= bottom {
                        continue;
                    }
                    for col in 0..metrics.width {
                        let pxl = gx + col as i32;
                        if pxl < left || pxl >= right {
                            continue;
                        }
                        let coverage = bitmap[row * metrics.width + col] as f32 / 255.0;
                        let alpha = coverage * color.alpha();
                        if alpha <= 0.0 {
                            continue;
                        }
                        let at = (py as usize * width + pxl as usize) * 4;
                        for (c, source) in [color.red(), color.green(), color.blue()]
                            .iter()
                            .enumerate()
                        {
                            bytes[at + c] = (source * 255.0 * alpha
                                + bytes[at + c] as f32 * (1.0 - alpha))
                                .round() as u8;
                        }
                        bytes[at + 3] =
                            (255.0 * alpha + bytes[at + 3] as f32 * (1.0 - alpha)).round() as u8;
                    }
                }
            }
            cursor += advance;
            if cursor > right as f32 + 64.0 {
                break;
            }
        }
        (cursor - x * self.scale) / self.scale
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn face() -> Option<Typeface> {
        Typeface::resolve("sans-serif", None).ok()
    }
    #[test]
    fn ellipsis_keeps_text_inside_its_budget_and_is_a_noop_when_it_fits() {
        let Some(mut face) = face() else { return };
        let full = face.measure("A rather long animation name.gif", 14.0, false);
        assert!(matches!(
            face.ellipsize("Short", 14.0, false, 400.0),
            Cow::Borrowed("Short")
        ));
        let cut = face.ellipsize("A rather long animation name.gif", 14.0, false, full / 2.0);
        assert!(cut.ends_with('…'));
        assert!(face.measure(&cut, 14.0, false) <= full / 2.0 + 0.5);
    }
    #[test]
    fn caret_index_round_trips_through_measured_widths() {
        let Some(mut face) = face() else { return };
        let text = "Café 鳥 path";
        for index in 0..=text.chars().count() {
            let x = face.prefix_width(text, index, 14.0, false);
            assert_eq!(face.index_at(text, x + 0.2, 14.0, false), index);
        }
    }
    #[test]
    fn drawing_is_clipped_and_scale_changes_measurement_consistently() {
        let Some(mut face) = face() else { return };
        let mut pix = Pixmap::new(80, 40).unwrap();
        face.draw(
            &mut pix,
            "MMMMMMMM",
            2.0,
            2.0,
            14.0,
            false,
            Color::WHITE,
            Some((0, 0, 20, 40)),
        );
        let data = pix.data();
        assert!(
            (0..40).all(|y| (20..80).all(|x| data[(y * 80 + x) * 4 + 3] == 0)),
            "pixels outside the clip stay untouched"
        );
        let one = face.measure("Hello", 14.0, false);
        face.set_scale(2.0);
        assert!((face.measure("Hello", 14.0, false) - one).abs() < 1.5);
    }
}
