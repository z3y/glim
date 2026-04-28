use ash::vk::{self, Handle};

use crate::{math::Vector3, vulkan_context::VulkanContext};

#[repr(u32)]
pub enum LightType {
    Directional = 0,
    Point = 1,
    Spot = 2,
}

#[repr(C)]
pub struct Light {
    pub ty: LightType,
    pub position: Vector3,

    pub direction: Vector3,
    pub range: f32,

    pub color: Vector3,
    pub shadow_range_or_angle: f32,
}

pub struct GpuLights {
    pub buffer: vk::Buffer,
    pub memory: vk::DeviceMemory,
    pub address: vk::DeviceAddress,
}

impl GpuLights {
    pub fn new(vk: &VulkanContext, lights: &[Light]) -> Self {
        let usage = vk::BufferUsageFlags::TRANSFER_DST
            | vk::BufferUsageFlags::STORAGE_BUFFER
            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS;

        let size = (lights.len() * std::mem::size_of::<Light>()) as vk::DeviceSize;
        let (buffer, memory) = vk.create_buffer(size, usage, vk::MemoryPropertyFlags::DEVICE_LOCAL);

        let info = vk::BufferDeviceAddressInfo {
            buffer,
            ..Default::default()
        };

        let address = unsafe { vk.device.get_buffer_device_address(&info) };

        let (_, bytes, _) = unsafe { lights.align_to::<u8>() };
        vk.upload_buffer(bytes, buffer);

        Self {
            buffer,
            memory,
            address,
        }
    }

    pub fn destroy(&mut self, vk: &VulkanContext) {
        assert!(!self.buffer.is_null());
        assert!(!self.memory.is_null());

        unsafe {
            vk.device.destroy_buffer(self.buffer, None);
            vk.device.free_memory(self.memory, None);
        };

        self.buffer = vk::Buffer::null();
        self.memory = vk::DeviceMemory::null();
        self.address = 0;
    }

    pub fn address(&self) -> u64 {
        self.address
    }
}
