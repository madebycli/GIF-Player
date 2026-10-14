use crate::{
    decode,
    paths::{Paths, private_dir},
    store,
};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::{
    fs,
    hash::{Hash, Hasher},
    path::Path,
};

pub fn generate(paths: &Paths, path: &Path) -> Result<Value> {
    let metadata = path.metadata()?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hash);
    metadata.len().hash(&mut hash);
    metadata.modified()?.hash(&mut hash);
    env!("CARGO_PKG_VERSION").hash(&mut hash);
    let cache = paths
        .cache_dir
        .join("thumbs")
        .join(format!("{:016x}", hash.finish()));
    let manifest = cache.join("preview.json");
    if manifest.is_file() {
        let value = store::read(&manifest)?;
        if value["frames"].as_array().is_some_and(|frames| {
            frames
                .iter()
                .all(|f| f["path"].as_str().is_some_and(|p| Path::new(p).is_file()))
        }) {
            prune(&cache)?;
            return Ok(value);
        }
    }
    private_dir(&cache)?;
    let animation = decode::load(path, 8 * 1024 * 1024)?;
    let scale = (160.0 / animation.width.max(animation.height) as f64).min(1.0);
    let w = (animation.width as f64 * scale).round().max(1.0) as u32;
    let h = (animation.height as f64 * scale).round().max(1.0) as u32;
    let mut frames = Vec::new();
    let mut pixmap = tiny_skia::Pixmap::new(w, h).context("Preview dimensions")?;
    // Hover previews sample long animations, preserving their total loop time.
    // Playback itself retains all frames. Bound PNG/manifest size per preview.
    let stride = animation.frames.len().div_ceil(128);
    for (index, group) in animation.frames.chunks(stride).enumerate() {
        let frame = &group[0];
        pixmap.data_mut().copy_from_slice(&decode::resize(
            &frame.pixels,
            animation.width,
            animation.height,
            w,
            h,
        ));
        let dest = cache.join(format!("{index:06}.png"));
        let temp = cache.join(format!(".{index:06}.{}.png", std::process::id()));
        pixmap.save_png(&temp)?;
        fs::rename(&temp, &dest)?;
        let duration: u64 = group.iter().map(|frame| frame.delay_ms as u64).sum();
        frames.push(json!({"path":dest,"duration_ms":duration}));
    }
    let result =
        json!({"ok":true,"frames":frames,"width":w,"height":h,"warning":animation.warning});
    store::atomic(&manifest, &result)?;
    prune(&cache)?;
    Ok(result)
}

fn prune(current: &Path) -> Result<()> {
    let parent = current.parent().context("Preview cache parent")?;
    let mut entries = Vec::new();
    let mut total = 0u64;
    for directory in fs::read_dir(parent)?.flatten() {
        if !directory.file_type()?.is_dir() {
            continue;
        }
        let size: u64 = fs::read_dir(directory.path())?
            .flatten()
            .filter_map(|file| file.metadata().ok())
            .filter(|metadata| metadata.is_file())
            .map(|metadata| metadata.len())
            .sum();
        total = total.saturating_add(size);
        entries.push((directory.metadata()?.modified()?, directory.path(), size));
    }
    entries.sort_by_key(|entry| entry.0);
    for (_, path, size) in entries {
        if total <= 64 * 1024 * 1024 {
            break;
        }
        if path != current {
            fs::remove_dir_all(path)?;
            total = total.saturating_sub(size);
        }
    }
    Ok(())
}
