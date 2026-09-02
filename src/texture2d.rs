use crate::{buffer::Buffer, vulkan_context::VulkanContext};
use ash::vk::{self, Handle};
use std::{
    ptr, slice,
    sync::atomic::{AtomicU64, Ordering},
};

pub struct Texture2D {
    pub format: vk::Format,
    pub width: u32,
    pub height: u32,
    pub layout: vk::ImageLayout,

    pub image: vk::Image,
    pub memory: vk::DeviceMemory,
    pub view: vk::ImageView,

    pub bytes: u64,
    pub name: String,
}

static ALLOCATED_GPU_MEMORY: AtomicU64 = AtomicU64::new(0);

pub fn register_gpu_alloc(bytes: u64) -> f64 {
    let val = ALLOCATED_GPU_MEMORY.fetch_add(bytes, Ordering::Relaxed) + bytes;

    let mb = val as f64 / (1024.0 * 1024.0);
    mb
}

fn unregister_gpu_alloc(bytes: u64) {
    ALLOCATED_GPU_MEMORY.fetch_sub(bytes, Ordering::Relaxed);
}

#[allow(dead_code)]
impl Texture2D {
    pub fn new(
        vk: &VulkanContext,
        width: u32,
        height: u32,
        format: vk::Format,
        usage: vk::ImageUsageFlags,
        name: String,
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

        let allocated = register_gpu_alloc(mem_reqs.size);
        let mb = mem_reqs.size as f64 / (1024.0 * 1024.0);
        println!(
            "Created Texture '{:#x}' VRAM:{:.3} MiB (Total: {:.3} MiB) ({}) {}x{}",
            image.as_raw(),
            mb,
            allocated,
            &name,
            width,
            height
        );

        Self {
            format,
            image,
            memory,
            view,
            width,
            height,
            layout,
            bytes: mem_reqs.size,
            name,
        }
    }

    pub fn destroy(&mut self, vk: &VulkanContext) {
        debug_assert!(!self.image.is_null());
        debug_assert!(!self.view.is_null());
        debug_assert!(!self.memory.is_null());

        unsafe {
            if !self.view().is_null() {
                vk.device.destroy_image_view(self.view, None);
            }
            if !self.memory().is_null() {
                vk.device.free_memory(self.memory, None);
                unregister_gpu_alloc(self.bytes);
            }

            if !self.image.is_null() {
                vk.device.destroy_image(self.image, None);
            }
        };

        self.view = vk::ImageView::null();
        self.memory = vk::DeviceMemory::null();
        self.image = vk::Image::null();
    }

    fn get_device_size(&self) -> vk::DeviceSize {
        let res = (self.width * self.height) as u64;
        let pixel_size = self.pixel_size();

        res * pixel_size
    }

    fn pixel_size(&self) -> u64 {
        let size = match self.format() {
            vk::Format::R32G32B32A32_SFLOAT => std::mem::size_of::<f32>() * 4,
            vk::Format::R8G8B8A8_UNORM => std::mem::size_of::<u8>() * 4,
            vk::Format::B10G11R11_UFLOAT_PACK32 => std::mem::size_of::<u8>() * 4,
            _ => unreachable!(),
        } as u64;

        size
    }

    // only 4 channel f32 or u8 textures
    // pub fn set_pixels<T: Copy>(&mut self, vk: &VulkanContext, pixels: &[T]) {
    //     assert!(pixels.len() as u32 == self.width * self.height * 4);
    //     assert!(
    //         std::mem::size_of::<T>() as u64 * pixels.len() as u64 == self.get_device_size(),
    //         "pixel type size doesn't match image format"
    //     );

    //     let size = self.get_device_size();

    //     let (staging_buffer, staging_memory, _) = vk.create_buffer(
    //         size,
    //         vk::BufferUsageFlags::TRANSFER_SRC,
    //         vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    //     );

    //     let ptr = unsafe {
    //         vk.device
    //             .map_memory(staging_memory, 0, size, vk::MemoryMapFlags::empty())
    //             .unwrap()
    //     } as *mut T;

    //     unsafe {
    //         ptr::copy_nonoverlapping(pixels.as_ptr(), ptr, pixels.len());
    //         vk.device.unmap_memory(staging_memory);
    //     };

    //     let cmd = vk.begin_single_use_cmd();

    //     let barrier = self.barrier(
    //         vk::ImageLayout::TRANSFER_DST_OPTIMAL,
    //         vk::AccessFlags::default(),
    //         vk::AccessFlags::TRANSFER_WRITE,
    //     );

    //     unsafe {
    //         vk.device.cmd_pipeline_barrier(
    //             cmd,
    //             vk::PipelineStageFlags::TOP_OF_PIPE,
    //             vk::PipelineStageFlags::TRANSFER,
    //             vk::DependencyFlags::empty(),
    //             &[],
    //             &[],
    //             &[barrier],
    //         )
    //     };

    //     let image_subresource = vk::ImageSubresourceLayers {
    //         aspect_mask: vk::ImageAspectFlags::COLOR,
    //         mip_level: 0,
    //         base_array_layer: 0,
    //         layer_count: 1,
    //     };

    //     let image_extent = vk::Extent3D {
    //         width: self.width,
    //         height: self.height,
    //         depth: 1,
    //     };

    //     let region = vk::BufferImageCopy {
    //         image_subresource,
    //         image_extent,
    //         ..Default::default()
    //     };

    //     unsafe {
    //         vk.device.cmd_copy_buffer_to_image(
    //             cmd,
    //             staging_buffer,
    //             self.image,
    //             vk::ImageLayout::TRANSFER_DST_OPTIMAL,
    //             &[region],
    //         )
    //     };

    //     let barrier = self.barrier(
    //         vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
    //         vk::AccessFlags::TRANSFER_WRITE,
    //         vk::AccessFlags::SHADER_READ,
    //     );

    //     unsafe {
    //         vk.device.cmd_pipeline_barrier(
    //             cmd,
    //             vk::PipelineStageFlags::TRANSFER,
    //             vk::PipelineStageFlags::FRAGMENT_SHADER,
    //             vk::DependencyFlags::empty(),
    //             &[],
    //             &[],
    //             &[barrier],
    //         )
    //     };

    //     vk.end_single_use_cmd(cmd);

    //     unsafe {
    //         vk.device.destroy_buffer(staging_buffer, None);
    //         vk.device.free_memory(staging_memory, None);
    //     };
    // }

    // only 4 channel f32 or u8 textures
    pub fn set_pixels<T: Copy>(&mut self, vk: &VulkanContext, pixels: &[T], staging: &Buffer) {
        assert!(pixels.len() as u32 == self.width * self.height * 4);
        assert!(
            (std::mem::size_of::<T>() as u64) * (pixels.len() as u64) == self.get_device_size(),
            "pixel type size doesn't match image format"
        );

        // let start_time = std::time::Instant::now();

        let staging_buffer = staging.buffer;

        let ptr = staging.ptr as *mut T;
        let pixels = pixels.as_ptr() as *const T;

        let channels = 4;
        let elements_per_row = (self.width as usize) * channels;
        let rows_per_chunk =
            ((staging.bytes as usize) / (elements_per_row * std::mem::size_of::<T>())) as u32;

        assert!(
            rows_per_chunk > 0,
            "Image width is too large for staging buffer"
        );

        let mut current_y = 0;

        while current_y < self.height {
            let chunk_height = std::cmp::min(rows_per_chunk, self.height - current_y);
            let chunk_pixel_count = (self.width * chunk_height * 4) as usize;

            unsafe {
                let src_offset = (current_y * self.width * 4) as usize;
                // let dst_ptr = ptr.add((current_y * self.width * 4) as usize);

                ptr::copy_nonoverlapping(pixels.add(src_offset), ptr, chunk_pixel_count);
            }

            let cmd = vk.begin_single_use_cmd();

            if self.layout() != vk::ImageLayout::TRANSFER_DST_OPTIMAL {
                let barrier = self.barrier(
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    vk::AccessFlags::empty(),
                    vk::AccessFlags::TRANSFER_WRITE,
                );

                unsafe {
                    vk.device.cmd_pipeline_barrier(
                        cmd,
                        vk::PipelineStageFlags::TOP_OF_PIPE,
                        vk::PipelineStageFlags::TRANSFER,
                        vk::DependencyFlags::empty(),
                        &[],
                        &[],
                        &[barrier],
                    );
                }
            }

            let image_subresource = vk::ImageSubresourceLayers {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            };

            let region = vk::BufferImageCopy {
                buffer_offset: 0,
                buffer_row_length: 0,
                buffer_image_height: 0,
                image_subresource,
                image_offset: vk::Offset3D {
                    x: 0,
                    y: current_y as i32,
                    z: 0,
                },
                image_extent: vk::Extent3D {
                    width: self.width,
                    height: chunk_height,
                    depth: 1,
                },
            };

            unsafe {
                vk.device.cmd_copy_buffer_to_image(
                    cmd,
                    staging_buffer,
                    self.image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[region],
                );
            }

            vk.end_single_use_cmd(cmd);

            current_y += chunk_height;
        }

        let cmd = vk.begin_single_use_cmd();

        if self.layout() != vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL {
            let barrier = self.barrier(
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                vk::AccessFlags::TRANSFER_WRITE,
                vk::AccessFlags::SHADER_READ,
            );

            unsafe {
                vk.device.cmd_pipeline_barrier(
                    cmd,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::FRAGMENT_SHADER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[barrier],
                );
            }
        }

        vk.end_single_use_cmd(cmd);

        // println!("set pixels in {}s", start_time.elapsed().as_secs_f32());
    }

    pub fn read_pixels(&mut self, vk: &VulkanContext, dst: &mut Vec<f32>, staging: &Buffer) {
        let start_time = std::time::Instant::now();

        let staging_buffer = staging.buffer;

        dst.clear();
        dst.reserve((self.width * self.height * 4) as usize);

        let ptr = staging.ptr as *mut f32;

        let bytes_per_row = (self.width * self.pixel_size() as u32) as vk::DeviceSize;
        let rows_per_chunk = (staging.bytes / bytes_per_row) as u32;

        assert!(
            rows_per_chunk > 0,
            "Image width is too large! A single row ({:.2} MiB) exceeds the staging buffer limit ({:.2} MiB).",
            bytes_per_row as f64 / (1024.0 * 1024.0),
            staging.bytes as f64 / (1024.0 * 1024.0)
        );

        // for chunk_index in 0..copy_chunks {
        //     let y_offset = chunk_index * chunk_height;

        //     let cmd = vk.begin_single_use_cmd();

        //     let previous_layout = self.layout();

        //     if previous_layout != vk::ImageLayout::TRANSFER_SRC_OPTIMAL {
        //         let barrier = self.barrier(
        //             vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
        //             vk::AccessFlags::SHADER_WRITE,
        //             vk::AccessFlags::TRANSFER_READ,
        //         );

        //         unsafe {
        //             vk.device.cmd_pipeline_barrier(
        //                 cmd,
        //                 vk::PipelineStageFlags::COMPUTE_SHADER,
        //                 vk::PipelineStageFlags::TRANSFER,
        //                 vk::DependencyFlags::empty(),
        //                 &[],
        //                 &[],
        //                 &[barrier],
        //             )
        //         };
        //     }

        //     let image_subresource = vk::ImageSubresourceLayers {
        //         aspect_mask: vk::ImageAspectFlags::COLOR,
        //         mip_level: 0,
        //         base_array_layer: 0,
        //         layer_count: 1,
        //     };

        //     let region = vk::BufferImageCopy {
        //         buffer_offset: 0,
        //         buffer_row_length: 0,
        //         buffer_image_height: 0,
        //         image_subresource,
        //         image_offset: vk::Offset3D {
        //             x: 0,
        //             y: y_offset as i32,
        //             z: 0,
        //         },
        //         image_extent: vk::Extent3D {
        //             width: self.width,
        //             height: chunk_height,
        //             depth: 1,
        //         },
        //     };

        //     unsafe {
        //         vk.device.cmd_copy_image_to_buffer(
        //             cmd,
        //             self.image,
        //             vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
        //             staging_buffer,
        //             &[region],
        //         )
        //     };

        //     vk.end_single_use_cmd(cmd);

        //     unsafe {
        //         let chunk_slice = slice::from_raw_parts(ptr, chunk_pixel_count);
        //         dst.extend_from_slice(chunk_slice);
        //     }
        // }

        let mut current_y = 0;

        while current_y < self.height {
            let current_chunk_height = std::cmp::min(rows_per_chunk, self.height - current_y);
            let chunk_pixel_count = (self.width * current_chunk_height * 4) as usize;

            let cmd = vk.begin_single_use_cmd();

            if self.layout() != vk::ImageLayout::TRANSFER_SRC_OPTIMAL {
                let barrier = self.barrier(
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    vk::AccessFlags::SHADER_WRITE,
                    vk::AccessFlags::TRANSFER_READ,
                );

                unsafe {
                    vk.device.cmd_pipeline_barrier(
                        cmd,
                        vk::PipelineStageFlags::COMPUTE_SHADER,
                        vk::PipelineStageFlags::TRANSFER,
                        vk::DependencyFlags::empty(),
                        &[],
                        &[],
                        &[barrier],
                    )
                };
            }

            let image_subresource = vk::ImageSubresourceLayers {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            };

            let region = vk::BufferImageCopy {
                buffer_offset: 0,
                buffer_row_length: 0,
                buffer_image_height: 0,
                image_subresource,
                image_offset: vk::Offset3D {
                    x: 0,
                    y: current_y as i32,
                    z: 0,
                },
                image_extent: vk::Extent3D {
                    width: self.width,
                    height: current_chunk_height,
                    depth: 1,
                },
            };

            unsafe {
                vk.device.cmd_copy_image_to_buffer(
                    cmd,
                    self.image,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    staging_buffer,
                    &[region],
                )
            };

            vk.end_single_use_cmd(cmd);

            unsafe {
                let chunk_slice = slice::from_raw_parts(ptr, chunk_pixel_count);
                dst.extend_from_slice(chunk_slice);
            }

            current_y += current_chunk_height;
        }

        let now = std::time::Instant::now();
        let elapsed = now.duration_since(start_time).as_secs_f32();
        println!("read pixels in {}s", elapsed);
    }

    pub fn barrier<'a>(
        &'a mut self,
        new_layout: vk::ImageLayout,
        src_access_mask: vk::AccessFlags,
        dst_access_mask: vk::AccessFlags,
    ) -> vk::ImageMemoryBarrier<'a> {
        #[cfg(debug_assertions)]
        if self.layout == new_layout {
            panic!(
                "texture {:#x} layout already correct: {:?} -> {:?}",
                self.image().as_raw(),
                self.layout,
                new_layout
            )
        }

        let subresource_range = vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        };

        let barrier = vk::ImageMemoryBarrier {
            src_access_mask,
            dst_access_mask,
            old_layout: self.layout(),
            new_layout,
            image: self.image,
            subresource_range,
            ..Default::default()
        };

        self.layout = new_layout;

        barrier
    }

    pub fn null() -> Self {
        Self {
            format: vk::Format::UNDEFINED,
            width: 0,
            height: 0,
            layout: vk::ImageLayout::UNDEFINED,
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            view: vk::ImageView::null(),
            bytes: 0,
            name: String::new(),
        }
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

    pub fn image(&self) -> vk::Image {
        self.image
    }

    pub fn memory(&self) -> vk::DeviceMemory {
        self.memory
    }

    pub fn view(&self) -> vk::ImageView {
        self.view
    }

    pub fn set_layout(&mut self, layout: vk::ImageLayout) {
        self.layout = layout;
    }
}

/// Encodes three non-negative floats into the packed R11G11B10_FLOAT format,
/// returned as little-endian bytes of the packed u32.
///
/// Layout (LSB to MSB): R[10:0], G[10:0], B[9:0]
/// R and G each have 5 exponent bits + 6 mantissa bits.
/// B has 5 exponent bits + 5 mantissa bits.
/// All three are unsigned (no sign bit) — negative inputs clamp to 0.
#[allow(dead_code)]
pub fn encode_r11g11b10(r: f32, g: f32, b: f32) -> [u8; 4] {
    let packed = f32_to_ufloat(r, 6) | (f32_to_ufloat(g, 6) << 11) | (f32_to_ufloat(b, 5) << 22);
    packed.to_le_bytes()
}

/// Converts an f32 to an unsigned mini-float with 5 exponent bits (bias 15)
/// and `mantissa_bits` mantissa bits (6 for R/G, 5 for B).
fn f32_to_ufloat(value: f32, mantissa_bits: u32) -> u32 {
    const EXP_BITS: u32 = 5;
    const BIAS: i32 = 15;
    const MAX_EXP: i32 = (1 << EXP_BITS) - 1; // 31

    let mantissa_mask = (1u32 << mantissa_bits) - 1;

    if value.is_nan() {
        return (MAX_EXP as u32) << mantissa_bits | 1; // NaN
    }
    if value <= 0.0 {
        return 0; // negative and zero clamp to 0
    }
    if value.is_infinite() {
        return (MAX_EXP as u32) << mantissa_bits; // Inf
    }

    let bits = value.to_bits();
    let f32_exp = ((bits >> 23) & 0xFF) as i32 - 127;
    let f32_mantissa = bits & 0x7FFFFF;
    let full_mantissa = (1u64 << 23) | f32_mantissa as u64; // implicit leading 1

    let mut exp = f32_exp + BIAS;
    let shift = 23 - mantissa_bits as i32;

    if exp >= MAX_EXP {
        // Overflow -> saturate to infinity
        return (MAX_EXP as u32) << mantissa_bits;
    }

    if exp <= 0 {
        // Subnormal (or underflow to zero)
        let extra_shift = shift - exp + 1;
        if extra_shift >= 64 {
            return 0;
        }
        let mantissa = round_shift(full_mantissa, extra_shift as u32);
        return (mantissa as u32) & mantissa_mask;
    }

    // Normalized case
    let mut mantissa = round_shift(full_mantissa, shift as u32);

    // Rounding may have carried the mantissa into the next power of two
    if mantissa > mantissa_mask as u64 {
        mantissa = 0;
        exp += 1;
        if exp >= MAX_EXP {
            return (MAX_EXP as u32) << mantissa_bits; // rounded up to infinity
        }
    }

    ((exp as u32) << mantissa_bits) | (mantissa as u32 & mantissa_mask)
}

/// Shifts `value` right by `shift` bits, rounding to nearest-even.
fn round_shift(value: u64, shift: u32) -> u64 {
    if shift == 0 {
        return value;
    }
    if shift >= 64 {
        return 0;
    }
    let half = 1u64 << (shift - 1);
    let mask = (1u64 << shift) - 1;
    let truncated = value >> shift;
    let remainder = value & mask;

    if remainder > half || (remainder == half && (truncated & 1) == 1) {
        truncated + 1
    } else {
        truncated
    }
}
