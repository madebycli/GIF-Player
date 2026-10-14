fn main() -> anyhow::Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    anyhow::ensure!(args.len() == 3, "Usage: png_pixel FILE X Y");
    let image = tiny_skia::Pixmap::load_png(&args[0])?;
    let x: u32 = args[1].parse()?;
    let y: u32 = args[2].parse()?;
    anyhow::ensure!(
        x < image.width() && y < image.height(),
        "Pixel out of bounds"
    );
    let pixel = image.pixels()[(y * image.width() + x) as usize];
    println!(
        "{}",
        serde_json::json!([pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()])
    );
    Ok(())
}
