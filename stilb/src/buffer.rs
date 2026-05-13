use ash::vk::{self, Handle};

use crate::vulkan_context::VulkanContext;

pub struct Buffer {
    pub buffer: vk::Buffer,
    pub memory: vk::DeviceMemory,
    pub address: vk::DeviceAddress,
}

impl Buffer {
    pub fn null() -> Self {
        Self {
            buffer: vk::Buffer::null(),
            memory: vk::DeviceMemory::null(),
            address: 0,
        }
    }

    pub fn new<T>(vk: &VulkanContext, bytes: &[T], usage: vk::BufferUsageFlags) -> Self {
        let size = (bytes.len() * std::mem::size_of::<T>()) as vk::DeviceSize;
        let (buffer, memory) = vk.create_buffer(size, usage, vk::MemoryPropertyFlags::DEVICE_LOCAL);

        let info = vk::BufferDeviceAddressInfo {
            buffer,
            ..Default::default()
        };

        let address = unsafe { vk.device.get_buffer_device_address(&info) };

        let (a, bytes, b) = unsafe { bytes.align_to::<u8>() };

        assert!(a.len() == 0);
        assert!(b.len() == 0);

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
}
