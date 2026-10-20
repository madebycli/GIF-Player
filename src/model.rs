use crate::{
    decode::{self, Animation},
    paths::Paths,
    store::Store,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::Write,
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Instant,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub x: f64,
    pub y: f64,
    pub scale: f64,
    pub opacity: f64,
    pub speed: f64,
    pub locked: bool,
    pub paused: bool,
    pub visible: bool,
    pub flip_h: bool,
    pub flip_v: bool,
    pub bouncing: bool,
    pub jumping: bool,
    pub jump_rate: f64,
    pub layer: String,
    pub canvas: bool,
    pub output: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            x: 100.0,
            y: 100.0,
            scale: 0.7,
            opacity: 1.0,
            speed: 1.0,
            locked: true,
            paused: false,
            visible: true,
            flip_h: false,
            flip_v: false,
            bouncing: false,
            jumping: false,
            jump_rate: 6.0,
            layer: "overlay".into(),
            canvas: false,
            output: String::new(),
            extra: BTreeMap::new(),
        }
    }
}
impl Settings {
    pub fn validate(&mut self) -> Result<()> {
        ensure!(
            [
                self.x,
                self.y,
                self.scale,
                self.opacity,
                self.speed,
                self.jump_rate
            ]
            .iter()
            .all(|v| v.is_finite()),
            "Values must be finite"
        );
        ensure!(
            self.x.abs() <= 1e7 && self.y.abs() <= 1e7,
            "Position exceeds 10 million logical pixels"
        );
        self.scale = self.scale.clamp(0.1, 5.0);
        self.opacity = self.opacity.clamp(0.0, 1.0);
        self.speed = self.speed.clamp(0.1, 10.0);
        self.jump_rate = self.jump_rate.clamp(0.7, 60.0);
        ensure!(
            ["background", "bottom", "top", "overlay"].contains(&self.layer.as_str()),
            "Unknown layer"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Output {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}
pub struct Asset {
    pub animation: Option<Arc<Animation>>,
    pub error: Option<String>,
    pub reserved: usize,
    cancel: Arc<AtomicBool>,
}
pub struct Widget {
    pub id: String,
    pub path: PathBuf,
    pub settings: Settings,
    pub frame: usize,
    pub position: (f64, f64),
    pub size: (u32, u32),
    pub bounds: (u32, u32),
    pub dirty: bool,
    pub started: f64,
    pub media_ms: f64,
    pub hop: Option<f64>,
    pub next_jump: f64,
    pub bounce_origin: (f64, f64),
    pub bounce_start: f64,
}
impl Widget {
    fn base_at(&self, now: f64) -> (f64, f64) {
        if self.settings.bouncing {
            (
                bounce(
                    self.bounce_origin.0,
                    360.0,
                    now - self.bounce_start,
                    self.bounds.0 as f64,
                    self.size.0 as f64,
                ),
                bounce(
                    self.bounce_origin.1,
                    254.558,
                    now - self.bounce_start,
                    self.bounds.1 as f64,
                    self.size.1 as f64,
                ),
            )
        } else {
            (self.settings.x, self.settings.y)
        }
    }
    pub fn elapsed(&self, now: f64) -> f64 {
        self.media_ms
            + if self.settings.paused {
                0.0
            } else {
                (now - self.started) * 1000.0 * self.settings.speed
            }
    }
    pub fn sync_clock(&mut self, now: f64) {
        self.media_ms = self.elapsed(now);
        self.started = now;
    }
    pub fn status(&self, asset: Option<&Asset>) -> Value {
        let count = asset
            .and_then(|a| a.animation.as_ref())
            .map_or(0, |a| a.frames.len());
        let mut value = serde_json::to_value(&self.settings).unwrap();
        let obj = value.as_object_mut().unwrap();
        for (key,val) in json!({"ok":true,"id":self.id,"file":self.path,"gif":self.path,"x":self.position.0,"y":self.position.1,"base_position":[self.settings.x,self.settings.y],"size":[self.size.0,self.size.1],"screen":[self.bounds.0,self.bounds.1],"frames":[count,count],"loading":asset.is_some_and(|a| a.animation.is_none()&&a.error.is_none()),"error":asset.and_then(|a| a.error.as_ref()),"warning":asset.and_then(|a| a.animation.as_ref()).and_then(|a| a.warning.as_ref()),"source_size":asset.and_then(|a|a.animation.as_ref()).map(|a|[a.width,a.height]),"frame_storage":asset.and_then(|a|a.animation.as_ref()).map(|a|if a.backing.is_some(){"lossless-disk"}else{"memory"}),"surface_mode":if self.canvas(){"canvas"}else{"compact"},"frame":self.frame}).as_object().unwrap() { obj.insert(key.clone(), val.clone()); }
        // An absent error must stay absent for protocol-v2 clients.
        if obj.get("error").is_some_and(Value::is_null) {
            obj.remove("error");
        }
        value
    }
    pub fn canvas(&self) -> bool {
        self.settings.canvas
            || !self.settings.locked
            || self.settings.bouncing
            || self.hop.is_some()
    }
}
struct DecodeJob {
    path: PathBuf,
    budget: usize,
    thumbnail: bool,
    cancel: Arc<AtomicBool>,
}
struct DecodeResult {
    path: PathBuf,
    result: Result<Animation>,
    thumbnail: bool,
    cancel: Arc<AtomicBool>,
}
pub struct Engine {
    pub paths: Paths,
    pub store: Store,
    pub widgets: BTreeMap<String, Widget>,
    pub assets: HashMap<PathBuf, Asset>,
    pub outputs: Vec<Output>,
    pub revision: u64,
    pub quit: bool,
    pub ui_request: Option<String>,
    pub managed_frontend: bool,
    pub frontend_request: Option<Value>,
    pub error: Option<String>,
    clock: Instant,
    random: u64,
    jobs: mpsc::SyncSender<DecodeJob>,
    results: mpsc::Receiver<DecodeResult>,
    pub wake: UnixStream,
    notify: UnixStream,
    export_revision: u64,
    pub previews: HashSet<PathBuf>,
    pub thumbnails: HashMap<PathBuf, Arc<Animation>>,
    thumb_pending: HashSet<PathBuf>,
    pub next_save: Option<f64>,
    pub deadline: Option<f64>,
    status_deadline: f64,
}
impl Engine {
    pub fn new(mut paths: Paths) -> Result<Self> {
        let store = Store::open(&paths)?;
        if !paths.gif_dir_explicit {
            if let Some(directory) = store.settings.get("gif_dir").and_then(Value::as_str) {
                let path = crate::paths::expand(directory);
                if path.is_dir() {
                    paths.gif_dir = path;
                }
            }
        }
        let (jobs, receiver) = mpsc::sync_channel::<DecodeJob>(32);
        let (sender, results) = mpsc::channel();
        let (wake, mut notify) = UnixStream::pair()?;
        wake.set_nonblocking(true)?;
        notify.set_nonblocking(true)?;
        let export_notify = notify.try_clone()?;
        std::thread::Builder::new()
            .name("gif-decode".into())
            .spawn(move || {
                while let Ok(job) = receiver.recv() {
                    let started = Instant::now();
                    crate::diag!(Debug,"decoder","job_started",json!({"path":job.path,"thumbnail":job.thumbnail,"budget_bytes":job.budget}));
                    let result = if job.thumbnail {
                        decode::thumbnail(&job.path)
                    } else {
                        decode::load_cancellable(&job.path, job.budget, &job.cancel)
                    };
                    match &result {
                        Ok(animation) => crate::diag!(Info,"decoder","job_finished",json!({"path":job.path,"thumbnail":job.thumbnail,"duration_ms":started.elapsed().as_secs_f64()*1000.0,"source_size":[animation.width,animation.height],"frames":animation.frames.len(),"resident_bytes":animation.bytes,"storage":if animation.backing.is_some(){"lossless-disk"}else{"memory"},"warning":animation.warning})),
                        Err(error) => crate::diag!(Warn,"decoder","job_failed",json!({"path":job.path,"thumbnail":job.thumbnail,"duration_ms":started.elapsed().as_secs_f64()*1000.0,"cancelled":job.cancel.load(Ordering::Relaxed),"error":format!("{error:#}")})),
                    }
                    if sender
                        .send(DecodeResult {
                            path: job.path,
                            result,
                            thumbnail: job.thumbnail,
                            cancel: job.cancel,
                        })
                        .is_err()
                    {
                        break;
                    }
                    let _ = notify.write_all(&[1]);
                }
            })?;
        Ok(Self {
            paths,
            store,
            widgets: BTreeMap::new(),
            assets: HashMap::new(),
            outputs: Vec::new(),
            revision: 0,
            quit: false,
            ui_request: None,
            managed_frontend: false,
            frontend_request: None,
            error: None,
            clock: Instant::now(),
            random: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos() as u64
                | 1,
            jobs,
            results,
            wake,
            notify: export_notify,
            export_revision: 0,
            previews: HashSet::new(),
            thumbnails: HashMap::new(),
            thumb_pending: HashSet::new(),
            next_save: None,
            deadline: None,
            status_deadline: 0.0,
        })
    }
    pub fn now(&self) -> f64 {
        self.clock.elapsed().as_secs_f64()
    }
    pub fn changed(&mut self) {
        self.revision += 1;
        self.next_save = Some(self.now() + 0.4);
    }
    pub fn drag_to(&mut self, id: &str, position: (f64, f64)) {
        let now = self.now();
        if let Some(widget) = self.widgets.get_mut(id) {
            if widget.settings.locked || !position.0.is_finite() || !position.1.is_finite() {
                return;
            }
            widget.settings.x = position.0.clamp(-1e7, 1e7);
            widget.settings.y = position.1.clamp(-1e7, 1e7);
            widget.settings.bouncing = false;
            widget.hop = None;
            widget.next_jump = now + widget.settings.jump_rate;
            widget.position = (widget.settings.x, widget.settings.y);
            crate::diag!(
                Trace,
                "input",
                "drag_motion",
                json!({"id":id,"position":widget.position,"bounds":widget.bounds,"size":widget.size})
            );
            widget.dirty = true;
            self.next_save = Some(now + 0.4);
            // Pointer events must not serialize snapshots or re-layout frontends at mouse rate.
            if now >= self.status_deadline {
                self.revision += 1;
                self.status_deadline = now + 0.25;
            }
        }
    }
    pub fn finish_drag(&mut self, id: &str) {
        if let Some(widget) = self.widgets.get_mut(id) {
            crate::diag!(
                Debug,
                "input",
                "drag_released",
                json!({"id":id,"position":widget.position})
            );
            widget.settings.x = snap(
                widget.settings.x,
                widget.bounds.0 as f64,
                widget.size.0 as f64,
            );
            widget.settings.y = snap(
                widget.settings.y,
                widget.bounds.1 as f64,
                widget.size.1 as f64,
            );
            widget.position = (widget.settings.x, widget.settings.y);
            widget.dirty = true;
            if let Err(error) = remember(&mut self.store, widget) {
                self.error = Some(error.to_string());
            }
            self.changed();
        }
    }
    fn random_interval(&mut self, mean: f64) -> f64 {
        self.random ^= self.random << 13;
        self.random ^= self.random >> 7;
        self.random ^= self.random << 17;
        let unit = ((self.random >> 11) as f64 + 1.0) / ((1u64 << 53) as f64 + 1.0);
        (-unit.ln() * mean).clamp(0.7, mean * 6.0)
    }
    pub fn request_asset(&mut self, path: PathBuf) -> Result<()> {
        if self.assets.contains_key(&path) {
            return Ok(());
        }
        let used: usize = self.assets.values().map(|a| a.reserved).sum();
        let available = (decode::CACHE_LIMIT - 16 * 1024 * 1024).saturating_sub(used);
        ensure!(
            available >= 1024 * 1024,
            "Decoded GIF cache budget exhausted; stop a running GIF or close the picker and try again"
        );
        let native = decode::native_dimensions(&path)?;
        let required = native.0 as usize * native.1 as usize * 4 + 1024 * 1024;
        ensure!(
            available >= required,
            "Not enough cache memory for one native-resolution frame; stop another GIF"
        );
        let budget = available.min((32 * 1024 * 1024).max(required));
        let cancel = Arc::new(AtomicBool::new(false));
        self.jobs
            .try_send(DecodeJob {
                path: path.clone(),
                budget,
                thumbnail: false,
                cancel: cancel.clone(),
            })
            .context("GIF decode queue is full")?;
        self.assets.insert(
            path,
            Asset {
                animation: None,
                error: None,
                reserved: budget,
                cancel,
            },
        );
        Ok(())
    }
    pub fn finish_loads(&mut self) {
        let now = self.now();
        while let Ok(loaded) = self.results.try_recv() {
            if loaded.thumbnail {
                self.thumb_pending.remove(&loaded.path);
                if self.previews.contains(&loaded.path) {
                    match loaded.result {
                        Ok(animation) => {
                            self.thumbnails.insert(loaded.path, Arc::new(animation));
                        }
                        Err(e) => self.error = Some(format!("Thumbnail: {e:#}")),
                    }
                    self.revision += 1;
                }
                continue;
            }
            if let Some(asset) = self.assets.get_mut(&loaded.path) {
                if !Arc::ptr_eq(&asset.cancel, &loaded.cancel) {
                    crate::diag!(
                        Debug,
                        "decoder",
                        "stale_result_discarded",
                        json!({"path":loaded.path})
                    );
                    continue;
                }
                match loaded.result {
                    Ok(animation) => {
                        asset.error = None;
                        asset.reserved = animation.bytes;
                        asset.animation = Some(Arc::new(animation));
                    }
                    Err(e) => {
                        asset.reserved = 0;
                        asset.error = Some(format!("{e:#}"));
                    }
                }
                for widget in self.widgets.values_mut().filter(|w| w.path == loaded.path) {
                    widget.dirty = true;
                    widget.started = now;
                    widget.media_ms = 0.0;
                }
                self.revision += 1;
            }
        }
    }
    pub fn gc(&mut self) {
        self.thumbnails
            .retain(|path, _| self.previews.contains(path));
        let active: HashSet<_> = self
            .widgets
            .values()
            .map(|w| w.path.clone())
            .chain(
                self.previews
                    .iter()
                    .filter(|path| self.assets.contains_key(*path))
                    .cloned(),
            )
            .collect();
        self.assets.retain(|path, asset| {
            if active.contains(path) {
                true
            } else {
                asset.cancel.store(true, Ordering::Relaxed);
                crate::diag!(Debug,"cache","asset_released",json!({"path":path,"reserved_bytes":asset.reserved,"pending":asset.animation.is_none()}));
                false
            }
        });
    }
    pub fn request_thumbnail(&mut self, path: PathBuf) -> Result<()> {
        if self.thumbnails.contains_key(&path)
            || self.thumb_pending.contains(&path)
            || self
                .assets
                .get(&path)
                .is_some_and(|a| a.animation.is_some())
        {
            return Ok(());
        }
        ensure!(self.thumb_pending.len() < 8, "Thumbnail queue is busy");
        self.jobs.try_send(DecodeJob {
            path: path.clone(),
            budget: 2 * 1024 * 1024,
            thumbnail: true,
            cancel: Arc::new(AtomicBool::new(false)),
        })?;
        self.thumb_pending.insert(path);
        Ok(())
    }
    pub fn snapshot(&self) -> Value {
        json!({"ok":true,"protocol":2,"product":"Noctalia GIF Player","engine":"rust","collection_dir":self.paths.gif_dir,"revision":self.revision,"widgets":self.widgets.values().map(|w|w.status(self.assets.get(&w.path))).collect::<Vec<_>>(),"outputs":self.outputs,"profiles":self.store.profiles,"settings":self.store.settings,"cache_bytes":self.assets.values().map(|a|a.reserved).sum::<usize>()+self.thumbnails.values().map(|a|a.bytes).sum::<usize>(),"frontend_request":self.frontend_request,"error":self.error,"logging":crate::diagnostics::status(&self.paths)})
    }
    pub fn restore(&mut self) -> Result<()> {
        crate::diag!(
            Info,
            "persistence",
            "restore_requested",
            json!({"enabled":self.store.settings.get("restore").and_then(Value::as_bool).unwrap_or(true)})
        );
        if self
            .store
            .settings
            .get("restore")
            .and_then(Value::as_bool)
            .unwrap_or(true)
        {
            let entries = self
                .store
                .session
                .get("widgets")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for entry in entries {
                if let Err(e) = self.spawn(&entry) {
                    crate::diag!(
                        Warn,
                        "persistence",
                        "restore_failed",
                        json!({"request":crate::diagnostics::command_fields(&entry),"error":format!("{e:#}")})
                    );
                    self.error = Some(format!("Restore: {e:#}"));
                }
            }
        }
        Ok(())
    }
    pub fn save(&mut self) -> Result<()> {
        let entries = self
            .widgets
            .values()
            .map(|w| {
                let mut state = serde_json::to_value(&w.settings).unwrap();
                state["gif"] = json!(w.path);
                state["id"] = json!(w.id);
                state
            })
            .collect::<Vec<_>>();
        for widget in self.widgets.values() {
            let stem = widget
                .path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy();
            if widget.id == stem {
                let mut old = self
                    .store
                    .states
                    .get(stem.as_ref())
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                if !old.is_object() {
                    old = json!({});
                }
                for (k, v) in serde_json::to_value(&widget.settings)?.as_object().unwrap() {
                    old[k] = v.clone();
                }
                self.store.states[stem.as_ref()] = old;
            }
        }
        self.store.session = json!({"widgets":entries});
        self.store.save()?;
        crate::diag!(
            Debug,
            "persistence",
            "saved",
            json!({"directory":self.paths.config_dir,"widgets":self.widgets.len()})
        );
        self.next_save = None;
        Ok(())
    }
    fn spawn(&mut self, cmd: &Value) -> Result<Value> {
        ensure!(self.widgets.len() < 256, "Maximum of 256 widgets reached");
        let path = self.paths.resolve(
            cmd.get("gif")
                .and_then(Value::as_str)
                .context("Missing gif")?,
        )?;
        let dimensions = decode::dimensions(&path)?;
        ensure!(
            cmd.get("state").is_none_or(Value::is_object),
            "state must be a JSON object"
        );
        let stem = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let id = if let Some(id) = cmd.get("id").and_then(Value::as_str) {
            id.to_string()
        } else {
            let mut id = stem.clone();
            let mut n = 2;
            while self.widgets.contains_key(&id) {
                id = format!("{stem}-{n}");
                n += 1;
            }
            id
        };
        ensure!(
            !id.is_empty() && id.len() <= 128 && id != "*",
            "Invalid widget id"
        );
        ensure!(
            !self.widgets.contains_key(&id),
            "Widget '{id}' already running"
        );
        let mut value = serde_json::to_value(Settings::default())?;
        if let Some(saved) = self.store.states.get(&stem).and_then(Value::as_object) {
            for (k, v) in saved {
                value[k] = v.clone();
            }
        }
        let override_state = cmd.get("state").unwrap_or(cmd);
        if let Some(state) = override_state.as_object() {
            for (k, v) in state {
                if !["action", "gif", "id", "monitor", "state"].contains(&k.as_str()) {
                    value[k] = v.clone();
                }
            }
        }
        let mut settings: Settings = serde_json::from_value(value)?;
        settings.validate()?;
        if let Some(index) = cmd.get("monitor").and_then(Value::as_u64) {
            if let Some(output) = self.outputs.get(index as usize) {
                settings.output = output.name.clone();
            }
        }
        if let Some(output) = cmd.get("output").and_then(Value::as_str) {
            settings.output = output.into();
        }
        if settings.output.is_empty() {
            if let Some(output) = self.outputs.first() {
                settings.output = output.name.clone();
            }
        }
        if id != stem && cmd.get("state").is_none() {
            settings.x += 30.0;
            settings.y += 30.0;
        }
        self.request_asset(path.clone())?;
        let now = self.now();
        let next_jump = now + self.random_interval(settings.jump_rate);
        let size = (
            (dimensions.0 as f64 * settings.scale).round().max(1.0) as u32,
            (dimensions.1 as f64 * settings.scale).round().max(1.0) as u32,
        );
        let widget = Widget {
            id: id.clone(),
            path,
            position: (settings.x, settings.y),
            bounce_origin: (settings.x, settings.y),
            settings,
            frame: 0,
            size,
            bounds: (1920, 1080),
            dirty: true,
            started: now,
            media_ms: 0.0,
            hop: None,
            next_jump,
            bounce_start: now,
        };
        let status = widget.status(self.assets.get(&widget.path));
        self.widgets.insert(id, widget);
        self.changed();
        Ok(status)
    }
    pub fn dispatch(&mut self, cmd: &Value) -> Value {
        let started = Instant::now();
        let result = match self.command(cmd) {
            Ok(value) => value,
            Err(e) => json!({"error":format!("{e:#}"),"ok":false}),
        };
        let fields = || json!({"request":crate::diagnostics::command_fields(cmd),"duration_us":started.elapsed().as_micros(),"widgets":self.widgets.len(),"revision":self.revision,"error":result.get("error")});
        if result.get("error").is_some_and(|v| !v.is_null()) {
            crate::diag!(Warn, "command", "failed", fields());
        } else if matches!(
            cmd["action"].as_str(),
            Some("ping" | "snapshot" | "list" | "watch" | "status" | "library" | "profiles")
        ) {
            crate::diag!(Trace, "command", "read", fields());
        } else {
            crate::diag!(Info, "command", "completed", fields());
        }
        result
    }
    fn command(&mut self, cmd: &Value) -> Result<Value> {
        ensure!(cmd.is_object(), "Request must be a JSON object");
        let action = cmd
            .get("action")
            .and_then(Value::as_str)
            .context("Missing action")?;
        match action {
            "logs-level" => {
                let level = cmd
                    .get("level")
                    .and_then(Value::as_str)
                    .context("Missing log level")?;
                crate::diagnostics::set_level(level)?;
                self.store.settings["log_level"] = json!(level);
                self.changed();
                return Ok(json!({"ok":true,"logging":crate::diagnostics::status(&self.paths)}));
            }
            "logs-export" => {
                let requested = cmd
                    .get("directory")
                    .filter(|v| !v.is_null())
                    .map(|v| v.as_str().context("Export directory must be a string"))
                    .transpose()?;
                crate::diag!(
                    Info,
                    "diagnostics",
                    "export_requested",
                    json!({"directory":requested})
                );
                let mut snapshot = self.snapshot();
                snapshot["diagnostics"] = json!({"uptime_seconds":self.now(),"asset_count":self.assets.len(),"thumbnail_count":self.thumbnails.len(),"pending_thumbnails":self.thumb_pending.len(),"managed_frontend":self.managed_frontend,"assets":self.assets.iter().map(|(path,asset)|json!({"path":path,"reserved_bytes":asset.reserved,"error":asset.error,"cancelled":asset.cancel.load(Ordering::Relaxed),"animation":asset.animation.as_ref().map(|a|json!({"source_size":[a.width,a.height],"logical_size":a.logical_size,"frames":a.frames.len(),"cycle_ms":a.ends_ms.last(),"delay_sample_ms":a.frames.iter().take(32).map(|f|f.delay_ms).collect::<Vec<_>>(),"storage":if a.backing.is_some(){"lossless-disk"}else{"memory"}}))})).collect::<Vec<_>>()});
                return crate::diagnostics::start_export(
                    self.paths.clone(),
                    requested,
                    snapshot,
                    self.notify.try_clone()?,
                );
            }
            "logs-export-status" => {
                return Ok(crate::diagnostics::export_status(
                    cmd.get("directory")
                        .and_then(Value::as_str)
                        .context("Missing export directory")?,
                ));
            }
            "ping" => {
                return Ok(
                    json!({"ok":true,"daemon":true,"widgets":self.widgets.len(),"protocol":2,"engine":"rust","pid":std::process::id()}),
                );
            }
            "list" | "snapshot" | "watch" => return Ok(self.snapshot()),
            "spawn" => return self.spawn(cmd),
            "picker" | "control" | "app" | "settings-ui" => {
                self.ui_request = Some(
                    if action == "settings-ui" {
                        "settings"
                    } else {
                        action
                    }
                    .into(),
                );
                return Ok(json!({"ok":true}));
            }
            "ui-close" => {
                self.ui_request = Some("close".into());
                return Ok(json!({"ok":true}));
            }
            "frontend-ack" => {
                if self
                    .frontend_request
                    .as_ref()
                    .is_some_and(|request| request["nonce"] == cmd["nonce"])
                {
                    self.frontend_request = None;
                }
                return Ok(json!({"ok":true}));
            }
            "stop-all" => {
                let n = self.widgets.len();
                for widget in self.widgets.values() {
                    remember(&mut self.store, widget)?;
                }
                self.widgets.clear();
                self.gc();
                self.changed();
                return Ok(json!({"ok":true,"stopped":n}));
            }
            "quit-daemon" => {
                self.quit = true;
                return Ok(json!({"ok":true,"daemon":false}));
            }
            "appearance" => {
                let palette = cmd
                    .get("palette")
                    .filter(|v| v.is_object())
                    .context("Missing palette")?;
                ensure!(
                    palette.as_object().unwrap().len() <= 24,
                    "Too many color roles"
                );
                for value in palette.as_object().unwrap().values() {
                    ensure!(
                        value.as_str().is_some_and(|s| s.len() == 7
                            && s.starts_with('#')
                            && s[1..].bytes().all(|c| c.is_ascii_hexdigit())),
                        "Invalid palette color"
                    );
                }
                let font = cmd
                    .get("font_family")
                    .and_then(Value::as_str)
                    .unwrap_or("sans-serif");
                ensure!(font.len() <= 256, "Font family too long");
                let style = crate::ui::theme::HostStyle::parse(cmd.get("style"))?;
                let border = cmd.get("panel_border_color").unwrap_or(&palette["outline"]);
                ensure!(
                    border.is_null() || crate::ui::theme::parse_hex(border).is_some(),
                    "Invalid panel border color"
                );
                let appearance = json!({"palette":palette,"font_family":font,"style":style,"panel_border_color":border});
                if self.store.settings["appearance"] != appearance {
                    crate::diag!(
                        Debug,
                        "appearance",
                        "changed",
                        json!({"palette":palette,"font_family":font})
                    );
                    self.store.settings["appearance"] = appearance;
                    for widget in self.widgets.values_mut() {
                        widget.dirty = true;
                    }
                    self.changed();
                }
                return Ok(json!({"ok":true}));
            }
            "settings" => {
                if let Some(settings) = cmd.get("settings").and_then(Value::as_object) {
                    if let Some(level) = settings.get("log_level") {
                        crate::diagnostics::set_level(
                            level.as_str().context("Log level must be a string")?,
                        )?;
                    }
                    for (k, v) in settings {
                        self.store.settings[k] = v.clone();
                    }
                    self.changed();
                }
                return Ok(json!({"ok":true,"settings":self.store.settings}));
            }
            "library" => {
                return Ok(
                    json!({"ok":true,"files":self.paths.collection()?.iter().map(|p|json!({"path":p,"name":p.file_stem().unwrap_or_default().to_string_lossy(),"category":p.parent().and_then(|p|p.strip_prefix(&self.paths.gif_dir).ok()).map(|p|p.to_string_lossy().to_string()).unwrap_or_default()})).collect::<Vec<_>>()}),
                );
            }
            "library-directory" => {
                let path = crate::paths::expand(
                    cmd.get("path")
                        .and_then(Value::as_str)
                        .context("Missing path")?,
                );
                ensure!(path.is_dir(), "Collection directory does not exist");
                self.paths.gif_dir = path.canonicalize()?;
                self.store.settings["gif_dir"] = json!(self.paths.gif_dir);
                self.changed();
                return Ok(json!({"ok":true}));
            }
            "profiles" => return Ok(json!({"ok":true,"profiles":self.store.profiles})),
            "profile-save" => {
                let name = cmd
                    .get("name")
                    .and_then(Value::as_str)
                    .context("Missing profile name")?;
                ensure!(
                    !name.is_empty() && name.len() <= 128,
                    "Invalid profile name"
                );
                let entries = self
                    .widgets
                    .values()
                    .map(|w| {
                        let mut v = serde_json::to_value(&w.settings).unwrap();
                        v["gif"] = json!(w.path);
                        v["id"] = json!(w.id);
                        v
                    })
                    .collect::<Vec<_>>();
                self.store.profiles[name] = json!({"widgets":entries});
                self.changed();
                return Ok(json!({"ok":true}));
            }
            "profile-delete" | "profile-rename" => {
                let name = cmd
                    .get("name")
                    .and_then(Value::as_str)
                    .context("Missing profile name")?;
                if action == "profile-rename" {
                    let new = cmd
                        .get("new_name")
                        .and_then(Value::as_str)
                        .context("Missing new_name")?;
                    ensure!(
                        !self.store.profiles.as_object().unwrap().contains_key(new)
                            && !new.is_empty(),
                        "Profile name already exists or empty"
                    );
                    let value = self
                        .store
                        .profiles
                        .as_object_mut()
                        .unwrap()
                        .remove(name)
                        .context("Profile not found")?;
                    self.store.profiles[new] = value;
                } else {
                    self.store
                        .profiles
                        .as_object_mut()
                        .unwrap()
                        .remove(name)
                        .context("Profile not found")?;
                }
                self.changed();
                return Ok(json!({"ok":true}));
            }
            "profile-apply" | "apply-setup" => {
                let entries = if action == "profile-apply" {
                    self.store
                        .profiles
                        .get(
                            cmd.get("name")
                                .and_then(Value::as_str)
                                .context("Missing name")?,
                        )
                        .and_then(|p| p.get("widgets"))
                        .context("Profile not found")?
                        .clone()
                } else {
                    cmd.get("widgets").context("Missing widgets")?.clone()
                };
                let entries = entries.as_array().context("widgets must be an array")?;
                // Validate every path and setting before replacing a working layout.
                ensure!(entries.len() <= 256, "Too many profile entries");
                for entry in entries {
                    self.paths.resolve(
                        entry
                            .get("gif")
                            .and_then(Value::as_str)
                            .context("Missing gif in profile")?,
                    )?;
                    let mut state: Settings = serde_json::from_value(entry.clone())?;
                    state.validate()?;
                }
                let old = std::mem::take(&mut self.widgets);
                let mut results = Vec::new();
                for entry in entries {
                    match self.spawn(entry) {
                        Ok(status) => results.push(status),
                        Err(e) => {
                            self.widgets = old;
                            self.gc();
                            bail!("Layout kept: {e:#}");
                        }
                    }
                }
                self.gc();
                self.changed();
                return Ok(json!({"ok":true,"applied":results.len(),"results":results}));
            }
            _ => {}
        }
        let id = cmd
            .get("id")
            .and_then(Value::as_str)
            .context("Action requires widget id")?;
        let ids = if id == "*" {
            self.widgets.keys().cloned().collect::<Vec<_>>()
        } else {
            ensure!(self.widgets.contains_key(id), "No widget '{id}' running");
            vec![id.into()]
        };
        let mut results = serde_json::Map::new();
        for id in ids {
            if action == "quit" || action == "stop" {
                if let Some(widget) = self.widgets.remove(&id) {
                    remember(&mut self.store, &widget)?;
                }
                results.insert(id, json!({"ok":true}));
                self.changed();
                continue;
            }
            let result = self.widget_action(&id, action, cmd);
            results.insert(
                id,
                result.unwrap_or_else(|e| json!({"error":e.to_string()})),
            );
        }
        self.gc();
        if id == "*" {
            Ok(json!({"ok":true,"results":results}))
        } else {
            Ok(results.remove(id).unwrap())
        }
    }
    fn widget_action(&mut self, id: &str, action: &str, cmd: &Value) -> Result<Value> {
        fn number(cmd: &Value, key: &str) -> Result<f64> {
            let n = cmd
                .get(key)
                .and_then(Value::as_f64)
                .with_context(|| format!("Missing numeric {key}"))?;
            ensure!(n.is_finite(), "Nonfinite number");
            Ok(n)
        }
        let now = self.now();
        let w = self.widgets.get_mut(id).unwrap();
        let old = w.settings.clone();
        match action {
            "status" => return Ok(w.status(self.assets.get(&w.path))),
            "lock" => w.settings.locked = true,
            "unlock" => w.settings.locked = false,
            "toggle" => w.settings.locked = !w.settings.locked,
            "pause" => {
                w.sync_clock(now);
                w.settings.paused = true;
            }
            "play" | "resume" => {
                w.sync_clock(now);
                w.settings.paused = false;
            }
            "hide" => w.settings.visible = false,
            "show" => w.settings.visible = true,
            "visibility" => {
                w.settings.visible = cmd
                    .get("visible")
                    .and_then(Value::as_bool)
                    .unwrap_or(!w.settings.visible)
            }
            "move" | "move-by" => {
                let (x, y) = if action == "move" {
                    (number(cmd, "x")?, number(cmd, "y")?)
                } else {
                    (
                        w.settings.x + number(cmd, "dx")?,
                        w.settings.y + number(cmd, "dy")?,
                    )
                };
                ensure!(x.abs() <= 1e7 && y.abs() <= 1e7, "Position out of range");
                w.settings.x = x;
                w.settings.y = y;
                w.settings.bouncing = false;
            }
            "scale" => {
                let scale = number(cmd, "scale")?.clamp(0.1, 5.0);
                let logical = self
                    .assets
                    .get(&w.path)
                    .and_then(|asset| asset.animation.as_ref())
                    .map(|animation| animation.logical_size)
                    .unwrap_or_else(|| {
                        (
                            (w.size.0 as f64 / w.settings.scale).round() as u32,
                            (w.size.1 as f64 / w.settings.scale).round() as u32,
                        )
                    });
                w.settings.x -=
                    (logical.0 as f64 * scale).round().max(1.0) / 2.0 - w.size.0 as f64 / 2.0;
                w.settings.y -=
                    (logical.1 as f64 * scale).round().max(1.0) / 2.0 - w.size.1 as f64 / 2.0;
                w.settings.scale = scale;
            }
            "opacity" => w.settings.opacity = number(cmd, "opacity")?.clamp(0.0, 1.0),
            "speed" => {
                let speed = number(cmd, "speed")?.clamp(0.1, 10.0);
                w.sync_clock(now);
                w.settings.speed = speed;
            }
            "flip" => match cmd
                .get("mode")
                .and_then(Value::as_str)
                .context("Missing flip mode")?
            {
                "none" => {
                    w.settings.flip_h = false;
                    w.settings.flip_v = false;
                }
                "h" => {
                    w.settings.flip_h = true;
                    w.settings.flip_v = false;
                }
                "v" => {
                    w.settings.flip_h = false;
                    w.settings.flip_v = true;
                }
                "hv" | "vh" | "both" => {
                    w.settings.flip_h = true;
                    w.settings.flip_v = true;
                }
                "toggle-h" => w.settings.flip_h = !w.settings.flip_h,
                "toggle-v" => w.settings.flip_v = !w.settings.flip_v,
                _ => bail!("Unknown flip mode"),
            },
            "bounce" => {
                let position = w.base_at(now);
                w.settings.bouncing = !w.settings.bouncing;
                if w.settings.bouncing {
                    w.bounce_origin = position;
                    w.bounce_start = now;
                } else {
                    w.settings.x = position.0;
                    w.settings.y = position.1;
                }
            }
            "stop-bounce" => {
                let position = w.base_at(now);
                w.settings.bouncing = false;
                w.settings.x = position.0;
                w.settings.y = position.1;
            }
            "hop" => w.hop = Some(now),
            "jump" => {
                w.settings.jumping = !w.settings.jumping;
                w.next_jump = now + w.settings.jump_rate;
            }
            "jump-rate" => {
                w.settings.jump_rate = number(cmd, "seconds")?.clamp(0.7, 60.0);
                w.next_jump = now + w.settings.jump_rate;
            }
            "corner" | "center" => {
                let position = if action == "center" {
                    "center"
                } else {
                    cmd.get("position")
                        .and_then(Value::as_str)
                        .context("Missing position")?
                };
                let maxx = w.bounds.0 as f64 - w.size.0 as f64;
                let maxy = w.bounds.1 as f64 - w.size.1 as f64;
                let (x, y) = match position {
                    "tl" => (20.0, 20.0),
                    "tr" => (maxx - 20.0, 20.0),
                    "bl" => (20.0, maxy - 20.0),
                    "br" => (maxx - 20.0, maxy - 20.0),
                    "center" => (maxx / 2.0, maxy / 2.0),
                    _ => bail!("Unknown corner"),
                };
                w.settings.x = x;
                w.settings.y = y;
                w.settings.bouncing = false;
            }
            "layer" => {
                let layer = cmd
                    .get("layer")
                    .and_then(Value::as_str)
                    .context("Missing layer")?;
                ensure!(
                    ["background", "bottom", "top", "overlay"].contains(&layer),
                    "Unknown layer"
                );
                w.settings.layer = layer.into();
            }
            "canvas" => {
                w.settings.canvas = cmd
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(!w.settings.canvas)
            }
            "output" => {
                let output = cmd
                    .get("output")
                    .and_then(Value::as_str)
                    .context("Missing output")?;
                ensure!(
                    self.outputs.iter().any(|o| o.name == output),
                    "Output not connected"
                );
                w.settings.output = output.into();
            }
            "reset" => {
                let locked = w.settings.locked;
                let output = w.settings.output.clone();
                w.settings = Settings::default();
                w.settings.locked = locked;
                w.settings.output = output;
                w.frame = 0;
                w.media_ms = 0.0;
                w.started = now;
                w.hop = None;
            }
            _ => bail!("Unknown action: {action}"),
        }
        if let Err(error) = w.settings.validate() {
            w.settings = old;
            return Err(error);
        }
        w.dirty = true;
        remember(&mut self.store, w)?;
        let result = w.status(self.assets.get(&w.path));
        self.changed();
        Ok(result)
    }
    pub fn tick(&mut self) {
        let export_revision = crate::diagnostics::export_revision();
        if self.export_revision != export_revision {
            self.export_revision = export_revision;
            self.revision += 1;
        }
        self.finish_loads();
        let now = self.now();
        let mut deadline: Option<f64> = None;
        let mut jumps = Vec::new();
        let mut motion_changed = false;
        for w in self.widgets.values_mut() {
            if let Some(output) = self
                .outputs
                .iter()
                .find(|o| o.name == w.settings.output)
                .or_else(|| self.outputs.first())
            {
                w.bounds = (output.width, output.height);
            }
            let Some(animation) = self.assets.get(&w.path).and_then(|a| a.animation.as_ref())
            else {
                continue;
            };
            let size = (
                (animation.logical_size.0 as f64 * w.settings.scale)
                    .round()
                    .max(1.0) as u32,
                (animation.logical_size.1 as f64 * w.settings.scale)
                    .round()
                    .max(1.0) as u32,
            );
            if size != w.size {
                w.size = size;
                w.dirty = true;
            }
            let (index, remaining) = animation.frame_at(w.elapsed(now));
            if index != w.frame {
                crate::diag!(
                    Trace,
                    "playback",
                    "frame_selected",
                    json!({"id":w.id,"from":w.frame,"to":index,"elapsed_media_ms":w.elapsed(now),"remaining_ms":remaining,"speed":w.settings.speed,"paused":w.settings.paused})
                );
                w.frame = index;
                w.dirty = true;
            }
            let mut position = w.base_at(now);
            if w.settings.jumping && now >= w.next_jump {
                w.hop = Some(now);
                jumps.push((w.id.clone(), w.settings.jump_rate));
            }
            if let Some(start) = w.hop {
                let t = (now - start) / 0.65;
                if t >= 1.0 {
                    w.hop = None;
                } else {
                    position.1 -= jump(t, 60.0);
                }
            }
            if w.position != position {
                w.position = position;
                w.dirty = true;
                motion_changed = true;
            }
            let visible = w.position.0 + w.size.0 as f64 > 0.0
                && w.position.0 < w.bounds.0 as f64
                && w.position.1 + w.size.1 as f64 > 0.0
                && w.position.1 - if w.settings.jumping { 60.0 } else { 0.0 } < w.bounds.1 as f64;
            if w.settings.visible && visible {
                if !w.settings.paused && animation.frames.len() > 1 {
                    minimum(&mut deadline, now + remaining / (1000.0 * w.settings.speed));
                }
                if w.settings.bouncing || w.hop.is_some() {
                    minimum(&mut deadline, now + 1.0 / 60.0);
                }
                if w.settings.jumping {
                    minimum(&mut deadline, w.next_jump);
                }
            }
        }
        for (id, mean) in jumps {
            let next = now + self.random_interval(mean);
            if let Some(w) = self.widgets.get_mut(&id) {
                w.next_jump = next;
            }
        }
        if motion_changed && now >= self.status_deadline {
            self.revision += 1;
            self.status_deadline = now + 0.25;
        }
        if let Some(save) = self.next_save {
            minimum(&mut deadline, save);
            if save <= now {
                if let Err(e) = self.save() {
                    crate::diag!(
                        Error,
                        "persistence",
                        "save_failed",
                        json!({"error":format!("{e:#}")})
                    );
                    self.error = Some(format!("Save failed: {e:#}"));
                    self.next_save = Some(now + 5.0);
                }
            }
        }
        self.deadline = deadline;
    }
}
fn minimum(value: &mut Option<f64>, next: f64) {
    *value = Some(value.map_or(next, |v| v.min(next)));
}
fn remember(store: &mut Store, widget: &Widget) -> Result<()> {
    let stem = widget
        .path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    if widget.id == stem {
        let mut value = store
            .states
            .get(stem.as_ref())
            .filter(|v| v.is_object())
            .cloned()
            .unwrap_or_else(|| json!({}));
        for (key, field) in serde_json::to_value(&widget.settings)?.as_object().unwrap() {
            value[key] = field.clone();
        }
        store.states[stem.as_ref()] = value;
    }
    Ok(())
}
pub fn jump(progress: f64, height: f64) -> f64 {
    if progress <= 0.0 || progress >= 1.0 {
        0.0
    } else {
        (progress * std::f64::consts::PI).sin() * height
    }
}
pub fn snap(position: f64, bound: f64, size: f64) -> f64 {
    for target in [0.0, (bound - size) / 2.0, bound - size] {
        if (position - target).abs() < 14.0 {
            return target;
        }
    }
    position
}
pub fn bounce(position: f64, speed: f64, elapsed: f64, bound: f64, size: f64) -> f64 {
    let extent = bound - size;
    if extent <= 0.0 {
        return extent / 2.0;
    }
    let phase = (position.clamp(0.0, extent) + speed * elapsed).rem_euclid(2.0 * extent);
    if phase <= extent {
        phase
    } else {
        2.0 * extent - phase
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drag_motion_is_direct_and_frontend_updates_are_bounded() {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths {
            runtime_dir: root.path().join("runtime"),
            config_dir: root.path().join("config"),
            cache_dir: root.path().join("cache"),
            data_dir: root.path().join("data"),
            state_dir: root.path().join("state"),
            gif_dir: root.path().join("gifs"),
            gif_dir_explicit: true,
        };
        let mut engine = Engine::new(paths).unwrap();
        let settings = Settings {
            locked: false,
            ..Settings::default()
        };
        engine.widgets.insert(
            "test".into(),
            Widget {
                id: "test".into(),
                path: root.path().join("test.gif"),
                settings,
                frame: 0,
                position: (100.0, 100.0),
                size: (100, 100),
                bounds: (1000, 800),
                dirty: false,
                started: 0.0,
                media_ms: 0.0,
                hop: None,
                next_jump: 10.0,
                bounce_origin: (0.0, 0.0),
                bounce_start: 0.0,
            },
        );
        for x in 430..470 {
            engine.drag_to("test", (x as f64, 200.0));
            assert_eq!(engine.widgets["test"].position, (x as f64, 200.0));
        }
        assert!(
            engine.revision <= 2,
            "Mouse motion must not rebuild the frontend for each event"
        );
        engine.finish_drag("test");
        assert_eq!(engine.widgets["test"].settings.x, 469.0);
    }
    #[test]
    fn media_clock_pause_speed_and_motion_base() {
        let mut widget = Widget {
            id: "test".into(),
            path: PathBuf::new(),
            settings: Settings::default(),
            frame: 0,
            position: (100.0, 40.0),
            size: (100, 100),
            bounds: (1000, 800),
            dirty: false,
            started: 1.0,
            media_ms: 0.0,
            hop: Some(1.0),
            next_jump: 10.0,
            bounce_origin: (100.0, 100.0),
            bounce_start: 1.0,
        };
        assert_eq!(widget.elapsed(1.25), 250.0);
        widget.sync_clock(1.25);
        widget.settings.paused = true;
        assert_eq!(widget.elapsed(100.0), 250.0);
        widget.sync_clock(100.0);
        widget.settings.paused = false;
        widget.settings.speed = 2.0;
        assert_eq!(widget.elapsed(100.25), 750.0);
        // A transient hop offset must not become the persistent base position.
        assert_eq!(widget.base_at(1.0), (100.0, 100.0));
        widget.settings.bouncing = true;
        assert_eq!(widget.base_at(1.25).0, 190.0);
    }
    #[test]
    fn motion_extremes() {
        assert_eq!(jump(0.0, 60.0), 0.0);
        assert_eq!(jump(1.0, 60.0), 0.0);
        assert!((jump(0.5, 60.0) - 60.0).abs() < 1e-8);
        assert_eq!(bounce(500.0, 360.0, 0.016, 100.0, 180.0), -40.0);
        for elapsed in [0.0, 0.1, 10.0, 1000000.0] {
            let x = bounce(20.0, 360.0, elapsed, 1920.0, 200.0);
            assert!((0.0..=1720.0).contains(&x));
        }
    }
}
