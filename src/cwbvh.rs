//! CWBVH builder: a Rust port of tinybvh's `BVH8_CWBVH::BuildHQ` path.
//!
//! Pipeline (same stages, same heuristics as the C++):
//!   1. SBVH build (binned SAH + spatial splits + unsplitting), 8 bins, single threaded
//!   2. Compact (depth-first node order, leaf primitive indices in traversal order)
//!   3. Split leaves to at most 3 triangles
//!   4. Collapse the binary tree into an 8-wide tree (greedy, largest-surface-area first)
//!   5. Encode to the compressed wide layout of Ylitie et al. 2017
//!
//! Input:  triangles as `[x, y, z, w]` vertices, three per triangle (w is ignored).
//! Output: `nodes` and `triangles` as raw 16-byte blocks, ready to upload.
//!
//! Node layout (5 blocks = 80 bytes per node, node index = block / 5):
//!   bytes  0..12  origin (f32 x3)       bytes 12..15  exponents ex, ey, ez (i8)
//!   byte  15      imask (bit i = child slot i is interior)
//!   bytes 16..20  child base node index (u32)
//!   bytes 20..24  triangle base block index (u32)
//!   bytes 24..32  per-slot meta: interior = (1<<5)|(24+i); leaf = (unary tri count<<5)|tri offset
//!   bytes 32..80  quantized child boxes: lo.x[8], lo.y[8], lo.z[8], hi.x[8], hi.y[8], hi.z[8]
//! Triangle layout (3 blocks per triangle reference): e2, e1, v0 (v0.w = original triangle index bits).

const BVH_FAR: f32 = 1e30;
const BINS: usize = 8;
const C_TRAV: f32 = 1.0;
const C_INT: f32 = 1.0;
const MAX_LEAF_TRIS: u32 = 3;

type V3 = [f32; 3];

// ---------------------------------------------------------------------------
// small math helpers (mirror tinybvh's NaN behaviour: `a < b ? a : b`)
// ---------------------------------------------------------------------------

#[inline(always)]
fn fmin(a: f32, b: f32) -> f32 {
    if a < b { a } else { b }
}
#[inline(always)]
fn fmax(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}
#[inline(always)]
fn vmin(a: V3, b: V3) -> V3 {
    [fmin(a[0], b[0]), fmin(a[1], b[1]), fmin(a[2], b[2])]
}
#[inline(always)]
fn vmax(a: V3, b: V3) -> V3 {
    [fmax(a[0], b[0]), fmax(a[1], b[1]), fmax(a[2], b[2])]
}
#[inline(always)]
fn vsub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline(always)]
fn vert(verts: &[[f32; 4]], i: usize) -> V3 {
    let v = verts[i];
    [v[0], v[1], v[2]]
}
/// Half surface area of an extent. Empty boxes (extent below -BVH_FAR) give 0.
#[inline(always)]
fn halfarea(e: V3) -> f32 {
    if e[0] < -BVH_FAR {
        0.0
    } else {
        e[0] * e[1] + e[1] * e[2] + e[2] * e[0]
    }
}
#[inline(always)]
fn split_cost_sah(r_parent: f32, a_left: f32, n_left: usize, a_right: f32, n_right: usize) -> f32 {
    C_TRAV + C_INT * r_parent * (a_left * n_left as f32 + a_right * n_right as f32)
}
#[inline(always)]
fn pow2(e: i32) -> f32 {
    f32::from_bits(((e + 127) as u32) << 23)
}

// ---------------------------------------------------------------------------
// data
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
struct Fragment {
    bmin: V3,
    bmax: V3,
    prim: u32,
    clipped: bool,
}

impl Fragment {
    #[inline(always)]
    fn empty(prim: u32, clipped: bool) -> Self {
        Self {
            bmin: [BVH_FAR; 3],
            bmax: [-BVH_FAR; 3],
            prim,
            clipped,
        }
    }
    #[inline(always)]
    fn extend(&mut self, p: V3) {
        self.bmin = vmin(self.bmin, p);
        self.bmax = vmax(self.bmax, p);
    }
}

/// Binary BVH node. Leaf iff `tri_count > 0`.
#[derive(Clone, Copy, Default)]
struct Node {
    bmin: V3,
    bmax: V3,
    left_first: u32,
    tri_count: u32,
}

impl Node {
    #[inline(always)]
    fn is_leaf(&self) -> bool {
        self.tri_count > 0
    }
    #[inline(always)]
    fn sa(&self) -> f32 {
        let e = vsub(self.bmax, self.bmin);
        e[0] * e[1] + e[1] * e[2] + e[2] * e[0]
    }
}

/// 8-wide node used during collapse. `child[i] == 0` means empty slot.
#[derive(Clone, Copy, Default)]
struct WideNode {
    bmin: V3,
    bmax: V3,
    first_tri: u32,
    tri_count: u32,
    child: [u32; 8],
    child_count: u32,
}

impl WideNode {
    #[inline(always)]
    fn sa(&self) -> f32 {
        let e = vsub(self.bmax, self.bmin);
        e[0] * e[1] + e[1] * e[2] + e[2] * e[0]
    }
}

// ---------------------------------------------------------------------------
// public API
// ---------------------------------------------------------------------------

pub struct Cwbvh {
    /// 16-byte blocks, 5 per node. Contains raw u32 words (floats stored as bits).
    pub nodes: Vec<[u32; 4]>,
    /// 16-byte blocks, 3 per triangle reference.
    pub triangles: Vec<[u32; 4]>,
}

impl Cwbvh {
    /// Build over `triangles` (three `[x,y,z,w]` vertices per triangle).
    /// Empty input gives an empty BVH.
    pub fn build(triangles: &[[f32; 4]]) -> Self {
        Self::build_with_unsplittable(triangles, |_| false)
    }

    /// Like `build`, but triangles for which `unsplittable(triangle_index)` is true are never
    /// spatially split, so they are referenced by exactly one leaf. Traversal then reports each of
    /// them at most once, which any hit accumulation (transparent shadows) relies on.
    pub fn build_with_unsplittable(
        triangles: &[[f32; 4]],
        unsplittable: impl Fn(u32) -> bool,
    ) -> Self {
        assert_eq!(
            triangles.len() % 3,
            0,
            "vertex count must be a multiple of 3"
        );
        let tri_count = triangles.len() / 3;
        if tri_count == 0 {
            return Self {
                nodes: Vec::new(),
                triangles: Vec::new(),
            };
        }
        let (nodes, prim_idx) = build_sbvh(triangles, tri_count, &unsplittable);
        let (mut nodes, prim_idx) = compact(nodes, prim_idx);
        split_leafs(&mut nodes, MAX_LEAF_TRIS);
        let mut wide = collapse(&nodes);
        let (n, t) = encode_cwbvh(&mut wide, &prim_idx, triangles);
        Self {
            nodes: n,
            triangles: t,
        }
    }

    /// Number of 16-byte node blocks (what tinybvh calls `usedBlocks`).
    pub fn node_block_count(&self) -> usize {
        self.nodes.len()
    }
    /// Number of triangle references (>= input triangle count when spatial splits happened).
    pub fn triangle_ref_count(&self) -> usize {
        self.triangles.len() / 3
    }
    pub fn nodes_bytes(&self) -> &[u8] {
        blocks_as_bytes(&self.nodes)
    }
    pub fn triangles_bytes(&self) -> &[u8] {
        blocks_as_bytes(&self.triangles)
    }
}

fn blocks_as_bytes(b: &[[u32; 4]]) -> &[u8] {
    // SAFETY: [u32; 4] has no padding and u8 has alignment 1. Layout is little endian on all GPU targets.
    unsafe { std::slice::from_raw_parts(b.as_ptr() as *const u8, std::mem::size_of_val(b)) }
}

// ---------------------------------------------------------------------------
// stage 1: SBVH
// ---------------------------------------------------------------------------

/// Cut a fragment in two at `pos` on `axis`. Returns (left, right, ok).
fn split_frag(
    verts: &[[f32; 4]],
    orig: &Fragment,
    axis: usize,
    pos: f32,
) -> (Fragment, Fragment, bool) {
    let mut left = Fragment::empty(orig.prim, true);
    let mut right = left;
    let base = orig.prim as usize * 3;
    let v = [
        vert(verts, base),
        vert(verts, base + 1),
        vert(verts, base + 2),
    ];
    let l = [v[0][axis] <= pos, v[1][axis] <= pos, v[2][axis] <= pos];
    for k in 0..3 {
        if l[k] {
            left.extend(v[k])
        } else {
            right.extend(v[k])
        }
    }
    for k in 0..3 {
        let k2 = (k + 1) % 3;
        if l[k] ^ l[k2] {
            let (a, b) = (v[k], v[k2]);
            let t = (pos - a[axis]) / (b[axis] - a[axis]);
            let mut c = [
                a[0] + t * (b[0] - a[0]),
                a[1] + t * (b[1] - a[1]),
                a[2] + t * (b[2] - a[2]),
            ];
            c[axis] = pos;
            left.extend(c);
            right.extend(c);
        }
    }
    if orig.clipped {
        left.bmin = vmax(orig.bmin, left.bmin);
        left.bmax = vmin(orig.bmax, left.bmax);
        right.bmin = vmax(orig.bmin, right.bmin);
        right.bmax = vmin(orig.bmax, right.bmax);
    }
    let ok =
        halfarea(vsub(left.bmax, left.bmin)) > 0.0 && halfarea(vsub(right.bmax, right.bmin)) > 0.0;
    (left, right, ok)
}

/// Clip a fragment's triangle to the slab [bmin[axis], bmax[axis]] for spatial binning.
fn clip_frag(
    verts: &[[f32; 4]],
    orig: &Fragment,
    bmin: V3,
    bmax: V3,
    axis: usize,
) -> Option<Fragment> {
    let base = orig.prim as usize * 3;
    let v = [
        vert(verts, base),
        vert(verts, base + 1),
        vert(verts, base + 2),
    ];
    let (lo, hi) = (bmin[axis], bmax[axis]);
    let mut t1 = Fragment::empty(orig.prim, true);
    let mut t2 = t1;
    let edge = |a: V3, b: V3, pos: f32| -> V3 {
        let t = (pos - a[axis]) / (b[axis] - a[axis]);
        let mut c = [
            a[0] + t * (b[0] - a[0]),
            a[1] + t * (b[1] - a[1]),
            a[2] + t * (b[2] - a[2]),
        ];
        c[axis] = pos;
        c
    };
    let in_lo = [v[0][axis] >= lo, v[1][axis] >= lo, v[2][axis] >= lo];
    let in_hi = [v[0][axis] <= hi, v[1][axis] <= hi, v[2][axis] <= hi];
    for k in 0..3 {
        let k2 = (k + 1) % 3;
        if in_lo[k] {
            t1.extend(v[k]);
        }
        if in_lo[k] ^ in_lo[k2] {
            t1.extend(edge(v[k], v[k2], lo));
        }
        if in_hi[k] {
            t2.extend(v[k]);
        }
        if in_hi[k] ^ in_hi[k2] {
            t2.extend(edge(v[k], v[k2], hi));
        }
    }
    let mut nmin = vmax(t1.bmin, t2.bmin);
    let mut nmax = vmin(t1.bmax, t2.bmax);
    if orig.clipped {
        nmin = vmax(nmin, orig.bmin);
        nmax = vmin(nmax, orig.bmax);
    }
    if halfarea(vsub(nmax, nmin)) > 0.0 {
        Some(Fragment {
            bmin: nmin,
            bmax: nmax,
            prim: orig.prim,
            clipped: true,
        })
    } else {
        None
    }
}

/// Returns the node pool (node 1 unused, as in tinybvh) and the primitive index array.
/// Leaf `left_first`/`tri_count` index into the returned array, which holds *original* triangle
/// indices (a triangle may appear in several leaves when it was split).
/// `unsplittable` triangles are binned and partitioned whole by their centroid in spatial splits.
fn build_sbvh(
    verts: &[[f32; 4]],
    tri_count: usize,
    unsplittable: &dyn Fn(u32) -> bool,
) -> (Vec<Node>, Vec<u32>) {
    let slack = tri_count >> 1; // room for split references
    let total = tri_count + slack;
    let mut frags = vec![Fragment::default(); total];
    let mut prim_idx = vec![0u32; total];
    let mut idx_tmp = vec![0u32; total];
    let mut next_frag = tri_count;

    let mut root = Node {
        bmin: [BVH_FAR; 3],
        bmax: [-BVH_FAR; 3],
        left_first: 0,
        tri_count: tri_count as u32,
    };
    for i in 0..tri_count {
        let (v0, v1, v2) = (
            vert(verts, i * 3),
            vert(verts, i * 3 + 1),
            vert(verts, i * 3 + 2),
        );
        let bmin = vmin(v0, vmin(v1, v2));
        let bmax = vmax(v0, vmax(v1, v2));
        frags[i] = Fragment {
            bmin,
            bmax,
            prim: i as u32,
            clipped: false,
        };
        root.bmin = vmin(root.bmin, bmin);
        root.bmax = vmax(root.bmax, bmax);
        prim_idx[i] = i as u32;
    }
    let mut nodes: Vec<Node> = Vec::with_capacity(total * 2 + 2);
    nodes.push(root);
    nodes.push(Node::default()); // node 1 stays unused (cache line alignment in the original)

    let root_ext = vsub(root.bmax, root.bmin);
    let root_area = halfarea(root_ext);
    let min_dim = [root_ext[0] * 1e-7, root_ext[1] * 1e-7, root_ext[2] * 1e-7];

    struct Task {
        node: usize,
        start: usize,
        end: usize,
    }
    let mut stack: Vec<Task> = Vec::with_capacity(128);
    let (mut node_idx, mut slice_start, mut slice_end) = (0usize, 0usize, total);

    'outer: loop {
        loop {
            let node = nodes[node_idx];
            let first = node.left_first as usize;
            let count = node.tri_count as usize;
            let ext = vsub(node.bmax, node.bmin);

            // ---- object split: binning ----
            let mut bin_min = [[[BVH_FAR; 3]; BINS]; 3];
            let mut bin_max = [[[-BVH_FAR; 3]; BINS]; 3];
            let mut cnt = [[0usize; BINS]; 3];
            let rpd = [
                BINS as f32 / ext[0],
                BINS as f32 / ext[1],
                BINS as f32 / ext[2],
            ];
            for i in 0..count {
                let f = &frags[prim_idx[first + i] as usize];
                for a in 0..3 {
                    let c = ((f.bmin[a] + f.bmax[a]) * 0.5 - node.bmin[a]) * rpd[a];
                    let bi = (c as i32).clamp(0, BINS as i32 - 1) as usize;
                    bin_min[a][bi] = vmin(bin_min[a][bi], f.bmin);
                    bin_max[a][bi] = vmax(bin_max[a][bi], f.bmax);
                    cnt[a][bi] += 1;
                }
            }

            // ---- object split: sweep ----
            let no_split = count as f32 * C_INT;
            let mut split_cost = no_split;
            let r_sav = 1.0 / node.sa();
            let (mut best_axis, mut best_pos) = (0usize, 0usize);
            let (mut b_lmin, mut b_lmax, mut b_rmin, mut b_rmax) =
                ([0f32; 3], [0f32; 3], [0f32; 3], [0f32; 3]);
            for a in 0..3 {
                if !(ext[a] > min_dim[a]) {
                    continue;
                }
                let (mut l_bmin, mut l_bmax) = ([[0f32; 3]; BINS - 1], [[0f32; 3]; BINS - 1]);
                let (mut r_bmin, mut r_bmax) = ([[0f32; 3]; BINS - 1], [[0f32; 3]; BINS - 1]);
                let (mut al, mut ar) = ([0f32; BINS - 1], [0f32; BINS - 1]);
                let (mut nl, mut nr) = ([0usize; BINS - 1], [0usize; BINS - 1]);
                let (mut l1, mut l2, mut r1, mut r2) =
                    ([BVH_FAR; 3], [-BVH_FAR; 3], [BVH_FAR; 3], [-BVH_FAR; 3]);
                let (mut ln, mut rn) = (0usize, 0usize);
                for i in 0..BINS - 1 {
                    l1 = vmin(l1, bin_min[a][i]);
                    l_bmin[i] = l1;
                    r1 = vmin(r1, bin_min[a][BINS - 1 - i]);
                    r_bmin[BINS - 2 - i] = r1;
                    l2 = vmax(l2, bin_max[a][i]);
                    l_bmax[i] = l2;
                    r2 = vmax(r2, bin_max[a][BINS - 1 - i]);
                    r_bmax[BINS - 2 - i] = r2;
                    ln += cnt[a][i];
                    rn += cnt[a][BINS - 1 - i];
                    nl[i] = ln;
                    nr[BINS - 2 - i] = rn;
                    al[i] = if ln == 0 {
                        BVH_FAR
                    } else {
                        halfarea(vsub(l2, l1))
                    };
                    ar[BINS - 2 - i] = if rn == 0 {
                        BVH_FAR
                    } else {
                        halfarea(vsub(r2, r1))
                    };
                }
                for i in 0..BINS - 1 {
                    let c = split_cost_sah(r_sav, al[i], nl[i], ar[i], nr[i]);
                    if c >= split_cost {
                        continue;
                    }
                    split_cost = c;
                    best_axis = a;
                    best_pos = i;
                    b_lmin = l_bmin[i];
                    b_rmin = r_bmin[i];
                    b_lmax = l_bmax[i];
                    b_rmax = r_bmax[i];
                }
            }

            // ---- spatial split candidates ----
            let mut spatial = false;
            let (mut best_nl, mut best_nr) = (0usize, 0usize);
            let budget = slice_end - slice_start;
            let spatial_overlap = halfarea(vsub(b_lmax, b_rmin)) / root_area;
            if budget > count && (spatial_overlap > 1e-4 || split_cost >= no_split) {
                let mut min_split_cost = split_cost * 0.985; // don't accept a spatial split for minimal gain
                for a in 0..3 {
                    if !(ext[a] > min_dim[a]) {
                        continue;
                    }
                    let mut sbin_min = [[BVH_FAR; 3]; BINS];
                    let mut sbin_max = [[-BVH_FAR; 3]; BINS];
                    let mut count_in = [0usize; BINS];
                    let mut count_out = [0usize; BINS];
                    let plane_dist = ext[a] / (BINS as f32 * 0.9999);
                    let r_plane = 1.0 / plane_dist;
                    let node_min = node.bmin[a];
                    for i in 0..count {
                        let f = frags[prim_idx[first + i] as usize];
                        if unsplittable(f.prim) {
                            let center = (f.bmin[a] + f.bmax[a]) * 0.5;
                            let bin = (((center - node_min) * r_plane) as i32)
                                .clamp(0, BINS as i32 - 1) as usize;
                            count_in[bin] += 1;
                            count_out[bin] += 1;
                            sbin_min[bin] = vmin(sbin_min[bin], f.bmin);
                            sbin_max[bin] = vmax(sbin_max[bin], f.bmax);
                            continue;
                        }
                        let bin1 = (((f.bmin[a] - node_min) * r_plane) as i32)
                            .clamp(0, BINS as i32 - 1) as usize;
                        let bin2 = (((f.bmax[a] - node_min) * r_plane) as i32)
                            .clamp(0, BINS as i32 - 1) as usize;
                        count_in[bin1] += 1;
                        count_out[bin2] += 1;
                        if bin1 == bin2 {
                            sbin_min[bin1] = vmin(sbin_min[bin1], f.bmin);
                            sbin_max[bin1] = vmax(sbin_max[bin1], f.bmax);
                        } else {
                            for j in bin1..=bin2 {
                                let (mut cmin, mut cmax) = (node.bmin, node.bmax);
                                cmin[a] = node_min + plane_dist * j as f32;
                                cmax[a] = if j == BINS - 2 {
                                    node.bmax[a]
                                } else {
                                    cmin[a] + plane_dist
                                };
                                if let Some(t) = clip_frag(verts, &f, cmin, cmax, a) {
                                    sbin_min[j] = vmin(sbin_min[j], t.bmin);
                                    sbin_max[j] = vmax(sbin_max[j], t.bmax);
                                }
                            }
                        }
                    }
                    let (mut l_bmin, mut l_bmax) = ([[0f32; 3]; BINS - 1], [[0f32; 3]; BINS - 1]);
                    let (mut r_bmin, mut r_bmax) = ([[0f32; 3]; BINS - 1], [[0f32; 3]; BINS - 1]);
                    let (mut al, mut ar) = ([0f32; BINS - 1], [0f32; BINS - 1]);
                    let (mut nl, mut nr) = ([0usize; BINS - 1], [0usize; BINS - 1]);
                    let (mut l1, mut l2, mut r1, mut r2) =
                        ([BVH_FAR; 3], [-BVH_FAR; 3], [BVH_FAR; 3], [-BVH_FAR; 3]);
                    let (mut ln, mut rn) = (0usize, 0usize);
                    for i in 0..BINS - 1 {
                        l1 = vmin(l1, sbin_min[i]);
                        l_bmin[i] = l1;
                        r1 = vmin(r1, sbin_min[BINS - 1 - i]);
                        r_bmin[BINS - 2 - i] = r1;
                        l2 = vmax(l2, sbin_max[i]);
                        l_bmax[i] = l2;
                        r2 = vmax(r2, sbin_max[BINS - 1 - i]);
                        r_bmax[BINS - 2 - i] = r2;
                        ln += count_in[i];
                        rn += count_out[BINS - 1 - i];
                        al[i] = if ln == 0 {
                            BVH_FAR
                        } else {
                            halfarea(vsub(l2, l1))
                        };
                        ar[BINS - 2 - i] = if rn == 0 {
                            BVH_FAR
                        } else {
                            halfarea(vsub(r2, r1))
                        };
                        nl[i] = ln;
                        nr[BINS - 2 - i] = rn;
                    }
                    for i in 0..BINS - 1 {
                        let c = split_cost_sah(r_sav, al[i], nl[i], ar[i], nr[i]);
                        if c < min_split_cost && nl[i] + nr[i] < budget && nl[i] > 0 && nr[i] > 0 {
                            spatial = true;
                            min_split_cost = c;
                            split_cost = c;
                            best_axis = a;
                            best_pos = i;
                            b_lmin = l_bmin[i];
                            b_lmax = l_bmax[i];
                            b_rmin = r_bmin[i];
                            b_rmax = r_bmax[i];
                            best_nl = nl[i];
                            best_nr = nr[i];
                            b_lmax[a] = b_rmin[a]; // accurate
                        }
                    }
                }
            }

            // ---- no split wins: finalize leaf ----
            if split_cost >= no_split {
                for i in 0..count {
                    let p = prim_idx[first + i] as usize;
                    prim_idx[first + i] = frags[p].prim;
                }
                break;
            }

            // ---- partition (double buffered through idx_tmp) ----
            let mut a_ptr = slice_start;
            let mut b_ptr = slice_end;
            if spatial {
                let plane_dist = ext[best_axis] / (BINS as f32 * 0.9999);
                let r_plane = 1.0 / plane_dist;
                let node_min = node.bmin[best_axis];
                for i in 0..count {
                    let frag_idx = prim_idx[first + i] as usize;
                    let f = frags[frag_idx];
                    if unsplittable(f.prim) {
                        // same side as in spatial binning
                        let center = (f.bmin[best_axis] + f.bmax[best_axis]) * 0.5;
                        let bin = (((center - node_min) * r_plane) as i32)
                            .clamp(0, BINS as i32 - 1) as usize;
                        if bin <= best_pos {
                            idx_tmp[a_ptr] = frag_idx as u32;
                            a_ptr += 1;
                        } else {
                            b_ptr -= 1;
                            idx_tmp[b_ptr] = frag_idx as u32;
                        }
                        continue;
                    }
                    let bin1 = fmax((f.bmin[best_axis] - node_min) * r_plane, 0.0) as u32 as usize;
                    let bin2 = fmax((f.bmax[best_axis] - node_min) * r_plane, 0.0) as u32 as usize;
                    if bin2 <= best_pos {
                        idx_tmp[a_ptr] = frag_idx as u32;
                        a_ptr += 1;
                    } else if bin1 > best_pos {
                        b_ptr -= 1;
                        idx_tmp[b_ptr] = frag_idx as u32;
                    } else {
                        // unsplitting: would adding the whole primitive to one side be cheaper?
                        if best_nr > 1 {
                            let umin = vmin(b_lmin, f.bmin);
                            let umax = vmax(b_lmax, f.bmax);
                            let al = halfarea(vsub(umax, umin));
                            let ar = halfarea(vsub(b_rmax, b_rmin));
                            let c = split_cost_sah(r_sav, al, best_nl, ar, best_nr - 1);
                            if c <= split_cost {
                                best_nr -= 1;
                                split_cost = c;
                                idx_tmp[a_ptr] = frag_idx as u32;
                                a_ptr += 1;
                                b_lmin = umin;
                                b_lmax = umax;
                                continue;
                            }
                        }
                        if best_nl > 1 {
                            let umin = vmin(b_rmin, f.bmin);
                            let umax = vmax(b_rmax, f.bmax);
                            let al = halfarea(vsub(b_lmax, b_lmin));
                            let ar = halfarea(vsub(umax, umin));
                            let c = split_cost_sah(r_sav, al, best_nl - 1, ar, best_nr);
                            if c <= split_cost {
                                best_nl -= 1;
                                split_cost = c;
                                b_ptr -= 1;
                                idx_tmp[b_ptr] = frag_idx as u32;
                                b_rmin = umin;
                                b_rmax = umax;
                                continue;
                            }
                        }
                        // split the straddler
                        let split_pos = b_lmax[best_axis];
                        let (p1, p2, ok) = split_frag(verts, &f, best_axis, split_pos);
                        if ok {
                            let new_idx = next_frag;
                            next_frag += 1;
                            frags[frag_idx] = p1;
                            idx_tmp[a_ptr] = frag_idx as u32;
                            a_ptr += 1;
                            frags[new_idx] = p2;
                            b_ptr -= 1;
                            idx_tmp[b_ptr] = new_idx as u32;
                        } else if halfarea(vsub(p1.bmax, p1.bmin)) > 0.0 {
                            idx_tmp[a_ptr] = frag_idx as u32;
                            a_ptr += 1;
                        } else {
                            b_ptr -= 1;
                            idx_tmp[b_ptr] = frag_idx as u32;
                        }
                    }
                }
                // clipping is never fully stable: refresh child bounds from the fragments
                b_lmin = [BVH_FAR; 3];
                b_rmin = [BVH_FAR; 3];
                b_lmax = [-BVH_FAR; 3];
                b_rmax = [-BVH_FAR; 3];
                for &fi in &idx_tmp[slice_start..a_ptr] {
                    b_lmin = vmin(b_lmin, frags[fi as usize].bmin);
                    b_lmax = vmax(b_lmax, frags[fi as usize].bmax);
                }
                for &fi in &idx_tmp[b_ptr..slice_end] {
                    b_rmin = vmin(b_rmin, frags[fi as usize].bmin);
                    b_rmax = vmax(b_rmax, frags[fi as usize].bmax);
                }
            } else {
                let (rp, nm) = (rpd[best_axis], node.bmin[best_axis]);
                for i in 0..count {
                    let fr = prim_idx[first + i];
                    let f = &frags[fr as usize];
                    let c = ((f.bmin[best_axis] + f.bmax[best_axis]) * 0.5 - nm) * rp;
                    let bi = (c as i32).clamp(0, BINS as i32 - 1) as usize;
                    if bi <= best_pos {
                        idx_tmp[a_ptr] = fr;
                        a_ptr += 1;
                    } else {
                        b_ptr -= 1;
                        idx_tmp[b_ptr] = fr;
                    }
                }
            }

            let left_count = a_ptr - slice_start;
            let right_count = slice_end - b_ptr;
            if left_count == 0 || right_count == 0 {
                // the split did not work out: make this a leaf. (prim_idx has not been overwritten
                // yet, so the leaf still sees its original references.)
                for i in 0..count {
                    let p = prim_idx[first + i] as usize;
                    prim_idx[first + i] = frags[p].prim;
                }
                nodes[node_idx].bmin = vmin(b_lmin, b_rmin);
                nodes[node_idx].bmax = vmax(b_lmax, b_rmax);
                break;
            }

            // copy back only the two live ranges; the gap between them is dead data
            prim_idx[slice_start..a_ptr].copy_from_slice(&idx_tmp[slice_start..a_ptr]);
            prim_idx[b_ptr..slice_end].copy_from_slice(&idx_tmp[b_ptr..slice_end]);

            let left_child = nodes.len();
            nodes.push(Node {
                bmin: b_lmin,
                bmax: b_lmax,
                left_first: slice_start as u32,
                tri_count: left_count as u32,
            });
            nodes.push(Node {
                bmin: b_rmin,
                bmax: b_rmax,
                left_first: b_ptr as u32,
                tri_count: right_count as u32,
            });
            nodes[node_idx].left_first = left_child as u32;
            nodes[node_idx].tri_count = 0;

            let mid = (a_ptr + b_ptr) >> 1;
            stack.push(Task {
                node: left_child + 1,
                start: mid,
                end: slice_end,
            });
            node_idx = left_child;
            slice_end = mid;
        }
        match stack.pop() {
            None => break 'outer,
            Some(t) => {
                node_idx = t.node;
                slice_start = t.start;
                slice_end = t.end;
            }
        }
    }
    (nodes, prim_idx)
}

// ---------------------------------------------------------------------------
// stage 2: compact
// ---------------------------------------------------------------------------

/// Rebuild the node pool in depth-first order (children adjacent) and the index array in leaf order.
fn compact(old: Vec<Node>, prim_idx: Vec<u32>) -> (Vec<Node>, Vec<u32>) {
    if old[0].is_leaf() {
        let n = old[0].tri_count as usize;
        return (old, prim_idx[..n].to_vec());
    }
    let mut tmp = vec![Node::default(); old.len()];
    tmp[0] = old[0];
    tmp[1] = old[1];
    let mut idx: Vec<u32> = Vec::with_capacity(prim_idx.len());
    let mut new_ptr = 2usize;
    let mut node_idx = 0usize;
    let mut stack: Vec<usize> = Vec::with_capacity(128);
    loop {
        let node = tmp[node_idx];
        if node.is_leaf() {
            tmp[node_idx].left_first = idx.len() as u32;
            let f = node.left_first as usize;
            idx.extend_from_slice(&prim_idx[f..f + node.tri_count as usize]);
            match stack.pop() {
                None => break,
                Some(n) => node_idx = n,
            }
        } else {
            let l = node.left_first as usize;
            tmp[new_ptr] = old[l];
            tmp[new_ptr + 1] = old[l + 1];
            tmp[node_idx].left_first = new_ptr as u32;
            stack.push(new_ptr + 1);
            node_idx = new_ptr;
            new_ptr += 2;
        }
    }
    tmp.truncate(new_ptr);
    (tmp, idx)
}

// ---------------------------------------------------------------------------
// stage 3: split leaves
// ---------------------------------------------------------------------------

/// Split every leaf with more than `max_prims` references into a chain of smaller leaves.
/// Children inherit the parent's bounds (same as tinybvh).
fn split_leafs(nodes: &mut Vec<Node>, max_prims: u32) {
    let mut stack: Vec<u32> = Vec::with_capacity(64);
    let mut node_idx = 0usize;
    loop {
        let node = nodes[node_idx];
        if node.is_leaf() {
            if node.tri_count > max_prims {
                let n = nodes.len();
                let mut left = node;
                let mut right = node;
                right.left_first = node.left_first + max_prims;
                right.tri_count = node.tri_count - max_prims;
                left.tri_count = max_prims;
                nodes.push(left);
                nodes.push(right);
                nodes[node_idx].left_first = n as u32;
                nodes[node_idx].tri_count = 0;
            } else {
                match stack.pop() {
                    None => break,
                    Some(i) => node_idx = i as usize,
                }
            }
        } else {
            node_idx = node.left_first as usize;
            stack.push(node.left_first + 1);
        }
    }
}

// ---------------------------------------------------------------------------
// stage 4: collapse binary -> 8-wide
// ---------------------------------------------------------------------------

fn collapse(nodes: &[Node]) -> Vec<WideNode> {
    let mut wide = vec![WideNode::default(); nodes.len()];
    for (i, n) in nodes.iter().enumerate() {
        if i == 1 {
            continue;
        }
        let w = &mut wide[i];
        w.bmin = n.bmin;
        w.bmax = n.bmax;
        if n.is_leaf() {
            w.tri_count = n.tri_count;
            w.first_tri = n.left_first;
        } else {
            w.child[0] = n.left_first;
            w.child[1] = n.left_first + 1;
            w.child_count = 2;
        }
    }
    let mut stack: Vec<usize> = Vec::with_capacity(128);
    let mut node_idx = 0usize;
    loop {
        if wide[node_idx].tri_count == 0 {
            // adopt grandchildren, largest child first, while there is room
            while wide[node_idx].child_count < 8 {
                let node = wide[node_idx];
                let mut best: Option<usize> = None;
                let mut best_sa = 0.0f32;
                for i in 0..node.child_count as usize {
                    let c = &wide[node.child[i] as usize];
                    if c.tri_count == 0
                        && node.child_count as usize - 1 + c.child_count as usize <= 8
                    {
                        let sa = c.sa();
                        if sa > best_sa {
                            best = Some(i);
                            best_sa = sa;
                        }
                    }
                }
                let Some(b) = best else { break };
                let c = wide[node.child[b] as usize];
                let n = &mut wide[node_idx];
                n.child[b] = c.child[0];
                for k in 1..c.child_count as usize {
                    n.child[n.child_count as usize] = c.child[k];
                    n.child_count += 1;
                }
            }
            let node = wide[node_idx];
            for i in 0..node.child_count as usize {
                if wide[node.child[i] as usize].tri_count == 0 {
                    stack.push(node.child[i] as usize);
                }
            }
        }
        match stack.pop() {
            None => break,
            Some(n) => node_idx = n,
        }
    }
    // a root leaf needs an extra level: the encoder only handles interior nodes
    if wide[0].tri_count > 0 {
        wide[1] = wide[0];
        wide[0].child_count = 1;
        wide[0].child[0] = 1;
        wide[0].tri_count = 0;
    }
    wide
}

// ---------------------------------------------------------------------------
// stage 5: CWBVH encoding
// ---------------------------------------------------------------------------

/// Shared exponent such that `extent / 2^e <= 255`. tinybvh uses `ceil(log2(extent / 255))`; this
/// additionally guards rounding (which could push the top quantized value to 256 and wrap),
/// degenerate extents and the representable exponent range.
fn quant_exp(extent: f32) -> i32 {
    if !(extent > 0.0) || !extent.is_finite() {
        return 0;
    }
    let mut e = ((extent / 255.0).log2().ceil() as i32).clamp(-126, 127);
    while e < 127 && extent > 255.0 * pow2(e) {
        e += 1;
    }
    e
}

fn encode_cwbvh(
    wide: &mut [WideNode],
    prim_idx: &[u32],
    verts: &[[f32; 4]],
) -> (Vec<[u32; 4]>, Vec<[u32; 4]>) {
    let mut nodes: Vec<[u32; 4]> = vec![[0; 4]; 5];
    let mut tris: Vec<[u32; 4]> = Vec::with_capacity(prim_idx.len() * 3);
    let mut stack: Vec<(usize, usize)> = Vec::with_capacity(256); // (wide node, block address)
    stack.push((0, 0));

    while let Some((wi, cur)) = stack.pop() {
        let orig = wide[wi];
        let (lo, hi) = (orig.bmin, orig.bmax);

        // greedy child -> slot assignment: slot s prefers children in the direction of octant s
        let centroid = [
            (lo[0] + hi[0]) * 0.5,
            (lo[1] + hi[1]) * 0.5,
            (lo[2] + hi[2]) * 0.5,
        ];
        let mut cost = [[BVH_FAR; 8]; 8];
        for s in 0..8 {
            let ds = [
                if (s >> 2) & 1 == 1 { -1.0f32 } else { 1.0 },
                if (s >> 1) & 1 == 1 { -1.0f32 } else { 1.0 },
                if s & 1 == 1 { -1.0f32 } else { 1.0 },
            ];
            for i in 0..8 {
                if orig.child[i] != 0 {
                    let c = &wide[orig.child[i] as usize];
                    let cc = [
                        (c.bmin[0] + c.bmax[0]) * 0.5,
                        (c.bmin[1] + c.bmax[1]) * 0.5,
                        (c.bmin[2] + c.bmax[2]) * 0.5,
                    ];
                    cost[s][i] = (cc[0] - centroid[0]) * ds[0]
                        + (cc[1] - centroid[1]) * ds[1]
                        + (cc[2] - centroid[2]) * ds[2];
                }
            }
        }
        let mut assignment = [usize::MAX; 8];
        let mut slot_empty = [true; 8];
        loop {
            let mut min_cost = BVH_FAR;
            let mut best: Option<(usize, usize)> = None;
            for s in 0..8 {
                for i in 0..8 {
                    if assignment[i] == usize::MAX && slot_empty[s] && cost[s][i] < min_cost {
                        min_cost = cost[s][i];
                        best = Some((s, i));
                    }
                }
            }
            let Some((s, i)) = best else { break };
            slot_empty[s] = false;
            assignment[i] = s;
        }
        for i in 0..8 {
            if assignment[i] == usize::MAX {
                for s in 0..8 {
                    if slot_empty[s] {
                        slot_empty[s] = false;
                        assignment[i] = s;
                        break;
                    }
                }
            }
        }
        let mut child = [0u32; 8];
        for i in 0..8 {
            child[assignment[i]] = orig.child[i];
        }

        // quantization frame
        let e = [
            quant_exp(hi[0] - lo[0]),
            quant_exp(hi[1] - lo[1]),
            quant_exp(hi[2] - lo[2]),
        ];
        let scale = [pow2(e[0]), pow2(e[1]), pow2(e[2])];

        let mut nb = [0u8; 80];
        let mut internal_count = 0u32;
        let mut leaf_tri_count = 0u32;
        let mut child_base = 0u32;
        let mut tri_base = 0u32;
        let mut imask = 0u8;

        for i in 0..8 {
            if child[i] == 0 {
                continue;
            }
            let c = wide[child[i] as usize];
            for a in 0..3 {
                let qlo = ((c.bmin[a] - lo[a]) / scale[a]).floor().clamp(0.0, 255.0) as u8;
                let qhi = ((c.bmax[a] - lo[a]) / scale[a]).ceil().clamp(0.0, 255.0) as u8;
                nb[32 + a * 8 + i] = qlo;
                nb[56 + a * 8 + i] = qhi;
            }
            if c.tri_count == 0 {
                // interior: children of one parent get consecutive node slots
                let addr = nodes.len();
                if internal_count == 0 {
                    child_base = (addr / 5) as u32;
                }
                internal_count += 1;
                nodes.extend_from_slice(&[[0u32; 4]; 5]);
                imask |= 1 << i;
                nb[24 + i] = (1 << 5) | (24 + i as u8);
                stack.push((child[i] as usize, addr));
            } else {
                let tcount = c.tri_count; // <= 3
                if leaf_tri_count == 0 {
                    tri_base = tris.len() as u32;
                }
                let unary: u8 = match tcount {
                    1 => 0b001,
                    2 => 0b011,
                    _ => 0b111,
                };
                nb[24 + i] = (unary << 5) | leaf_tri_count as u8;
                leaf_tri_count += tcount;
                for j in 0..tcount {
                    let tri_idx = prim_idx[(c.first_tri + j) as usize];
                    let b = tri_idx as usize * 3;
                    let v0 = verts[b];
                    let (v1, v2) = (verts[b + 1], verts[b + 2]);
                    tris.push([
                        (v2[0] - v0[0]).to_bits(),
                        (v2[1] - v0[1]).to_bits(),
                        (v2[2] - v0[2]).to_bits(),
                        v2[3].to_bits(),
                    ]);
                    tris.push([
                        (v1[0] - v0[0]).to_bits(),
                        (v1[1] - v0[1]).to_bits(),
                        (v1[2] - v0[2]).to_bits(),
                        v1[3].to_bits(),
                    ]);
                    tris.push([v0[0].to_bits(), v0[1].to_bits(), v0[2].to_bits(), tri_idx]);
                }
            }
        }

        nb[0..4].copy_from_slice(&lo[0].to_le_bytes());
        nb[4..8].copy_from_slice(&lo[1].to_le_bytes());
        nb[8..12].copy_from_slice(&lo[2].to_le_bytes());
        nb[12] = e[0] as i8 as u8;
        nb[13] = e[1] as i8 as u8;
        nb[14] = e[2] as i8 as u8;
        nb[15] = imask;
        nb[16..20].copy_from_slice(&child_base.to_le_bytes());
        nb[20..24].copy_from_slice(&tri_base.to_le_bytes());

        for b in 0..5 {
            for k in 0..4 {
                let o = b * 16 + k * 4;
                nodes[cur + b][k] = u32::from_le_bytes([nb[o], nb[o + 1], nb[o + 2], nb[o + 3]]);
            }
        }
    }
    (nodes, tris)
}

// ---------------------------------------------------------------------------
// tests: structural validation of the encoded layout
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> f32 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 40) as f32) / (1u64 << 24) as f32
        }
    }

    fn walk(
        b: &Cwbvh,
        node: usize,
        seen: &mut Vec<u32>,
        bounds: Option<(V3, V3)>,
        leaf_refs: &mut usize,
    ) {
        let w = |blk: usize, k: usize| b.nodes[node * 5 + blk][k];
        let lo = [
            f32::from_bits(w(0, 0)),
            f32::from_bits(w(0, 1)),
            f32::from_bits(w(0, 2)),
        ];
        let eb = w(0, 3).to_le_bytes();
        let e = [eb[0] as i8 as i32, eb[1] as i8 as i32, eb[2] as i8 as i32];
        let imask = eb[3];
        let child_base = w(1, 0) as usize;
        let tri_base = w(1, 1) as usize;
        let mut bytes = [0u8; 80];
        for blk in 0..5 {
            for k in 0..4 {
                bytes[blk * 16 + k * 4..][..4].copy_from_slice(&w(blk, k).to_le_bytes());
            }
        }
        let _ = bounds;
        for i in 0..8 {
            let meta = bytes[24 + i];
            if meta == 0 {
                continue;
            }
            let mut cmin = [0f32; 3];
            let mut cmax = [0f32; 3];
            for a in 0..3 {
                cmin[a] = lo[a] + bytes[32 + a * 8 + i] as f32 * pow2(e[a]);
                cmax[a] = lo[a] + bytes[56 + a * 8 + i] as f32 * pow2(e[a]);
            }
            if imask & (1 << i) != 0 {
                let rel = (imask & ((1u8 << i) - 1)).count_ones() as usize;
                walk(b, child_base + rel, seen, Some((cmin, cmax)), leaf_refs);
            } else {
                let n = (meta >> 5).count_ones() as usize;
                let off = (meta & 31) as usize;
                for t in 0..n {
                    let base = tri_base + (off + t) * 3;
                    let v0 = b.triangles[base + 2];
                    let p = [
                        f32::from_bits(v0[0]),
                        f32::from_bits(v0[1]),
                        f32::from_bits(v0[2]),
                    ];
                    let e1 = b.triangles[base + 1];
                    let e2 = b.triangles[base];
                    for k in 0..3 {
                        let pts = [
                            p[k],
                            p[k] + f32::from_bits(e1[k]),
                            p[k] + f32::from_bits(e2[k]),
                        ];
                        // at least the triangle's bounds must overlap the child box (clipped refs are partial)
                        let tmin = pts.iter().cloned().fold(f32::MAX, f32::min);
                        let tmax = pts.iter().cloned().fold(f32::MIN, f32::max);
                        assert!(
                            tmax >= cmin[k] - 1e-3 && tmin <= cmax[k] + 1e-3,
                            "tri outside child box"
                        );
                    }
                    seen[v0[3] as usize] += 1;
                    *leaf_refs += 1;
                }
            }
        }
    }

    fn random_triangles(n: usize) -> Vec<[f32; 4]> {
        let mut r = Lcg(12345);
        let mut verts = Vec::new();
        for _ in 0..n {
            let c = [r.next() * 100.0, r.next() * 100.0, r.next() * 100.0];
            let s = 0.2 + r.next() * 6.0; // some long thin ones to trigger spatial splits
            for _ in 0..3 {
                verts.push([
                    c[0] + r.next() * s * 4.0,
                    c[1] + r.next() * s * 0.2,
                    c[2] + r.next() * s,
                    0.0,
                ]);
            }
        }
        verts
    }

    /// Returns the number of references to each triangle.
    fn check(b: &Cwbvh, n: usize) -> Vec<u32> {
        assert_eq!(b.nodes.len() % 5, 0);
        assert!(b.triangle_ref_count() >= n);
        let mut seen = vec![0u32; n];
        let mut refs = 0;
        walk(b, 0, &mut seen, None, &mut refs);
        assert_eq!(refs, b.triangle_ref_count());
        assert!(seen.iter().all(|&s| s > 0), "missing triangles");
        seen
    }

    fn run(n: usize) {
        let verts = random_triangles(n);
        let b = Cwbvh::build(&verts);
        check(&b, n);
    }

    #[test]
    fn unsplittable_single_reference() {
        let n = 20_000;
        let verts = random_triangles(n);

        // the scene must actually produce spatial splits for this test to mean anything
        let splits = Cwbvh::build(&verts);
        assert!(splits.triangle_ref_count() > n, "no spatial splits");

        let b = Cwbvh::build_with_unsplittable(&verts, |prim| prim % 2 == 0);
        let seen = check(&b, n);
        for (prim, &count) in seen.iter().enumerate() {
            if prim % 2 == 0 {
                assert_eq!(count, 1, "unsplittable triangle {prim} referenced {count} times");
            }
        }
        assert!(b.triangle_ref_count() > n, "splittable triangles should still split");
    }

    #[test]
    fn tiny() {
        run(1);
        run(2);
        run(3);
        run(4);
        run(9);
    }
    #[test]
    fn medium() {
        run(1000);
    }
    #[test]
    fn large() {
        run(100_000);
    }
}
