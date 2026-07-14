// use std::slice;

// use tinybvh_rs::{bvh, cwbvh, mbvh};

// #[derive(Debug)]
// #[repr(C)]
// pub struct BVHTriangle {
//     v0: [f32; 4],
//     v1: [f32; 4],
//     v2: [f32; 4],
// }

// pub fn build_bvh(triangles: &[BVHTriangle]) {
//     let cast = triangles.as_ptr() as *const f32;

//     let cast = unsafe { slice::from_raw_parts(cast, triangles.len() * 12) };

//     // println!("{:#?}", &triangles);
//     // println!("{:#?}", &cast);

//     let mut bvh = bvh::BVH::new(cast).unwrap();
//     bvh.split_leaves(3);

//     let mbvh = mbvh::BVH::new(&bvh);
//     let cwbvh = cwbvh::BVH::new(&mbvh).unwrap();
// }

// #[test]
// fn test() {
//     let tri = BVHTriangle {
//         v0: [0.0, 2.0, 3.0, 0.0],
//         v1: [0.0, 2.0, 3.0, 0.0],
//         v2: [0.0, 2.0, 3.0, 0.0],
//     };

//     let tri2 = BVHTriangle {
//         v0: [0.1, 2.0, 3.0, 0.0],
//         v1: [0.1, 2.0, 3.0, 0.0],
//         v2: [0.1, 2.0, 3.0, 0.0],
//     };

//     let tri3 = BVHTriangle {
//         v0: [0.2, 2.0, 3.0, 0.0],
//         v1: [0.2, 2.0, 3.0, 0.0],
//         v2: [0.3, 2.0, 3.0, 0.0],
//     };

//     let tris = [tri, tri2, tri3];

//     let a = build_bvh(&tris);
// }
