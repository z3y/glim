use std::{
    ffi::{CStr, c_int},
    ptr,
};

use glfw_sys::*;

use crate::{Stilb, StilbConfig, math::Vector3, vulkan_context::VulkanConfig};

pub fn create_window(width: u32, height: u32) -> *mut GLFWwindow {
    const TITLE: &CStr = c"Lightmap Preview";
    let width = width as c_int;
    let height = height as c_int;

    let window = unsafe {
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
    };

    #[cfg(target_os = "windows")]
    {
        let hwnd = unsafe { glfwGetWin32Window(window) };
        dark_titlebar::set_dark_titlebar(hwnd);
    }

    window
}

pub fn initialize_window(
    config: &StilbConfig,
    vulkan_config: &mut VulkanConfig,
) -> *mut GLFWwindow {
    let mut window = ptr::null_mut();
    if vulkan_config.enable_window {
        window = create_window(
            config.preview_settings.width,
            config.preview_settings.height,
        );

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

pub fn update_camera(app: &mut Stilb, delta_time: f32) {
    let window = app.window;
    let camera = &mut app.camera;

    let mut move_speed = 5.0 * delta_time;
    let mouse_sensitivity = 0.0025;

    let mut camera_moved = false;

    let restart;

    unsafe {
        restart = glfwGetKey(window, GLFW_KEY_R) == GLFW_PRESS;

        if glfwGetKey(window, GLFW_KEY_LEFT_SHIFT) == GLFW_PRESS {
            move_speed *= 4.0;
        }
        if glfwGetKey(window, GLFW_KEY_LEFT_CONTROL) == GLFW_PRESS {
            move_speed *= 0.25;
        }

        let right_click_held = glfwGetMouseButton(window, GLFW_MOUSE_BUTTON_RIGHT) == GLFW_PRESS;

        let mut pos_x = 0.0f64;
        let mut pos_y = 0.0f64;
        glfwGetCursorPos(window, &mut pos_x, &mut pos_y);

        if !right_click_held {
            glfwSetInputMode(window, GLFW_CURSOR, GLFW_CURSOR_NORMAL);
            // Always track position while not held so there's no jump when you start holding
            camera.last_cursor_pos = Some((pos_x, pos_y));
        } else {
            glfwSetInputMode(window, GLFW_CURSOR, GLFW_CURSOR_DISABLED);

            if let Some((last_x, last_y)) = camera.last_cursor_pos {
                let dx = (pos_x - last_x) as f32 * mouse_sensitivity;
                let dy = (last_y - pos_y) as f32 * mouse_sensitivity;
                camera.last_cursor_pos = Some((pos_x, pos_y));

                if dx != 0.0 || dy != 0.0 {
                    camera.yaw -= dx;
                    camera.pitch = (camera.pitch + dy).clamp(-1.55334, 1.55334);
                    camera_moved = true;
                }
            }
        }

        let yaw_rad = camera.yaw;
        let pitch_rad = camera.pitch;

        let forward = Vector3::new(
            yaw_rad.sin() * pitch_rad.cos(),
            pitch_rad.sin(),
            yaw_rad.cos() * pitch_rad.cos(),
        )
        .normalize();

        let world_up = Vector3::new(0.0, 1.0, 0.0);
        let right = forward.cross(world_up).normalize();

        let mut direction = Vector3::ZERO;
        if glfwGetKey(window, GLFW_KEY_W) == GLFW_PRESS {
            direction = direction + forward;
            camera_moved = true;
        }
        if glfwGetKey(window, GLFW_KEY_S) == GLFW_PRESS {
            direction = direction - forward;
            camera_moved = true;
        }
        if glfwGetKey(window, GLFW_KEY_D) == GLFW_PRESS {
            direction = direction + right;
            camera_moved = true;
        }
        if glfwGetKey(window, GLFW_KEY_A) == GLFW_PRESS {
            direction = direction - right;
            camera_moved = true;
        }
        if glfwGetKey(window, GLFW_KEY_E) == GLFW_PRESS {
            direction = direction + Vector3::UP;
            camera_moved = true;
        }
        if glfwGetKey(window, GLFW_KEY_Q) == GLFW_PRESS {
            direction = direction - Vector3::UP;
            camera_moved = true;
        }

        if camera_moved {
            camera.position = camera.position + direction.normalize() * move_speed;
        }

        if glfwGetKey(window, GLFW_KEY_P) == GLFW_PRESS {
            println!(
                "Camera Forward: {:#?} Camera Position: {:#?}",
                camera.get_forward(),
                camera.position
            );
        }
    }

    if camera_moved || restart {
        app.preview_initialized = false;
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
