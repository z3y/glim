use ash::vk::{self};

use crate::math::{Vector2, Vector3};

#[repr(u32)]
pub enum LightType {
    Directional = 0,
    Point = 1,
    Spot = 2,
    Area = 3,
}

#[repr(C)]
pub struct Light {
    pub position: Vector3,
    pub ty: LightType,

    pub direction: Vector3,
    pub range: f32,

    pub color: Vector3,
    pub shadow_radius_or_angle: f32,

    pub spot_inner_percent: f32,
    pub spot_outer: f32,
    pub area_size: Vector2,

    pub up: Vector3,
    pub pad: u32,
}

pub fn light_buffer_flags() -> vk::BufferUsageFlags {
    vk::BufferUsageFlags::TRANSFER_DST
        | vk::BufferUsageFlags::STORAGE_BUFFER
        | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
}

impl Default for Light {
    fn default() -> Self {
        Self {
            position: Vector3::ZERO,
            ty: LightType::Directional,

            direction: Vector3::FORWARD,
            range: 0.0,

            color: Vector3::ONE,
            shadow_radius_or_angle: 0.0,

            spot_inner_percent: 0.0,
            spot_outer: 0.0,
            area_size: Vector2::ZERO,

            up: Vector3::UP,
            pad: 0,
        }
    }
}
