//! Lossless anonymous disk backing keeps native pixels instead of reducing resolution.
use anyhow::{Context, Result, ensure};
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use std::{
    collections::VecDeque,
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{FileExt, OpenOptionsExt},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
static NEXT: AtomicU64 = AtomicU64::new(0);
#[derive(Debug)]
pub struct FrameStore {
    file: File,
    records: Vec<(u64, usize)>,
    length: u64,
    frame_bytes: usize,
    slots: usize,
    cache: Mutex<VecDeque<(usize, Arc<[u8]>)>>,
}
impl FrameStore {
    pub fn new(directory: &Path, frame_bytes: usize) -> Result<Self> {
        crate::paths::private_dir(directory)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_TMPFILE | libc::O_CLOEXEC)
            .open(directory)
            .or_else(|_| {
                let path = directory.join(format!(
                    ".frames-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&path)?;
                std::fs::remove_file(path)?;
                Ok::<_, std::io::Error>(file)
            })?;
        Ok(Self {
            file,
            records: Vec::new(),
            length: 0,
            frame_bytes,
            slots: 1,
            cache: Mutex::new(VecDeque::new()),
        })
    }
    pub fn push(&mut self, pixels: &[u8]) -> Result<()> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(pixels)?;
        let compressed = encoder.finish()?;
        crate::diag!(
            Trace,
            "cache",
            "frame_compressed",
            serde_json::json!({"index":self.records.len(),"raw_bytes":pixels.len(),"compressed_bytes":compressed.len(),"backing_bytes":self.length})
        );
        ensure!(
            self.length + compressed.len() as u64 <= 1024 * 1024 * 1024,
            "Lossless GIF backing exceeds 1 GiB; use a shorter source animation"
        );
        self.file.write_all(&compressed)?;
        self.records.push((self.length, compressed.len()));
        self.length += compressed.len() as u64;
        Ok(())
    }
    pub fn set_slots(&mut self, slots: usize) {
        self.slots = slots.max(1);
    }
    pub fn pixels(&self, index: usize) -> Result<Arc<[u8]>> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| anyhow::anyhow!("Frame cache lock"))?;
        if let Some(at) = cache.iter().position(|(i, _)| *i == index) {
            crate::diag!(
                Trace,
                "cache",
                "resident_hit",
                serde_json::json!({"index":index,"resident_frames":cache.len()})
            );
            let entry = cache.remove(at).unwrap();
            let pixels = entry.1.clone();
            cache.push_back(entry);
            return Ok(pixels);
        }
        let &(offset, length) = self.records.get(index).context("Frame index")?;
        let started = std::time::Instant::now();
        let mut compressed = vec![0; length];
        self.file.read_exact_at(&mut compressed, offset)?;
        let mut pixels = Vec::with_capacity(self.frame_bytes);
        ZlibDecoder::new(&compressed[..])
            .take(self.frame_bytes as u64 + 1)
            .read_to_end(&mut pixels)?;
        ensure!(
            pixels.len() == self.frame_bytes,
            "Corrupt lossless frame backing"
        );
        let pixels: Arc<[u8]> = pixels.into();
        while cache.len() >= self.slots {
            cache.pop_front();
        }
        cache.push_back((index, pixels.clone()));
        crate::diag!(
            Trace,
            "cache",
            "frame_decompressed",
            serde_json::json!({"index":index,"compressed_bytes":length,"raw_bytes":self.frame_bytes,"duration_us":started.elapsed().as_micros(),"resident_frames":cache.len()})
        );
        Ok(pixels)
    }
}
