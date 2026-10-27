use crate::{LightmapGroup, Mesh, Vector2, mesh::decode_normal_octahedron};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VisibilityTexel {
    pub bary: Vector2,
    pub primitive_id: u32,
    pub x: u16,
    pub y: u16,
}

type P = (f64, f64);

const MAX_VERTS: usize = 8;
const MIN_COVERAGE_AREA: f64 = 1e-9;
const MIN_TRI_AREA2: f64 = 1e-12;

/// Sutherland-Hodgman against one axis-aligned half-plane.
fn clip_half_plane(
    src: &[P; MAX_VERTS],
    n: usize,
    axis: usize,
    bound: f64,
    keep_ge: bool,
    dst: &mut [P; MAX_VERTS],
) -> usize {
    #[inline(always)]
    fn coord(p: P, axis: usize) -> f64 {
        if axis == 0 { p.0 } else { p.1 }
    }

    let mut out = 0;
    if n == 0 {
        return 0;
    }

    let signed = |p: P| -> f64 {
        let c = coord(p, axis);
        if keep_ge { c - bound } else { bound - c }
    };

    let mut prev = src[n - 1];
    let mut prev_d = signed(prev);
    for i in 0..n {
        let cur = src[i];
        let cur_d = signed(cur);

        let prev_in = prev_d >= 0.0;
        let cur_in = cur_d >= 0.0;

        if prev_in != cur_in {
            // Edge crosses the plane: emit intersection.
            let t = prev_d / (prev_d - cur_d);
            let mut ip = (prev.0 + (cur.0 - prev.0) * t, prev.1 + (cur.1 - prev.1) * t);
            // Snap the clipped coordinate exactly onto the bound.
            if axis == 0 {
                ip.0 = bound
            } else {
                ip.1 = bound
            }
            if out < MAX_VERTS {
                dst[out] = ip;
                out += 1;
            }
        }
        if cur_in && out < MAX_VERTS {
            dst[out] = cur;
            out += 1;
        }

        prev = cur;
        prev_d = cur_d;
    }
    out
}

/// Returns (signed area, centroid) of a polygon, computed relative to `origin`
/// to keep precision high for large texel coordinates.
fn area_centroid(poly: &[P; MAX_VERTS], n: usize, origin: P) -> (f64, P) {
    if n < 3 {
        return (0.0, origin);
    }
    let mut a2 = 0.0;
    let mut cx = 0.0;
    let mut cy = 0.0;
    let mut prev = (poly[n - 1].0 - origin.0, poly[n - 1].1 - origin.1);
    for i in 0..n {
        let cur = (poly[i].0 - origin.0, poly[i].1 - origin.1);
        let cross = prev.0 * cur.1 - cur.0 * prev.1;
        a2 += cross;
        cx += (prev.0 + cur.0) * cross;
        cy += (prev.1 + cur.1) * cross;
        prev = cur;
    }
    if a2.abs() < f64::MIN_POSITIVE {
        return (0.0, origin);
    }
    let inv = 1.0 / (3.0 * a2); // centroid = sum / (6A) = sum / (3 * 2A)
    (a2 * 0.5, (origin.0 + cx * inv, origin.1 + cy * inv))
}

// stride 4
pub fn visibility_to_normals(
    texels: &[VisibilityTexel],
    mesh: &Mesh,
    groups: &LightmapGroup,
) -> Vec<f32> {
    let resolution = groups.settings.width as usize;
    let mut normals = vec![0.0; resolution * resolution * 4];

    for texel in texels {
        let x = texel.x as usize;
        let y = resolution - 1 - texel.y as usize;

        let index = y * resolution + x;

        let primitive_id = texel.primitive_id as usize;

        let i0 = mesh.indices[primitive_id * 3 + 0] as usize;
        let i1 = mesh.indices[primitive_id * 3 + 1] as usize;
        let i2 = mesh.indices[primitive_id * 3 + 2] as usize;

        let v0 = mesh.vertices[i0];
        let v1 = mesh.vertices[i1];
        let v2 = mesh.vertices[i2];

        let bary = texel.bary;
        let u = bary.x;
        let v = bary.y;
        let w = 1.0 - u - v;

        let n0 = decode_normal_octahedron(v0.normal);
        let n1 = decode_normal_octahedron(v1.normal);
        let n2 = decode_normal_octahedron(v2.normal);

        let normal = n0 * w + n1 * u + n2 * v;

        normals[index * 4 + 0] = normal.x;
        normals[index * 4 + 1] = normal.y;
        normals[index * 4 + 2] = normal.z;
        normals[index * 4 + 3] = 0.0;
    }

    normals
}

pub fn rasterize_visibility(mesh: &Mesh, groups: &[LightmapGroup]) -> Vec<VisibilityTexel> {
    let mut texels = Vec::new();

    let mut resolution_lookup = Vec::with_capacity(groups.len());
    for g in groups {
        resolution_lookup.push(g.settings.width); // w and h same
    }

    let mut primitive_id: u32 = 0;

    for tri in mesh.indices.chunks_exact(3) {
        let prim = primitive_id;
        primitive_id += 1;

        let v0 = &mesh.vertices[tri[0] as usize];
        let v1 = &mesh.vertices[tri[1] as usize];
        let v2 = &mesh.vertices[tri[2] as usize];

        let group = v0.group() as usize;
        let res = resolution_lookup[group] as i64;
        let resf = res as f64;

        // UV -> texel space in f64. Pixel (x, y) covers [x, x+1] x [y, y+1].
        let a: P = (v0.uv.x as f64 * resf, v0.uv.y as f64 * resf);
        let b: P = (v1.uv.x as f64 * resf, v1.uv.y as f64 * resf);
        let c: P = (v2.uv.x as f64 * resf, v2.uv.y as f64 * resf);

        // Barycentric setup (point -> weights of v1 and v2).
        let e0 = (b.0 - a.0, b.1 - a.1);
        let e1 = (c.0 - a.0, c.1 - a.1);
        let denom = e0.0 * e1.1 - e1.0 * e0.1;
        if denom.abs() < MIN_TRI_AREA2 || !denom.is_finite() {
            continue; // degenerate in UV space: no area, no coverage
        }
        let inv_denom = 1.0 / denom;

        let min_x = a.0.min(b.0).min(c.0);
        let max_x = a.0.max(b.0).max(c.0);
        let min_y = a.1.min(b.1).min(c.1);
        let max_y = a.1.max(b.1).max(c.1);

        let y_start = (min_y.floor() as i64).max(0);
        let y_end = (max_y.ceil() as i64).min(res); // exclusive
        if y_start >= y_end {
            continue;
        }
        let x_lo = (min_x.floor() as i64).max(0);
        let x_hi = (max_x.ceil() as i64).min(res); // exclusive
        if x_lo >= x_hi {
            continue;
        }

        let mut base = [(0.0, 0.0); MAX_VERTS];
        base[0] = a;
        base[1] = b;
        base[2] = c;

        let mut s0 = [(0.0, 0.0); MAX_VERTS];
        let mut s1 = [(0.0, 0.0); MAX_VERTS];
        let mut p0 = [(0.0, 0.0); MAX_VERTS];
        let mut p1 = [(0.0, 0.0); MAX_VERTS];

        for y in y_start..y_end {
            let yf = y as f64;

            // Clip the triangle to this row strip once.
            let n = clip_half_plane(&base, 3, 1, yf, true, &mut s0);
            let n = clip_half_plane(&s0, n, 1, yf + 1.0, false, &mut s1);
            if n < 3 {
                continue;
            }

            // X extent of the strip polygon limits which pixels are tested.
            let mut sx_min = f64::INFINITY;
            let mut sx_max = f64::NEG_INFINITY;
            for p in &s1[..n] {
                sx_min = sx_min.min(p.0);
                sx_max = sx_max.max(p.0);
            }
            let x_begin = (sx_min.floor() as i64).max(0);
            let x_finish = (sx_max.ceil() as i64).min(res); // exclusive

            for x in x_begin..x_finish {
                let xf = x as f64;

                let n2 = clip_half_plane(&s1, n, 0, xf, true, &mut p0);
                let n2 = clip_half_plane(&p0, n2, 0, xf + 1.0, false, &mut p1);
                if n2 < 3 {
                    continue;
                }

                let (area, centroid) = area_centroid(&p1, n2, (xf, yf));
                if area.abs() <= MIN_COVERAGE_AREA {
                    continue; // touches pixel only along an edge/corner
                }

                // Representative sample: centroid of the covered region, which
                // lies inside both the triangle and the pixel.
                let d = (centroid.0 - a.0, centroid.1 - a.1);
                let mut u = (d.0 * e1.1 - e1.0 * d.1) * inv_denom;
                let mut v = (e0.0 * d.1 - d.0 * e0.1) * inv_denom;

                // Guard against rounding pushing us marginally outside.
                u = u.clamp(0.0, 1.0);
                v = v.clamp(0.0, 1.0);
                let sum = u + v;
                if sum > 1.0 {
                    u /= sum;
                    v /= sum;
                }

                texels.push(VisibilityTexel {
                    bary: Vector2 {
                        x: u as f32,
                        y: v as f32,
                    },
                    primitive_id: prim,
                    x: x as u16,
                    y: y as u16,
                });
            }
        }
    }

    texels
}
