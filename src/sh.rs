use std::f32::{self};

use crate::math::Vector3;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SHProbeL2 {
    pub position: Vector3,
    pub radius: f32,

    // L0
    pub l0: Vector3,
    pub pad1: u32,

    // L1
    pub l1_1: Vector3,
    pub pad2: u32,
    pub l10: Vector3,
    pub pad3: u32,
    pub l11: Vector3,
    pub pad4: u32,

    // L2
    pub l2_2: Vector3,
    pub pad5: u32,
    pub l2_1: Vector3,
    pub pad6: u32,
    pub l20: Vector3,
    pub pad7: u32,
    pub l21: Vector3,
    pub pad8: u32,
    pub l22: Vector3,
    pub pad9: u32,
}
