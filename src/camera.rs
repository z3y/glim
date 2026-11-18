use glam::Vec3;

use crate::shader::RayTracingData;

pub struct Camera {
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub fov: f32,
}

#[repr(C, align(16))]
pub struct InitializePreviewPushConstants {
    pub rt: RayTracingData,

    pub camera_position: Vec3,
    pub fov: f32,

    pub camera_direction: Vec3,
    pub pad0: u32,

    pub resolution: [u32; 2],
    pub pad1: u32,
    pub pad2: u32,
}

impl Camera {
    pub fn camera_direction(&self) -> Vec3 {
        let x = self.yaw.sin() * self.pitch.cos();
        let y = self.pitch.sin();
        let z = self.yaw.cos() * self.pitch.cos();

        Vec3::new(x, y, z).normalize_or_zero()
    }

    pub fn fov_half_tan(&self) -> f32 {
        (self.fov.to_radians() * 0.5).tan()
    }

    pub fn look_at(&mut self, target: Vec3) {
        let dir = (target - self.position).normalize_or_zero();
        self.set_forward(dir);
    }

    pub fn set_forward(&mut self, dir: Vec3) {
        self.pitch = dir.y.asin();
        self.yaw = dir.x.atan2(dir.z);
    }

    pub fn get_forward(&self) -> Vec3 {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
        Vec3::new(cos_pitch * sin_yaw, sin_pitch, cos_pitch * cos_yaw)
    }
}
