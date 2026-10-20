//! Window chrome and the four pages (collection, playback, layouts, settings).
use super::{
    Action, Ui,
    icons::Icon,
    kit::{Btn, Variant},
    no_count,
    theme::*,
};
use crate::{model::Engine, render::Rect};
use serde_json::json;
use std::path::Path;
use tiny_skia::{Color, Pixmap};

fn rect(x: i32, y: i32, w: i32, h: i32) -> Rect {
    Rect { x, y, w, h }
}
impl Ui {
    fn btn_width(&mut self, label: &str, icon: bool) -> i32 {
        let text = if label.is_empty() {
            0.0
        } else {
            self.measure(label, FONT_BODY, false)
        };
        let icons = if icon {
            16 + if label.is_empty() { 0 } else { SPACE_SM }
        } else {
            0
        };
        ((text.ceil() as i32 + icons + 24).max(CONTROL_SM)).max(if label.is_empty() {
            CONTROL_SM
        } else {
            56
        })
    }
    pub fn draw(&mut self, engine: &Engine, width: u32, height: u32, scale: f64) -> Pixmap {
        let buffer_width = (width as f64 * scale).ceil().max(1.0) as u32;
        let buffer_height = (height as f64 * scale).ceil().max(1.0) as u32;
        let ui_scale = self.theme.style.ui_scale as f64;
        let width = (width as f64 / ui_scale).floor().max(1.0) as u32;
        let height = (height as f64 / ui_scale).floor().max(1.0) as u32;
        let scale = scale * ui_scale;
        self.paint_scale = scale as f32;
        if (self.cache_scale - scale as f32).abs() > 1e-6 {
            self.image_cache.clear();
            self.cache_scale = scale as f32;
        }
        if let Some(face) = self.face.as_mut() {
            face.set_scale(scale as f32);
        }
        let (w, h) = (width as i32, height as i32);
        let mut pix = Pixmap::new(buffer_width, buffer_height).unwrap();
        let t = self.theme;
        self.hits.clear();
        self.list_rect = None;
        self.dry = false;
        let full = rect(0, 0, w, h);
        self.set_region(&pix, (0, 0), full.clone(), false);
        if self.windowed {
            pix.fill(with_alpha(
                t.surface,
                if t.style.window_translucent {
                    t.style.panel_opacity
                } else {
                    1.0
                },
            ));
        } else {
            // Floating panels are rounded Noctalia panels: surface fill, 1 px outline, 12 px radius.
            pix.fill(Color::TRANSPARENT);
            self.fill_round(
                &mut pix,
                &full,
                with_alpha(t.surface, t.style.panel_opacity),
                RADIUS_XL,
            );
            if t.style.panel_borders && t.style.panel_border_width > 0.0 {
                self.stroke_round(
                    &mut pix,
                    &full,
                    with_alpha(t.panel_border, t.style.panel_opacity),
                    RADIUS_XL,
                    t.style.panel_border_width,
                );
            }
        }
        let sidebar = if self.windowed && w >= 680 {
            if w >= 900 { 204 } else { 176 }
        } else {
            0
        };
        if self.windowed && sidebar > 0 {
            self.sidebar(&mut pix, engine, h, sidebar);
        }
        let x = if sidebar > 0 { 12 + sidebar + 20 } else { 20 };
        let cw = (w - x - 20).max(1);
        let titles = ["collection", "starter", "profiles", "settings_tab"];
        let subtitles = [
            "picker_hint",
            "playback_hint",
            "layouts_hint",
            "settings_subtitle",
        ];
        let title = self.tr(titles[self.tab]);
        let subtitle = self.tr(subtitles[self.tab]);
        let header_right = if self.windowed {
            0
        } else {
            self.panel_actions(&mut pix, w)
        };
        let lh_title = self.line_height(FONT_HEADER, true);
        self.text(
            &mut pix,
            &title,
            x as f32,
            18.0,
            FONT_HEADER,
            true,
            t.primary,
            cw - header_right,
        );
        let lh_sub = self.line_height(FONT_CAPTION, false);
        self.text(
            &mut pix,
            &subtitle,
            x as f32,
            (18 + lh_title + 2) as f32,
            FONT_CAPTION,
            false,
            t.on_variant,
            cw - header_right,
        );
        let mut top = 18 + lh_title + 2 + lh_sub + 16;
        if self.windowed && sidebar == 0 {
            let icons = [Icon::Photo, Icon::Sliders, Icon::Layers, Icon::Settings];
            let button_width = ((cw - 3 * SPACE_SM) / 4).max(1);
            for (index, icon) in icons.into_iter().enumerate() {
                let label = self.tr(titles[index]);
                let button = Btn {
                    icon: Some(icon),
                    hint: &label,
                    selected: self.tab == index,
                    ..Btn::default()
                };
                self.button(
                    &mut pix,
                    &rect(
                        x + index as i32 * (button_width + SPACE_SM),
                        top,
                        button_width,
                        CONTROL_SM,
                    ),
                    &button,
                    Action::Tab(index),
                );
            }
            top += CONTROL_SM + SPACE_SM;
        }
        // Errors and notices stay visible until dismissed or the next interaction.
        let error = self.error.clone().or_else(|| engine.error.clone());
        if let Some(message) = error.clone().or_else(|| self.notice.clone()) {
            let is_error = error.is_some();
            let banner = rect(x, top, cw, 40);
            self.fill_round(&mut pix, &banner, t.variant, RADIUS_LG);
            self.stroke_round(
                &mut pix,
                &banner,
                if is_error { t.error } else { t.outline },
                RADIUS_LG,
                1.0,
            );
            self.icon(
                &mut pix,
                if is_error { Icon::Alert } else { Icon::Info },
                (x + 22) as f32,
                (top + 20) as f32,
                18.0,
                if is_error { t.error } else { t.primary },
            );
            let room = cw - 44 - if is_error { 40 } else { 12 };
            self.text_row(
                &mut pix,
                &message,
                (x + 40) as f32,
                top,
                40,
                FONT_BODY,
                false,
                t.on_surface,
                room,
            );
            if is_error {
                let dismiss = Btn {
                    icon: Some(Icon::Close),
                    variant: Variant::Ghost,
                    hint: "",
                    ..Btn::default()
                };
                self.button(
                    &mut pix,
                    &rect(x + cw - 36, top + 4, 32, 32),
                    &dismiss,
                    Action::DismissError,
                );
            }
            top += 52;
        }
        let footer = 28;
        let viewport = (h - top - footer).max(1);
        self.viewport_top = top;
        self.viewport_height = viewport;
        match self.tab {
            0 => self.page_library(&mut pix, engine, x, top, cw, viewport, w),
            1 => self.page_control(&mut pix, engine, x, top, cw, viewport, w),
            2 => self.page_layouts(&mut pix, engine, x, top, cw, viewport, w),
            _ => self.page_settings(&mut pix, engine, x, top, cw, viewport, w),
        }
        self.set_region(&pix, (0, 0), full.clone(), false);
        self.footer(&mut pix, engine, x, h, cw);
        // Hover follows the pointer across redraws (scrolling moves content under a still pointer).
        let (px, py) = self.pointer;
        let hovered = self.hit_at(px, py).map(|i| self.hits[i].action.clone());
        self.dirty = hovered != self.hover_action;
        self.hover_action = hovered;
        if self.pending_tab.is_some() {
            self.finish_pending_tab(engine);
            self.dirty = true;
        }
        pix
    }
    fn sidebar(&mut self, pix: &mut Pixmap, engine: &Engine, h: i32, sw: i32) {
        let t = self.theme;
        let card = rect(12, 12, sw, h - 24);
        self.card_frame(pix, &card);
        self.fill_round(pix, &rect(26, 26, 28, 28), t.primary, RADIUS_MD);
        self.icon(pix, Icon::Play, 40.0, 40.0, 14.0, t.on_primary);
        self.text_row(
            pix,
            "GIF Player",
            62.0,
            26,
            28,
            FONT_TITLE,
            true,
            t.on_surface,
            sw - 60,
        );
        let entries = [
            ("collection", Icon::Photo),
            ("starter", Icon::Sliders),
            ("profiles", Icon::Layers),
            ("settings_tab", Icon::Settings),
        ];
        let active = engine.widgets.len();
        for (index, (key, icon)) in entries.iter().enumerate() {
            let label = self.tr(key);
            let button = Btn {
                label: &label,
                icon: Some(*icon),
                variant: if self.tab == index {
                    Variant::TabActive
                } else {
                    Variant::Tab
                },
                start: true,
                badge: (index == 1 && active > 0).then(|| active.to_string()),
                ..Btn::default()
            };
            self.button(
                pix,
                &rect(20, 72 + index as i32 * 42, sw - 16, 38),
                &button,
                Action::Tab(index),
            );
        }
        self.text_row(
            pix,
            &format!("v{}", env!("CARGO_PKG_VERSION")),
            28.0,
            h - 12 - 14 - 16,
            16,
            FONT_MINI,
            false,
            t.on_variant,
            sw - 32,
        );
    }
    /// Buttons at the right of a floating panel's header; returns the width they occupy.
    fn panel_actions(&mut self, pix: &mut Pixmap, w: i32) -> i32 {
        let other = if self.tab == 0 {
            ("starter", Icon::Sliders, "control")
        } else {
            ("collection", Icon::Photo, "picker")
        };
        let (other_label, settings_label) = (self.tr(other.0), self.tr("settings_tab"));
        let (other_label, settings_label) = if w < 600 {
            (String::new(), String::new())
        } else {
            (other_label, settings_label)
        };
        let widths = [
            self.btn_width(&other_label, true),
            self.btn_width(&settings_label, true),
            CONTROL_SM,
        ];
        let mut right = w - 20;
        let items: [(&str, Option<Icon>, Action); 3] = [
            ("", Some(Icon::Close), Action::Close),
            (
                &settings_label,
                Some(Icon::Settings),
                Action::Command(json!({"action":"settings-ui"})),
            ),
            (
                &other_label,
                Some(other.1),
                Action::Command(json!({"action":other.2})),
            ),
        ];
        let order = [2usize, 1, 0];
        let mut used = 0;
        for (slot, (label, icon, action)) in items.into_iter().enumerate() {
            if action == Action::Close && !self.theme.style.show_close_buttons {
                continue;
            }
            let width = widths[order[slot]];
            right -= width;
            let hint = if label.is_empty() {
                self.tr("close")
            } else {
                String::new()
            };
            let button = Btn {
                label,
                icon,
                hint: &hint,
                ..Btn::default()
            };
            self.button(pix, &rect(right, 14, width, CONTROL_SM), &button, action);
            right -= SPACE_SM;
            used += width + SPACE_SM;
        }
        used + 12
    }
    fn footer(&mut self, pix: &mut Pixmap, engine: &Engine, x: i32, h: i32, cw: i32) {
        let t = self.theme;
        let status = self
            .hover_action
            .as_ref()
            .and_then(|a| match a {
                Action::ToggleGif(path) => Some(format!(
                    "{} · {}",
                    path.file_stem().unwrap_or_default().to_string_lossy(),
                    self.tr(if engine.widgets.values().any(|w| w.path == *path) {
                        "click_stop"
                    } else {
                        "click_play"
                    })
                )),
                _ => self
                    .hits
                    .iter()
                    .find(|hit| Some(&hit.action) == self.hover_action.as_ref())
                    .map(|hit| hit.hint.clone())
                    .filter(|s| !s.is_empty()),
            })
            .unwrap_or_default();
        let count = no_count(
            &self.tr(if engine.widgets.len() == 1 {
                "active.one"
            } else {
                "active.other"
            }),
            engine.widgets.len(),
        );
        let count_w = self.measure(&count, FONT_CAPTION, false).ceil() as i32;
        self.text_row(
            pix,
            &status,
            x as f32,
            h - 26,
            22,
            FONT_CAPTION,
            false,
            t.on_variant,
            cw - count_w - 16,
        );
        self.text_row(
            pix,
            &count,
            (x + cw - count_w) as f32,
            h - 26,
            22,
            FONT_CAPTION,
            false,
            t.on_variant,
            count_w + 2,
        );
    }
    /// Run `body` twice: dry to measure, then drawn inside the scrolling region below `sticky`.
    #[allow(clippy::too_many_arguments)]
    fn scrolled<F>(
        &mut self,
        pix: &mut Pixmap,
        x: i32,
        top: i32,
        w: i32,
        viewport: i32,
        sticky: i32,
        window_w: i32,
        mut body: F,
    ) where
        F: FnMut(&mut Ui, &mut Pixmap, i32) -> i32,
    {
        let view = (viewport - sticky).max(1);
        self.dry = true;
        let content = body(self, pix, w);
        self.dry = false;
        self.content_height = content;
        self.scroll_max = (content - view).max(0);
        self.scroll = self.scroll.clamp(0, self.scroll_max);
        let region = rect(x - 6, top + sticky, w + 12, view);
        self.set_region(pix, (x, top + sticky - self.scroll), region, true);
        body(self, pix, w);
        let track = rect(window_w - 18, top + sticky, 14, view);
        self.viewport_height = view;
        self.viewport_top = top + sticky;
        self.scrollbar(pix, track, view, content, self.scroll);
    }
    fn sticky_region(&mut self, pix: &Pixmap, x: i32, top: i32, w: i32, height: i32) {
        self.set_region(pix, (x, top), rect(x - 6, top, w + 12, height), false);
    }
    // ----- collection -----
    #[allow(clippy::too_many_arguments)]
    fn page_library(
        &mut self,
        pix: &mut Pixmap,
        engine: &Engine,
        x: i32,
        top: i32,
        w: i32,
        viewport: i32,
        window_w: i32,
    ) {
        let t = self.theme;
        self.sticky_region(pix, x, top, w, CONTROL + 12);
        let placeholder = self.tr("search");
        self.input(
            pix,
            &rect(0, 0, w - CONTROL - SPACE_SM, CONTROL),
            "search",
            &placeholder,
            true,
        );
        let refresh = Btn {
            icon: Some(Icon::Refresh),
            hint: &self.tr("refresh"),
            ..Btn::default()
        };
        self.button(
            pix,
            &rect(w - CONTROL, 0, CONTROL, CONTROL),
            &refresh,
            Action::Refresh,
        );
        let sticky = CONTROL + 14;
        self.refilter();
        let count = self.matches.len();
        let gap = SPACE_LG;
        let columns = (((w + gap) / (170 + gap)) as usize).clamp(2, 6);
        let cell = (w - gap * (columns as i32 - 1)) / columns as i32;
        let image_h = ((cell as f32 * 0.62) as i32).clamp(84, 150);
        let row_h = image_h + 8 + 20 + 18;
        self.columns = columns;
        self.row_height = row_h;
        let rows = count.div_ceil(columns);
        let view = (viewport - sticky).max(1);
        self.viewport_height = view;
        self.viewport_top = top + sticky;
        let content = if count == 0 {
            240
        } else {
            rows as i32 * row_h + 8
        };
        self.content_height = content;
        self.scroll_max = (content - view).max(0);
        self.scroll = self.scroll.clamp(0, self.scroll_max);
        let region = rect(x - 6, top + sticky, w + 12, view);
        self.set_region(pix, (x, top + sticky - self.scroll), region, true);
        if count == 0 {
            let empty_files = self.files.is_empty();
            let (title, hint) = (
                self.tr(if empty_files {
                    "collection_empty"
                } else {
                    "search_empty"
                }),
                self.tr("collection_empty_hint"),
            );
            let action_label = self.tr(if empty_files {
                "settings_tab"
            } else {
                "clear_search"
            });
            self.icon(pix, Icon::Photo, (w / 2) as f32, 50.0, 40.0, t.primary);
            let tw = self.measure(&title, FONT_TITLE, true);
            self.text(
                pix,
                &title,
                (w as f32 - tw) / 2.0,
                82.0,
                FONT_TITLE,
                true,
                t.on_surface,
                w,
            );
            let hw = self.measure(&hint, FONT_CAPTION, false);
            self.text(
                pix,
                &hint,
                ((w as f32 - hw) / 2.0).max(0.0),
                110.0,
                FONT_CAPTION,
                false,
                t.on_variant,
                w,
            );
            let bw = self.btn_width(&action_label, false) + 16;
            let button = Btn {
                label: &action_label,
                variant: Variant::Primary,
                ..Btn::default()
            };
            self.button(
                pix,
                &rect((w - bw) / 2, 140, bw, CONTROL),
                &button,
                if empty_files {
                    Action::Tab(3)
                } else {
                    Action::ClearSearch
                },
            );
        } else {
            let first_row = ((self.scroll / row_h) - 1).max(0) as usize;
            let last_row = (((self.scroll + view) / row_h) + 2) as usize;
            let range = (first_row * columns)..count.min(last_row * columns);
            for index in range {
                let path = self.entries[self.matches[index]].path.clone();
                let r = rect(
                    (index % columns) as i32 * (cell + gap),
                    (index / columns) as i32 * row_h,
                    cell,
                    image_h,
                );
                self.tile(pix, engine, &path, &r);
            }
        }
        self.scrollbar(
            pix,
            rect(window_w - 18, top + sticky, 14, view),
            view,
            content,
            self.scroll,
        );
    }
    fn tile(&mut self, pix: &mut Pixmap, engine: &Engine, path: &Path, r: &Rect) {
        let t = self.theme;
        let action = Action::ToggleGif(path.to_path_buf());
        let active = engine
            .widgets
            .values()
            .filter(|widget| widget.path == path)
            .count();
        let hover = self.is_hover(&action);
        let focus = self.is_focused(&action);
        let label_r = rect(r.x, r.y, r.w, r.h + 8 + 20);
        if self.visible(&label_r) {
            self.fill_round(
                pix,
                r,
                with_alpha(t.variant, t.style.card_opacity),
                RADIUS_LG,
            );
            let animation = engine
                .assets
                .get(path)
                .and_then(|a| a.animation.as_ref())
                .or_else(|| engine.thumbnails.get(path))
                .cloned();
            match animation {
                Some(anim) => {
                    let frame = if hover {
                        anim.frame_at(engine.now() * 1000.0).0
                    } else {
                        0
                    };
                    self.image_fit(
                        pix,
                        &anim,
                        frame,
                        &rect(r.x + 2, r.y + 2, r.w - 4, r.h - 4),
                        RADIUS_MD,
                        path,
                    );
                }
                None => self.icon(
                    pix,
                    Icon::Photo,
                    (r.x + r.w / 2) as f32,
                    (r.y + r.h / 2) as f32,
                    28.0,
                    with_alpha(t.on_variant, 0.45),
                ),
            }
            if active > 0 {
                self.stroke_round(pix, r, t.primary, RADIUS_LG, 2.0);
            } else if focus {
                self.stroke_round(pix, r, t.secondary, RADIUS_LG, FOCUS_RING);
            } else if hover {
                self.stroke_round(pix, r, t.hover, RADIUS_LG, 2.0);
            } else {
                self.stroke_round(pix, r, t.outline, RADIUS_LG, 1.0);
            }
            if active > 0 {
                let badge = rect(r.x + r.w - 28, r.y + 6, 22, 22);
                self.fill_round(pix, &badge, t.primary, 11.0);
                if active > 1 {
                    let label = active.to_string();
                    let tw = self.measure(&label, FONT_MINI, true);
                    self.text_row(
                        pix,
                        &label,
                        badge.x as f32 + (22.0 - tw) / 2.0,
                        badge.y,
                        22,
                        FONT_MINI,
                        true,
                        t.on_primary,
                        22,
                    );
                } else {
                    self.icon(
                        pix,
                        Icon::Play,
                        (badge.x + 11) as f32,
                        (badge.y + 11) as f32,
                        12.0,
                        t.on_primary,
                    );
                }
            }
            if hover || focus {
                let words = self.tr(if active > 0 {
                    "click_stop"
                } else {
                    "click_play"
                });
                let tw = self.measure(&words, FONT_CAPTION, true);
                let show_text = (tw as i32 + 44) < r.w;
                let pill_w = if show_text { tw.ceil() as i32 + 44 } else { 32 };
                let pill = rect(r.x + (r.w - pill_w) / 2, r.y + r.h - 34, pill_w, 26);
                self.fill_round(pix, &pill, with_alpha(t.surface, 0.88), 13.0);
                self.icon(
                    pix,
                    if active > 0 { Icon::Stop } else { Icon::Play },
                    (pill.x + 16) as f32,
                    (pill.y + 13) as f32,
                    13.0,
                    t.primary,
                );
                if show_text {
                    self.text_row(
                        pix,
                        &words,
                        (pill.x + 30) as f32,
                        pill.y,
                        26,
                        FONT_CAPTION,
                        true,
                        t.on_surface,
                        pill_w - 34,
                    );
                }
            }
            let name = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            self.text_row(
                pix,
                &name,
                (r.x + 2) as f32,
                r.y + r.h + 6,
                22,
                FONT_CAPTION,
                active > 0,
                if active > 0 { t.primary } else { t.on_surface },
                r.w - 4,
            );
        }
        let name = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        self.hit(&label_r, action, &name);
    }
    // ----- playback -----
    #[allow(clippy::too_many_arguments)]
    fn page_control(
        &mut self,
        pix: &mut Pixmap,
        engine: &Engine,
        x: i32,
        top: i32,
        w: i32,
        viewport: i32,
        window_w: i32,
    ) {
        let t = self.theme;
        if engine.widgets.is_empty() {
            self.set_region(pix, (x, top), rect(x - 6, top, w + 12, viewport), false);
            self.icon(pix, Icon::Photo, (w / 2) as f32, 56.0, 44.0, t.primary);
            let (title, hint, action) = (
                self.tr("no_active_title"),
                self.tr("no_active"),
                self.tr("browse_gifs"),
            );
            let tw = self.measure(&title, FONT_TITLE, true);
            self.text(
                pix,
                &title,
                (w as f32 - tw) / 2.0,
                92.0,
                FONT_TITLE,
                true,
                t.on_surface,
                w,
            );
            let hw = self.measure(&hint, FONT_CAPTION, false);
            self.text(
                pix,
                &hint,
                ((w as f32 - hw) / 2.0).max(0.0),
                120.0,
                FONT_CAPTION,
                false,
                t.on_variant,
                w,
            );
            let bw = self.btn_width(&action, true) + 16;
            let button = Btn {
                label: &action,
                icon: Some(Icon::Photo),
                variant: Variant::Primary,
                ..Btn::default()
            };
            self.button(
                pix,
                &rect((w - bw) / 2, 152, bw, CONTROL),
                &button,
                Action::Tab(0),
            );
            self.content_height = viewport;
            self.scroll_max = 0;
            self.list_scroll_max = 0;
            return;
        }
        let selected = self
            .selected
            .clone()
            .filter(|id| engine.widgets.contains_key(id))
            .or_else(|| engine.widgets.keys().next().cloned())
            .unwrap();
        let wide = w >= 720;
        let list_w = if wide { 208 } else { 0 };
        if wide {
            self.instance_list(
                pix, engine, x, top, list_w, viewport, &selected, viewport, true,
            );
        } else {
            self.list_rect = None;
            self.list_scroll_max = 0;
        }
        let content_x = x + if wide { list_w + 20 } else { 0 };
        let content_w = w - if wide { list_w + 20 } else { 0 };
        let selected_for_body = selected.clone();
        let narrow_list = !wide && engine.widgets.len() > 1;
        self.scrolled(
            pix,
            content_x,
            top,
            content_w,
            viewport,
            0,
            window_w,
            |ui, pix, cw| {
                let mut y = 0;
                if narrow_list {
                    let title = format!("{} · {}", ui.tr("selected_gif"), selected_for_body);
                    y += ui.fold(
                        pix,
                        0,
                        y,
                        cw,
                        "selector",
                        Icon::Layers,
                        &title,
                        "",
                        |ui, pix, x, y, w| {
                            ui.instance_list(
                                pix,
                                engine,
                                x,
                                y,
                                w,
                                6 * 58,
                                &selected_for_body,
                                6 * 58,
                                false,
                            );
                            6 * 58
                        },
                    ) + 12;
                }
                y + ui.control_body(pix, engine, &selected_for_body, cw, y)
            },
        );
    }
    #[allow(clippy::too_many_arguments)]
    fn instance_list(
        &mut self,
        pix: &mut Pixmap,
        engine: &Engine,
        x: i32,
        y: i32,
        w: i32,
        height: i32,
        selected: &str,
        _view: i32,
        primary: bool,
    ) {
        let t = self.theme;
        let item = 52;
        let step = item + 6;
        let total = engine.widgets.len() as i32 * step;
        let max = (total - height).max(0);
        if self.dry {
            return;
        }
        let list_rect = rect(x, y, w, height);
        // The wheel scrolls this list while the pointer is over it (window coordinates).
        self.list_scroll_max = max;
        self.list_rect = Some(self.to_screen(&list_rect));
        let _ = primary;
        self.list_scroll = self.list_scroll.clamp(0, max);
        let saved = (self.clip.clone(), self.origin, self.scrolls);
        let clip = {
            let s = self.to_screen(&list_rect);
            let c = &saved.0;
            let (x0, y0) = (s.x.max(c.x), s.y.max(c.y));
            rect(
                x0,
                y0,
                ((s.x + s.w).min(c.x + c.w) - x0).max(0),
                ((s.y + s.h).min(c.y + c.h) - y0).max(0),
            )
        };
        self.set_region(
            pix,
            (saved.1.0 + x, saved.1.1 + y - self.list_scroll),
            clip,
            saved.2,
        );
        // Virtualised: only rows near the viewport exist, however many instances are running.
        let first = ((self.list_scroll / step) - 1).max(0) as usize;
        let count = (height / step + 4) as usize;
        for (index, widget) in engine.widgets.values().enumerate().skip(first).take(count) {
            let r = rect(0, index as i32 * step, w, item);
            let action = Action::Select(widget.id.clone());
            let on = widget.id == selected;
            if self.visible(&r) {
                let hover = self.is_hover(&action);
                let focus = self.is_focused(&action);
                let (bg, border, fg, sub) = if on {
                    (
                        t.primary,
                        None,
                        t.on_primary,
                        with_alpha(t.on_primary, 0.78),
                    )
                } else if hover {
                    (t.hover, None, t.on_hover, with_alpha(t.on_hover, 0.78))
                } else {
                    (t.variant, Some(t.outline), t.on_surface, t.on_variant)
                };
                self.fill_round(pix, &r, bg, RADIUS_MD + 3.0);
                if focus {
                    self.stroke_round(pix, &r, t.secondary, RADIUS_MD + 3.0, FOCUS_RING);
                } else if let Some(c) = border {
                    self.stroke_round(pix, &r, c, RADIUS_MD + 3.0, 1.0);
                }
                self.thumb(
                    pix,
                    engine,
                    &widget.path,
                    &rect(r.x + 8, r.y + 8, 44, 36),
                    t.surface,
                );
                let state = self.state_label(engine, widget);
                self.text(
                    pix,
                    &widget.id,
                    (r.x + 62) as f32,
                    (r.y + 8) as f32,
                    FONT_BODY,
                    true,
                    fg,
                    w - 70,
                );
                self.text(
                    pix,
                    &state,
                    (r.x + 62) as f32,
                    (r.y + 8 + self.line_height(FONT_BODY, true)) as f32,
                    FONT_MINI,
                    false,
                    sub,
                    w - 70,
                );
            }
            self.hit(&r, action, &widget.id);
        }
        self.set_region(pix, saved.1, saved.0, saved.2);
    }
    fn state_label(&self, engine: &Engine, widget: &crate::model::Widget) -> String {
        let asset = engine.assets.get(&widget.path);
        self.tr(if asset.is_some_and(|a| a.error.is_some()) {
            "load_failed"
        } else if asset.is_none_or(|a| a.animation.is_none()) {
            "loading"
        } else if !widget.settings.visible {
            "hidden"
        } else if widget.settings.paused {
            "paused"
        } else {
            "playing"
        })
    }
    fn thumb(&mut self, pix: &mut Pixmap, engine: &Engine, path: &Path, r: &Rect, backdrop: Color) {
        let t = self.theme;
        self.fill_round(pix, r, backdrop, RADIUS_MD);
        let animation = engine
            .assets
            .get(path)
            .and_then(|a| a.animation.as_ref())
            .or_else(|| engine.thumbnails.get(path))
            .cloned();
        match animation {
            Some(anim) => self.image_fit(
                pix,
                &anim,
                0,
                &rect(r.x + 1, r.y + 1, r.w - 2, r.h - 2),
                RADIUS_SM,
                path,
            ),
            None => self.icon(
                pix,
                Icon::Photo,
                (r.x + r.w / 2) as f32,
                (r.y + r.h / 2) as f32,
                18.0,
                with_alpha(t.on_variant, 0.5),
            ),
        }
    }
    /// Segmented/chip choices that wrap onto further rows; returns the height used.
    fn chips(
        &mut self,
        pix: &mut Pixmap,
        x: i32,
        y: i32,
        w: i32,
        items: &[(String, Option<Icon>, bool, Action)],
    ) -> i32 {
        let (mut cx, mut cy) = (0, 0);
        for (label, icon, selected, action) in items {
            let width = self.btn_width(label, icon.is_some()).min(w);
            if cx > 0 && cx + width > w {
                cx = 0;
                cy += CONTROL_SM + SPACE_SM;
            }
            let button = Btn {
                label,
                icon: *icon,
                selected: *selected,
                ..Btn::default()
            };
            self.button(
                pix,
                &rect(x + cx, y + cy, width, CONTROL_SM),
                &button,
                action.clone(),
            );
            cx += width + SPACE_SM;
        }
        cy + CONTROL_SM
    }
    fn caption(&mut self, pix: &mut Pixmap, text: &str, x: i32, y: i32, w: i32) -> i32 {
        let t = self.theme;
        let lh = self.line_height(FONT_CAPTION, false);
        self.text(
            pix,
            text,
            x as f32,
            y as f32,
            FONT_CAPTION,
            false,
            t.on_variant,
            w,
        );
        lh
    }
    fn control_body(
        &mut self,
        pix: &mut Pixmap,
        engine: &Engine,
        id: &str,
        cw: i32,
        base: i32,
    ) -> i32 {
        let t = self.theme;
        let Some(widget) = engine.widgets.get(id) else {
            return 0;
        };
        let s = &widget.settings;
        let mut y = base;
        // Identity: thumbnail, name, live state and the two utility actions.
        self.thumb(pix, engine, &widget.path, &rect(0, y, 72, 60), t.variant);
        self.text(
            pix,
            &widget.id,
            88.0,
            (y + 6) as f32,
            FONT_TITLE,
            true,
            t.on_surface,
            cw - 88 - 84,
        );
        let state = self.state_label(engine, widget);
        let dot = if s.paused || !s.visible {
            t.on_variant
        } else {
            t.primary
        };
        self.fill_round(pix, &rect(88, y + 36, 8, 8), dot, 4.0);
        self.text_row(
            pix,
            &state,
            104.0,
            y + 30,
            20,
            FONT_CAPTION,
            false,
            t.on_variant,
            cw - 104 - 84,
        );
        let mut duplicate = serde_json::to_value(s).unwrap();
        duplicate["x"] = json!(s.x + 30.0);
        duplicate["y"] = json!(s.y + 30.0);
        let copy = Btn {
            icon: Some(Icon::Copy),
            hint: &self.tr("duplicate"),
            ..Btn::default()
        };
        self.button(
            pix,
            &rect(
                cw - 2 * CONTROL_SM - SPACE_SM,
                y + 14,
                CONTROL_SM,
                CONTROL_SM,
            ),
            &copy,
            Action::Command(json!({"action":"spawn","gif":widget.path,"state":duplicate})),
        );
        let reset = Btn {
            icon: Some(Icon::Reset),
            hint: &self.tr("reset"),
            ..Btn::default()
        };
        self.button(
            pix,
            &rect(cw - CONTROL_SM, y + 14, CONTROL_SM, CONTROL_SM),
            &reset,
            Action::Command(json!({"action":"reset","id":id})),
        );
        y += 76;
        // Primary actions.
        let (play_key, play_icon) = if s.paused {
            ("play", Icon::Play)
        } else {
            ("pause", Icon::Pause)
        };
        let (edit_key, edit_icon) = if s.locked {
            ("edit", Icon::Move)
        } else {
            ("done_editing", Icon::Check)
        };
        let (hide_key, hide_icon) = if s.visible {
            ("hide", Icon::EyeOff)
        } else {
            ("show", Icon::Eye)
        };
        let labels = [
            self.tr(play_key),
            self.tr(edit_key),
            self.tr(hide_key),
            self.tr("stop"),
        ];
        let specs: [(usize, Icon, Variant, bool, Action); 4] = [
            (
                0,
                play_icon,
                Variant::Primary,
                false,
                Action::Command(json!({"action":if s.paused {"play"} else {"pause"},"id":id})),
            ),
            (
                1,
                edit_icon,
                Variant::Default,
                !s.locked,
                Action::Command(json!({"action":if s.locked {"unlock"} else {"lock"},"id":id})),
            ),
            (
                2,
                hide_icon,
                Variant::Default,
                false,
                Action::Command(json!({"action":if s.visible {"hide"} else {"show"},"id":id})),
            ),
            (
                3,
                Icon::Stop,
                Variant::Destructive,
                false,
                Action::Command(json!({"action":"quit","id":id})),
            ),
        ];
        let columns = if cw >= 520 { 4 } else { 2 };
        let bw = (cw - (columns - 1) * SPACE_SM) / columns;
        for (slot, (label, icon, variant, selected, action)) in specs.into_iter().enumerate() {
            let col = slot as i32 % columns;
            let row = slot as i32 / columns;
            let button = Btn {
                label: &labels[label],
                icon: Some(icon),
                variant,
                selected,
                ..Btn::default()
            };
            self.button(
                pix,
                &rect(
                    col * (bw + SPACE_SM),
                    y + row * (CONTROL + SPACE_SM),
                    bw,
                    CONTROL,
                ),
                &button,
                action,
            );
        }
        y += (4 / columns - 1) * (CONTROL + SPACE_SM) + CONTROL + SPACE_MD;
        if !s.locked {
            let hint = self.tr("edit_hint");
            let banner = rect(0, y, cw, 40);
            self.fill_round(pix, &banner, with_alpha(t.primary, 0.14), RADIUS_LG);
            self.stroke_round(pix, &banner, with_alpha(t.primary, 0.55), RADIUS_LG, 1.0);
            self.icon(pix, Icon::Move, 22.0, (y + 20) as f32, 16.0, t.primary);
            self.text_row(
                pix,
                &hint,
                40.0,
                y,
                40,
                FONT_CAPTION,
                false,
                t.on_surface,
                cw - 52,
            );
            y += 40 + SPACE_MD;
        }
        // Quick adjustments.
        let title = self.tr("quick_adjustments");
        let (scale_l, opacity_l, speed_l) =
            (self.tr("scale"), self.tr("opacity"), self.tr("speed"));
        let (scale_v, opacity_v, speed_v) = (s.scale, s.opacity, s.speed);
        let slider = |key: &str, action: &str, min: f64, max: f64, step: f64| Action::Slider {
            id: id.to_owned(),
            action: action.to_owned(),
            key: key.to_owned(),
            min,
            max,
            step,
        };
        y += self.card(pix, 0, y, cw, |ui, pix, x, y0, w| {
            let mut y = y0;
            let th = ui.theme;
            ui.text(
                pix,
                &title,
                x as f32,
                y as f32,
                FONT_BODY,
                true,
                th.on_surface,
                w,
            );
            y += ui.line_height(FONT_BODY, true) + SPACE_MD;
            y += ui.slider(
                pix,
                x,
                y,
                w,
                &scale_l,
                &format!("{:.0}%", scale_v * 100.0),
                scale_v,
                slider("scale", "scale", 0.1, 5.0, 0.01),
            ) + SPACE_SM;
            y += ui.slider(
                pix,
                x,
                y,
                w,
                &opacity_l,
                &format!("{:.0}%", opacity_v * 100.0),
                opacity_v,
                slider("opacity", "opacity", 0.0, 1.0, 0.01),
            ) + SPACE_SM;
            y += ui.slider(
                pix,
                x,
                y,
                w,
                &speed_l,
                &format!("{speed_v:.2}×"),
                speed_v,
                slider("speed", "speed", 0.1, 10.0, 0.05),
            );
            y - y0
        }) + SPACE_MD;
        // Position & monitor.
        let outputs: Vec<String> = engine.outputs.iter().map(|o| o.name.clone()).collect();
        let (px, py, out) = (s.x, s.y, s.output.clone());
        y += self.fold(
            pix,
            0,
            y,
            cw,
            "placement",
            Icon::Center,
            &self.tr("placement"),
            &out,
            |ui, pix, x, y0, w| {
                let mut y = y0 + SPACE_XS;
                let (monitor, center) = (ui.tr("monitor"), ui.tr("center"));
                y += ui.caption(pix, &monitor, x, y, w) + SPACE_SM;
                let items: Vec<_> = outputs
                    .iter()
                    .map(|name| {
                        (
                            name.clone(),
                            Some(Icon::App),
                            *name == out,
                            Action::Command(json!({"action":"output","id":id,"output":name})),
                        )
                    })
                    .collect();
                y += ui.chips(pix, x, y, w, &items) + SPACE_MD;
                if ui.focus.as_deref() != Some("x") {
                    ui.fields.insert("x".into(), format!("{px:.0}"));
                }
                if ui.focus.as_deref() != Some("y") {
                    ui.fields.insert("y".into(), format!("{py:.0}"));
                }
                let half = (w - SPACE_SM) / 2;
                ui.caption(pix, "X", x, y, half);
                ui.caption(pix, "Y", x + half + SPACE_SM, y, half);
                y += 18;
                ui.input(pix, &rect(x, y, half, CONTROL), "x", "", false);
                ui.input(
                    pix,
                    &rect(x + half + SPACE_SM, y, half, CONTROL),
                    "y",
                    "",
                    false,
                );
                y += CONTROL + SPACE_MD;
                let corners = [
                    ("tl", Icon::ArrowUpLeft),
                    ("tr", Icon::ArrowUpRight),
                    ("bl", Icon::ArrowDownLeft),
                    ("br", Icon::ArrowDownRight),
                ];
                let center_w = w - 4 * (CONTROL + SPACE_SM);
                for (i, (pos, icon)) in corners.iter().enumerate() {
                    let hint = ui.tr(&format!("corner_{pos}"));
                    let b = Btn {
                        icon: Some(*icon),
                        hint: &hint,
                        ..Btn::default()
                    };
                    ui.button(
                        pix,
                        &rect(x + i as i32 * (CONTROL + SPACE_SM), y, CONTROL, CONTROL),
                        &b,
                        Action::Command(json!({"action":"corner","id":id,"position":pos})),
                    );
                }
                let b = Btn {
                    label: &center,
                    icon: Some(Icon::Center),
                    ..Btn::default()
                };
                ui.button(
                    pix,
                    &rect(
                        x + 4 * (CONTROL + SPACE_SM),
                        y,
                        center_w.max(CONTROL),
                        CONTROL,
                    ),
                    &b,
                    Action::Command(json!({"action":"center","id":id})),
                );
                y += CONTROL;
                y - y0
            },
        ) + SPACE_MD;
        // Motion effects.
        let (bouncing, jumping, jump_rate) = (s.bouncing, s.jumping, s.jump_rate);
        let motion_summary = if bouncing || jumping {
            self.tr("enabled")
        } else {
            String::new()
        };
        y += self.fold(
            pix,
            0,
            y,
            cw,
            "motion",
            Icon::Activity,
            &self.tr("motion"),
            &motion_summary,
            |ui, pix, x, y0, w| {
                let mut y = y0 + SPACE_XS;
                let (bounce, auto_jump, rate, hop) = (
                    ui.tr("bounce"),
                    ui.tr("auto_jump"),
                    ui.tr("jump_rate"),
                    ui.tr("hop"),
                );
                ui.toggle(
                    pix,
                    &rect(x, y, w, 36),
                    &bounce,
                    "",
                    bouncing,
                    Action::Command(
                        json!({"action":if bouncing {"stop-bounce"} else {"bounce"},"id":id}),
                    ),
                );
                y += 40;
                ui.toggle(
                    pix,
                    &rect(x, y, w, 36),
                    &auto_jump,
                    "",
                    jumping,
                    Action::Command(json!({"action":"jump","id":id})),
                );
                y += 44;
                if jumping {
                    y += ui.slider(
                        pix,
                        x,
                        y,
                        w,
                        &rate,
                        &format!("{jump_rate:.1} s"),
                        jump_rate,
                        slider("seconds", "jump-rate", 0.7, 60.0, 0.1),
                    ) + SPACE_SM;
                }
                let b = Btn {
                    label: &hop,
                    icon: Some(Icon::ArrowUp),
                    ..Btn::default()
                };
                let bw = ui.btn_width(&hop, true) + 16;
                ui.button(
                    pix,
                    &rect(x, y, bw, CONTROL),
                    &b,
                    Action::Command(json!({"action":"hop","id":id})),
                );
                y += CONTROL;
                y - y0
            },
        ) + SPACE_MD;
        // Advanced.
        let (flip_h, flip_v, layer, canvas) = (s.flip_h, s.flip_v, s.layer.clone(), s.canvas);
        let path_text = widget.path.to_string_lossy().into_owned();
        let asset_error = engine
            .assets
            .get(&widget.path)
            .and_then(|a| a.error.clone());
        y += self.fold(
            pix,
            0,
            y,
            cw,
            "advanced",
            Icon::Sliders,
            &self.tr("advanced"),
            "",
            |ui, pix, x, y0, w| {
                let mut y = y0 + SPACE_XS;
                let half = (w - SPACE_SM) / 2;
                let (fh, fv, layer_title, canvas_label, source) = (
                    ui.tr("flip_h"),
                    ui.tr("flip_v"),
                    ui.tr("layer"),
                    ui.tr("canvas"),
                    ui.tr("source"),
                );
                let b = Btn {
                    label: &fh,
                    icon: Some(Icon::FlipH),
                    selected: flip_h,
                    ..Btn::default()
                };
                ui.button(
                    pix,
                    &rect(x, y, half, CONTROL),
                    &b,
                    Action::Command(json!({"action":"flip","id":id,"mode":"toggle-h"})),
                );
                let b = Btn {
                    label: &fv,
                    icon: Some(Icon::FlipV),
                    selected: flip_v,
                    ..Btn::default()
                };
                ui.button(
                    pix,
                    &rect(x + half + SPACE_SM, y, half, CONTROL),
                    &b,
                    Action::Command(json!({"action":"flip","id":id,"mode":"toggle-v"})),
                );
                y += CONTROL + SPACE_MD;
                y += ui.caption(pix, &layer_title, x, y, w) + SPACE_SM;
                let items: Vec<_> = ["overlay", "top", "bottom", "background"]
                    .iter()
                    .map(|value| {
                        (
                            ui.tr(&format!("layer_short_{value}")),
                            None,
                            layer == *value,
                            Action::Command(json!({"action":"layer","id":id,"layer":value})),
                        )
                    })
                    .collect();
                y += ui.chips(pix, x, y, w, &items) + SPACE_MD;
                ui.toggle(
                    pix,
                    &rect(x, y, w, 36),
                    &canvas_label,
                    "",
                    canvas,
                    Action::Command(json!({"action":"canvas","id":id})),
                );
                y += 44;
                y += ui.caption(pix, &source, x, y, w) + 2;
                y += ui.caption(pix, &path_text, x, y, w);
                y - y0
            },
        ) + SPACE_MD;
        if engine.widgets.len() > 1 {
            let paused = engine.widgets.values().all(|w| w.settings.paused);
            let locked = engine.widgets.values().all(|w| w.settings.locked);
            let visible = engine.widgets.values().any(|w| w.settings.visible);
            let count = engine.widgets.len().to_string();
            y += self.fold(
                pix,
                0,
                y,
                cw,
                "all",
                Icon::Layers,
                &self.tr("all_gifs"),
                &count,
                |ui, pix, x, y0, w| {
                    let half = (w - SPACE_SM) / 2;
                    let specs: [(&str, Icon, &str, Variant); 4] = [
                        (
                            if paused { "play_all" } else { "pause_all" },
                            if paused { Icon::Play } else { Icon::Pause },
                            if paused { "play" } else { "pause" },
                            Variant::Default,
                        ),
                        (
                            if locked { "edit_all" } else { "done_editing" },
                            if locked { Icon::Move } else { Icon::Check },
                            if locked { "unlock" } else { "lock" },
                            Variant::Default,
                        ),
                        (
                            if visible { "hide_all" } else { "show_all" },
                            if visible { Icon::EyeOff } else { Icon::Eye },
                            if visible { "hide" } else { "show" },
                            Variant::Default,
                        ),
                        ("stop-all", Icon::Stop, "stop-all", Variant::Destructive),
                    ];
                    for (i, (label, icon, action, variant)) in specs.into_iter().enumerate() {
                        let text = ui.tr(label);
                        let b = Btn {
                            label: &text,
                            icon: Some(icon),
                            variant,
                            ..Btn::default()
                        };
                        ui.button(
                            pix,
                            &rect(
                                x + (i as i32 % 2) * (half + SPACE_SM),
                                y0 + SPACE_XS + (i as i32 / 2) * (CONTROL + SPACE_SM),
                                half,
                                CONTROL,
                            ),
                            &b,
                            Action::Command(json!({"action":action,"id":"*"})),
                        );
                    }
                    SPACE_XS + 2 * CONTROL + SPACE_SM
                },
            ) + SPACE_MD;
        }
        if let Some(error) = asset_error {
            self.icon(pix, Icon::Alert, 8.0, (y + 10) as f32, 16.0, t.error);
            self.text_row(
                pix,
                &error,
                24.0,
                y,
                20,
                FONT_CAPTION,
                false,
                t.error,
                cw - 24,
            );
            y += 28;
        }
        y + SPACE_LG - base
    }
    // ----- layouts -----
    #[allow(clippy::too_many_arguments)]
    fn page_layouts(
        &mut self,
        pix: &mut Pixmap,
        engine: &Engine,
        x: i32,
        top: i32,
        w: i32,
        viewport: i32,
        window_w: i32,
    ) {
        let t = self.theme;
        let sticky_h = 20 + SPACE_SM + CONTROL + SPACE_LG;
        self.sticky_region(pix, x, top, w, sticky_h);
        let title = self.tr("new_layout");
        self.text_row(pix, &title, 0.0, 0, 20, FONT_BODY, true, t.on_surface, w);
        let profiles = engine
            .store
            .profiles
            .as_object()
            .cloned()
            .unwrap_or_default();
        let exists = self
            .fields
            .get("profile")
            .is_some_and(|name| profiles.contains_key(name.trim()));
        let save_label = self.tr(if exists { "overwrite" } else { "save" });
        let save_w = self.btn_width(&save_label, true) + 12;
        let placeholder = self.tr("layout_name");
        self.input(
            pix,
            &rect(0, 28, w - save_w - SPACE_SM, CONTROL),
            "profile",
            &placeholder,
            false,
        );
        let save = Btn {
            label: &save_label,
            icon: Some(Icon::Save),
            variant: Variant::Primary,
            ..Btn::default()
        };
        self.button(
            pix,
            &rect(w - save_w, 28, save_w, CONTROL),
            &save,
            Action::SaveProfile,
        );
        let view = (viewport - sticky_h).max(1);
        self.viewport_height = view;
        self.viewport_top = top + sticky_h;
        let narrow = w < 560;
        let card_h = if narrow { 112 } else { 68 };
        let step = card_h + SPACE_MD;
        let names: Vec<String> = profiles.keys().cloned().collect();
        let content = if names.is_empty() {
            220
        } else {
            names.len() as i32 * step
        };
        self.content_height = content;
        self.scroll_max = (content - view).max(0);
        self.scroll = self.scroll.clamp(0, self.scroll_max);
        let region = rect(x - 6, top + sticky_h, w + 12, view);
        self.set_region(pix, (x, top + sticky_h - self.scroll), region, true);
        if names.is_empty() {
            let (title, hint) = (self.tr("no_layouts"), self.tr("no_layouts_hint"));
            self.icon(pix, Icon::Layers, (w / 2) as f32, 50.0, 40.0, t.primary);
            let tw = self.measure(&title, FONT_TITLE, true);
            self.text(
                pix,
                &title,
                (w as f32 - tw) / 2.0,
                82.0,
                FONT_TITLE,
                true,
                t.on_surface,
                w,
            );
            let hw = self.measure(&hint, FONT_CAPTION, false);
            self.text(
                pix,
                &hint,
                ((w as f32 - hw) / 2.0).max(0.0),
                110.0,
                FONT_CAPTION,
                false,
                t.on_variant,
                w,
            );
        }
        let first = ((self.scroll / step) - 1).max(0) as usize;
        let last = (((self.scroll + view) / step) + 2) as usize;
        for (index, name) in names.iter().enumerate().take(last).skip(first) {
            let name = name.clone();
            let count = profiles[&name]["widgets"].as_array().map_or(0, Vec::len);
            self.layout_card(
                pix,
                &name,
                count,
                &rect(0, index as i32 * step, w, card_h),
                narrow,
            );
        }
        self.scrollbar(
            pix,
            rect(window_w - 18, top + sticky_h, 14, view),
            view,
            content,
            self.scroll,
        );
    }
    fn layout_card(&mut self, pix: &mut Pixmap, name: &str, count: usize, r: &Rect, narrow: bool) {
        let t = self.theme;
        let renaming = self.sections.contains(&format!("rename:{name}"));
        let deleting = self.sections.contains(&format!("delete:{name}"));
        if self.visible(r) {
            self.card_frame(pix, r);
        }
        let inner = rect(
            r.x + CARD_PADDING,
            r.y + CARD_PADDING,
            r.w - 2 * CARD_PADDING,
            r.h - 2 * CARD_PADDING,
        );
        let line = |ui: &mut Ui, first: bool| -> Rect {
            if narrow {
                if first {
                    rect(inner.x, inner.y, inner.w, CONTROL_SM)
                } else {
                    rect(
                        inner.x,
                        inner.y + r.h - 2 * CARD_PADDING - CONTROL_SM,
                        inner.w,
                        CONTROL_SM,
                    )
                }
            } else {
                let _ = ui;
                rect(inner.x, inner.y, inner.w, inner.h)
            }
        };
        let info = line(self, true);
        let actions = line(self, false);
        let sub = no_count(
            &self.tr(if count == 1 {
                "layout_count.one"
            } else {
                "layout_count.other"
            }),
            count,
        );
        let label_w = if narrow {
            info.w
        } else {
            (info.w / 2).max(120)
        };
        if !renaming && !deleting {
            let lh = self.line_height(FONT_BODY, true) + 2 + self.line_height(FONT_CAPTION, false);
            let ty = info.y + (if narrow { CONTROL_SM } else { info.h } - lh) / 2;
            self.text(
                pix,
                name,
                info.x as f32,
                ty as f32,
                FONT_BODY,
                true,
                t.on_surface,
                label_w,
            );
            self.text(
                pix,
                &sub,
                info.x as f32,
                (ty + self.line_height(FONT_BODY, true) + 2) as f32,
                FONT_CAPTION,
                false,
                t.on_variant,
                label_w,
            );
        }
        let mut right = actions.x + actions.w;
        let mid_y = actions.y + (actions.h - CONTROL_SM) / 2;
        if renaming {
            let (save, cancel) = (self.tr("save"), self.tr("cancel"));
            let (sw, cw2) = (self.btn_width(&save, true), self.btn_width(&cancel, false));
            let b = Btn {
                label: &cancel,
                ..Btn::default()
            };
            right -= cw2;
            self.button(
                pix,
                &rect(right, mid_y, cw2, CONTROL_SM),
                &b,
                Action::Cancel(format!("rename:{name}")),
            );
            right -= SPACE_SM + sw;
            let b = Btn {
                label: &save,
                icon: Some(Icon::Check),
                variant: Variant::Primary,
                ..Btn::default()
            };
            self.button(
                pix,
                &rect(right, mid_y, sw, CONTROL_SM),
                &b,
                Action::SaveRename,
            );
            let placeholder = self.tr("rename_name");
            let field = rect(
                if narrow { actions.x } else { info.x },
                mid_y - 3,
                (right - SPACE_SM) - if narrow { actions.x } else { info.x },
                CONTROL,
            );
            self.input(pix, &field, "rename", &placeholder, false);
        } else if deleting {
            let (delete, cancel, ask) = (
                self.tr("delete"),
                self.tr("cancel"),
                self.tr("delete_layout"),
            );
            let (dw, cw2) = (
                self.btn_width(&delete, true),
                self.btn_width(&cancel, false),
            );
            let b = Btn {
                label: &cancel,
                ..Btn::default()
            };
            right -= cw2;
            self.button(
                pix,
                &rect(right, mid_y, cw2, CONTROL_SM),
                &b,
                Action::Cancel(format!("delete:{name}")),
            );
            right -= SPACE_SM + dw;
            let b = Btn {
                label: &delete,
                icon: Some(Icon::Trash),
                variant: Variant::Destructive,
                ..Btn::default()
            };
            self.button(
                pix,
                &rect(right, mid_y, dw, CONTROL_SM),
                &b,
                Action::Command(json!({"action":"profile-delete","name":name})),
            );
            let ask_x = if narrow { actions.x } else { info.x };
            self.icon(
                pix,
                Icon::Alert,
                (ask_x + 8) as f32,
                (mid_y + CONTROL_SM / 2) as f32,
                16.0,
                t.error,
            );
            self.text_row(
                pix,
                &ask,
                (ask_x + 24) as f32,
                mid_y,
                CONTROL_SM,
                FONT_BODY,
                false,
                t.on_surface,
                right - SPACE_SM - ask_x - 24,
            );
        } else {
            let apply = self.tr("apply");
            let aw = self.btn_width(&apply, true);
            let hints = [self.tr("overwrite"), self.tr("rename"), self.tr("delete")];
            let icons = [Icon::Save, Icon::Edit, Icon::Trash];
            let actions = [
                Action::Command(json!({"action":"profile-save","name":name})),
                Action::Rename(name.to_owned()),
                Action::Section(format!("delete:{name}")),
            ];
            // Left to right: Apply, Overwrite, Rename, Delete (the keyboard order matches).
            let icons_w = 3 * CONTROL_SM + 2 * SPACE_XS;
            let mut x = right - icons_w;
            let apply_x = x - SPACE_SM - aw;
            let b = Btn {
                label: &apply,
                icon: Some(Icon::Play),
                variant: Variant::Primary,
                ..Btn::default()
            };
            self.button(
                pix,
                &rect(apply_x, mid_y, aw, CONTROL_SM),
                &b,
                Action::Command(json!({"action":"profile-apply","name":name})),
            );
            for ((icon, hint), action) in icons.into_iter().zip(hints.iter()).zip(actions) {
                let b = Btn {
                    icon: Some(icon),
                    hint,
                    variant: Variant::Ghost,
                    ..Btn::default()
                };
                self.button(pix, &rect(x, mid_y, CONTROL_SM, CONTROL_SM), &b, action);
                x += CONTROL_SM + SPACE_XS;
            }
        }
    }
    // ----- settings -----
    #[allow(clippy::too_many_arguments)]
    fn page_settings(
        &mut self,
        pix: &mut Pixmap,
        engine: &Engine,
        x: i32,
        top: i32,
        w: i32,
        viewport: i32,
        window_w: i32,
    ) {
        let found = self.files.len();
        let level = crate::diagnostics::level_name();
        let restore = engine.store.settings["restore"].as_bool().unwrap_or(true);
        let export = crate::diagnostics::status(&engine.paths)
            .get("last_export")
            .filter(|r| r["ok"] == true && r["pending"] != true)
            .and_then(|r| r["directory"].as_str().map(str::to_owned));
        self.fields
            .entry("directory".into())
            .or_insert_with(|| engine.paths.gif_dir.to_string_lossy().into_owned());
        self.scrolled(pix, x, top, w, viewport, 0, window_w, |ui, pix, w| {
            let th = ui.theme;
            let mut y = 0;
            y += ui.card(pix, 0, y, w, |ui, pix, x, y0, w| {
                let mut y = y0;
                let (title, hint, save, restore_l, restore_h) = (
                    ui.tr("collection"),
                    ui.tr("directory_hint"),
                    ui.tr("save"),
                    ui.tr("restore_short"),
                    ui.tr("restore_hint"),
                );
                ui.text(
                    pix,
                    &title,
                    x as f32,
                    y as f32,
                    FONT_BODY,
                    true,
                    th.on_surface,
                    w,
                );
                y += ui.line_height(FONT_BODY, true) + 2;
                y += ui.caption(pix, &hint, x, y, w) + SPACE_MD;
                let save_w = ui.btn_width(&save, true) + 12;
                ui.input(
                    pix,
                    &rect(x, y, w - save_w - SPACE_SM, CONTROL),
                    "directory",
                    "",
                    false,
                );
                let b = Btn {
                    label: &save,
                    icon: Some(Icon::Check),
                    variant: Variant::Primary,
                    ..Btn::default()
                };
                ui.button(
                    pix,
                    &rect(x + w - save_w, y, save_w, CONTROL),
                    &b,
                    Action::SaveDirectory,
                );
                y += CONTROL + SPACE_SM;
                let count = no_count(&ui.tr("collection_count_n"), found);
                y += ui.caption(pix, &count, x, y, w) + SPACE_MD;
                ui.toggle(
                    pix,
                    &rect(x, y, w, 48),
                    &restore_l,
                    &restore_h,
                    restore,
                    Action::Command(json!({"action":"settings","settings":{"restore":!restore}})),
                );
                y += 48;
                y - y0
            }) + SPACE_MD;
            y += ui.fold(
                pix,
                0,
                y,
                w,
                "diagnostics",
                Icon::Activity,
                &ui.tr("doctor"),
                "",
                |ui, pix, x, y0, w| {
                    let mut y = y0 + SPACE_XS;
                    let (hint, logs, trace, export_l, open_l, done) = (
                        ui.tr("diagnostics_hint"),
                        ui.tr("logs_level"),
                        ui.tr("trace_hint"),
                        ui.tr("logs_export"),
                        ui.tr("open_folder"),
                        ui.tr("export_complete"),
                    );
                    y += ui.caption(pix, &hint, x, y, w) + SPACE_MD;
                    y += ui.caption(pix, &logs, x, y, w) + SPACE_SM;
                    let items: Vec<_> = ["info", "debug", "trace", "warn", "error", "off"]
                        .iter()
                        .map(|value| {
                            (
                                ui.tr(&format!("log_{value}")),
                                None,
                                level == *value,
                                Action::Command(json!({"action":"logs-level","level":value})),
                            )
                        })
                        .collect();
                    y += ui.chips(pix, x, y, w, &items) + SPACE_SM;
                    if level == "trace" {
                        y += ui.caption(pix, &trace, x, y, w) + SPACE_SM;
                    }
                    y += SPACE_XS;
                    let ew = ui.btn_width(&export_l, true) + 12;
                    let b = Btn {
                        label: &export_l,
                        icon: Some(Icon::Folder),
                        variant: Variant::Primary,
                        ..Btn::default()
                    };
                    ui.button(
                        pix,
                        &rect(x, y, ew, CONTROL),
                        &b,
                        Action::Command(json!({"action":"logs-export"})),
                    );
                    if let Some(directory) = &export {
                        let ow = ui.btn_width(&open_l, true) + 12;
                        let b = Btn {
                            label: &open_l,
                            icon: Some(Icon::FolderOpen),
                            ..Btn::default()
                        };
                        ui.button(
                            pix,
                            &rect(x + ew + SPACE_SM, y, ow, CONTROL),
                            &b,
                            Action::OpenFolder(directory.into()),
                        );
                        y += CONTROL + SPACE_SM;
                        y += ui.caption(pix, &done, x, y, w);
                    } else {
                        y += CONTROL;
                    }
                    y - y0
                },
            ) + SPACE_MD;
            y += ui.fold(
                pix,
                0,
                y,
                w,
                "application",
                Icon::App,
                &ui.tr("application"),
                "",
                |ui, pix, x, y0, w| {
                    let y = y0 + SPACE_XS;
                    let (refresh, quit) = (ui.tr("refresh_short"), ui.tr("quit_daemon"));
                    let half = (w - SPACE_SM) / 2;
                    let b = Btn {
                        label: &refresh,
                        icon: Some(Icon::Refresh),
                        ..Btn::default()
                    };
                    ui.button(pix, &rect(x, y, half, CONTROL), &b, Action::Refresh);
                    let b = Btn {
                        label: &quit,
                        icon: Some(Icon::Power),
                        variant: Variant::Destructive,
                        ..Btn::default()
                    };
                    ui.button(
                        pix,
                        &rect(x + half + SPACE_SM, y, half, CONTROL),
                        &b,
                        Action::Command(json!({"action":"quit-daemon"})),
                    );
                    SPACE_XS + CONTROL
                },
            ) + SPACE_MD;
            y + SPACE_LG
        });
    }
}
