use crate::cache::{CacheStats, SharedCache};
use anyhow::{anyhow, Context, Result};
use image::{
    codecs::gif::GifDecoder,
    imageops::{resize, FilterType},
    AnimationDecoder, RgbaImage,
};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MAX_SOURCE_DIM: u32 = 512;
const DEFAULT_FRAME_DURATION_MS: u64 = 100;
const MIN_FRAME_DURATION_MS: u64 = 20;
const DEFAULT_ASSET_BUDGET: usize = 64 * 1024 * 1024;

#[derive(Debug)]
pub struct FrameData {
    pub pixels: RgbaImage,
    pub duration_ms: u64,
}

#[derive(Debug)]
pub struct Animation {
    pub width: u32,
    pub height: u32,
    pub frames: Vec<FrameData>,
    decoded_bytes: usize,
}

pub fn normalize_frame_duration_ms(numerator_ms: u32, denominator: u32) -> u64 {
    if denominator == 0 || numerator_ms == 0 {
        return DEFAULT_FRAME_DURATION_MS;
    }
    let value = ((numerator_ms as f64) / (denominator as f64)).round() as u64;
    if value == 0 {
        DEFAULT_FRAME_DURATION_MS
    } else {
        value.max(MIN_FRAME_DURATION_MS)
    }
}

fn duration_ms(delay: image::Delay) -> u64 {
    let (numerator, denominator) = delay.numer_denom_ms();
    normalize_frame_duration_ms(numerator, denominator)
}

impl Animation {
    pub fn load(path: &Path) -> Result<Self> {
        Self::load_with_budget(path, DEFAULT_ASSET_BUDGET)
    }

    pub fn load_with_budget(path: &Path, max_decoded_bytes: usize) -> Result<Self> {
        let file =
            File::open(path).with_context(|| format!("cannot open GIF {}", path.display()))?;
        let decoder = GifDecoder::new(BufReader::new(file))
            .with_context(|| format!("cannot decode GIF {}", path.display()))?;
        let decoded = decoder
            .into_frames()
            .collect_frames()
            .with_context(|| format!("cannot compose GIF frames {}", path.display()))?;
        if decoded.is_empty() {
            return Err(anyhow!("GIF has no frames: {}", path.display()));
        }

        let source_width = decoded[0].buffer().width();
        let source_height = decoded[0].buffer().height();
        if source_width == 0 || source_height == 0 {
            return Err(anyhow!("GIF has invalid dimensions: {}", path.display()));
        }

        let longest = source_width.max(source_height) as f64;
        let mut factor = (MAX_SOURCE_DIM as f64 / longest).min(1.0);
        let budget = max_decoded_bytes.max(4) as f64;
        let estimated = decoded.len() as f64
            * (source_width as f64 * factor)
            * (source_height as f64 * factor)
            * 4.0;
        if estimated > budget {
            factor *= (budget / estimated).sqrt();
        }

        let target_width = ((source_width as f64 * factor).round() as u32).max(1);
        let target_height = ((source_height as f64 * factor).round() as u32).max(1);
        let frame_bytes = target_width as usize * target_height as usize * 4;
        let decoded_bytes = frame_bytes.saturating_mul(decoded.len());

        let mut frames = Vec::with_capacity(decoded.len());
        for frame in decoded {
            let delay = duration_ms(frame.delay());
            let rgba = frame.into_buffer();
            let pixels = if rgba.width() == target_width && rgba.height() == target_height {
                rgba
            } else {
                resize(&rgba, target_width, target_height, FilterType::Lanczos3)
            };
            frames.push(FrameData {
                pixels,
                duration_ms: delay,
            });
        }

        Ok(Self {
            width: target_width,
            height: target_height,
            frames,
            decoded_bytes,
        })
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn duration(&self, index: usize) -> u64 {
        self.frames
            .get(index % self.frames.len())
            .map(|frame| frame.duration_ms)
            .unwrap_or(DEFAULT_FRAME_DURATION_MS)
    }

    pub fn decoded_bytes(&self) -> usize {
        self.decoded_bytes
    }
}

pub struct AnimationStore {
    cache: SharedCache<Animation>,
    max_asset_bytes: usize,
}

impl AnimationStore {
    pub fn new(global_budget: usize, max_asset_bytes: usize) -> Self {
        Self {
            cache: SharedCache::new(global_budget),
            max_asset_bytes: max_asset_bytes.max(4),
        }
    }

    pub fn stats(&self) -> CacheStats {
        self.cache.stats()
    }

    pub fn load(&mut self, path: &Path) -> Result<Arc<Animation>> {
        let key = canonical_key(path)?;
        if let Some(animation) = self.cache.get(&key) {
            return Ok(animation);
        }
        let animation = Animation::load_with_budget(&key, self.max_asset_bytes)?;
        let bytes = animation.decoded_bytes();
        Ok(self.cache.insert(key, animation, bytes))
    }
}

fn canonical_key(path: &Path) -> Result<PathBuf> {
    path.canonicalize()
        .with_context(|| format!("cannot canonicalize GIF {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_or_missing_delays_use_stable_default() {
        assert_eq!(normalize_frame_duration_ms(0, 1000), 100);
        assert_eq!(normalize_frame_duration_ms(10, 0), 100);
    }

    #[test]
    fn very_fast_frames_are_clamped() {
        assert_eq!(normalize_frame_duration_ms(5, 1), 20);
        assert_eq!(normalize_frame_duration_ms(20, 1), 20);
        assert_eq!(normalize_frame_duration_ms(40, 1), 40);
    }
}
