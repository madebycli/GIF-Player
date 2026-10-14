use crate::{
    model::Engine,
    render::{Item, Rect, Signature},
};
use anyhow::{Context, Result};
use fontdue::{Font, FontSettings};
use serde_json::{Value, json};
use std::{collections::HashMap, path::PathBuf, process::Command};
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Transform};

#[derive(Clone)]
pub enum Action {
    Command(Value),
    Tab(usize),
    Close,
    Page(i32),
    Select(String),
    Rename(String),
    Field(String),
    Refresh,
    Slider {
        id: String,
        action: String,
        key: String,
        min: f64,
        max: f64,
    },
}
pub struct Hit {
    pub rect: Rect,
    pub action: Action,
}
pub struct Ui {
    pub open: bool,
    pub tab: usize,
    pub dirty: bool,
    pub page: usize,
    pub selected: Option<String>,
    pub fields: HashMap<String, String>,
    pub focus: Option<String>,
    pub hits: Vec<Hit>,
    pub files: Vec<PathBuf>,
    pub hovered: Option<usize>,
    pub active_slider: Option<usize>,
    scroll: i32,
    scroll_max: i32,
    paint_scale: f32,
    pub error: Option<String>,
    font: Option<Font>,
    glyphs: HashMap<(char, u32), (fontdue::Metrics, Vec<u8>)>,
    revision: u64,
}
impl Ui {
    pub fn new() -> Self {
        Self {
            open: false,
            tab: 0,
            dirty: true,
            page: 0,
            selected: None,
            fields: HashMap::new(),
            focus: None,
            hits: Vec::new(),
            files: Vec::new(),
            hovered: None,
            active_slider: None,
            scroll: 0,
            scroll_max: 0,
            paint_scale: 1.0,
            error: None,
            font: None,
            glyphs: HashMap::new(),
            revision: u64::MAX,
        }
    }
    pub fn show(&mut self, mode: &str, engine: &mut Engine) -> Result<()> {
        self.tab = usize::from(mode == "control");
        self.dirty = true;
        self.focus = Some("search".into());
        if self.font.is_none() {
            let chosen = engine
                .store
                .settings
                .get("font_file")
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .or_else(|| {
                    Command::new("fc-match")
                        .args(["-f", "%{file}", "sans-serif"])
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
                })
                .or_else(|| {
                    [
                        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                        "/usr/share/fonts/TTF/DejaVuSans.ttf",
                    ]
                    .iter()
                    .map(PathBuf::from)
                    .find(|p| p.exists())
                })
                .or_else(|| option_env!("GIF_PLAYER_DEFAULT_FONT").map(PathBuf::from));
            let path =
                chosen.context("Standalone UI needs a system font; set settings.json font_file")?;
            self.font = Some(
                Font::from_bytes(std::fs::read(path)?, FontSettings::default())
                    .map_err(|e| anyhow::anyhow!(e))?,
            );
        }
        self.refresh(engine)?;
        self.open = true;
        Ok(())
    }
    pub fn refresh(&mut self, engine: &mut Engine) -> Result<()> {
        self.files = engine.paths.collection()?;
        self.page = 0;
        self.dirty = true;
        Ok(())
    }
    pub fn sync(&mut self, engine: &Engine) {
        if self.revision != engine.revision {
            self.revision = engine.revision;
            self.dirty = true;
        }
        if self
            .selected
            .as_ref()
            .is_some_and(|id| !engine.widgets.contains_key(id))
        {
            self.selected = None;
        }
        if self.tab == 1 && self.selected.is_none() {
            self.selected = engine.widgets.keys().next().cloned();
        }
    }
    fn filtered(&self) -> Vec<PathBuf> {
        let query = self
            .fields
            .get("search")
            .map(|s| s.to_lowercase())
            .unwrap_or_default();
        self.files
            .iter()
            .filter(|p| p.to_string_lossy().to_lowercase().contains(&query))
            .cloned()
            .collect()
    }
    pub fn prepare(&mut self, engine: &mut Engine) {
        if !self.open {
            self.font = None;
            self.glyphs.clear();
            self.files.clear();
            self.hits.clear();
            self.focus = None;
        }
        if !self.open || self.tab != 0 {
            engine.previews.clear();
            engine.gc();
            return;
        }
        let files = self.filtered();
        let page = self.page.min(files.len().saturating_sub(1) / 8);
        self.page = page;
        engine.previews = files.into_iter().skip(page * 8).take(8).collect();
        for path in engine.previews.clone() {
            if let Err(e) = engine.request_thumbnail(path) {
                self.error = Some(e.to_string());
            }
        }
        if self.error.as_deref() == Some("Thumbnail queue is busy")
            && engine.previews.iter().all(|path| {
                engine.thumbnails.contains_key(path) || engine.assets.contains_key(path)
            })
        {
            self.error = None;
        }
        engine.gc();
    }
    pub fn activate(&mut self, action: Action, engine: &mut Engine) {
        match action {
            Action::Command(cmd) => {
                let result = engine.dispatch(&cmd);
                self.error = result
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            Action::Tab(tab) => {
                self.tab = tab;
                self.page = 0;
                self.focus = None;
                self.scroll = 0;
            }
            Action::Close => {
                self.open = false;
            }
            Action::Page(delta) => {
                self.page = (self.page as i32 + delta).max(0) as usize;
            }
            Action::Select(id) => {
                self.selected = Some(id);
                self.fields
                    .retain(|k, _| ["search", "profile", "directory"].contains(&k.as_str()));
            }
            Action::Rename(name) => {
                let new_name = self.fields.get("rename").cloned().unwrap_or_default();
                let result = engine
                    .dispatch(&json!({"action":"profile-rename","name":name,"new_name":new_name}));
                self.error = result
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            Action::Field(key) => {
                self.focus = Some(key);
            }
            Action::Refresh => {
                if let Err(e) = self.refresh(engine) {
                    self.error = Some(e.to_string());
                }
            }
            Action::Slider { .. } => {}
        }
        self.dirty = true;
    }
    pub fn click(&mut self, x: f64, y: f64, engine: &mut Engine) {
        if let Some(index) = self.hits.iter().rposition(|h| h.rect.contains(x, y)) {
            if matches!(self.hits[index].action, Action::Slider { .. }) {
                self.active_slider = Some(index);
                self.slider(x, engine);
            } else {
                self.active_slider = None;
                self.activate(self.hits[index].action.clone(), engine);
            }
        } else {
            self.focus = None;
            self.dirty = true;
        }
    }
    pub fn slider(&mut self, x: f64, engine: &mut Engine) {
        if let Some(index) = self.active_slider {
            if let Some(Hit {
                rect,
                action:
                    Action::Slider {
                        id,
                        action,
                        key,
                        min,
                        max,
                    },
            }) = self.hits.get(index)
            {
                let value =
                    min + (max - min) * ((x - rect.x as f64) / rect.w as f64).clamp(0.0, 1.0);
                let mut cmd = json!({"id":id,"action":action});
                cmd[key] = json!(value);
                let result = engine.dispatch(&cmd);
                self.error = result
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                self.dirty = true;
            }
        }
    }
    pub fn motion(&mut self, x: f64, y: f64) {
        let hovered = self.hits.iter().rposition(|h| h.rect.contains(x, y));
        if hovered != self.hovered {
            self.hovered = hovered;
            self.dirty = true;
        }
    }
    pub fn request_hover(&self, engine: &mut Engine) {
        if let Some(index) = self.hovered {
            if let Some(Hit {
                action: Action::Command(cmd),
                ..
            }) = self.hits.get(index)
            {
                if cmd["action"] == "spawn" {
                    if let Some(path) = cmd["gif"].as_str() {
                        let _ = engine.request_asset(PathBuf::from(path));
                    }
                }
            }
        }
    }
    pub fn preview_deadline(&self, engine: &Engine) -> Option<std::time::Duration> {
        if !self.open || self.tab != 0 {
            return None;
        }
        let index = self.hovered?;
        let Action::Command(cmd) = &self.hits.get(index)?.action else {
            return None;
        };
        if cmd["action"] != "spawn" {
            return None;
        }
        let path = PathBuf::from(cmd["gif"].as_str()?);
        let animation = engine.assets.get(&path)?.animation.as_ref()?;
        if animation.frames.len() < 2 {
            return None;
        }
        Some(std::time::Duration::from_secs_f64(
            animation.frame_at(engine.now() * 1000.0).1 / 1000.0,
        ))
    }
    pub fn text_input(&mut self, text: &str, engine: &mut Engine) {
        if let Some(key) = self.focus.clone() {
            self.fields.entry(key.clone()).or_default().push_str(text);
            if key == "search" {
                self.page = 0;
            }
            self.dirty = true;
            let _ = engine;
        }
    }
    pub fn backspace(&mut self) {
        if let Some(key) = &self.focus {
            self.fields.entry(key.clone()).or_default().pop();
            self.dirty = true;
        }
    }
    pub fn scroll(&mut self, delta: f64) {
        self.scroll = (self.scroll + (delta * 3.0) as i32).clamp(0, self.scroll_max);
        self.active_slider = None;
        self.dirty = true;
    }
    pub fn submit(&mut self, engine: &mut Engine) {
        let key = self.focus.clone().unwrap_or_default();
        let value = self.fields.get(&key).cloned().unwrap_or_default();
        let cmd=match key.as_str(){
            "profile"=>Some(json!({"action":"profile-save","name":value})),"directory"=>Some(json!({"action":"library-directory","path":value})),
            "search"=>self.filtered().get(self.page*8).map(|p|json!({"action":"spawn","gif":p})),
            "x"|"y"=>self.selected.as_ref().and_then(|id|engine.widgets.get(id)).and_then(|w|value.parse::<f64>().ok().map(|n|json!({"action":"move","id":w.id,"x":if key=="x"{n}else{w.settings.x},"y":if key=="y"{n}else{w.settings.y}}))),
            _=>None,
        };
        if let Some(cmd) = cmd {
            self.activate(Action::Command(cmd), engine);
            if key == "directory" {
                let _ = self.refresh(engine);
            }
        }
    }
    fn rect(&mut self, pix: &mut Pixmap, rect: &Rect, color: Color, radius: f32) {
        let mut path = PathBuilder::new();
        let x = rect.x as f32;
        let y = rect.y as f32;
        let w = rect.w as f32;
        let h = rect.h as f32;
        let r = radius.min(w / 2.0).min(h / 2.0);
        path.move_to(x + r, y);
        path.line_to(x + w - r, y);
        path.quad_to(x + w, y, x + w, y + r);
        path.line_to(x + w, y + h - r);
        path.quad_to(x + w, y + h, x + w - r, y + h);
        path.line_to(x + r, y + h);
        path.quad_to(x, y + h, x, y + h - r);
        path.line_to(x, y + r);
        path.quad_to(x, y, x + r, y);
        path.close();
        let mut paint = Paint::default();
        paint.set_color(color);
        pix.fill_path(
            &path.finish().unwrap(),
            &paint,
            tiny_skia::FillRule::Winding,
            Transform::from_scale(self.paint_scale, self.paint_scale),
            None,
        );
    }
    // Raster primitives carry their position, font metrics and clipping width explicitly.
    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        pix: &mut Pixmap,
        text: &str,
        x: i32,
        y: i32,
        size: u32,
        color: Color,
        max: i32,
    ) {
        let mut cursor = x as f32 * self.paint_scale;
        let baseline = (y as f32 * self.paint_scale).round() as i32;
        let size = (size as f32 * self.paint_scale).round().max(1.0) as u32;
        let Some(font) = &self.font else {
            return;
        };
        for ch in text.chars() {
            let (metrics, bitmap) = self
                .glyphs
                .entry((ch, size))
                .or_insert_with(|| font.rasterize(ch, size as f32));
            if cursor + metrics.advance_width > (x + max) as f32 * self.paint_scale {
                break;
            }
            for row in 0..metrics.height {
                for col in 0..metrics.width {
                    let px = cursor as i32 + metrics.xmin + col as i32;
                    let py =
                        baseline + size as i32 - metrics.height as i32 - metrics.ymin + row as i32;
                    if px < 0 || py < 0 || px >= pix.width() as i32 || py >= pix.height() as i32 {
                        continue;
                    }
                    let alpha = bitmap[row * metrics.width + col] as f32 / 255.0 * color.alpha();
                    if alpha == 0.0 {
                        continue;
                    }
                    let at = (py as usize * pix.width() as usize + px as usize) * 4;
                    let bytes = pix.data_mut();
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
            cursor += metrics.advance_width;
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn button(
        &mut self,
        pix: &mut Pixmap,
        label: &str,
        x: i32,
        y: i32,
        w: i32,
        action: Action,
        active: bool,
    ) {
        let rect = Rect { x, y, w, h: 38 };
        let hovered = self.hovered == Some(self.hits.len());
        self.rect(
            pix,
            &rect,
            if active {
                accent()
            } else if hovered {
                Color::from_rgba8(66, 64, 80, 255)
            } else {
                variant()
            },
            6.0,
        );
        self.text(
            pix,
            label,
            x + 12,
            y + 10,
            14,
            if active {
                Color::from_rgba8(22, 21, 32, 255)
            } else {
                foreground()
            },
            w - 24,
        );
        self.hits.push(Hit { rect, action });
    }
    fn input(&mut self, pix: &mut Pixmap, key: &str, hint: &str, x: i32, y: i32, w: i32) {
        let rect = Rect { x, y, w, h: 38 };
        self.rect(pix, &rect, variant(), 6.0);
        let text = self
            .fields
            .get(key)
            .filter(|v| !v.is_empty())
            .map(String::as_str)
            .unwrap_or(hint)
            .to_string();
        self.text(pix, &text, x + 12, y + 10, 14, foreground(), w - 24);
        if self.focus.as_deref() == Some(key) {
            self.rect(
                pix,
                &Rect {
                    x: x + 8,
                    y: y + 34,
                    w: w - 16,
                    h: 2,
                },
                accent(),
                0.0,
            );
        }
        self.hits.push(Hit {
            rect,
            action: Action::Field(key.into()),
        });
    }
    pub fn draw(&mut self, engine: &Engine, width: u32, height: u32, scale: f64) -> Pixmap {
        self.paint_scale = scale as f32;
        let physical_width = (width as f64 * scale).ceil() as u32;
        let physical_height = (height as f64 * scale).ceil() as u32;
        let mut pix = Pixmap::new(physical_width, physical_height).unwrap();
        self.hits.clear();
        let w = width as i32;
        let h = height as i32;
        self.rect(
            &mut pix,
            &Rect { x: 0, y: 0, w, h },
            Color::from_rgba8(26, 25, 35, 250),
            12.0,
        );
        self.text(
            &mut pix,
            "Noctalia GIF Player",
            20,
            18,
            20,
            foreground(),
            w - 80,
        );
        self.button(&mut pix, "×", w - 52, 12, 36, Action::Close, false);
        for (i, title) in ["Collection", "Active", "Profiles", "Settings"]
            .iter()
            .enumerate()
        {
            self.button(
                &mut pix,
                title,
                20 + i as i32 * 126,
                58,
                118,
                Action::Tab(i),
                i == self.tab,
            );
        }
        let all_paused =
            !engine.widgets.is_empty() && engine.widgets.values().all(|w| w.settings.paused);
        let all_locked = engine.widgets.values().all(|w| w.settings.locked);
        for (i, (title, action)) in [
            (
                if all_paused { "Play all" } else { "Pause all" },
                if all_paused { "play" } else { "pause" },
            ),
            (
                if all_locked { "Edit" } else { "Lock" },
                if all_locked { "unlock" } else { "lock" },
            ),
            ("Show all", "show"),
            ("Hide all", "hide"),
            ("Stop all", "stop-all"),
        ]
        .iter()
        .enumerate()
        {
            self.button(
                &mut pix,
                title,
                20 + i as i32 * 116,
                108,
                108,
                Action::Command(json!({"action":action,"id":"*"})),
                false,
            );
        }
        let body_height = h.max(760);
        self.scroll_max = body_height - h;
        self.scroll = self.scroll.clamp(0, self.scroll_max);
        let mut body =
            Pixmap::new(physical_width, (body_height as f64 * scale).ceil() as u32).unwrap();
        body.fill(Color::from_rgba8(26, 25, 35, 250));
        let header_hits = self.hits.len();
        match self.tab {
            0 => self.library(&mut body, engine, w, body_height),
            1 => self.active(&mut body, engine, w, body_height),
            2 => self.profiles(&mut body, engine, w, body_height),
            _ => self.settings(&mut body, engine, w, body_height),
        }
        let body_start = (158.0 * scale).ceil() as u32;
        let body_end = ((h - 32).max(158) as f64 * scale).ceil() as u32;
        let offset = (self.scroll as f64 * scale).round() as u32;
        for y in body_start..body_end {
            let source = ((y + offset) as usize * physical_width as usize) * 4;
            let destination = y as usize * physical_width as usize * 4;
            pix.data_mut()[destination..destination + physical_width as usize * 4]
                .copy_from_slice(&body.data()[source..source + physical_width as usize * 4]);
        }
        let mut body_hits = self.hits.split_off(header_hits);
        for hit in &mut body_hits {
            hit.rect.y -= self.scroll;
            let bottom = (hit.rect.y + hit.rect.h).min(h - 32);
            hit.rect.y = hit.rect.y.max(158);
            hit.rect.h = (bottom - hit.rect.y).max(0);
        }
        self.hits
            .extend(body_hits.into_iter().filter(|hit| hit.rect.h > 0));
        if self.scroll_max > 0 {
            let viewport = h - 190;
            let thumb = (viewport as f64 * h as f64 / body_height as f64).max(24.0) as i32;
            let y = 158 + (viewport - thumb) * self.scroll / self.scroll_max;
            self.rect(
                &mut pix,
                &Rect {
                    x: w - 9,
                    y,
                    w: 4,
                    h: thumb,
                },
                muted(),
                2.0,
            );
        }
        let status = self
            .error
            .as_ref()
            .or(engine.error.as_ref())
            .cloned()
            .unwrap_or_else(|| {
                format!(
                    "{} active · {:.1} MiB decoded · native Wayland",
                    engine.widgets.len(),
                    engine.assets.values().map(|a| a.reserved).sum::<usize>() as f64 / 1048576.0
                )
            });
        self.text(
            &mut pix,
            &status,
            20,
            h - 26,
            11,
            if self.error.is_some() {
                Color::from_rgba8(255, 170, 170, 255)
            } else {
                muted()
            },
            w - 40,
        );
        self.dirty = false;
        pix
    }
    fn library(&mut self, pix: &mut Pixmap, engine: &Engine, w: i32, h: i32) {
        self.input(pix, "search", "Search GIFs and folders…", 20, 158, w - 152);
        self.button(pix, "Refresh", w - 124, 158, 104, Action::Refresh, false);
        let files = self.filtered();
        let cell = (w - 52) / 4;
        let rowheight = ((h - 284) / 2).max(100);
        for (index, path) in files.iter().skip(self.page * 8).take(8).enumerate() {
            let x = 20 + (index % 4) as i32 * (cell + 4);
            let y = 210 + (index / 4) as i32 * rowheight;
            let rect = Rect {
                x,
                y,
                w: cell,
                h: rowheight - 10,
            };
            self.rect(pix, &rect, variant(), 9.0);
            if let Some(anim) = engine
                .assets
                .get(path)
                .and_then(|a| a.animation.as_ref())
                .or_else(|| engine.thumbnails.get(path))
            {
                let factor = ((cell - 24) as f64 / anim.width as f64)
                    .min((rowheight - 62) as f64 / anim.height as f64);
                let rw = (anim.width as f64 * factor).round() as i32;
                let rh = (anim.height as f64 * factor).round() as i32;
                let frame = if self.hovered == Some(self.hits.len()) {
                    anim.frame_at(engine.now() * 1000.0).0
                } else {
                    0
                };
                let item = Item {
                    id: "preview".into(),
                    animation: anim.clone(),
                    signature: Signature {
                        rect: Rect {
                            x: ((x + (cell - rw) / 2) as f32 * self.paint_scale).round() as i32,
                            y: ((y + 8) as f32 * self.paint_scale).round() as i32,
                            w: (rw as f32 * self.paint_scale).round() as i32,
                            h: (rh as f32 * self.paint_scale).round() as i32,
                        },
                        frame,
                        opacity: 1.0,
                        flip_h: false,
                        flip_v: false,
                        editing: false,
                    },
                };
                composite_preview(pix, &item);
            }
            self.text(
                pix,
                &path.file_stem().unwrap_or_default().to_string_lossy(),
                x + 12,
                y + rowheight - 44,
                13,
                foreground(),
                cell - 24,
            );
            let active = engine
                .widgets
                .values()
                .filter(|widget| widget.path == *path)
                .count();
            if active > 0 {
                self.text(
                    pix,
                    &format!("{active} active"),
                    x + 12,
                    y + rowheight - 26,
                    11,
                    accent(),
                    cell - 24,
                );
            }
            self.hits.push(Hit {
                rect,
                action: Action::Command(json!({"action":"spawn","gif":path})),
            });
        }
        if files.is_empty() {
            self.text(pix,"No matching GIFs. Add files to your collection or change its directory in Settings.",20,230,14,muted(),w-40);
        }
        self.button(pix, "Previous", 20, h - 72, 108, Action::Page(-1), false);
        self.button(pix, "Next", 136, h - 72, 108, Action::Page(1), false);
        self.text(
            pix,
            &format!(
                "Page {} / {} · {} GIFs",
                self.page + 1,
                files.len().div_ceil(8).max(1),
                files.len()
            ),
            260,
            h - 62,
            13,
            muted(),
            w - 280,
        );
    }
    fn active(&mut self, pix: &mut Pixmap, engine: &Engine, w: i32, h: i32) {
        let list_width = 220.min(w / 3);
        let visible = ((h - 250) / 46).max(1) as usize;
        let rows = engine
            .widgets
            .values()
            .skip(self.page * visible)
            .take(visible)
            .collect::<Vec<_>>();
        for (i, widget) in rows.iter().enumerate() {
            self.button(
                pix,
                &widget.id,
                20,
                166 + i as i32 * 46,
                list_width,
                Action::Select(widget.id.clone()),
                self.selected.as_deref() == Some(&widget.id),
            );
        }
        self.button(pix, "‹", 20, h - 72, 46, Action::Page(-1), false);
        self.button(pix, "›", 74, h - 72, 46, Action::Page(1), false);
        let Some(widget) = self.selected.as_ref().and_then(|id| engine.widgets.get(id)) else {
            self.text(
                pix,
                "Start a GIF from the collection.",
                list_width + 40,
                176,
                16,
                muted(),
                w - list_width - 60,
            );
            return;
        };
        let x = list_width + 40;
        let cw = w - x - 20;
        let id = &widget.id;
        let s = &widget.settings;
        self.text(pix, &widget.id, x, 162, 16, foreground(), cw);
        self.text(pix, &widget.path.to_string_lossy(), x, 186, 11, muted(), cw);
        for (i, (title, action)) in [
            (
                if s.paused { "Play" } else { "Pause" },
                if s.paused { "play" } else { "pause" },
            ),
            (
                if s.locked { "Unlock" } else { "Lock" },
                if s.locked { "unlock" } else { "lock" },
            ),
            (
                if s.visible { "Hide" } else { "Show" },
                if s.visible { "hide" } else { "show" },
            ),
            ("Stop", "quit"),
        ]
        .iter()
        .enumerate()
        {
            self.button(
                pix,
                title,
                x + i as i32 * (cw / 4),
                212,
                cw / 4 - 6,
                Action::Command(json!({"action":action,"id":id})),
                false,
            );
        }
        self.input(pix, "x", &format!("X: {:.0}", s.x), x, 260, cw / 2 - 6);
        self.input(pix, "y", &format!("Y: {:.0}", s.y), x + cw / 2, 260, cw / 2);
        for (i, (label, action, key, min, max, value)) in [
            ("Size", "scale", "scale", 0.1, 5.0, s.scale),
            ("Opacity", "opacity", "opacity", 0.0, 1.0, s.opacity),
            ("Speed", "speed", "speed", 0.1, 10.0, s.speed),
            (
                "Jump interval",
                "jump-rate",
                "seconds",
                0.7,
                60.0,
                s.jump_rate,
            ),
        ]
        .iter()
        .enumerate()
        {
            let y = 314 + i as i32 * 43;
            self.text(pix, &format!("{label}  {value:.2}"), x, y, 13, muted(), 180);
            let rect = Rect {
                x: x + 188,
                y: y + 4,
                w: cw - 188,
                h: 26,
            };
            self.rect(
                pix,
                &Rect {
                    x: rect.x,
                    y: rect.y + 9,
                    w: rect.w,
                    h: 8,
                },
                variant(),
                4.0,
            );
            let fill = (((value - min) / (max - min)) * rect.w as f64) as i32;
            self.rect(
                pix,
                &Rect {
                    x: rect.x,
                    y: rect.y + 9,
                    w: fill.max(1),
                    h: 8,
                },
                accent(),
                4.0,
            );
            self.hits.push(Hit {
                rect,
                action: Action::Slider {
                    id: id.clone(),
                    action: action.to_string(),
                    key: key.to_string(),
                    min: *min,
                    max: *max,
                },
            });
        }
        for (i, (title, action)) in [
            ("Bounce", "bounce"),
            ("Hop", "hop"),
            ("Auto jump", "jump"),
            ("Reset", "reset"),
        ]
        .iter()
        .enumerate()
        {
            self.button(
                pix,
                title,
                x + i as i32 * (cw / 4),
                490,
                cw / 4 - 6,
                Action::Command(json!({"action":action,"id":id})),
                (*action == "bounce" && s.bouncing) || (*action == "jump" && s.jumping),
            );
        }
        for (i, (title, mode)) in [
            ("Flip H", "toggle-h"),
            ("Flip V", "toggle-v"),
            ("Center", "center"),
            ("Canvas", "canvas"),
        ]
        .iter()
        .enumerate()
        {
            let cmd = match *mode {
                "center" => json!({"action":"corner","id":id,"position":"center"}),
                "canvas" => json!({"action":"canvas","id":id}),
                _ => json!({"action":"flip","id":id,"mode":mode}),
            };
            self.button(
                pix,
                title,
                x + i as i32 * (cw / 4),
                536,
                cw / 4 - 6,
                Action::Command(cmd),
                (*mode == "toggle-h" && s.flip_h)
                    || (*mode == "toggle-v" && s.flip_v)
                    || (*mode == "canvas" && s.canvas),
            );
        }
        for (i, pos) in ["tl", "tr", "bl", "br"].iter().enumerate() {
            self.button(
                pix,
                pos,
                x + i as i32 * 56,
                582,
                48,
                Action::Command(json!({"action":"corner","id":id,"position":pos})),
                false,
            );
        }
        let layer = match s.layer.as_str() {
            "overlay" => "top",
            "top" => "bottom",
            "bottom" => "background",
            _ => "overlay",
        };
        self.button(
            pix,
            &format!("Layer: {}", s.layer),
            x + 240,
            582,
            (cw - 240).max(48),
            Action::Command(json!({"action":"layer","id":id,"layer":layer})),
            false,
        );
        if let Some(output) = engine
            .outputs
            .iter()
            .position(|o| o.name == s.output)
            .or(Some(0))
            .and_then(|i| engine.outputs.get((i + 1) % engine.outputs.len().max(1)))
        {
            self.button(
                pix,
                &format!("Output: {}", s.output),
                x,
                628,
                cw,
                Action::Command(json!({"action":"output","id":id,"output":output.name})),
                false,
            );
        }
    }
    fn profiles(&mut self, pix: &mut Pixmap, engine: &Engine, w: i32, h: i32) {
        self.input(
            pix,
            "profile",
            "Profile name — Enter saves current layout",
            20,
            166,
            w - 40,
        );
        self.input(
            pix,
            "rename",
            "New profile name for Rename",
            20,
            210,
            w - 40,
        );
        let visible = ((h - 330) / 46).max(1) as usize;
        for (i, (name, _)) in engine
            .store
            .profiles
            .as_object()
            .unwrap()
            .iter()
            .skip(self.page * visible)
            .take(visible)
            .enumerate()
        {
            let y = 260 + i as i32 * 46;
            self.button(
                pix,
                name,
                20,
                y,
                w - 400,
                Action::Command(json!({"action":"profile-apply","name":name})),
                false,
            );
            self.button(
                pix,
                "Rename",
                w - 368,
                y,
                108,
                Action::Rename(name.clone()),
                false,
            );
            self.button(
                pix,
                "Overwrite",
                w - 248,
                y,
                108,
                Action::Command(json!({"action":"profile-save","name":name})),
                false,
            );
            self.button(
                pix,
                "Delete",
                w - 128,
                y,
                108,
                Action::Command(json!({"action":"profile-delete","name":name})),
                false,
            );
        }
        self.button(pix, "Previous", 20, h - 72, 108, Action::Page(-1), false);
        self.button(pix, "Next", 136, h - 72, 108, Action::Page(1), false);
    }
    fn settings(&mut self, pix: &mut Pixmap, engine: &Engine, w: i32, _h: i32) {
        self.text(
            pix,
            "Collection directory — type an absolute path and press Enter",
            20,
            170,
            14,
            muted(),
            w - 40,
        );
        self.input(
            pix,
            "directory",
            &engine.paths.gif_dir.to_string_lossy(),
            20,
            202,
            w - 40,
        );
        let restore = engine
            .store
            .settings
            .get("restore")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        self.button(
            pix,
            if restore {
                "Restore at startup: on"
            } else {
                "Restore at startup: off"
            },
            20,
            260,
            300,
            Action::Command(json!({"action":"settings","settings":{"restore":!restore}})),
            restore,
        );
        self.text(
            pix,
            "Overlays never request keyboard focus. Locked surfaces have an empty input region.",
            20,
            326,
            14,
            foreground(),
            w - 40,
        );
        self.text(pix,"Drag unlocked GIFs with the left mouse button. Scroll to resize; right click opens controls.",20,356,14,muted(),w-40);
        self.text(pix,"Overlay stacking above fullscreen windows depends on your compositor and its configuration.",20,400,14,muted(),w-40);
        self.button(
            pix,
            "Reload collection",
            20,
            454,
            180,
            Action::Refresh,
            false,
        );
        self.button(
            pix,
            "Quit daemon",
            212,
            454,
            180,
            Action::Command(json!({"action":"quit-daemon"})),
            false,
        );
    }
}
fn foreground() -> Color {
    Color::from_rgba8(234, 232, 241, 255)
}
fn muted() -> Color {
    Color::from_rgba8(183, 180, 194, 255)
}
fn accent() -> Color {
    Color::from_rgba8(255, 245, 155, 255)
}
fn variant() -> Color {
    Color::from_rgba8(44, 42, 56, 255)
}
fn composite_preview(pix: &mut Pixmap, item: &Item) {
    let anim = &item.animation;
    let frame = &anim.frames[item.signature.frame];
    if let Some(source) = tiny_skia::PixmapRef::from_bytes(&frame.pixels, anim.width, anim.height) {
        let rect = &item.signature.rect;
        pix.draw_pixmap(
            0,
            0,
            source,
            &tiny_skia::PixmapPaint {
                quality: tiny_skia::FilterQuality::Bilinear,
                ..Default::default()
            },
            Transform::from_scale(
                rect.w as f32 / anim.width as f32,
                rect.h as f32 / anim.height as f32,
            )
            .post_translate(rect.x as f32, rect.y as f32),
            None,
        );
    }
}
