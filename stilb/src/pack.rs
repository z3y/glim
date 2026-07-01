// split mesh into charts or treat entire mesh as one chart per renderer and use scale offset for uvs
// calculate a scale multipler for each chart based on world space scale of the object https://github.com/z3y/XatlasLightmap/blob/main/Scripts/XatlasLightmapPacker.cs#L495
// multiply with scale in lightmap property and scale the uvs
// calculate area sum of all charts and use as a maximum
// sort charts by area from largest to smallest
// calculate bounds for each chart
// find an approximate float (something like 75% to 100% coverage) to scale all charts to fit inside area of lightmap texture in texel units
// rasterize each uv chart into a bitmap
// pack
// if everything fits repeat with larger approximation or stop and scale charts back into [0, 1] uv range

use rayon::iter::{IntoParallelRefMutIterator, ParallelIterator};

use crate::math::{Vector2, Vector3};
use core::slice;

pub struct Chart {
    pub uvs: Vec<Vector2>,
    base_uvs: Vec<Vector2>,

    pub positions: Vec<Vector3>,
    pub indices: Vec<u32>,
    pub mesh_id: usize,
    pub uv_area: f64,
    pub uv_bounds_area: f64,
    pub bitmap: Bitmap,

    pub chart_uv_min: Vector2,

    pub placed_offset: (u32, u32),
    pub scale: f32,
    pub world_scale: f32,
}

impl Chart {
    fn calculate_area_multiplier(&self) -> f32 {
        let mut uv_area = 0.0f64;
        let mut world_area = 0.0f64;

        for chunk in self.indices.chunks_exact(3) {
            let (ia, ib, ic) = (chunk[0] as usize, chunk[1] as usize, chunk[2] as usize);

            let (v1, v2, v3) = (self.positions[ia], self.positions[ib], self.positions[ic]);
            world_area += (v2 - v1).cross(v3 - v1).length() as f64;

            let (u1, u2, u3) = (self.uvs[ia], self.uvs[ib], self.uvs[ic]);
            uv_area += determinant(u1, u2, u3).abs() as f64;
        }

        uv_area *= 0.5;
        world_area *= 0.5;

        if uv_area == 0.0 || world_area == 0.0 {
            return 1.0;
        }

        (world_area.sqrt() / uv_area.sqrt()) as f32
    }

    fn calculate_uv_area(&self) -> f64 {
        let mut area = 0.0f64;

        for chunk in self.indices.chunks_exact(3) {
            let (ia, ib, ic) = (chunk[0] as usize, chunk[1] as usize, chunk[2] as usize);
            area += determinant(self.uvs[ia], self.uvs[ib], self.uvs[ic]).abs() as f64;
        }

        area * 0.5
    }

    fn calculate_uv_bounds_area(&self) -> f64 {
        let mut min_x = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_y = f32::NEG_INFINITY;

        for uv in &self.uvs {
            if uv.x < min_x {
                min_x = uv.x;
            }
            if uv.x > max_x {
                max_x = uv.x;
            }
            if uv.y < min_y {
                min_y = uv.y;
            }
            if uv.y > max_y {
                max_y = uv.y;
            }
        }

        let width = (max_x - min_x) as f64;
        let height = (max_y - min_y) as f64;

        (width * height).abs()
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
        self.chart_uv_min = offset;
        self.uvs.iter_mut().for_each(|uv| *uv -= offset);
    }

    fn scale_uvs_from_base(&mut self, scale: f32) {
        self.scale = scale;
        for (uv, &base) in self.uvs.iter_mut().zip(self.base_uvs.iter()) {
            *uv = base * scale;
        }
    }
}

fn determinant(c: Vector2, c2: Vector2, c3: Vector2) -> f32 {
    let num = c2.y - c3.y;
    let num2 = c.y - c3.y;
    let num3 = c.y - c2.y;
    c.x * num - c2.x * num2 + c3.x * num3
}

pub struct UVPacker {
    charts: Vec<Chart>,
    width: u32,
    height: u32,
    area: f64,
    brute_force: bool,
    pub target: Option<Bitmap>,
    iterations: u32,
}

impl UVPacker {
    pub fn new(width: u32, height: u32, iterations: u32, brute_force: bool) -> Self {
        Self {
            charts: Vec::new(),
            width,
            height,
            area: width as f64 * height as f64,
            brute_force,
            target: None,
            iterations,
        }
    }

    pub fn add_mesh(
        &mut self,
        positions: &[Vector3],
        uvs: &[Vector2],
        indices: &[u32],
        scale_multiplier: f32,
        mesh_id: usize,
    ) {
        if indices.len() % 3 != 0 {
            return;
        }

        if positions.len() != uvs.len() {
            return;
        }

        // todo split into charts for non scale offset mode

        let mut chart = Chart {
            uvs: uvs.to_vec(),
            base_uvs: Vec::new(),
            positions: positions.to_vec(),
            indices: indices.to_vec(),
            mesh_id,
            uv_area: 0.0,
            uv_bounds_area: 0.0,
            bitmap: Bitmap::empty(),
            placed_offset: (0, 0),
            scale: 1.0,
            world_scale: 1.0,
            chart_uv_min: Vector2::ZERO,
        };

        chart.offset_uvs();

        let mut scale = chart.calculate_area_multiplier();
        scale *= scale_multiplier;
        chart.uvs.iter_mut().for_each(|x| *x *= scale);

        chart.base_uvs = chart.uvs.clone();
        chart.uv_area = chart.calculate_uv_area();
        chart.uv_bounds_area = chart.calculate_uv_bounds_area();
        chart.world_scale = scale;

        self.charts.push(chart);
    }

    pub fn pack(&mut self) -> bool {
        if self.charts.is_empty() {
            return true;
        }

        // self.charts.sort_by(|a, b| {
        //     b.uv_area
        //         .partial_cmp(&a.uv_area)
        //         .unwrap_or(std::cmp::Ordering::Equal)
        // });

        self.charts.sort_by(|a, b| {
            b.uv_bounds_area
                .partial_cmp(&a.uv_bounds_area)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let total_area: f64 = self.charts.iter().map(|x| x.uv_area).sum();
        if total_area == 0.0 {
            return false;
        }

        let maximum_scale = (self.area / total_area).sqrt() as f32;

        let mut low = 0.0_f32;
        let mut high = maximum_scale;
        let mut scale_guess = maximum_scale * 0.75;

        let mut best_scale = 0.0_f32;
        let mut best_placements: Option<Vec<(u32, u32)>> = None;

        for _ in 0..self.iterations {
            if let Some(placements) = self.try_pack_at_scale(scale_guess) {
                if scale_guess > best_scale {
                    best_scale = scale_guess;
                    best_placements = Some(placements);
                }
                low = scale_guess;
                scale_guess = (scale_guess + high) * 0.5;
            } else {
                high = scale_guess;
                scale_guess = (low + scale_guess) * 0.5;
            }

            if high - low < 1e-4 {
                break;
            }
        }

        let Some(placements) = best_placements else {
            return false;
        };

        let inv_w = 1.0 / self.width as f32;
        let inv_h = 1.0 / self.height as f32;

        for (chart, &(ox, oy)) in self.charts.iter_mut().zip(placements.iter()) {
            chart.placed_offset = (ox, oy);

            chart.scale_uvs_from_base(best_scale);
            let bm = Bitmap::rasterize(chart);
            chart.bitmap = bm;

            for (uv, &base) in chart.uvs.iter_mut().zip(chart.base_uvs.iter()) {
                uv.x = (base.x * best_scale + ox as f32) * inv_w;
                uv.y = (base.y * best_scale + oy as f32) * inv_h;
            }
        }

        true
    }

    pub fn get_scale_offset(&self, chart: usize) -> (Vector2, Vector2) {
        let chart = self.charts.iter().find(|c| c.mesh_id == chart);

        match chart {
            Some(chart) => {
                let scale = Vector2::new(
                    chart.scale * chart.world_scale / self.width as f32,
                    chart.scale * chart.world_scale / self.height as f32,
                );

                let atlas_offset = Vector2::new(
                    (chart.placed_offset.0 as f32) / self.width as f32,
                    (chart.placed_offset.1 as f32) / self.height as f32,
                );

                let offset = atlas_offset - chart.chart_uv_min * scale;

                (scale, offset)
            }
            None => (Vector2::ONE, Vector2::ZERO),
        }
    }

    fn try_pack_at_scale(&mut self, scale: f32) -> Option<Vec<(u32, u32)>> {
        let brute_force = self.brute_force;

        // for chart in &mut self.charts {
        //     chart.scale_uvs_from_base(scale);
        //     chart.bitmap = Bitmap::rasterize(chart);
        // }

        self.charts.par_iter_mut().for_each(|chart| {
            chart.scale_uvs_from_base(scale);
            chart.bitmap = Bitmap::rasterize(chart);
        });

        let mut target = Bitmap::new(self.width, self.height);
        let mut placements: Vec<(u32, u32)> = Vec::with_capacity(self.charts.len());

        let mut cursor = (0_u32, 0_u32);

        for chart in &self.charts {
            let (cw, ch) = (chart.bitmap.width, chart.bitmap.height);

            if cw == 0 || ch == 0 {
                placements.push((0, 0));
                continue;
            }
            if cw > self.width || ch > self.height {
                return None;
            }

            let start = if brute_force { (0, 0) } else { cursor };

            let placed = find_placement(&target, &chart.bitmap, start.0, start.1).or_else(|| {
                if !brute_force {
                    find_placement(&target, &chart.bitmap, 0, 0)
                } else {
                    None
                }
            });

            let (ox, oy) = placed?;

            target.paint(&chart.bitmap, ox, oy);
            placements.push((ox, oy));

            if !brute_force {
                cursor = (ox + cw, oy);
                if cursor.0 >= self.width {
                    cursor = (0, oy + 1);
                }
            }
        }

        self.target = Some(target);

        Some(placements)
    }

    pub fn charts(&self) -> &[Chart] {
        &self.charts
    }
}

fn find_placement(
    target: &Bitmap,
    chart: &Bitmap,
    start_x: u32,
    start_y: u32,
) -> Option<(u32, u32)> {
    if chart.width > target.width || chart.height > target.height {
        return None;
    }

    let max_x = target.width - chart.width;
    let max_y = target.height - chart.height;

    let mut y = start_y;
    let mut x = start_x;

    if y > max_y {
        return None;
    }
    if x > max_x {
        x = 0;
        y += 1;
        if y > max_y {
            return None;
        }
    }

    loop {
        if !target.overlaps(chart, x, y) {
            return Some((x, y));
        }
        x += 1;
        if x > max_x {
            x = 0;
            y += 1;
            if y > max_y {
                return None;
            }
        }
    }
}

pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    row_stride: usize,
    pixels: Vec<u64>,
}

impl Bitmap {
    fn new(width: u32, height: u32) -> Self {
        let row_stride = (width as usize + 63) / 64;
        Self {
            width,
            height,
            row_stride,
            pixels: vec![0u64; row_stride * height as usize],
        }
    }

    fn empty() -> Self {
        Self {
            width: 0,
            height: 0,
            row_stride: 0,
            pixels: Vec::new(),
        }
    }

    #[inline]
    fn set_pixel(&mut self, x: u32, y: u32) {
        debug_assert!(x < self.width && y < self.height);

        if x >= self.width || y >= self.height {
            return;
        }

        let wi = y as usize * self.row_stride + x as usize / 64;
        self.pixels[wi] |= 1u64 << (x % 64);
    }

    fn overlaps(&self, other: &Bitmap, ox: u32, oy: u32) -> bool {
        let word_off = ox as usize / 64;
        let shift = ox % 64;

        for cy in 0..other.height {
            let ty = oy + cy;
            if ty >= self.height {
                break;
            }
            let trow = ty as usize * self.row_stride;
            let crow = cy as usize * other.row_stride;

            for cw in 0..other.row_stride {
                let word = other.pixels[crow + cw];

                if word == 0 {
                    continue;
                }

                let tw = trow + word_off + cw;

                if tw < self.pixels.len() && self.pixels[tw] & (word << shift) != 0 {
                    return true;
                }

                if shift > 0 {
                    let hi = word >> (64 - shift);
                    if hi != 0 && tw + 1 < self.pixels.len() && self.pixels[tw + 1] & hi != 0 {
                        return true;
                    }
                }
            }
        }

        false
    }

    fn paint(&mut self, other: &Bitmap, ox: u32, oy: u32) {
        let word_off = ox as usize / 64;
        let shift = ox % 64;

        for cy in 0..other.height {
            let ty = oy + cy;
            if ty >= self.height {
                break;
            }
            let trow = ty as usize * self.row_stride;
            let crow = cy as usize * other.row_stride;

            for cw in 0..other.row_stride {
                let word = other.pixels[crow + cw];
                if word == 0 {
                    continue;
                }

                let tw = trow + word_off + cw;

                if tw < self.pixels.len() {
                    self.pixels[tw] |= word << shift;
                }
                if shift > 0 {
                    let hi = word >> (64 - shift);
                    if hi != 0 && tw + 1 < self.pixels.len() {
                        self.pixels[tw + 1] |= hi;
                    }
                }
            }
        }
    }

    fn rasterize(chart: &Chart) -> Self {
        let mut max_x = 0.0_f32;
        let mut max_y = 0.0_f32;
        for uv in &chart.uvs {
            max_x = max_x.max(uv.x);
            max_y = max_y.max(uv.y);
        }

        let width = ((max_x + 1.5).floor() as i32 + 1) as u32;
        let height = ((max_y + 1.5).floor() as i32 + 1) as u32;

        let uv_offset = Vector2::new(1.0, 1.0);

        let mut bm = Self::new(width, height);

        for chunk in chart.indices.chunks_exact(3) {
            let a = chart.uvs[chunk[0] as usize] + uv_offset;
            let b = chart.uvs[chunk[1] as usize] + uv_offset;
            let c = chart.uvs[chunk[2] as usize] + uv_offset;
            rasterize_triangle_bilinear(a, b, c, &mut bm);
        }

        bm
    }

    #[cfg(test)]
    fn get_pixel(&self, x: u32, y: u32) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let wi = y as usize * self.row_stride + x as usize / 64;
        (self.pixels[wi] >> (x % 64)) & 1 != 0
    }

    #[cfg(test)]
    pub fn save_bmp(&self, path: &str) {
        use image::{GrayImage, Luma};
        let img = GrayImage::from_fn(self.width, self.height, |x, y| {
            Luma([if self.get_pixel(x, y) { 255u8 } else { 0u8 }])
        });
        img.save(path).expect("failed to save bitmap");
    }
}

#[inline(always)]
fn rasterize_triangle_bilinear(a: Vector2, b: Vector2, c: Vector2, bm: &mut Bitmap) {
    let min_x = a.x.min(b.x).min(c.x);
    let max_x = a.x.max(b.x).max(c.x);
    let min_y = a.y.min(b.y).min(c.y);
    let max_y = a.y.max(b.y).max(c.y);

    let tri_min_x = ((min_x - 0.5).floor() as i32).max(0) as u32;
    let tri_max_x = ((max_x + 0.5).ceil() as i32).min(bm.width as i32).max(0) as u32;
    let tri_min_y = ((min_y - 0.5).floor() as i32).max(0) as u32;
    let tri_max_y = ((max_y + 0.5).ceil() as i32).min(bm.height as i32).max(0) as u32;

    let extent = (max_x - min_x).max(max_y - min_y).max(1.0);
    let eps = 1e-5 * extent;

    let edges = [b - a, c - b, a - c];
    let normals = [
        Vector2 {
            x: -edges[0].y,
            y: edges[0].x,
        },
        Vector2 {
            x: -edges[1].y,
            y: edges[1].x,
        },
        Vector2 {
            x: -edges[2].y,
            y: edges[2].x,
        },
    ];

    let axes = [
        Vector2 { x: 1.0, y: 0.0 },
        Vector2 { x: 0.0, y: 1.0 },
        normals[0],
        normals[1],
        normals[2],
    ];

    for py in tri_min_y..tri_max_y {
        for px in tri_min_x..tri_max_x {
            let rect_min = Vector2 {
                x: px as f32 - 0.5,
                y: py as f32 - 0.5,
            };
            let rect_max = Vector2 {
                x: px as f32 + 1.5,
                y: py as f32 + 1.5,
            };
            let rect_corners = [
                rect_min,
                Vector2 {
                    x: rect_max.x,
                    y: rect_min.y,
                },
                Vector2 {
                    x: rect_min.x,
                    y: rect_max.y,
                },
                rect_max,
            ];

            let mut separated = false;
            for axis in &axes {
                let tri_proj = [
                    a.x * axis.x + a.y * axis.y,
                    b.x * axis.x + b.y * axis.y,
                    c.x * axis.x + c.y * axis.y,
                ];
                let tri_min_proj = tri_proj[0].min(tri_proj[1]).min(tri_proj[2]);
                let tri_max_proj = tri_proj[0].max(tri_proj[1]).max(tri_proj[2]);

                let rect_proj = [
                    rect_corners[0].x * axis.x + rect_corners[0].y * axis.y,
                    rect_corners[1].x * axis.x + rect_corners[1].y * axis.y,
                    rect_corners[2].x * axis.x + rect_corners[2].y * axis.y,
                    rect_corners[3].x * axis.x + rect_corners[3].y * axis.y,
                ];
                let rect_min_proj = rect_proj[0]
                    .min(rect_proj[1])
                    .min(rect_proj[2])
                    .min(rect_proj[3]);
                let rect_max_proj = rect_proj[0]
                    .max(rect_proj[1])
                    .max(rect_proj[2])
                    .max(rect_proj[3]);

                if tri_max_proj <= rect_min_proj + eps || rect_max_proj <= tri_min_proj + eps {
                    separated = true;
                    break;
                }
            }

            if !separated {
                bm.set_pixel(px, py);
            }
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn uvpacker_create(
    width: u32,
    height: u32,
    iterations: u32,
    brute_force: bool,
) -> *mut UVPacker {
    Box::into_raw(Box::new(UVPacker::new(
        width,
        height,
        iterations,
        brute_force,
    ))) as *mut UVPacker
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn uvpacker_add_mesh(
    handle: *mut UVPacker,
    positions: *const Vector3,
    position_count: u32,
    uvs: *const Vector2,
    uv_count: u32,
    indices: *const u32,
    index_count: u32,
    scale_multiplier: f32,
    mesh_id: u32,
) {
    let positions =
        unsafe { slice::from_raw_parts(positions as *const Vector3, position_count as usize) };
    let uvs = unsafe { slice::from_raw_parts(uvs as *const Vector2, uv_count as usize) };
    let indices = unsafe { slice::from_raw_parts(indices, index_count as usize) };

    let packer = unsafe { &mut *handle };

    packer.add_mesh(positions, uvs, indices, scale_multiplier, mesh_id as usize);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn uvpacker_pack(handle: *mut UVPacker) -> bool {
    let packer = unsafe { &mut *handle };
    packer.pack()
}

#[repr(C)]
pub struct ScaleOffset {
    pub scale: Vector2,
    pub offset: Vector2,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn uvpacker_get_scale_offset(
    handle: *mut UVPacker,
    chart: u32,
) -> ScaleOffset {
    let packer = unsafe { &mut *handle };

    let (scale, offset) = packer.get_scale_offset(chart as usize);

    ScaleOffset { scale, offset }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn uvpacker_destroy(handle: *mut UVPacker) {
    if !handle.is_null() {
        let _ = unsafe { Box::from_raw(handle) };
    }
}
