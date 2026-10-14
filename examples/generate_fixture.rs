//! Generate synthetic GIFs for tests/benchmarks; no third-party media.
use std::{fs::File, path::Path};
fn main() -> anyhow::Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let path = Path::new(
        args.first()
            .ok_or_else(|| anyhow::anyhow!("Usage: generate_fixture FILE [SIZE] [FRAMES]"))?,
    );
    let size = args.get(1).map(|s| s.parse()).transpose()?.unwrap_or(32u16);
    let count = args
        .get(2)
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(4usize);
    let palette = [22, 24, 40, 255, 245, 155, 160, 180, 255, 180, 255, 200];
    let mut encoder = gif::Encoder::new(File::create(path)?, size, size, &palette)?;
    encoder.set_repeat(gif::Repeat::Infinite)?;
    for index in 0..count {
        let mut pixels = vec![0; size as usize * size as usize];
        for y in 0..size as usize {
            for x in 0..size as usize {
                pixels[y * size as usize + x] = if (x + index * size as usize / count.max(1))
                    % (size as usize)
                    < size as usize / 2
                {
                    (index % 3 + 1) as u8
                } else {
                    0
                };
            }
        }
        encoder.write_frame(&gif::Frame {
            width: size,
            height: size,
            buffer: pixels.into(),
            delay: 5,
            dispose: gif::DisposalMethod::Background,
            ..Default::default()
        })?;
    }
    Ok(())
}
