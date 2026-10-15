//! Drawing primitives for the standalone interface, styled after Noctalia 5's native controls.
//! Positions are logical "page" coordinates; `origin` maps them to the window and `clip` limits
//! drawing/hit-testing to the active region. While `dry` is set nothing is painted or registered,
//! which lets pages measure their height before drawing.
use super::{
    Action, Hit, Ui,
    icons::{self, Icon},
    theme::*,
};
use crate::decode::Animation;
use crate::render::Rect;
use std::path::Path as FilePath;
use tiny_skia::{
    Color, FillRule, FilterQuality, Mask, Paint, Path, PathBuilder, Pattern, Pixmap, PixmapPaint,
    PixmapRef, SpreadMode, Stroke, Transform,
};

#[derive(Clone, Copy, PartialEq)]
pub enum Variant {
    Default,
    Primary,
    Ghost,
    Destructive,
    Tab,
    TabActive,
}
#[derive(Clone)]
pub struct Btn<'a> {
    pub label: &'a str,
    pub icon: Option<Icon>,
    pub variant: Variant,
    pub selected: bool,
    pub enabled: bool,
    pub start: bool,
    pub hint: &'a str,
    pub badge: Option<String>,
}
impl Default for Btn<'_> {
    fn default() -> Self {
        Self {
            label: "",
            icon: None,
            variant: Variant::Default,
            selected: false,
            enabled: true,
            start: false,
            hint: "",
            badge: None,
        }
    }
}
fn round_path(x: f32, y: f32, w: f32, h: f32, radius: f32) -> Option<Path> {
    let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);
    let mut b = PathBuilder::new();
    if r <= 0.01 {
        b.move_to(x, y);
        b.line_to(x + w, y);
        b.line_to(x + w, y + h);
        b.line_to(x, y + h);
        b.close();
        return b.finish();
    }
    let k = r * 0.552_284_8;
    b.move_to(x + r, y);
    b.line_to(x + w - r, y);
    b.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    b.line_to(x + w, y + h - r);
    b.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    b.line_to(x + r, y + h);
    b.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    b.line_to(x, y + r);
    b.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    b.close();
    b.finish()
}
impl Ui {
    pub(super) fn sc(&self) -> f32 {
        self.paint_scale
    }
    pub(super) fn to_screen(&self, r: &Rect) -> Rect {
        Rect {
            x: r.x + self.origin.0,
            y: r.y + self.origin.1,
            w: r.w,
            h: r.h,
        }
    }
    fn intersect(a: &Rect, b: &Rect) -> Rect {
        let x = a.x.max(b.x);
        let y = a.y.max(b.y);
        let right = (a.x + a.w).min(b.x + b.w);
        let bottom = (a.y + a.h).min(b.y + b.h);
        Rect {
            x,
            y,
            w: (right - x).max(0),
            h: (bottom - y).max(0),
        }
    }
    /// The clip mask is only needed when a shape reaches past the clip rectangle; skipping it keeps
    /// the common case on tiny-skia's cheaper unmasked pipeline.
    fn mask_for(&self, s: &Rect) -> Option<&Mask> {
        let c = &self.clip;
        let inside = s.x >= c.x && s.y >= c.y && s.x + s.w <= c.x + c.w && s.y + s.h <= c.y + c.h;
        if inside { None } else { self.mask.as_ref() }
    }
    pub(super) fn visible(&self, page: &Rect) -> bool {
        if self.dry {
            return false;
        }
        let s = self.to_screen(page);
        let c = &self.clip;
        s.x < c.x + c.w && s.x + s.w > c.x && s.y < c.y + c.h && s.y + s.h > c.y
    }
    /// Switch the active region: page origin offset, clip rectangle (window coordinates).
    pub(super) fn set_region(
        &mut self,
        pix: &Pixmap,
        origin: (i32, i32),
        clip: Rect,
        scrolls: bool,
    ) {
        self.origin = origin;
        self.scrolls = scrolls;
        self.clip = clip.clone();
        let s = self.sc();
        let mask = self
            .mask
            .get_or_insert_with(|| Mask::new(pix.width(), pix.height()).unwrap());
        if mask.width() != pix.width() || mask.height() != pix.height() {
            *mask = Mask::new(pix.width(), pix.height()).unwrap();
        }
        mask.clear();
        if let Some(path) = round_path(
            clip.x as f32,
            clip.y as f32,
            clip.w as f32,
            clip.h as f32,
            0.0,
        ) {
            mask.fill_path(&path, FillRule::Winding, false, Transform::from_scale(s, s));
        }
    }
    pub(super) fn clip_px(&self) -> (i32, i32, i32, i32) {
        let s = self.sc();
        let c = &self.clip;
        (
            (c.x as f32 * s).round() as i32,
            (c.y as f32 * s).round() as i32,
            (c.w as f32 * s).round() as i32,
            (c.h as f32 * s).round() as i32,
        )
    }
    pub(super) fn fill_round(&mut self, pix: &mut Pixmap, r: &Rect, color: Color, radius: f32) {
        if !self.visible(r) {
            return;
        }
        let s = self.to_screen(r);
        let Some(path) = round_path(s.x as f32, s.y as f32, s.w as f32, s.h as f32, radius) else {
            return;
        };
        let mut paint = Paint::default();
        paint.set_color(color);
        paint.anti_alias = true;
        let k = self.sc();
        pix.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::from_scale(k, k),
            self.mask_for(&s),
        );
    }
    /// Border drawn inside the rectangle, like Noctalia's rounded-rect border.
    pub(super) fn stroke_round(
        &mut self,
        pix: &mut Pixmap,
        r: &Rect,
        color: Color,
        radius: f32,
        width: f32,
    ) {
        if !self.visible(r) {
            return;
        }
        let s = self.to_screen(r);
        let half = width / 2.0;
        let Some(path) = round_path(
            s.x as f32 + half,
            s.y as f32 + half,
            s.w as f32 - width,
            s.h as f32 - width,
            (radius - half).max(0.0),
        ) else {
            return;
        };
        let mut paint = Paint::default();
        paint.set_color(color);
        paint.anti_alias = true;
        let k = self.sc();
        pix.stroke_path(
            &path,
            &paint,
            &Stroke {
                width,
                ..Default::default()
            },
            Transform::from_scale(k, k),
            self.mask_for(&s),
        );
    }
    /// Draw one animation frame "contain"-fitted into `area` with rounded corners. Scaled tiles are
    /// cached per (file, frame, size) so redraws caused by hover or scrolling are plain blits.
    pub(super) fn image_fit(
        &mut self,
        pix: &mut Pixmap,
        anim: &Animation,
        frame: usize,
        area: &Rect,
        radius: f32,
        file: &FilePath,
    ) {
        if !self.visible(area) {
            return;
        }
        let k = self.sc();
        let s = self.to_screen(area);
        let factor = (s.w as f32 / anim.width as f32).min(s.h as f32 / anim.height as f32);
        let (rw, rh) = (anim.width as f32 * factor, anim.height as f32 * factor);
        let rx = s.x as f32 + (s.w as f32 - rw) / 2.0;
        let ry = s.y as f32 + (s.h as f32 - rh) / 2.0;
        let (dx, dy) = ((rx * k).round() as i32, (ry * k).round() as i32);
        let (dw, dh) = (
            ((rw * k).round() as u32).max(1),
            ((rh * k).round() as u32).max(1),
        );
        let key = (
            file.to_path_buf(),
            anim.width,
            anim.height,
            frame,
            dw,
            dh,
            (radius * k).round() as u32,
        );
        if !self.image_cache.contains_key(&key) {
            let Ok(pixels) = anim.pixels(frame) else {
                return;
            };
            let Some(source) = PixmapRef::from_bytes(&pixels, anim.width, anim.height) else {
                return;
            };
            let Some(mut tile) = Pixmap::new(dw, dh) else {
                return;
            };
            let paint = Paint {
                shader: Pattern::new(
                    source,
                    SpreadMode::Pad,
                    FilterQuality::Bilinear,
                    1.0,
                    Transform::from_scale(
                        dw as f32 / anim.width as f32,
                        dh as f32 / anim.height as f32,
                    ),
                ),
                anti_alias: true,
                ..Paint::default()
            };
            let Some(path) = round_path(0.0, 0.0, dw as f32, dh as f32, radius * k) else {
                return;
            };
            tile.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
            if self.image_cache.len() >= 96 {
                self.image_cache.clear();
            }
            self.image_cache.insert(key.clone(), tile);
        }
        let tile = &self.image_cache[&key];
        let target = Rect {
            x: dx,
            y: dy,
            w: dw as i32,
            h: dh as i32,
        };
        // `mask_for` works in logical units; compare against the device rectangle instead.
        let device_clip = self.clip_px();
        let inside = dx >= device_clip.0
            && dy >= device_clip.1
            && dx + target.w <= device_clip.0 + device_clip.2
            && dy + target.h <= device_clip.1 + device_clip.3;
        pix.draw_pixmap(
            dx,
            dy,
            tile.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            if inside { None } else { self.mask.as_ref() },
        );
    }
    pub(super) fn icon(
        &mut self,
        pix: &mut Pixmap,
        icon: Icon,
        cx: f32,
        cy: f32,
        size: f32,
        color: Color,
    ) {
        let probe = Rect {
            x: (cx - size / 2.0) as i32,
            y: (cy - size / 2.0) as i32,
            w: size as i32 + 1,
            h: size as i32 + 1,
        };
        if !self.visible(&probe) {
            return;
        }
        let k = self.sc();
        let (ox, oy) = self.origin;
        icons::draw(
            pix,
            icon,
            (cx + ox as f32) * k,
            (cy + oy as f32) * k,
            size * k,
            color,
            self.mask_for(&self.to_screen(&probe)),
        );
    }
    pub(super) fn measure(&mut self, text: &str, size: f32, bold: bool) -> f32 {
        self.face
            .as_mut()
            .map_or(0.0, |f| f.measure(text, size, bold))
    }
    pub(super) fn line_height(&self, size: f32, bold: bool) -> i32 {
        self.face
            .as_ref()
            .map_or(size as i32, |f| f.line_height(size, bold).ceil() as i32)
    }
    /// Draw text with its line box top at `y`; text longer than `max` is ellipsized. Returns width.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn text(
        &mut self,
        pix: &mut Pixmap,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        bold: bool,
        color: Color,
        max: i32,
    ) -> f32 {
        let lh = self.line_height(size, bold);
        let Some(face) = self.face.as_mut() else {
            return 0.0;
        };
        let shown = face.ellipsize(text, size, bold, max as f32).into_owned();
        let width = face.measure(&shown, size, bold);
        let probe = Rect {
            x: x as i32,
            y: y as i32,
            w: width.ceil() as i32 + 1,
            h: lh,
        };
        if !self.visible(&probe) {
            return width;
        }
        let clip = self.clip_px();
        let (ox, oy) = self.origin;
        if let Some(face) = self.face.as_mut() {
            face.draw(
                pix,
                &shown,
                x + ox as f32,
                y + oy as f32,
                size,
                bold,
                color,
                Some(clip),
            );
        }
        width
    }
    /// Vertically centred text inside a row of height `h`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn text_row(
        &mut self,
        pix: &mut Pixmap,
        text: &str,
        x: f32,
        y: i32,
        h: i32,
        size: f32,
        bold: bool,
        color: Color,
        max: i32,
    ) -> f32 {
        let lh = self.line_height(size, bold);
        self.text(
            pix,
            text,
            x,
            y as f32 + (h - lh) as f32 / 2.0,
            size,
            bold,
            color,
            max,
        )
    }
    pub(super) fn text_right(
        &mut self,
        pix: &mut Pixmap,
        text: &str,
        right: i32,
        (y, h): (i32, i32),
        size: f32,
        color: Color,
    ) {
        let width = self.measure(text, size, false);
        self.text_row(
            pix,
            text,
            right as f32 - width,
            y,
            h,
            size,
            false,
            color,
            width.ceil() as i32 + 2,
        );
    }
    /// Register an interactive region; returns its index.
    pub(super) fn hit(&mut self, r: &Rect, action: Action, hint: &str) {
        if self.dry {
            return;
        }
        let s = self.to_screen(r);
        let rect = Self::intersect(&s, &self.clip);
        self.hits.push(Hit {
            rect,
            page: r.clone(),
            action,
            hint: hint.to_owned(),
            scrolls: self.scrolls,
        });
    }
    pub(super) fn is_hover(&self, action: &Action) -> bool {
        self.hover_action.as_ref() == Some(action)
    }
    pub(super) fn is_pressed(&self, action: &Action) -> bool {
        self.pressed.as_ref() == Some(action) && self.is_hover(action)
    }
    pub(super) fn is_focused(&self, action: &Action) -> bool {
        self.keyboard_focus.as_ref() == Some(action)
    }
    // ----- controls -----
    pub(super) fn button(&mut self, pix: &mut Pixmap, r: &Rect, b: &Btn, action: Action) {
        let t = self.theme;
        let hover = b.enabled && self.is_hover(&action);
        let pressed = b.enabled && self.is_pressed(&action);
        let focused = b.enabled && self.is_focused(&action);
        let selected = b.enabled && b.selected;
        let (mut bg, mut border, mut label): (Option<Color>, Option<Color>, Color) = match b.variant
        {
            Variant::Default => (Some(t.variant), Some(t.outline), t.on_surface),
            Variant::Primary => (Some(t.primary), None, t.on_primary),
            Variant::Destructive => (Some(t.error), Some(t.outline), t.on_error),
            Variant::Ghost | Variant::Tab => (None, None, t.on_surface),
            Variant::TabActive => (Some(t.primary), None, t.on_primary),
        };
        if b.enabled && b.variant != Variant::TabActive {
            if pressed || selected {
                (bg, border, label) = match b.variant {
                    Variant::Destructive => (Some(t.error), Some(t.error), t.on_error),
                    Variant::Ghost | Variant::Tab if pressed && !selected => {
                        (Some(t.variant), None, t.on_surface)
                    }
                    _ => (Some(t.primary), Some(t.primary), t.on_primary),
                };
            } else if hover {
                (bg, border, label) = (Some(t.hover), None, t.on_hover);
            }
        }
        if !b.enabled {
            bg = bg.map(|c| with_alpha(c, DISABLED_ALPHA));
            border = border.map(|c| with_alpha(c, 0.5));
            label = with_alpha(label, DISABLED_ALPHA);
        }
        if self.visible(r) {
            if let Some(color) = bg {
                self.fill_round(pix, r, color, RADIUS_MD);
            }
            if focused {
                self.stroke_round(pix, r, t.secondary, RADIUS_MD, FOCUS_RING);
            } else if let Some(color) = border {
                self.stroke_round(pix, r, color, RADIUS_MD, 1.0);
            }
            let icon = if b.icon.is_some() { 16.0 } else { 0.0 };
            let badge = b.badge.clone();
            let badge_w = badge
                .as_deref()
                .map_or(0.0, |s| (self.measure(s, FONT_MINI, true) + 12.0).max(20.0));
            let gap = if icon > 0.0 && !b.label.is_empty() {
                SPACE_SM as f32
            } else {
                0.0
            };
            let avail = r.w as f32 - 24.0 - badge_w - if badge_w > 0.0 { 6.0 } else { 0.0 };
            let text_w = if b.label.is_empty() {
                0.0
            } else {
                self.measure(b.label, FONT_BODY, false)
                    .min((avail - icon - gap).max(0.0))
            };
            let total = icon + gap + text_w;
            let start_x = if b.start {
                r.x as f32 + 12.0
            } else {
                r.x as f32 + (r.w as f32 - total) / 2.0
            };
            if let Some(i) = b.icon {
                self.icon(
                    pix,
                    i,
                    start_x + 8.0,
                    r.y as f32 + r.h as f32 / 2.0,
                    16.0,
                    label,
                );
            }
            if !b.label.is_empty() {
                self.text_row(
                    pix,
                    b.label,
                    start_x + icon + gap,
                    r.y,
                    r.h,
                    FONT_BODY,
                    false,
                    label,
                    (avail - icon - gap).ceil() as i32,
                );
            }
            if let Some(count) = badge {
                let pill = Rect {
                    x: r.x + r.w - badge_w as i32 - 8,
                    y: r.y + (r.h - 20) / 2,
                    w: badge_w as i32,
                    h: 20,
                };
                let fill = if b.variant == Variant::TabActive || selected {
                    with_alpha(t.on_primary, 0.18)
                } else {
                    with_alpha(t.primary, 0.22)
                };
                self.fill_round(pix, &pill, fill, 10.0);
                let w = self.measure(&count, FONT_MINI, true);
                self.text_row(
                    pix,
                    &count,
                    pill.x as f32 + (pill.w as f32 - w) / 2.0,
                    pill.y,
                    pill.h,
                    FONT_MINI,
                    true,
                    label,
                    pill.w,
                );
            }
        }
        if b.enabled {
            self.hit(r, action, b.hint);
        }
    }
    /// Text field with optional leading search icon and trailing clear button.
    pub(super) fn input(
        &mut self,
        pix: &mut Pixmap,
        r: &Rect,
        key: &str,
        placeholder: &str,
        search: bool,
    ) {
        let t = self.theme;
        let action = Action::Field(key.to_owned());
        let focused = self.focus.as_deref() == Some(key);
        let hover = self.is_hover(&action);
        let value = self.fields.get(key).cloned().unwrap_or_default();
        let clear = search && !value.is_empty();
        let text_x = r.x + 12 + if search { 24 } else { 0 };
        let avail = (r.x + r.w - 12 - if clear { 28 } else { 0 } - text_x).max(8);
        if self.visible(r) {
            // Inside a card the field sinks to the page surface so it stays distinguishable.
            let base = if self.in_card { t.surface } else { t.variant };
            self.fill_round(pix, r, if focused { t.surface } else { base }, RADIUS_MD);
            if focused {
                self.stroke_round(pix, r, t.secondary, RADIUS_MD, FOCUS_RING);
            } else {
                self.stroke_round(
                    pix,
                    r,
                    if hover { t.hover } else { t.outline },
                    RADIUS_MD,
                    1.0,
                );
            }
            if search {
                self.icon(
                    pix,
                    Icon::Search,
                    (r.x + 12 + 8) as f32,
                    r.y as f32 + r.h as f32 / 2.0,
                    16.0,
                    t.on_variant,
                );
            }
            let caret_w = if focused {
                self.face.as_mut().map_or(0.0, |f| {
                    f.prefix_width(&value, self.caret, FONT_BODY, false)
                })
            } else {
                0.0
            };
            let scroll = if focused {
                (caret_w - avail as f32 + 2.0).max(0.0)
            } else {
                0.0
            };
            let lh = self.line_height(FONT_BODY, false);
            let top = r.y + (r.h - lh) / 2;
            // Clip the text to the field interior.
            let saved = (self.clip.clone(), self.origin, self.scrolls);
            let inner = Self::intersect(
                &self.to_screen(&Rect {
                    x: text_x,
                    y: r.y,
                    w: avail,
                    h: r.h,
                }),
                &saved.0,
            );
            self.set_region(pix, saved.1, inner, saved.2);
            if value.is_empty() {
                self.text(
                    pix,
                    placeholder,
                    text_x as f32,
                    top as f32,
                    FONT_BODY,
                    false,
                    with_alpha(t.on_variant, PLACEHOLDER_ALPHA),
                    avail,
                );
            } else if focused {
                if let Some(anchor) = self.anchor.filter(|a| *a != self.caret) {
                    let (lo, hi) = (anchor.min(self.caret), anchor.max(self.caret));
                    let x0 = self
                        .face
                        .as_mut()
                        .map_or(0.0, |f| f.prefix_width(&value, lo, FONT_BODY, false));
                    let x1 = self
                        .face
                        .as_mut()
                        .map_or(0.0, |f| f.prefix_width(&value, hi, FONT_BODY, false));
                    let sel = Rect {
                        x: (text_x as f32 + x0 - scroll).round() as i32,
                        y: top - 2,
                        w: ((x1 - x0).ceil() as i32).max(1),
                        h: lh + 4,
                    };
                    self.fill_round(pix, &sel, with_alpha(t.primary, 0.45), RADIUS_SM);
                }
                self.text(
                    pix,
                    &value,
                    text_x as f32 - scroll,
                    top as f32,
                    FONT_BODY,
                    false,
                    t.on_surface,
                    i32::MAX / 4,
                );
                let cx = (text_x as f32 + caret_w - scroll).round() as i32;
                self.fill_round(
                    pix,
                    &Rect {
                        x: cx,
                        y: top - 1,
                        w: 2,
                        h: lh + 2,
                    },
                    t.primary,
                    1.0,
                );
            } else {
                self.text(
                    pix,
                    &value,
                    text_x as f32,
                    top as f32,
                    FONT_BODY,
                    false,
                    t.on_surface,
                    avail,
                );
            }
            self.set_region(pix, saved.1, saved.0, saved.2);
            if clear {
                let clear_r = Rect {
                    x: r.x + r.w - 32,
                    y: r.y + (r.h - 28) / 2,
                    w: 28,
                    h: 28,
                };
                let hov = self.is_hover(&Action::ClearSearch);
                if hov {
                    self.fill_round(pix, &clear_r, t.hover, RADIUS_SM + 3.0);
                }
                self.icon(
                    pix,
                    Icon::Close,
                    (clear_r.x + 14) as f32,
                    (clear_r.y + 14) as f32,
                    14.0,
                    if hov { t.on_hover } else { t.on_variant },
                );
            }
        }
        if !self.dry {
            let screen = self.to_screen(&Rect {
                x: text_x,
                y: r.y,
                w: 0,
                h: 0,
            });
            let scroll = if focused {
                let w = self.face.as_mut().map_or(0.0, |f| {
                    f.prefix_width(&value, self.caret, FONT_BODY, false)
                });
                (w - avail as f32 + 2.0).max(0.0)
            } else {
                0.0
            };
            self.field_origin
                .insert(key.to_owned(), screen.x as f32 - scroll);
        }
        self.hit(r, action, "");
        if clear {
            self.hit(
                &Rect {
                    x: r.x + r.w - 32,
                    y: r.y + (r.h - 28) / 2,
                    w: 28,
                    h: 28,
                },
                Action::ClearSearch,
                &self.tr("clear_search"),
            );
        }
    }
    pub(super) fn toggle(
        &mut self,
        pix: &mut Pixmap,
        r: &Rect,
        label: &str,
        hint: &str,
        checked: bool,
        action: Action,
    ) {
        let t = self.theme;
        let hover = self.is_hover(&action);
        let focused = self.is_focused(&action);
        if self.visible(r) {
            let right = r.x + r.w;
            let track = Rect {
                x: right - TOGGLE_W,
                y: r.y + (r.h - TOGGLE_H) / 2,
                w: TOGGLE_W,
                h: TOGGLE_H,
            };
            let text_w = r.w - TOGGLE_W - 16;
            if hint.is_empty() {
                self.text_row(
                    pix,
                    label,
                    r.x as f32,
                    r.y,
                    r.h,
                    FONT_BODY,
                    false,
                    t.on_surface,
                    text_w,
                );
            } else {
                let lh =
                    self.line_height(FONT_BODY, false) + 2 + self.line_height(FONT_CAPTION, false);
                let top = r.y + (r.h - lh) / 2;
                self.text(
                    pix,
                    label,
                    r.x as f32,
                    top as f32,
                    FONT_BODY,
                    false,
                    t.on_surface,
                    text_w,
                );
                let below = top + self.line_height(FONT_BODY, false) + 2;
                self.text(
                    pix,
                    hint,
                    r.x as f32,
                    below as f32,
                    FONT_CAPTION,
                    false,
                    t.on_variant,
                    text_w,
                );
            }
            self.fill_round(
                pix,
                &track,
                if checked { t.primary } else { t.outline },
                TOGGLE_H as f32 / 2.0,
            );
            let ring = if focused {
                Some((t.secondary, FOCUS_RING))
            } else if hover {
                Some((t.hover, 1.0))
            } else if checked {
                Some((t.primary, 1.0))
            } else {
                Some((t.outline, 1.0))
            };
            if let Some((c, w)) = ring {
                self.stroke_round(pix, &track, c, TOGGLE_H as f32 / 2.0, w);
            }
            let thumb_x = track.x + 3 + if checked { 16 } else { 0 };
            self.fill_round(
                pix,
                &Rect {
                    x: thumb_x,
                    y: track.y + 3,
                    w: 18,
                    h: 18,
                },
                t.on_primary,
                9.0,
            );
        }
        self.hit(r, action, "");
    }
    /// Label/value row above a Noctalia-style slider; returns the consumed height.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn slider(
        &mut self,
        pix: &mut Pixmap,
        x: i32,
        y: i32,
        w: i32,
        label: &str,
        display: &str,
        value: f64,
        action: Action,
    ) -> i32 {
        let t = self.theme;
        let Action::Slider { min, max, .. } = &action else {
            return 0;
        };
        let (min, max) = (*min, *max);
        let row = Rect { x, y, w, h: 20 };
        if self.visible(&row) {
            self.text_row(
                pix,
                label,
                x as f32,
                y,
                20,
                FONT_BODY,
                false,
                t.on_surface,
                w - 80,
            );
            self.text_right(pix, display, x + w, (y, 20), FONT_CAPTION, t.primary);
        }
        let track = Rect {
            x,
            y: y + 26,
            w,
            h: 22,
        };
        let fraction = ((value - min) / (max - min)).clamp(0.0, 1.0) as f32;
        let span = (w - 4 - SLIDER_THUMB) as f32;
        let center_x = x as f32 + 2.0 + SLIDER_THUMB as f32 / 2.0 + fraction * span;
        let cy = track.y + track.h / 2;
        if self.visible(&track) {
            let hover = self.is_hover(&action)
                || self
                    .active_slider
                    .as_ref()
                    .is_some_and(|d| d.action == action);
            let focused = self.is_focused(&action);
            let bar = Rect {
                x: x + 2,
                y: cy - SLIDER_TRACK / 2,
                w: w - 4,
                h: SLIDER_TRACK,
            };
            self.fill_round(pix, &bar, t.outline, SLIDER_TRACK as f32 / 2.0);
            let fill_w = (center_x - bar.x as f32).round() as i32;
            self.fill_round(
                pix,
                &Rect {
                    w: fill_w.max(SLIDER_TRACK),
                    ..bar
                },
                t.primary,
                SLIDER_TRACK as f32 / 2.0,
            );
            let thumb = Rect {
                x: (center_x - SLIDER_THUMB as f32 / 2.0).round() as i32,
                y: cy - SLIDER_THUMB / 2,
                w: SLIDER_THUMB,
                h: SLIDER_THUMB,
            };
            self.fill_round(pix, &thumb, t.on_primary, SLIDER_THUMB as f32 / 2.0);
            let (c, bw) = if focused {
                (t.secondary, FOCUS_RING)
            } else if hover {
                (t.hover, 1.0)
            } else {
                (t.outline, 1.0)
            };
            self.stroke_round(pix, &thumb, c, SLIDER_THUMB as f32 / 2.0, bw);
        }
        self.hit(&track, action, "");
        48
    }
    pub(super) fn card_frame(&mut self, pix: &mut Pixmap, r: &Rect) {
        let t = self.theme;
        self.fill_round(pix, r, t.variant, RADIUS_XL);
        self.stroke_round(pix, r, t.outline, RADIUS_XL, 1.0);
    }
    /// Card with Noctalia padding; `build` runs twice (measure, then draw) and returns its height.
    pub(super) fn card<F>(&mut self, pix: &mut Pixmap, x: i32, y: i32, w: i32, mut build: F) -> i32
    where
        F: FnMut(&mut Ui, &mut Pixmap, i32, i32, i32) -> i32,
    {
        let pad = CARD_PADDING;
        let (was, was_card) = (self.dry, self.in_card);
        self.dry = true;
        self.in_card = true;
        let inner = build(self, pix, x + pad, y + pad, w - 2 * pad);
        self.dry = was;
        let total = inner + 2 * pad;
        if !self.dry {
            self.card_frame(pix, &Rect { x, y, w, h: total });
            build(self, pix, x + pad, y + pad, w - 2 * pad);
        }
        self.in_card = was_card;
        total
    }
    /// Collapsible card; the header toggles `key`. `build` returns the content height.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn fold<F>(
        &mut self,
        pix: &mut Pixmap,
        x: i32,
        y: i32,
        w: i32,
        key: &str,
        icon: Icon,
        title: &str,
        summary: &str,
        mut build: F,
    ) -> i32
    where
        F: FnMut(&mut Ui, &mut Pixmap, i32, i32, i32) -> i32,
    {
        let t = self.theme;
        let open = self.sections.contains(key);
        let head = 48;
        let pad = CARD_PADDING;
        let (was, was_card) = (self.dry, self.in_card);
        let content = if open {
            self.dry = true;
            self.in_card = true;
            let h = build(self, pix, x + pad, y + head, w - 2 * pad);
            self.dry = was;
            self.in_card = was_card;
            h + pad
        } else {
            0
        };
        let total = head + content;
        let frame = Rect { x, y, w, h: total };
        let header = Rect { x, y, w, h: head };
        let action = Action::Section(key.to_owned());
        let hover = self.is_hover(&action);
        if self.visible(&frame) {
            self.card_frame(pix, &frame);
            if hover {
                self.fill_round(pix, &header, with_alpha(t.hover, 0.16), RADIUS_XL);
            }
            if self.is_focused(&action) {
                self.stroke_round(pix, &frame, t.secondary, RADIUS_XL, FOCUS_RING);
            }
            self.icon(
                pix,
                icon,
                (x + pad + 8) as f32,
                (y + head / 2) as f32,
                16.0,
                if open { t.primary } else { t.on_variant },
            );
            self.text_row(
                pix,
                title,
                (x + pad + 24) as f32,
                y,
                head,
                FONT_BODY,
                true,
                if open { t.primary } else { t.on_surface },
                w / 2,
            );
            let chevron = if open {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            };
            self.icon(
                pix,
                chevron,
                (x + w - pad - 8) as f32,
                (y + head / 2) as f32,
                16.0,
                t.on_variant,
            );
            if !summary.is_empty() {
                self.text_right(
                    pix,
                    summary,
                    x + w - pad - 24,
                    (y, head),
                    FONT_CAPTION,
                    t.on_variant,
                );
            }
        }
        if open && !self.dry {
            self.in_card = true;
            build(self, pix, x + pad, y + head, w - 2 * pad);
            self.in_card = was_card;
        }
        self.hit(&header, action, "");
        total
    }
    /// Vertical overlay scrollbar (6 px, 12 px while hovered/dragged), like Noctalia's.
    pub(super) fn scrollbar(
        &mut self,
        pix: &mut Pixmap,
        track: Rect,
        viewport: i32,
        content: i32,
        offset: i32,
    ) {
        if content <= viewport || self.dry {
            return;
        }
        let t = self.theme;
        let thumb_h = ((viewport as f64 * viewport as f64 / content as f64) as i32)
            .max(32)
            .min(viewport);
        let travel = (viewport - thumb_h).max(1) as f64;
        let thumb_y = track.y + (travel * offset as f64 / (content - viewport) as f64) as i32;
        let hot = self.scroll_drag.is_some() || self.is_hover(&Action::Scrollbar);
        let width = if hot { 12 } else { 6 };
        let thumb = Rect {
            x: track.x + track.w - width - 2,
            y: thumb_y,
            w: width,
            h: thumb_h,
        };
        let saved = (self.clip.clone(), self.origin, self.scrolls);
        self.set_region(
            pix,
            (0, 0),
            Rect {
                x: 0,
                y: 0,
                w: i32::MAX / 4,
                h: i32::MAX / 4,
            },
            false,
        );
        let color = if hot {
            with_alpha(t.on_variant, 0.55)
        } else {
            with_alpha(t.on_variant, 0.35)
        };
        self.fill_round(pix, &thumb, color, width as f32 / 2.0);
        self.hit(
            &Rect {
                x: track.x + track.w - 16,
                y: track.y,
                w: 16,
                h: track.h,
            },
            Action::Scrollbar,
            "",
        );
        self.set_region(pix, saved.1, saved.0, saved.2);
    }
}
