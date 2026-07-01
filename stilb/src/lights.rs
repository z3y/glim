use ash::vk::{self};

use crate::math::Vector3;

#[repr(u32)]
pub enum LightType {
    Directional = 0,
    Point = 1,
    Spot = 2,
}

#[repr(C)]
pub struct Light {
    pub position: Vector3,
    pub ty: LightType,

    pub direction: Vector3,
    pub range: f32,

    pub color: Vector3,
    pub shadow_radius_or_angle: f32,
}

pub fn light_buffer_flags() -> vk::BufferUsageFlags {
    vk::BufferUsageFlags::TRANSFER_DST
        | vk::BufferUsageFlags::STORAGE_BUFFER
        | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
}
