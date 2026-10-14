use anyhow::{Context, Result, bail, ensure};
use gif::{ColorOutput, DecodeOptions, DisposalMethod};
use std::{
    fs::File,
    io::BufReader,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub const SOURCE_DIM: u32 = 512;
pub const CACHE_LIMIT: usize = 256 * 1024 * 1024;
pub fn dimensions(path: &Path) -> Result<(u32, u32)> {
    use std::io::Read;
    let mut header = [0u8; 10];
    File::open(path)?.read_exact(&mut header)?;
    ensure!(
        &header[..6] == b"GIF87a" || &header[..6] == b"GIF89a",
        "File is not a GIF"
    );
    let w = u16::from_le_bytes(header[6..8].try_into().unwrap()) as u32;
    let h = u16::from_le_bytes(header[8..10].try_into().unwrap()) as u32;
    ensure!(
        w > 0 && h > 0 && w as u64 * h as u64 <= 16_777_216,
        "GIF canvas exceeds 16 megapixels"
    );
    let factor = (SOURCE_DIM as f64 / w.max(h) as f64).min(1.0);
    Ok((
        (w as f64 * factor).round().max(1.0) as u32,
        (h as f64 * factor).round().max(1.0) as u32,
    ))
}
#[derive(Debug)]
pub struct Frame {
    pub pixels: Arc<[u8]>,
    pub delay_ms: u32,
}
#[derive(Debug)]
pub struct Animation {
    pub width: u32,
    pub height: u32,
    pub logical_size: (u32, u32),
    pub frames: Vec<Frame>,
    pub ends_ms: Vec<u64>,
    pub bytes: usize,
    pub warning: Option<String>,
}
impl Animation {
    pub fn frame_at(&self, elapsed_ms: f64) -> (usize, f64) {
        let total = *self.ends_ms.last().unwrap_or(&100) as f64;
        let phase = elapsed_ms.rem_euclid(total);
        let i = self
            .ends_ms
            .partition_point(|end| (*end as f64) <= phase)
            .min(self.frames.len() - 1);
        (i, self.ends_ms[i] as f64 - phase)
    }
}
pub fn duration(delay: u16) -> u32 {
    if delay == 0 {
        100
    } else {
        (delay as u32 * 10).max(20)
    }
}

/// Materialize delta frames and disposal before resampling; transparent pixels never erase Keep frames.
pub fn load(path: &Path, budget: usize) -> Result<Animation> {
    load_frames(path, budget, None, None)
}
pub fn load_cancellable(path: &Path, budget: usize, cancel: &AtomicBool) -> Result<Animation> {
    load_frames(path, budget, None, Some(cancel))
}
pub fn thumbnail(path: &Path) -> Result<Animation> {
    load_frames(path, 2 * 1024 * 1024, Some(1), None)
}
fn load_frames(
    path: &Path,
    budget: usize,
    limit: Option<usize>,
    cancel: Option<&AtomicBool>,
) -> Result<Animation> {
    let file = File::open(path).with_context(|| format!("Open GIF {}", path.display()))?;
    ensure!(
        file.metadata()?.len() <= 256 * 1024 * 1024,
        "GIF file exceeds 256 MiB"
    );
    let mut options = DecodeOptions::new();
    options.set_color_output(ColorOutput::RGBA);
    let mut reader = options
        .read_info(BufReader::new(file))
        .context("Invalid GIF header")?;
    let sw = reader.width() as usize;
    let sh = reader.height() as usize;
    ensure!(
        sw > 0 && sh > 0 && sw.checked_mul(sh).is_some_and(|n| n <= 16_777_216),
        "GIF canvas exceeds 16 megapixels"
    );
    let factor = (SOURCE_DIM as f64 / sw.max(sh) as f64).min(1.0);
    let mut width = (sw as f64 * factor).round().max(1.0) as u32;
    let mut height = (sh as f64 * factor).round().max(1.0) as u32;
    let logical_size = (width, height);
    let mut canvas = vec![0u8; sw * sh * 4];
    let mut restore = Vec::new();
    let background = reader
        .bg_color()
        .and_then(|index| {
            reader
                .global_palette()
                .and_then(|palette| palette.get(index * 3..index * 3 + 3))
        })
        .map(|rgb| [rgb[0], rgb[1], rgb[2], 255])
        .unwrap_or([0; 4]);
    let mut prior = (DisposalMethod::Keep, 0, 0, 0, 0, [0u8; 4]);
    let mut frames: Vec<Frame> = Vec::new();
    let mut warning = None;
    let budget = budget.min(CACHE_LIMIT);
    loop {
        ensure!(
            !cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)),
            "GIF decode cancelled"
        );
        let frame = match reader.read_next_frame() {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) if !frames.is_empty() => {
                warning = Some(format!("Partial GIF: {e}"));
                break;
            }
            Err(e) => return Err(e.into()),
        };
        ensure!(frames.len() < 100_000, "GIF has more than 100,000 frames");
        if frames.is_empty() && frame.transparent.is_none() {
            for pixel in canvas.chunks_exact_mut(4) {
                pixel.copy_from_slice(&background);
            }
        }
        let (disposal, x, y, w, h, clear) = prior;
        if disposal == DisposalMethod::Background {
            for row in y..(y + h).min(sh) {
                for pixel in canvas[(row * sw + x.min(sw)) * 4..(row * sw + (x + w).min(sw)) * 4]
                    .chunks_exact_mut(4)
                {
                    pixel.copy_from_slice(&clear);
                }
            }
        } else if disposal == DisposalMethod::Previous && !restore.is_empty() {
            canvas.copy_from_slice(&restore);
        }
        if frame.dispose == DisposalMethod::Previous {
            restore.clone_from(&canvas);
        }
        let x = frame.left as usize;
        let y = frame.top as usize;
        let w = frame.width as usize;
        let h = frame.height as usize;
        ensure!(
            w > 0 && h > 0 && x + w <= sw && y + h <= sh,
            "Frame exceeds logical GIF canvas"
        );
        for row in 0..h {
            for col in 0..w {
                let source = (row * w + col) * 4;
                if frame.buffer[source + 3] != 0 {
                    let target = ((y + row) * sw + x + col) * 4;
                    canvas[target..target + 4].copy_from_slice(&frame.buffer[source..source + 4]);
                }
            }
        }
        prior = (
            frame.dispose,
            x,
            y,
            w,
            h,
            if frame.transparent.is_some() {
                [0; 4]
            } else {
                background
            },
        );
        // Reduce the whole cache, never discard animation frames when reaching the budget.
        while (frames.len() + 1)
            * (width as usize * height as usize * 4 + std::mem::size_of::<Frame>() + 8)
            > budget
        {
            ensure!(
                width > 1 || height > 1,
                "Frame metadata/cache exceeds memory budget"
            );
            let nw = width.div_ceil(2);
            let nh = height.div_ceil(2);
            for old in &mut frames {
                old.pixels = resize(&old.pixels, width, height, nw, nh).into();
            }
            width = nw;
            height = nh;
        }
        let mut pixels = resize(&canvas, sw as u32, sh as u32, width, height);
        for px in pixels.chunks_exact_mut(4) {
            let a = px[3] as u16;
            for c in &mut px[..3] {
                *c = ((*c as u16 * a + 127) / 255) as u8;
            }
        }
        // Deduplicate identical composited frames without changing their timing.
        let pixels = if let Some(last) = frames
            .last()
            .filter(|last| last.pixels.as_ref() == pixels.as_slice())
        {
            last.pixels.clone()
        } else {
            pixels.into()
        };
        frames.push(Frame {
            pixels,
            delay_ms: duration(frame.delay),
        });
        if limit.is_some_and(|limit| frames.len() >= limit) {
            break;
        }
    }
    if frames.is_empty() {
        bail!("GIF contains no frames");
    }
    let mut end = 0;
    let ends_ms = frames
        .iter()
        .map(|f| {
            end += f.delay_ms as u64;
            end
        })
        .collect();
    let bytes =
        frames.len() * (width as usize * height as usize * 4 + std::mem::size_of::<Frame>() + 8);
    Ok(Animation {
        width,
        height,
        logical_size,
        frames,
        ends_ms,
        bytes,
        warning,
    })
}
pub fn resize(source: &[u8], sw: u32, sh: u32, w: u32, h: u32) -> Vec<u8> {
    if sw == w && sh == h {
        return source.to_vec();
    }
    let mut result = vec![0; w as usize * h as usize * 4];
    for y in 0..h {
        for x in 0..w {
            let sx = (x as u64 * sw as u64 / w as u64) as usize;
            let sy = (y as u64 * sh as u64 / h as u64) as usize;
            let at = (y as usize * w as usize + x as usize) * 4;
            result[at..at + 4].copy_from_slice(
                &source[(sy * sw as usize + sx) * 4..(sy * sw as usize + sx) * 4 + 4],
            );
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use gif::{Encoder, Frame as GifFrame};
    #[test]
    fn cache_budget_cancellation_and_truncated_prefix() {
        let file = tempfile::NamedTempFile::new().unwrap();
        {
            let mut encoder = Encoder::new(
                File::create(file.path()).unwrap(),
                64,
                32,
                &[0, 0, 0, 255, 0, 0],
            )
            .unwrap();
            for index in 0..8 {
                encoder
                    .write_frame(&GifFrame {
                        width: 64,
                        height: 32,
                        buffer: vec![index % 2; 64 * 32].into(),
                        delay: if index == 0 { 0 } else { 5 },
                        ..Default::default()
                    })
                    .unwrap();
            }
        }
        let animation = load(file.path(), 4096).unwrap();
        assert_eq!(animation.logical_size, (64, 32));
        assert_eq!(animation.frames.len(), 8);
        assert!(animation.width < 64 && animation.bytes <= 4096);
        assert_eq!(animation.ends_ms[7], 450);
        assert_eq!(animation.frame_at(100.0).0, 1);
        assert_eq!(animation.frame_at(450.0 * 1e6 + 100.0).0, 1);
        let cancel = AtomicBool::new(true);
        assert!(load_cancellable(file.path(), CACHE_LIMIT, &cancel).is_err());
        let bytes = std::fs::read(file.path()).unwrap();
        std::fs::write(file.path(), &bytes[..bytes.len() / 2]).unwrap();
        let partial = load(file.path(), CACHE_LIMIT).unwrap();
        assert!(!partial.frames.is_empty() && partial.frames.len() < 8);
        assert!(partial.warning.is_some());
    }
    #[test]
    fn disposal_and_local_palettes() {
        for disposal in [DisposalMethod::Background, DisposalMethod::Previous] {
            let file = tempfile::NamedTempFile::new().unwrap();
            {
                let mut encoder =
                    Encoder::new(File::create(file.path()).unwrap(), 3, 1, &[]).unwrap();
                let mut f = GifFrame {
                    width: 1,
                    height: 1,
                    buffer: vec![1].into(),
                    palette: Some(vec![0, 0, 0, 255, 0, 0]),
                    delay: 1,
                    dispose: DisposalMethod::Keep,
                    transparent: Some(0),
                    ..Default::default()
                };
                encoder.write_frame(&f).unwrap();
                f.left = 1;
                f.palette = Some(vec![0, 0, 0, 0, 0, 255]);
                f.dispose = disposal;
                f.delay = 4;
                encoder.write_frame(&f).unwrap();
                f.left = 2;
                f.palette = Some(vec![0, 0, 0, 0, 255, 0]);
                f.dispose = DisposalMethod::Keep;
                f.delay = 25;
                encoder.write_frame(&f).unwrap();
            }
            let anim = load(file.path(), CACHE_LIMIT).unwrap();
            assert_eq!(anim.ends_ms, [20, 60, 310]);
            assert_eq!(&anim.frames[2].pixels[0..4], &[255, 0, 0, 255]);
            assert_eq!(&anim.frames[2].pixels[4..8], &[0, 0, 0, 0]);
            assert_eq!(&anim.frames[2].pixels[8..12], &[0, 255, 0, 255]);
            assert_eq!(anim.frame_at(60.0).0, 2);
            assert_eq!(anim.frame_at(310.0).0, 0);
            assert_eq!(anim.frame_at(60.0 + 310.0 * 1e6).0, 2);
        }
    }
}
