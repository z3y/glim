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

pub fn load_bmp(path: &str) -> std::io::Result<(u32, u32, Vec<f32>)> {
    let data = std::fs::read(path)?;

    let width = u32::from_le_bytes(data[18..22].try_into().unwrap());
    let height = u32::from_le_bytes(data[22..26].try_into().unwrap());
    let data_offset = u32::from_le_bytes(data[10..14].try_into().unwrap()) as usize;

    let mut pixels = vec![0.0f32; (width * height * 4) as usize];

    for y in (0..height).rev() {
        for x in 0..width {
            let src = data_offset + ((height - 1 - y) * width + x) as usize * 4;
            let dst = ((y * width + x) * 4) as usize;

            pixels[dst + 0] = data[src + 2] as f32 / 255.0; // r
            pixels[dst + 1] = data[src + 1] as f32 / 255.0; // g
            pixels[dst + 2] = data[src + 0] as f32 / 255.0; // b
            pixels[dst + 3] = data[src + 3] as f32 / 255.0; // a
        }
    }

    Ok((width, height, pixels))
}
