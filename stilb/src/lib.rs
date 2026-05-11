use ash::vk::{self, Handle};
use std::io::{self, Write};
use std::{ptr, time::Duration};

use glfw_sys::{
    GLFW_KEY_ESCAPE, GLFW_PRESS, GLFWwindow, glfwCreateWindowSurface, glfwGetKey, glfwPollEvents,
    glfwSetWindowShouldClose, glfwWindowShouldClose,
};

use crate::sobol::SobolBuffer;
use crate::{
    camera::Camera,
    compute_shader::{
        BakePushConstants, ComputeShader, load_bake_lights_shader, load_init_from_camera_shader,
        update_bake_lights_shader, update_init_from_camera_shader,
    },
    graphics_shader::{VisibilityPushConstants, create_visibility_shader},
    lights::{GpuLights, Light},
    math::Vector3,
    mesh::{GpuMesh, Mesh, VulkanAs, create_tlas},
    oidn::Oidn,
    texture2d::Texture2D,
    vulkan_context::{VulkanConfig, VulkanContext},
    window::{initialize_window, update_camera},
};

mod bindings;
mod bmp;
mod camera;
mod compute_shader;
mod graphics_shader;
mod lights;
mod math;
mod mesh;
mod oidn;
mod sobol;
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

    // pub group_settings: Vec<LightmapSettings>,
    pub cpu_mesh: Mesh,
    pub cpu_lights: Vec<Light>,
    pub groups: Vec<LightmapGroup>,
    pub sobol: SobolBuffer,

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

    pub render_target: RenderTarget,
}

pub enum RenderTarget {
    NonDirectional {
        visibility: Texture2D,
        diffuse: Texture2D,
    },
    None,
}

type ReadbackCallback = extern "C" fn(data: ReadbackData);

#[repr(C)]
pub struct ReadbackData {
    pub group_index: u32,
    pub ty: u32,
    pub width: u32,
    pub height: u32,
    pub pixels: *const f32,
    pub pixels_count: u32,
}

#[repr(C)]
#[derive(Clone, Debug)]
pub struct LightmapSettings {
    pub width: u32,
    pub height: u32,

    pub max_samples: u32,
    pub bounce_count: u32,

    pub denoise: bool,
}

pub struct LightmapGroup {
    pub settings: LightmapSettings,

    pub albedo: Texture2D,
    pub emission: Texture2D,
}

#[repr(u32)]
#[derive(Clone, Copy)]
pub enum CoordinateSystem {
    Default = 0,
    Unity = 1,
}

#[repr(C)]
#[derive(Clone)]
pub struct StilbConfig {
    pub coordinate_system: CoordinateSystem,

    pub is_preview: bool,
    pub throttle_preview_ms: u32,
    pub preview_settings: LightmapSettings,

    pub camera_position: Vector3,
    pub camera_forward: Vector3,

    pub callback: ReadbackCallback,
}

#[inline]
pub fn as_bytes<T>(v: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v as *const T as *const u8, std::mem::size_of::<T>()) }
}

// pub fn blit_with_shader(vk: &VulkanContext, cmd: vk::CommandBuffer, image: vk::ImageView) {
// }

fn render_visibility_buffer_bake(
    app: &mut Stilb,
    width: u32,
    height: u32,
    group_index: u32,
) -> Texture2D {
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
        group_index,
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

fn render_visibility_buffer_camera(app: &mut Stilb, width: u32, height: u32) -> Texture2D {
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

fn rasterize_visibility_from_camera(app: &mut Stilb, cmd: vk::CommandBuffer) {
    let width = app.config.preview_settings.width;
    let height = app.config.preview_settings.height;

    let vk = &mut app.vk;
    let shader = &app.init_from_camera_shader;

    let push = app.camera.make_push_constants();

    let constants_bytes = as_bytes(&push);

    let RenderTarget::NonDirectional {
        visibility,
        diffuse: _,
    } = &mut app.render_target
    else {
        unreachable!()
    };

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
    vk: &VulkanContext,
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

    let vk = &vk.device;

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

fn start_bake(app: &mut Stilb) {
    assert!(app.cpu_mesh.vertices.len() > 0);

    app.bake_shader =
        load_bake_lights_shader(&app.vk, app.config.is_preview, app.groups.len() as u32);

    // upload lights
    if app.cpu_lights.len() > 0 {
        let gpu_lights = GpuLights::new(&app.vk, &app.cpu_lights);
        app.gpu_lights = gpu_lights;
    }

    app.gpu_mesh = GpuMesh::new(&app.vk, &app.cpu_mesh);
    println!(
        "Uploaded mesh Vertices: {} Triangles: {}",
        app.cpu_mesh.vertices.len(),
        app.cpu_mesh.indices.len()
    );
    // free cpu mesh
    app.cpu_mesh = Mesh {
        vertices: Vec::new(),
        indices: Vec::new(),
    };

    let mesh::AccelerationStructureType::RayQuery(blas) = &app.gpu_mesh.acceleration_structure
    else {
        panic!("Expected RayQuery variant");
    };

    app.tlas = create_tlas(&app.vk, blas);

    bake_lightmaps(app);
}

fn initialize_bake_push_constants(
    app: &mut Stilb,
    width: u32,
    height: u32,
    max_samples: u32,
    bounce_count: u32,
) {
    app.push = BakePushConstants {
        vertices: app.gpu_mesh.vertex_address(),
        indices: app.gpu_mesh.index_address(),
        lights: app.gpu_lights.address(),
        lights_count: app.gpu_lights.count,
        sample_index: 0,
        width: width,
        height: height,
        max_samples,
        bounce_count,
    };
}

fn bake_lightmaps(app: &mut Stilb) {
    // let mut group = group;

    let albedos: Vec<vk::ImageView> = app.groups.iter().map(|x| x.albedo.view()).collect();
    let emissions: Vec<vk::ImageView> = app.groups.iter().map(|x| x.emission.view()).collect();

    if app.config.is_preview {
        let window = app.window;

        let preview_settings = app.config.preview_settings.clone();

        update_render_target(app, &preview_settings, 0);

        let RenderTarget::NonDirectional {
            visibility,
            diffuse,
        } = &mut app.render_target
        else {
            unreachable!()
        };

        update_bake_lights_shader(
            &app.vk,
            &app.bake_shader,
            app.tlas.acceleration_structure(),
            visibility.view(),
            &albedos,
            &emissions,
            diffuse.view(),
            app.sampler_linear_clamp,
            &app.sobol,
        );

        let mut previous_time = std::time::Instant::now();

        let mut bake_start_time = std::time::Instant::now();
        let mut bake_complete_printed = false;

        unsafe {
            while glfwWindowShouldClose(window) == 0 {
                glfwPollEvents();

                print!(
                    "\rSample: {} / {}",
                    app.push.sample_index, preview_settings.max_samples
                );
                io::stdout().flush().unwrap();

                let now = std::time::Instant::now();

                if glfwGetKey(window, GLFW_KEY_ESCAPE) == GLFW_PRESS {
                    glfwSetWindowShouldClose(window, 1);
                }

                let delta_time = now.duration_since(previous_time).as_secs_f32();

                update_camera(app, delta_time);

                if !app.preview_initialized {
                    app.push.sample_index = 0;
                    bake_start_time = std::time::Instant::now();
                    bake_complete_printed = false;
                }

                // render finished
                if app.push.sample_index >= preview_settings.max_samples {
                    std::thread::sleep(Duration::from_millis(16));
                    if !bake_complete_printed {
                        io::stdout().flush().unwrap();
                        let bake_time = now.duration_since(bake_start_time).as_secs_f32();
                        println!("bake complete in {}s", bake_time);
                        bake_complete_printed = true;
                    }
                }

                if !render_sample_camera(app, &preview_settings) {
                    // restart bake
                    app.config.preview_settings.width = app.vk.swapchain.extent.width;
                    app.config.preview_settings.height = app.vk.swapchain.extent.height;

                    update_render_target(app, &preview_settings, 0);
                    let RenderTarget::NonDirectional {
                        visibility,
                        diffuse,
                    } = &mut app.render_target
                    else {
                        unreachable!()
                    };

                    update_bake_lights_shader(
                        &app.vk,
                        &app.bake_shader,
                        app.tlas.acceleration_structure(),
                        visibility.view(),
                        &albedos,
                        &emissions,
                        diffuse.view(),
                        app.sampler_linear_clamp,
                        &app.sobol,
                    );

                    continue;
                }

                if app.config.throttle_preview_ms > 0 {
                    std::thread::sleep(Duration::from_millis(
                        app.config.throttle_preview_ms as u64,
                    ));
                }

                previous_time = now;
            }
        }
    } else {
        let any_denoise = app.groups.iter().any(|x| x.settings.denoise);

        let oidn = if any_denoise {
            Some(Oidn::load().expect("failed to load oidn"))
        } else {
            None
        };

        for i in 0..app.groups.len() {
            let group_index = i as u32;

            let group = &app.groups[i];
            app.push.sample_index = 0;
            let settings = group.settings.clone();
            update_render_target(app, &settings, group_index);

            let RenderTarget::NonDirectional {
                visibility,
                diffuse,
            } = &mut app.render_target
            else {
                unreachable!()
            };

            update_bake_lights_shader(
                &app.vk,
                &app.bake_shader,
                app.tlas.acceleration_structure(),
                visibility.view(),
                &albedos,
                &emissions,
                diffuse.view(),
                app.sampler_linear_clamp,
                &app.sobol,
            );

            loop {
                render_sample_bake(app, &settings);
                if app.push.sample_index >= settings.max_samples {
                    break;
                }
            }

            println!("lightmap baked");

            unsafe {
                app.vk.device.device_wait_idle().unwrap();
            }

            let RenderTarget::NonDirectional {
                visibility: _,
                diffuse,
            } = &mut app.render_target
            else {
                unreachable!()
            };

            let callback = app.config.callback;

            let mut pixels_read = diffuse.read_pixels(&app.vk);

            if settings.denoise {
                match &oidn {
                    Some(oidn) => {
                        pixels_read = oidn.denoise(
                            &mut pixels_read,
                            settings.width as usize,
                            settings.height as usize,
                        );
                    }
                    None => {}
                }
            }

            let readback_data = ReadbackData {
                group_index,
                ty: 0,
                pixels: pixels_read.as_ptr(),
                pixels_count: pixels_read.len() as u32,
                width: settings.width,
                height: settings.height,
            };

            callback(readback_data);
        }
    }

    unsafe {
        app.vk.device.device_wait_idle().unwrap();
    }
}

fn render_sample_bake(app: &mut Stilb, settings: &LightmapSettings) {
    let width = settings.width;
    let height = settings.height;

    let vk = &app.vk.device;

    let cmd = app.vk.command_buffer;

    let begin_info = vk::CommandBufferBeginInfo {
        flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
        ..Default::default()
    };

    unsafe {
        vk.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())
            .unwrap();

        vk.begin_command_buffer(cmd, &begin_info).unwrap();

        let RenderTarget::NonDirectional {
            visibility: _,
            diffuse,
        } = &mut app.render_target
        else {
            unreachable!()
        };

        if diffuse.layout() != vk::ImageLayout::GENERAL {
            let barrier = diffuse.barrier(
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
        }

        if app.push.sample_index < settings.max_samples {
            render_sample(app, cmd, width, height);
            app.push.sample_index += 1;
        }
        let vk = &app.vk.device;

        let cmds = [cmd];
        let submit = vk::SubmitInfo::default().command_buffers(&cmds);

        vk.end_command_buffer(cmd).unwrap();

        vk.queue_submit(app.vk.compute_queue, &[submit], vk::Fence::null())
            .unwrap();

        vk.queue_wait_idle(app.vk.compute_queue).unwrap()
    };
}

fn render_sample_camera(app: &mut Stilb, settings: &LightmapSettings) -> bool {
    let frame_index = app.vk.swapchain.frame_index;

    let frame = &app.vk.swapchain.frames[frame_index];

    let width = app.config.preview_settings.width;
    let height = app.config.preview_settings.height;

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
            rasterize_visibility_from_camera(app, cmd);
            app.preview_initialized = true;
            let clear = vk::ClearColorValue {
                float32: [0.0, 0.0, 0.0, 0.0],
            };

            let RenderTarget::NonDirectional {
                visibility: _,
                diffuse,
            } = &mut app.render_target
            else {
                unreachable!()
            };

            clear_texture(&app.vk, diffuse, cmd, clear);
        }

        let vk = &app.vk.device;

        let RenderTarget::NonDirectional {
            visibility: _,
            diffuse,
        } = &mut app.render_target
        else {
            unreachable!()
        };

        let barrier = diffuse.barrier(
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

        if app.push.sample_index < settings.max_samples {
            render_sample(
                app,
                cmd,
                app.config.preview_settings.width,
                app.config.preview_settings.height,
            );
            app.push.sample_index += 1;
        }

        let RenderTarget::NonDirectional {
            visibility: _,
            diffuse,
        } = &mut app.render_target
        else {
            unreachable!()
        };

        let swapchain_image = &app.vk.swapchain.frames[image_index as usize];

        let vk = &app.vk.device;

        {
            let barrier = diffuse.barrier(
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
                diffuse.image(),
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

fn render_sample(app: &mut Stilb, cmd: vk::CommandBuffer, width: u32, height: u32) {
    let vk = &app.vk;
    let shader = &app.bake_shader;

    let constants_bytes = as_bytes(&app.push);

    // println!("rendering sample: {}", group.push.sample_index);

    let groups_x = (width + 7) / 8;
    let groups_y = (height + 7) / 8;

    unsafe {
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

fn update_render_target(app: &mut Stilb, settings: &LightmapSettings, group_index: u32) {
    if let RenderTarget::NonDirectional {
        visibility,
        diffuse,
    } = &mut app.render_target
    {
        if !visibility.image().is_null() {
            visibility.destroy(&app.vk);
        }
        if !diffuse.image().is_null() {
            diffuse.destroy(&app.vk);
        }
    };

    let (target_width, target_height) = if app.config.is_preview {
        (
            app.config.preview_settings.width,
            app.config.preview_settings.height,
        )
    } else {
        (settings.width, settings.height)
    };

    let diffuse = Texture2D::new(
        &app.vk,
        target_width,
        target_height,
        vk::Format::R32G32B32A32_SFLOAT,
        vk::ImageUsageFlags::STORAGE
            | vk::ImageUsageFlags::TRANSFER_SRC
            | vk::ImageUsageFlags::TRANSFER_DST,
    );

    let visibility = if app.config.is_preview {
        render_visibility_buffer_camera(app, target_width, target_height)
    } else {
        render_visibility_buffer_bake(app, target_width, target_height, group_index)
    };

    println!("visibility: {:#x}", visibility.image().as_raw());
    println!("diffuse: {:#x}", diffuse.image().as_raw());

    app.render_target = RenderTarget::NonDirectional {
        visibility,
        diffuse,
    };

    initialize_bake_push_constants(
        app,
        target_width,
        target_height,
        settings.max_samples,
        settings.bounce_count,
    );

    app.preview_initialized = false;
}

impl LightmapGroup {
    fn new(
        app: &mut Stilb,
        settings: LightmapSettings,
        albedo_pixels: &[u8],
        emission_pixels: &[f32],
    ) -> LightmapGroup {
        // println!("creating lightmap group {:?}", &settings);

        let mut albedo = Texture2D::new(
            &app.vk,
            settings.width,
            settings.height,
            vk::Format::R8G8B8A8_UNORM,
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

        // if emission_pixels.len() > 0 {
        emission.set_pixels(&app.vk, emission_pixels);
        // }

        // if albedo_pixels.len() > 0 {
        albedo.set_pixels(&app.vk, albedo_pixels);
        // }

        println!("albedo: {:#x}", albedo.image().as_raw());
        println!("emission: {:#x}", emission.image().as_raw());

        // let cmd = app.vk.begin_single_use_cmd();
        // unsafe {
        //     // let clear = vk::ClearColorValue {
        //     //     float32: [1.0, 1.0, 1.0, 1.0],
        //     // };
        //     // clear_texture(&app.vk, &mut albedo, cmd, clear);

        //     let barrier = albedo.barrier(
        //         vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        //         vk::AccessFlags::default(),
        //         vk::AccessFlags::SHADER_READ,
        //     );
        //     let barrier1 = emission.barrier(
        //         vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        //         vk::AccessFlags::default(),
        //         vk::AccessFlags::SHADER_READ,
        //     );

        //     app.vk.device.cmd_pipeline_barrier(
        //         cmd,
        //         vk::PipelineStageFlags::TOP_OF_PIPE,
        //         vk::PipelineStageFlags::COMPUTE_SHADER,
        //         vk::DependencyFlags::empty(),
        //         &[],
        //         &[],
        //         &[barrier, barrier1],
        //     );
        // }
        // app.vk.end_single_use_cmd(cmd);

        LightmapGroup {
            settings,
            albedo,
            emission,
        }
    }

    pub fn destroy(&mut self, vk: &VulkanContext) {
        self.albedo.destroy(vk);
        self.emission.destroy(vk);
    }
}

impl Stilb {
    pub fn new(config: StilbConfig) -> Stilb {
        let is_debug = cfg!(debug_assertions);

        let mut vulkan_config = VulkanConfig {
            enable_validation_layers: is_debug,
            enable_window: config.is_preview,
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

        if config.is_preview {
            vk.create_swapchain(
                config.preview_settings.width,
                config.preview_settings.height,
            );
        }

        let mut pos = config.camera_position;
        pos.transform_space(config.coordinate_system);
        let mut camera = Camera {
            position: pos,
            yaw: 0.0,
            pitch: 0.0,
            fov: 60.0,
            last_cursor_pos: None,
        };
        let mut fwd = config.camera_forward;
        fwd.transform_space(config.coordinate_system);
        camera.set_forward(fwd);

        let init_from_camera_shader = load_init_from_camera_shader(&vk);

        let gpu_lights = GpuLights {
            buffer: vk::Buffer::null(),
            memory: vk::DeviceMemory::null(),
            address: 0,
            count: 0,
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

        let sampler_linear_clamp =
            unsafe { vk.device.create_sampler(&sampler_info, None).unwrap() };

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

        let cpu_mesh = Mesh {
            vertices: Vec::new(),
            indices: Vec::new(),
        };

        let sobol = SobolBuffer::new(&vk);

        Self {
            vk,
            cpu_mesh,
            window: window,
            config: config,
            cpu_lights: Vec::new(),
            bake_shader: ComputeShader::null(),
            gpu_mesh: GpuMesh::null(),
            tlas: VulkanAs::null(),
            groups: Vec::new(),
            camera,
            init_from_camera_shader,
            preview_initialized: false,
            gpu_lights,
            sampler_linear_clamp,
            push,
            render_target: RenderTarget::None,
            sobol,
        }
    }
}
