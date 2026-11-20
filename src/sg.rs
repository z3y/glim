// Spherical Gaussian lightmaps
//
// 8 fixed lobes on the corners of a cube in Unity world space. Must match shaders/includes/sg.slang
// and unity/ShaderLibrary/GlimSGLightmap.hlsl.
//
// The GPU accumulates the cosine weighted projections b_i = integral(w(n, x) * G_i(x) * L(x) dx)
// of the incoming radiance onto each lobe. The amplitudes a are the weighted least squares fit
//
//     minimize integral(w(n, x) * (sum_i a_i * G_i(x) - L(x))^2 dx),  a >= 0
//
// with normal equations M(n) a = b, M_ij(n) = integral(w(n, x) * G_i(x) * G_j(x) dx).
// The weight is the cosine above the surface (so b is exactly what the cosine sampled bake estimates)
// and a small constant below it. There is no light below the surface, so the small weight keeps
// lobes pointing into the surface near zero instead of leaving them unconstrained.
//
// M only depends on the normal, it is tabulated on an octahedral grid of normals and interpolated.

use glam::{Vec2, Vec3};
use rayon::prelude::*;

use crate::Mesh;
use crate::mesh::decode_normal_octahedron;
use crate::raster::Texels;

pub const SG_LOBE_COUNT: usize = 8;
pub const SG_SHARPNESS: f32 = 3.0;
/// The lightmap is a SG_TILES x SG_TILES grid of tiles: tile 0 is the diffuse lightmap, tile 1 + k is lobe k.
pub const SG_TILES: usize = 3;

const BELOW_SURFACE_WEIGHT: f64 = 0.05;
const RIDGE: f64 = 1e-4;
const TABLE_RESOLUTION: usize = 32;
const QUADRATURE_POINTS: usize = 4096;
const NNLS_ITERATIONS: usize = 32;

type Mat = [[f32; SG_LOBE_COUNT]; SG_LOBE_COUNT];

pub fn sg_axis(lobe: usize) -> Vec3 {
    let s = 1.0 / 3.0f32.sqrt();
    Vec3::new(
        if lobe & 1 != 0 { s } else { -s },
        if lobe & 2 != 0 { s } else { -s },
        if lobe & 4 != 0 { s } else { -s },
    )
}

#[inline]
fn sg_eval(axis: Vec3, dir: Vec3) -> f32 {
    (SG_SHARPNESS * (axis.dot(dir) - 1.0)).exp()
}

fn fibonacci_sphere(i: usize, n: usize) -> Vec3 {
    let golden = std::f64::consts::PI * (3.0 - 5.0f64.sqrt());
    let z = 1.0 - (2.0 * i as f64 + 1.0) / n as f64;
    let r = (1.0 - z * z).max(0.0).sqrt();
    let phi = golden * i as f64;
    Vec3::new((r * phi.cos()) as f32, (r * phi.sin()) as f32, z as f32)
}

fn octahedral_encode(n: Vec3) -> Vec2 {
    crate::mesh::encode_normal_octahedron(n)
}

fn invert(m: &[[f64; SG_LOBE_COUNT]; SG_LOBE_COUNT]) -> Mat {
    const N: usize = SG_LOBE_COUNT;
    let mut a = *m;
    let mut inv = [[0.0f64; N]; N];
    for i in 0..N {
        inv[i][i] = 1.0;
    }

    for col in 0..N {
        let mut pivot = col;
        for row in col + 1..N {
            if a[row][col].abs() > a[pivot][col].abs() {
                pivot = row;
            }
        }
        a.swap(col, pivot);
        inv.swap(col, pivot);

        let p = a[col][col];
        for k in 0..N {
            a[col][k] /= p;
            inv[col][k] /= p;
        }

        for row in 0..N {
            if row == col {
                continue;
            }
            let f = a[row][col];
            if f == 0.0 {
                continue;
            }
            for k in 0..N {
                a[row][k] -= f * a[col][k];
                inv[row][k] -= f * inv[col][k];
            }
        }
    }

    let mut out = [[0.0f32; N]; N];
    for i in 0..N {
        for j in 0..N {
            out[i][j] = inv[i][j] as f32;
        }
    }
    out
}

pub struct SGSolver {
    gram: Vec<Mat>,
    inverse: Vec<Mat>,
}

impl SGSolver {
    pub fn new() -> Self {
        const N: usize = SG_LOBE_COUNT;

        let axes: [Vec3; N] = std::array::from_fn(sg_axis);
        let solid_angle = 4.0 * std::f64::consts::PI / QUADRATURE_POINTS as f64;

        let points: Vec<(Vec3, [f64; N])> = (0..QUADRATURE_POINTS)
            .map(|i| {
                let dir = fibonacci_sphere(i, QUADRATURE_POINTS);
                let g = std::array::from_fn(|lobe| sg_eval(axes[lobe], dir) as f64);
                (dir, g)
            })
            .collect();

        let cells: Vec<(Mat, Mat)> = (0..TABLE_RESOLUTION * TABLE_RESOLUTION)
            .into_par_iter()
            .map(|cell| {
                let x = cell % TABLE_RESOLUTION;
                let y = cell / TABLE_RESOLUTION;
                let p = Vec2::new(
                    (x as f32 + 0.5) / TABLE_RESOLUTION as f32 * 2.0 - 1.0,
                    (y as f32 + 0.5) / TABLE_RESOLUTION as f32 * 2.0 - 1.0,
                );
                let normal = decode_normal_octahedron(p);

                let mut m = [[0.0f64; N]; N];
                for (dir, g) in &points {
                    let cos = normal.dot(*dir) as f64;
                    let w = if cos > 0.0 { cos } else { BELOW_SURFACE_WEIGHT } * solid_angle;
                    for i in 0..N {
                        let wi = w * g[i];
                        for j in i..N {
                            m[i][j] += wi * g[j];
                        }
                    }
                }

                let mut trace = 0.0;
                for i in 0..N {
                    for j in 0..i {
                        m[i][j] = m[j][i];
                    }
                    trace += m[i][i];
                }
                for i in 0..N {
                    m[i][i] += RIDGE * trace / N as f64;
                }

                let mut gram = [[0.0f32; N]; N];
                for i in 0..N {
                    for j in 0..N {
                        gram[i][j] = m[i][j] as f32;
                    }
                }
                (gram, invert(&m))
            })
            .collect();

        let (gram, inverse) = cells.into_iter().unzip();
        Self { gram, inverse }
    }

    /// bilinear lookup of the gram matrix and its inverse for a normal
    fn lookup(&self, normal: Vec3) -> (Mat, Mat) {
        const N: usize = SG_LOBE_COUNT;
        let res = TABLE_RESOLUTION;

        let p = octahedral_encode(normal);
        let fx = ((p.x * 0.5 + 0.5) * res as f32 - 0.5).clamp(0.0, (res - 1) as f32);
        let fy = ((p.y * 0.5 + 0.5) * res as f32 - 0.5).clamp(0.0, (res - 1) as f32);
        let x0 = fx.floor() as usize;
        let y0 = fy.floor() as usize;
        let x1 = (x0 + 1).min(res - 1);
        let y1 = (y0 + 1).min(res - 1);
        let tx = fx - x0 as f32;
        let ty = fy - y0 as f32;

        let corners = [
            (x0 + y0 * res, (1.0 - tx) * (1.0 - ty)),
            (x1 + y0 * res, tx * (1.0 - ty)),
            (x0 + y1 * res, (1.0 - tx) * ty),
            (x1 + y1 * res, tx * ty),
        ];

        let mut gram = [[0.0f32; N]; N];
        let mut inverse = [[0.0f32; N]; N];
        for (cell, w) in corners {
            for i in 0..N {
                for j in 0..N {
                    gram[i][j] += self.gram[cell][i][j] * w;
                    inverse[i][j] += self.inverse[cell][i][j] * w;
                }
            }
        }
        (gram, inverse)
    }

    /// normal in Unity space, b are the projections from the bake
    pub fn solve(&self, normal: Vec3, b: &[Vec3; SG_LOBE_COUNT]) -> [Vec3; SG_LOBE_COUNT] {
        const N: usize = SG_LOBE_COUNT;

        let (gram, inverse) = self.lookup(normal);

        let mut a = [Vec3::ZERO; N];
        let mut negative = false;
        for i in 0..N {
            for j in 0..N {
                a[i] += b[j] * inverse[i][j];
            }
            negative |= a[i].min_element() < 0.0;
        }

        if negative {
            // projected Gauss-Seidel for the non negative solution, warm started from the clamped one
            for v in a.iter_mut() {
                *v = v.max(Vec3::ZERO);
            }
            for _ in 0..NNLS_ITERATIONS {
                for i in 0..N {
                    let mut r = b[i];
                    for j in 0..N {
                        r -= a[j] * gram[i][j];
                    }
                    a[i] = (a[i] + r / gram[i][i]).max(Vec3::ZERO);
                }
            }
        }

        a
    }
}

/// Replaces the projections stored in lightmap_texels (lobe k at block 1 + k) with the fitted amplitudes.
/// Only front face texels are solved, back faces are never decompacted.
pub fn solve_sg_texels(lightmap_texels: &mut [f32], all: &Texels, mesh: &Mesh) {
    const N: usize = SG_LOBE_COUNT;

    let solver = SGSolver::new();
    let texel_count = all.visibility.len();
    let block = |lobe: usize, index: usize| (index + texel_count * (1 + lobe)) * 3;

    const CHUNK: usize = 1 << 16;
    let mut start = 0;
    while start < all.front_count {
        let end = (start + CHUNK).min(all.front_count);

        let src: &[f32] = lightmap_texels;
        let results: Vec<Option<[Vec3; N]>> = (start..end)
            .into_par_iter()
            .map(|index| {
                let texel = &all.visibility[index];
                let primitive_id = texel.primitive_id;
                if primitive_id == u32::MAX {
                    return None;
                }

                let b: [Vec3; N] = std::array::from_fn(|lobe| {
                    let o = block(lobe, index);
                    Vec3::new(src[o], src[o + 1], src[o + 2])
                });
                if !b.iter().all(|v| v.is_finite()) {
                    return None;
                }

                let i0 = mesh.indices[primitive_id as usize * 3 + 0] as usize;
                let i1 = mesh.indices[primitive_id as usize * 3 + 1] as usize;
                let i2 = mesh.indices[primitive_id as usize * 3 + 2] as usize;
                let u = texel.bary.x;
                let v = texel.bary.y;
                let w = 1.0 - u - v;
                let mut normal = (mesh.vertices[i0].decode_normal() * w
                    + mesh.vertices[i1].decode_normal() * u
                    + mesh.vertices[i2].decode_normal() * v)
                    .normalize_or_zero();
                if normal == Vec3::ZERO {
                    return None;
                }
                // flip Z for Unity, same as the shaders
                normal.z = -normal.z;

                Some(solver.solve(normal, &b))
            })
            .collect();

        for (offset, result) in results.into_iter().enumerate() {
            let index = start + offset;
            for lobe in 0..N {
                let o = block(lobe, index);
                let value = match result {
                    Some(a) => a[lobe],
                    None => Vec3::ZERO,
                };
                lightmap_texels[o] = value.x;
                lightmap_texels[o + 1] = value.y;
                lightmap_texels[o + 2] = value.z;
            }
        }

        start = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // a constant radiance above the surface should be reproduced in the directions above the surface
    #[test]
    fn sg_fit_constant_radiance() {
        let solver = SGSolver::new();
        let normal = Vec3::new(0.0, 1.0, 0.0);
        let radiance = Vec3::new(1.0, 0.5, 0.25);

        let n = 20000;
        let mut b = [Vec3::ZERO; SG_LOBE_COUNT];
        let solid_angle = 4.0 * std::f32::consts::PI / n as f32;
        for i in 0..n {
            let dir = fibonacci_sphere(i, n);
            let cos = normal.dot(dir);
            if cos <= 0.0 {
                continue;
            }
            for lobe in 0..SG_LOBE_COUNT {
                b[lobe] += radiance * (cos * sg_eval(sg_axis(lobe), dir) * solid_angle);
            }
        }

        let a = solver.solve(normal, &b);
        for v in &a {
            assert!(v.min_element() >= 0.0);
        }

        let eval = |dir: Vec3| -> Vec3 {
            let mut sum = Vec3::ZERO;
            for lobe in 0..SG_LOBE_COUNT {
                sum += a[lobe] * sg_eval(sg_axis(lobe), dir);
            }
            sum
        };

        let up = eval(normal);
        let tilted = eval(Vec3::new(0.5, 0.8, 0.3).normalize());
        println!("up {up:?} tilted {tilted:?}");
        assert!((up - radiance).abs().max_element() < 0.15);
        assert!((tilted - radiance).abs().max_element() < 0.2);
    }
}
