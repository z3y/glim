// Mostly adapted from https://gist.github.com/ssylvan/18fb6875824c14aa2b8c
// The MIT License (MIT)
// Copyright © 2023 Sebastian Sylvan
// Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:
// The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.
// THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

use crate::math::*;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone)]
struct Edge {
    a: u32,
    b: u32,
}

impl PartialEq for Edge {
    fn eq(&self, other: &Self) -> bool {
        (self.a == other.a && self.b == other.b) || (self.a == other.b && self.b == other.a)
    }
}

impl Eq for Edge {}

impl Hash for Edge {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let (a, b) = if self.a < self.b {
            (self.a, self.b)
        } else {
            (self.b, self.a)
        };

        a.hash(state);
        b.hash(state);
    }
}

impl Edge {
    #[inline]
    fn new(a: u32, b: u32) -> Self {
        Self { a, b }
    }
}

#[derive(Debug, Clone)]
pub struct Seam {
    edge0_uv0: Vector2,
    edge1_uv0: Vector2,
    edge0_uv1: Vector2,
    edge1_uv1: Vector2,
    group: u32,
}

#[derive(Debug, Clone)]
struct SamplePoint {
    uv_a: Vector2,
    uv_b: Vector2,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct Vector2Int {
    x: i32,
    y: i32,
}

impl Vector2Int {
    fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Debug)]
struct PixelInfo {
    position: Vector2Int,
    color: Vector3,
}

#[inline]
fn approx_eq_vec3(a: Vector3, b: Vector3) -> bool {
    const EPS: f32 = 0.001;
    (a - b).length_squared() < EPS * EPS
}

#[inline]
fn approx_eq_vec2(a: Vector2, b: Vector2) -> bool {
    const EPS: f32 = 0.001;
    (a - b).length_squared() < EPS * EPS
}

pub fn find_seams(
    indices: &[u32],
    positions: &[Vector3],
    normals: &[Vector3],
    uvs: &[Vector2],
    flip_uv_y: bool,
    group: u32,
) -> Vec<Seam> {
    let is_seam = |e0: &Edge, e1: &Edge| -> u8 {
        let pa0 = positions[e0.a as usize];
        let na0 = normals[e0.a as usize];
        let uva0 = uvs[e0.a as usize];

        let pb0 = positions[e1.a as usize];
        let nb0 = normals[e1.a as usize];
        let uvb0 = uvs[e1.a as usize];

        let positions_equal = approx_eq_vec3(pa0, pb0);
        let normals_equal = approx_eq_vec3(na0, nb0);
        let uvs_equal = approx_eq_vec2(uva0, uvb0);

        if positions_equal && normals_equal && !uvs_equal {
            let pa1 = positions[e0.b as usize];
            let na1 = normals[e0.b as usize];
            let uva1 = uvs[e0.b as usize];
            let pb1 = positions[e1.b as usize];
            let nb1 = normals[e1.b as usize];
            let uvb1 = uvs[e1.b as usize];

            let positions_equal = approx_eq_vec3(pa1, pb1);
            let normals_equal = approx_eq_vec3(na1, nb1);
            let uvs_equal = approx_eq_vec2(uva1, uvb1);

            if positions_equal && normals_equal && !uvs_equal {
                return 1;
            }
        }

        let positions_equal = approx_eq_vec3(pa0, positions[e1.b as usize]);
        let normals_equal = approx_eq_vec3(na0, normals[e1.b as usize]);
        let uvs_equal = approx_eq_vec2(uva0, uvs[e1.b as usize]);

        if positions_equal && normals_equal && !uvs_equal {
            let pa1 = positions[e0.b as usize];
            let na1 = normals[e0.b as usize];
            let uva1 = uvs[e0.b as usize];
            let positions_equal = approx_eq_vec3(pa1, positions[e1.a as usize]);
            let normals_equal = approx_eq_vec3(na1, normals[e1.a as usize]);
            let uvs_equal = approx_eq_vec2(uva1, uvs[e1.a as usize]);
            if positions_equal && normals_equal && !uvs_equal {
                return 2;
            }
        }

        0
    };

    let mut edges = HashSet::new();

    let mut i = 0;
    while i + 2 < indices.len() {
        let i0 = indices[i + 0];
        let i1 = indices[i + 1];
        let i2 = indices[i + 2];

        edges.insert(Edge::new(i0, i1));
        edges.insert(Edge::new(i1, i2));
        edges.insert(Edge::new(i2, i0));

        i += 3;
    }

    let edges: Vec<Edge> = edges.into_iter().collect();

    let reference_dir = Vector2::new(1.0, 1.0).normalize();

    // todo slow
    let mut seams = Vec::new();
    for i in 0..edges.len() {
        for j in (i + 1)..edges.len() {
            let mut e0 = edges[i].clone();
            let mut e1 = edges[j].clone();

            if e0.a > e0.b {
                std::mem::swap(&mut e0.a, &mut e0.b);
            }
            if e1.a > e1.b {
                std::mem::swap(&mut e1.a, &mut e1.b);
            }

            let seam = is_seam(&e0, &e1);

            if seam != 0 {
                if seam == 2 {
                    std::mem::swap(&mut e1.a, &mut e1.b);
                }

                let mut edge0_uv0 = uvs[e0.a as usize];
                let mut edge0_uv1 = uvs[e0.b as usize];
                let edge0_p0 = positions[e0.a as usize];
                let edge0_p1 = positions[e0.b as usize];

                let mut edge1_uv0 = uvs[e1.a as usize];
                let mut edge1_uv1 = uvs[e1.b as usize];
                let edge1_p0 = positions[e1.a as usize];
                let edge1_p1 = positions[e1.b as usize];

                if flip_uv_y {
                    edge0_uv0.y = 1.0 - edge0_uv0.y;
                    edge0_uv1.y = 1.0 - edge0_uv1.y;

                    edge1_uv0.y = 1.0 - edge1_uv0.y;
                    edge1_uv1.y = 1.0 - edge1_uv1.y;
                }

                let seam_dir = (edge0_uv0 - edge1_uv0).normalize();
                if seam_dir.dot(reference_dir) < 0.0 {
                    std::mem::swap(&mut edge0_uv0, &mut edge1_uv0);
                    std::mem::swap(&mut edge0_uv1, &mut edge1_uv1);
                }

                debug_assert!(approx_eq_vec3(edge0_p0, edge1_p0));
                debug_assert!(approx_eq_vec3(edge0_p1, edge1_p1));

                seams.push(Seam {
                    edge0_uv0,
                    edge0_uv1,
                    edge1_uv0,
                    edge1_uv1,
                    group,
                });
            }
        }
    }

    println!("found {} seams out of {} edges", seams.len(), edges.len());

    // println!("Seams: {:#?} ", seams);

    seams
}

// pub fn inpaint(
//     pixels: &mut [f32],
//     width: u32,
//     height: u32,
//     backface_threshold: f32,
//     iterations: usize,
// ) {
//     let w = width as usize;
//     let h = height as usize;

//     for _ in 0..iterations {
//         let prev = pixels.to_vec();

//         for y in 0..h {
//             for x in 0..w {
//                 let idx = (y * w + x) * 4;

//                 if prev[idx + 3] > backface_threshold {
//                     continue;
//                 }

//                 let mut r = 0.0_f32;
//                 let mut g = 0.0_f32;
//                 let mut b = 0.0_f32;
//                 let mut total_weight = 0.0_f32;

//                 for dy in -1..=1 {
//                     for dx in -1..=1 {
//                         if dx == 0 && dy == 0 {
//                             continue;
//                         }

//                         let nx = (x as isize + dx) as usize;
//                         let ny = (y as isize + dy) as usize;

//                         if nx >= w || ny >= h {
//                             continue;
//                         }

//                         let nidx = (ny * w + nx) * 4;

//                         let neighbor_has_data = prev[nidx + 3] > backface_threshold
//                             || (prev[nidx] > 0.0 || prev[nidx + 1] > 0.0 || prev[nidx + 2] > 0.0);

//                         if neighbor_has_data {
//                             let distance_weight = if dx != 0 && dy != 0 {
//                                 0.7071_f32
//                             } else {
//                                 1.0_f32
//                             };

//                             let reliability_weight = if prev[nidx + 3] > backface_threshold {
//                                 1.0
//                             } else {
//                                 0.5
//                             };

//                             let final_weight = distance_weight * reliability_weight;

//                             r += prev[nidx] * final_weight;
//                             g += prev[nidx + 1] * final_weight;
//                             b += prev[nidx + 2] * final_weight;
//                             total_weight += final_weight;
//                         }
//                     }
//                 }

//                 if total_weight > 0.0 {
//                     let inv = 1.0 / total_weight;
//                     pixels[idx] = r * inv;
//                     pixels[idx + 1] = g * inv;
//                     pixels[idx + 2] = b * inv;
//                 }
//             }
//         }
//     }

//     for pixel in pixels.chunks_exact_mut(4) {
//         pixel[3] = 1.0
//     }
// }

pub fn fix_seams(
    pixels: &mut [f32],
    width: u32,
    height: u32,
    seams: &[Seam],
    debug: bool,
    group: u32,
) {
    let mut sample_points = Vec::new();

    let sample_scale = (width * height).isqrt() as f32 * 1.5;

    let edge_constraint_weight = 5.0;
    let tolerance = 0.001;
    let iterations = 100;

    for seam in seams {
        if seam.group != group {
            continue;
        }

        let length = Vector2::distance(seam.edge0_uv0, seam.edge0_uv1);
        let samples = u32::max(3, (length * sample_scale).ceil() as u32);

        for i in 0..samples {
            let t = i as f32 / (samples - 1) as f32;

            let uv_a = Vector2::lerp(seam.edge0_uv0, seam.edge0_uv1, t);
            let uv_b = Vector2::lerp(seam.edge1_uv0, seam.edge1_uv1, t);

            sample_points.push(SamplePoint { uv_a, uv_b });
        }
    }

    if sample_points.len() == 0 {
        return;
    }

    let mut pixel_info = Vec::new();
    let mut self_pixel_map: HashMap<Vector2Int, usize> = HashMap::new();
    let mut other_pixel_map: HashMap<Vector2Int, usize> = HashMap::new();

    for point in &sample_points {
        let uv_a = point.uv_a * Vector2::new(width as f32, height as f32) - Vector2::new(0.5, 0.5);
        let uv_b = point.uv_b * Vector2::new(width as f32, height as f32) - Vector2::new(0.5, 0.5);

        let width = width as i32;
        let height = height as i32;

        for i in 0..4 {
            let offset_x = i & 0b01;
            let offset_y = (i & 0b10) >> 1;

            let pos_self = Vector2Int::new(uv_a.x as i32 + offset_x, uv_a.y as i32 + offset_y);
            let pos_other = Vector2Int::new(uv_b.x as i32 + offset_x, uv_b.y as i32 + offset_y);

            if !self_pixel_map.contains_key(&pos_self)
                && pos_self.x >= 0
                && pos_self.x < width as i32
                && pos_self.y >= 0
                && pos_self.y < height as i32
            {
                let pixel_index = ((pos_self.y * width + pos_self.x) * 4) as usize;

                let r = pixels[pixel_index];
                let g = pixels[pixel_index + 1];
                let b = pixels[pixel_index + 2];

                if debug {
                    pixels[pixel_index] = 1.0;
                    pixels[pixel_index + 1] = 0.0;
                    pixels[pixel_index + 2] = 0.0;
                }

                let color = Vector3::new(r, g, b);

                pixel_info.push(PixelInfo {
                    position: pos_self.clone(),
                    color,
                });

                self_pixel_map.insert(pos_self, pixel_info.len() - 1);
            }

            if !other_pixel_map.contains_key(&pos_other)
                && pos_other.x >= 0
                && pos_other.x < width as i32
                && pos_other.y >= 0
                && pos_other.y < height as i32
            {
                let pixel_index = ((pos_other.y * width + pos_other.x) * 4) as usize;

                let r = pixels[pixel_index];
                let g = pixels[pixel_index + 1];
                let b = pixels[pixel_index + 2];

                if debug {
                    pixels[pixel_index] = 0.0;
                    pixels[pixel_index + 1] = 1.0;
                    pixels[pixel_index + 2] = 0.0;
                }

                let color = Vector3::new(r, g, b);

                pixel_info.push(PixelInfo {
                    position: pos_other.clone(),
                    color,
                });

                other_pixel_map.insert(pos_other, pixel_info.len() - 1);
            }
        }
    }

    println!(
        "created sample points {} for {} seams",
        sample_points.len(),
        seams.len()
    );

    if debug {
        return;
    }

    let total_pixels = pixel_info.len();
    let mut at_a = SparseMat::new(total_pixels, total_pixels);
    let mut at_bs = [
        VectorX::new(total_pixels),
        VectorX::new(total_pixels),
        VectorX::new(total_pixels),
    ];
    let mut guesses = [
        VectorX::new(total_pixels),
        VectorX::new(total_pixels),
        VectorX::new(total_pixels),
    ];

    setup_least_squares(
        width,
        height,
        edge_constraint_weight,
        &sample_points,
        &pixel_info,
        self_pixel_map,
        other_pixel_map,
        &mut at_a,
        &mut at_bs,
        &mut guesses,
    );

    let solution0 =
        conjugate_gradient_optimize(&mut at_a, &guesses[0], &at_bs[0], iterations, tolerance);
    let solution1 =
        conjugate_gradient_optimize(&mut at_a, &guesses[1], &at_bs[1], iterations, tolerance);
    let solution2 =
        conjugate_gradient_optimize(&mut at_a, &guesses[2], &at_bs[2], iterations, tolerance);

    let solutions = [solution0, solution1, solution2];

    let width = width as i32;

    for i in 0..total_pixels {
        let pixel = &pixel_info[i];
        let r = solutions[0][i];
        let g = solutions[1][i];
        let b = solutions[2][i];
        pixels[((pixel.position.y * width + pixel.position.x) as usize) * 4 + 0] = r;
        pixels[((pixel.position.y * width + pixel.position.x) as usize) * 4 + 1] = g;
        pixels[((pixel.position.y * width + pixel.position.x) as usize) * 4 + 2] = b;
    }
}

fn bilinear_sample(
    pixel_map: &HashMap<Vector2Int, usize>,
    sample: Vector2,
    width: u32,
    height: u32,
    weight: f32,
    ixs: &mut [usize; 4],
    weights: &mut [f32; 4],
) {
    let truncu = sample.x as i32;
    let truncv = sample.y as i32;

    let xs = [truncu, truncu + 1, truncu + 1, truncu];
    let ys = [truncv, truncv, truncv + 1, truncv + 1];

    for i in 0..4 {
        let x = (xs[i].rem_euclid(width as i32)) as i32;
        let y = (ys[i].rem_euclid(height as i32)) as i32;

        // todo something fails here
        let key = Vector2Int { x, y };
        // ixs[i] = *pixel_map.get(&key).unwrap();

        if let Some(&pixel_idx) = pixel_map.get(&key) {
            ixs[i] = pixel_idx;
        } else {
            let x = xs[i].clamp(0, width as i32 - 1);
            let y = ys[i].clamp(0, height as i32 - 1);

            // todo something fails here
            let key = Vector2Int { x, y };
            if let Some(&pixel_idx) = pixel_map.get(&key) {
                ixs[i] = pixel_idx;
            } else {
                println!("no pixel");
                ixs[i] = 0;
            }
        }
    }

    let frac_x = sample.x - truncu as f32;
    let frac_y = sample.y - truncv as f32;

    weights[0] = (1.0 - frac_x) * (1.0 - frac_y);
    weights[1] = frac_x * (1.0 - frac_y);
    weights[2] = frac_x * frac_y;
    weights[3] = (1.0 - frac_x) * frac_y;

    for i in 0..4 {
        weights[i] *= weight;
    }
}

fn setup_least_squares(
    width: u32,
    height: u32,
    edge_constraint_weight: f32,
    sample_points: &[SamplePoint],
    pixel_info: &[PixelInfo],
    self_pixel_map: HashMap<Vector2Int, usize>,
    other_pixel_map: HashMap<Vector2Int, usize>,
    at_a: &mut SparseMat,
    at_bs: &mut [VectorX],
    guesses: &mut [VectorX],
) {
    let mut self_ixs = [0; 4];
    let mut other_ixs = [0; 4];

    let mut self_weight = [0.0; 4];
    let mut other_weight = [0.0; 4];

    for point in sample_points {
        let scaled_uv_a =
            point.uv_a * Vector2::new(width as f32, height as f32) - Vector2::new(0.5, 0.5);
        let scaled_uv_b =
            point.uv_b * Vector2::new(width as f32, height as f32) - Vector2::new(0.5, 0.5);

        bilinear_sample(
            &self_pixel_map,
            scaled_uv_a,
            width,
            height,
            edge_constraint_weight,
            &mut self_ixs,
            &mut self_weight,
        );

        bilinear_sample(
            &other_pixel_map,
            scaled_uv_b,
            width,
            height,
            edge_constraint_weight,
            &mut other_ixs,
            &mut other_weight,
        );

        for i in 0..4 {
            for j in 0..4 {
                let val = at_a.get(self_ixs[i] as usize, self_ixs[j] as usize);
                at_a.set(
                    self_ixs[i] as usize,
                    self_ixs[j] as usize,
                    val + self_weight[i] * self_weight[j],
                );

                let val = at_a.get(other_ixs[i], other_ixs[j]);
                at_a.set(
                    other_ixs[i],
                    other_ixs[j],
                    val + other_weight[i] * other_weight[j],
                );

                let val = at_a.get(self_ixs[i] as usize, other_ixs[j]);
                at_a.set(
                    self_ixs[i] as usize,
                    other_ixs[j],
                    val - self_weight[i] * other_weight[j],
                );

                let val = at_a.get(other_ixs[i], self_ixs[j] as usize);
                at_a.set(
                    other_ixs[i],
                    self_ixs[j] as usize,
                    val - other_weight[i] * self_weight[j],
                );
            }
        }
    }

    for i in 0..pixel_info.len() {
        let pixel = &pixel_info[i];

        let val = at_a.get(i, i);
        at_a.set(i, i, val + 1.0);

        at_bs[0][i] = pixel.color.x;
        at_bs[1][i] = pixel.color.y;
        at_bs[2][i] = pixel.color.z;

        guesses[0][i] = pixel.color.x;
        guesses[1][i] = pixel.color.y;
        guesses[2][i] = pixel.color.z;
    }
}

fn conjugate_gradient_optimize(
    a: &SparseMat,
    guess: &VectorX,
    b: &VectorX,
    num_iterations: usize,
    tolerance: f32,
) -> VectorX {
    let n = guess.size();

    let mut p = VectorX::new(n);
    let mut r = VectorX::new(n);
    let mut ap = VectorX::new(n);
    let mut tmp = VectorX::new(n);
    let mut x = VectorX::new(n);

    x.copy_from(guess);

    SparseMat::mul(&mut tmp, a, &x);
    VectorX::sub(&mut r, b, &tmp);

    p.copy_from(&r);
    let mut rsq = VectorX::dot(&r, &r);

    for _ in 0..num_iterations {
        SparseMat::mul(&mut ap, a, &p);

        let alpha = rsq / VectorX::dot(&p, &ap);

        x.add_scaled(&p, alpha);
        r.add_scaled(&ap, -alpha);

        let rsq_new = VectorX::dot(&r, &r);

        if (rsq_new - rsq).abs() < tolerance * (n as f32) {
            break;
        }

        let beta = rsq_new / rsq;
        p.mul_add_assign(beta, &r);
        rsq = rsq_new;
    }

    x
}

#[derive(Clone, Default)]
pub struct Row {
    pub size: usize,
    pub capacity: usize,
    pub coefficients: Vec<f32>,
    pub indices: Vec<usize>,
}

impl Row {
    pub fn new() -> Self {
        Self::default()
    }

    // pub fn get(&mut self, column: usize) -> f32 {
    //     let index = self.get_column_index_and_grow_if_needed(column);
    //     self.coefficients[index]
    // }

    pub fn get(&self, column: usize) -> f32 {
        for i in 0..self.size {
            if self.indices[i] == column {
                return self.coefficients[i];
            }
        }

        0.0
    }

    pub fn set(&mut self, column: usize, value: f32) {
        let index = self.get_column_index_and_grow_if_needed(column);
        self.coefficients[index] = value;
    }

    fn grow(&mut self) {
        self.capacity = if self.capacity == 0 {
            16
        } else {
            self.capacity + self.capacity / 2
        };
        self.coefficients.resize(self.capacity, 0.0);
        self.indices.resize(self.capacity, 0);
    }

    fn find_closest_index(&self, column_index: usize) -> usize {
        for i in 0..self.size {
            if self.indices[i] >= column_index {
                return i;
            }
        }
        self.size
    }

    fn get_column_index_and_grow_if_needed(&mut self, column: usize) -> usize {
        let index = self.find_closest_index(column);

        if self.size == 0 || index >= self.indices.len() || self.indices[index] != column {
            if self.size == self.capacity {
                self.grow();
            }

            let mut prev_coeff = 0.0;
            let mut prev_index = column;
            self.size += 1;

            for i in index..self.size {
                let tmp_coeff = self.coefficients[i];
                let tmp_index = self.indices[i];
                self.coefficients[i] = prev_coeff;
                self.indices[i] = prev_index;
                prev_coeff = tmp_coeff;
                prev_index = tmp_index;
            }
        }
        index
    }
}

pub struct SparseMat {
    pub rows: Vec<Row>,
    pub num_rows: usize,
    // pub num_cols: usize,
}

impl SparseMat {
    pub fn new(num_rows: usize, _num_cols: usize) -> Self {
        let rows = vec![Row::new(); num_rows];
        SparseMat {
            rows,
            num_rows,
            // num_cols,
        }
    }

    pub fn get(&mut self, row: usize, column: usize) -> f32 {
        self.rows[row].get(column)
    }

    pub fn set(&mut self, row: usize, column: usize, value: f32) {
        self.rows[row].set(column, value);
    }

    pub fn mul(out_vector: &mut VectorX, a: &SparseMat, x: &VectorX) {
        for r in 0..a.num_rows {
            out_vector[r] = Self::dot(x, &a.rows[r]);
        }
    }

    fn dot(x: &VectorX, row: &Row) -> f32 {
        let mut sum = 0.0;
        for i in 0..row.size {
            sum += x[row.indices[i]] * row.coefficients[i];
        }
        sum
    }
}

pub struct VectorX {
    pub data: Vec<f32>,
}

impl VectorX {
    pub fn new(size: usize) -> Self {
        VectorX {
            data: vec![0.0; size],
        }
    }

    pub fn size(&self) -> usize {
        self.data.len()
    }

    pub fn copy_from(&mut self, other: &VectorX) {
        self.data.clone_from_slice(&other.data);
    }

    pub fn sub(result: &mut VectorX, a: &VectorX, b: &VectorX) {
        for i in 0..a.size() {
            result[i] = a[i] - b[i];
        }
    }

    pub fn dot(a: &VectorX, b: &VectorX) -> f32 {
        let mut sum = 0.0;
        for i in 0..a.size() {
            sum += a[i] * b[i];
        }
        sum
    }

    pub fn add_scaled(&mut self, other: &VectorX, scale: f32) {
        for i in 0..self.size() {
            self.data[i] += other.data[i] * scale;
        }
    }

    pub fn mul_add_assign(&mut self, scale: f32, other: &VectorX) {
        for i in 0..self.size() {
            self.data[i] = self.data[i] * scale + other.data[i];
        }
    }
}

impl std::ops::Index<usize> for VectorX {
    type Output = f32;
    fn index(&self, index: usize) -> &Self::Output {
        &self.data[index]
    }
}

impl std::ops::IndexMut<usize> for VectorX {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.data[index]
    }
}
