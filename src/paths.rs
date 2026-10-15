use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::{
    env, fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize)]
pub struct Paths {
    pub runtime_dir: PathBuf,
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
    pub gif_dir: PathBuf,
    #[serde(skip)]
    pub gif_dir_explicit: bool,
}
pub fn expand(path: &str) -> PathBuf {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));
    if path == "~" {
        home
    } else if let Some(rest) = path.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(path)
    }
}
fn xdg(key: &str, fallback: PathBuf) -> PathBuf {
    env::var_os(key)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or(fallback)
}
impl Paths {
    pub fn new(gif_dir: Option<&str>) -> Self {
        let gif_dir_explicit = gif_dir.is_some() || env::var_os("GIF_PLAYER_GIF_DIR").is_some();
        let home = expand("~");
        let config_dir = xdg("XDG_CONFIG_HOME", home.join(".config")).join("gif-player");
        let cache_dir = xdg("XDG_CACHE_HOME", home.join(".cache")).join("gif-player");
        let data_dir = xdg("XDG_DATA_HOME", home.join(".local/share")).join("gif-player");
        let state_dir = xdg("XDG_STATE_HOME", home.join(".local/state")).join("gif-player");
        let runtime_dir = env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .map(|p| p.join("gif-player"))
            .unwrap_or_else(|| {
                PathBuf::from(format!("/tmp/gif-player-{}", unsafe { libc::getuid() }))
            });
        let legacy = home.join("Scripts/Gif-Overlay/Gifs");
        let gif_dir = gif_dir
            .map(expand)
            .or_else(|| env::var("GIF_PLAYER_GIF_DIR").ok().map(|p| expand(&p)))
            .unwrap_or_else(|| {
                if legacy.is_dir() && !data_dir.join("gifs").exists() {
                    legacy
                } else {
                    data_dir.join("gifs")
                }
            });
        let mut gif_dir = if gif_dir.is_absolute() {
            gif_dir
        } else {
            env::current_dir().unwrap_or_default().join(gif_dir)
        };
        if !gif_dir_explicit {
            let settings = config_dir.join("settings.json");
            if settings
                .metadata()
                .is_ok_and(|m| m.len() < 16 * 1024 * 1024)
            {
                if let Ok(contents) = fs::read(settings) {
                    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&contents) {
                        if let Some(directory) =
                            value.get("gif_dir").and_then(serde_json::Value::as_str)
                        {
                            let path = expand(directory);
                            if path.is_dir() {
                                gif_dir = path;
                            }
                        }
                    }
                }
            }
        }
        Self {
            runtime_dir,
            config_dir,
            cache_dir,
            data_dir,
            state_dir,
            gif_dir,
            gif_dir_explicit,
        }
    }
    pub fn socket(&self) -> PathBuf {
        self.runtime_dir.join("daemon.sock")
    }
    pub fn ensure_runtime(&self) -> Result<()> {
        private_dir(&self.runtime_dir)
    }
    pub fn collection(&self) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        if self.gif_dir.exists() {
            scan(&self.gif_dir, &mut files, 0)?;
        }
        // Case-insensitive so "aurora.gif" and "Zebra.gif" order as people expect.
        files.sort_by_cached_key(|p| (p.to_string_lossy().to_lowercase(), p.clone()));
        Ok(files)
    }
    pub fn resolve(&self, name: &str) -> Result<PathBuf> {
        let candidate = expand(name);
        if candidate.is_file() {
            return Ok(candidate.canonicalize()?);
        }
        if self.gif_dir.join(&candidate).is_file() {
            return Ok(self.gif_dir.join(candidate).canonicalize()?);
        }
        let wanted = candidate
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        let matches: Vec<_> = self
            .collection()?
            .into_iter()
            .filter(|p| {
                p.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase()
                    == wanted
            })
            .collect();
        match matches.as_slice() {
            [path] => Ok(path.canonicalize()?),
            [] => bail!("GIF '{name}' not found in {}", self.gif_dir.display()),
            _ => bail!(
                "Ambiguous GIF '{name}': {}",
                matches
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}
fn scan(dir: &Path, files: &mut Vec<PathBuf>, depth: usize) -> Result<()> {
    ensure!(
        depth < 64 && files.len() < 100_000,
        "Collection exceeds scan limits"
    );
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        // One unreadable subfolder must not hide the rest of the collection.
        Err(_) if depth > 0 => return Ok(()),
        Err(error) => return Err(error).with_context(|| format!("Read {}", dir.display())),
    };
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            scan(&entry.path(), files, depth + 1)?;
        } else if (kind.is_file() || (kind.is_symlink() && entry.path().is_file()))
            && entry
                .path()
                .extension()
                .is_some_and(|s| s.eq_ignore_ascii_case("gif"))
        {
            files.push(entry.path());
        }
    }
    Ok(())
}
pub fn private_dir(path: &Path) -> Result<()> {
    if !path.exists() {
        fs::create_dir_all(path)?;
    }
    let info = fs::symlink_metadata(path)?;
    ensure!(
        info.is_dir() && !info.file_type().is_symlink(),
        "Unsafe directory {}",
        path.display()
    );
    ensure!(
        info.uid() == unsafe { libc::getuid() },
        "Wrong owner: {}",
        path.display()
    );
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
