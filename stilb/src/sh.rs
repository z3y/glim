use std::f32::{self};

use crate::math::Vector3;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SHProbe {
    pub position: Vector3,
    pub pad0: u32,

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

impl SHProbe {
    #[inline]
    pub fn normalize(&mut self, samples: u32) {
        let scale = 1.0 / (samples as f32);

        self.l0 = self.l0 * scale;

        self.l1_1 = self.l1_1 * scale;
        self.l10 = self.l10 * scale;
        self.l11 = self.l11 * scale;

        self.l2_2 = self.l2_2 * scale;
        self.l2_1 = self.l2_1 * scale;
        self.l20 = self.l20 * scale;
        self.l21 = self.l21 * scale;
        self.l22 = self.l22 * scale;
    }
}
