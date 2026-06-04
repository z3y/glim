// split mesh into charts or treat entire mesh as one chart per renderer and use scale offset for uvs
// calculate a scale multipler for each chart based on world space scale of the object https://github.com/z3y/XatlasLightmap/blob/main/Scripts/XatlasLightmapPacker.cs#L495
// multiply with scale in lightmap property and scale the uvs
// calculate area sum of all charts and use as a maximum
// sort charts by area from largest to smallest
// calculate bounds for each chart
// find an approximate float (something like 75% to 100% coverage) to scale all charts to fit inside area of lightmap texture in texel units (256 x 256 = 65536.0)
// rasterize each uv chart into a bitmap
// pack
// if everything fits repeat with larger approximation or stop and scale charts back into [0, 1] uv range

use crate::math::{Vector2, Vector3};

pub struct UVPacker {
    charts: Vec<Chart>,
    width: u32,
    height: u32,
    area: f64,
}

pub struct Chart {
    uvs: Vec<Vector2>,
    positions: Vec<Vector3>,
    indices: Vec<u32>,

    original_indices: Vec<u32>,
    mesh_id: usize,

    uv_area: f64,
    bitmap: Bitmap,
}

impl Chart {
    fn calculate_area_multiplier(&self) -> f32 {
        let mut uv_area = 0.0;
        let mut world_area = 0.0;

        // todo can be faster in parallel
        for chunk in self.indices.chunks_exact(3) {
            let index_a = chunk[0] as usize;
            let index_b = chunk[1] as usize;
            let index_c = chunk[2] as usize;

            let v1 = self.positions[index_a];
            let v2 = self.positions[index_b];
            let v3 = self.positions[index_c];

            world_area += (v2 - v1).cross(v3 - v1).length() as f64;

            let u1 = self.uvs[index_a];
            let u2 = self.uvs[index_b];
            let u3 = self.uvs[index_c];

            let d = determinant(u1, u2, u3);
            uv_area += d.abs() as f64;
        }

        (world_area.sqrt() / uv_area.sqrt()) as f32
    }

    fn calculate_uv_area(&self) -> f64 {
        let mut uv_area = 0.0;

        // todo can be faster in parallel
        for chunk in self.indices.chunks_exact(3) {
            let index_a = chunk[0] as usize;
            let index_b = chunk[1] as usize;
            let index_c = chunk[2] as usize;

            let u1 = self.uvs[index_a];
            let u2 = self.uvs[index_b];
            let u3 = self.uvs[index_c];

            let d = determinant(u1, u2, u3);
            uv_area += d.abs() as f64;
        }

        uv_area
    }

    pub fn bitmap(&self) -> &Bitmap {
        &self.bitmap
    }

    fn offset_uvs(&mut self) {
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;

        for uv in &self.uvs {
            min_x = min_x.min(uv.x);
            min_y = min_y.min(uv.y);
        }

        let offset = Vector2::new(min_x, min_y);

        self.uvs.iter_mut().for_each(|uv| *uv -= offset);
    }
}

fn determinant(c: Vector2, c2: Vector2, c3: Vector2) -> f32 {
    let num = c2.y - c3.y;
    let num2 = c.y - c3.y;
    let num3 = c.y - c2.y;
    c.x * num - c2.x * num2 + c3.x * num3
}

impl UVPacker {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            charts: Vec::new(),
            width: width,
            height: height,
            area: width as f64 * height as f64,
        }
    }

    // mesh with applied transform
    pub fn add_mesh(
        &mut self,
        positions: &[Vector3],
        uvs: &[Vector2],
        indices: &[u32],
        scale_multiplier: f32,
        mesh_id: usize,
    ) {
        // todo split into charts for non scale offset mode

        let mut chart = Chart {
            uvs: uvs.to_vec(),
            indices: indices.to_vec(),
            positions: positions.to_vec(),
            mesh_id,
            original_indices: indices.to_vec(),
            uv_area: 0.0,
            bitmap: Bitmap::empty(),
        };

        chart.offset_uvs();

        let mut scale = chart.calculate_area_multiplier();
        scale *= scale_multiplier;

        chart.uvs.iter_mut().for_each(|x| *x *= scale);

        chart.uv_area = chart.calculate_uv_area();

        self.charts.push(chart);
    }

    pub fn pack(&mut self) {
        // sort from largest to smallest chart
        self.charts
            .sort_by(|a, b| b.uv_area.partial_cmp(&a.uv_area).unwrap());

        let total_area: f64 = self.charts.iter().map(|x| x.uv_area).sum();

        // scale up to texel units
        let maximum_scale = (self.area / total_area).sqrt() as f32;
        let scale_guess = maximum_scale * 0.75;

        for chart in &mut self.charts {
            chart.uvs.iter_mut().for_each(|uv| *uv *= scale_guess);

            chart.bitmap = Bitmap::rasterize(chart);
        }
    }

    pub fn charts(&self) -> &[Chart] {
        &self.charts
    }
}

pub struct Bitmap {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Bitmap {
    fn rasterize(chart: &Chart) -> Self {
        let mut max_x = 0.0f32;
        let mut max_y = 0.0f32;

        for uv in &chart.uvs {
            max_x = max_x.max(uv.x);
            max_y = max_y.max(uv.y);
        }

        let width = max_x.ceil() as u32 + 1;
        let height = max_y.ceil() as u32 + 1;

        println!("width {} height {}", width, height);

        let resolution = (width * height) as usize;

        let mut pixels = vec![0; resolution];

        for chunk in chart.indices.chunks_exact(3) {
            let a = chart.uvs[chunk[0] as usize];
            let b = chart.uvs[chunk[1] as usize];
            let c = chart.uvs[chunk[2] as usize];

            rasterize_triangle_conservative(a, b, c, width, height, &mut pixels);
        }

        Self {
            width,
            height,
            pixels,
        }
    }

    #[cfg(test)]
    pub fn save_bmp(&self, path: &str) {
        use image::GrayImage;
        use image::Luma;

        let img = GrayImage::from_fn(self.width, self.height, |x, y| {
            let val = if self.pixels[(y * self.width + x) as usize] != 0 {
                255
            } else {
                0
            };
            Luma([val])
        });

        img.save(path).expect("failed to save bitmap");
    }

    fn empty() -> Self {
        Self {
            width: 0,
            height: 0,
            pixels: Vec::new(),
        }
    }
}

#[inline(always)]
fn rasterize_triangle_conservative(
    a: Vector2,
    b: Vector2,
    c: Vector2,
    width: u32,
    height: u32,
    pixels: &mut [u8],
) {
    let tri_min_x = (a.x.min(b.x).min(c.x).floor() as i32 - 1).max(0) as u32;
    let tri_min_y = (a.y.min(b.y).min(c.y).floor() as i32 - 1).max(0) as u32;
    let tri_max_x = ((a.x.max(b.x).max(c.x).ceil() as u32) + 1).min(width);
    let tri_max_y = ((a.y.max(b.y).max(c.y).ceil() as u32) + 1).min(height);

    for py in tri_min_y..tri_max_y {
        for px in tri_min_x..tri_max_x {
            let fx = px as f32;
            let fy = py as f32;

            let corners = [
                Vector2 { x: fx, y: fy },
                Vector2 { x: fx + 1.0, y: fy },
                Vector2 { x: fx, y: fy + 1.0 },
                Vector2 {
                    x: fx + 1.0,
                    y: fy + 1.0,
                },
            ];

            let covered = corners.iter().any(|&p| {
                let e0 = edge(a, b, p);
                let e1 = edge(b, c, p);
                let e2 = edge(c, a, p);
                (e0 >= 0.0 && e1 >= 0.0 && e2 >= 0.0) || (e0 <= 0.0 && e1 <= 0.0 && e2 <= 0.0)
            });

            if covered {
                pixels[(py * width + px) as usize] = 1;
            }
        }
    }
}

#[inline(always)]
fn edge(a: Vector2, b: Vector2, p: Vector2) -> f32 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}
