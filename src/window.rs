//! Standalone GLFW preview window, only built for tests or with the `glfw` feature

use std::{
    ffi::{c_char, c_int},
    fmt::Write,
    ptr,
};

use ash::vk::{self, Handle};
use glfw_sys::*;

use crate::{
    preview::{HostFrame, PreviewHost, PreviewInput, PreviewStats, keys},
    vulkan_context::{VulkanConfig, VulkanContext},
};

pub struct GlfwHost {
    window: *mut GLFWwindow,
    last_cursor_pos: Option<(f64, f64)>,
    title: String,
    image_index: u32,
    image_available_semaphore: vk::Semaphore,
    fence: vk::Fence,
}

impl GlfwHost {
    /// Creates the window and adds the required instance extensions, call before creating the Vulkan instance
    pub fn new(width: u32, height: u32, vulkan_config: &mut VulkanConfig) -> Self {
        let window = unsafe {
            let init = glfwInit();
            assert!(init == GLFW_TRUE);

            glfwWindowHint(GLFW_CLIENT_API, GLFW_NO_API);
            glfwWindowHint(GLFW_RESIZABLE, GLFW_TRUE);

            let window = glfwCreateWindow(
                width as c_int,
                height as c_int,
                c"Preview".as_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert!(!window.is_null(), "failed to create GLFW window");

            let mut count: u32 = 0;
            let extensions = glfwGetRequiredInstanceExtensions(&mut count);
            for i in 0..count {
                vulkan_config
                    .window_extensions
                    .push(*extensions.add(i as usize));
            }

            window
        };

        #[cfg(target_os = "windows")]
        {
            let hwnd = unsafe { glfwGetWin32Window(window) };
            dark_titlebar::set_dark_titlebar(hwnd);
        }

        Self {
            window,
            last_cursor_pos: None,
            title: String::new(),
            image_index: 0,
            image_available_semaphore: vk::Semaphore::null(),
            fence: vk::Fence::null(),
        }
    }

    pub fn create_surface(&self, instance: &ash::Instance) -> vk::SurfaceKHR {
        unsafe {
            let raw_instance = instance.handle().as_raw() as VkInstance;
            let mut surface: VkSurfaceKHR = ptr::null_mut();
            glfwCreateWindowSurface(raw_instance, self.window, ptr::null(), &mut surface);
            glfwSetWindowAttrib(self.window, GLFW_FLOATING, GLFW_TRUE);
            vk::SurfaceKHR::from_raw(surface as u64)
        }
    }

    fn key(&self, key: c_int) -> bool {
        unsafe { glfwGetKey(self.window, key) == GLFW_PRESS }
    }
}

impl PreviewHost for GlfwHost {
    fn begin_frame(&mut self) -> HostFrame {
        let window = self.window;
        let mut input = PreviewInput::default();

        unsafe {
            glfwPollEvents();

            if self.key(GLFW_KEY_ESCAPE) {
                glfwSetWindowShouldClose(window, 1);
            }

            const MAPPING: [(c_int, u32); 10] = [
                (GLFW_KEY_W, keys::FORWARD),
                (GLFW_KEY_S, keys::BACK),
                (GLFW_KEY_A, keys::LEFT),
                (GLFW_KEY_D, keys::RIGHT),
                (GLFW_KEY_Q, keys::DOWN),
                (GLFW_KEY_E, keys::UP),
                (GLFW_KEY_LEFT_SHIFT, keys::FAST),
                (GLFW_KEY_LEFT_CONTROL, keys::SLOW),
                (GLFW_KEY_R, keys::RESTART),
                (GLFW_KEY_P, keys::PRINT_CAMERA),
            ];
            for (key, flag) in MAPPING {
                if self.key(key) {
                    input.keys |= flag;
                }
            }

            let mut pos = (0.0f64, 0.0f64);
            glfwGetCursorPos(window, &mut pos.0, &mut pos.1);

            input.looking = glfwGetMouseButton(window, GLFW_MOUSE_BUTTON_RIGHT) == GLFW_PRESS;
            if input.looking {
                glfwSetInputMode(window, GLFW_CURSOR, GLFW_CURSOR_DISABLED);
                if let Some((last_x, last_y)) = self.last_cursor_pos {
                    input.look_delta_x = (pos.0 - last_x) as f32;
                    input.look_delta_y = (pos.1 - last_y) as f32;
                }
            } else {
                glfwSetInputMode(window, GLFW_CURSOR, GLFW_CURSOR_NORMAL);
            }
            // always track so there's no jump when looking starts
            self.last_cursor_pos = Some(pos);

            HostFrame {
                input,
                close: glfwWindowShouldClose(window) != 0,
                settings: None,
            }
        }
    }

    fn size(&self, vk: &VulkanContext) -> (u32, u32) {
        let extent = vk.swapchain.extent;
        (extent.width, extent.height)
    }

    fn acquire(
        &mut self,
        vk: &mut VulkanContext,
        width: u32,
        height: u32,
    ) -> Option<vk::CommandBuffer> {
        let frame = &vk.swapchain.frames[vk.swapchain.frame_index];
        let fence = frame.fence;
        let cmd = frame.command_buffer;
        let image_available_semaphore = frame.image_available_semaphore;

        unsafe {
            vk.device.wait_for_fences(&[fence], true, u64::MAX).unwrap();
        }

        let result = unsafe {
            vk.swapchain_device.acquire_next_image(
                vk.swapchain.swapchain,
                u64::MAX,
                image_available_semaphore,
                vk::Fence::null(),
            )
        };

        match result {
            Ok((image_index, _suboptimal)) => {
                unsafe { vk.device.reset_fences(&[fence]).unwrap() };
                self.image_index = image_index;
                self.image_available_semaphore = image_available_semaphore;
                self.fence = fence;
                Some(cmd)
            }
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                unsafe { vk.device.device_wait_idle().unwrap() };
                vk.create_swapchain(width, height);
                None
            }
            Err(e) => panic!("acquire failed: {e}"),
        }
    }

    fn record_output(
        &mut self,
        vk: &VulkanContext,
        cmd: vk::CommandBuffer,
        src: vk::Image,
        width: u32,
        height: u32,
        _changed: bool,
    ) {
        let device = &vk.device;
        let swapchain_image = vk.swapchain.frames[self.image_index as usize].image;

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

        unsafe {
            let to_dst = vk::ImageMemoryBarrier {
                src_access_mask: vk::AccessFlags::empty(),
                dst_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                old_layout: vk::ImageLayout::UNDEFINED,
                new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                image: swapchain_image,
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
                swapchain_image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[blit],
                vk::Filter::NEAREST,
            );

            let to_present = vk::ImageMemoryBarrier {
                src_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                dst_access_mask: vk::AccessFlags::empty(),
                old_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                new_layout: vk::ImageLayout::PRESENT_SRC_KHR,
                image: swapchain_image,
                subresource_range,
                ..Default::default()
            };
            device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[to_present],
            );
        }
    }

    fn submit(
        &mut self,
        vk: &mut VulkanContext,
        cmd: vk::CommandBuffer,
        width: u32,
        height: u32,
    ) -> bool {
        let render_finished_semaphore =
            vk.swapchain.frames[self.image_index as usize].render_finished_semaphore;

        let wait_dst_stage_mask = [vk::PipelineStageFlags::TRANSFER];
        let cmds = [cmd];
        let wait_semaphores = [self.image_available_semaphore];
        let signal_semaphores = [render_finished_semaphore];
        let submit_info = vk::SubmitInfo::default()
            .command_buffers(&cmds)
            .wait_semaphores(&wait_semaphores)
            .signal_semaphores(&signal_semaphores)
            .wait_dst_stage_mask(&wait_dst_stage_mask);

        unsafe {
            vk.device
                .queue_submit(vk.compute_queue, &[submit_info], self.fence)
                .unwrap();
        }

        let swapchains = [vk.swapchain.swapchain];
        let image_indices = [self.image_index];
        let present_info = vk::PresentInfoKHR::default()
            .wait_semaphores(&signal_semaphores)
            .swapchains(&swapchains)
            .image_indices(&image_indices);

        match unsafe {
            vk.swapchain_device
                .queue_present(vk.present_queue, &present_info)
        } {
            Ok(_) => {}
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                unsafe { vk.device.device_wait_idle().unwrap() };
                vk.create_swapchain(width, height);
                return false;
            }
            Err(e) => panic!("present failed: {e}"),
        }

        vk.swapchain.frame_index = (vk.swapchain.frame_index + 1) % vk.swapchain.frames.len();
        true
    }

    fn end_frame(&mut self, stats: &PreviewStats) {
        let (mut w, mut h) = (0, 0);
        unsafe { glfwGetWindowSize(self.window, &mut w, &mut h) };

        self.title.clear();
        let _ = write!(
            self.title,
            "Preview | Hardware RT: {} | {}x{} | Lights {} | Sample {}/{}\0",
            stats.hardware_rt, w, h, stats.light_count, stats.sample_index, stats.sample_count
        );

        unsafe { glfwSetWindowTitle(self.window, self.title.as_ptr().cast::<c_char>()) };
    }

    fn destroy(&mut self, _vk: &mut VulkanContext) {
        if self.window.is_null() {
            return;
        }
        unsafe {
            glfwPollEvents();
            glfwDestroyWindow(self.window);
            // glfwTerminate();
        }
        self.window = ptr::null_mut();
    }
}

#[cfg(target_os = "windows")]
mod dark_titlebar {
    use std::ffi::c_void;

    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: *mut c_void,
            dw_attribute: u32,
            pv_attribute: *const c_void,
            cb_attribute: u32,
        ) -> i32;
    }

    const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;

    pub fn set_dark_titlebar(hwnd: *mut c_void) {
        unsafe {
            let value: u32 = 1;
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &value as *const u32 as *const c_void,
                std::mem::size_of::<u32>() as u32,
            );
        }
    }
}
