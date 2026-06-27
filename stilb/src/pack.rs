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

// ─── Chart ───────────────────────────────────────────────────────────────────

pub struct Chart {
    /// Working UV array.  After a successful `pack()` call these are the final
    /// [0, 1] lightmap UVs.  Before that they are in scaled world-space units.
    pub uvs: Vec<Vector2>,

    /// UVs after the world-space area multiplier has been applied but *before*
    /// the atlas `scale_guess` is applied.  Never mutated after `add_mesh`.
    /// Every pack attempt re-derives `uvs` from this.
    base_uvs: Vec<Vector2>,

    pub positions: Vec<Vector3>,
    pub indices: Vec<u32>,
    pub original_indices: Vec<u32>,
    pub mesh_id: usize,
    pub uv_area: f64,
    pub bitmap: Bitmap,

    /// Texel-space top-left corner where this chart was placed.
    /// Set after a successful `pack()`.
    pub placed_offset: (u32, u32),
    pub scale: f32,
    pub world_scale: f32,
}

impl Chart {
    fn calculate_area_multiplier(&self) -> f32 {
        let mut uv_area = 0.0f64;
        let mut world_area = 0.0f64;

        // todo can be faster in parallel
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

        // todo can be faster in parallel
        for chunk in self.indices.chunks_exact(3) {
            let (ia, ib, ic) = (chunk[0] as usize, chunk[1] as usize, chunk[2] as usize);
            area += determinant(self.uvs[ia], self.uvs[ib], self.uvs[ic]).abs() as f64;
        }

        area * 0.5
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

    /// Overwrite `self.uvs` with `self.base_uvs * scale`.  Used to cheaply
    /// re-scale for each pack attempt without re-running the area multiplier.
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

// ─── UVPacker ─────────────────────────────────────────────────────────────────

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
            base_uvs: Vec::new(),
            positions: positions.to_vec(),
            indices: indices.to_vec(),
            original_indices: indices.to_vec(),
            mesh_id,
            uv_area: 0.0,
            bitmap: Bitmap::empty(),
            placed_offset: (0, 0),
            scale: 1.0,
            world_scale: 1.0,
        };

        chart.offset_uvs();

        let mut scale = chart.calculate_area_multiplier();
        scale *= scale_multiplier;
        chart.uvs.iter_mut().for_each(|x| *x *= scale);

        // Snapshot the area-scaled UVs.  Every later `try_pack_at_scale` call
        // multiplies from here rather than compounding scales.
        chart.base_uvs = chart.uvs.clone();
        chart.uv_area = chart.calculate_uv_area();
        chart.world_scale = scale;

        self.charts.push(chart);
    }

    /// Pack all charts into the lightmap.
    ///
    /// **Modes**
    /// - `brute_force = true`:  For every chart the search cursor resets to
    ///   (0, 0).  Finds the optimal placement but is slower.
    /// - `brute_force = false`: The cursor advances to the right edge of the
    ///   last placed chart on the same row.  When the cursor-based scan fails
    ///   for a chart the algorithm falls back to a full scan from (0, 0) to
    ///   fill holes.  Only gives up after that second pass also fails.
    ///
    /// Up to 5 binary-search attempts are made between 0 and `maximum_scale`
    /// (the theoretical scale where charts fill the atlas with zero waste,
    /// which is never achievable in practice).  The best successful scale is
    /// committed and `chart.uvs` is written with final [0, 1] lightmap UVs.
    ///
    /// Returns `true` on success.
    pub fn pack(&mut self) -> bool {
        if self.charts.is_empty() {
            return true;
        }

        // Sort largest charts first so they anchor the layout.
        self.charts
            .sort_by(|a, b| b.uv_area.partial_cmp(&a.uv_area).unwrap());

        let total_area: f64 = self.charts.iter().map(|x| x.uv_area).sum();
        if total_area == 0.0 {
            return false;
        }

        // At `maximum_scale` chart area == atlas area → impossible to pack
        // without gaps, so the valid search space is (0, maximum_scale).
        let maximum_scale = (self.area / total_area).sqrt() as f32;

        let mut low = 0.0_f32;
        let mut high = maximum_scale;
        let mut scale_guess = maximum_scale * 0.75; // start at 75 %

        let mut best_scale = 0.0_f32;
        let mut best_placements: Option<Vec<(u32, u32)>> = None;

        for _ in 0..self.iterations {
            if let Some(placements) = self.try_pack_at_scale(scale_guess) {
                if scale_guess > best_scale {
                    best_scale = scale_guess;
                    best_placements = Some(placements);
                }
                // Success → search the upper half.
                low = scale_guess;
                scale_guess = (scale_guess + high) * 0.5;
            } else {
                // Failure → search the lower half.
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

            // Rebuild the bitmap at the winning scale (useful for debugging /
            // downstream consumers of chart.bitmap()).
            chart.scale_uvs_from_base(best_scale);
            let bm = Bitmap::rasterize(chart);
            chart.bitmap = bm;

            // Write final [0, 1] atlas UVs derived from the frozen base_uvs.
            for (uv, &base) in chart.uvs.iter_mut().zip(chart.base_uvs.iter()) {
                uv.x = (base.x * best_scale + ox as f32) * inv_w;
                uv.y = (base.y * best_scale + oy as f32) * inv_h;
            }
        }

        true
    }

    pub fn get_scale_offset(&self, chart: usize) -> (Vector2, Vector2) {
        let chart = &self.charts[chart];

        let scale = Vector2::new(
            chart.scale * chart.world_scale / self.width as f32,
            chart.scale * chart.world_scale / self.height as f32,
        );

        let offset = Vector2::new(
            chart.placed_offset.0 as f32 / self.width as f32,
            chart.placed_offset.1 as f32 / self.height as f32,
        );

        (scale, offset)
    }

    /// Attempt to place every chart at `scale` into a fresh atlas bitmap.
    /// Returns `Some(placements)` on full success or `None` if any chart could
    /// not be placed.
    fn try_pack_at_scale(&mut self, scale: f32) -> Option<Vec<(u32, u32)>> {
        let brute_force = self.brute_force;
        // Rasterise all charts at this scale (mutable pass).
        for chart in &mut self.charts {
            chart.scale_uvs_from_base(scale);
            chart.bitmap = Bitmap::rasterize(chart);
        }

        let mut target = Bitmap::new(self.width, self.height);
        let mut placements: Vec<(u32, u32)> = Vec::with_capacity(self.charts.len());

        // Cursor remembers the right edge of the last placed chart so the next
        // chart starts scanning from there rather than (0, 0) every time.
        let mut cursor = (0_u32, 0_u32);

        for chart in &self.charts {
            let (cw, ch) = (chart.bitmap.width, chart.bitmap.height);

            if cw == 0 || ch == 0 {
                // Degenerate chart (no rasterisable triangles) – assign origin.
                placements.push((0, 0));
                continue;
            }
            if cw > self.width || ch > self.height {
                return None; // can never fit at any position
            }

            // Pick starting position.
            let start = if brute_force { (0, 0) } else { cursor };

            // Primary scan from `start`.  For non-brute-force, fall back to a
            // full (0, 0) scan to fill holes when the cursor scan fails.
            let placed = find_placement(&target, &chart.bitmap, start.0, start.1).or_else(|| {
                if !brute_force {
                    find_placement(&target, &chart.bitmap, 0, 0)
                } else {
                    None
                }
            });

            // Propagate failure upward.
            let (ox, oy) = placed?;

            target.paint(&chart.bitmap, ox, oy);
            placements.push((ox, oy));

            if !brute_force {
                // Advance cursor to just past the right edge on the same row.
                cursor = (ox + cw, oy);
                if cursor.0 >= self.width {
                    // Wrap to the beginning of the next row.
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

// ─── Placement search ────────────────────────────────────────────────────────

/// Scan `target` left-to-right, top-to-bottom starting from (`start_x`,
/// `start_y`) and return the first position where `chart` fits without
/// overlapping any occupied pixel.  Returns `None` if no such position exists.
///
/// TODO: a skyline / jump-ahead optimisation can skip past occupied regions
/// without testing every pixel column, making this O(occupied regions) rather
/// than O(W × H) in the worst case.
fn find_placement(
    target: &Bitmap,
    chart: &Bitmap,
    start_x: u32,
    start_y: u32,
) -> Option<(u32, u32)> {
    if chart.width > target.width || chart.height > target.height {
        return None;
    }

    // Maximum top-left corner that keeps the chart inside the atlas.
    let max_x = target.width - chart.width;
    let max_y = target.height - chart.height;

    let mut y = start_y;
    let mut x = start_x;

    if y > max_y {
        return None;
    }
    // If x is past the valid range for this row, wrap to the next row.
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

// ─── Bitmap ──────────────────────────────────────────────────────────────────

/// Bit-packed 2-D occupancy bitmap.
///
/// Each pixel is stored as a single bit inside a `u64` word so overlap
/// detection between two bitmaps costs only O(rows × ⌈width/64⌉) bitwise AND
/// operations instead of a per-pixel byte comparison.
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    /// Number of `u64` words per row: `⌈width / 64⌉`.
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
        let wi = y as usize * self.row_stride + x as usize / 64;
        self.pixels[wi] |= 1u64 << (x % 64);
    }

    /// Return `true` if placing `other` at (ox, oy) would overlap any set bit
    /// in `self`.
    ///
    /// Implementation: for each row of `other` we shift its words by `ox % 64`
    /// bits and AND against the corresponding target words.  A non-zero result
    /// means overlap.  The shift splits each source word into a low part
    /// (written to word `w`) and a high part (written to word `w + 1`).
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

                // Low part: shift word left into the target column position.
                if tw < self.pixels.len() && self.pixels[tw] & (word << shift) != 0 {
                    return true;
                }
                // High part: bits that spill into the next target word.
                // The `shift > 0` guard avoids an undefined `>> 64` when shift == 0.
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

    /// Stamp `other` at (ox, oy) into `self` (bitwise OR).  Used after a
    /// successful placement to mark the atlas region as occupied.
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

        let width = max_x.ceil() as u32 + 1;
        let height = max_y.ceil() as u32 + 1;

        let mut bm = Self::new(width, height);

        for chunk in chart.indices.chunks_exact(3) {
            let a = chart.uvs[chunk[0] as usize];
            let b = chart.uvs[chunk[1] as usize];
            let c = chart.uvs[chunk[2] as usize];
            rasterize_triangle_conservative(a, b, c, &mut bm);
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

// ─── Triangle rasterisation ──────────────────────────────────────────────────

#[inline(always)]
fn rasterize_triangle_conservative(a: Vector2, b: Vector2, c: Vector2, bm: &mut Bitmap) {
    let tri_min_x = (a.x.min(b.x).min(c.x).floor() as i32 - 1).max(0) as u32;
    let tri_min_y = (a.y.min(b.y).min(c.y).floor() as i32 - 1).max(0) as u32;
    let tri_max_x = (a.x.max(b.x).max(c.x).ceil() as u32 + 1).min(bm.width);
    let tri_max_y = (a.y.max(b.y).max(c.y).ceil() as u32 + 1).min(bm.height);

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
                bm.set_pixel(px, py);
            }
        }
    }
}

#[inline(always)]
fn edge(a: Vector2, b: Vector2, p: Vector2) -> f32 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}
