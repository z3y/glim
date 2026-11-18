use std::f32::{self};

use glam::Vec3;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SHProbeL2 {
    pub position: Vec3,
    pub radius: f32,

    // L0
    pub l0: Vec3,
    pub occlusion0: f32, // light probe occlusion, shadowmask channel 0

    // L1
    pub l1_1: Vec3,
    pub occlusion1: f32,
    pub l10: Vec3,
    pub occlusion2: f32,
    pub l11: Vec3,
    pub occlusion3: f32,

    // L2
    pub l2_2: Vec3,
    pub occlusion_light0: i32, // light index for shadowmask channel 0, -1 = none
    pub l2_1: Vec3,
    pub occlusion_light1: i32,
    pub l20: Vec3,
    pub occlusion_light2: i32,
    pub l21: Vec3,
    pub occlusion_light3: i32,
    pub l22: Vec3,
    pub pad9: u32,
}

impl Default for SHProbeL2 {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            radius: 0.0,
            l0: Vec3::ZERO,
            occlusion0: 0.0,
            l1_1: Vec3::ZERO,
            occlusion1: 0.0,
            l10: Vec3::ZERO,
            occlusion2: 0.0,
            l11: Vec3::ZERO,
            occlusion3: 0.0,
            l2_2: Vec3::ZERO,
            occlusion_light0: -1,
            l2_1: Vec3::ZERO,
            occlusion_light1: -1,
            l20: Vec3::ZERO,
            occlusion_light2: -1,
            l21: Vec3::ZERO,
            occlusion_light3: -1,
            l22: Vec3::ZERO,
            pad9: 0,
        }
    }
}
