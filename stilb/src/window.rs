use std::{
    ffi::{CStr, c_int},
    ptr,
};

use glfw_sys::*;

use crate::{StilbConfig, vulkan_core::VulkanConfig};

pub fn create_window(width: u32, height: u32) -> *mut GLFWwindow {
    const TITLE: &CStr = c"Stilb Preview";
    let width = width as c_int;
    let height = height as c_int;

    unsafe {
        let init = glfwInit();
        assert!(init == GLFW_TRUE);

        glfwWindowHint(GLFW_CLIENT_API, GLFW_NO_API);
        glfwWindowHint(GLFW_RESIZABLE, GLFW_TRUE);

        glfwCreateWindow(
            width,
            height,
            TITLE.as_ptr(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    }
}

pub fn platform_loop(window: *mut GLFWwindow) {
    unsafe {
        while glfwWindowShouldClose(window) == 0 {
            glfwPollEvents();

            if glfwGetKey(window, GLFW_KEY_ESCAPE) == GLFW_PRESS {
                glfwSetWindowShouldClose(window, 1);
                println!("ESC")
            }
        }
    }
}

pub fn initialize_window(
    config: &StilbConfig,
    vulkan_config: &mut VulkanConfig,
) -> *mut GLFWwindow {
    let mut window = ptr::null_mut();
    if vulkan_config.enable_window {
        window = create_window(config.preview_width, config.preview_height);

        unsafe {
            let mut window_extensions_count: u32 = 0;
            let window_extensions = glfwGetRequiredInstanceExtensions(&mut window_extensions_count);

            for i in 0..window_extensions_count {
                let str = *window_extensions.add(i as usize);
                vulkan_config.window_extensions.push(str);
            }
        }
    }
    window
}
