use glam::Vec3;

use crate::shader::RayTracingData;

pub struct Camera {
    pub position: Vec3,
    /// normalized view direction
    pub forward: Vec3,
    /// normalized, orthogonal to `forward`
    pub up: Vec3,
    /// vertical field of view in degrees, perspective only
    pub fov: f32,
    /// half of the vertical view size, orthographic when > 0
    pub ortho_size: f32,
}

#[repr(C, align(16))]
pub struct InitializePreviewPushConstants {
    pub rt: RayTracingData,

    pub camera_position: Vec3,
    /// tan(fov / 2) for perspective, half the view height for orthographic
    pub fov: f32,

    pub camera_direction: Vec3,
    /// 0 perspective, 1 orthographic
    pub orthographic: u32,

    pub camera_up: Vec3,
    pub pad0: u32,

    pub resolution: [u32; 2],
    pub pad1: u32,
    pub pad2: u32,
}

impl Camera {
    pub fn fov_half_tan(&self) -> f32 {
        (self.fov.to_radians() * 0.5).tan()
    }

    pub fn orthographic(&self) -> bool {
        self.ortho_size > 0.0
    }

    /// tan(fov / 2) or the orthographic half height, what the init shader scales the view plane with
    pub fn view_scale(&self) -> f32 {
        if self.orthographic() {
            self.ortho_size
        } else {
            self.fov_half_tan()
        }
    }

    pub fn look_at(&mut self, target: Vec3) {
        let dir = (target - self.position).normalize_or_zero();
        self.set_forward(dir);
    }

    /// Sets the direction with world up, no roll
    pub fn set_forward(&mut self, dir: Vec3) {
        let dir = dir.normalize_or_zero();
        if dir == Vec3::ZERO {
            return;
        }
        self.forward = dir;
        let right = dir.cross(Vec3::Y).normalize_or_zero();
        self.up = if right == Vec3::ZERO {
            // looking straight up or down
            Vec3::Z
        } else {
            right.cross(dir)
        };
    }

    /// Sets the full orientation, `up` doesn't have to be exactly orthogonal
    pub fn set_basis(&mut self, forward: Vec3, up: Vec3) {
        let forward = forward.normalize_or_zero();
        if forward == Vec3::ZERO {
            return;
        }
        let right = forward.cross(up).normalize_or_zero();
        if right == Vec3::ZERO {
            self.set_forward(forward);
            return;
        }
        self.forward = forward;
        self.up = right.cross(forward);
    }

    pub fn get_forward(&self) -> Vec3 {
        self.forward
    }

    pub fn yaw(&self) -> f32 {
        self.forward.x.atan2(self.forward.z)
    }

    pub fn pitch(&self) -> f32 {
        self.forward.y.clamp(-1.0, 1.0).asin()
    }

    pub fn set_yaw_pitch(&mut self, yaw: f32, pitch: f32) {
        let (sin_yaw, cos_yaw) = yaw.sin_cos();
        let (sin_pitch, cos_pitch) = pitch.sin_cos();
        self.set_forward(Vec3::new(
            cos_pitch * sin_yaw,
            sin_pitch,
            cos_pitch * cos_yaw,
        ));
    }
}
