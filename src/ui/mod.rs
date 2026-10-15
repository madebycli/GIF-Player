//! Standalone Rust interface: picker, playback controls, layouts and settings.
//!
//! It is a CPU-rendered, demand-driven immediate-mode UI that follows Noctalia 5's design system
//! (tokens in `theme`, controls in `kit`, pages in `pages`) and the live host palette, so it can
//! be used with or without Noctalia running. Native Noctalia panels are separate Luau/host UI.
mod icons;
mod kit;
mod pages;
mod text;
mod theme;

use crate::{model::Engine, render::Rect};
use anyhow::Result;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    process::Command,
};
use text::Typeface;
use theme::Theme;
use tiny_skia::Mask;

#[derive(Clone, PartialEq, Debug)]
pub enum Action {
    Command(Value),
    Section(String),
    SaveDirectory,
    SaveProfile,
    SaveRename,
    OpenFolder(PathBuf),
    ClearSearch,
    DismissError,
    ToggleGif(PathBuf),
    Tab(usize),
    Close,
    Select(String),
    Rename(String),
    Cancel(String),
    Field(String),
    Refresh,
    Scrollbar,
    Slider {
        id: String,
        action: String,
        key: String,
        min: f64,
        max: f64,
        step: f64,
    },
}
pub struct Hit {
    /// Window rectangle clipped to its region; empty while scrolled out of view.
    pub rect: Rect,
    /// Unclipped page rectangle, used to scroll a keyboard-focused control into view.
    pub page: Rect,
    pub action: Action,
    pub hint: String,
    pub scrolls: bool,
}
pub struct SliderDrag {
    pub action: Action,
    pub rect: Rect,
}
#[derive(Clone, Copy, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Escape,
    Enter,
    Space,
    Tab,
    BackTab,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
}
/// (file, source width, source height, frame, device width, device height, radius)
pub(super) type ImageKey = (PathBuf, u32, u32, usize, u32, u32, u32);
struct Entry {
    path: PathBuf,
    /// Case- and accent-folded relative path without extension, searched by the filter.
    key: String,
}
pub struct Ui {
    pub open: bool,
    pub windowed: bool,
    mode: String,
    sections: HashSet<String>,
    translations: Value,
    columns: usize,
    row_height: i32,
    content_height: i32,
    seen_export: Option<String>,
    theme: Theme,
    appearance: Value,
    viewport_height: i32,
    viewport_top: i32,
    selecting: bool,
    pending_tab: Option<bool>,
    pub tab: usize,
    pub dirty: bool,
    pub selected: Option<String>,
    pub fields: HashMap<String, String>,
    pub focus: Option<String>,
    caret: usize,
    anchor: Option<usize>,
    field_origin: HashMap<String, f32>,
    keyboard_focus: Option<Action>,
    pub hits: Vec<Hit>,
    pub files: Vec<PathBuf>,
    entries: Vec<Entry>,
    matches: Vec<usize>,
    matched_for: Option<String>,
    library_dir: PathBuf,
    hover_action: Option<Action>,
    pressed: Option<Action>,
    pub active_slider: Option<SliderDrag>,
    scroll_drag: Option<i32>,
    pointer: (f64, f64),
    scroll: i32,
    scroll_max: i32,
    list_scroll: i32,
    list_scroll_max: i32,
    list_rect: Option<Rect>,
    paint_scale: f32,
    pub error: Option<String>,
    notice: Option<String>,
    face: Option<Typeface>,
    revision: u64,
    // Drawing state shared with `kit`.
    dry: bool,
    origin: (i32, i32),
    clip: Rect,
    scrolls: bool,
    in_card: bool,
    mask: Option<Mask>,
    image_cache: HashMap<ImageKey, tiny_skia::Pixmap>,
    cache_scale: f32,
}
fn fold_char(c: char) -> impl Iterator<Item = char> {
    let folded: &str = match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => "a",
        'ç' | 'ć' | 'č' => "c",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' => "e",
        'ì' | 'í' | 'î' | 'ï' | 'ī' => "i",
        'ñ' | 'ń' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' => "o",
        'ù' | 'ú' | 'û' | 'ü' | 'ū' => "u",
        'ý' | 'ÿ' => "y",
        'ś' | 'š' => "s",
        'ž' | 'ź' | 'ż' => "z",
        'ß' => "ss",
        _ => "",
    };
    let direct = if folded.is_empty() { Some(c) } else { None };
    direct.into_iter().chain(folded.chars())
}
/// Lower-case and strip common Latin diacritics so "cafe" finds "Café" and "uber" finds "Über".
pub fn fold_text(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .flat_map(fold_char)
        .collect()
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
                    include_str!("../../plugins/gif-player/translations/de.json")
                } else {
                    include_str!("../../plugins/gif-player/translations/en.json")
                },
            )
            .unwrap(),
            columns: 3,
            row_height: 150,
            content_height: 0,
            seen_export: None,
            theme: Theme::default(),
            appearance: Value::Null,
            viewport_height: 500,
            viewport_top: 0,
            selecting: false,
            pending_tab: None,
            tab: 0,
            dirty: true,
            selected: None,
            fields: HashMap::new(),
            focus: None,
            caret: 0,
            anchor: None,
            field_origin: HashMap::new(),
            keyboard_focus: None,
            hits: Vec::new(),
            files: Vec::new(),
            entries: Vec::new(),
            matches: Vec::new(),
            matched_for: None,
            library_dir: PathBuf::new(),
            hover_action: None,
            pressed: None,
            active_slider: None,
            scroll_drag: None,
            pointer: (-1.0, -1.0),
            scroll: 0,
            scroll_max: 0,
            list_scroll: 0,
            list_scroll_max: 0,
            list_rect: None,
            paint_scale: 1.0,
            error: None,
            notice: None,
            face: None,
            revision: u64::MAX,
            dry: false,
            origin: (0, 0),
            clip: Rect {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
            },
            scrolls: false,
            in_card: false,
            mask: None,
            image_cache: HashMap::new(),
            cache_scale: 0.0,
        }
    }
    pub(super) fn tr(&self, key: &str) -> String {
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
        self.list_scroll = 0;
        self.sync_appearance(engine);
        self.load_font(engine)?;
        self.dirty = true;
        self.keyboard_focus = None;
        self.pressed = None;
        self.error = None;
        self.notice = None;
        self.refresh(engine)?;
        if self.tab == 0 {
            self.focus_field("search", false, engine);
        } else {
            self.blur();
        }
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
                self.face = None;
            }
            self.theme = Theme::from_palette(&appearance["palette"]);
            self.appearance = appearance;
            self.dirty = true;
        }
    }
    fn load_font(&mut self, engine: &Engine) -> Result<()> {
        if self.face.is_none() {
            let explicit = engine
                .store
                .settings
                .get("font_file")
                .and_then(Value::as_str)
                .map(PathBuf::from);
            let family = self.appearance["font_family"]
                .as_str()
                .filter(|f| !f.trim().is_empty())
                .unwrap_or("sans-serif")
                .to_owned();
            self.face = Some(Typeface::resolve(&family, explicit)?);
        }
        Ok(())
    }
    pub fn refresh(&mut self, engine: &mut Engine) -> Result<()> {
        self.files = engine.paths.collection()?;
        self.library_dir = engine.paths.gif_dir.clone();
        self.rebuild_entries();
        self.image_cache.clear();
        self.scroll = 0;
        self.dirty = true;
        Ok(())
    }
    /// Pick up GIFs added or removed while another window had the keyboard focus.
    pub fn focus_gained(&mut self, engine: &Engine) {
        if !self.open {
            return;
        }
        if let Ok(files) = engine.paths.collection() {
            if files != self.files {
                self.files = files;
                self.rebuild_entries();
                self.dirty = true;
            }
        }
    }
    fn rebuild_entries(&mut self) {
        let root = self.library_dir.clone();
        self.entries = self
            .files
            .iter()
            .map(|path| {
                let relative = path.strip_prefix(&root).unwrap_or(path);
                Entry {
                    path: path.clone(),
                    key: fold_text(&relative.with_extension("").to_string_lossy()),
                }
            })
            .collect();
        self.matched_for = None;
    }
    pub fn sync(&mut self, engine: &Engine) {
        if self.open && self.library_dir != engine.paths.gif_dir {
            self.library_dir = engine.paths.gif_dir.clone();
            self.fields.remove("directory");
            match engine.paths.collection() {
                Ok(files) => {
                    self.files = files;
                    self.rebuild_entries();
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
    /// Entries matching every whitespace-separated search token (folder names included).
    fn refilter(&mut self) {
        let query = fold_text(self.fields.get("search").map_or("", String::as_str));
        if self.matched_for.as_deref() == Some(query.as_str()) {
            return;
        }
        let tokens = query.split_whitespace().collect::<Vec<_>>();
        self.matches = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| tokens.iter().all(|token| entry.key.contains(token)))
            .map(|(index, _)| index)
            .collect();
        self.matched_for = Some(query);
    }
    fn filtered(&mut self) -> Vec<PathBuf> {
        self.refilter();
        self.matches
            .iter()
            .map(|&i| self.entries[i].path.clone())
            .collect()
    }
    pub fn prepare(&mut self, engine: &mut Engine) {
        if !self.open {
            self.face = None;
            self.files.clear();
            self.entries.clear();
            self.matched_for = None;
            self.hits.clear();
            self.blur();
            self.mask = None;
            self.image_cache.clear();
            // A closed window must not keep asking for the tile the pointer last rested on.
            self.hover_action = None;
            self.pressed = None;
            self.active_slider = None;
            self.scroll_drag = None;
            self.selecting = false;
        }
        if !self.open || self.tab != 0 {
            engine.previews.clear();
            engine.gc();
            return;
        }
        self.refilter();
        let row = self.row_height.max(1);
        let first = (self.scroll / row).max(0) as usize;
        let count =
            ((self.viewport_height.max(1) / row + 2) as usize * self.columns.max(1)).min(28);
        engine.previews = self
            .matches
            .iter()
            .skip(first * self.columns.max(1))
            .take(count)
            .map(|&i| self.entries[i].path.clone())
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
    // ----- focus and text editing -----
    fn blur(&mut self) {
        self.focus = None;
        self.anchor = None;
    }
    fn field_len(&self) -> usize {
        self.focus
            .as_ref()
            .and_then(|k| self.fields.get(k))
            .map_or(0, |v| v.chars().count())
    }
    fn focus_field(&mut self, key: &str, select_all: bool, engine: &Engine) {
        if self.focus.as_deref() != Some(key) {
            match key {
                "directory" => {
                    self.fields
                        .entry(key.into())
                        .or_insert_with(|| engine.paths.gif_dir.to_string_lossy().into());
                }
                "x" | "y" => {
                    if let Some(widget) =
                        self.selected.as_ref().and_then(|id| engine.widgets.get(id))
                    {
                        let value = if key == "x" {
                            widget.settings.x
                        } else {
                            widget.settings.y
                        };
                        self.fields.insert(key.into(), format!("{value:.0}"));
                    }
                }
                _ => {}
            }
        }
        self.focus = Some(key.into());
        self.keyboard_focus = None;
        let len = self.field_len();
        self.caret = len;
        self.anchor = if select_all && len > 0 { Some(0) } else { None };
        self.dirty = true;
    }
    fn selection(&self) -> Option<(usize, usize)> {
        self.anchor
            .filter(|a| *a != self.caret)
            .map(|a| (a.min(self.caret), a.max(self.caret)))
    }
    fn delete_selection(&mut self) -> bool {
        let (Some((lo, hi)), Some(key)) = (self.selection(), self.focus.clone()) else {
            return false;
        };
        let value = self.fields.entry(key).or_default();
        let from = byte_index(value, lo);
        let to = byte_index(value, hi);
        value.replace_range(from..to, "");
        self.caret = lo;
        self.anchor = None;
        true
    }
    pub fn text_input(&mut self, text: &str, engine: &mut Engine) {
        let Some(key) = self.focus.clone() else {
            return;
        };
        let clean: String = text
            .chars()
            .map(|c| {
                if c == '\n' || c == '\r' || c == '\t' {
                    ' '
                } else {
                    c
                }
            })
            .filter(|c| !c.is_control())
            .collect();
        if clean.is_empty() {
            return;
        }
        self.delete_selection();
        let value = self.fields.entry(key.clone()).or_default();
        let at = byte_index(value, self.caret);
        value.insert_str(at, &clean);
        self.caret += clean.chars().count();
        self.anchor = None;
        if key == "search" {
            self.scroll = 0;
        }
        self.dirty = true;
        let _ = engine;
    }
    pub fn paste(&mut self, text: &str, engine: &mut Engine) {
        self.text_input(text, engine);
    }
    /// Selected text of the focused field (for Ctrl+C); the Wayland layer owns the clipboard.
    pub fn selected_text(&self) -> Option<String> {
        let (lo, hi) = self.selection()?;
        let value = self.fields.get(self.focus.as_ref()?)?;
        Some(value[byte_index(value, lo)..byte_index(value, hi)].to_owned())
    }
    pub fn cut_selection(&mut self) -> Option<String> {
        let text = self.selected_text()?;
        self.delete_selection();
        self.dirty = true;
        Some(text)
    }
    fn erase(&mut self, forward: bool, word: bool) {
        let Some(key) = self.focus.clone() else {
            return;
        };
        if !self.delete_selection() {
            let value = self.fields.entry(key.clone()).or_default();
            let (from, to) = if forward {
                let end = if word {
                    word_right(value, self.caret)
                } else {
                    (self.caret + 1).min(value.chars().count())
                };
                (self.caret, end)
            } else {
                let start = if word {
                    word_left(value, self.caret)
                } else {
                    self.caret.saturating_sub(1)
                };
                (start, self.caret)
            };
            let (a, b) = (byte_index(value, from), byte_index(value, to));
            value.replace_range(a..b, "");
            self.caret = from;
        }
        if key == "search" {
            self.scroll = 0;
        }
        self.dirty = true;
    }
    fn move_caret(&mut self, target: usize, extend: bool) {
        if extend {
            self.anchor.get_or_insert(self.caret);
        } else {
            self.anchor = None;
        }
        self.caret = target.min(self.field_len());
        self.dirty = true;
    }
    pub fn select_text(&mut self) {
        if self.focus.is_some() {
            self.anchor = Some(0);
            self.caret = self.field_len();
            self.dirty = true;
        }
    }
    /// Place the caret from a pointer x position (window coordinates).
    fn place_caret(&mut self, x: f64, extend: bool) {
        let Some(key) = self.focus.clone() else {
            return;
        };
        let value = self.fields.get(&key).cloned().unwrap_or_default();
        let origin = self.field_origin.get(&key).copied().unwrap_or(0.0);
        let offset = (x as f32 - origin).max(0.0);
        let index = self
            .face
            .as_mut()
            .map_or(0, |f| f.index_at(&value, offset, theme::FONT_BODY, false));
        self.move_caret(index, extend);
    }
    // ----- actions -----
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
                if result.get("directory").and_then(Value::as_str).is_some() {
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
            }
            Action::Cancel(key) => {
                self.sections.remove(&key);
                if key.starts_with("rename:") {
                    self.blur();
                }
            }
            Action::SaveDirectory => self.submit_field("directory", engine),
            Action::SaveProfile => self.submit_field("profile", engine),
            Action::SaveRename => self.submit_field("rename", engine),
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
                self.focus_field("search", false, engine);
            }
            Action::DismissError => {
                self.error = None;
                engine.error = None;
            }
            Action::Tab(tab) => {
                self.notice = None;
                self.keyboard_focus = None;
                self.tab = tab;
                self.blur();
                self.scroll = 0;
                self.list_scroll = 0;
                if tab == 0 {
                    self.focus_field("search", false, engine);
                }
            }
            Action::Close => {
                self.open = false;
            }
            Action::Select(id) => {
                self.selected = Some(id);
                self.fields
                    .retain(|k, _| ["search", "profile", "directory"].contains(&k.as_str()));
                self.blur();
            }
            Action::Rename(name) => {
                self.sections.retain(|k| !k.starts_with("rename:"));
                self.sections.insert(format!("rename:{name}"));
                self.fields.insert("rename".into(), name);
                self.focus_field("rename", true, engine);
            }
            Action::Field(key) => {
                self.focus_field(&key, true, engine);
            }
            Action::Refresh => match self.refresh(engine) {
                Ok(()) => self.notice = Some(self.tr("reloaded")),
                Err(e) => self.error = Some(e.to_string()),
            },
            Action::Scrollbar | Action::Slider { .. } => {}
        }
        self.dirty = true;
    }
    fn hit_at(&self, x: f64, y: f64) -> Option<usize> {
        self.hits
            .iter()
            .rposition(|h| h.rect.w > 0 && h.rect.h > 0 && h.rect.contains(x, y))
    }
    pub fn press(&mut self, x: f64, y: f64, engine: &mut Engine) {
        self.pointer = (x, y);
        self.notice = None;
        self.keyboard_focus = None;
        self.dirty = true;
        let Some(index) = self.hit_at(x, y) else {
            self.blur();
            return;
        };
        let (action, rect) = (
            self.hits[index].action.clone(),
            self.hits[index].rect.clone(),
        );
        match action {
            Action::Slider { .. } => {
                self.blur();
                self.active_slider = Some(SliderDrag { action, rect });
                self.drag_slider(x, engine);
            }
            Action::Field(key) => {
                if self.focus.as_deref() == Some(&key) {
                    self.place_caret(x, false);
                    self.anchor = Some(self.caret);
                    self.selecting = true;
                } else {
                    // The first click focuses and selects everything, so typing replaces a value.
                    self.focus_field(&key, true, engine);
                }
            }
            Action::Scrollbar => {
                self.blur();
                let grab = (y as i32 - self.thumb_top()).clamp(0, self.thumb_height());
                self.scroll_drag = Some(grab);
                self.drag_scrollbar(y);
            }
            other => {
                self.blur();
                self.pressed = Some(other);
            }
        }
    }
    pub fn release(&mut self, x: f64, y: f64, engine: &mut Engine) {
        self.pointer = (x, y);
        self.active_slider = None;
        self.scroll_drag = None;
        self.selecting = false;
        if self.anchor == Some(self.caret) {
            self.anchor = None;
        }
        if let Some(pressed) = self.pressed.take() {
            let over = self.hit_at(x, y).map(|i| self.hits[i].action.clone());
            if over.as_ref() == Some(&pressed) {
                self.activate(pressed, engine);
            }
        }
        self.dirty = true;
    }
    /// Press + release at one point (used by tests).
    #[cfg(test)]
    pub fn click(&mut self, x: f64, y: f64, engine: &mut Engine) {
        self.press(x, y, engine);
        self.release(x, y, engine);
    }
    pub fn drag_slider(&mut self, x: f64, engine: &mut Engine) {
        let Some(SliderDrag { action, rect }) = self.active_slider.as_ref() else {
            return;
        };
        let Action::Slider {
            id,
            action: name,
            key,
            min,
            max,
            step,
        } = action.clone()
        else {
            return;
        };
        let (left, span) = (
            rect.x as f64 + 2.0 + theme::SLIDER_THUMB as f64 / 2.0,
            (rect.w - 4 - theme::SLIDER_THUMB).max(1) as f64,
        );
        let fraction = ((x - left) / span).clamp(0.0, 1.0);
        let raw = min + (max - min) * fraction;
        let value = ((raw / step).round() * step).clamp(min, max);
        let mut cmd = json!({"id":id,"action":name});
        cmd[&key] = json!((value * 1e6).round() / 1e6);
        let result = engine.dispatch(&cmd);
        self.error = result
            .get("error")
            .and_then(Value::as_str)
            .map(str::to_string);
        self.dirty = true;
    }
    pub fn motion(&mut self, x: f64, y: f64, engine: &mut Engine) {
        self.pointer = (x, y);
        if self.active_slider.is_some() {
            self.drag_slider(x, engine);
            return;
        }
        if self.scroll_drag.is_some() {
            self.drag_scrollbar(y);
            return;
        }
        if self.selecting {
            self.place_caret(x, true);
            return;
        }
        let hovered = self.hit_at(x, y).map(|i| self.hits[i].action.clone());
        if hovered != self.hover_action {
            self.hover_action = hovered;
            self.dirty = true;
        }
    }
    pub fn pointer_left(&mut self) {
        self.pointer = (-1.0, -1.0);
        if self.hover_action.take().is_some() {
            self.dirty = true;
        }
    }
    fn thumb_height(&self) -> i32 {
        let v = self.viewport_height.max(1) as f64;
        ((v * v / (v + self.scroll_max as f64)) as i32).clamp(32, self.viewport_height.max(32))
    }
    fn thumb_top(&self) -> i32 {
        let travel = (self.viewport_height - self.thumb_height()).max(1) as f64;
        let ratio = if self.scroll_max > 0 {
            self.scroll as f64 / self.scroll_max as f64
        } else {
            0.0
        };
        self.viewport_top + (travel * ratio) as i32
    }
    fn drag_scrollbar(&mut self, y: f64) {
        let grab = self.scroll_drag.unwrap_or(0);
        let travel = (self.viewport_height - self.thumb_height()).max(1) as f64;
        let ratio = ((y - grab as f64 - self.viewport_top as f64) / travel).clamp(0.0, 1.0);
        self.scroll = (ratio * self.scroll_max as f64).round() as i32;
        self.dirty = true;
    }
    fn hover_path(&self) -> Option<PathBuf> {
        match self.hover_action.as_ref()? {
            Action::ToggleGif(path) => Some(path.clone()),
            _ => None,
        }
    }
    pub fn request_hover(&self, engine: &mut Engine) {
        // Only while the collection is showing, and only for tiles the cache keeps (`previews`):
        // anything else would be released again at once and requested again by the next wake-up.
        if !self.open || self.tab != 0 {
            return;
        }
        if let Some(path) = self.hover_path().filter(|p| engine.previews.contains(p)) {
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
    /// Wheel/touchpad scrolling goes to the instance list while the pointer is over it.
    pub fn scroll(&mut self, delta: f64) {
        let step = (delta * 3.0) as i32;
        let over_list = self
            .list_rect
            .as_ref()
            .is_some_and(|r| r.contains(self.pointer.0, self.pointer.1));
        if over_list && self.list_scroll_max > 0 {
            self.list_scroll = (self.list_scroll + step).clamp(0, self.list_scroll_max);
        } else {
            self.scroll = (self.scroll + step).clamp(0, self.scroll_max);
        }
        self.active_slider = None;
        // Hover is re-evaluated against the moved content at the end of the next draw.
        self.dirty = true;
    }
    pub fn submit(&mut self, engine: &mut Engine) {
        match self.focus.clone() {
            Some(key) => self.submit_field(&key, engine),
            None => {
                if let Some(action) = self.keyboard_focus.clone() {
                    self.activate(action, engine);
                }
            }
        }
    }
    fn submit_field(&mut self, key: &str, engine: &mut Engine) {
        let value = self.fields.get(key).cloned().unwrap_or_default();
        if key == "search" {
            if let Some(path) = self.filtered().first() {
                self.activate(Action::ToggleGif(path.clone()), engine);
            }
            return;
        }
        let cmd = match key {
            "profile" if !value.trim().is_empty() => {
                Some(json!({"action":"profile-save","name":value.trim()}))
            }
            "directory" if !value.trim().is_empty() => {
                Some(json!({"action":"library-directory","path":value.trim()}))
            }
            "rename" => self
                .sections
                .iter()
                .find_map(|key| key.strip_prefix("rename:"))
                .map(|name| json!({"action":"profile-rename","name":name,"new_name":value.trim()})),
            "x" | "y" => self
                .selected
                .as_ref()
                .and_then(|id| engine.widgets.get(id))
                .and_then(|w| {
                    value
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .filter(|v| v.is_finite())
                        .map(|n| {
                            json!({"action":"move","id":w.id,
                            "x":if key == "x" { n } else { w.settings.x },
                            "y":if key == "y" { n } else { w.settings.y }})
                        })
                }),
            _ => None,
        };
        if let Some(cmd) = cmd {
            self.activate(Action::Command(cmd), engine);
            if self.error.is_none() {
                match key {
                    "directory" => {
                        let _ = self.refresh(engine);
                        self.notice = Some(self.tr("reloaded"));
                    }
                    "profile" => {
                        self.fields.insert("profile".into(), String::new());
                        self.caret = 0;
                        self.notice = Some(self.tr("layout_saved"));
                    }
                    "rename" => {
                        self.sections.retain(|key| !key.starts_with("rename:"));
                        self.blur();
                    }
                    _ => {}
                }
            }
        } else {
            self.error = Some(self.tr(match key {
                "directory" => "directory_hint",
                "profile" => "profile_name",
                _ => "invalid_value",
            }));
        }
        self.dirty = true;
    }
    // ----- keyboard navigation -----
    fn focus_order(&self) -> Vec<usize> {
        self.hits
            .iter()
            .enumerate()
            .filter(|(_, h)| h.page.w > 0 && h.page.h > 0 && !matches!(h.action, Action::Scrollbar))
            .map(|(i, _)| i)
            .collect()
    }
    fn current_focus_index(&self) -> Option<usize> {
        let target = match (&self.focus, &self.keyboard_focus) {
            (Some(key), _) => Action::Field(key.clone()),
            (None, Some(action)) => action.clone(),
            _ => return None,
        };
        self.hits.iter().position(|h| h.action == target)
    }
    fn focus_hit(&mut self, index: usize, engine: &Engine) {
        let action = self.hits[index].action.clone();
        if let Action::Field(key) = &action {
            self.focus_field(key, true, engine);
            self.ensure_visible(index);
            return;
        }
        self.blur();
        self.keyboard_focus = Some(action);
        self.ensure_visible(index);
        self.dirty = true;
    }
    fn ensure_visible(&mut self, index: usize) {
        let hit = &self.hits[index];
        if !hit.scrolls {
            return;
        }
        let (top, height) = (hit.page.y, hit.page.h);
        let view = self.viewport_height.max(1);
        let margin = 12;
        if top - margin < self.scroll {
            self.scroll = (top - margin).max(0);
        } else if top + height + margin > self.scroll + view {
            self.scroll = (top + height + margin - view).max(0);
        }
        self.scroll = self.scroll.clamp(0, self.scroll_max.max(self.scroll));
        self.dirty = true;
    }
    pub fn focus_next(&mut self, reverse: bool, engine: &mut Engine) {
        let order = self.focus_order();
        if order.is_empty() {
            return;
        }
        let current = self
            .current_focus_index()
            .and_then(|i| order.iter().position(|&o| o == i));
        // Long lists only register rows near the viewport: at their edge, Tab scrolls one page
        // and continues into the rows that appear, instead of wrapping around too early.
        let at_edge = match current {
            Some(i) => (!reverse && i + 1 == order.len()) || (reverse && i == 0),
            None => false,
        };
        let in_list = current.is_some_and(|i| self.hits[order[i]].scrolls);
        if at_edge
            && in_list
            && (if reverse {
                self.scroll > 0
            } else {
                self.scroll < self.scroll_max
            })
        {
            let page = (self.viewport_height as f64 * 0.75) as i32;
            self.scroll =
                (self.scroll + if reverse { -page } else { page }).clamp(0, self.scroll_max);
            self.pending_tab = Some(reverse);
            self.dirty = true;
            return;
        }
        let next = match current {
            Some(i) if reverse => (i + order.len() - 1) % order.len(),
            Some(i) => (i + 1) % order.len(),
            None if reverse => order.len() - 1,
            None => 0,
        };
        self.focus_hit(order[next], engine);
    }
    /// Continue a Tab press that had to scroll first: focus the first/last control now in view.
    fn finish_pending_tab(&mut self, engine: &Engine) {
        let Some(reverse) = self.pending_tab.take() else {
            return;
        };
        let mut visible = self
            .hits
            .iter()
            .enumerate()
            .filter(|(_, h)| {
                h.scrolls && h.rect.w > 0 && h.rect.h > 0 && !matches!(h.action, Action::Scrollbar)
            })
            .map(|(i, _)| i);
        let pick = if reverse {
            visible.next_back()
        } else {
            visible.next()
        };
        if let Some(index) = pick {
            self.focus_hit(index, engine);
        }
    }
    /// Arrow-key navigation between controls by geometry (grid tiles, list rows, buttons).
    fn focus_direction(&mut self, dx: i32, dy: i32, engine: &mut Engine) {
        let order = self.focus_order();
        let Some(current) = self.current_focus_index().filter(|i| order.contains(i)) else {
            if let Some(first) = order.first() {
                self.focus_hit(*first, engine);
            }
            return;
        };
        let from = self.hits[current].page.clone();
        let (cx, cy) = (from.x + from.w / 2, from.y + from.h / 2);
        let mut best: Option<(i64, usize)> = None;
        for &i in &order {
            if i == current || self.hits[i].scrolls != self.hits[current].scrolls {
                continue;
            }
            let r = &self.hits[i].page;
            let (px, py) = (r.x + r.w / 2 - cx, r.y + r.h / 2 - cy);
            let (along, across) = if dx != 0 {
                (px * dx, py)
            } else {
                (py * dy, px)
            };
            if along <= 0 {
                continue;
            }
            let score = along as i64 + across.abs() as i64 * 3;
            if best.is_none_or(|(s, _)| score < s) {
                best = Some((score, i));
            }
        }
        if let Some((_, i)) = best {
            self.focus_hit(i, engine);
        }
    }
    pub fn adjust_focused_slider(&mut self, direction: f64, engine: &mut Engine) -> bool {
        let Some(Action::Slider {
            id,
            action,
            key,
            min,
            max,
            ..
        }) = self.keyboard_focus.clone()
        else {
            return false;
        };
        let Some(widget) = engine.widgets.get(&id) else {
            return true;
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
        true
    }
    pub fn key(&mut self, key: Key, mods: Mods, engine: &mut Engine) {
        let typing = self.focus.is_some();
        match key {
            Key::Escape => {
                if typing {
                    self.blur();
                    self.dirty = true;
                } else if self.keyboard_focus.take().is_some() {
                    self.dirty = true;
                } else if !self.windowed {
                    self.open = false;
                }
            }
            Key::Enter => self.submit(engine),
            Key::Space if !typing => self.submit(engine),
            Key::Space => self.text_input(" ", engine),
            Key::Tab => self.focus_next(false, engine),
            Key::BackTab => self.focus_next(true, engine),
            Key::Backspace if typing => self.erase(false, mods.ctrl),
            Key::Delete if typing => self.erase(true, mods.ctrl),
            Key::Left if typing => {
                let target = match self.selection() {
                    Some((lo, _)) if !mods.shift && !mods.ctrl => lo,
                    _ if mods.ctrl => {
                        let v = self
                            .fields
                            .get(self.focus.as_ref().unwrap())
                            .cloned()
                            .unwrap_or_default();
                        word_left(&v, self.caret)
                    }
                    _ => self.caret.saturating_sub(1),
                };
                self.move_caret(target, mods.shift);
            }
            Key::Right if typing => {
                let target = match self.selection() {
                    Some((_, hi)) if !mods.shift && !mods.ctrl => hi,
                    _ if mods.ctrl => {
                        let v = self
                            .fields
                            .get(self.focus.as_ref().unwrap())
                            .cloned()
                            .unwrap_or_default();
                        word_right(&v, self.caret)
                    }
                    _ => self.caret + 1,
                };
                self.move_caret(target, mods.shift);
            }
            Key::Home if typing => self.move_caret(0, mods.shift),
            Key::End if typing => self.move_caret(usize::MAX, mods.shift),
            Key::Left | Key::Right => {
                let direction = if key == Key::Left { -1.0 } else { 1.0 };
                if !self.adjust_focused_slider(direction, engine) {
                    self.focus_direction(direction as i32, 0, engine);
                }
            }
            Key::Up => {
                if typing {
                    self.move_caret(0, mods.shift);
                } else {
                    self.focus_direction(0, -1, engine);
                }
            }
            Key::Down => {
                if typing {
                    self.move_caret(usize::MAX, mods.shift);
                } else {
                    self.focus_direction(0, 1, engine);
                }
            }
            Key::Home | Key::End => {
                self.scroll = if key == Key::Home { 0 } else { self.scroll_max };
                self.dirty = true;
            }
            Key::Backspace | Key::Delete => {}
            Key::PageUp | Key::PageDown => {
                let page = (self.viewport_height as f64 * 0.9) as i32;
                let delta = if key == Key::PageUp { -page } else { page };
                self.scroll = (self.scroll + delta).clamp(0, self.scroll_max);
                self.dirty = true;
            }
        }
    }
    pub fn shortcut(&mut self, letter: char, mods: Mods, engine: &mut Engine) -> bool {
        if !mods.ctrl {
            return false;
        }
        match letter.to_ascii_lowercase() {
            'a' => self.select_text(),
            'f' => {
                self.tab = 0;
                self.focus_field("search", true, engine);
            }
            'w' => self.open = false,
            _ => return false,
        }
        self.dirty = true;
        true
    }
}
fn byte_index(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map_or(text.len(), |(i, _)| i)
}
fn word_left(text: &str, from: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut i = from.min(chars.len());
    while i > 0 && !chars[i - 1].is_alphanumeric() {
        i -= 1;
    }
    while i > 0 && chars[i - 1].is_alphanumeric() {
        i -= 1;
    }
    i
}
fn word_right(text: &str, from: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut i = from.min(chars.len());
    while i < chars.len() && !chars[i].is_alphanumeric() {
        i += 1;
    }
    while i < chars.len() && chars[i].is_alphanumeric() {
        i += 1;
    }
    i
}
pub(super) fn no_count(template: &str, count: usize) -> String {
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
    fn tiny_gif(path: &std::path::Path) {
        let mut encoder = gif::Encoder::new(
            std::fs::File::create(path).unwrap(),
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
    fn window(
        ui: &mut Ui,
        engine: &Engine,
        tab: usize,
        w: u32,
        h: u32,
        scale: f64,
    ) -> tiny_skia::Pixmap {
        ui.tab = tab;
        ui.windowed = true;
        ui.open = true;
        ui.draw(engine, w, h, scale)
    }
    fn hit_for(ui: &Ui, wanted: impl Fn(&Action) -> bool) -> &Hit {
        ui.hits
            .iter()
            .find(|h| wanted(&h.action))
            .expect("control exists")
    }
    fn center(r: &Rect) -> (f64, f64) {
        (r.x as f64 + r.w as f64 / 2.0, r.y as f64 + r.h as f64 / 2.0)
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
        assert_eq!(ui.keyboard_focus, Some(Action::SaveDirectory));
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
    fn typing_the_directory_survives_pressing_save_with_the_mouse() {
        let (root, mut engine) = session();
        let target = root.path().join("elsewhere");
        std::fs::create_dir(&target).unwrap();
        let mut ui = Ui::new();
        let _ = window(&mut ui, &engine, 3, 900, 680, 1.0);
        let field = hit_for(&ui, |a| matches!(a, Action::Field(k) if k == "directory"))
            .rect
            .clone();
        let (x, y) = center(&field);
        ui.click(x, y, &mut engine);
        ui.text_input(target.to_str().unwrap(), &mut engine);
        let _ = ui.draw(&engine, 900, 680, 1.0);
        let save = hit_for(&ui, |a| *a == Action::SaveDirectory).rect.clone();
        let (x, y) = center(&save);
        ui.click(x, y, &mut engine);
        assert!(ui.error.is_none(), "{:?}", ui.error);
        assert_eq!(
            engine.paths.gif_dir, target,
            "the typed text, not the old folder, is saved"
        );
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
        let field = hit_for(&ui, |a| matches!(a, Action::Field(key) if key == "rename"));
        let save = hit_for(&ui, |a| matches!(a, Action::SaveRename));
        assert_eq!(
            field.page.y + field.page.h / 2,
            save.page.y + save.page.h / 2,
            "rename field and save share a row"
        );
        for hit in ui.hits.iter().filter(|h| h.rect.w > 0 && h.rect.h > 0) {
            assert!(
                hit.rect.x >= 0
                    && hit.rect.y >= 0
                    && hit.rect.x + hit.rect.w <= 640
                    && hit.rect.y + hit.rect.h <= 420,
                "{:?}",
                hit.rect
            );
        }
    }
    #[test]
    fn many_instances_keep_the_narrow_selector_bounded() {
        let (root, mut engine) = session();
        let file = root.path().join("gifs/tiny.gif");
        tiny_gif(&file);
        for n in 0..100 {
            assert_eq!(
                engine.dispatch(&json!({"action":"spawn","gif":file,"id":format!("gif-{n:03}")}))["ok"],
                true
            );
        }
        let mut ui = Ui::new();
        ui.selected = Some("gif-000".into());
        ui.sections.insert("selector".into());
        let _ = window(&mut ui, &engine, 1, 640, 420, 1.0);
        let selections = ui
            .hits
            .iter()
            .filter(|h| matches!(h.action, Action::Select(_)))
            .count();
        assert!(
            selections <= 12,
            "a small selector must not register rows for all instances: {selections}"
        );
        ui.scroll = ui.scroll_max;
        let _ = ui.draw(&engine, 640, 420, 1.0);
        assert!(
            ui.hits
                .iter()
                .filter(|h| h.rect.h > 0)
                .all(|h| h.rect.y + h.rect.h <= 420)
        );
    }
    #[test]
    fn search_matches_names_and_folders_but_never_the_parent_path() {
        let (root, mut engine) = session();
        std::fs::create_dir_all(root.path().join("gifs/Reaktionen")).unwrap();
        for name in [
            "Café Cat.gif",
            "Über Wave.gif",
            "Reaktionen/Daumen hoch.gif",
            "plain.gif",
        ] {
            tiny_gif(&root.path().join("gifs").join(name));
        }
        let mut ui = Ui::new();
        ui.refresh(&mut engine).unwrap();
        let hits = |ui: &mut Ui, q: &str| {
            ui.fields.insert("search".into(), q.into());
            ui.filtered()
                .iter()
                .map(|p| p.file_stem().unwrap().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        // The temp directory's own name must not match everything.
        let parent = root
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_lowercase();
        assert!(hits(&mut ui, &parent).is_empty());
        assert_eq!(hits(&mut ui, "cafe"), ["Café Cat"]);
        assert_eq!(hits(&mut ui, "UBER"), ["Über Wave"]);
        assert_eq!(
            hits(&mut ui, "reaktionen daumen"),
            ["Daumen hoch"],
            "folder + name tokens combine"
        );
        assert_eq!(hits(&mut ui, "").len(), 4);
        assert_eq!(
            hits(&mut ui, ""),
            ["Café Cat", "plain", "Daumen hoch", "Über Wave"],
            "collection order is case-insensitive by relative path"
        );
    }
    #[test]
    fn caret_editing_selection_word_delete_and_paste() {
        let (_root, mut engine) = session();
        let mut ui = Ui::new();
        ui.activate(Action::Field("profile".into()), &mut engine);
        let mods = Mods::default();
        let shift = Mods {
            shift: true,
            ..Mods::default()
        };
        let ctrl = Mods {
            ctrl: true,
            ..Mods::default()
        };
        ui.text_input("hello big world", &mut engine);
        ui.key(Key::Left, mods, &mut engine);
        ui.key(Key::Left, mods, &mut engine);
        ui.text_input("X", &mut engine);
        assert_eq!(ui.fields["profile"], "hello big worXld");
        ui.key(Key::Home, mods, &mut engine);
        ui.key(Key::Delete, mods, &mut engine);
        assert_eq!(ui.fields["profile"], "ello big worXld");
        ui.key(Key::End, mods, &mut engine);
        ui.key(Key::Backspace, ctrl, &mut engine);
        assert_eq!(
            ui.fields["profile"], "ello big ",
            "Ctrl+Backspace removes the previous word"
        );
        ui.key(Key::Left, shift, &mut engine);
        ui.key(Key::Left, shift, &mut engine);
        assert_eq!(ui.selected_text().as_deref(), Some("g "));
        ui.paste("new\nline\t!", &mut engine);
        assert_eq!(
            ui.fields["profile"], "ello binew line !",
            "paste replaces the selection and flattens control characters"
        );
        assert_eq!(ui.cut_selection(), None);
        ui.select_text();
        assert_eq!(ui.cut_selection().as_deref(), Some("ello binew line !"));
        assert_eq!(ui.fields["profile"], "");
        ui.key(Key::Escape, mods, &mut engine);
        assert!(ui.focus.is_none());
    }
    #[test]
    fn buttons_fire_on_release_over_the_same_control_only() {
        let (_root, mut engine) = session();
        let mut ui = Ui::new();
        let _ = window(&mut ui, &engine, 3, 900, 680, 1.0);
        let toggle = hit_for(
            &ui,
            |a| matches!(a, Action::Command(c) if c["action"] == "settings"),
        )
        .rect
        .clone();
        let before = engine.store.settings["restore"].as_bool().unwrap_or(true);
        let (x, y) = center(&toggle);
        ui.press(x, y, &mut engine);
        ui.release(5.0, 5.0, &mut engine);
        assert_eq!(
            engine.store.settings["restore"].as_bool().unwrap_or(true),
            before,
            "dragging off cancels"
        );
        ui.click(x, y, &mut engine);
        assert_ne!(
            engine.store.settings["restore"].as_bool().unwrap_or(true),
            before
        );
    }
    #[test]
    fn dragging_a_slider_sets_a_snapped_value() {
        let (root, mut engine) = session();
        let file = root.path().join("gifs/tiny.gif");
        tiny_gif(&file);
        engine.dispatch(&json!({"action":"spawn","gif":file,"id":"one"}));
        let mut ui = Ui::new();
        let _ = window(&mut ui, &engine, 1, 1280, 800, 1.0);
        let slider = hit_for(
            &ui,
            |a| matches!(a, Action::Slider { key, .. } if key == "scale"),
        )
        .rect
        .clone();
        let middle = slider.x as f64 + 2.0 + 9.0 + (slider.w - 4 - 18) as f64 / 2.0;
        ui.press(middle, slider.y as f64 + 8.0, &mut engine);
        ui.release(middle, slider.y as f64 + 8.0, &mut engine);
        let scale = engine.widgets["one"].settings.scale;
        assert!((scale - 2.55).abs() < 0.011, "scale {scale}");
        assert!(
            ((scale * 100.0).round() - scale * 100.0).abs() < 1e-6,
            "snapped to 1 %"
        );
    }
    #[test]
    fn floating_panels_are_rounded_and_the_window_is_flat() {
        let (_root, engine) = session();
        let mut ui = Ui::new();
        ui.tab = 0;
        ui.windowed = false;
        let panel = ui.draw(&engine, 600, 400, 1.0);
        assert_eq!(
            panel.pixel(0, 0).unwrap().alpha(),
            0,
            "panel corners are transparent"
        );
        assert_eq!(panel.pixel(300, 380).unwrap().alpha(), 255);
        ui.windowed = true;
        let flat = ui.draw(&engine, 600, 400, 1.0);
        assert_eq!(flat.pixel(0, 0).unwrap().alpha(), 255);
    }
    #[test]
    fn keyboard_focus_follows_controls_by_identity_and_scrolls_into_view() {
        let (_root, mut engine) = session();
        for n in 0..30 {
            engine.store.profiles[format!("Layout {n:02}")] = json!({"widgets":[]});
        }
        let mut ui = Ui::new();
        let _ = window(&mut ui, &engine, 2, 900, 480, 1.0);
        ui.blur();
        for _ in 0..90 {
            ui.focus_next(false, &mut engine);
            let _ = ui.draw(&engine, 900, 480, 1.0);
        }
        let focused = ui.keyboard_focus.clone().expect("a control is focused");
        let hit = ui
            .hits
            .iter()
            .find(|h| h.action == focused)
            .expect("focused control still exists");
        assert!(
            hit.rect.h > 0 || !hit.scrolls,
            "focused row was scrolled into view: {:?}",
            hit.rect
        );
        assert!(ui.scroll > 0);
    }
    #[test]
    fn arrow_keys_move_between_tiles_in_a_grid() {
        let (root, mut engine) = session();
        for n in 0..12 {
            tiny_gif(&root.path().join(format!("gifs/g{n:02}.gif")));
        }
        let mut ui = Ui::new();
        ui.refresh(&mut engine).unwrap();
        let _ = window(&mut ui, &engine, 0, 1100, 700, 1.0);
        ui.blur();
        for _ in 0..20 {
            ui.focus_next(false, &mut engine);
            if matches!(ui.keyboard_focus, Some(Action::ToggleGif(_))) {
                break;
            }
        }
        let _ = ui.draw(&engine, 1100, 700, 1.0);
        let first = ui.keyboard_focus.clone();
        assert!(matches!(first, Some(Action::ToggleGif(_))));
        ui.key(Key::Right, Mods::default(), &mut engine);
        let _ = ui.draw(&engine, 1100, 700, 1.0);
        let second = ui.keyboard_focus.clone();
        assert_ne!(first, second);
        ui.key(Key::Down, Mods::default(), &mut engine);
        ui.key(Key::Left, Mods::default(), &mut engine);
        assert!(matches!(ui.keyboard_focus, Some(Action::ToggleGif(_))));
    }
    #[test]
    fn the_overlay_scrollbar_can_be_dragged() {
        let (_root, mut engine) = session();
        for n in 0..40 {
            engine.store.profiles[format!("Layout {n:02}")] = json!({"widgets":[]});
        }
        let mut ui = Ui::new();
        let _ = window(&mut ui, &engine, 2, 900, 480, 1.0);
        let bar = hit_for(&ui, |a| *a == Action::Scrollbar).rect.clone();
        let top = ui.viewport_top;
        ui.press(bar.x as f64 + 8.0, top as f64 + 4.0, &mut engine);
        assert!(ui.scroll_drag.is_some());
        ui.motion(bar.x as f64 + 8.0, top as f64 + 120.0, &mut engine);
        let first = ui.scroll;
        assert!(first > 0, "dragging the thumb down scrolls the page");
        ui.motion(bar.x as f64 + 8.0, top as f64 + 1000.0, &mut engine);
        assert_eq!(ui.scroll, ui.scroll_max, "the thumb stops at the end");
        ui.release(bar.x as f64, 0.0, &mut engine);
        assert!(ui.scroll_drag.is_none());
    }
    #[test]
    fn an_unreadable_subfolder_does_not_hide_the_rest_of_the_collection() {
        use std::os::unix::fs::PermissionsExt;
        if unsafe { libc::geteuid() } == 0 {
            return; // root ignores directory permissions
        }
        let (root, engine) = session();
        std::fs::create_dir(root.path().join("gifs/locked")).unwrap();
        tiny_gif(&root.path().join("gifs/visible.gif"));
        std::fs::set_permissions(
            root.path().join("gifs/locked"),
            std::fs::Permissions::from_mode(0o000),
        )
        .unwrap();
        let files = engine.paths.collection().unwrap();
        std::fs::set_permissions(
            root.path().join("gifs/locked"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        assert_eq!(files.len(), 1);
    }
    #[test]
    fn regaining_focus_picks_up_gifs_added_in_the_meantime() {
        let (root, mut engine) = session();
        tiny_gif(&root.path().join("gifs/first.gif"));
        let mut ui = Ui::new();
        ui.open = true;
        ui.refresh(&mut engine).unwrap();
        assert_eq!(ui.filtered().len(), 1);
        tiny_gif(&root.path().join("gifs/second.gif"));
        ui.focus_gained(&engine);
        assert_eq!(
            ui.filtered().len(),
            2,
            "the new file is listed without a manual refresh"
        );
    }
    #[test]
    fn a_closed_window_never_requests_the_tile_it_last_hovered() {
        let (root, mut engine) = session();
        let gif = root.path().join("gifs/spin.gif");
        tiny_gif(&gif);
        let mut ui = Ui::new();
        ui.refresh(&mut engine).unwrap();
        let _ = window(&mut ui, &engine, 0, 900, 600, 1.0);
        ui.prepare(&mut engine);
        let tile = hit_for(&ui, |a| matches!(a, Action::ToggleGif(_)))
            .rect
            .clone();
        let (x, y) = center(&tile);
        ui.motion(x, y, &mut engine);
        ui.request_hover(&mut engine);
        assert!(
            engine.assets.contains_key(&gif),
            "an open window requests the hovered tile"
        );
        ui.open = false;
        ui.prepare(&mut engine);
        engine.gc();
        assert!(engine.assets.is_empty(), "closing drops the cache");
        ui.request_hover(&mut engine);
        assert!(
            engine.assets.is_empty(),
            "no request after close: it would spin the daemon"
        );
        assert!(ui.preview_deadline(&engine).is_none());
    }
}
