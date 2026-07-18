use ash::vk;

use crate::{buffer::Buffer, math::Vector3, vulkan_context::VulkanContext};

pub struct Background {
    width: u32,
    height: u32,
    layout: vk::ImageLayout,
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
    sampler: vk::Sampler,
    bytes: u64,
}

impl Background {
    pub fn new(ctx: &VulkanContext, width: u32, height: u32, pixels: &[f32]) -> Background {
        let vk = &ctx.device;
        let format = vk::Format::R32G32B32A32_SFLOAT;

        let face_bytes = (width * height * 4 * std::mem::size_of::<f32>() as u32) as u64;
        let total_bytes = face_bytes * 6;

        assert_eq!(
            pixels.len() as u64,
            total_bytes / std::mem::size_of::<f32>() as u64,
            "pixels must be width*height*4 floats per face * 6 faces"
        );

        let image_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(vk::Extent3D {
                width,
                height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(6)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .flags(vk::ImageCreateFlags::CUBE_COMPATIBLE);

        unsafe {
            let image = vk.create_image(&image_info, None).unwrap();
            let requirements = vk.get_image_memory_requirements(image);
            let memory_type = ctx.find_memory_type(
                requirements.memory_type_bits,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
            );
            let alloc_info = vk::MemoryAllocateInfo::default()
                .allocation_size(requirements.size)
                .memory_type_index(memory_type);
            let memory = vk.allocate_memory(&alloc_info, None).unwrap();
            vk.bind_image_memory(image, memory, 0).unwrap();

            let mut staging = Buffer::empty(
                &ctx,
                "Background Staging".to_owned(),
                total_bytes,
                vk::BufferUsageFlags::TRANSFER_SRC,
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            );
            std::ptr::copy_nonoverlapping(pixels.as_ptr(), staging.ptr as *mut f32, pixels.len());

            let cmd = ctx.begin_single_use_cmd();

            let subresource_range = vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .base_mip_level(0)
                .level_count(1)
                .base_array_layer(0)
                .layer_count(6);

            let to_transfer_dst = vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
                .subresource_range(subresource_range)
                .src_access_mask(vk::AccessFlags::empty())
                .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE);

            vk.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[to_transfer_dst],
            );

            let region = vk::BufferImageCopy::default()
                .buffer_offset(0)
                .buffer_row_length(0)
                .buffer_image_height(0)
                .image_subresource(
                    vk::ImageSubresourceLayers::default()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .mip_level(0)
                        .base_array_layer(0)
                        .layer_count(6),
                )
                .image_offset(vk::Offset3D::default())
                .image_extent(vk::Extent3D {
                    width,
                    height,
                    depth: 1,
                });

            vk.cmd_copy_buffer_to_image(
                cmd,
                staging.buffer,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[region],
            );

            let to_shader_read = vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
                .subresource_range(subresource_range)
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::SHADER_READ);

            vk.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[to_shader_read],
            );

            ctx.end_single_use_cmd(cmd);

            staging.destroy(&ctx);

            let view_info = vk::ImageViewCreateInfo::default()
                .image(image)
                .view_type(vk::ImageViewType::CUBE)
                .format(format)
                .subresource_range(subresource_range);
            let view = vk.create_image_view(&view_info, None).unwrap();

            let sampler_info = vk::SamplerCreateInfo::default()
                .mag_filter(vk::Filter::LINEAR)
                .min_filter(vk::Filter::LINEAR)
                .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .anisotropy_enable(false)
                .max_anisotropy(1.0)
                .border_color(vk::BorderColor::FLOAT_OPAQUE_BLACK)
                .unnormalized_coordinates(false)
                .compare_enable(false)
                .compare_op(vk::CompareOp::ALWAYS)
                .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
                .min_lod(0.0)
                .max_lod(0.0)
                .mip_lod_bias(0.0);
            let sampler = vk.create_sampler(&sampler_info, None).unwrap();

            Background {
                width,
                height,
                layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                image,
                memory,
                view,
                bytes: total_bytes,
                sampler,
            }
        }
    }

    pub fn destroy(&mut self, ctx: &VulkanContext) {
        let vk = &ctx.device;
        unsafe {
            vk.destroy_image_view(self.view, None);
            vk.destroy_image(self.image, None);
            vk.free_memory(self.memory, None);
            vk.destroy_sampler(self.sampler, None);
        };
    }

    pub fn solid(ctx: &VulkanContext, width: u32, height: u32, color: Vector3) -> Background {
        let texels_per_face = (width * height) as usize;
        let mut pixels = Vec::with_capacity(texels_per_face * 4 * 6);
        for _ in 0..(texels_per_face * 6) {
            pixels.push(color.x);
            pixels.push(color.y);
            pixels.push(color.z);
            pixels.push(1.0);
        }
        Background::new(ctx, width, height, &pixels)
    }

    pub fn null() -> Background {
        Background {
            width: 0,
            height: 0,
            layout: vk::ImageLayout::UNDEFINED,
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            view: vk::ImageView::null(),
            sampler: vk::Sampler::null(),
            bytes: 0,
        }
    }

    pub fn image(&self) -> vk::Image {
        self.image
    }

    pub fn view(&self) -> vk::ImageView {
        self.view
    }

    pub fn sampler(&self) -> vk::Sampler {
        self.sampler
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn layout(&self) -> vk::ImageLayout {
        self.layout
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}
