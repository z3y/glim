use std::ptr;

use ash::vk::{self, Handle};

use glfw_sys::{GLFWwindow, glfwCreateWindowSurface};

use crate::{
    mesh::{FfiMesh, Mesh},
    vulkan_core::{VulkanConfig, VulkanContext},
    window::{initialize_window, platform_loop},
};

mod bmp;
mod compute_shader;
mod graphics_shader;
mod math;
mod mesh;
mod test;
mod texture2d;
mod vulkan_cmd;
mod vulkan_core;
mod vulkan_swapchain;
mod window;

pub fn blit_with_shader(vk: &VulkanContext, cmd: vk::CommandBuffer, image: vk::ImageView) {

    // vk.device.bindre
    // transition to general
}

pub struct Stilb {
    pub vk: VulkanContext,
    pub meshes: Vec<Mesh>,
    pub window: *mut GLFWwindow,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct StilbConfig {
    is_preview: u8,
    preview_width: u32,
    preview_height: u32,
}

#[unsafe(no_mangle)]
pub extern "C" fn initialize(config: StilbConfig) -> *mut Stilb {
    let is_debug = cfg!(debug_assertions);

    let mut vulkan_config = VulkanConfig {
        enable_validation_layers: is_debug,
        enable_window: config.is_preview != 0,
        window_extensions: Vec::new(),
    };

    let window = initialize_window(&config, &mut vulkan_config);

    let create_surface_callback = |instance: &ash::Instance| unsafe {
        let instance = instance.handle().as_raw() as glfw_sys::VkInstance;
        let mut surface: glfw_sys::VkSurfaceKHR = ptr::null_mut();
        glfwCreateWindowSurface(instance, window, std::ptr::null(), &mut surface);
        ash::vk::SurfaceKHR::from_raw(surface as u64)
    };

    let mut vk = VulkanContext::new(&vulkan_config, create_surface_callback);
    println!("Vulkan Initialized");

    if config.is_preview != 0 {
        vk.create_swapchain(config.preview_width, config.preview_height);
    }

    let stilb = Stilb {
        vk,
        meshes: Vec::new(),
        window: window,
    };

    Box::into_raw(Box::new(stilb))
}

#[unsafe(no_mangle)]
pub extern "C" fn deinitialize(stilb: *mut Stilb) {
    if !stilb.is_null() {
        // Take ownership back from the pointer and let Box drop it
        let _ = unsafe { Box::from_raw(stilb) };
        println!("Stilb destroyed");
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn add_mesh(stilb: *mut Stilb, raw: FfiMesh) {
    unsafe {
        let stilb_obj = &mut *stilb;

        let mesh = Mesh::from_ffi_mesh(raw);

        // println!("Added mesh: {:#?}", mesh);

        stilb_obj.meshes.push(mesh);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn run(stilb: *mut Stilb) {
    let stilb = unsafe { &mut *stilb };

    platform_loop(stilb.window);
}
