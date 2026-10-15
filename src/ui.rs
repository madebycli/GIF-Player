use crate::{
    model::Engine,
    render::{Item, Rect, Signature},
};
use anyhow::{Context, Result};
use fontdue::{Font, FontSettings};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    process::Command,
};
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Transform};

#[derive(Clone, PartialEq)]
pub enum Action {
    Command(Value),
    Section(String),
    Choose(String, Value),
    SaveDirectory,
    SaveProfile,
    SaveRename,
    OpenFolder(PathBuf),
    ClearSearch,
    ToggleGif(PathBuf),
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
    pub windowed: bool,
    mode: String,
    sections: HashSet<String>,
    translations: Value,
    columns: usize,
    content_height: i32,
    keyboard_hit: Option<usize>,
    select_all: bool,
    seen_export: Option<String>,
    theme: Theme,
    appearance: Value,
    viewport_height: i32,
    pub tab: usize,
    pub dirty: bool,
    pub page: usize,
    pub selected: Option<String>,
    pub fields: HashMap<String, String>,
    pub focus: Option<String>,
    pub hits: Vec<Hit>,
    pub files: Vec<PathBuf>,
    library_dir: PathBuf,
    pub hovered: Option<usize>,
    hover_action: Option<Action>,
    pub active_slider: Option<usize>,
    scroll: i32,
    scroll_max: i32,
    paint_scale: f32,
    pub error: Option<String>,
    notice: Option<String>,
    font: Option<Font>,
    glyphs: HashMap<(char, u32), (fontdue::Metrics, Vec<u8>)>,
    revision: u64,
}
impl Ui {
    pub fn new() -> Self {
        Self {
            open: false,
            windowed: false,
            mode: "picker".into(),
            sections: HashSet::new(),
            translations: serde_json::from_str(
                if ["LC_ALL", "LC_MESSAGES", "LANG"]
                    .iter()
                    .filter_map(|key| std::env::var(key).ok())
                    .find(|value| !value.is_empty())
                    .unwrap_or_default()
                    .starts_with("de")
                {
                    include_str!("../plugins/gif-player/translations/de.json")
                } else {
                    include_str!("../plugins/gif-player/translations/en.json")
                },
            )
            .unwrap(),
            columns: 3,
            content_height: 0,
            keyboard_hit: None,
            select_all: false,
            seen_export: None,
            theme: Theme::default(),
            appearance: Value::Null,
            viewport_height: 500,
            tab: 0,
            dirty: true,
            page: 0,
            selected: None,
            fields: HashMap::new(),
            focus: None,
            hits: Vec::new(),
            files: Vec::new(),
            library_dir: PathBuf::new(),
            hovered: None,
            hover_action: None,
            active_slider: None,
            scroll: 0,
            scroll_max: 0,
            paint_scale: 1.0,
            error: None,
            notice: None,
            font: None,
            glyphs: HashMap::new(),
            revision: u64::MAX,
        }
    }
    fn tr(&self, key: &str) -> String {
        self.translations[key].as_str().unwrap_or(key).to_owned()
    }
    pub fn show(&mut self, mode: &str, engine: &mut Engine) -> Result<()> {
        crate::diag!(Info, "ui", "opened", json!({"mode":mode}));
        self.windowed = mode == "app";
        self.mode = mode.into();
        self.tab = match mode {
            "control" => 1,
            "settings" => 3,
            _ => 0,
        };
        self.scroll = 0;
        self.sync_appearance(engine);
        self.load_font(engine)?;

        self.dirty = true;
        self.focus = if self.tab == 0 {
            Some("search".into())
        } else {
            None
        };
        self.keyboard_hit = None;
        self.refresh(engine)?;
        self.open = true;
        Ok(())
    }
    fn sync_appearance(&mut self, engine: &Engine) {
        let appearance = engine
            .store
            .settings
            .get("appearance")
            .cloned()
            .unwrap_or(Value::Null);
        if appearance != self.appearance {
            if appearance["font_family"] != self.appearance["font_family"] {
                self.font = None;
                self.glyphs.clear();
            }
            self.theme = Theme::from_palette(&appearance["palette"]);
            self.appearance = appearance;
            self.dirty = true;
        }
    }
    fn load_font(&mut self, engine: &Engine) -> Result<()> {
        if self.font.is_none() {
            let chosen = engine
                .store
                .settings
                .get("font_file")
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .or_else(|| {
                    Command::new("fc-match")
                        .args([
                            "-f",
                            "%{file}",
                            self.appearance["font_family"]
                                .as_str()
                                .unwrap_or("sans-serif"),
                        ])
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
        Ok(())
    }
    pub fn refresh(&mut self, engine: &mut Engine) -> Result<()> {
        self.files = engine.paths.collection()?;
        self.library_dir = engine.paths.gif_dir.clone();
        self.page = 0;
        self.scroll = 0;
        self.dirty = true;
        Ok(())
    }
    pub fn sync(&mut self, engine: &Engine) {
        if self.open && self.library_dir != engine.paths.gif_dir {
            self.library_dir = engine.paths.gif_dir.clone();
            self.fields.remove("directory");
            match engine.paths.collection() {
                Ok(files) => {
                    self.files = files;
                    self.library_dir = engine.paths.gif_dir.clone();
                    self.scroll = 0;
                    self.dirty = true;
                }
                Err(error) => self.error = Some(error.to_string()),
            }
        }
        self.sync_appearance(engine);
        if self.open {
            if let Err(error) = self.load_font(engine) {
                self.error = Some(error.to_string());
            }
        }
        if self.revision != engine.revision {
            if let Some(report) = crate::diagnostics::status(&engine.paths)
                .get("last_export")
                .filter(|r| r["pending"] != true)
            {
                if let Some(directory) = report["directory"].as_str() {
                    if self.seen_export.as_deref() != Some(directory) {
                        self.seen_export = Some(directory.into());
                        if let Some(error) = report["error"].as_str() {
                            self.error = Some(error.into());
                        } else {
                            self.notice = Some(self.tr("report_done"));
                        }
                    }
                }
            }
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
        let first = (self.scroll / 150).max(0) as usize;
        let count = ((self.viewport_height.max(1) / 150 + 2) as usize * self.columns).min(28);
        engine.previews = files
            .into_iter()
            .skip(first * self.columns)
            .take(count)
            .collect();
        for path in engine.previews.clone() {
            if let Err(e) = engine.request_thumbnail(path) {
                if e.to_string().contains("Thumbnail queue is busy") {
                    break;
                }
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
            Action::ToggleGif(path) => {
                let ids = engine
                    .widgets
                    .values()
                    .filter(|w| w.path == path)
                    .map(|w| w.id.clone())
                    .collect::<Vec<_>>();
                if ids.is_empty() {
                    self.activate(
                        Action::Command(json!({"action":"spawn","gif":path})),
                        engine,
                    );
                } else {
                    for id in ids {
                        self.activate(Action::Command(json!({"action":"quit","id":id})), engine);
                    }
                }
            }
            Action::Command(cmd) => {
                let result = engine.dispatch(&cmd);
                if let Some(directory) = result.get("directory").and_then(Value::as_str) {
                    let _ = directory;
                    self.notice = Some(self.tr("report_exporting"));
                }
                self.error = result
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            Action::Section(key) => {
                if !self.sections.remove(&key) {
                    self.sections.insert(key);
                }
                self.keyboard_hit = None;
            }
            Action::Choose(section, command) => {
                self.sections.remove(&section);
                self.activate(Action::Command(command), engine);
            }
            Action::SaveDirectory => {
                self.focus = Some("directory".into());
                self.submit(engine);
            }
            Action::SaveProfile => {
                self.focus = Some("profile".into());
                self.submit(engine);
            }
            Action::SaveRename => {
                self.focus = Some("rename".into());
                self.submit(engine);
            }
            Action::OpenFolder(path) => {
                if let Err(error) = Command::new("xdg-open")
                    .arg(path)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                {
                    self.error = Some(error.to_string());
                }
            }
            Action::ClearSearch => {
                self.fields.insert("search".into(), String::new());
                self.scroll = 0;
                self.focus = Some("search".into());
            }
            Action::Tab(tab) => {
                self.notice = None;
                self.keyboard_hit = None;
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
                self.sections.remove("selector");
                self.fields
                    .retain(|k, _| ["search", "profile", "directory"].contains(&k.as_str()));
            }
            Action::Rename(name) => {
                self.sections.insert(format!("rename:{name}"));
                self.fields.insert("rename".into(), name);
                self.focus = Some("rename".into());
                self.select_all = true;
            }
            Action::Field(key) => {
                if self.focus.as_ref() != Some(&key) {
                    if key == "directory" {
                        self.fields
                            .insert(key.clone(), engine.paths.gif_dir.to_string_lossy().into());
                    }
                    if let Some(widget) =
                        self.selected.as_ref().and_then(|id| engine.widgets.get(id))
                    {
                        if key == "x" || key == "y" {
                            self.fields.insert(
                                key.clone(),
                                format!(
                                    "{:.0}",
                                    if key == "x" {
                                        widget.settings.x
                                    } else {
                                        widget.settings.y
                                    }
                                ),
                            );
                        }
                    }
                    self.select_all = true;
                }
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
        self.keyboard_hit = None;
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
            self.hover_action = hovered
                .and_then(|i| self.hits.get(i))
                .map(|h| h.action.clone());
            self.dirty = true;
        }
    }
    fn hover_path(&self) -> Option<PathBuf> {
        let hit = self.hits.get(self.hovered?)?;
        match &hit.action {
            Action::ToggleGif(path) => Some(path.clone()),
            _ => None,
        }
    }
    pub fn request_hover(&self, engine: &mut Engine) {
        if let Some(path) = self.hover_path() {
            let _ = engine.request_asset(path);
        }
    }
    pub fn preview_deadline(&self, engine: &Engine) -> Option<std::time::Duration> {
        if !self.open || self.tab != 0 {
            return None;
        }
        let path = self.hover_path()?;
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
            if self.select_all {
                self.fields.insert(key.clone(), String::new());
                self.select_all = false;
            }
            self.fields.entry(key.clone()).or_default().push_str(text);
            if key == "search" {
                self.page = 0;
                self.scroll = 0;
            }
            self.dirty = true;
            let _ = engine;
        }
    }
    pub fn backspace(&mut self) {
        if let Some(key) = &self.focus {
            if self.select_all {
                self.fields.insert(key.clone(), String::new());
                self.select_all = false;
            } else {
                self.fields.entry(key.clone()).or_default().pop();
            }
            if key == "search" {
                self.scroll = 0;
            }
            self.dirty = true;
        }
    }
    pub fn scroll(&mut self, delta: f64) {
        self.scroll = (self.scroll + (delta * 3.0) as i32).clamp(0, self.scroll_max);
        self.active_slider = None;
        self.keyboard_hit = None;
        self.hovered = None;
        self.hover_action = None;
        self.dirty = true;
    }
    pub fn submit(&mut self, engine: &mut Engine) {
        let key = self.focus.clone().unwrap_or_default();
        let value = self.fields.get(&key).cloned().unwrap_or_default();
        if key == "search" {
            if let Some(path) = self.filtered().first() {
                self.activate(Action::ToggleGif(path.clone()), engine);
            }
            return;
        }
        if self.focus.is_none() {
            if let Some(hit) = self
                .keyboard_hit
                .and_then(|i| self.hits.get(i))
                .map(|h| h.action.clone())
            {
                self.activate(hit, engine);
            }
            return;
        }
        let cmd=match key.as_str(){
            "profile" if !value.trim().is_empty()=>Some(json!({"action":"profile-save","name":value.trim()})),
            "directory" if !value.trim().is_empty()=>Some(json!({"action":"library-directory","path":value})),
            "rename"=>self.sections.iter().find_map(|key|key.strip_prefix("rename:")).map(|name|json!({"action":"profile-rename","name":name,"new_name":value})),
            "x"|"y"=>self.selected.as_ref().and_then(|id|engine.widgets.get(id)).and_then(|w|value.parse::<f64>().ok().filter(|v|v.is_finite()).map(|n|json!({"action":"move","id":w.id,"x":if key=="x"{n}else{w.settings.x},"y":if key=="y"{n}else{w.settings.y}}))),
            _=>None,
        };
        if let Some(cmd) = cmd {
            self.activate(Action::Command(cmd), engine);
            if self.error.is_none() {
                if key == "directory" {
                    let _ = self.refresh(engine);
                    self.notice = Some(self.tr("reloaded"));
                }
                if key == "profile" {
                    self.fields.insert("profile".into(), String::new());
                    self.notice = Some(self.tr("layout_saved"));
                }
                if key == "rename" {
                    self.sections.retain(|key| !key.starts_with("rename:"));
                    self.focus = None;
                }
            }
        } else {
            self.error = Some(self.tr(if key == "directory" {
                "directory_hint"
            } else if key == "profile" {
                "profile_name"
            } else {
                "invalid_value"
            }));
            self.dirty = true;
        }
    }
    pub fn focus_next(&mut self, reverse: bool, engine: &mut Engine) {
        if self.hits.is_empty() {
            return;
        }
        let current = self
            .focus
            .as_ref()
            .and_then(|key| {
                self.hits
                    .iter()
                    .position(|h| matches!(&h.action,Action::Field(k) if k==key))
            })
            .or(self.keyboard_hit);
        let index = match current {
            Some(i) if reverse => (i + self.hits.len() - 1) % self.hits.len(),
            Some(i) => (i + 1) % self.hits.len(),
            None if reverse => self.hits.len() - 1,
            None => 0,
        };
        self.focus = None;
        self.keyboard_hit = Some(index);
        if let Action::Field(key) = &self.hits[index].action {
            self.activate(Action::Field(key.clone()), engine);
        }
        self.dirty = true;
    }
    pub fn select_text(&mut self) {
        if self.focus.is_some() {
            self.select_all = true;
            self.dirty = true;
        }
    }
    pub fn adjust_focused_slider(&mut self, direction: f64, engine: &mut Engine) {
        let Some(Action::Slider {
            id,
            action,
            key,
            min,
            max,
        }) = self
            .keyboard_hit
            .and_then(|i| self.hits.get(i))
            .map(|h| h.action.clone())
        else {
            return;
        };
        let Some(widget) = engine.widgets.get(&id) else {
            return;
        };
        let current = if key == "seconds" {
            widget.settings.jump_rate
        } else {
            serde_json::to_value(&widget.settings)
                .ok()
                .and_then(|v| v[&key].as_f64())
                .unwrap_or(min)
        };
        let step = match key.as_str() {
            "opacity" | "scale" => 0.05,
            "speed" => 0.1,
            _ => 0.5,
        };
        let mut command = json!({"action":action,"id":id});
        command[&key] = json!((current + direction * step).clamp(min, max));
        self.activate(Action::Command(command), engine);
    }
    pub fn escape(&mut self) {
        self.focus = None;
        if !self.windowed {
            self.open = false;
        }
        self.keyboard_hit = None;
        self.select_all = false;
        self.dirty = true;
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
    fn outline(&mut self, pix: &mut Pixmap, rect: &Rect, color: Color, radius: f32) {
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
        pix.stroke_path(
            &path.finish().unwrap(),
            &paint,
            &tiny_skia::Stroke {
                width: 1.0,
                ..Default::default()
            },
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
        let hovered = self.hover_action.as_ref() == Some(&action);
        self.rect(
            pix,
            &rect,
            if active {
                self.theme.primary
            } else if hovered {
                self.theme.hover
            } else {
                self.theme.variant
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
                self.theme.on_primary
            } else {
                self.theme.foreground
            },
            w - 24,
        );
        if !active {
            self.outline(
                pix,
                &rect,
                if self.keyboard_hit == Some(self.hits.len()) {
                    self.theme.primary
                } else {
                    self.theme.outline
                },
                6.0,
            );
        }
        self.hits.push(Hit { rect, action });
    }
    fn input(&mut self, pix: &mut Pixmap, key: &str, hint: &str, x: i32, y: i32, w: i32) {
        let rect = Rect { x, y, w, h: 38 };
        self.rect(pix, &rect, self.theme.variant, 6.0);
        self.outline(
            pix,
            &rect,
            if self.focus.as_deref() == Some(key) {
                self.theme.primary
            } else {
                self.theme.outline
            },
            6.0,
        );
        let text = self
            .fields
            .get(key)
            .filter(|v| !v.is_empty())
            .map(String::as_str)
            .unwrap_or(hint)
            .to_string();
        self.text(
            pix,
            &text,
            x + 12,
            y + 10,
            14,
            self.theme.foreground,
            w - 24,
        );
        if self.focus.as_deref() == Some(key) {
            if self.select_all {
                self.outline(
                    pix,
                    &Rect {
                        x: x + 6,
                        y: y + 5,
                        w: w - 12,
                        h: 28,
                    },
                    self.theme.primary,
                    3.0,
                );
            }
            let advance = self
                .font
                .as_ref()
                .map(|font| {
                    text.chars()
                        .map(|c| font.metrics(c, 14.0).advance_width)
                        .sum::<f32>()
                })
                .unwrap_or(0.0) as i32;
            self.rect(
                pix,
                &Rect {
                    x: (x + 12 + advance).min(x + w - 12),
                    y: y + 9,
                    w: 1,
                    h: 20,
                },
                self.theme.foreground,
                0.0,
            );
        }
        self.hits.push(Hit {
            rect,
            action: Action::Field(key.into()),
        });
    }
    fn fold(&mut self, pix: &mut Pixmap, key: &str, title: &str, x: i32, y: i32, w: i32) -> bool {
        let open = self.sections.contains(key);
        let rect = Rect { x, y, w, h: 42 };
        self.rect(pix, &rect, self.theme.variant, 9.0);
        self.outline(pix, &rect, self.theme.outline, 9.0);
        self.text(
            pix,
            title,
            x + 14,
            y + 12,
            14,
            if open {
                self.theme.primary
            } else {
                self.theme.foreground
            },
            w - 56,
        );
        self.text(
            pix,
            if open { "⌄" } else { "›" },
            x + w - 30,
            y + 10,
            17,
            self.theme.muted,
            24,
        );
        self.hits.push(Hit {
            rect,
            action: Action::Section(key.into()),
        });
        open
    }
    #[allow(clippy::too_many_arguments)]
    fn slider_control(
        &mut self,
        pix: &mut Pixmap,
        id: &str,
        key: &str,
        action: &str,
        label: &str,
        display: &str,
        value: f64,
        min: f64,
        max: f64,
        x: i32,
        y: i32,
        w: i32,
    ) {
        self.text(pix, label, x, y, 13, self.theme.foreground, w - 96);
        self.text(pix, display, x + w - 86, y, 13, self.theme.primary, 86);
        let rect = Rect {
            x: x + 8,
            y: y + 22,
            w: w - 16,
            h: 28,
        };
        let fraction = ((value - min) / (max - min)).clamp(0.0, 1.0);
        let at = (fraction * rect.w as f64).round() as i32;
        self.rect(
            pix,
            &Rect {
                x: rect.x,
                y: rect.y + 10,
                w: rect.w,
                h: 6,
            },
            self.theme.outline,
            3.0,
        );
        self.rect(
            pix,
            &Rect {
                x: rect.x,
                y: rect.y + 10,
                w: at.max(1),
                h: 6,
            },
            self.theme.primary,
            3.0,
        );
        self.rect(
            pix,
            &Rect {
                x: rect.x + at - 8,
                y: rect.y + 5,
                w: 16,
                h: 16,
            },
            self.theme.primary,
            8.0,
        );
        self.hits.push(Hit {
            rect,
            action: Action::Slider {
                id: id.into(),
                action: action.into(),
                key: key.into(),
                min,
                max,
            },
        });
    }
    #[allow(clippy::too_many_arguments)]
    fn toggle(
        &mut self,
        pix: &mut Pixmap,
        label: &str,
        hint: &str,
        x: i32,
        y: i32,
        w: i32,
        checked: bool,
        action: Action,
    ) {
        self.text(pix, label, x, y + 2, 14, self.theme.foreground, w - 64);
        if !hint.is_empty() {
            self.text(pix, hint, x, y + 25, 12, self.theme.muted, w - 64);
        }
        let rect = Rect {
            x: x + w - 44,
            y: y + 3,
            w: 44,
            h: 24,
        };
        self.rect(
            pix,
            &rect,
            if checked {
                self.theme.primary
            } else {
                self.theme.outline
            },
            12.0,
        );
        self.rect(
            pix,
            &Rect {
                x: rect.x + if checked { 23 } else { 3 },
                y: rect.y + 3,
                w: 18,
                h: 18,
            },
            if checked {
                self.theme.on_primary
            } else {
                self.theme.foreground
            },
            9.0,
        );
        self.hits.push(Hit {
            rect: Rect { x, y, w, h: 48 },
            action,
        });
    }
    pub fn draw(&mut self, engine: &Engine, width: u32, height: u32, scale: f64) -> Pixmap {
        self.paint_scale = scale as f32;
        let w = width as i32;
        let h = height as i32;
        let mut pix = Pixmap::new(
            (width as f64 * scale).ceil() as u32,
            (height as f64 * scale).ceil() as u32,
        )
        .unwrap();
        pix.fill(self.theme.surface);
        self.hits.clear();
        let side = if self.windowed {
            if w < 800 { 164 } else { 184 }
        } else {
            0
        };
        let top = 94;
        let bottom = 32;
        let viewport = (h - top - bottom).max(1);
        self.viewport_height = viewport;
        let title = self.tr(["collection", "starter", "profiles", "settings_tab"][self.tab]);
        self.text(
            &mut pix,
            &title,
            side + 24,
            20,
            22,
            self.theme.foreground,
            w - side - 180,
        );
        let subtitle = self.tr(match self.tab {
            0 => "picker_hint",
            1 => "playback_hint",
            2 => "layouts_hint",
            _ => "settings_subtitle",
        });
        self.text(
            &mut pix,
            &subtitle,
            side + 24,
            54,
            12,
            self.theme.muted,
            w - side - 48,
        );
        if self.windowed {
            self.text(
                &mut pix,
                "GIF Player",
                18,
                22,
                17,
                self.theme.primary,
                side - 30,
            );
            for (index, key) in ["collection", "starter", "profiles", "settings_tab"]
                .iter()
                .enumerate()
            {
                self.button(
                    &mut pix,
                    &self.tr(key),
                    12,
                    94 + index as i32 * 48,
                    side - 24,
                    Action::Tab(index),
                    self.tab == index,
                );
            }
            self.text(
                &mut pix,
                &no_count(
                    &self.tr(if engine.widgets.len() == 1 {
                        "active.one"
                    } else {
                        "active.other"
                    }),
                    engine.widgets.len(),
                ),
                18,
                h - 60,
                11,
                self.theme.muted,
                side - 30,
            );
        } else {
            let other = if self.tab == 0 { "control" } else { "picker" };
            self.button(
                &mut pix,
                &self.tr(if self.tab == 0 {
                    "starter"
                } else {
                    "collection"
                }),
                w - 372,
                16,
                132,
                Action::Command(json!({"action":other})),
                false,
            );
            self.button(
                &mut pix,
                &self.tr("settings_tab"),
                w - 232,
                16,
                148,
                Action::Command(json!({"action":"settings-ui"})),
                false,
            );
            self.button(&mut pix, "×", w - 70, 16, 46, Action::Close, false);
        }
        let body_w = (w - side - 48).max(1);
        self.columns = if body_w >= 1000 {
            5
        } else if body_w >= 760 {
            4
        } else if body_w >= 540 {
            3
        } else {
            2
        };
        let virtualized = self.tab == 0 || self.tab == 2;
        let allocation = if virtualized {
            viewport
        } else {
            viewport.max(900 + self.sections.len().min(8) as i32 * 320)
        };
        let mut body = Pixmap::new(
            (body_w as f64 * scale).ceil() as u32,
            (allocation as f64 * scale).ceil() as u32,
        )
        .unwrap();
        body.fill(self.theme.surface);
        let header_hits = self.hits.len();
        self.content_height = match self.tab {
            0 => self.library(&mut body, engine, body_w, viewport),
            1 => self.active(&mut body, engine, body_w, viewport),
            2 => self.profiles(&mut body, engine, body_w, viewport),
            _ => self.settings(&mut body, engine, body_w),
        };
        self.scroll_max = (self.content_height - viewport).max(0);
        self.scroll = self.scroll.clamp(0, self.scroll_max);
        let offset = if virtualized { 0 } else { self.scroll };
        let top_px = (top as f64 * scale).round() as usize;
        let left = ((side + 24) as f64 * scale).round() as usize;
        let stride = body.width() as usize * 4;
        let source_y = (offset as f64 * scale).round() as usize;
        let rows = ((viewport as f64 * scale).ceil() as usize)
            .min(body.height() as usize - source_y)
            .min(pix.height() as usize - top_px);
        for row in 0..rows {
            let source = (source_y + row) * stride;
            let destination = ((top_px + row) * pix.width() as usize + left) * 4;
            let len = stride.min(pix.width() as usize * 4 - left * 4);
            pix.data_mut()[destination..destination + len]
                .copy_from_slice(&body.data()[source..source + len]);
        }
        let mut hits = self.hits.split_off(header_hits);
        for hit in &mut hits {
            hit.rect.x += side + 24;
            hit.rect.y -= offset;
            let end = (hit.rect.y + hit.rect.h).min(viewport);
            hit.rect.y = hit.rect.y.max(0);
            hit.rect.h = (end - hit.rect.y).max(0);
            hit.rect.y += top;
        }
        self.hits
            .extend(hits.into_iter().filter(|h| h.rect.h > 0 && h.rect.w > 0));
        if self.scroll_max > 0 {
            let thumb = (viewport as f64 * viewport as f64 / (viewport + self.scroll_max) as f64)
                .max(24.0) as i32;
            self.rect(
                &mut pix,
                &Rect {
                    x: w - 8,
                    y: top + (viewport - thumb) * self.scroll / self.scroll_max,
                    w: 3,
                    h: thumb,
                },
                self.theme.outline,
                2.0,
            );
        }
        let hovered = self.hovered.and_then(|i| self.hits.get(i)).and_then(|hit| {
            if let Action::ToggleGif(path) = &hit.action {
                Some(format!(
                    "{} · {}",
                    path.file_stem().unwrap_or_default().to_string_lossy(),
                    self.tr(
                        if engine.widgets.values().any(|widget| widget.path == *path) {
                            "click_stop"
                        } else {
                            "click_play"
                        }
                    )
                ))
            } else {
                None
            }
        });
        let status = self
            .error
            .as_ref()
            .or(engine.error.as_ref())
            .or(self.notice.as_ref())
            .cloned()
            .or(hovered)
            .unwrap_or_else(|| {
                if self.windowed {
                    String::new()
                } else {
                    no_count(
                        &self.tr(if engine.widgets.len() == 1 {
                            "active.one"
                        } else {
                            "active.other"
                        }),
                        engine.widgets.len(),
                    )
                }
            });
        self.text(
            &mut pix,
            &status,
            side + 24,
            h - 24,
            11,
            if self.error.is_some() {
                self.theme.primary
            } else {
                self.theme.muted
            },
            w - side - 48,
        );
        self.dirty = false;
        pix
    }
    fn library(&mut self, pix: &mut Pixmap, engine: &Engine, w: i32, h: i32) -> i32 {
        let files = self.filtered();
        let columns = self.columns;
        let cell = (w - (columns as i32 - 1) * 16) / columns as i32;
        let first = (self.scroll / 150).max(0) as usize;
        let rows = (h / 150 + 2).max(1) as usize;
        for (index, path) in files
            .iter()
            .enumerate()
            .skip(first * columns)
            .take(rows * columns)
        {
            let x = (index % columns) as i32 * (cell + 16);
            let y = 64 + (index / columns) as i32 * 150 - self.scroll;
            if let Some(anim) = engine
                .assets
                .get(path)
                .and_then(|a| a.animation.as_ref())
                .or_else(|| engine.thumbnails.get(path))
            {
                let factor =
                    ((cell - 12) as f64 / anim.width as f64).min(108.0 / anim.height as f64);
                let rw = (anim.width as f64 * factor).round() as i32;
                let rh = (anim.height as f64 * factor).round() as i32;
                let frame = if self.hover_action.as_ref() == Some(&Action::ToggleGif(path.clone()))
                {
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
                            y: ((y + (108 - rh) / 2) as f32 * self.paint_scale).round() as i32,
                            w: (rw as f32 * self.paint_scale).round() as i32,
                            h: (rh as f32 * self.paint_scale).round() as i32,
                        },
                        frame,
                        opacity: 1.0,
                        flip_h: false,
                        flip_v: false,
                        editing: false,
                        edit_color: 0,
                    },
                };
                composite_preview(pix, &item);
            }
            let active = engine
                .widgets
                .values()
                .filter(|widget| widget.path == *path)
                .count();
            if active > 0 {
                self.outline(
                    pix,
                    &Rect {
                        x,
                        y,
                        w: cell,
                        h: 108,
                    },
                    self.theme.primary,
                    9.0,
                );
            }
            self.text(
                pix,
                &path.file_stem().unwrap_or_default().to_string_lossy(),
                x + 4,
                y + 118,
                13,
                if active > 0 {
                    self.theme.primary
                } else {
                    self.theme.muted
                },
                cell - 8,
            );
            let top = y.max(64);
            let end = (y + 140).min(h);
            if end > top {
                self.hits.push(Hit {
                    rect: Rect {
                        x,
                        y: top,
                        w: cell,
                        h: end - top,
                    },
                    action: Action::ToggleGif(path.clone()),
                });
            }
        }
        // Paint the fixed search row last, so partially visible images never cover it.
        self.rect(
            pix,
            &Rect {
                x: 0,
                y: 0,
                w,
                h: 60,
            },
            self.theme.surface,
            0.0,
        );
        self.input(pix, "search", &self.tr("search"), 0, 0, w - 120);
        let query = self.fields.get("search").is_some_and(|v| !v.is_empty());
        self.button(
            pix,
            &self.tr(if query {
                "clear_search_short"
            } else {
                "refresh_short"
            }),
            w - 108,
            0,
            108,
            if query {
                Action::ClearSearch
            } else {
                Action::Refresh
            },
            false,
        );
        if files.is_empty() {
            self.text(
                pix,
                &self.tr(if self.files.is_empty() {
                    "collection_empty"
                } else {
                    "search_empty"
                }),
                0,
                104,
                20,
                self.theme.foreground,
                w,
            );
            self.text(
                pix,
                &self.tr("collection_empty_hint"),
                0,
                140,
                13,
                self.theme.muted,
                w,
            );
            self.button(
                pix,
                &self.tr(if self.files.is_empty() {
                    "settings_tab"
                } else {
                    "clear_search"
                }),
                0,
                184,
                180,
                if self.files.is_empty() {
                    Action::Tab(3)
                } else {
                    Action::ClearSearch
                },
                true,
            );
        }
        (files.len().div_ceil(columns) as i32 * 150 + 64).max(h)
    }
    fn active(&mut self, pix: &mut Pixmap, engine: &Engine, w: i32, h: i32) -> i32 {
        if engine.widgets.is_empty() {
            self.text(
                pix,
                &self.tr("no_active_title"),
                0,
                28,
                24,
                self.theme.foreground,
                w,
            );
            self.text(pix, &self.tr("no_active"), 0, 72, 14, self.theme.muted, w);
            self.button(
                pix,
                &self.tr("browse_gifs"),
                0,
                122,
                200,
                Action::Tab(0),
                true,
            );
            return 184;
        }
        let list = if w >= 720 { 180 } else { 0 };
        let x = if list > 0 { list + 24 } else { 0 };
        let cw = w - x;
        if list > 0 {
            self.text(
                pix,
                &self.tr("active_tab"),
                0,
                0,
                13,
                self.theme.muted,
                list,
            );
            let per_page = ((h - 110) / 48).max(1) as usize;
            self.page = self
                .page
                .min(engine.widgets.len().saturating_sub(1) / per_page);
            for (i, widget) in engine
                .widgets
                .values()
                .skip(self.page * per_page)
                .take(per_page)
                .enumerate()
            {
                self.button(
                    pix,
                    &widget.id,
                    0,
                    30 + i as i32 * 48,
                    list,
                    Action::Select(widget.id.clone()),
                    self.selected.as_deref() == Some(&widget.id),
                );
            }
            if engine.widgets.len() > per_page {
                self.button(pix, "‹", 0, h - 54, 50, Action::Page(-1), false);
                self.button(pix, "›", 58, h - 54, 50, Action::Page(1), false);
            }
        }
        let selected = self.selected.clone();
        let Some(widget) = selected.as_ref().and_then(|id| engine.widgets.get(id)) else {
            return h;
        };
        let id = &widget.id;
        let s = &widget.settings;
        let mut y = 0;
        if list == 0 && engine.widgets.len() > 1 {
            if self.fold(
                pix,
                "selector",
                &format!("{} · {}", self.tr("selected_gif"), id),
                x,
                y,
                cw,
            ) {
                y += 50;
                self.page = self.page.min(engine.widgets.len().saturating_sub(1) / 8);
                for item in engine.widgets.values().skip(self.page * 8).take(8) {
                    self.button(
                        pix,
                        &item.id,
                        x,
                        y,
                        cw,
                        Action::Select(item.id.clone()),
                        item.id == *id,
                    );
                    y += 44;
                }
                if engine.widgets.len() > 8 {
                    self.button(pix, "‹", x, y, 50, Action::Page(-1), false);
                    self.button(pix, "›", x + 58, y, 50, Action::Page(1), false);
                    y += 48;
                }
            } else {
                y += 50;
            }
        }
        self.text(pix, id, x, y, 21, self.theme.foreground, cw);
        y += 32;
        self.text(
            pix,
            &self.tr(if !s.visible {
                "hidden"
            } else if s.paused {
                "paused"
            } else {
                "playing"
            }),
            x,
            y,
            12,
            self.theme.muted,
            cw,
        );
        y += 30;
        let columns = if cw >= 520 { 4 } else { 2 };
        let bw = (cw - (columns - 1) * 8) / columns;
        for (i, (label, action, active)) in [
            (
                if s.paused { "play" } else { "pause" },
                if s.paused { "play" } else { "pause" },
                true,
            ),
            (
                if s.locked { "edit" } else { "done_editing" },
                if s.locked { "unlock" } else { "lock" },
                !s.locked,
            ),
            (
                if s.visible { "hide" } else { "show" },
                if s.visible { "hide" } else { "show" },
                false,
            ),
            ("stop", "quit", false),
        ]
        .iter()
        .enumerate()
        {
            self.button(
                pix,
                &self.tr(label),
                x + (i as i32 % columns) * (bw + 8),
                y + (i as i32 / columns) * 46,
                bw,
                Action::Command(json!({"action":action,"id":id})),
                *active,
            );
        }
        y += if columns == 4 { 54 } else { 100 };
        if !s.locked {
            self.text(
                pix,
                &self.tr("edit_hint_short"),
                x,
                y,
                12,
                self.theme.primary,
                cw,
            );
            y += 38;
        }
        for (label, key, action, min, max, value, display) in [
            (
                "scale",
                "scale",
                "scale",
                0.1,
                5.0,
                s.scale,
                format!("{:.0}%", s.scale * 100.0),
            ),
            (
                "opacity",
                "opacity",
                "opacity",
                0.0,
                1.0,
                s.opacity,
                format!("{:.0}%", s.opacity * 100.0),
            ),
            (
                "speed",
                "speed",
                "speed",
                0.1,
                10.0,
                s.speed,
                format!("{:.2}×", s.speed),
            ),
        ] {
            self.slider_control(
                pix,
                id,
                key,
                action,
                &self.tr(label),
                &display,
                value,
                min,
                max,
                x,
                y,
                cw,
            );
            y += 68;
        }
        let open = self.fold(pix, "placement", &self.tr("placement"), x, y, cw);
        y += 50;
        if open {
            self.text(pix, "X", x, y, 12, self.theme.muted, cw / 2);
            self.text(pix, "Y", x + cw / 2 + 4, y, 12, self.theme.muted, cw / 2);
            y += 22;
            self.input(pix, "x", &format!("{:.0}", s.x), x, y, cw / 2 - 4);
            self.input(
                pix,
                "y",
                &format!("{:.0}", s.y),
                x + cw / 2 + 4,
                y,
                cw / 2 - 4,
            );
            y += 48;
            let width = (cw - 32) / 5;
            for (i, (label, pos)) in [
                ("↖", "tl"),
                ("↗", "tr"),
                ("↙", "bl"),
                ("↘", "br"),
                (&self.tr("center"), "center"),
            ]
            .iter()
            .enumerate()
            {
                self.button(
                    pix,
                    label,
                    x + i as i32 * (width + 8),
                    y,
                    width,
                    Action::Command(json!({"action":"corner","id":id,"position":pos})),
                    false,
                );
            }
            y += 50;
            let show = self.fold(
                pix,
                "outputs",
                &format!("{} · {}", self.tr("monitor"), s.output),
                x,
                y,
                cw,
            );
            y += 50;
            if show {
                for output in &engine.outputs {
                    self.button(
                        pix,
                        &output.name,
                        x,
                        y,
                        cw,
                        Action::Choose(
                            "outputs".into(),
                            json!({"action":"output","id":id,"output":output.name}),
                        ),
                        s.output == output.name,
                    );
                    y += 44;
                }
            }
            y += 8;
        }
        let open = self.fold(pix, "motion", &self.tr("motion"), x, y, cw);
        y += 50;
        if open {
            self.toggle(
                pix,
                &self.tr("bounce"),
                "",
                x,
                y,
                cw,
                s.bouncing,
                Action::Command(
                    json!({"action":if s.bouncing{"stop-bounce"}else{"bounce"},"id":id}),
                ),
            );
            y += 44;
            self.toggle(
                pix,
                &self.tr("auto_jump"),
                "",
                x,
                y,
                cw,
                s.jumping,
                Action::Command(json!({"action":"jump","id":id})),
            );
            y += 44;
            if s.jumping {
                self.slider_control(
                    pix,
                    id,
                    "seconds",
                    "jump-rate",
                    &self.tr("jump_rate"),
                    &format!("{:.1} s", s.jump_rate),
                    s.jump_rate,
                    0.7,
                    60.0,
                    x,
                    y,
                    cw,
                );
                y += 68;
            }
            self.button(
                pix,
                &self.tr("hop"),
                x,
                y,
                132,
                Action::Command(json!({"action":"hop","id":id})),
                false,
            );
            y += 50;
        }
        let open = self.fold(pix, "advanced", &self.tr("advanced"), x, y, cw);
        y += 50;
        if open {
            self.button(
                pix,
                &self.tr("flip_h"),
                x,
                y,
                cw / 2 - 4,
                Action::Command(json!({"action":"flip","id":id,"mode":"toggle-h"})),
                s.flip_h,
            );
            self.button(
                pix,
                &self.tr("flip_v"),
                x + cw / 2 + 4,
                y,
                cw / 2 - 4,
                Action::Command(json!({"action":"flip","id":id,"mode":"toggle-v"})),
                s.flip_v,
            );
            y += 50;
            let show = self.fold(
                pix,
                "layers",
                &format!(
                    "{} · {}",
                    self.tr("layer"),
                    self.tr(&format!("layer_{}", s.layer))
                ),
                x,
                y,
                cw,
            );
            y += 50;
            if show {
                for value in ["overlay", "top", "bottom", "background"] {
                    self.button(
                        pix,
                        &self.tr(&format!("layer_{value}")),
                        x,
                        y,
                        cw,
                        Action::Choose(
                            "layers".into(),
                            json!({"action":"layer","id":id,"layer":value}),
                        ),
                        s.layer == value,
                    );
                    y += 44;
                }
            }
            self.toggle(
                pix,
                &self.tr("canvas"),
                "",
                x,
                y,
                cw,
                s.canvas,
                Action::Command(json!({"action":"canvas","id":id})),
            );
            y += 48;
            let mut duplicate = serde_json::to_value(s).unwrap();
            duplicate["x"] = json!(s.x + 30.0);
            duplicate["y"] = json!(s.y + 30.0);
            self.button(
                pix,
                &self.tr("duplicate"),
                x,
                y,
                cw / 2 - 4,
                Action::Command(json!({"action":"spawn","gif":widget.path,"state":duplicate})),
                false,
            );
            self.button(
                pix,
                &self.tr("reset"),
                x + cw / 2 + 4,
                y,
                cw / 2 - 4,
                Action::Command(json!({"action":"reset","id":id})),
                false,
            );
            y += 50;
            self.text(
                pix,
                &widget.path.to_string_lossy(),
                x,
                y,
                11,
                self.theme.muted,
                cw,
            );
            y += 32;
        }
        if engine.widgets.len() > 1 {
            let open = self.fold(pix, "all", &self.tr("all_gifs"), x, y, cw);
            y += 50;
            if open {
                let paused = engine.widgets.values().all(|w| w.settings.paused);
                let locked = engine.widgets.values().all(|w| w.settings.locked);
                let visible = engine.widgets.values().any(|w| w.settings.visible);
                for (i, (label, action)) in [
                    (
                        if paused { "play_all" } else { "pause_all" },
                        if paused { "play" } else { "pause" },
                    ),
                    (
                        if locked { "edit_all" } else { "done_editing" },
                        if locked { "unlock" } else { "lock" },
                    ),
                    (
                        if visible { "hide_all" } else { "show_all" },
                        if visible { "hide" } else { "show" },
                    ),
                    ("stop-all", "stop-all"),
                ]
                .iter()
                .enumerate()
                {
                    self.button(
                        pix,
                        &self.tr(label),
                        x + (i % 2) as i32 * (cw / 2 + 4),
                        y + (i / 2) as i32 * 46,
                        cw / 2 - 4,
                        Action::Command(json!({"action":action,"id":"*"})),
                        false,
                    );
                }
                y += 98;
            }
        }
        if let Some(error) = engine
            .assets
            .get(&widget.path)
            .and_then(|a| a.error.as_ref())
        {
            self.text(pix, error, x, y, 13, self.theme.primary, cw);
            y += 38;
        }
        y + 16
    }
    fn profiles(&mut self, pix: &mut Pixmap, engine: &Engine, w: i32, h: i32) -> i32 {
        let profiles = engine.store.profiles.as_object().unwrap();
        let row_hit_start = self.hits.len();
        let narrow = w < 650;
        let row_height = if narrow { 184 } else { 136 };
        let first = (self.scroll / row_height).max(0) as usize;
        for (index, (name, _)) in profiles
            .iter()
            .enumerate()
            .skip(first)
            .take((h / row_height + 2) as usize)
        {
            let y = 100 + index as i32 * row_height - self.scroll;
            self.text(pix, name, 0, y, 17, self.theme.foreground, w);
            let columns = if narrow { 2 } else { 4 };
            let bw = (w - (columns - 1) * 8) / columns;
            let confirm = self.sections.contains(&format!("delete:{name}"));
            for (i, (label, action)) in [
                (
                    "apply",
                    Action::Command(json!({"action":"profile-apply","name":name})),
                ),
                (
                    "overwrite",
                    Action::Command(json!({"action":"profile-save","name":name})),
                ),
                ("rename", Action::Rename(name.clone())),
                (
                    if confirm { "confirm_delete" } else { "delete" },
                    if confirm {
                        Action::Command(json!({"action":"profile-delete","name":name}))
                    } else {
                        Action::Section(format!("delete:{name}"))
                    },
                ),
            ]
            .into_iter()
            .enumerate()
            {
                self.button(
                    pix,
                    &self.tr(label),
                    (i as i32 % columns) * (bw + 8),
                    y + 30 + (i as i32 / columns) * 44,
                    bw,
                    action,
                    false,
                );
            }
            if self.sections.contains(&format!("rename:{name}")) {
                self.input(
                    pix,
                    "rename",
                    &self.tr("rename_name"),
                    0,
                    y + if narrow { 120 } else { 76 },
                    w - 112,
                );
                self.button(
                    pix,
                    &self.tr("save"),
                    w - 100,
                    y + if narrow { 120 } else { 76 },
                    100,
                    Action::SaveRename,
                    false,
                );
            } else if confirm {
                self.button(
                    pix,
                    &self.tr("cancel"),
                    0,
                    y + if narrow { 120 } else { 76 },
                    100,
                    Action::Section(format!("delete:{name}")),
                    false,
                );
            }
        }
        for hit in &mut self.hits[row_hit_start..] {
            let end = (hit.rect.y + hit.rect.h).min(h);
            hit.rect.y = hit.rect.y.max(94);
            hit.rect.h = (end - hit.rect.y).max(0);
        }
        // Sticky create row remains visible while the layout list scrolls.
        self.rect(
            pix,
            &Rect {
                x: 0,
                y: 0,
                w,
                h: 94,
            },
            self.theme.surface,
            0.0,
        );
        self.text(
            pix,
            &self.tr("new_layout"),
            0,
            0,
            14,
            self.theme.foreground,
            w,
        );
        self.input(pix, "profile", &self.tr("layout_name"), 0, 30, w - 120);
        let exists = self
            .fields
            .get("profile")
            .is_some_and(|name| profiles.contains_key(name));
        self.button(
            pix,
            &self.tr(if exists { "overwrite" } else { "save" }),
            w - 108,
            30,
            108,
            Action::SaveProfile,
            true,
        );
        if profiles.is_empty() {
            self.text(pix, &self.tr("no_layouts"), 0, 120, 16, self.theme.muted, w);
        }
        (100 + profiles.len() as i32 * row_height).max(h)
    }
    fn settings(&mut self, pix: &mut Pixmap, engine: &Engine, w: i32) -> i32 {
        let mut y = 0;
        self.text(
            pix,
            &self.tr("collection"),
            0,
            y,
            16,
            self.theme.foreground,
            w,
        );
        y += 30;
        self.text(
            pix,
            &self.tr("directory_hint"),
            0,
            y,
            12,
            self.theme.muted,
            w,
        );
        y += 26;
        self.input(
            pix,
            "directory",
            &engine.paths.gif_dir.to_string_lossy(),
            0,
            y,
            w - 120,
        );
        self.button(
            pix,
            &self.tr("save"),
            w - 108,
            y,
            108,
            Action::SaveDirectory,
            false,
        );
        y += 62;
        let restore = engine.store.settings["restore"].as_bool().unwrap_or(true);
        self.toggle(
            pix,
            &self.tr("restore_short"),
            &self.tr("restore_hint"),
            0,
            y,
            w,
            restore,
            Action::Command(json!({"action":"settings","settings":{"restore":!restore}})),
        );
        y += 72;
        let open = self.fold(pix, "diagnostics", &self.tr("doctor"), 0, y, w);
        y += 50;
        if open {
            self.text(
                pix,
                &self.tr("diagnostics_hint"),
                0,
                y,
                12,
                self.theme.muted,
                w,
            );
            y += 32;
            let level = crate::diagnostics::level_name();
            let show = self.fold(
                pix,
                "log-level",
                &format!(
                    "{} · {}",
                    self.tr("logs_level"),
                    self.tr(&format!("log_{level}"))
                ),
                0,
                y,
                w,
            );
            y += 50;
            if show {
                for value in ["info", "debug", "trace", "warn", "error", "off"] {
                    self.button(
                        pix,
                        &self.tr(&format!("log_{value}")),
                        0,
                        y,
                        w,
                        Action::Choose(
                            "log-level".into(),
                            json!({"action":"logs-level","level":value}),
                        ),
                        level == value,
                    );
                    y += 44;
                }
            }
            if level == "trace" {
                self.text(pix, &self.tr("trace_hint"), 0, y, 12, self.theme.muted, w);
                y += 40;
            }
            self.button(
                pix,
                &self.tr("logs_export"),
                0,
                y,
                200.min(w),
                Action::Command(json!({"action":"logs-export"})),
                true,
            );
            y += 50;
            if let Some(report) = crate::diagnostics::status(&engine.paths)
                .get("last_export")
                .filter(|r| r["ok"] == true && r["pending"] != true)
            {
                if let Some(path) = report["directory"].as_str() {
                    self.text(
                        pix,
                        &self.tr("export_complete"),
                        0,
                        y,
                        12,
                        self.theme.muted,
                        w,
                    );
                    y += 26;
                    self.button(
                        pix,
                        &self.tr("open_folder"),
                        0,
                        y,
                        220.min(w),
                        Action::OpenFolder(path.into()),
                        false,
                    );
                    y += 50;
                }
            }
        }
        let open = self.fold(pix, "application", &self.tr("application"), 0, y, w);
        y += 50;
        if open {
            self.button(
                pix,
                &self.tr("refresh"),
                0,
                y,
                w / 2 - 4,
                Action::Refresh,
                false,
            );
            self.button(
                pix,
                &self.tr("quit_daemon"),
                w / 2 + 4,
                y,
                w / 2 - 4,
                Action::Command(json!({"action":"quit-daemon"})),
                false,
            );
            y += 52;
        }
        y + 16
    }
}
fn no_count(template: &str, count: usize) -> String {
    template.replace("{count}", &count.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn session() -> (tempfile::TempDir, Engine) {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::paths::Paths {
            runtime_dir: root.path().join("run"),
            config_dir: root.path().join("config"),
            cache_dir: root.path().join("cache"),
            data_dir: root.path().join("data"),
            state_dir: root.path().join("state"),
            gif_dir: root.path().join("gifs"),
            gif_dir_explicit: true,
        };
        std::fs::create_dir(&paths.gif_dir).unwrap();
        (root, Engine::new(paths).unwrap())
    }
    #[test]
    fn directory_form_replaces_text_and_saves_with_keyboard() {
        let (root, mut engine) = session();
        let mut ui = Ui::new();
        ui.tab = 3;
        ui.windowed = true;
        ui.activate(Action::Field("directory".into()), &mut engine);
        let replacement = root.path().join("different folder 鳥");
        std::fs::create_dir(&replacement).unwrap();
        ui.text_input(replacement.to_str().unwrap(), &mut engine);
        assert_eq!(ui.fields["directory"], replacement.to_string_lossy());
        let _ = ui.draw(&engine, 900, 680, 1.0);
        ui.focus_next(false, &mut engine);
        assert!(matches!(
            ui.keyboard_hit
                .and_then(|i| ui.hits.get(i))
                .map(|h| &h.action),
            Some(Action::SaveDirectory)
        ));
        ui.submit(&mut engine);
        assert!(ui.error.is_none(), "{:?}", ui.error);
        assert_eq!(engine.paths.gif_dir, replacement);
        ui.activate(Action::Tab(0), &mut engine);
        ui.activate(Action::Field("search".into()), &mut engine);
        ui.text_input("old query", &mut engine);
        ui.select_text();
        ui.text_input("new", &mut engine);
        assert_eq!(ui.fields["search"], "new");
    }
    #[test]
    fn narrow_scrolled_layouts_keep_create_and_rename_controls_reachable() {
        let (_root, mut engine) = session();
        for n in 0..40 {
            engine.store.profiles[format!("Layout {n:02}")] = json!({"widgets":[]});
        }
        let mut ui = Ui::new();
        ui.tab = 2;
        ui.windowed = true;
        ui.scroll = 350;
        ui.sections.insert("rename:Layout 02".into());
        let image = ui.draw(&engine, 640, 420, 1.5);
        assert_eq!((image.width(), image.height()), (960, 630));
        assert!(
            ui.hits
                .iter()
                .any(|h| matches!(h.action, Action::SaveProfile))
        );
        let field = ui
            .hits
            .iter()
            .find(|h| matches!(&h.action,Action::Field(key) if key=="rename"))
            .unwrap();
        let save = ui
            .hits
            .iter()
            .find(|h| matches!(h.action, Action::SaveRename))
            .unwrap();
        assert_eq!(
            field.rect.y, save.rect.y,
            "rename field and save must share a row"
        );
        for hit in &ui.hits {
            assert!(
                hit.rect.x >= 0 && hit.rect.y >= 0 && hit.rect.w > 0 && hit.rect.h > 0,
                "{:?}",
                hit.rect
            );
            assert!(
                hit.rect.x + hit.rect.w <= 640 && hit.rect.y + hit.rect.h <= 420,
                "{:?}",
                hit.rect
            );
        }
    }
    #[test]
    fn many_instances_keep_the_narrow_selector_bounded() {
        let (root, mut engine) = session();
        let file = root.path().join("gifs/tiny.gif");
        {
            let mut encoder = gif::Encoder::new(
                std::fs::File::create(&file).unwrap(),
                1,
                1,
                &[0, 0, 0, 255, 0, 0],
            )
            .unwrap();
            encoder
                .write_frame(&gif::Frame {
                    width: 1,
                    height: 1,
                    buffer: vec![1].into(),
                    delay: 5,
                    ..Default::default()
                })
                .unwrap();
        }
        for n in 0..100 {
            assert_eq!(
                engine.dispatch(&json!({"action":"spawn","gif":file,"id":format!("gif-{n:03}")}))["ok"],
                true
            );
        }
        let mut ui = Ui::new();
        ui.tab = 1;
        ui.windowed = true;
        ui.selected = Some("gif-000".into());
        ui.sections.insert("selector".into());
        let _ = ui.draw(&engine, 640, 420, 1.0);
        let selections = ui
            .hits
            .iter()
            .filter(|h| matches!(h.action, Action::Select(_)))
            .count();
        assert!(
            selections <= 8,
            "a small selector must not allocate rows for all instances"
        );
        ui.scroll = ui.scroll_max;
        let _ = ui.draw(&engine, 640, 420, 1.0);
        assert!(ui.hits.iter().all(|h| h.rect.y + h.rect.h <= 420));
    }
}

#[derive(Clone, Copy)]
struct Theme {
    surface: Color,
    variant: Color,
    foreground: Color,
    muted: Color,
    primary: Color,
    on_primary: Color,
    hover: Color,
    outline: Color,
}
impl Default for Theme {
    fn default() -> Self {
        Self {
            surface: Color::from_rgba8(18, 18, 23, 255),
            variant: Color::from_rgba8(32, 31, 36, 255),
            foreground: Color::from_rgba8(224, 222, 230, 255),
            muted: Color::from_rgba8(189, 186, 198, 255),
            primary: Color::from_rgba8(204, 185, 255, 255),
            on_primary: Color::from_rgba8(46, 32, 77, 255),
            hover: Color::from_rgba8(51, 47, 59, 255),
            outline: Color::from_rgba8(72, 69, 80, 255),
        }
    }
}
impl Theme {
    fn from_palette(palette: &Value) -> Self {
        let mut theme = Self::default();
        for (name, target) in [
            ("surface", &mut theme.surface),
            ("surface_variant", &mut theme.variant),
            ("on_surface", &mut theme.foreground),
            ("on_surface_variant", &mut theme.muted),
            ("primary", &mut theme.primary),
            ("on_primary", &mut theme.on_primary),
            ("hover", &mut theme.hover),
            ("outline", &mut theme.outline),
        ] {
            if let Some(value) = palette[name]
                .as_str()
                .and_then(|s| s.strip_prefix('#'))
                .and_then(|s| u32::from_str_radix(s, 16).ok())
            {
                *target =
                    Color::from_rgba8((value >> 16) as u8, (value >> 8) as u8, value as u8, 255);
            }
        }
        theme
    }
}
fn composite_preview(pix: &mut Pixmap, item: &Item) {
    let anim = &item.animation;
    let Ok(frame) = anim.pixels(item.signature.frame) else {
        return;
    };
    if let Some(source) = tiny_skia::PixmapRef::from_bytes(&frame, anim.width, anim.height) {
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
