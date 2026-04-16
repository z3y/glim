use std::ptr;

use ash::vk::{self, Handle};

use crate::vulkan_core::VulkanContext;

pub struct Texture2D {
    format: vk::Format,
    width: u32,
    height: u32,
    layout: vk::ImageLayout,

    pub image: vk::Image,
    pub memory: vk::DeviceMemory,
    pub view: vk::ImageView,
}

#[allow(dead_code)]
impl Texture2D {
    pub fn new(
        vk: &VulkanContext,
        width: u32,
        height: u32,
        format: vk::Format,
        usage: vk::ImageUsageFlags,
    ) -> Self {
        let extent = vk::Extent3D {
            width,
            height,
            depth: 1,
        };

        let layout = vk::ImageLayout::UNDEFINED;

        let create_info = vk::ImageCreateInfo {
            image_type: vk::ImageType::TYPE_2D,
            format,
            extent,
            mip_levels: 1,
            array_layers: 1,
            samples: vk::SampleCountFlags::TYPE_1,
            tiling: vk::ImageTiling::OPTIMAL,
            usage,
            sharing_mode: vk::SharingMode::EXCLUSIVE,
            initial_layout: layout,
            ..Default::default()
        };

        let image = unsafe { vk.device.create_image(&create_info, None) }.unwrap();

        let mem_reqs = unsafe { vk.device.get_image_memory_requirements(image) };

        let memory_type_index = vk.find_memory_type(
            mem_reqs.memory_type_bits,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        );

        let allocate_info = vk::MemoryAllocateInfo {
            allocation_size: mem_reqs.size,
            memory_type_index,
            ..Default::default()
        };

        let memory = unsafe { vk.device.allocate_memory(&allocate_info, None) }.unwrap();
        unsafe { vk.device.bind_image_memory(image, memory, 0) }.unwrap();

        let subresource_range = vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        };

        let create_info = vk::ImageViewCreateInfo {
            image,
            view_type: vk::ImageViewType::TYPE_2D,
            format,
            subresource_range,
            ..Default::default()
        };

        let view = unsafe { vk.device.create_image_view(&create_info, None) }.unwrap();

        Self {
            format,
            image,
            memory,
            view,
            width,
            height,
            layout,
        }
    }

    pub fn destroy(&mut self, vk: &VulkanContext) {
        if self.image.is_null() {
            return;
        }

        unsafe {
            vk.device.destroy_image_view(self.view, None);
            vk.device.free_memory(self.memory, None);
            vk.device.destroy_image(self.image, None);
        };

        self.view = vk::ImageView::null();
        self.memory = vk::DeviceMemory::null();
        self.image = vk::Image::null();
    }

    // only 4 channel f32 textures
    pub fn set_pixels(&self, vk: &VulkanContext, pixels: &[f32]) {
        let size = (pixels.len() * std::mem::size_of::<f32>()) as vk::DeviceSize;

        assert!(pixels.len() as u32 == self.width * self.height * 4);

        let (staging_buffer, staging_memory) = vk.create_buffer(
            size,
            vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        );

        let ptr = unsafe {
            vk.device
                .map_memory(staging_memory, 0, size, vk::MemoryMapFlags::empty())
                .unwrap()
        } as *mut f32;

        unsafe {
            ptr::copy_nonoverlapping(pixels.as_ptr(), ptr, pixels.len());
            vk.device.unmap_memory(staging_memory);
        };

        let cmd = vk.begin_temp_graphics_cmd();

        let subresource_range = vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        };

        let barrier = vk::ImageMemoryBarrier {
            dst_access_mask: vk::AccessFlags::TRANSFER_WRITE,
            old_layout: vk::ImageLayout::UNDEFINED,
            new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            image: self.image,
            subresource_range,
            ..Default::default()
        };

        unsafe {
            vk.device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            )
        };

        let image_subresource = vk::ImageSubresourceLayers {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        };

        let image_extent = vk::Extent3D {
            width: self.width,
            height: self.height,
            depth: 1,
        };

        let region = vk::BufferImageCopy {
            image_subresource,
            image_extent,
            ..Default::default()
        };

        unsafe {
            vk.device.cmd_copy_buffer_to_image(
                cmd,
                staging_buffer,
                self.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[region],
            )
        };

        let barrier = vk::ImageMemoryBarrier {
            src_access_mask: vk::AccessFlags::TRANSFER_WRITE,
            dst_access_mask: vk::AccessFlags::TRANSFER_READ,
            old_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            new_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            image: self.image,
            subresource_range,
            ..Default::default()
        };

        unsafe {
            vk.device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            )
        };

        vk.end_temp_graphics_cmd(cmd);

        unsafe {
            vk.device.destroy_buffer(staging_buffer, None);
            vk.device.free_memory(staging_memory, None);
        };
    }

    pub fn layout(&self) -> vk::ImageLayout {
        self.layout
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn format(&self) -> vk::Format {
        self.format
    }
}
