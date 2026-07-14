use std::{
    ffi::c_void,
    ptr,
    sync::atomic::{AtomicU64, Ordering},
};

use crate::vulkan_context::VulkanContext;
use ash::vk::{self, Handle};

static ALLOCATED_GPU_MEMORY: AtomicU64 = AtomicU64::new(0);

fn register_gpu_alloc(bytes: u64) -> f64 {
    let val = ALLOCATED_GPU_MEMORY.fetch_add(bytes, Ordering::Relaxed) + bytes;

    let mb = val as f64 / (1024.0 * 1024.0);
    mb
}

fn unregister_gpu_alloc(bytes: u64) {
    ALLOCATED_GPU_MEMORY.fetch_sub(bytes, Ordering::Relaxed);
}

pub struct Buffer {
    pub buffer: vk::Buffer,
    pub memory: vk::DeviceMemory,
    pub address: vk::DeviceAddress,

    pub ptr: *mut c_void,

    pub bytes: u64,
    pub name: String,
}

impl Buffer {
    pub fn null() -> Self {
        Self {
            buffer: vk::Buffer::null(),
            memory: vk::DeviceMemory::null(),
            address: 0,
            bytes: 0,
            name: String::new(),
            ptr: ptr::null_mut(),
        }
    }

    pub fn new<T>(
        vk: &VulkanContext,
        name: String,
        bytes: &[T],
        usage: vk::BufferUsageFlags,
        properties: vk::MemoryPropertyFlags,
    ) -> Self {
        let size = (bytes.len() * std::mem::size_of::<T>()) as vk::DeviceSize;

        let buffer = Buffer::empty(vk, name, size, usage, properties);

        let (a, bytes, b) = unsafe { bytes.align_to::<u8>() };
        assert!(a.len() == 0);
        assert!(b.len() == 0);

        vk.upload_buffer(bytes, buffer.buffer);

        buffer
    }

    pub fn empty(
        vk: &VulkanContext,
        name: String,
        size: vk::DeviceSize,
        usage: vk::BufferUsageFlags,
        properties: vk::MemoryPropertyFlags,
    ) -> Self {
        let create_info = vk::BufferCreateInfo {
            size,
            usage,
            sharing_mode: vk::SharingMode::EXCLUSIVE,
            ..Default::default()
        };

        let buffer = unsafe { vk.device.create_buffer(&create_info, None).unwrap() };
        let mem_reqs = unsafe { vk.device.get_buffer_memory_requirements(buffer) };
        let memory_type_index = vk.find_memory_type(mem_reqs.memory_type_bits, properties);

        let mut allocate_info = vk::MemoryAllocateInfo {
            allocation_size: mem_reqs.size,
            memory_type_index,
            ..Default::default()
        };

        let mut allocate_flags = vk::MemoryAllocateFlagsInfo {
            flags: vk::MemoryAllocateFlags::DEVICE_ADDRESS,
            ..Default::default()
        };

        allocate_info = allocate_info.push_next(&mut allocate_flags);

        let memory = unsafe { vk.device.allocate_memory(&allocate_info, None) }.unwrap();

        unsafe { vk.device.bind_buffer_memory(buffer, memory, 0) }.unwrap();

        let info = vk::BufferDeviceAddressInfo {
            buffer,
            ..Default::default()
        };

        let address = if properties.contains(vk::MemoryPropertyFlags::HOST_VISIBLE) {
            0
        } else {
            unsafe { vk.device.get_buffer_device_address(&info) }
        };

        let allocated = register_gpu_alloc(mem_reqs.size);
        let mb = mem_reqs.size as f64 / (1024.0 * 1024.0);

        println!(
            "Created Buffer '{:#x}' VRAM:{:.3} MiB (Total: {:.3} MiB) ({})",
            buffer.as_raw(),
            mb,
            allocated,
            &name,
        );

        let ptr = if properties.contains(vk::MemoryPropertyFlags::HOST_VISIBLE) {
            unsafe {
                vk.device
                    .map_memory(memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
                    .unwrap()
            }
        } else {
            ptr::null_mut()
        };

        Self {
            buffer,
            memory,
            address,
            bytes: size,
            name,
            ptr,
        }
    }

    pub fn destroy(&mut self, vk: &VulkanContext) {
        debug_assert!(!self.buffer.is_null());
        debug_assert!(!self.memory.is_null());

        unsafe {
            vk.device.destroy_buffer(self.buffer, None);
            if !self.memory.is_null() {
                unregister_gpu_alloc(self.bytes);
                vk.device.free_memory(self.memory, None);
            }
        };

        self.buffer = vk::Buffer::null();
        self.memory = vk::DeviceMemory::null();
        self.address = 0;
    }
}
