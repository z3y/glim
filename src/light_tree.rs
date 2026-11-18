//! Light tree over emissive triangles, Conty Estevez & Kulla 2018 with a pbrt-v4 style SAOH build.

use glam::Vec3;
use std::f32::consts::{FRAC_PI_2, PI};

pub const INVALID: u32 = u32::MAX;
const BIN_COUNT: usize = 12;
/// Past this depth the builder stops evaluating SAOH and splits at the median.
const MAX_SAOH_DEPTH: u32 = 48;

#[derive(Clone, Copy)]
struct Bounds {
    min: Vec3,
    max: Vec3,
}

impl Bounds {
    const EMPTY: Bounds = Bounds {
        min: Vec3::INFINITY,
        max: Vec3::NEG_INFINITY,
    };

    fn union(self, o: Bounds) -> Bounds {
        Bounds {
            min: self.min.min(o.min),
            max: self.max.max(o.max),
        }
    }

    fn extend(self, p: Vec3) -> Bounds {
        Bounds {
            min: self.min.min(p),
            max: self.max.max(p),
        }
    }

    fn diagonal(&self) -> Vec3 {
        self.max - self.min
    }

    fn surface_area(&self) -> f32 {
        let d = self.diagonal();
        2.0 * (d.x * d.y + d.y * d.z + d.z * d.x)
    }
}

#[derive(Clone, Copy, Debug)]
struct Cone {
    axis: Vec3,
    cos_o: f32,
}

impl Cone {
    fn union(a: Cone, b: Cone) -> Cone {
        let theta_a = a.cos_o.clamp(-1.0, 1.0).acos();
        let theta_b = b.cos_o.clamp(-1.0, 1.0).acos();
        let theta_d = a.axis.dot(b.axis).clamp(-1.0, 1.0).acos();

        if (theta_d + theta_b).min(PI) <= theta_a {
            return a;
        }
        if (theta_d + theta_a).min(PI) <= theta_b {
            return b;
        }

        let full_sphere = Cone {
            axis: a.axis,
            cos_o: -1.0,
        };

        let theta_o = 0.5 * (theta_a + theta_d + theta_b);
        if theta_o >= PI {
            return full_sphere;
        }

        let Some(k) = a.axis.cross(b.axis).try_normalize() else {
            return full_sphere;
        };
        let (s, c) = (theta_o - theta_a).sin_cos();
        let axis = a.axis * c + k.cross(a.axis) * s;
        Cone {
            axis: axis.normalize_or(Vec3::Z),
            cos_o: theta_o.cos(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct LightTriangle {
    pub primitive_id: u32,
    bounds: Bounds,
    centroid: Vec3,
    axis: Vec3,
    cos_theta_o: f32,
    power: f32,
}

impl LightTriangle {
    pub fn new(
        primitive_id: u32,
        p: [Vec3; 3],
        radiance_luminance: f32,
        two_sided: bool,
    ) -> Option<LightTriangle> {
        // same winding as Surface::Ng, which ShadeTriangleLight uses to decide the emitting side
        let n = (p[1] - p[0]).cross(p[2] - p[0]);
        let len = n.length();
        let area = 0.5 * len;
        let power = area * radiance_luminance;
        if !(area > 0.0) || !(power > 0.0) || !power.is_finite() {
            return None;
        }

        Some(LightTriangle {
            primitive_id,
            bounds: Bounds::EMPTY.extend(p[0]).extend(p[1]).extend(p[2]),
            centroid: (p[0] + p[1] + p[2]) / 3.0,
            axis: n / len,
            cos_theta_o: if two_sided { -1.0 } else { 1.0 },
            power,
        })
    }
}

/// Mirrors `LightTreeNode` in light_tree.slang. Leaf: `left` = primitive id, `right` = INVALID.
#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LightTreeNode {
    pub bounds_min: Vec3,
    pub energy: f32,
    pub bounds_max: Vec3,
    pub cos_theta_o: f32,
    pub axis: Vec3,
    pub cos_theta_e: f32,
    pub left: u32,
    pub right: u32,
    pub parent: u32,
    pub flags: u32,
}

const _: () = assert!(size_of::<LightTreeNode>() == 64);

impl LightTreeNode {
    fn bounds(&self) -> Bounds {
        Bounds {
            min: self.bounds_min,
            max: self.bounds_max,
        }
    }

    fn cone(&self) -> Cone {
        Cone {
            axis: self.axis,
            cos_o: self.cos_theta_o,
        }
    }

    fn is_leaf(&self) -> bool {
        self.right == INVALID
    }
}

pub struct LightTree {
    pub nodes: Vec<LightTreeNode>,
    pub primitive_to_leaf: Vec<u32>,
}

#[derive(Clone, Copy)]
struct Agg {
    bounds: Bounds,
    cone: Option<Cone>,
    cos_e: f32,
    phi: f32,
    count: u32,
}

impl Agg {
    const EMPTY: Agg = Agg {
        bounds: Bounds::EMPTY,
        cone: None,
        cos_e: 1.0,
        phi: 0.0,
        count: 0,
    };

    fn from_triangle(t: &LightTriangle) -> Agg {
        Agg {
            bounds: t.bounds,
            cone: Some(Cone {
                axis: t.axis,
                cos_o: t.cos_theta_o,
            }),
            cos_e: 0.0,
            phi: t.power,
            count: 1,
        }
    }

    fn merge(&mut self, o: &Agg) {
        if o.count == 0 {
            return;
        }
        self.cone = match (self.cone, o.cone) {
            (Some(a), Some(b)) => Some(Cone::union(a, b)),
            (None, b) => b,
            (a, None) => a,
        };
        self.bounds = self.bounds.union(o.bounds);
        self.cos_e = self.cos_e.min(o.cos_e);
        self.phi += o.phi;
        self.count += o.count;
    }
}

fn m_omega(cos_o: f32, cos_e: f32) -> f32 {
    let cos_o = cos_o.clamp(-1.0, 1.0);
    let theta_o = cos_o.acos();
    let theta_e = cos_e.clamp(-1.0, 1.0).acos();
    let theta_w = (theta_o + theta_e).min(PI);
    let sin_o = (1.0 - cos_o * cos_o).max(0.0).sqrt();
    2.0 * PI * (1.0 - cos_o)
        + FRAC_PI_2
            * (2.0 * theta_w * sin_o - (theta_o - 2.0 * theta_w).cos() - 2.0 * theta_o * sin_o
                + cos_o)
}

fn saoh_cost(a: &Agg, bounds: &Bounds, dim: usize) -> f32 {
    let Some(cone) = a.cone else {
        return f32::INFINITY;
    };
    let d = bounds.diagonal();
    let kr = d.max_element() / d[dim];
    a.phi * m_omega(cone.cos_o, a.cos_e) * kr * a.bounds.surface_area()
}

fn split(tris: &[LightTriangle], order: &mut [u32], depth: u32) -> usize {
    let len = order.len();
    debug_assert!(len >= 2);

    let mut bounds = Bounds::EMPTY;
    let mut centroid_bounds = Bounds::EMPTY;
    for &i in order.iter() {
        let t = &tris[i as usize];
        bounds = bounds.union(t.bounds);
        centroid_bounds = centroid_bounds.extend(t.centroid);
    }

    let bin_of = |c: f32, dim: usize| -> usize {
        let cmin = centroid_bounds.min[dim];
        let cmax = centroid_bounds.max[dim];
        (((c - cmin) / (cmax - cmin) * BIN_COUNT as f32) as usize).min(BIN_COUNT - 1)
    };

    if depth < MAX_SAOH_DEPTH {
        let mut best: Option<(f32, usize, usize)> = None;

        for dim in 0..3 {
            if !(centroid_bounds.max[dim] > centroid_bounds.min[dim]) {
                continue;
            }

            let mut bins = [Agg::EMPTY; BIN_COUNT];
            for &i in order.iter() {
                let t = &tris[i as usize];
                bins[bin_of(t.centroid[dim], dim)].merge(&Agg::from_triangle(t));
            }

            let mut left = [Agg::EMPTY; BIN_COUNT];
            let mut acc = Agg::EMPTY;
            for b in 0..BIN_COUNT {
                acc.merge(&bins[b]);
                left[b] = acc;
            }
            let mut right = [Agg::EMPTY; BIN_COUNT];
            let mut acc = Agg::EMPTY;
            for b in (0..BIN_COUNT).rev() {
                acc.merge(&bins[b]);
                right[b] = acc;
            }

            for s in 0..BIN_COUNT - 1 {
                if left[s].count == 0 || right[s + 1].count == 0 {
                    continue;
                }
                let c = saoh_cost(&left[s], &bounds, dim) + saoh_cost(&right[s + 1], &bounds, dim);
                if c.is_finite() && best.is_none_or(|b| c < b.0) {
                    best = Some((c, dim, s));
                }
            }
        }

        if let Some((_, dim, s)) = best {
            let mut i = 0;
            let mut j = len;
            while i < j {
                if bin_of(tris[order[i] as usize].centroid[dim], dim) <= s {
                    i += 1;
                } else {
                    j -= 1;
                    order.swap(i, j);
                }
            }
            if i > 0 && i < len {
                return i;
            }
        }
    }

    let d = centroid_bounds.diagonal();
    let axis = if d.x >= d.y && d.x >= d.z {
        0
    } else if d.y >= d.z {
        1
    } else {
        2
    };
    let mid = len / 2;
    order.select_nth_unstable_by(mid, |&a, &b| {
        tris[a as usize].centroid[axis].total_cmp(&tris[b as usize].centroid[axis])
    });
    mid
}

struct BuildNode {
    start: usize,
    end: usize,
    left: u32,
    right: u32,
    parent: u32,
}

pub fn build(triangles: &[LightTriangle], total_primitive_count: usize) -> LightTree {
    let n = triangles.len();

    let max_id = triangles
        .iter()
        .map(|t| t.primitive_id as usize + 1)
        .max()
        .unwrap_or(0);
    let mut primitive_to_leaf = vec![INVALID; total_primitive_count.max(max_id).max(1)];

    if n == 0 {
        return LightTree {
            nodes: Vec::new(),
            primitive_to_leaf,
        };
    }

    let mut order: Vec<u32> = (0..n as u32).collect();
    let mut build_nodes: Vec<BuildNode> = Vec::with_capacity(2 * n);
    build_nodes.push(BuildNode {
        start: 0,
        end: n,
        left: INVALID,
        right: INVALID,
        parent: INVALID,
    });

    let mut stack: Vec<(usize, u32)> = vec![(0, 0)];
    while let Some((ni, depth)) = stack.pop() {
        let (start, end) = (build_nodes[ni].start, build_nodes[ni].end);
        if end - start <= 1 {
            continue;
        }

        let mid = start + split(triangles, &mut order[start..end], depth);

        let l = build_nodes.len();
        for (start, end) in [(start, mid), (mid, end)] {
            build_nodes.push(BuildNode {
                start,
                end,
                left: INVALID,
                right: INVALID,
                parent: ni as u32,
            });
        }
        build_nodes[ni].left = l as u32;
        build_nodes[ni].right = (l + 1) as u32;

        stack.push((l, depth + 1));
        stack.push((l + 1, depth + 1));
    }

    let mut nodes = vec![LightTreeNode::default(); build_nodes.len()];
    for i in (0..build_nodes.len()).rev() {
        let b = &build_nodes[i];

        nodes[i] = if b.right == INVALID {
            let t = &triangles[order[b.start] as usize];
            primitive_to_leaf[t.primitive_id as usize] = i as u32;
            LightTreeNode {
                bounds_min: t.bounds.min,
                energy: t.power,
                bounds_max: t.bounds.max,
                cos_theta_o: t.cos_theta_o,
                axis: t.axis,
                cos_theta_e: 0.0,
                left: t.primitive_id,
                right: INVALID,
                parent: b.parent,
                flags: 0,
            }
        } else {
            let l = &nodes[b.left as usize];
            let r = &nodes[b.right as usize];
            let bounds = l.bounds().union(r.bounds());
            let cone = Cone::union(l.cone(), r.cone());
            LightTreeNode {
                bounds_min: bounds.min,
                energy: l.energy + r.energy,
                bounds_max: bounds.max,
                cos_theta_o: cone.cos_o,
                axis: cone.axis,
                cos_theta_e: l.cos_theta_e.min(r.cos_theta_e),
                left: b.left,
                right: b.right,
                parent: b.parent,
                flags: 0,
            }
        };
    }

    LightTree {
        nodes,
        primitive_to_leaf,
    }
}

#[inline]
fn sin_from_cos(c: f32) -> f32 {
    (1.0 - c * c).max(0.0).sqrt()
}

#[inline]
fn cos_sub_clamped(sin_a: f32, cos_a: f32, sin_b: f32, cos_b: f32) -> f32 {
    if cos_a > cos_b {
        1.0
    } else {
        cos_a * cos_b + sin_a * sin_b
    }
}

#[inline]
fn sin_sub_clamped(sin_a: f32, cos_a: f32, sin_b: f32, cos_b: f32) -> f32 {
    if cos_a > cos_b {
        0.0
    } else {
        sin_a * cos_b - cos_a * sin_b
    }
}

#[allow(dead_code)]
pub fn importance(node: &LightTreeNode, p: Vec3, n: Option<Vec3>) -> f32 {
    let c = 0.5 * (node.bounds_min + node.bounds_max);
    let radius_sq = 0.25 * node.bounds().diagonal().length_squared();

    let to_p = p - c;
    let dist_sq = to_p.length_squared();
    let d2 = dist_sq.max(radius_sq).max(1e-8);
    let wi = to_p / dist_sq.max(1e-12).sqrt();

    let cos_w = node.axis.dot(wi);
    let sin_w = sin_from_cos(cos_w);

    let cos_b = if dist_sq <= radius_sq {
        -1.0
    } else {
        (1.0 - radius_sq / dist_sq).max(0.0).sqrt()
    };
    let sin_b = sin_from_cos(cos_b);

    let cos_o = node.cos_theta_o;
    let sin_o = sin_from_cos(cos_o);

    let cos_x = cos_sub_clamped(sin_w, cos_w, sin_o, cos_o);
    let sin_x = sin_sub_clamped(sin_w, cos_w, sin_o, cos_o);
    let cos_p = cos_sub_clamped(sin_x, cos_x, sin_b, cos_b);

    if cos_p <= node.cos_theta_e {
        return 0.0;
    }

    let mut imp = node.energy * cos_p / d2;

    if let Some(n) = n {
        let cos_i = n.dot(-wi);
        imp *= cos_sub_clamped(sin_from_cos(cos_i), cos_i, sin_b, cos_b).max(0.0);
    }

    imp.max(0.0)
}

#[allow(dead_code)]
impl LightTree {
    pub fn sample(&self, p: Vec3, n: Option<Vec3>, mut u: f32) -> Option<(u32, f32)> {
        if self.nodes.is_empty() {
            return None;
        }
        let mut cur = 0usize;
        let mut pmf = 1.0f32;
        loop {
            let node = &self.nodes[cur];
            if node.is_leaf() {
                return Some((node.left, pmf));
            }
            let wl = importance(&self.nodes[node.left as usize], p, n);
            let wr = importance(&self.nodes[node.right as usize], p, n);
            let sum = wl + wr;
            if !(sum > 0.0) {
                return None;
            }
            let pl = wl / sum;
            if u < pl || wr <= 0.0 {
                u = (u / pl).min(0.999_999_94);
                pmf *= pl;
                cur = node.left as usize;
            } else {
                let pr = wr / sum;
                u = ((u - pl) / pr).clamp(0.0, 0.999_999_94);
                pmf *= pr;
                cur = node.right as usize;
            }
        }
    }

    pub fn leaf_pmf(&self, primitive_id: u32, p: Vec3, n: Option<Vec3>) -> f32 {
        let mut cur = match self.primitive_to_leaf.get(primitive_id as usize) {
            Some(&l) if l != INVALID => l as usize,
            _ => return 0.0,
        };
        let mut pmf = 1.0f32;
        loop {
            let node = &self.nodes[cur];
            if node.parent == INVALID {
                return pmf;
            }
            let parent = &self.nodes[node.parent as usize];
            let sibling = if parent.left as usize == cur {
                parent.right
            } else {
                parent.left
            };
            let wc = importance(node, p, n);
            let ws = importance(&self.nodes[sibling as usize], p, n);
            if !(wc > 0.0) {
                return 0.0;
            }
            pmf *= wc / (wc + ws);
            cur = node.parent as usize;
        }
    }
}
