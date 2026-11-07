use crate::{math::Vector3, shader::RayTracingData};

pub struct Camera {
    pub position: Vector3,
    pub yaw: f32,
    pub pitch: f32,
    pub fov: f32,

    pub last_cursor_pos: Option<(f64, f64)>,
}

#[repr(C, align(16))]
pub struct InitializePreviewPushConstants {
    pub rt: RayTracingData,

    pub camera_position: Vector3,
    pub fov: f32,

    pub camera_direction: Vector3,
    pub pad0: u32,

    pub resolution: [u32; 2],
    pub pad1: u32,
    pub pad2: u32,
}

impl Camera {
    pub fn camera_direction(&self) -> Vector3 {
        let x = self.yaw.sin() * self.pitch.cos();
        let y = self.pitch.sin();
        let z = self.yaw.cos() * self.pitch.cos();

        Vector3::new(x, y, z).normalize()
    }

    pub fn fov_half_tan(&self) -> f32 {
        (self.fov.to_radians() * 0.5).tan()
    }

    pub fn look_at(&mut self, target: Vector3) {
        let dir = (target - self.position).normalize();
        self.set_forward(dir);
    }

    pub fn set_forward(&mut self, dir: Vector3) {
        self.pitch = dir.y.asin();
        self.yaw = dir.x.atan2(dir.z);
    }

    pub fn get_forward(&self) -> Vector3 {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();
        Vector3::new(cos_pitch * sin_yaw, sin_pitch, cos_pitch * cos_yaw)
    }
}
