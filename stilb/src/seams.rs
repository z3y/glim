use std::collections::{HashMap, HashSet};

use crate::math::*;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct Edge {
    pub i0: u32,
    pub i1: u32,
}

impl Edge {
    #[inline]
    fn new_sorted(i0: u32, i1: u32) -> Self {
        if i0 < i1 {
            Self { i0, i1 }
        } else {
            Self { i0: i1, i1: i0 }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Seam {
    pub position0: Vector3,
    pub position1: Vector3,
    pub edge0_uv0: Vector2,
    pub edge1_uv0: Vector2,
    pub edge0_uv1: Vector2,
    pub edge1_uv1: Vector2,
}

#[derive(Debug, Clone)]
pub struct SamplePoint {
    pub position: Vector3,
    pub uv_a: Vector2,
    pub uv_b: Vector2,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct Vector2Int {
    x: i32,
    y: i32,
}

impl Vector2Int {
    pub fn new(x: i32, y: i32) -> Self {
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
    const EPS: f32 = 0.0001;
    (a - b).length_squared() < EPS * EPS
}

#[inline]
fn approx_eq_vec2(a: Vector2, b: Vector2) -> bool {
    const EPS: f32 = 0.0001;
    (a - b).length_squared() < EPS * EPS
}

pub fn find_seams(
    indices: &[u32],
    positions: &[Vector3],
    normals: &[Vector3],
    uvs: &[Vector2],
) -> Vec<Seam> {
    let mut edges = HashSet::new();

    let is_seam = |a: &Edge, b: &Edge| -> bool {
        let pa0 = positions[a.i0 as usize];
        let na0 = normals[a.i0 as usize];
        let uva0 = uvs[a.i0 as usize];

        let pb0 = positions[b.i0 as usize];
        let nb0 = normals[b.i0 as usize];
        let uvb0 = uvs[b.i0 as usize];

        let positions_equal = approx_eq_vec3(pa0, pb0);
        let normals_equal = approx_eq_vec3(na0, nb0);
        let uvs_equal = approx_eq_vec2(uva0, uvb0);

        if positions_equal && normals_equal && !uvs_equal {
            let pa1 = positions[a.i1 as usize];
            let na1 = normals[a.i1 as usize];
            let uva1 = uvs[a.i1 as usize];
            let pb1 = positions[b.i1 as usize];
            let nb1 = normals[b.i1 as usize];
            let uvb1 = uvs[b.i1 as usize];

            let positions_equal = approx_eq_vec3(pa1, pb1);
            let normals_equal = approx_eq_vec3(na1, nb1);
            let uvs_equal = approx_eq_vec2(uva1, uvb1);

            return positions_equal && normals_equal && !uvs_equal;
        }

        false
    };

    let mut i = 0;
    while i + 2 < indices.len() {
        let i0 = indices[i + 0];
        let i1 = indices[i + 1];
        let i2 = indices[i + 2];

        edges.insert(Edge::new_sorted(i0, i1));
        edges.insert(Edge::new_sorted(i1, i2));
        edges.insert(Edge::new_sorted(i2, i0));

        i += 3;
    }

    let edges: Vec<Edge> = edges.into_iter().collect();

    let mut seam_edges = Vec::new();

    let mut seams = Vec::new();
    for i in 0..edges.len() {
        for j in (i + 1)..edges.len() {
            let e0 = &edges[i];
            let e1 = &edges[j];

            if is_seam(e0, e1) {
                seam_edges.push(e0.clone());
                seam_edges.push(e1.clone());

                let position0 = positions[e0.i0 as usize];
                let position1 = positions[e0.i1 as usize];

                let edge0_uv0 = uvs[e0.i0 as usize];
                let edge0_uv1 = uvs[e0.i1 as usize];

                let edge1_uv0 = uvs[e1.i0 as usize];
                let edge1_uv1 = uvs[e1.i1 as usize];

                debug_assert!(approx_eq_vec3(
                    positions[e0.i0 as usize],
                    positions[e1.i0 as usize]
                ));
                debug_assert!(approx_eq_vec3(
                    positions[e0.i1 as usize],
                    positions[e1.i1 as usize]
                ));
                debug_assert!(approx_eq_vec3(
                    normals[e0.i0 as usize],
                    normals[e1.i0 as usize]
                ));
                debug_assert!(approx_eq_vec3(
                    normals[e0.i1 as usize],
                    normals[e1.i1 as usize]
                ));

                seams.push(Seam {
                    position0,
                    position1,
                    edge0_uv0,
                    edge0_uv1,
                    edge1_uv0,
                    edge1_uv1,
                });
            }
        }
    }

    println!("found {} seams out of {} edges", seams.len(), edges.len());

    // println!("Seams: {:#?} ", seams);

    seams
}

fn is_inside_chart(pixels: &[f32]) -> bool {
    false
}

pub fn fix_seams(pixels: &mut [f32], width: u32, height: u32, seams: &[Seam], sample_scale: f32) {
    let mut sample_points = Vec::new();

    for seam in seams {
        let position0 = seam.position0;
        let position1 = seam.position1;

        let length = Vector3::distance(position0, position1);

        let samples = u32::max(3, (length * sample_scale).ceil() as u32);

        for i in 0..samples {
            let t = i as f32 / (samples - 1) as f32;

            let position = Vector3::lerp(position0, position1, t);

            let uv_a = Vector2::lerp(seam.edge0_uv0, seam.edge0_uv1, t);
            let uv_b = Vector2::lerp(seam.edge1_uv0, seam.edge1_uv1, t);

            sample_points.push(SamplePoint {
                position,
                uv_a,
                uv_b,
            });
        }
    }

    let mut pixel_info = Vec::new();
    let mut self_pixel_map: HashMap<Vector2Int, i32> = HashMap::new();
    let mut other_pixel_map: HashMap<Vector2Int, i32> = HashMap::new();

    for point in &sample_points {
        let uv_a = point.uv_a * Vector2::new(width as f32, width as f32) + Vector2::new(0.5, 0.5);
        let uv_b = point.uv_b * Vector2::new(height as f32, height as f32) + Vector2::new(0.5, 0.5);

        let width = width as i32;
        let height = height as i32;

        for i in 0..4 {
            let offset_x = i & 0b01;
            let offset_y = (i & 0b10) >> 1;

            let pos_self = Vector2Int::new(uv_a.x as i32 + offset_x, uv_a.y as i32 + offset_y);
            let pos_other = Vector2Int::new(uv_b.x as i32 + offset_x, uv_b.y as i32 + offset_y);

            if !self_pixel_map.contains_key(&pos_self) && pos_self.x < width && pos_self.y < height
            {
                let pixel_index = ((pos_self.y * width + pos_self.x) * 4) as usize;

                let r = pixels[pixel_index];
                let g = pixels[pixel_index + 1];
                let b = pixels[pixel_index + 2];

                let color = Vector3::new(r, g, b);

                pixel_info.push(PixelInfo {
                    position: pos_self.clone(),
                    color,
                });

                self_pixel_map.insert(pos_self, pixel_info.len() as i32 - 1);
            }

            if !other_pixel_map.contains_key(&pos_other)
                && pos_other.x < width
                && pos_other.y < height
            {
                let pixel_index = ((pos_other.y * width + pos_other.x) * 4) as usize;

                let r = pixels[pixel_index];
                let g = pixels[pixel_index + 1];
                let b = pixels[pixel_index + 2];

                let color = Vector3::new(r, g, b);

                pixel_info.push(PixelInfo {
                    position: pos_other.clone(),
                    color,
                });

                other_pixel_map.insert(pos_other, pixel_info.len() as i32 - 1);
            }
        }
    }

    println!(
        "created sample points {} for {} seams",
        sample_points.len(),
        seams.len()
    );
}
