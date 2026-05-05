use crate::{compute_shader::InitFromCameraPushConstants, math::Vector3};

pub struct Camera {
    pub position: Vector3,
    pub yaw: f32,
    pub pitch: f32,
    pub fov: f32,

    pub last_cursor_pos: Option<(f64, f64)>,
}

impl Camera {
    pub fn make_push_constants(&self) -> InitFromCameraPushConstants {
        let x = self.yaw.cos() * self.pitch.cos();
        let y = self.pitch.sin();
        let z = self.yaw.sin() * self.pitch.cos();

        let camera_direction = Vector3::new(x, y, z).normalize();

        let fov_half_tan = (self.fov.to_radians() * 0.5).tan();

        InitFromCameraPushConstants {
            camera_position: self.position,
            fov_half_tan,
            camera_direction,
            pad: 0,
        }
    }

    pub fn look_at(&mut self, target: Vector3) {
        let dir = (target - self.position).normalize();
        self.pitch = dir.y.asin();
        self.yaw = dir.z.atan2(dir.x);
    }

    pub fn set_forward(&mut self, dir: Vector3) {
        self.pitch = dir.y.asin();
        self.yaw = dir.z.atan2(dir.x);
    }
}
