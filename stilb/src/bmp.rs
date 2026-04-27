use std::{fs::File, io::Write};

#[allow(dead_code)]
pub fn save_bmp(path: &str, width: u32, height: u32, pixels: &[f32]) -> std::io::Result<()> {
    let mut file = File::create(path)?;

    let file_size = 54 + (width * height * 4);

    file.write_all(b"BM")?;
    file.write_all(&(file_size).to_le_bytes())?;
    file.write_all(&[0, 0, 0, 0])?;
    file.write_all(&54u32.to_le_bytes())?;

    file.write_all(&40u32.to_le_bytes())?;
    file.write_all(&(width as i32).to_le_bytes())?;
    file.write_all(&(height as i32).to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&32u16.to_le_bytes())?;
    file.write_all(&0u32.to_le_bytes())?;
    file.write_all(&0u32.to_le_bytes())?;
    file.write_all(&0i32.to_le_bytes())?;
    file.write_all(&0i32.to_le_bytes())?;
    file.write_all(&0u32.to_le_bytes())?;
    file.write_all(&0u32.to_le_bytes())?;

    for y in (0..height).rev() {
        for x in 0..width {
            let i = ((y * width + x) * 4) as usize;

            let r = (pixels[i + 0].clamp(0.0, 1.0) * 255.0) as u8;
            let g = (pixels[i + 1].clamp(0.0, 1.0) * 255.0) as u8;
            let b = (pixels[i + 2].clamp(0.0, 1.0) * 255.0) as u8;
            let a = (pixels[i + 3].clamp(0.0, 1.0) * 255.0) as u8;

            file.write_all(&[b, g, r, a])?;
        }
    }

    Ok(())
}
