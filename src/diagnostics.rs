//! Bounded asynchronous JSONL diagnostics. The renderer never waits for disk writes.
use crate::{
    ipc,
    paths::{Paths, private_dir},
};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
        mpsc::{self, SyncSender},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const FILE_LIMIT: u64 = 8 * 1024 * 1024;
const ARCHIVES: usize = 4;
const QUEUE: usize = 256;
static LOGGER: OnceLock<SyncSender<Message>> = OnceLock::new();
static START: OnceLock<Instant> = OnceLock::new();
static LEVEL: AtomicU8 = AtomicU8::new(0);
static DROPPED: AtomicU64 = AtomicU64::new(0);
static IO_ERROR: Mutex<Option<String>> = Mutex::new(None);
static EXPORT_BUSY: AtomicBool = AtomicBool::new(false);
static EXPORT_REVISION: AtomicU64 = AtomicU64::new(0);
static EXPORT_RESULTS: Mutex<std::collections::VecDeque<Value>> =
    Mutex::new(std::collections::VecDeque::new());

#[derive(Clone, Copy)]
pub enum Level {
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    Trace = 5,
}
impl Level {
    fn name(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }
}
pub fn parse_level(name: &str) -> Result<u8> {
    match name {
        "off" => Ok(0),
        "error" => Ok(1),
        "warn" => Ok(2),
        "info" => Ok(3),
        "debug" => Ok(4),
        "trace" => Ok(5),
        _ => bail!("Log level must be off, error, warn, info, debug or trace"),
    }
}
pub fn level_name() -> &'static str {
    match LEVEL.load(Ordering::Relaxed) {
        0 => "off",
        1 => "error",
        2 => "warn",
        3 => "info",
        4 => "debug",
        _ => "trace",
    }
}
pub fn set_level(name: &str) -> Result<()> {
    LEVEL.store(parse_level(name)?, Ordering::Relaxed);
    Ok(())
}
pub fn enabled(level: Level) -> bool {
    LOGGER.get().is_some() && LEVEL.load(Ordering::Relaxed) >= level as u8
}
#[macro_export]
macro_rules! diag {
    ($level:ident, $component:expr, $event:expr, $fields:expr) => {
        if $crate::diagnostics::enabled($crate::diagnostics::Level::$level) {
            $crate::diagnostics::record(
                $crate::diagnostics::Level::$level,
                $component,
                $event,
                $fields,
            );
        }
    };
}
enum Message {
    Record(Vec<u8>),
    Flush(mpsc::Sender<std::result::Result<(), String>>),
    Copy(
        PathBuf,
        mpsc::Sender<std::result::Result<Vec<String>, String>>,
    ),
}
fn timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
pub fn record(level: Level, component: &str, event: &str, fields: Value) {
    let Some(sender) = LOGGER.get() else { return };
    let value = json!({"schema":1,"timestamp_unix_ms":timestamp_ms(),"elapsed_us":START.get().map(|s|s.elapsed().as_micros()),"pid":std::process::id(),"thread":std::thread::current().name().unwrap_or("main"),"level":level.name(),"component":component,"event":event,"fields":fields});
    let Ok(line) = encode_record(value) else {
        return;
    };
    if sender.try_send(Message::Record(line)).is_err() {
        DROPPED.fetch_add(1, Ordering::Relaxed);
    }
}
pub fn log_dir(paths: &Paths) -> PathBuf {
    paths.state_dir.join("logs")
}
pub fn status(paths: &Paths) -> Value {
    json!({"directory":log_dir(paths),"level":level_name(),"writer_running":LOGGER.get().is_some(),"dropped_events":DROPPED.load(Ordering::Relaxed),"io_error":IO_ERROR.lock().ok().and_then(|e|e.clone()),"file_limit_bytes":FILE_LIMIT,"archives":ARCHIVES,"queue_capacity":QUEUE,"export_busy":export_busy(),"last_export":EXPORT_RESULTS.lock().ok().and_then(|e|e.back().cloned())})
}
pub fn export_busy() -> bool {
    EXPORT_BUSY.load(Ordering::Relaxed)
}
pub fn export_revision() -> u64 {
    EXPORT_REVISION.load(Ordering::Acquire)
}
pub fn export_status(directory: &str) -> Value {
    EXPORT_RESULTS
        .lock()
        .ok()
        .and_then(|results| {
            results
                .iter()
                .find(|r| r["directory"] == directory)
                .cloned()
        })
        .unwrap_or_else(
            || json!({"ok":false,"error":"Unknown export directory","directory":directory}),
        )
}
pub fn start_export(
    paths: Paths,
    requested: Option<&str>,
    snapshot: Value,
    mut notify: std::os::unix::net::UnixStream,
) -> Result<Value> {
    let directory = export_directory(&paths, requested)?;
    ensure!(
        !directory.exists(),
        "Export directory must not exist: {}",
        directory.display()
    );
    ensure!(
        EXPORT_BUSY
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok(),
        "Another log export is running"
    );
    if let Ok(mut results) = EXPORT_RESULTS.lock() {
        results.retain(|r| r["directory"] != json!(directory));
        if results.len() >= 8 {
            results.pop_front();
        }
        results.push_back(json!({"ok":true,"pending":true,"directory":directory}));
    }
    let target = directory.clone();
    let spawned = std::thread::Builder::new()
        .name("gif-export".into())
        .spawn(move || {
            let report = match export(&paths, target.to_str(), snapshot) {
                Ok(value) => value,
                Err(error) => json!({"ok":false,"directory":target,"error":format!("{error:#}")}),
            };
            crate::diag!(
                Info,
                "diagnostics",
                "export_finished",
                json!({"report":report})
            );
            if let Ok(mut results) = EXPORT_RESULTS.lock() {
                if let Some(previous) = results
                    .iter_mut()
                    .find(|r| r["directory"] == report["directory"])
                {
                    *previous = report;
                }
            }
            EXPORT_BUSY.store(false, Ordering::Release);
            EXPORT_REVISION.fetch_add(1, Ordering::Release);
            let _ = notify.write_all(&[1]);
        });
    if let Err(error) = spawned {
        if let Ok(mut results) = EXPORT_RESULTS.lock() {
            if let Some(result) = results.back_mut() {
                *result = json!({"ok":false,"directory":directory,"error":error.to_string()});
            }
        }
        EXPORT_BUSY.store(false, Ordering::Release);
        return Err(error.into());
    }
    Ok(json!({"ok":true,"pending":true,"directory":directory}))
}
pub fn init(paths: &Paths) -> Result<()> {
    let directory = log_dir(paths);
    private_dir(&directory)?;
    let writer = Writer::new(directory, FILE_LIMIT)?;
    let configured = crate::store::read(&paths.config_dir.join("settings.json"))
        .ok()
        .and_then(|v| v["log_level"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "info".into());
    let level = std::env::var("GIF_PLAYER_LOG").unwrap_or(configured);
    set_level(&level)?;
    let (tx, rx) = mpsc::sync_channel(QUEUE);
    LOGGER
        .set(tx)
        .map_err(|_| anyhow::anyhow!("Logger already initialized"))?;
    START.get_or_init(Instant::now);
    std::thread::Builder::new().name("gif-log".into()).spawn(move || {
        let mut writer = writer;
        let mut last_dropped = 0;
        loop {
            let message = if writer.pending {rx.recv_timeout(Duration::from_secs(1))} else {rx.recv().map_err(|_|mpsc::RecvTimeoutError::Disconnected)};
            let result = match message {
                Ok(Message::Record(line)) => writer.write_line(line),
                Ok(Message::Flush(reply)) => {
                    let result = writer.flush();
                    let _ = reply.send(result.as_ref().map(|_|()).map_err(|e|e.to_string()));
                    result
                }
                Ok(Message::Copy(destination, reply)) => {
                    let result = writer.copy(&destination);
                    let _ = reply.send(result.as_ref().cloned().map_err(|e|e.to_string()));
                    result.map(|_|())
                }
                Err(mpsc::RecvTimeoutError::Timeout) => writer.flush(),
                Err(mpsc::RecvTimeoutError::Disconnected) => { let _ = writer.flush(); break; }
            };
            if let Err(error) = result {
                if let Ok(mut stored) = IO_ERROR.lock() {
                    if stored.is_none() { eprintln!("gif-player diagnostics: {error:#}"); }
                    *stored = Some(format!("{error:#}"));
                }
            }
            let dropped = DROPPED.load(Ordering::Relaxed);
            if dropped != last_dropped {
                let _ = writer.write(json!({"schema":1,"timestamp_unix_ms":timestamp_ms(),"pid":std::process::id(),"level":"warn","component":"logging","event":"queue_overflow","fields":{"lost_events":dropped-last_dropped,"total_lost":dropped}}));
                last_dropped = dropped;
            }
        }
    })?;
    crate::diag!(
        Info,
        "daemon",
        "logging_started",
        json!({"version":env!("CARGO_PKG_VERSION"),"paths":paths,"logging":status(paths)})
    );
    std::panic::set_hook(Box::new(|info| {
        record(
            Level::Error,
            "daemon",
            "panic",
            json!({"message":info.to_string(),"backtrace":std::backtrace::Backtrace::force_capture().to_string()}),
        );
        let _ = flush();
        eprintln!("gif-player: {info}");
    }));
    Ok(())
}
pub fn flush() -> Result<()> {
    let Some(sender) = LOGGER.get() else {
        return Ok(());
    };
    let (tx, rx) = mpsc::channel();
    sender
        .try_send(Message::Flush(tx))
        .context("Log queue is full; retry export")?;
    rx.recv_timeout(Duration::from_secs(3))
        .context("Log writer flush timeout")?
        .map_err(anyhow::Error::msg)
}
struct Writer {
    directory: PathBuf,
    file: BufWriter<File>,
    bytes: u64,
    limit: u64,
    pending: bool,
    _lock: File,
}
fn encode_record(mut value: Value) -> Result<Vec<u8>> {
    let mut line = serde_json::to_vec(&value)?;
    if line.len() > 8191 {
        value["fields"] = json!({"truncated":true,"original_bytes":line.len()});
        line = serde_json::to_vec(&value)?;
    }
    ensure!(line.len() <= 8191, "Diagnostic envelope too large");
    line.push(b'\n');
    Ok(line)
}
fn open_log(path: &Path) -> Result<BufWriter<File>> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    ensure!(file.metadata()?.is_file(), "Log is not a regular file");
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(BufWriter::with_capacity(32 * 1024, file))
}
impl Writer {
    fn new(directory: PathBuf, limit: u64) -> Result<Self> {
        let lock = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(directory.join("logging.lock"))?;
        ensure!(
            unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
            "Another daemon owns this log directory"
        );
        let file = open_log(&directory.join("daemon.jsonl"))?;
        let bytes = file.get_ref().metadata()?.len();
        Ok(Self {
            directory,
            file,
            bytes,
            limit,
            pending: false,
            _lock: lock,
        })
    }
    fn flush(&mut self) -> Result<()> {
        self.file.flush()?;
        self.pending = false;
        Ok(())
    }
    fn write(&mut self, value: Value) -> Result<()> {
        self.write_line(encode_record(value)?)
    }
    fn write_line(&mut self, line: Vec<u8>) -> Result<()> {
        if self.bytes + line.len() as u64 > self.limit {
            self.flush()?;
            let last = self.directory.join(format!("daemon.{ARCHIVES}.jsonl"));
            if last.exists() {
                fs::remove_file(last)?;
            }
            for n in (1..ARCHIVES).rev() {
                let from = self.directory.join(format!("daemon.{n}.jsonl"));
                if from.exists() {
                    fs::rename(from, self.directory.join(format!("daemon.{}.jsonl", n + 1)))?;
                }
            }
            fs::rename(
                self.directory.join("daemon.jsonl"),
                self.directory.join("daemon.1.jsonl"),
            )?;
            self.file = open_log(&self.directory.join("daemon.jsonl"))?;
            self.bytes = 0;
        }
        self.file.write_all(&line)?;
        self.pending = true;
        self.bytes += line.len() as u64;
        Ok(())
    }
    fn copy(&mut self, destination: &Path) -> Result<Vec<String>> {
        self.flush()?;
        copy_logs(&self.directory, destination)
    }
}
fn copy_file(source: &Path, destination: &Path, limit: u64) -> Result<()> {
    let mut input = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(source)?;
    let meta = input.metadata()?;
    ensure!(
        meta.is_file() && meta.len() <= limit,
        "Diagnostic source exceeds limit or is not a regular file: {}",
        source.display()
    );
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(destination)?;
    std::io::copy(&mut Read::by_ref(&mut input).take(limit), &mut output)?;
    output.sync_all()?;
    Ok(())
}
fn copy_logs(directory: &Path, destination: &Path) -> Result<Vec<String>> {
    let mut copied = Vec::new();
    for name in std::iter::once("daemon.jsonl".to_string())
        .chain((1..=ARCHIVES).map(|n| format!("daemon.{n}.jsonl")))
    {
        let source = directory.join(&name);
        if source.exists() {
            copy_file(&source, &destination.join(&name), FILE_LIMIT + 8192)?;
            copied.push(name);
        }
    }
    Ok(copied)
}
fn write_json(directory: &Path, name: &str, value: &Value) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(directory.join(name))?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}
pub fn command_fields(cmd: &Value) -> Value {
    // Unknown state/settings fields may contain private data; never log arbitrary payloads.
    let mut fields = serde_json::Map::new();
    for key in [
        "action", "id", "gif", "x", "y", "dx", "dy", "scale", "opacity", "speed", "seconds",
        "mode", "position", "layer", "output", "enabled", "level",
    ] {
        if let Some(v) = cmd.get(key) {
            if v.is_boolean() || v.is_number() || v.is_null() {
                fields.insert(key.into(), v.clone());
            } else if let Some(s) = v.as_str() {
                fields.insert(key.into(), json!(s.chars().take(1024).collect::<String>()));
            }
        }
    }
    json!(fields)
}
fn safe_snapshot(mut value: Value) -> Value {
    value["profiles"] = json!(
        value["profiles"]
            .as_object()
            .map(|p| p.keys().collect::<Vec<_>>())
    );
    let settings = value["settings"].clone();
    value["settings"] = json!({});
    for key in ["restore", "gif_dir", "font_file", "log_level"] {
        if let Some(v) = settings
            .get(key)
            .filter(|v| v.is_boolean() || v.is_string())
        {
            value["settings"][key] = v.clone();
        }
    }
    if let Some(appearance) = settings.get("appearance") {
        value["settings"]["appearance"] =
            json!({"palette":appearance["palette"],"font_family":appearance["font_family"]});
    }
    // Widget status includes preserved legacy settings; keep only the engine's known fields.
    if let Some(widgets) = value["widgets"].as_array_mut() {
        let known = serde_json::to_value(crate::model::Settings::default()).unwrap();
        for widget in widgets {
            if let Some(fields) = widget.as_object_mut() {
                fields.retain(|key, _| {
                    known.get(key).is_some()
                        || [
                            "ok",
                            "id",
                            "file",
                            "gif",
                            "base_position",
                            "size",
                            "screen",
                            "frames",
                            "loading",
                            "error",
                            "warning",
                            "source_size",
                            "frame_storage",
                            "surface_mode",
                            "frame",
                        ]
                        .contains(&key.as_str())
                });
            }
        }
    }
    value
}
fn export_directory(paths: &Paths, requested: Option<&str>) -> Result<PathBuf> {
    let directory = requested.map(crate::paths::expand).unwrap_or_else(|| {
        paths.state_dir.join("exports").join(format!(
            "diagnostics-{}-{}",
            timestamp_ms(),
            std::process::id()
        ))
    });
    let directory = if directory.is_absolute() {
        directory
    } else {
        std::env::current_dir()?.join(directory)
    };
    ensure!(directory.to_str().is_some(), "Export path must be UTF-8");
    Ok(directory)
}
pub fn export(paths: &Paths, requested: Option<&str>, snapshot: Value) -> Result<Value> {
    let directory = export_directory(paths, requested)?;
    // A new directory prevents overwriting existing user reports or arbitrary files.
    let parent = directory
        .parent()
        .context("Export directory needs a parent")?;
    fs::create_dir_all(parent)?;
    fs::create_dir(&directory).with_context(|| {
        format!(
            "Create new export directory {} (must not exist)",
            directory.display()
        )
    })?;
    private_dir(&directory)?;
    let mut warnings = Vec::new();
    let copied = if let Some(sender) = LOGGER.get() {
        let (tx, rx) = mpsc::channel();
        sender
            .try_send(Message::Copy(directory.clone(), tx))
            .context("Log queue is full; retry export")?;
        rx.recv_timeout(Duration::from_secs(4))
            .context("Log export timeout")?
            .map_err(anyhow::Error::msg)?
    } else {
        match copy_logs(&log_dir(paths), &directory) {
            Ok(names) => names,
            Err(e) => {
                warnings.push(format!("Offline logs: {e:#}"));
                Vec::new()
            }
        }
    };
    for name in ["daemon.log", "daemon.log.1"] {
        let source = paths.runtime_dir.join(name);
        if source.exists() {
            if let Err(e) = copy_file(&source, &directory.join(name), 8 * 1024 * 1024) {
                warnings.push(format!("{name}: {e:#}"));
            }
        }
    }
    write_json(&directory, "snapshot.json", &safe_snapshot(snapshot))?;
    let mut environment = serde_json::Map::new();
    for key in [
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_TYPE",
        "WAYLAND_DISPLAY",
        "XDG_RUNTIME_DIR",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
        "LANG",
        "LC_ALL",
        "GIF_PLAYER_LOG",
    ] {
        if let Ok(value) = std::env::var(key) {
            environment.insert(key.into(), json!(value));
        }
    }
    let mut system = json!({"version":env!("CARGO_PKG_VERSION"),"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"paths":paths,"environment":environment,"logging":status(paths)});
    for (name, source) in [
        ("os_release", "/etc/os-release"),
        ("kernel", "/proc/sys/kernel/osrelease"),
    ] {
        if let Ok(file) = File::open(source) {
            let mut text = String::new();
            if file.take(64 * 1024).read_to_string(&mut text).is_ok() {
                system[name] = json!(text);
            }
        }
    }
    write_json(&directory, "system.json", &system)?;
    for (name, source) in [
        ("process-status.txt", "/proc/self/status"),
        ("process-io.txt", "/proc/self/io"),
        ("process-stat.txt", "/proc/self/stat"),
        ("process-limits.txt", "/proc/self/limits"),
    ] {
        if Path::new(source).exists() {
            if let Err(e) = copy_file(Path::new(source), &directory.join(name), 256 * 1024) {
                warnings.push(format!("{name}: {e:#}"));
            }
        }
    }
    let mut configuration = serde_json::Map::new();
    for name in [
        "state.json",
        "profiles.json",
        "settings.json",
        "session.json",
    ] {
        let file = paths.config_dir.join(name);
        configuration.insert(name.into(),match crate::store::read(&file) {
            Ok(value)=>json!({"present":file.exists(),"bytes":file.metadata().ok().map(|m|m.len()),"object_keys":value.as_object().map(|v|v.len()),"valid_json":true}),
            Err(error)=>json!({"valid_json":false,"error":format!("{error:#}")})
        });
    }
    write_json(&directory, "configuration.json", &json!(configuration))?;
    write_json(
        &directory,
        "manifest.json",
        &json!({"format":1,"product":"Noctalia GIF Player","version":env!("CARGO_PKG_VERSION"),"created_unix_ms":timestamp_ms(),"log_files":copied,"warnings":warnings,"privacy":"Contains GIF paths, widget positions, display names and selected session variables. No GIF pixels, arbitrary config fields, passwords or complete environment dump are included.","limitations":"Only this engine's events and captured stderr are included. Noctalia/compositor terminal logs are external. Offline process files describe the export client, not a stopped daemon."}),
    )?;
    Ok(json!({"ok":true,"directory":directory,"warnings":warnings}))
}
pub fn cli(paths: &Paths, args: &[String]) -> Result<i32> {
    match args.get(1).map(String::as_str).unwrap_or("path") {
        "path" => {
            ensure!(args.len() <= 2, "logs path takes no arguments");
            println!("{}", log_dir(paths).display());
        }
        "level" => {
            let level = args.get(2).context("Usage: gif-player logs level LEVEL")?;
            ensure!(args.len() == 3, "Unexpected log level arguments");
            parse_level(level)?;
            crate::cli::ensure_daemon(paths)?;
            return crate::cli::print_result(ipc::request(
                paths,
                &json!({"action":"logs-level","level":level}),
            )?);
        }
        "export" => {
            ensure!(
                args.len() <= 3,
                "Usage: gif-player logs export [NEW_DIRECTORY]"
            );
            let directory = export_directory(paths, args.get(2).map(String::as_str))?;
            let requested = directory.to_str();
            // Do not start a stopped daemon just to export a failure report.
            let mut result = match ipc::request(
                paths,
                &json!({"action":"logs-export","directory":requested}),
            ) {
                Ok(result) => result,
                Err(error) if error.chain().any(|e| e.to_string() == "Daemon not running") => {
                    export(
                        paths,
                        requested,
                        json!({"connected":false,"error":format!("{error:#}")}),
                    )?
                }
                Err(error) => {
                    return Err(
                        error.context("Live daemon export failed; no offline report substituted")
                    );
                }
            };
            let started = Instant::now();
            while result["pending"] == true {
                ensure!(
                    started.elapsed() < Duration::from_secs(30),
                    "Export still running in {}; check logs-export-status",
                    directory.display()
                );
                std::thread::sleep(Duration::from_millis(50));
                result = ipc::request(
                    paths,
                    &json!({"action":"logs-export-status","directory":directory}),
                )?;
            }
            return crate::cli::print_result(result);
        }
        _ => bail!("Usage: gif-player logs path | level LEVEL | export [NEW_DIRECTORY]"),
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rotation_is_bounded_and_export_is_private() {
        let root = tempfile::tempdir().unwrap();
        let logs = root.path().join("logs");
        private_dir(&logs).unwrap();
        let mut writer = Writer::new(logs.clone(), 512).unwrap();
        for i in 0..50 {
            writer
                .write(json!({"i":i,"message":"x".repeat(100)}))
                .unwrap();
        }
        let export = root.path().join("export");
        private_dir(&export).unwrap();
        let files = writer.copy(&export).unwrap();
        assert_eq!(files.len(), ARCHIVES + 1);
        for name in files {
            let path = export.join(name);
            assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o600);
            for line in fs::read_to_string(path).unwrap().lines() {
                let _: Value = serde_json::from_str(line).unwrap();
            }
        }
        assert!(writer.copy(&export).is_err(), "must not overwrite reports");
    }
    #[test]
    fn payloads_do_not_log_unknown_fields() {
        let cmd = json!({"action":"settings","settings":{"password":"sensitive"},"token":"sensitive","id":"bird"});
        let fields = command_fields(&cmd);
        assert_eq!(fields, json!({"action":"settings","id":"bird"}));
        assert!(parse_level("verbose").is_err());
        assert_eq!(parse_level("trace").unwrap(), 5);
        let line = encode_record(json!({"event":"large","fields":{"detail":"x".repeat(100_000)}}))
            .unwrap();
        assert!(line.len() <= 8192);
        let parsed: Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(parsed["fields"]["truncated"], true);
    }
}
