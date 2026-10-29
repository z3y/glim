use crate::{LightmapGroup, Mesh, Vector2, math::Vector3, mesh::decode_normal_octahedron};

#[repr(C)]
#[derive(Debug, Clone)]
pub struct VisibilityTexel {
    pub bary: Vector2,
    pub primitive_id: u32,
    pub x: u16,
    pub y: u16,
}

impl VisibilityTexel {
    #[inline]
    fn index(&self, resolution: usize) -> usize {
        let x = self.x as usize;
        let y = resolution - 1 - self.y as usize;

        y * resolution + x
    }
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
#[allow(dead_code)]
pub fn render_normals(
    texels: &[VisibilityTexel],
    coverage: &[f32],
    mesh: &Mesh,
    group: &LightmapGroup,
) -> Vec<f32> {
    let resolution = group.settings.width as usize;
    let group_id = group.id;
    let mut normals = vec![0.0; resolution * resolution * 4];

    for compacted_index in 0..texels.len() {
        let texel = &texels[compacted_index];
        let primitive_id = texel.primitive_id as usize;
        let i0 = mesh.indices[primitive_id * 3 + 0] as usize;
        let i1 = mesh.indices[primitive_id * 3 + 1] as usize;
        let i2 = mesh.indices[primitive_id * 3 + 2] as usize;

        let v0 = mesh.vertices[i0];
        let v1 = mesh.vertices[i1];
        let v2 = mesh.vertices[i2];

        if v0.group() != group_id {
            continue;
        }

        let bary = texel.bary;
        let u = bary.x;
        let v = bary.y;
        let w = 1.0 - u - v;

        let n0 = decode_normal_octahedron(v0.normal);
        let n1 = decode_normal_octahedron(v1.normal);
        let n2 = decode_normal_octahedron(v2.normal);

        let normal = n0 * w + n1 * u + n2 * v;

        let cov = coverage[compacted_index];

        let index = texel.index(resolution);
        normals[index * 4 + 0] += normal.x * cov;
        normals[index * 4 + 1] += normal.y * cov;
        normals[index * 4 + 2] += normal.z * cov;
        normals[index * 4 + 3] += cov;
    }

    for px in normals.chunks_exact_mut(4) {
        let w = px[3];
        if w > 0.0 {
            let n = Vector3::new(px[0], px[1], px[2]);
            let n = n.normalize();
            px[0] = n.x;
            px[1] = n.y;
            px[2] = n.z;
        }
    }

    normals
}

pub fn decompact_diffuse(
    texels: &[VisibilityTexel],
    coverage: &[f32],
    lightmap_texels: &[f32],
    mesh: &Mesh,
    group: &LightmapGroup,
) -> (Vec<f32>, Vec<f32>) {
    let resolution = group.settings.width as usize;
    let group_id = group.id;
    let mut color = vec![0.0; resolution * resolution * 4];

    let mut cov_out = vec![0.0f32; resolution * resolution];

    for compacted_index in 0..texels.len() {
        let texel = &texels[compacted_index];

        let primitive_id = texel.primitive_id as usize;
        let i0 = mesh.indices[primitive_id * 3 + 0] as usize;
        let v0 = mesh.vertices[i0];

        if v0.group() != group_id {
            continue;
        }

        let cov = coverage[compacted_index];
        let col_x = lightmap_texels[compacted_index * 3 + 0];
        let col_y = lightmap_texels[compacted_index * 3 + 1];
        let col_z = lightmap_texels[compacted_index * 3 + 2];

        let index = texel.index(resolution);

        color[index * 4 + 0] += col_x * cov;
        color[index * 4 + 1] += col_y * cov;
        color[index * 4 + 2] += col_z * cov;
        color[index * 4 + 3] += cov;
    }

    for (i, pixel) in color.chunks_exact_mut(4).enumerate() {
        let total_cov = pixel[3].min(1.0);

        if total_cov > 0.0 {
            pixel[0] /= total_cov;
            pixel[1] /= total_cov;
            pixel[2] /= total_cov;
            pixel[3] = total_cov;

            cov_out[i] = total_cov;
        }
    }

    (color, cov_out)
}

pub fn encode_directional(pixels: &mut [f32], normals: &[f32]) {
    for (i, px) in pixels.chunks_exact_mut(4).enumerate() {
        let w = px[3];
        if w > 0.0 {
            let dir = Vector3::new(px[0], px[1], px[2]).normalize();
            let n = Vector3::new(normals[i * 4 + 0], normals[i * 4 + 1], normals[i * 4 + 2]);
            let nol = dir.dot(n).clamp(0.0, 1.0);

            px[0] = dir.x * 0.5 + 0.5;
            px[1] = dir.y * 0.5 + 0.5;
            px[2] = (-dir.z) * 0.5 + 0.5;
            px[3] = nol * 0.5 + 0.5;
        } else {
            px[0] = 0.5;
            px[1] = 0.5;
            px[2] = 0.5;
            px[3] = 0.5;
        }
    }
}

pub fn decompact_directional(
    texels: &[VisibilityTexel],
    coverage: &[f32],
    lightmap_texels: &[f32],
    mesh: &Mesh,
    group: &LightmapGroup,
) -> Vec<f32> {
    let resolution = group.settings.width as usize;
    let group_id = group.id;
    let mut color = vec![0.0; resolution * resolution * 4];

    let directional_offset = texels.len();

    for compacted_index in 0..texels.len() {
        let texel = &texels[compacted_index];

        let primitive_id = texel.primitive_id as usize;
        let i0 = mesh.indices[primitive_id * 3 + 0] as usize;
        let v0 = mesh.vertices[i0];

        if v0.group() != group_id {
            continue;
        }

        let cov = coverage[compacted_index];
        let diffuse = Vector3::new(
            lightmap_texels[compacted_index * 3 + 0],
            lightmap_texels[compacted_index * 3 + 1],
            lightmap_texels[compacted_index * 3 + 2],
        );
        let directional = Vector3::new(
            lightmap_texels[(compacted_index + directional_offset) * 3 + 0],
            lightmap_texels[(compacted_index + directional_offset) * 3 + 1],
            lightmap_texels[(compacted_index + directional_offset) * 3 + 2],
        );

        let luminance = diffuse.luminance();
        if luminance > 0.0 {
            let dir = (directional / luminance).normalize();
            let index = texel.index(resolution);

            color[index * 4 + 0] += dir.x * cov;
            color[index * 4 + 1] += dir.y * cov;
            color[index * 4 + 2] += dir.z * cov;
            color[index * 4 + 3] += cov;
        }
    }

    for px in color.chunks_exact_mut(4) {
        let w = px[3];
        if w > 0.0 {
            let dir = Vector3::new(px[0], px[1], px[2]);
            let dir = dir.normalize();
            px[0] = dir.x;
            px[1] = dir.y;
            px[2] = dir.z;
        } else {
            px[0] = 0.5;
            px[1] = 0.5;
            px[2] = 0.5;
        }
    }

    color
}

/// Dilates an RGBA f32 texture (stride 4) where alpha = texel coverage.
/// Each empty texel copies the color of ONE neighbor, never a blend.
/// Real texels always beat propagated ones; among real texels the best
/// coverage (scaled by distance) wins. Real texels are never modified.
/// Filled texels get alpha = 1.0.
pub fn dilate_texture(color: &mut [f32], resolution: usize, iterations: usize) {
    const NEIGHBORS: [(i32, i32, f32); 8] = [
        (-1, 0, 1.0),
        (1, 0, 1.0),
        (0, -1, 1.0),
        (0, 1, 1.0),
        (-1, -1, 0.7071),
        (1, -1, 0.7071),
        (-1, 1, 0.7071),
        (1, 1, 0.7071),
    ];
    let res = resolution as i32;

    let real: Vec<bool> = color.chunks_exact(4).map(|p| p[3] > 0.0).collect();
    let mut filled = vec![false; resolution * resolution];
    // Coverage of the original real texel each filled texel was copied from,
    // so propagated texels keep ranking by the quality of their source.
    let mut inherited = vec![0.0f32; resolution * resolution];

    for _ in 0..iterations {
        let src = color.to_vec();
        let src_filled = filled.clone();
        let src_inherited = inherited.clone();
        let mut changed = false;

        for y in 0..res {
            for x in 0..res {
                let i = (y * res + x) as usize;
                if real[i] || src_filled[i] {
                    continue;
                }

                // Best candidate as (tier, score): tier 1 = real, 0 = propagated.
                let mut best: Option<(u8, f32, usize, f32)> = None; // tier, score, index, source coverage

                for &(dx, dy, k) in &NEIGHBORS {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= res || ny >= res {
                        continue;
                    }
                    let ni = (ny * res + nx) as usize;

                    let (tier, cov) = if real[ni] {
                        (1u8, src[ni * 4 + 3].min(1.0))
                    } else if src_filled[ni] {
                        (0u8, src_inherited[ni])
                    } else {
                        continue;
                    };

                    // Coverage scaled by distance: a cardinal neighbor wins ties,
                    // but a clearly better covered diagonal can still win.
                    let score = cov * k;

                    let better = match best {
                        None => true,
                        Some((bt, bs, _, _)) => tier > bt || (tier == bt && score > bs),
                    };
                    if better {
                        best = Some((tier, score, ni, cov));
                    }
                }

                if let Some((_, _, ni, cov)) = best {
                    color[i * 4] = src[ni * 4];
                    color[i * 4 + 1] = src[ni * 4 + 1];
                    color[i * 4 + 2] = src[ni * 4 + 2];
                    color[i * 4 + 3] = 1.0;
                    filled[i] = true;
                    inherited[i] = cov;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
}

pub struct Texels {
    // vecs with equal len
    pub visibility: Vec<VisibilityTexel>,
    pub coverage: Vec<f32>,

    // corresponds to each lightmap group
    pub coverage_counts: Vec<Vec<u8>>,
}

pub fn rasterize_texels(
    mesh: &Mesh,
    groups: &[LightmapGroup],
    coverage_threshold: f32,
    max_coverage: u32,
) -> Texels {
    // avoids too many reallocations
    let approx_texels = {
        let mut sum = 0;
        for g in groups {
            sum += g.settings.width * g.settings.width;
        }
        sum as usize
    };

    let mut visibility_and_coverage = Vec::with_capacity(approx_texels);
    // let mut coverage = Vec::with_capacity(approx_texels);

    let mut coverage_counts = Vec::with_capacity(groups.len());
    let mut resolution_lookups = Vec::with_capacity(groups.len());
    for g in groups {
        let res = g.settings.width as usize;
        coverage_counts.push(vec![0u8; res * res]);
        resolution_lookups.push(res);
    }

    let mut primitive_id: u32 = 0;

    for tri in mesh.indices.chunks_exact(3) {
        let prim = primitive_id;
        primitive_id += 1;

        let v0 = &mesh.vertices[tri[0] as usize];
        let v1 = &mesh.vertices[tri[1] as usize];
        let v2 = &mesh.vertices[tri[2] as usize];

        let group = v0.group() as usize;
        let res = resolution_lookups[group] as i64;
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

                let texel = VisibilityTexel {
                    bary: Vector2 {
                        x: u as f32,
                        y: v as f32,
                    },
                    primitive_id: prim,
                    x: x as u16,
                    y: y as u16,
                };

                let coverage_count = &mut coverage_counts[group];
                let index = texel.index(res as usize);
                coverage_count[index] = coverage_count[index].saturating_add(1u8);

                let c = area.abs().clamp(0.0, 1.0) as f32;
                visibility_and_coverage.push((texel, c));
            }
        }
    }

    let key = |x: &(VisibilityTexel, f32)| -> u64 {
        let t = &x.0;
        let primitive_id = t.primitive_id;
        let vertex_index = mesh.indices[primitive_id as usize * 3];
        let group_index = mesh.vertices[vertex_index as usize].group();
        let resolution = resolution_lookups[group_index as usize];
        let texel_index = t.index(resolution);
        ((group_index as u64) << 32) | texel_index as u64
    };
    visibility_and_coverage
        .sort_unstable_by(|a, b| key(a).cmp(&key(b)).then_with(|| b.1.total_cmp(&a.1)));

    let mut visibility = Vec::with_capacity(visibility_and_coverage.len());
    let mut coverage = Vec::with_capacity(visibility_and_coverage.len());

    let mut prev_key = (u32::MAX, usize::MAX);
    let mut current_coverage = 0;
    for (texel, cov) in visibility_and_coverage {
        let primitive_id = texel.primitive_id;
        let vertex_index = mesh.indices[primitive_id as usize * 3];
        let group_index = mesh.vertices[vertex_index as usize].group();
        let resolution = resolution_lookups[group_index as usize];
        let texel_index = texel.index(resolution);
        let key = (group_index, texel_index);

        if key != prev_key {
            current_coverage = 0;
            prev_key = key;
        }

        if (cov >= coverage_threshold && current_coverage < max_coverage) || current_coverage == 0 {
            visibility.push(texel);
            coverage.push(cov);
            current_coverage += 1;
        }
    }

    // todo set coverage to 1.0 for texels with only one triangle
    // for higher precision when decompacting

    // todo maybe trim the vecs to avoid larger peak memory

    Texels {
        visibility,
        coverage,
        coverage_counts,
    }
}

pub fn build_compaction(
    texels: &Texels,
    mesh: &Mesh,
    groups: &[LightmapGroup],
) -> (Vec<[u32; 2]>, Vec<u32>, u32) {
    let visibility = &texels.visibility;

    let mut per_group: Vec<Vec<[u32; 2]>> = groups
        .iter()
        .map(|g| {
            let res = g.settings.width as usize;
            vec![[0u32; 2]; (res * res).div_ceil(32)]
        })
        .collect();

    // One entry per occupied cell: [start, count].
    // Relies on rasterize_texels output being sorted by (group, cell),
    // with the best-coverage texel first in each run.
    let mut table: Vec<[u32; 2]> = Vec::new();
    let mut run: Option<(usize, usize)> = None;

    for (i, t) in visibility.iter().enumerate() {
        let v0 = &mesh.vertices[mesh.indices[t.primitive_id as usize * 3] as usize];
        let g = v0.group() as usize;
        let res = groups[g].settings.width as usize;
        let cell = t.index(res);
        per_group[g][cell >> 5][0] |= 1u32 << (cell & 31);

        if run != Some((g, cell)) {
            run = Some((g, cell));
            table.push([i as u32, 1]);
        } else {
            table.last_mut().unwrap()[1] += 1;
        }
    }

    let mut words: Vec<[u32; 2]> = Vec::new();
    let mut offsets = Vec::with_capacity(groups.len());
    let mut cells = 0u32;
    for g_words in per_group.iter_mut() {
        offsets.push(words.len() as u32);
        for w in g_words.iter_mut() {
            w[1] = cells; // global cell rank at the start of this word
            cells += w[0].count_ones();
        }
        words.extend_from_slice(g_words);
    }
    assert_eq!(cells as usize, table.len());

    let table_base = words.len() as u32;
    words.extend_from_slice(&table);
    (words, offsets, table_base)
}
