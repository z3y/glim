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

struct UVPacker {
    charts: Vec<Chart>,
    width: u32,
    height: u32,
    area: f64,
}

struct Chart {
    uvs: Vec<Vector2>,
    positions: Vec<Vector3>,
    indices: Vec<u32>,

    original_indices: Vec<u32>,
    mesh_id: usize,

    uv_area: f64,
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

        uv_area.sqrt()
    }
}

fn determinant(c: Vector2, c2: Vector2, c3: Vector2) -> f32 {
    let num = c2.y - c3.y;
    let num2 = c.y - c3.y;
    let num3 = c.y - c2.y;
    c.x * num - c2.x * num2 + c3.x * num3
}

impl UVPacker {
    fn new(width: u32, height: u32) -> Self {
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
        };

        // todo maybe offset uvs so theyre always positive

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

        let maximum_scale = self.area / total_area;

        // scale up to texel units
        let scale_guess = (maximum_scale * 0.75) as f32;

        for chart in &mut self.charts {
            chart.uvs.iter_mut().for_each(|uv| *uv *= scale_guess);
        }
    }
}

struct Bitmap {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Bitmap {
    fn rasterize(chart: &Chart) -> Self {
        let (mut min_x, mut max_x) = (u32::MAX, u32::MIN);
        let (mut min_y, mut max_y) = (u32::MAX, u32::MIN);

        for uv in &chart.uvs {
            min_x = min_x.min(uv.x.floor() as u32);
            max_x = max_x.max(uv.x.ceil() as u32);

            min_y = min_y.min(uv.y.floor() as u32);
            max_y = max_y.max(uv.y.ceil() as u32);
        }

        let width = max_x - min_x;
        let height = max_y - min_y;

        let pixels = vec![0; (width * height) as usize];

        Self {
            width,
            height,
            pixels,
        }
    }
}
