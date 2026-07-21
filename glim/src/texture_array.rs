use ash::vk;
use ash::vk::Handle;

use crate::{
    texture2d::{Texture2D, register_gpu_alloc},
    vulkan_context::VulkanContext,
};

pub struct TextureArray {
    pub textures: Vec<Texture2D>,
    memory: vk::DeviceMemory,
}

pub struct TextureDescriptor {
    pub width: u32,
    pub height: u32,
    pub format: vk::Format,
    pub usage: vk::ImageUsageFlags,
    pub name: String,
}

impl TextureArray {
    pub fn new(vk: &VulkanContext, specs: Vec<TextureDescriptor>) -> Self {
        struct Pending {
            image: vk::Image,
            view: vk::ImageView,
            mem_reqs: vk::MemoryRequirements,
            offset: vk::DeviceSize,
            spec: TextureDescriptor,
        }
        let mut pending: Vec<Pending> = specs
            .into_iter()
            .map(|spec| {
                let create_info = vk::ImageCreateInfo {
                    image_type: vk::ImageType::TYPE_2D,
                    format: spec.format,
                    extent: vk::Extent3D {
                        width: spec.width,
                        height: spec.height,
                        depth: 1,
                    },
                    mip_levels: 1,
                    array_layers: 1,
                    samples: vk::SampleCountFlags::TYPE_1,
                    tiling: vk::ImageTiling::OPTIMAL,
                    usage: spec.usage,
                    sharing_mode: vk::SharingMode::EXCLUSIVE,
                    initial_layout: vk::ImageLayout::UNDEFINED,
                    ..Default::default()
                };
                let image = unsafe { vk.device.create_image(&create_info, None) }.unwrap();
                let mem_reqs = unsafe { vk.device.get_image_memory_requirements(image) };
                Pending {
                    image,
                    view: vk::ImageView::null(),
                    mem_reqs,
                    offset: 0,
                    spec,
                }
            })
            .collect();

        let mut memory_type_bits = !0u32;
        let mut cursor: vk::DeviceSize = 0;
        for p in &mut pending {
            memory_type_bits &= p.mem_reqs.memory_type_bits;
            let align = p.mem_reqs.alignment;
            cursor = (cursor + align - 1) & !(align - 1);
            p.offset = cursor;
            cursor += p.mem_reqs.size;
        }
        let total_size = cursor;

        let memory_type_index =
            vk.find_memory_type(memory_type_bits, vk::MemoryPropertyFlags::DEVICE_LOCAL);

        let allocate_info = vk::MemoryAllocateInfo {
            allocation_size: total_size,
            memory_type_index,
            ..Default::default()
        };
        let memory = unsafe { vk.device.allocate_memory(&allocate_info, None) }.unwrap();

        let allocated = register_gpu_alloc(total_size);
        println!(
            "TextureArray: allocated {:.3} MiB (Total: {:.3} MiB) for {} textures",
            total_size as f64 / (1024.0 * 1024.0),
            allocated,
            pending.len()
        );

        let textures = pending
            .into_iter()
            .map(|p| {
                unsafe { vk.device.bind_image_memory(p.image, memory, p.offset) }.unwrap();

                let view_info = vk::ImageViewCreateInfo {
                    image: p.image,
                    view_type: vk::ImageViewType::TYPE_2D,
                    format: p.spec.format,
                    subresource_range: vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    },
                    ..Default::default()
                };
                let view = unsafe { vk.device.create_image_view(&view_info, None) }.unwrap();

                let mb = p.mem_reqs.size as f64 / (1024.0 * 1024.0);
                println!(
                    "Created Texture '{:#x}' VRAM:{:.3} MiB (Total: {:.3} MiB) ({}) {}x{}",
                    p.image.as_raw(),
                    mb,
                    allocated,
                    &p.spec.name,
                    p.spec.width,
                    p.spec.height
                );

                Texture2D {
                    format: p.spec.format,
                    image: p.image,
                    memory,
                    view,
                    width: p.spec.width,
                    height: p.spec.height,
                    layout: vk::ImageLayout::UNDEFINED,
                    bytes: p.mem_reqs.size,
                    name: p.spec.name,
                }
            })
            .collect();

        Self { textures, memory }
    }

    pub fn null() -> TextureArray {
        TextureArray {
            textures: Vec::new(),
            memory: vk::DeviceMemory::null(),
        }
    }

    pub fn views(&self) -> Vec<vk::ImageView> {
        self.textures.iter().map(|t| t.view).collect()
    }

    pub fn destroy(&self, vk: &VulkanContext) {
        for tex in &self.textures {
            unsafe {
                vk.device.destroy_image_view(tex.view, None);
                vk.device.destroy_image(tex.image, None);
            }
        }
        unsafe { vk.device.free_memory(self.memory, None) };
    }
}
