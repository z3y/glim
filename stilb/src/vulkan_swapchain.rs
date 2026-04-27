use ash::vk::{self, Handle};

use crate::vulkan_context::VulkanContext;

pub struct SwapchainFrame {
    pub image_view: vk::ImageView,
    pub image: vk::Image,
    pub command_buffer: vk::CommandBuffer,
    pub image_available_semaphore: vk::Semaphore,
    pub render_finished_semaphore: vk::Semaphore,
    pub fence: vk::Fence,
}

pub struct SwapchainData {
    pub swapchain: vk::SwapchainKHR,
    pub extent: vk::Extent2D,
    pub frame_index: usize,
    pub frames: Vec<SwapchainFrame>,
}

fn query_swapchain_support(
    vk: &VulkanContext,
) -> (
    vk::SurfaceCapabilitiesKHR,
    Vec<vk::SurfaceFormatKHR>,
    Vec<vk::PresentModeKHR>,
) {
    let capabilities = unsafe {
        vk.surface_instance
            .get_physical_device_surface_capabilities(vk.physical_device, vk.surface)
            .unwrap()
    };

    let formats = unsafe {
        vk.surface_instance
            .get_physical_device_surface_formats(vk.physical_device, vk.surface)
            .unwrap()
    };

    let present_modes = unsafe {
        vk.surface_instance
            .get_physical_device_surface_present_modes(vk.physical_device, vk.surface)
            .unwrap()
    };

    (capabilities, formats, present_modes)
}

impl VulkanContext {
    pub fn create_swapchain(&mut self, width: u32, height: u32) {
        self.destroy_swapchain();

        let (capabilities, formats, present_modes) = query_swapchain_support(self);

        let mut selected_format = formats[0];
        for format in formats {
            if format.format == vk::Format::B8G8R8A8_SRGB
                && format.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
            {
                selected_format = format;
                break;
            }
        }

        let mut selected_present_mode = present_modes[0];
        for mode in present_modes {
            if mode == vk::PresentModeKHR::MAILBOX {
                selected_present_mode = mode;
                break;
            }
        }

        let extent = if capabilities.current_extent.width != u32::MAX {
            capabilities.current_extent
        } else {
            vk::Extent2D {
                width: width.clamp(
                    capabilities.min_image_extent.width,
                    capabilities.max_image_extent.width,
                ),
                height: height.clamp(
                    capabilities.min_image_extent.height,
                    capabilities.max_image_extent.height,
                ),
            }
        };

        let image_count = 3.clamp(capabilities.min_image_count, capabilities.max_image_count);

        println!(
            "images: {:?}\nextent: {:?}\nformat: {:?}\nmode: {:?}",
            image_count, extent, selected_format, selected_present_mode
        );

        let mut create_info = vk::SwapchainCreateInfoKHR {
            surface: self.surface,
            min_image_count: image_count,
            image_format: selected_format.format,
            image_color_space: selected_format.color_space,
            image_extent: extent,
            image_array_layers: 1,
            image_usage: vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_DST,
            pre_transform: capabilities.current_transform,
            composite_alpha: vk::CompositeAlphaFlagsKHR::OPAQUE,
            present_mode: selected_present_mode,
            clipped: vk::TRUE,
            old_swapchain: vk::SwapchainKHR::null(), // todo set old swapchain
            ..Default::default()
        };

        let queue_family_indices = [
            self.queue_family_indices.graphics,
            self.queue_family_indices.present,
        ];

        if self.queue_family_indices.graphics != self.queue_family_indices.present {
            create_info.image_sharing_mode = vk::SharingMode::CONCURRENT;
            create_info = create_info.queue_family_indices(&queue_family_indices);
        } else {
            create_info.image_sharing_mode = vk::SharingMode::EXCLUSIVE;
        }

        let swapchain = unsafe {
            self.swapchain_device
                .create_swapchain(&create_info, None)
                .unwrap()
        };

        let swapchain_images = unsafe {
            self.swapchain_device
                .get_swapchain_images(swapchain)
                .unwrap()
        };

        let mut frames = Vec::with_capacity(swapchain_images.len());

        for image in swapchain_images {
            let subresource_range = vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            };

            let create_info = vk::ImageViewCreateInfo {
                image: image,
                view_type: vk::ImageViewType::TYPE_2D,
                format: selected_format.format,
                subresource_range,
                ..Default::default()
            };

            let image_view = unsafe { self.device.create_image_view(&create_info, None).unwrap() };

            let allocate_info = vk::CommandBufferAllocateInfo {
                command_pool: self.command_pool,
                level: vk::CommandBufferLevel::PRIMARY,
                command_buffer_count: 1,
                ..Default::default()
            };

            let command_buffer = unsafe {
                self.device
                    .allocate_command_buffers(&allocate_info)
                    .unwrap()[0]
            };

            let create_info = vk::SemaphoreCreateInfo {
                ..Default::default()
            };

            let image_available_semaphore =
                unsafe { self.device.create_semaphore(&create_info, None).unwrap() };
            let render_finished_semaphore =
                unsafe { self.device.create_semaphore(&create_info, None).unwrap() };

            let create_info = vk::FenceCreateInfo {
                flags: vk::FenceCreateFlags::SIGNALED,
                ..Default::default()
            };

            let fence = unsafe { self.device.create_fence(&create_info, None).unwrap() };

            let frame = SwapchainFrame {
                image_view,
                command_buffer,
                image_available_semaphore,
                render_finished_semaphore,
                fence,
                image,
            };

            frames.push(frame);
        }

        self.swapchain = SwapchainData {
            frames,
            swapchain,
            frame_index: 0,
            extent,
        };
    }

    pub fn destroy_swapchain(&mut self) {
        for frame in &self.swapchain.frames {
            unsafe {
                self.device.destroy_image_view(frame.image_view, None);
                self.device.destroy_fence(frame.fence, None);
                self.device
                    .destroy_semaphore(frame.image_available_semaphore, None);
                self.device
                    .destroy_semaphore(frame.render_finished_semaphore, None);
            };
        }

        self.swapchain.frames.clear();

        let swapchain = self.swapchain.swapchain;

        if !swapchain.is_null() {
            unsafe { self.swapchain_device.destroy_swapchain(swapchain, None) };
        }

        self.swapchain.swapchain = vk::SwapchainKHR::null();
    }
}
