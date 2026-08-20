#![allow(non_camel_case_types)]

#[repr(C)]
pub struct BVHHandle {
    _private: [u8; 0],
}

unsafe extern "C" {
    pub fn bvh_build(triangles: *const f32, tri_count: u32) -> *mut BVHHandle;
    pub fn bvh_get_nodes_size(h: *mut BVHHandle) -> u32;
    pub fn bvh_get_nodes_count(h: *mut BVHHandle) -> u32;
    pub fn bvh_get_triangles_size(h: *mut BVHHandle) -> u32;
    pub fn bvh_get_triangles_count(h: *mut BVHHandle) -> u32;
    pub fn bvh_copy_nodes(h: *mut BVHHandle, dst: *mut std::ffi::c_void);
    pub fn bvh_copy_triangles(h: *mut BVHHandle, dst: *mut std::ffi::c_void);
    pub fn bvh_free(h: *mut BVHHandle);
}

pub struct Cwbvh {
    handle: *mut BVHHandle,
}

pub struct CwbvhData {
    pub nodes: Vec<u8>,
    pub triangles: Vec<u8>,
    pub node_count: u32,
    pub triangle_count: u32,
}

impl Cwbvh {
    pub fn build(triangles: &[[f32; 4]]) -> Self {
        assert_eq!(triangles.len() % 3, 0);
        let tri_count = (triangles.len() / 3) as u32;
        let handle = unsafe { bvh_build(triangles.as_ptr() as *const f32, tri_count) };
        Self { handle }
    }

    pub fn extract(&self) -> CwbvhData {
        unsafe {
            let nodes_size = bvh_get_nodes_size(self.handle) as usize;
            let tris_size = bvh_get_triangles_size(self.handle) as usize;

            let mut nodes = vec![0u8; nodes_size];
            let mut triangles = vec![0u8; tris_size];

            bvh_copy_nodes(self.handle, nodes.as_mut_ptr() as *mut _);
            bvh_copy_triangles(self.handle, triangles.as_mut_ptr() as *mut _);

            CwbvhData {
                nodes,
                triangles,
                node_count: bvh_get_nodes_count(self.handle),
                triangle_count: bvh_get_triangles_count(self.handle),
            }
        }
    }

    pub fn free(&mut self) {
        unsafe { bvh_free(self.handle) };
    }
}
