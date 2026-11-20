//! Spectral rendering tables, based on
//! "Spectral Rendering, Part 1: Spectra" by Christoph Peters
//! https://momentsingraphics.de/SpectralRendering1Spectra.html
//! and Peters et al. 2019, "Using Moments to Represent Bounded Signals for Spectral Rendering".
//!
//! Reflectance spectra are bounded MESE spectra given by 3 trigonometric moments ("Fourier sRGB").
//! At wavelength phase phi in [-pi, 0]:
//!   a(phi) = atan(L0 + 2 L1 cos(phi) + 2 L2 cos(2 phi)) / pi + 1/2
//! where the Lagrange multipliers L are computed from the moments in closed form (on the GPU).
//!
//! RGB emitters are an upsampled reflectance times D65 (the sRGB white point) times a scale,
//! so every emitter shares one wavelength sampling density
//!   p(lambda) ~ (|r(lambda)| + |g(lambda)| + |b(lambda)|) * D65(lambda)
//! which is tabulated as 1024 entries of (r * D65 / p, g * D65 / p, b * D65 / p, phase).
//!
//! GPU buffer layout (float4):
//!   [0, SAMPLING_ENTRIES)                          wavelength sampling table
//!   [SAMPLING_ENTRIES, SAMPLING_ENTRIES + RES^3)   sRGB -> Fourier sRGB moments (c0, c1, c2, 0),
//!                                                  indexed by sRGB encoded coordinates, x = red

use std::f64::consts::PI;
use std::sync::OnceLock;

use rayon::prelude::*;

/// must match spectral.slang
pub const SAMPLING_ENTRIES: usize = 1024;
/// must match spectral.slang
pub const LUT_RESOLUTION: usize = 32;
/// Reflectances are solved for `DELTA + (1 - 2 * DELTA) * rgb` to stay away from the
/// singular boundary of the moment space (constant 0 or 1 spectra). The shader undoes it.
/// must match spectral.slang
pub const DELTA: f64 = 0.002;

const LAMBDA_MIN: f64 = 360.0;
const LAMBDA_MAX: f64 = 830.0;

/// CIE 1931 2 degree color matching functions, multi-lobe fit of
/// Wyman, Sloan and Shirley 2013, "Simple Analytic Approximations to the CIE XYZ Color Matching Functions".
fn cie_xyz(lambda: f64) -> [f64; 3] {
    fn g(x: f64, mu: f64, s1: f64, s2: f64) -> f64 {
        let s = if x < mu { s1 } else { s2 };
        let t = (x - mu) / s;
        (-0.5 * t * t).exp()
    }
    let x = 1.056 * g(lambda, 599.8, 37.9, 31.0) + 0.362 * g(lambda, 442.0, 16.0, 26.7)
        - 0.065 * g(lambda, 501.1, 20.4, 26.2);
    let y = 0.821 * g(lambda, 568.8, 46.9, 40.5) + 0.286 * g(lambda, 530.9, 16.3, 31.1);
    let z = 1.217 * g(lambda, 437.0, 11.8, 36.0) + 0.681 * g(lambda, 459.0, 26.0, 13.8);
    [x, y, z]
}

/// XYZ to linear sRGB
const XYZ_TO_RGB: [[f64; 3]; 3] = [
    [3.2406255, -1.5372080, -0.4986286],
    [-0.9689307, 1.8757561, 0.0415175],
    [0.0557101, -0.2040211, 1.0569959],
];

/// sRGB color matching functions
fn cie_rgb(lambda: f64) -> [f64; 3] {
    let xyz = cie_xyz(lambda);
    let mut rgb = [0.0; 3];
    for c in 0..3 {
        rgb[c] = XYZ_TO_RGB[c][0] * xyz[0] + XYZ_TO_RGB[c][1] * xyz[1] + XYZ_TO_RGB[c][2] * xyz[2];
    }
    rgb
}

/// CIE standard illuminant D65, 360 nm to 830 nm in 10 nm steps
const D65: [f64; 48] = [
    46.6383, 52.0891, 49.9755, 54.6482, 82.7549, 91.486, 93.4318, 86.6823, 104.865, 117.008,
    117.812, 114.861, 115.923, 108.811, 109.354, 107.802, 104.79, 107.689, 104.405, 104.046, 100.0,
    96.3342, 95.788, 88.6856, 90.0062, 89.5991, 87.6987, 83.2886, 83.6992, 80.0268, 80.2146,
    82.2778, 78.2842, 69.7213, 71.6091, 74.349, 61.604, 69.8856, 75.087, 63.5927, 46.4182, 66.8054,
    63.3828, 64.304, 59.4519, 51.959, 57.4406, 60.3125,
];

fn d65(lambda: f64) -> f64 {
    let t = ((lambda - LAMBDA_MIN) / 10.0).clamp(0.0, (D65.len() - 1) as f64);
    let i = (t as usize).min(D65.len() - 2);
    let f = t - i as f64;
    D65[i] * (1.0 - f) + D65[i + 1] * f
}

/// Piecewise linear CDF over [LAMBDA_MIN, LAMBDA_MAX] of a tabulated non negative density.
struct Cdf {
    cdf: Vec<f64>,
    step: f64,
}

const CDF_BINS: usize = 4700; // 0.1 nm

impl Cdf {
    fn new(density: impl Fn(f64) -> f64) -> Self {
        let step = (LAMBDA_MAX - LAMBDA_MIN) / CDF_BINS as f64;
        let mut cdf = Vec::with_capacity(CDF_BINS + 1);
        cdf.push(0.0);
        let mut sum = 0.0;
        for i in 0..CDF_BINS {
            let lambda = LAMBDA_MIN + (i as f64 + 0.5) * step;
            sum += density(lambda).max(0.0) * step;
            cdf.push(sum);
        }
        for v in &mut cdf {
            *v /= sum;
        }
        Self { cdf, step }
    }

    fn eval(&self, lambda: f64) -> f64 {
        let t = ((lambda - LAMBDA_MIN) / self.step).clamp(0.0, CDF_BINS as f64);
        let i = (t as usize).min(CDF_BINS - 1);
        let f = t - i as f64;
        self.cdf[i] * (1.0 - f) + self.cdf[i + 1] * f
    }

    fn invert(&self, u: f64) -> f64 {
        // first index with cdf[i] > u
        let i = self.cdf.partition_point(|&c| c <= u).clamp(1, CDF_BINS);
        let c0 = self.cdf[i - 1];
        let c1 = self.cdf[i];
        let f = if c1 > c0 { (u - c0) / (c1 - c0) } else { 0.5 };
        LAMBDA_MIN + ((i - 1) as f64 + f) * self.step
    }
}

/// Warp from wavelength to phase in [-pi, 0]. The article warps wavelengths so the visible
/// range gets most of the phase domain, here it is the CDF of the summed XYZ matching functions
/// mixed with a bit of uniform density so the warp stays strictly monotonic.
fn phase_warp() -> &'static Cdf {
    static WARP: OnceLock<Cdf> = OnceLock::new();
    WARP.get_or_init(|| {
        Cdf::new(|lambda| {
            let xyz = cie_xyz(lambda);
            xyz[0] + xyz[1] + xyz[2] + 0.05
        })
    })
}

fn wavelength_to_phase(lambda: f64) -> f64 {
    PI * (phase_warp().eval(lambda) - 1.0)
}

/// Stratified importance sampled quadrature of the sRGB color of a D65 lit reflectance.
/// rgb = 1 / n * sum_i weight_i * a(phase_i), weights are normalized so a constant spectrum 1 maps to (1, 1, 1).
#[derive(Clone, Copy, Debug)]
pub struct SpectralSample {
    pub weight: [f64; 3],
    pub phase: f64,
    pub lambda: f64,
}

fn build_sampling(n: usize) -> Vec<SpectralSample> {
    let pdf = Cdf::new(|lambda| {
        let rgb = cie_rgb(lambda);
        (rgb[0].abs() + rgb[1].abs() + rgb[2].abs()) * d65(lambda)
    });

    let mut samples: Vec<SpectralSample> = (0..n)
        .map(|i| {
            let u = (i as f64 + 0.5) / n as f64;
            let lambda = pdf.invert(u);
            let rgb = cie_rgb(lambda);
            let sum = rgb[0].abs() + rgb[1].abs() + rgb[2].abs();
            // rgb * D65 / p, D65 and the normalization of p cancel out after normalizing below
            let weight = if sum > 0.0 {
                [rgb[0] / sum, rgb[1] / sum, rgb[2] / sum]
            } else {
                [0.0; 3]
            };
            SpectralSample {
                weight,
                phase: wavelength_to_phase(lambda),
                lambda,
            }
        })
        .collect();

    // white balance, a constant spectrum must map exactly to (1, 1, 1)
    let mut mean = [0.0; 3];
    for s in &samples {
        for c in 0..3 {
            mean[c] += s.weight[c] / n as f64;
        }
    }
    for s in &mut samples {
        for c in 0..3 {
            s.weight[c] /= mean[c];
        }
    }
    samples
}

/// Bounded MESE reflectance from Lagrange multipliers
fn bounded_mese(lagrange: [f64; 3], phase: f64) -> f64 {
    let x = lagrange[0] + 2.0 * lagrange[1] * phase.cos() + 2.0 * lagrange[2] * (2.0 * phase).cos();
    x.atan() / PI + 0.5
}

fn spectrum_to_rgb(samples: &[SpectralSample], lagrange: [f64; 3]) -> [f64; 3] {
    let mut rgb = [0.0; 3];
    for s in samples {
        let a = bounded_mese(lagrange, s.phase);
        for c in 0..3 {
            rgb[c] += s.weight[c] * a;
        }
    }
    let inv = 1.0 / samples.len() as f64;
    rgb.map(|v| v * inv)
}

/// rgb and d rgb / d lagrange (jacobian[channel][multiplier])
fn spectrum_to_rgb_jacobian(samples: &[SpectralSample], lagrange: [f64; 3]) -> ([f64; 3], [[f64; 3]; 3]) {
    let mut rgb = [0.0; 3];
    let mut jac = [[0.0; 3]; 3];
    for s in samples {
        let c1 = s.phase.cos();
        let c2 = (2.0 * s.phase).cos();
        let x = lagrange[0] + 2.0 * lagrange[1] * c1 + 2.0 * lagrange[2] * c2;
        let a = x.atan() / PI + 0.5;
        let da = 1.0 / (PI * (1.0 + x * x));
        let basis = [1.0, 2.0 * c1, 2.0 * c2];
        for c in 0..3 {
            rgb[c] += s.weight[c] * a;
            for k in 0..3 {
                jac[c][k] += s.weight[c] * da * basis[k];
            }
        }
    }
    let inv = 1.0 / samples.len() as f64;
    for c in 0..3 {
        rgb[c] *= inv;
        for k in 0..3 {
            jac[c][k] *= inv;
        }
    }
    (rgb, jac)
}

fn solve3(a: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
        - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
    if !(det.abs() > 1e-300) || !det.is_finite() {
        return None;
    }
    let mut x = [0.0; 3];
    for i in 0..3 {
        let mut m = a;
        for r in 0..3 {
            m[r][i] = b[r];
        }
        let d = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
        x[i] = d / det;
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

fn residual_norm(samples: &[SpectralSample], lagrange: [f64; 3], target: [f64; 3]) -> f64 {
    let rgb = spectrum_to_rgb(samples, lagrange);
    (0..3).map(|c| (rgb[c] - target[c]).powi(2)).sum::<f64>()
}

/// Levenberg-Marquardt on the Lagrange multipliers, returns the best found.
fn refine(
    samples: &[SpectralSample],
    mut lagrange: [f64; 3],
    target: [f64; 3],
    iterations: usize,
    tolerance: f64,
) -> [f64; 3] {
    let mut error = residual_norm(samples, lagrange, target);
    let mut mu = 1e-6;
    for _ in 0..iterations {
        if error < tolerance {
            break;
        }
        let (rgb, jac) = spectrum_to_rgb_jacobian(samples, lagrange);
        let r = [rgb[0] - target[0], rgb[1] - target[1], rgb[2] - target[2]];

        // J^T J + mu * diag, -J^T r
        let mut jtj = [[0.0; 3]; 3];
        let mut jtr = [0.0; 3];
        for i in 0..3 {
            for j in 0..3 {
                for c in 0..3 {
                    jtj[i][j] += jac[c][i] * jac[c][j];
                }
            }
            for c in 0..3 {
                jtr[i] -= jac[c][i] * r[c];
            }
        }

        let mut improved = false;
        for _ in 0..12 {
            let mut m = jtj;
            for i in 0..3 {
                m[i][i] += mu * jtj[i][i].max(1e-12);
            }
            if let Some(step) = solve3(m, jtr) {
                let candidate = [lagrange[0] + step[0], lagrange[1] + step[1], lagrange[2] + step[2]];
                let candidate_error = residual_norm(samples, candidate, target);
                if candidate_error < error {
                    lagrange = candidate;
                    error = candidate_error;
                    mu = (mu * 0.3).max(1e-9);
                    improved = true;
                    break;
                }
            }
            mu *= 8.0;
        }
        if !improved {
            break;
        }
    }
    lagrange
}

/// Lagrange multipliers of the bounded MESE spectrum that reproduces the linear sRGB color.
/// Continuation from the gray of the same mean towards the target.
fn solve_lagrange(samples: &[SpectralSample], target: [f64; 3]) -> [f64; 3] {
    let mean = (target[0] + target[1] + target[2]) / 3.0;
    let mut lagrange = [(PI * (mean - 0.5)).tan(), 0.0, 0.0];
    const STEPS: usize = 6;
    for step in 1..=STEPS {
        let t = step as f64 / STEPS as f64;
        let intermediate = [
            mean + t * (target[0] - mean),
            mean + t * (target[1] - mean),
            mean + t * (target[2] - mean),
        ];
        // intermediate targets only need to be close enough to start the next step
        let (iterations, tolerance) = if step == STEPS { (60, 1e-16) } else { (10, 1e-8) };
        lagrange = refine(samples, lagrange, intermediate, iterations, tolerance);
    }
    lagrange
}

/// Trigonometric moments c_k = 1 / pi * integral over [-pi, 0] of a(phi) cos(k phi)
/// (moments of the even extension to [-pi, pi]).
fn lagrange_to_moments(lagrange: [f64; 3]) -> [f64; 3] {
    const N: usize = 4096;
    let mut c = [0.0; 3];
    for i in 0..N {
        let phi = -PI + (i as f64 + 0.5) * PI / N as f64;
        let a = bounded_mese(lagrange, phi);
        c[0] += a;
        c[1] += a * phi.cos();
        c[2] += a * (2.0 * phi).cos();
    }
    c.map(|v| v / N as f64)
}

pub fn srgb_to_linear(v: f64) -> f64 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct Complex {
    re: f64,
    im: f64,
}

#[cfg(test)]
impl Complex {
    fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }
    fn mul(self, o: Self) -> Self {
        Self::new(self.re * o.re - self.im * o.im, self.re * o.im + self.im * o.re)
    }
    fn add(self, o: Self) -> Self {
        Self::new(self.re + o.re, self.im + o.im)
    }
    fn scale(self, s: f64) -> Self {
        Self::new(self.re * s, self.im * s)
    }
    fn conj(self) -> Self {
        Self::new(self.re, -self.im)
    }
    fn abs2(self) -> f64 {
        self.re * self.re + self.im * self.im
    }
}

/// CPU reference of `MomentsToLagrange` in spectral.slang, closed form bounded MESE for 3 real moments.
#[cfg(test)]
fn moments_to_lagrange(c: [f64; 3]) -> [f64; 3] {
    let i_pi = Complex::new(0.0, PI);

    // exponential moments, power series of exp(i pi (C(z) - 1/2)), C(z) = c0 + 2 c1 z + 2 c2 z^2
    let phase = PI * (c[0] - 0.5);
    let f0 = Complex::new(phase.cos(), phase.sin());
    let f1 = f0.mul(i_pi).scale(2.0 * c[1]);
    let f2 = f1.scale(c[1]).add(f0.scale(2.0 * c[2])).mul(i_pi);

    // Toeplitz moments of the density
    let g0 = f0.re;
    let g1 = f1.scale(0.5);
    let g2 = f2.scale(0.5);

    // Levinson recursion, T a = e0 * E with T_jk = gamma_(j-k), a0 = 1
    let mut e = g0;
    let k1 = g1.scale(-1.0 / e);
    let a1 = k1;
    e *= 1.0 - k1.abs2();
    let delta = g2.add(g1.mul(a1));
    let k2 = delta.scale(-1.0 / e);
    let b1 = a1.add(k2.mul(a1.conj()));
    let b2 = k2;
    let q = [Complex::new(1.0, 0.0), b1, b2];

    // P = F * Q truncated to degree 2
    let p0 = f0;
    let p1 = f1.add(f0.mul(q[1]));
    let p2 = f2.add(f1.mul(q[1])).add(f0.mul(q[2]));
    let p = [p0, p1, p2];

    // s_l = sum_{j - k = l} p_j conj(q_k), lambda_l = (s_l - conj(s_-l)) / (2 i Re s_0)
    let s = |l: i32| -> Complex {
        let mut sum = Complex::new(0.0, 0.0);
        for j in 0..3i32 {
            let k = j - l;
            if (0..3).contains(&k) {
                sum = sum.add(p[j as usize].mul(q[k as usize].conj()));
            }
        }
        sum
    };
    let s0 = s(0);
    let mut lagrange = [0.0; 3];
    for l in 0..3 {
        let d = s(l as i32).add(s(-(l as i32)).conj().scale(-1.0));
        // d / (2 i) = (d.im - i d.re) / 2, real part d.im / 2
        lagrange[l] = 0.5 * d.im / s0.re;
    }
    lagrange
}

pub struct SpectralTables {
    pub sampling: Vec<SpectralSample>,
    /// moments (c0, c1, c2) for each grid point, x = red fastest
    pub moments: Vec<[f64; 3]>,
}

fn build_tables() -> SpectralTables {
    let sampling = build_sampling(SAMPLING_ENTRIES);
    let solver_samples = build_sampling(256);

    let n = LUT_RESOLUTION;
    let moments: Vec<[f64; 3]> = (0..n * n * n)
        .into_par_iter()
        .map(|index| {
            let x = index % n;
            let y = (index / n) % n;
            let z = index / (n * n);
            let encoded = [x, y, z].map(|v| v as f64 / (n - 1) as f64);
            let target = encoded.map(|v| DELTA + (1.0 - 2.0 * DELTA) * srgb_to_linear(v));
            let lagrange = solve_lagrange(&solver_samples, target);
            // finish on the full table the shader integrates with
            let lagrange = refine(&sampling, lagrange, target, 4, 1e-16);
            lagrange_to_moments(lagrange)
        })
        .collect();

    SpectralTables { sampling, moments }
}

/// Tables are only built once per process (about a second of CPU time).
pub fn tables() -> &'static SpectralTables {
    static TABLES: OnceLock<SpectralTables> = OnceLock::new();
    TABLES.get_or_init(build_tables)
}

/// GPU buffer contents, see the module documentation for the layout.
pub fn gpu_data() -> Vec<[f32; 4]> {
    let tables = tables();
    let mut data = Vec::with_capacity(SAMPLING_ENTRIES + tables.moments.len());
    for s in &tables.sampling {
        data.push([s.weight[0] as f32, s.weight[1] as f32, s.weight[2] as f32, s.phase as f32]);
    }
    for m in &tables.moments {
        data.push([m[0] as f32, m[1] as f32, m[2] as f32, 0.0]);
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spectral_d65_white_point() {
        let mut xyz = [0.0; 3];
        let mut lambda = LAMBDA_MIN;
        while lambda <= LAMBDA_MAX {
            let cmf = cie_xyz(lambda);
            for c in 0..3 {
                xyz[c] += cmf[c] * d65(lambda);
            }
            lambda += 1.0;
        }
        let sum = xyz[0] + xyz[1] + xyz[2];
        let (x, y) = (xyz[0] / sum, xyz[1] / sum);
        assert!((x - 0.3127).abs() < 0.005 && (y - 0.3290).abs() < 0.005, "x {x} y {y}");
    }

    #[test]
    fn spectral_sampling_white() {
        let samples = build_sampling(SAMPLING_ENTRIES);
        let rgb = spectrum_to_rgb(&samples, [1e9, 0.0, 0.0]);
        for c in 0..3 {
            assert!((rgb[c] - 1.0).abs() < 1e-6, "{rgb:?}");
        }
        let mut previous = -10.0;
        for s in &samples {
            assert!(s.phase >= -PI - 1e-9 && s.phase <= 1e-9);
            assert!(s.phase >= previous);
            previous = s.phase;
        }
    }

    #[test]
    fn spectral_closed_form_matches_solver() {
        let lagranges = [
            [0.3, 0.0, 0.0],
            [0.5, 0.4, -0.2],
            [-1.0, 1.5, 0.7],
            [2.0, -3.0, 1.0],
            [-4.0, 6.0, -3.0],
        ];
        for lagrange in lagranges {
            let moments = lagrange_to_moments(lagrange);
            let closed = moments_to_lagrange(moments);
            for k in 0..3 {
                let tolerance = 1e-3 * (1.0 + lagrange[k].abs());
                assert!((closed[k] - lagrange[k]).abs() < tolerance, "{lagrange:?} -> {closed:?}");
            }
        }
    }

    #[test]
    fn spectral_round_trip() {
        let samples = build_sampling(SAMPLING_ENTRIES);
        let colors = [
            [0.5, 0.5, 0.5],
            [0.8, 0.2, 0.1],
            [0.1, 0.7, 0.2],
            [0.05, 0.1, 0.9],
            [0.9, 0.9, 0.05],
            [0.99, 0.01, 0.01],
            [0.01, 0.01, 0.99],
        ];
        for rgb in colors {
            let target = rgb.map(|v| DELTA + (1.0 - 2.0 * DELTA) * v);
            let lagrange = solve_lagrange(&samples, target);
            let result = spectrum_to_rgb(&samples, lagrange);
            for c in 0..3 {
                assert!((result[c] - target[c]).abs() < 2e-3, "{rgb:?} -> {result:?}");
            }
        }
    }

    #[test]
    fn spectral_lut_interpolation() {
        let tables = tables();
        let samples = &tables.sampling;
        let n = LUT_RESOLUTION;
        let lookup = |rgb: [f64; 3]| -> [f64; 3] {
            let p = rgb.map(|v| {
                let e = if v <= 0.0031308 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
                e.clamp(0.0, 1.0) * (n - 1) as f64
            });
            let i = p.map(|v| (v as usize).min(n - 2));
            let f = [p[0] - i[0] as f64, p[1] - i[1] as f64, p[2] - i[2] as f64];
            let mut m = [0.0; 3];
            for corner in 0..8 {
                let o = [corner & 1, (corner >> 1) & 1, (corner >> 2) & 1];
                let w = (0..3).map(|a| if o[a] == 1 { f[a] } else { 1.0 - f[a] }).product::<f64>();
                let index = (i[0] + o[0]) + (i[1] + o[1]) * n + (i[2] + o[2]) * n * n;
                for k in 0..3 {
                    m[k] += w * tables.moments[index][k];
                }
            }
            m
        };

        let mut max_error: f64 = 0.0;
        let mut state = 12345u32;
        let mut rand = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as f64 / u32::MAX as f64
        };
        for _ in 0..500 {
            let rgb = [rand(), rand(), rand()];
            let lagrange = moments_to_lagrange(lookup(rgb));
            let result = spectrum_to_rgb(samples, lagrange).map(|v| ((v - DELTA) / (1.0 - 2.0 * DELTA)));
            for c in 0..3 {
                max_error = max_error.max((result[c] - rgb[c]).abs());
            }
        }
        assert!(max_error < 0.02, "max round trip error {max_error}");
    }
}
