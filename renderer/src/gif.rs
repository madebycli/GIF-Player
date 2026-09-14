use anyhow::{anyhow, Context, Result};
use image::{
    codecs::gif::GifDecoder,
    imageops::{resize, FilterType},
    AnimationDecoder, RgbaImage,
};
use std::{fs::File, io::BufReader, path::Path};

const MAX_SOURCE_DIM: u32 = 512;
const MEMORY_BUDGET_BYTES: f64 = 256.0 * 1024.0 * 1024.0;
const DEFAULT_FRAME_DURATION_MS: u64 = 100;
const MIN_FRAME_DURATION_MS: u64 = 20;

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
}

fn duration_ms(delay: image::Delay) -> u64 {
    let (numerator, denominator) = delay.numer_denom_ms();
    if denominator == 0 || numerator == 0 {
        return DEFAULT_FRAME_DURATION_MS;
    }
    let value = ((numerator as f64) / (denominator as f64)).round() as u64;
    if value == 0 {
        DEFAULT_FRAME_DURATION_MS
    } else {
        value.max(MIN_FRAME_DURATION_MS)
    }
}

impl Animation {
    pub fn load(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("cannot open GIF {}", path.display()))?;
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
        let estimated = decoded.len() as f64
            * (source_width as f64 * factor)
            * (source_height as f64 * factor)
            * 4.0;
        if estimated > MEMORY_BUDGET_BYTES {
            factor *= (MEMORY_BUDGET_BYTES / estimated).sqrt();
        }

        let target_width = ((source_width as f64 * factor).round() as u32).max(1);
        let target_height = ((source_height as f64 * factor).round() as u32).max(1);
        let mut frames = Vec::with_capacity(decoded.len());
        for frame in decoded {
            let delay = duration_ms(frame.delay());
            let rgba = frame.into_buffer();
            let pixels = if rgba.width() == target_width && rgba.height() == target_height {
                rgba
            } else {
                resize(&rgba, target_width, target_height, FilterType::Lanczos3)
            };
            frames.push(FrameData { pixels, duration_ms: delay });
        }

        Ok(Self { width: target_width, height: target_height, frames })
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
}
