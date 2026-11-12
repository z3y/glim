use crate::{
    LightmapGroup, Mesh, Vector2,
    math::{Vector3, Vector4, vec3, vec4, vec4_slice},
};

#[repr(C, align(16))]
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

pub fn render_normals(
    buffer: &mut [Vector4],
    texels: &[VisibilityTexel],
    coverage: &[f32],
    mesh: &Mesh,
    group: &LightmapGroup,
) {
    let resolution = group.settings.width as usize;
    let group_id = group.id;

    let count = resolution * resolution;
    assert!(buffer.len() >= count);

    let buffer = &mut buffer[0..count];
    buffer.fill(Vector4::ZERO);

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

        let n0 = v0.decode_normal();
        let n1 = v1.decode_normal();
        let n2 = v2.decode_normal();

        let normal = (n0 * w + n1 * u + n2 * v).normalize();

        let cov = coverage[compacted_index];

        let index = texel.index(resolution);

        let col = vec4(normal.x * cov, normal.y * cov, normal.z * cov, cov);
        buffer[index] += col;
    }

    for px in buffer {
        let w = px.w;

        if w > 0.0 {
            let n = Vector3::new(px.x, px.y, px.z);
            let n = n.normalize();
            px.x = n.x;
            px.y = n.y;
            px.z = n.z;
        }
    }
}

pub fn decompact_diffuse(
    buffer: &mut [Vector4],
    texels: &[VisibilityTexel],
    coverage: &[f32],
    lightmap_texels: &[f32],
    mesh: &Mesh,
    group: &LightmapGroup,
    l0: bool,
) {
    let resolution = group.settings.width as usize;
    let group_id = group.id;

    let count = resolution * resolution;

    let buffer = &mut buffer[0..count];
    buffer.fill(Vector4::ZERO);

    let diffuse_offset = if l0 { texels.len() } else { 0 };

    for compacted_index in 0..texels.len() {
        let texel = &texels[compacted_index];

        let primitive_id = texel.primitive_id;
        if primitive_id == u32::MAX {
            continue;
        }
        let i0 = mesh.indices[(primitive_id as usize) * 3 + 0] as usize;
        let v0 = mesh.vertices[i0];

        if v0.group() != group_id {
            continue;
        }

        let cov = coverage[compacted_index];
        let col_x = lightmap_texels[(compacted_index + diffuse_offset) * 3 + 0];
        let col_y = lightmap_texels[(compacted_index + diffuse_offset) * 3 + 1];
        let col_z = lightmap_texels[(compacted_index + diffuse_offset) * 3 + 2];

        if !col_x.is_finite() || !col_y.is_finite() || !col_z.is_finite() {
            continue;
        }

        let index = texel.index(resolution);

        let col = vec4(col_x * cov, col_y * cov, col_z * cov, cov);
        buffer[index] += col;
    }

    for col in buffer {
        let w = col.w;

        if w > 0.0 {
            col.x /= w;
            col.y /= w;
            col.z /= w;
        }
    }
}

pub fn decompact_shadowmask(
    buffer: &mut [Vector4],
    buffer_coverage: &mut [f32],
    texels: &[VisibilityTexel],
    coverage: &[f32],
    lightmap_texels: &[f32],
    mesh: &Mesh,
    group: &LightmapGroup,
) {
    let resolution = group.settings.width as usize;
    let group_id = group.id;

    let count = resolution * resolution;

    let buffer = &mut buffer[0..count];
    buffer.fill(Vector4::ZERO);

    for compacted_index in 0..texels.len() {
        let texel = &texels[compacted_index];

        let primitive_id = texel.primitive_id;
        if primitive_id == u32::MAX {
            continue;
        }
        let i0 = mesh.indices[(primitive_id as usize) * 3 + 0] as usize;
        let v0 = mesh.vertices[i0];

        if v0.group() != group_id {
            continue;
        }

        let cov = coverage[compacted_index];
        let col_r = lightmap_texels[compacted_index * 4 + 0];
        let col_g = lightmap_texels[compacted_index * 4 + 1];
        let col_b = lightmap_texels[compacted_index * 4 + 2];
        let col_a = lightmap_texels[compacted_index * 4 + 3];

        let index = texel.index(resolution);

        let col = vec4(col_r, col_g, col_b, col_a) * cov;
        buffer[index] += col;
        buffer_coverage[index] += cov;
    }

    for i in 0..buffer.len() {
        let w = buffer_coverage[i];
        if w > 0.0 {
            buffer[i] /= w;
        }
    }
}

pub fn encode_directional(pixels: &mut [Vector4], normals: &[Vector4], tangent_space: bool) {
    for (i, px) in pixels.iter_mut().enumerate() {
        let mut dir = Vector3::new(px.x, px.y, px.z);

        if dir.length() > 0.0 {
            dir = if dir.length() > 1.0 {
                dir.normalize()
            } else {
                dir
            };

            let n = if tangent_space {
                vec3(0.0, 0.0, 1.0)
            } else {
                vec3(normals[i].x, normals[i].y, normals[i].z)
            };

            let nol = dir.dot(n);

            if tangent_space {
                // let oct = encode_hemi_octahedron(dir);
                // px.x = oct.x * 0.5 + 0.5;
                // px.y = oct.y * 0.5 + 0.5;
                // px.z = nol;

                px.x = dir.x * 0.5 + 0.5;
                px.y = dir.y * 0.5 + 0.5;
                px.z = dir.z * 0.5 + 0.5;
                px.w = nol;
            } else {
                dir.z = -dir.z; // unity
                px.x = dir.x * 0.5 + 0.5;
                px.y = dir.y * 0.5 + 0.5;
                px.z = dir.z * 0.5 + 0.5;
                px.w = nol * 0.5 + 0.5;
            }
        } else {
            if tangent_space {
                px.x = 0.5;
                px.y = 0.5;
                // px.z = 0.0;
                px.z = 0.5;
                px.w = 0.0;
            } else {
                px.x = 0.5;
                px.y = 0.5;
                px.z = 0.5;
                px.w = 0.5;
            };
        }
    }
}

pub fn decompact_directional(
    buffer: &mut [Vector4],
    texels: &[VisibilityTexel],
    coverage: &[f32],
    lightmap_texels: &[f32],
    mesh: &Mesh,
    group: &LightmapGroup,
    tangent_space: bool,
) {
    let resolution = group.settings.width as usize;
    let group_id = group.id;

    let count = resolution * resolution;

    let buffer = &mut buffer[0..count];
    buffer.fill(Vector4::ZERO);

    let directional_offset = texels.len();

    for compacted_index in 0..texels.len() {
        let texel = &texels[compacted_index];

        let primitive_id = texel.primitive_id;
        if primitive_id == u32::MAX {
            continue;
        }
        let i0 = mesh.indices[(primitive_id as usize) * 3 + 0] as usize;
        let i1 = mesh.indices[(primitive_id as usize) * 3 + 1] as usize;
        let i2 = mesh.indices[(primitive_id as usize) * 3 + 2] as usize;

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

        if !diffuse.x.is_finite() || !diffuse.y.is_finite() || !diffuse.z.is_finite() {
            continue;
        }

        let directional = Vector3::new(
            lightmap_texels[(compacted_index + directional_offset) * 3 + 0],
            lightmap_texels[(compacted_index + directional_offset) * 3 + 1],
            lightmap_texels[(compacted_index + directional_offset) * 3 + 2],
        );

        if !directional.x.is_finite() || !directional.y.is_finite() || !directional.z.is_finite() {
            continue;
        }

        let luminance = diffuse.luminance();
        if luminance > 0.0 {
            let mut dir = directional / luminance;

            if dir.length() > 1.0 {
                dir = dir.normalize();
            }

            if tangent_space {
                let u = texel.bary.x;
                let v = texel.bary.y;
                let w = 1.0 - u - v;

                let n0 = mesh.original_normals[i0];
                let n1 = mesh.original_normals[i1];
                let n2 = mesh.original_normals[i2];

                dir.z = -dir.z; //unity

                let normal = (n0 * w + n1 * u + n2 * v).normalize();

                let t0 = vec4_slice(mesh.tangents[i0]);
                let t1 = vec4_slice(mesh.tangents[i1]);
                let t2 = vec4_slice(mesh.tangents[i2]);

                let tangent4 = t0 * w + t1 * u + t2 * v;
                let tangent = vec3(tangent4.x, tangent4.y, tangent4.z);
                let tangent = (tangent - normal * normal.dot(tangent)).normalize();

                let handedness = if tangent4.w < 0.0 { -1.0 } else { 1.0 };
                let bitangent = normal.cross(tangent) * handedness;

                dir = vec3(dir.dot(tangent), dir.dot(bitangent), dir.dot(normal));
            }
            let index = texel.index(resolution);

            let col = vec4(dir.x * cov, dir.y * cov, dir.z * cov, cov);
            buffer[index] += col;
        }
    }

    for px in buffer {
        let w = px.w;

        if w > 0.0 {
            px.x /= w;
            px.y /= w;
            px.z /= w;
        } else {
            px.x = 0.0;
            px.y = 0.0;
            px.z = 0.0;
        }
    }
}

#[inline]
fn encode_combined_sh(l0: Vector3, l1: Vector3) -> Vector3 {
    let safe_f0 = l0.luminance().max(1e-4);
    let l1 = l1 / (safe_f0 * 2.0).clamp(-1.0, 1.0);

    Vector3 {
        x: (l1.x * 0.5 + 0.5).clamp(0.0, 1.0),
        y: (l1.y * 0.5 + 0.5).clamp(0.0, 1.0),
        z: (l1.z * 0.5 + 0.5).clamp(0.0, 1.0),
    }
}

pub fn decompact_combined_sh(
    buffer: &mut [Vector4],
    texels: &[VisibilityTexel],
    coverage: &[f32],
    lightmap_texels: &[f32],
    mesh: &Mesh,
    group: &LightmapGroup,
) {
    let resolution = group.settings.width as usize;
    let group_id = group.id;

    let count = resolution * resolution;

    let buffer = &mut buffer[0..count];
    buffer.fill(Vector4::ZERO);

    let l0_offset = texels.len();
    let l1_offset = texels.len() * 2;

    for compacted_index in 0..texels.len() {
        let texel = &texels[compacted_index];

        let primitive_id = texel.primitive_id;
        if primitive_id == u32::MAX {
            continue;
        }
        let i0 = mesh.indices[(primitive_id as usize) * 3 + 0] as usize;
        let v0 = mesh.vertices[i0];

        if v0.group() != group_id {
            continue;
        }

        let cov = coverage[compacted_index];

        let l0 = Vector3::new(
            lightmap_texels[(compacted_index + l0_offset) * 3 + 0],
            lightmap_texels[(compacted_index + l0_offset) * 3 + 1],
            lightmap_texels[(compacted_index + l0_offset) * 3 + 2],
        );

        let l1 = Vector3::new(
            lightmap_texels[(compacted_index + l1_offset) * 3 + 0],
            lightmap_texels[(compacted_index + l1_offset) * 3 + 1],
            lightmap_texels[(compacted_index + l1_offset) * 3 + 2],
        );

        if !l0.x.is_finite() || !l0.y.is_finite() || !l0.z.is_finite() {
            continue;
        }

        if !l1.x.is_finite() || !l1.y.is_finite() || !l1.z.is_finite() {
            continue;
        }

        let sh = encode_combined_sh(l0, l1);

        let index = texel.index(resolution);

        let col = vec4(sh.x * cov, sh.y * cov, sh.z * cov, cov);
        buffer[index] += col;
    }

    for px in buffer {
        let w = px.w;

        if w > 0.0 {
            px.x /= w;
            px.y /= w;
            px.z /= w;
        }
    }
}

pub fn dilate_texture(pixels: &mut [Vector4], mask: &mut [bool], resolution: usize, radius: usize) {
    let mask = &mut mask[0..(resolution * resolution)];

    for y in 0..resolution {
        for x in 0..resolution {
            let index = y * resolution + x;

            let cov = pixels[index].w;
            let covered = cov > 0.0;

            if covered {
                mask[index] = true;
                continue;
            }

            let mut best_index = None;
            let mut best_ring = usize::MAX;
            let mut best_cov = 0.0f32;
            let mut best_dist = usize::MAX;

            let min_x = x.saturating_sub(radius);
            let max_x = (x + radius).min(resolution - 1);
            let min_y = y.saturating_sub(radius);
            let max_y = (y + radius).min(resolution - 1);

            for sy in min_y..=max_y {
                for sx in min_x..=max_x {
                    let dx = sx.abs_diff(x);
                    let dy = sy.abs_diff(y);
                    let ring = dx.max(dy);

                    if ring > best_ring {
                        continue;
                    }

                    let si = sy * resolution + sx;
                    let dist = dx * dx + dy * dy;

                    let sample_cov = pixels[si].w;
                    if sample_cov <= 0.0 {
                        continue;
                    }

                    let better = ring < best_ring
                        || sample_cov > best_cov
                        || (sample_cov == best_cov && dist < best_dist);

                    if better {
                        best_index = Some(si);
                        best_ring = ring;
                        best_cov = sample_cov;
                        best_dist = dist;
                    }
                }
            }

            if let Some(si) = best_index {
                pixels[index].x = pixels[si].x;
                pixels[index].y = pixels[si].y;
                pixels[index].z = pixels[si].z;
                mask[index] = true;
            } else {
                pixels[index].x = 0.0;
                pixels[index].y = 0.0;
                pixels[index].z = 0.0;
                mask[index] = false;
            }
        }
    }
}

pub fn dilate_shadowmask(
    color: &mut [Vector4],
    coverage: &[f32],
    resolution: usize,
    radius: usize,
) {
    for y in 0..resolution {
        for x in 0..resolution {
            let index = y * resolution + x;

            let cov = coverage[index];
            let covered = cov > 0.0;

            if covered {
                continue;
            }

            let mut best_index = None;
            let mut best_ring = usize::MAX;
            let mut best_cov = 0.0f32;
            let mut best_dist = usize::MAX;

            let min_x = x.saturating_sub(radius);
            let max_x = (x + radius).min(resolution - 1);
            let min_y = y.saturating_sub(radius);
            let max_y = (y + radius).min(resolution - 1);

            for sy in min_y..=max_y {
                for sx in min_x..=max_x {
                    let dx = sx.abs_diff(x);
                    let dy = sy.abs_diff(y);
                    let ring = dx.max(dy);

                    if ring > best_ring {
                        continue;
                    }

                    let si = sy * resolution + sx;
                    let dist = dx * dx + dy * dy;

                    let sample_cov = coverage[sy * resolution + sx];
                    if sample_cov <= 0.0 {
                        continue;
                    }

                    let better = ring < best_ring
                        || sample_cov > best_cov
                        || (sample_cov == best_cov && dist < best_dist);

                    if better {
                        best_index = Some(si);
                        best_ring = ring;
                        best_cov = sample_cov;
                        best_dist = dist;
                    }
                }
            }

            if let Some(si) = best_index {
                color[index] = color[si];
            } else {
                color[index] = Vector4::ZERO;
            }
        }
    }
}

pub fn block_average_fill(
    pixels: &mut [Vector4],
    copy: &mut [Vector4],
    mask: &[bool],
    resolution: usize,
) {
    const BLOCK: usize = 4;

    // level 0 = pixels, level l = (level l-1) reduced 4x4
    let mut sizes = [0usize; 16];
    let mut color_base = [0usize; 16]; // texel offset of each level inside copy
    sizes[0] = resolution;

    let mut levels = 0;
    let mut total = 0;
    while sizes[levels] > 1 {
        let next = levels + 1;
        sizes[next] = (sizes[levels] + BLOCK - 1) / BLOCK;
        color_base[next] = total;
        total += sizes[next] * sizes[next];
        levels = next;
    }

    // copy layout: [ color of every level (total) | weight of every level (total, stored in .x) ]
    let weight_base = total;
    assert!(copy.len() >= total * 2, "copy buffer too small");

    // Pull: average the blocks of the level below, weighted by how many real pixels they cover
    for l in 1..=levels {
        let size = sizes[l];
        let child = sizes[l - 1];

        for y in 0..size {
            for x in 0..size {
                let mut sum = Vector4::ZERO;
                let mut weight = 0.0f32;

                for cy in (y * BLOCK)..((y + 1) * BLOCK).min(child) {
                    for cx in (x * BLOCK)..((x + 1) * BLOCK).min(child) {
                        let ci = cy * child + cx;

                        if l == 1 {
                            if mask[ci] {
                                sum = sum + pixels[ci];
                                weight += 1.0;
                            }
                        } else {
                            let t = color_base[l - 1] + ci;
                            let w = copy[weight_base + t].x;
                            if w > 0.0 {
                                sum = sum + copy[t] * w;
                                weight += w;
                            }
                        }
                    }
                }

                let t = color_base[l] + y * size + x;
                let scale = if weight > 0.0 { 1.0 / weight } else { 0.0 };
                copy[t] = sum * scale;
                copy[weight_base + t] = Vector4::new(weight, 0.0, 0.0, 0.0);
            }
        }
    }

    // Fill: each empty pixel walks up until it finds a block that has coverage
    for py in 0..resolution {
        for px in 0..resolution {
            let index = py * resolution + px;
            if mask[index] {
                continue;
            }

            let (mut x, mut y) = (px, py);
            for l in 1..=levels {
                x /= BLOCK;
                y /= BLOCK;
                let t = color_base[l] + y * sizes[l] + x;

                if copy[weight_base + t].x > 0.0 {
                    pixels[index] = copy[t];
                    break;
                }
            }
        }
    }
}

pub struct Texels {
    // vecs with equal len
    pub visibility: Vec<VisibilityTexel>,
    pub coverage: Vec<f32>,
    // corresponds to each lightmap group
    // pub coverage_counts: Vec<Vec<u8>>,
}

pub fn rasterize_texels(
    mesh: &Mesh,
    groups: &[LightmapGroup],
    coverage_min_threshold: f32,
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

    // let mut coverage_counts = Vec::with_capacity(groups.len());
    let mut resolution_lookups = Vec::with_capacity(groups.len());
    for g in groups {
        let res = g.settings.width as usize;
        // coverage_counts.push(vec![0u8; res * res]);
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

                // let coverage_count = &mut coverage_counts[group];
                // let index = texel.index(res as usize);
                // coverage_count[index] = coverage_count[index].saturating_add(1u8);

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

        if (cov >= coverage_min_threshold && current_coverage < max_coverage)
            || current_coverage == 0
        {
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
        // coverage_counts,
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
