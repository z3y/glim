use std::{ptr, slice, time::Duration};

use ash::vk::{self, Handle};

use glfw_sys::{
    GLFW_KEY_ESCAPE, GLFW_PRESS, GLFWwindow, glfwCreateWindowSurface, glfwGetKey, glfwPollEvents,
    glfwSetWindowShouldClose, glfwWindowShouldClose,
};

use crate::{
    bmp::save_bmp,
    camera::Camera,
    compute_shader::{
        BakePushConstants, ComputeShader, load_bake_lights_shader, load_init_from_camera_shader,
        update_bake_lights_shader, update_init_from_camera_shader,
    },
    graphics_shader::{VisibilityPushConstants, create_visibility_shader},
    lights::{GpuLights, Light},
    math::Vector3,
    mesh::{FfiMesh, GpuMesh, Mesh, VulkanAs, create_tlas},
    texture2d::Texture2D,
    vulkan_context::{VulkanConfig, VulkanContext},
    window::{initialize_window, update_camera},
};

mod bmp;
mod camera;
mod compute_shader;
mod graphics_shader;
mod lights;
mod math;
mod mesh;
mod test;
mod texture2d;
mod vulkan_cmd;
mod vulkan_context;
mod vulkan_swapchain;
mod window;

pub struct Stilb {
    pub config: StilbConfig,
    pub vk: VulkanContext,
    pub window: *mut GLFWwindow,

    pub group_settings: Vec<LightmapSettings>,
    pub cpu_meshes: Vec<Mesh>,
    pub cpu_lights: Vec<Light>,

    pub gpu_mesh: GpuMesh,
    pub gpu_lights: GpuLights,
    pub tlas: VulkanAs,

    pub camera: Camera,

    pub bake_shader: ComputeShader,
    pub init_from_camera_shader: ComputeShader,
    // pub bake_init_shader: ComputeShader,
    pub preview_initialized: bool,

    pub sampler_linear_clamp: vk::Sampler,

    pub push: BakePushConstants,
}

#[repr(C)]
#[derive(Clone, Debug)]
pub struct LightmapSettings {
    pub width: u32,
    pub height: u32,

    pub max_samples: u32,
    pub bounce_count: u32,

    pub denoise: bool,

    pub emission_pixels: *const f32,
    pub emission_pixels_length: u32,
}

pub struct LightmapGroup {
    pub settings: LightmapSettings,

    pub albedo: Texture2D,
    pub emission: Texture2D,

    pub visibility: Texture2D,
    pub diffuse_lightmap: Texture2D,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct StilbConfig {
    is_preview: bool,
    preview_width: u32,
    preview_height: u32,
}

#[inline]
pub fn as_bytes<T>(v: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v as *const T as *const u8, std::mem::size_of::<T>()) }
}

// pub fn blit_with_shader(vk: &VulkanContext, cmd: vk::CommandBuffer, image: vk::ImageView) {
// }

fn init_from_bake(app: &mut Stilb, width: u32, height: u32) -> Texture2D {
    let vk = &mut app.vk;
    let mesh = &app.gpu_mesh;

    let visibility = Texture2D::new(
        vk,
        width,
        height,
        vk::Format::R32G32B32A32_SFLOAT,
        vk::ImageUsageFlags::STORAGE
            | vk::ImageUsageFlags::TRANSFER_SRC
            | vk::ImageUsageFlags::TRANSFER_DST
            | vk::ImageUsageFlags::SAMPLED
            | vk::ImageUsageFlags::COLOR_ATTACHMENT,
    );

    let mut shader = create_visibility_shader(vk, &visibility);

    let cmd = vk.begin_single_use_cmd();

    let clear_values = [vk::ClearValue {
        color: vk::ClearColorValue {
            float32: [0.0, 0.0, 0.0, 0.0],
        },
    }];

    let mut render_pass_begin = vk::RenderPassBeginInfo {
        render_pass: shader.render_pass,
        framebuffer: shader.framebuffer,
        render_area: vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: vk::Extent2D {
                width: visibility.width(),
                height: visibility.height(),
            },
        },
        ..Default::default()
    };

    render_pass_begin = render_pass_begin.clear_values(&clear_values);

    let push = VisibilityPushConstants {
        vertices: mesh.vertex_address(),
        indices: mesh.index_address(),
        width: visibility.width(),
        height: visibility.height(),
        pad0: 0,
        pad1: 0,
    };

    let constants_bytes = as_bytes(&push);

    unsafe {
        vk.device
            .cmd_begin_render_pass(cmd, &render_pass_begin, vk::SubpassContents::INLINE);
        vk.device
            .cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, shader.pipeline);

        vk.device.cmd_push_constants(
            cmd,
            shader.pipeline_layout,
            vk::ShaderStageFlags::GEOMETRY
                | vk::ShaderStageFlags::FRAGMENT
                | vk::ShaderStageFlags::VERTEX,
            0,
            &constants_bytes,
        );

        vk.device.cmd_draw(cmd, mesh.index_len, 25, 0, 0);

        vk.device.cmd_end_render_pass(cmd);
    }
    vk.end_single_use_cmd(cmd);

    shader.destroy(vk);

    visibility
}

fn init_from_camera(app: &mut Stilb, width: u32, height: u32) -> Texture2D {
    let vk = &mut app.vk;
    let shader = &app.init_from_camera_shader;

    let visibility = Texture2D::new(
        vk,
        width,
        height,
        vk::Format::R32G32B32A32_SFLOAT,
        vk::ImageUsageFlags::STORAGE
            | vk::ImageUsageFlags::TRANSFER_SRC
            | vk::ImageUsageFlags::TRANSFER_DST
            | vk::ImageUsageFlags::SAMPLED,
    );

    update_init_from_camera_shader(vk, shader, app.tlas.acceleration_structure(), &visibility);
    visibility
}

fn rasterize_visibility_from_camera(
    app: &mut Stilb,
    visibility: &mut Texture2D,
    cmd: vk::CommandBuffer,
) {
    let width = app.config.preview_width;
    let height = app.config.preview_height;

    let vk = &mut app.vk;
    let shader = &app.init_from_camera_shader;

    let push = app.camera.make_push_constants();

    let constants_bytes = as_bytes(&push);

    unsafe {
        if visibility.layout() != vk::ImageLayout::GENERAL {
            let barrier = visibility.barrier(
                vk::ImageLayout::GENERAL,
                vk::AccessFlags::default(),
                vk::AccessFlags::SHADER_WRITE,
            );
            vk.device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            );
        }

        vk.device
            .cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, shader.pipeline);

        vk.device.cmd_bind_descriptor_sets(
            cmd,
            vk::PipelineBindPoint::COMPUTE,
            shader.pipeline_layout,
            0,
            &[shader.descriptor_set],
            &[],
        );

        vk.device.cmd_push_constants(
            cmd,
            shader.pipeline_layout,
            vk::ShaderStageFlags::COMPUTE,
            0,
            &constants_bytes,
        );

        let groups_x = (width + 7) / 8;
        let groups_y = (height + 7) / 8;
        vk.device.cmd_dispatch(cmd, groups_x, groups_y, 1);
    }

    app.preview_initialized = true;
}

fn clear_texture(
    app: &mut Stilb,
    texture: &mut Texture2D,
    cmd: vk::CommandBuffer,
    clear: vk::ClearColorValue,
) {
    let range = vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    };

    let vk = &app.vk.device;

    unsafe {
        let barrier = texture.barrier(
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            vk::AccessFlags::empty(),
            vk::AccessFlags::TRANSFER_WRITE,
        );

        vk.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[barrier],
        );

        vk.cmd_clear_color_image(
            cmd,
            texture.image(),
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &clear,
            &[range],
        );
    }
}

fn start_bake(app: &mut Stilb, settings: LightmapSettings) {
    assert!(app.cpu_meshes.len() > 0);

    app.gpu_mesh = GpuMesh::new(&app.vk, &app.cpu_meshes[0]);
    app.cpu_meshes = Vec::new();

    let mesh::AccelerationStructureType::RayQuery(blas) = &app.gpu_mesh.acceleration_structure
    else {
        panic!("Expected RayQuery variant");
    };

    app.tlas = create_tlas(&app.vk, blas);

    let group = create_lightmap_group(app, settings);
    bake_lightmap_group(app, group);
}

fn update_push_constants(app: &mut Stilb, settings: &LightmapSettings) {
    let (width, height) = if app.config.is_preview {
        (app.config.preview_width, app.config.preview_height)
    } else {
        (settings.width, settings.height)
    };

    app.push = BakePushConstants {
        vertices: app.gpu_mesh.vertex_address(),
        indices: app.gpu_mesh.index_address(),
        lights: app.gpu_lights.address(),
        lights_count: app.cpu_lights.len() as u32,
        sample_index: 0,
        width: width,
        height: height,
        max_samples: settings.max_samples,
        bounce_count: settings.bounce_count,
    };
}

fn bake_lightmap_group(app: &mut Stilb, group: LightmapGroup) {
    let mut group = group;

    if app.config.is_preview {
        let window = app.window;

        update_push_constants(app, &group.settings);
        app.push.sample_index = 0;

        let mut previous_time = std::time::Instant::now();

        unsafe {
            while glfwWindowShouldClose(window) == 0 {
                glfwPollEvents();

                let now = std::time::Instant::now();

                if glfwGetKey(window, GLFW_KEY_ESCAPE) == GLFW_PRESS {
                    glfwSetWindowShouldClose(window, 1);
                }

                let delta_time = now.duration_since(previous_time).as_secs_f32();

                update_camera(app, delta_time);

                if !app.preview_initialized {
                    app.push.sample_index = 0;
                }

                if app.push.sample_index >= group.settings.max_samples {
                    std::thread::sleep(Duration::from_millis(16));
                }

                if !render_sample_camera(app, &mut group) {
                    destroy_group(&app.vk, &mut group);
                    app.config.preview_width = app.vk.swapchain.extent.width;
                    app.config.preview_height = app.vk.swapchain.extent.height;
                    group = create_lightmap_group(app, group.settings);

                    app.push.sample_index = 0;
                    app.preview_initialized = false;
                    continue;
                }

                #[cfg(debug_assertions)]
                std::thread::sleep(Duration::from_millis(1000 / 100));

                previous_time = now;
            }
        }
    } else {
        let width = group.settings.width;
        let height = group.settings.height;

        update_push_constants(app, &group.settings);

        for i in 0..group.settings.max_samples {
            app.push.sample_index = i as u32;

            let cmd = app.vk.begin_single_use_cmd();
            render_sample(app, cmd, &mut group, width, height);
            app.vk.end_single_use_cmd(cmd);
        }

        let pixels_read = group.diffuse_lightmap.read_pixels(&app.vk);
        save_bmp(
            "../temp/diffuse_lightmap.bmp",
            group.diffuse_lightmap.width(),
            group.diffuse_lightmap.height(),
            &pixels_read,
        )
        .unwrap();
    }

    unsafe {
        app.vk.device.device_wait_idle().unwrap();
    }

    destroy_group(&app.vk, &mut group);
}

fn render_sample_camera(app: &mut Stilb, group: &mut LightmapGroup) -> bool {
    let frame_index = app.vk.swapchain.frame_index;

    let frame = &app.vk.swapchain.frames[frame_index];

    let width = app.config.preview_width;
    let height = app.config.preview_height;

    let vk = &app.vk.device;

    unsafe {
        vk.wait_for_fences(&[frame.fence], true, u64::MAX).unwrap();
        vk.reset_fences(&[frame.fence]).unwrap()
    }

    let (image_index, _is_optimal) = match unsafe {
        app.vk.swapchain_device.acquire_next_image(
            app.vk.swapchain.swapchain,
            u64::MAX,
            frame.image_available_semaphore,
            vk::Fence::null(),
        )
    } {
        Ok(result) => result,
        Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
            unsafe {
                vk.device_wait_idle().unwrap();
            }
            app.vk.create_swapchain(width, height);
            return false;
        }
        Err(e) => panic!("acquire failed: {e}"),
    };

    // assert!(is_optimal);

    // todo: handle is_optimal
    let fence = frame.fence;
    let cmd = frame.command_buffer;
    let image_available_semaphore = frame.image_available_semaphore;

    let begin_info = vk::CommandBufferBeginInfo {
        flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
        ..Default::default()
    };

    let subresource_range = vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    };

    unsafe {
        vk.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())
            .unwrap();

        vk.begin_command_buffer(cmd, &begin_info).unwrap();

        if app.push.sample_index == 0 {
            rasterize_visibility_from_camera(app, &mut group.visibility, cmd);
            app.preview_initialized = true;
            let clear = vk::ClearColorValue {
                float32: [0.0, 0.0, 0.0, 0.0],
            };
            clear_texture(app, &mut group.diffuse_lightmap, cmd, clear);
        }

        let vk = &app.vk.device;

        let barrier = group.diffuse_lightmap.barrier(
            vk::ImageLayout::GENERAL,
            vk::AccessFlags::default(),
            vk::AccessFlags::SHADER_WRITE,
        );
        vk.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[barrier],
        );

        if app.push.sample_index < group.settings.max_samples {
            render_sample(
                app,
                cmd,
                group,
                app.config.preview_width,
                app.config.preview_height,
            );
            app.push.sample_index += 1;
        }

        let swapchain_image = &app.vk.swapchain.frames[image_index as usize];

        let vk = &app.vk.device;

        {
            let barrier = group.diffuse_lightmap.barrier(
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                vk::AccessFlags::SHADER_WRITE,
                vk::AccessFlags::TRANSFER_READ,
            );

            let swapchain_barrier = vk::ImageMemoryBarrier {
                src_access_mask: vk::AccessFlags::empty(),
                dst_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                old_layout: vk::ImageLayout::UNDEFINED,
                new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                image: swapchain_image.image,
                subresource_range,
                ..Default::default()
            };

            vk.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier, swapchain_barrier],
            );
        }

        {
            let offset0 = vk::Offset3D { x: 0, y: 0, z: 0 };
            let offset1 = vk::Offset3D {
                x: width as i32,
                y: height as i32,
                z: 1,
            };

            let blit = vk::ImageBlit {
                src_subresource: vk::ImageSubresourceLayers {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    mip_level: 0,
                    base_array_layer: 0,
                    layer_count: 1,
                },
                src_offsets: [offset0, offset1],
                dst_subresource: vk::ImageSubresourceLayers {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    mip_level: 0,
                    base_array_layer: 0,
                    layer_count: 1,
                },
                dst_offsets: [offset0, offset1],
            };

            vk.cmd_blit_image(
                cmd,
                group.diffuse_lightmap.image(),
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                swapchain_image.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[blit],
                vk::Filter::NEAREST,
            );
        }

        {
            let swapchain_barrier = vk::ImageMemoryBarrier {
                src_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                dst_access_mask: vk::AccessFlags::empty(),
                old_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                new_layout: vk::ImageLayout::PRESENT_SRC_KHR,
                image: swapchain_image.image,
                subresource_range,
                ..Default::default()
            };

            vk.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[swapchain_barrier],
            );
        }

        vk.end_command_buffer(cmd).unwrap();

        let render_finished_semaphore =
            app.vk.swapchain.frames[image_index as usize].render_finished_semaphore;

        let wait_dst_stage_mask = [vk::PipelineStageFlags::TRANSFER];
        let cmds = [cmd];
        let wait_semaphores = [image_available_semaphore];
        let signal_semaphores = [render_finished_semaphore];
        let submit_info = vk::SubmitInfo::default()
            .command_buffers(&cmds)
            .wait_semaphores(&wait_semaphores)
            .signal_semaphores(&signal_semaphores)
            .wait_dst_stage_mask(&wait_dst_stage_mask);

        let submits = [submit_info];
        vk.queue_submit(app.vk.compute_queue, &submits, fence)
            .unwrap();

        let swapchains = [app.vk.swapchain.swapchain];
        let image_indices = [image_index];
        let present_info = vk::PresentInfoKHR::default()
            .wait_semaphores(&signal_semaphores)
            .swapchains(&swapchains)
            .image_indices(&image_indices);

        match {
            app.vk
                .swapchain_device
                .queue_present(app.vk.present_queue, &present_info)
        } {
            Ok(_) => {}
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                vk.device_wait_idle().unwrap();
                app.vk.create_swapchain(width, height);
                return false;
            }
            Err(e) => panic!("present failed: {e}"),
        }
    };

    app.vk.swapchain.frame_index =
        (app.vk.swapchain.frame_index + 1) % app.vk.swapchain.frames.len();

    true
}

fn render_sample(
    app: &mut Stilb,
    cmd: vk::CommandBuffer,
    group: &mut LightmapGroup,
    width: u32,
    height: u32,
) {
    let vk = &app.vk;
    let shader = &app.bake_shader;

    let constants_bytes = as_bytes(&app.push);

    // println!("rendering sample: {}", group.push.sample_index);

    // let barrier = group.diffuse_lightmap.barrier(
    //     vk::ImageLayout::GENERAL,
    //     vk::AccessFlags::default(),
    //     vk::AccessFlags::SHADER_WRITE,
    // );

    // let barrier2 = group.visibility.barrier(
    //     vk::ImageLayout::GENERAL,
    //     vk::AccessFlags::default(),
    //     vk::AccessFlags::SHADER_READ,
    // );

    let groups_x = (width + 7) / 8;
    let groups_y = (height + 7) / 8;

    unsafe {
        // vk.device.cmd_pipeline_barrier(
        //     cmd,
        //     vk::PipelineStageFlags::TOP_OF_PIPE,
        //     vk::PipelineStageFlags::COMPUTE_SHADER,
        //     vk::DependencyFlags::empty(),
        //     &[],
        //     &[],
        //     &[barrier2],
        // );

        vk.device
            .cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, shader.pipeline);

        vk.device.cmd_bind_descriptor_sets(
            cmd,
            vk::PipelineBindPoint::COMPUTE,
            shader.pipeline_layout,
            0,
            &[shader.descriptor_set],
            &[],
        );

        vk.device.cmd_push_constants(
            cmd,
            shader.pipeline_layout,
            vk::ShaderStageFlags::COMPUTE,
            0,
            &constants_bytes,
        );

        vk.device.cmd_dispatch(cmd, groups_x, groups_y, 1);
    }
}

fn create_lightmap_group(app: &mut Stilb, settings: LightmapSettings) -> LightmapGroup {
    let visibility = if app.config.is_preview {
        init_from_camera(app, app.config.preview_width, app.config.preview_height)
    } else {
        init_from_bake(app, settings.width, settings.height)
    };

    // println!("creating lightmap group {:?}", &settings);

    let mut albedo = Texture2D::new(
        &app.vk,
        settings.width,
        settings.height,
        vk::Format::R32G32B32A32_SFLOAT,
        vk::ImageUsageFlags::SAMPLED
            | vk::ImageUsageFlags::TRANSFER_SRC
            | vk::ImageUsageFlags::TRANSFER_DST,
    );

    let mut emission = Texture2D::new(
        &app.vk,
        settings.width,
        settings.height,
        vk::Format::R32G32B32A32_SFLOAT,
        vk::ImageUsageFlags::SAMPLED
            | vk::ImageUsageFlags::TRANSFER_SRC
            | vk::ImageUsageFlags::TRANSFER_DST,
    );

    if settings.emission_pixels_length > 0 {
        let pixels = unsafe {
            slice::from_raw_parts(
                settings.emission_pixels,
                settings.emission_pixels_length as usize,
            )
        };

        emission.set_pixels(&app.vk, pixels);
    }

    let (target_width, target_height) = if app.config.is_preview {
        (app.config.preview_width, app.config.preview_height)
    } else {
        (settings.width, settings.height)
    };

    let diffuse_lightmap = Texture2D::new(
        &app.vk,
        target_width,
        target_height,
        vk::Format::R32G32B32A32_SFLOAT,
        vk::ImageUsageFlags::STORAGE
            | vk::ImageUsageFlags::TRANSFER_SRC
            | vk::ImageUsageFlags::TRANSFER_DST,
    );

    let push = BakePushConstants {
        vertices: app.gpu_mesh.vertex_address(),
        indices: app.gpu_mesh.index_address(),
        lights: app.gpu_lights.address(),
        lights_count: app.cpu_lights.len() as u32,
        sample_index: 0,
        width: target_width,
        height: target_height,
        max_samples: settings.max_samples,
        bounce_count: settings.bounce_count,
    };

    update_bake_lights_shader(
        &app.vk,
        &app.bake_shader,
        app.tlas.acceleration_structure(),
        &visibility,
        &albedo,
        &emission,
        &diffuse_lightmap,
        app.sampler_linear_clamp,
    );

    println!("visibility: {:#x}", visibility.image().as_raw());
    println!("albedo: {:#x}", albedo.image().as_raw());
    println!("emission: {:#x}", emission.image().as_raw());
    println!("diffuse_lightmap: {:#x}", diffuse_lightmap.image().as_raw());

    let cmd = app.vk.begin_single_use_cmd();
    unsafe {
        let clear = vk::ClearColorValue {
            float32: [1.0, 1.0, 1.0, 1.0],
        };
        clear_texture(app, &mut albedo, cmd, clear);

        let barrier = albedo.barrier(
            vk::ImageLayout::GENERAL,
            vk::AccessFlags::default(),
            vk::AccessFlags::SHADER_READ,
        );
        let barrier1 = emission.barrier(
            vk::ImageLayout::GENERAL,
            vk::AccessFlags::default(),
            vk::AccessFlags::SHADER_READ,
        );
        app.vk.device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[barrier, barrier1],
        );
    }

    app.vk.end_single_use_cmd(cmd);

    LightmapGroup {
        settings,
        visibility,
        albedo,
        diffuse_lightmap,
        emission,
    }
}

fn destroy_group(vk: &VulkanContext, group: &mut LightmapGroup) {
    group.albedo.destroy(vk);
    group.diffuse_lightmap.destroy(vk);
    group.visibility.destroy(vk);
    group.emission.destroy(vk);
}

#[unsafe(no_mangle)]
pub extern "C" fn app_initialize(app_config: StilbConfig) -> *mut Stilb {
    let is_debug = cfg!(debug_assertions);

    let mut vulkan_config = VulkanConfig {
        enable_validation_layers: is_debug,
        enable_window: app_config.is_preview,
        window_extensions: Vec::new(),
    };

    let window = initialize_window(&app_config, &mut vulkan_config);

    let create_surface_callback = |instance: &ash::Instance| unsafe {
        let instance = instance.handle().as_raw() as glfw_sys::VkInstance;
        let mut surface: glfw_sys::VkSurfaceKHR = ptr::null_mut();
        glfwCreateWindowSurface(instance, window, std::ptr::null(), &mut surface);
        ash::vk::SurfaceKHR::from_raw(surface as u64)
    };

    let mut vk = VulkanContext::new(&vulkan_config, create_surface_callback);
    println!("Vulkan Initialized");

    if app_config.is_preview {
        vk.create_swapchain(app_config.preview_width, app_config.preview_height);
    }

    let bake_lights_shader = load_bake_lights_shader(&vk, app_config.is_preview);

    let mut camera = Camera {
        position: Vector3::new(0.0, 1.0, -5.0),
        yaw: 0.0,
        pitch: 0.0,
        fov: 60.0,
        last_cursor_pos: None,
    };

    camera.look_at(Vector3::ZERO);

    let init_from_camera_shader = load_init_from_camera_shader(&vk);

    let gpu_lights = GpuLights {
        buffer: vk::Buffer::null(),
        memory: vk::DeviceMemory::null(),
        address: 0,
    };

    let sampler_info = vk::SamplerCreateInfo::default()
        .mag_filter(vk::Filter::LINEAR)
        .min_filter(vk::Filter::LINEAR)
        .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
        .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
        .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
        .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
        .mip_lod_bias(0.0)
        .anisotropy_enable(false)
        .compare_enable(false)
        .min_lod(0.0)
        .max_lod(vk::LOD_CLAMP_NONE)
        .border_color(vk::BorderColor::FLOAT_OPAQUE_BLACK)
        .unnormalized_coordinates(false);

    let sampler_linear_clamp = unsafe { vk.device.create_sampler(&sampler_info, None).unwrap() };

    let push = BakePushConstants {
        vertices: 0,
        indices: 0,
        lights: 0,
        lights_count: 0,
        sample_index: 0,
        width: 0,
        height: 0,
        max_samples: 0,
        bounce_count: 0,
    };

    let app = Stilb {
        vk,
        cpu_meshes: Vec::new(),
        window: window,
        config: app_config,
        cpu_lights: Vec::new(),
        bake_shader: bake_lights_shader,
        gpu_mesh: GpuMesh::null(),
        tlas: VulkanAs::null(),
        group_settings: Vec::new(),
        camera,
        init_from_camera_shader,
        preview_initialized: false,
        gpu_lights,
        sampler_linear_clamp,
        push,
    };

    Box::into_raw(Box::new(app))
}

#[unsafe(no_mangle)]
pub extern "C" fn app_add_mesh(app: *mut Stilb, raw: FfiMesh) {
    let app = unsafe { &mut *app };
    let mesh = Mesh::from_ffi_mesh(raw);
    app.cpu_meshes.push(mesh);
}

#[unsafe(no_mangle)]
pub extern "C" fn app_add_light(app: *mut Stilb, light: Light) {
    let app = unsafe { &mut *app };
    app.cpu_lights.push(light);
}

#[unsafe(no_mangle)]
pub extern "C" fn app_add_lightmap_group(app: *mut Stilb, settings: LightmapSettings) {
    let app = unsafe { &mut *app };
    app.group_settings.push(settings);
}

#[unsafe(no_mangle)]
pub extern "C" fn app_run(app: *mut Stilb) {
    let app = unsafe { &mut *app };

    // assert!(app.cpu_lights.len() > 0);

    let settings = app.group_settings[0].clone();

    if app.cpu_lights.len() > 0 {
        let gpu_lights = GpuLights::new(&app.vk, &app.cpu_lights);
        app.gpu_lights = gpu_lights;
    }

    start_bake(app, settings);
}

#[unsafe(no_mangle)]
pub extern "C" fn app_deinitialize(app: *mut Stilb) {
    if !app.is_null() {
        // Take ownership back from the pointer and let Box drop it
        let mut app = unsafe { Box::from_raw(app) };

        app.bake_shader.destroy(&app.vk);
        app.gpu_mesh.destroy(&app.vk);
        app.tlas.destroy(&app.vk);
        app.init_from_camera_shader.destroy(&app.vk);

        if app.gpu_lights.address != 0 {
            app.gpu_lights.destroy(&app.vk);
        }

        unsafe {
            app.vk
                .device
                .destroy_sampler(app.sampler_linear_clamp, None)
        };

        println!("App destroyed");
    }
}
