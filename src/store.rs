use crate::paths::{Paths, expand, private_dir};
use anyhow::{Context, Result, ensure};
use serde_json::{Map, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

pub fn read(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(Value::Object(Map::new()));
    }
    let file = File::open(path)?;
    ensure!(
        file.metadata()?.len() < 16 * 1024 * 1024,
        "Configuration exceeds 16 MiB"
    );
    serde_json::from_reader(file)
        .with_context(|| format!("Invalid JSON in {} (file kept unchanged)", path.display()))
}
pub fn atomic(path: &Path, value: &Value) -> Result<()> {
    let dir = path.parent().context("Missing parent directory")?;
    private_dir(dir)?;
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    let result = (|| -> Result<()> {
        serde_json::to_writer_pretty(&mut file, value)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&tmp, path)?;
        File::open(dir)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}
pub struct Store {
    pub states: Value,
    pub profiles: Value,
    pub settings: Value,
    pub session: Value,
    paths: Paths,
}
impl Store {
    pub fn open(paths: &Paths) -> Result<Self> {
        private_dir(&paths.config_dir)?;
        let old = expand("~/.config/gif-widget");
        for name in ["state.json", "profiles.json"] {
            let target = paths.config_dir.join(name);
            if !target.exists() && old.join(name).is_file() {
                let original = read(&old.join(name))?;
                atomic(
                    &paths.config_dir.join(format!("{name}.legacy-backup")),
                    &original,
                )?;
                atomic(&target, &original)?;
            }
        }
        let states = read(&paths.config_dir.join("state.json"))?;
        let profiles = read(&paths.config_dir.join("profiles.json"))?;
        let settings = read(&paths.config_dir.join("settings.json"))?;
        let session = read(&paths.config_dir.join("session.json"))?;
        ensure!(
            [&states, &profiles, &settings, &session]
                .iter()
                .all(|v| v.is_object()),
            "Configuration root must be an object"
        );
        Ok(Self {
            states,
            profiles,
            settings,
            session,
            paths: paths.clone(),
        })
    }
    pub fn save(&self) -> Result<()> {
        for (name, value) in [
            ("state.json", &self.states),
            ("profiles.json", &self.profiles),
            ("settings.json", &self.settings),
            ("session.json", &self.session),
        ] {
            atomic(&self.paths.config_dir.join(name), value)?;
        }
        Ok(())
    }
}
