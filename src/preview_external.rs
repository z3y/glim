//! Preview host without a window, the image is read back into host visible buffers and
//! the embedding application (Unity) polls for new frames and supplies the input.
//! Works with any graphics API on the consumer side since only CPU memory is shared.

use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use ash::vk::{self, Handle};

use crate::bindings::is_cancelled;
use crate::buffer::Buffer;
use crate::preview::{HostFrame, PreviewHost, PreviewInput, PreviewSettings, PreviewStats};
use crate::texture2d::Texture2D;
use crate::vulkan_context::VulkanContext;

const FRAMES_IN_FLIGHT: usize = 2;
// one published (or locked by the consumer) + one being written.
// if the consumer still holds an older frame while a newer one is published, both are taken and
// that iteration's readback is skipped, rare since the consumer copy takes ~1ms
const READBACK_SLOTS: usize = 2;
const MAX_SIZE: u32 = 8192;

#[derive(Clone, Copy)]
struct Published {
    slot: usize,
    ptr: usize,
    width: u32,
    height: u32,
    id: u64,
}

struct Shared {
    keys: u32,
    looking: bool,
    look_delta: [f32; 2],

    requested_size: (u32, u32),
    settings: Option<PreviewSettings>,

    accepting: bool,
    latest: Option<Published>,
    locked: Option<usize>,
    consumed_id: u64,

    stats: PreviewStats,
}

static SHARED: Mutex<Shared> = Mutex::new(Shared {
    keys: 0,
    looking: false,
    look_delta: [0.0; 2],
    requested_size: (0, 0),
    settings: None,
    accepting: false,
    latest: None,
    locked: None,
    consumed_id: 0,
    stats: PreviewStats {
        sample_index: 0,
        sample_count: 0,
        width: 0,
        height: 0,
        light_count: 0,
        hardware_rt: false,
    },
});

fn shared() -> MutexGuard<'static, Shared> {
    SHARED.lock().unwrap_or_else(|e| e.into_inner())
}

#[repr(C)]
pub struct PreviewFrame {
    /// tightly packed R8G8B8A8 sRGB, first row is the top of the image
    pub pixels: *const u8,
    pub width: u32,
    pub height: u32,
    pub id: u64,
}

/// Sets the held keys and adds the mouse delta, deltas accumulate until the render thread consumes them
#[unsafe(no_mangle)]
pub extern "C" fn app_preview_set_input(input: PreviewInput) {
    let mut s = shared();
    s.keys = input.keys;
    s.looking = input.looking;
    s.look_delta[0] += input.look_delta_x;
    s.look_delta[1] += input.look_delta_y;
}

/// Sample count, bounces and throttle, the accumulation restarts only if samples or bounces changed
#[unsafe(no_mangle)]
pub extern "C" fn app_preview_set_settings(settings: PreviewSettings) {
    shared().settings = Some(settings);
}

#[unsafe(no_mangle)]
pub extern "C" fn app_preview_resize(width: u32, height: u32) {
    shared().requested_size = (width.min(MAX_SIZE), height.min(MAX_SIZE));
}

/// Returns true and locks the frame if there's one newer than the last acquired.
/// `app_preview_release_frame` must be called once done reading `pixels`.
#[unsafe(no_mangle)]
pub extern "C" fn app_preview_acquire_frame(out: *mut PreviewFrame) -> bool {
    if out.is_null() {
        return false;
    }

    let mut s = shared();
    if !s.accepting || s.locked.is_some() {
        return false;
    }

    let Some(latest) = s.latest else {
        return false;
    };

    if latest.id == s.consumed_id {
        return false;
    }

    s.locked = Some(latest.slot);
    s.consumed_id = latest.id;

    unsafe {
        *out = PreviewFrame {
            pixels: latest.ptr as *const u8,
            width: latest.width,
            height: latest.height,
            id: latest.id,
        };
    }

    true
}

#[unsafe(no_mangle)]
pub extern "C" fn app_preview_release_frame() {
    shared().locked = None;
}

#[unsafe(no_mangle)]
pub extern "C" fn app_preview_get_stats(out: *mut PreviewStats) {
    if out.is_null() {
        return;
    }
    unsafe { *out = shared().stats };
}

struct Frame {
    cmd: vk::CommandBuffer,
    fence: vk::Fence,
    readback: Option<usize>,
}

struct Slot {
    buffer: Buffer,
    width: u32,
    height: u32,
    id: u64,
}

pub struct ExternalHost {
    frames: Vec<Frame>,
    frame_index: usize,
    slots: Vec<Slot>,
    // R8G8B8A8_SRGB conversion target, the blit does the linear -> sRGB encode
    image: Texture2D,
    readback_memory: vk::MemoryPropertyFlags,
    default_size: (u32, u32),
    dirty: bool,
    next_id: u64,
}

impl ExternalHost {
    pub fn new(vk: &VulkanContext, width: u32, height: u32) -> Self {
        {
            let mut s = shared();
            s.keys = 0;
            s.looking = false;
            s.look_delta = [0.0; 2];
            s.requested_size = (0, 0);
            s.settings = None;
            s.latest = None;
            s.locked = None;
            s.consumed_id = 0;
            s.stats = PreviewStats::default();
            s.accepting = true;
        }

        let allocate_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(vk.command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(FRAMES_IN_FLIGHT as u32);
        let cmds = unsafe { vk.device.allocate_command_buffers(&allocate_info) }.unwrap();

        let frames = cmds
            .into_iter()
            .map(|cmd| {
                let info = vk::FenceCreateInfo {
                    flags: vk::FenceCreateFlags::SIGNALED,
                    ..Default::default()
                };
                let fence = unsafe { vk.device.create_fence(&info, None) }.unwrap();
                Frame {
                    cmd,
                    fence,
                    readback: None,
                }
            })
            .collect();

        let slots = (0..READBACK_SLOTS)
            .map(|_| Slot {
                buffer: Buffer::null(),
                width: 0,
                height: 0,
                id: 0,
            })
            .collect();

        // cached memory makes the CPU copy on the consumer side a lot faster
        let cached = vk::MemoryPropertyFlags::HOST_VISIBLE
            | vk::MemoryPropertyFlags::HOST_COHERENT
            | vk::MemoryPropertyFlags::HOST_CACHED;
        let readback_memory = if vk.supports_memory_properties(cached) {
            cached
        } else {
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT
        };

        Self {
            frames,
            frame_index: 0,
            slots,
            image: Texture2D::null(),
            readback_memory,
            default_size: (width.max(1), height.max(1)),
            dirty: true,
            next_id: 0,
        }
    }

    fn find_free_slot(&self, s: &Shared) -> Option<usize> {
        (0..self.slots.len()).find(|&i| {
            s.latest.map(|l| l.slot) != Some(i)
                && s.locked != Some(i)
                && self.frames.iter().all(|f| f.readback != Some(i))
        })
    }

    /// Publishes readbacks whose frame finished on the GPU, doesn't block
    fn publish_finished(&mut self, vk: &VulkanContext) {
        for frame in &mut self.frames {
            let Some(index) = frame.readback else {
                continue;
            };

            let done = unsafe { vk.device.get_fence_status(frame.fence) }.unwrap_or(false);
            if !done {
                continue;
            }

            frame.readback = None;

            let slot = &self.slots[index];
            shared().latest = Some(Published {
                slot: index,
                ptr: slot.buffer.ptr as usize,
                width: slot.width,
                height: slot.height,
                id: slot.id,
            });
        }
    }

    fn ensure_resources(&mut self, vk: &VulkanContext, slot: usize, width: u32, height: u32) {
        if self.image.width != width || self.image.height != height || self.image.image.is_null() {
            if !self.image.image.is_null() {
                self.image.destroy(vk);
            }
            self.image = Texture2D::new(
                vk,
                width,
                height,
                vk::Format::R8G8B8A8_SRGB,
                // SAMPLED only because Texture2D always creates a view
                vk::ImageUsageFlags::TRANSFER_DST
                    | vk::ImageUsageFlags::TRANSFER_SRC
                    | vk::ImageUsageFlags::SAMPLED,
                String::from("Preview Readback Image"),
            );
        }

        let bytes = width as u64 * height as u64 * 4;
        let buffer = &mut self.slots[slot].buffer;
        if buffer.buffer.is_null() || buffer.bytes < bytes {
            if !buffer.buffer.is_null() {
                buffer.destroy(vk);
            }
            *buffer = Buffer::empty(
                vk,
                format!("Preview Readback {slot}"),
                bytes,
                vk::BufferUsageFlags::TRANSFER_DST,
                self.readback_memory,
            );
        }
    }
}

impl PreviewHost for ExternalHost {
    fn begin_frame(&mut self) -> HostFrame {
        let mut s = shared();
        let input = PreviewInput {
            keys: s.keys,
            look_delta_x: s.look_delta[0],
            look_delta_y: s.look_delta[1],
            looking: s.looking,
        };
        s.look_delta = [0.0; 2];

        HostFrame {
            input,
            close: is_cancelled(),
            settings: s.settings.take(),
        }
    }

    fn size(&self, _vk: &VulkanContext) -> (u32, u32) {
        let requested = shared().requested_size;
        if requested.0 > 0 && requested.1 > 0 {
            requested
        } else {
            self.default_size
        }
    }

    fn acquire(
        &mut self,
        vk: &mut VulkanContext,
        _width: u32,
        _height: u32,
    ) -> Option<vk::CommandBuffer> {
        let fence = self.frames[self.frame_index].fence;
        unsafe {
            vk.device.wait_for_fences(&[fence], true, u64::MAX).unwrap();
        }

        // publish as soon as the GPU is done instead of when the frame slot comes around again,
        // otherwise every readback reaches the consumer one iteration late
        self.publish_finished(vk);

        let frame = &mut self.frames[self.frame_index];
        unsafe { vk.device.reset_fences(&[frame.fence]).unwrap() };

        Some(frame.cmd)
    }

    fn record_output(
        &mut self,
        vk: &VulkanContext,
        cmd: vk::CommandBuffer,
        src: vk::Image,
        width: u32,
        height: u32,
        changed: bool,
    ) {
        self.dirty |= changed;
        if !self.dirty {
            return;
        }

        // one readback in flight at a time, every changed frame is read back so the consumer
        // always gets the newest one, frames it didn't pick up are simply replaced
        if self.frames.iter().any(|f| f.readback.is_some()) {
            return;
        }

        let slot = {
            let s = shared();
            match self.find_free_slot(&s) {
                Some(slot) => slot,
                None => return,
            }
        };

        // nothing in flight reads the image or the slot (no pending readbacks), safe to reallocate
        self.ensure_resources(vk, slot, width, height);

        let subresource_range = vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        };
        let layers = vk::ImageSubresourceLayers {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        };

        let device = &vk.device;
        let image = self.image.image;
        let buffer = self.slots[slot].buffer.buffer;

        unsafe {
            let to_dst = vk::ImageMemoryBarrier {
                src_access_mask: vk::AccessFlags::TRANSFER_READ,
                dst_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                old_layout: vk::ImageLayout::UNDEFINED,
                new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                image,
                subresource_range,
                ..Default::default()
            };
            device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[to_dst],
            );

            let offset0 = vk::Offset3D { x: 0, y: 0, z: 0 };
            let offset1 = vk::Offset3D {
                x: width as i32,
                y: height as i32,
                z: 1,
            };
            let blit = vk::ImageBlit {
                src_subresource: layers,
                src_offsets: [offset0, offset1],
                dst_subresource: layers,
                dst_offsets: [offset0, offset1],
            };
            device.cmd_blit_image(
                cmd,
                src,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[blit],
                vk::Filter::NEAREST,
            );

            let to_src = vk::ImageMemoryBarrier {
                src_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                dst_access_mask: vk::AccessFlags::TRANSFER_READ,
                old_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                new_layout: vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                image,
                subresource_range,
                ..Default::default()
            };
            device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[to_src],
            );

            let region = vk::BufferImageCopy {
                buffer_offset: 0,
                buffer_row_length: 0,
                buffer_image_height: 0,
                image_subresource: layers,
                image_offset: offset0,
                image_extent: vk::Extent3D {
                    width,
                    height,
                    depth: 1,
                },
            };
            device.cmd_copy_image_to_buffer(
                cmd,
                image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                buffer,
                &[region],
            );

            let to_host = vk::BufferMemoryBarrier {
                src_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                dst_access_mask: vk::AccessFlags::HOST_READ,
                src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                buffer,
                offset: 0,
                size: vk::WHOLE_SIZE,
                ..Default::default()
            };
            device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::HOST,
                vk::DependencyFlags::empty(),
                &[],
                &[to_host],
                &[],
            );
        }

        self.next_id += 1;
        let s = &mut self.slots[slot];
        s.width = width;
        s.height = height;
        s.id = self.next_id;

        self.frames[self.frame_index].readback = Some(slot);
        self.dirty = false;
    }

    fn submit(
        &mut self,
        vk: &mut VulkanContext,
        cmd: vk::CommandBuffer,
        _width: u32,
        _height: u32,
    ) -> bool {
        let frame = &self.frames[self.frame_index];
        let cmds = [cmd];
        let submit_info = vk::SubmitInfo::default().command_buffers(&cmds);

        unsafe {
            vk.device
                .queue_submit(vk.compute_queue, &[submit_info], frame.fence)
                .unwrap();
        }

        self.frame_index = (self.frame_index + 1) % self.frames.len();
        true
    }

    fn end_frame(&mut self, stats: &PreviewStats) {
        shared().stats = *stats;
    }

    fn destroy(&mut self, vk: &mut VulkanContext) {
        unsafe { vk.device.device_wait_idle().unwrap() };

        {
            let mut s = shared();
            s.accepting = false;
            s.latest = None;
        }

        // the consumer copies and releases within one call, never waits on this thread
        while shared().locked.is_some() {
            std::thread::sleep(Duration::from_millis(1));
        }

        for frame in self.frames.drain(..) {
            unsafe {
                vk.device.destroy_fence(frame.fence, None);
                vk.device
                    .free_command_buffers(vk.command_pool, &[frame.cmd]);
            }
        }

        for slot in &mut self.slots {
            if !slot.buffer.buffer.is_null() {
                slot.buffer.destroy(vk);
            }
        }

        if !self.image.image.is_null() {
            self.image.destroy(vk);
        }
    }
}
