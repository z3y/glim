use ash::vk::{self, Handle};
use std::io::{self, Write};
use std::path::PathBuf;
use std::{ptr, time::Duration};

use glfw_sys::{
    GLFW_KEY_ESCAPE, GLFW_PRESS, GLFWwindow, glfwCreateWindowSurface, glfwGetKey, glfwPollEvents,
    glfwSetWindowShouldClose, glfwWindowShouldClose,
};

use crate::bindings::*;
use crate::buffer::Buffer;
use crate::compute_shader::*;
use crate::graphics_shader::update_visibility_shader;
use crate::lights::light_buffer_flags;
use crate::math::{Vector2, Vector3};
use crate::seams::{Seam, dilate, fix_seams};
use crate::sh::SHProbeL2;
use crate::shaders::bake_direct::{
    BakeDirectPushConstants, load_bake_direct_shader, update_bake_direct_shader,
};
use crate::shaders::bake_indirect::{
    BakeIndirectPushConstants, load_bake_indirect_shader, update_bake_indirect_shader,
};
use crate::shaders::compact_visibility::{
    load_shader_compact_visibility, update_shader_compact_visibility,
};
use crate::shaders::compaction_mask::{
    CompactionPushConstants, load_shader_compaction_mask, update_shader_compaction_mask,
};
use crate::shaders::decompact::{load_shader_decompact, update_shader_decompact};
use crate::skybox::Skybox;
use crate::{
    camera::Camera,
    compute_shader::{
        ComputeShader, PreviewPushConstants, load_init_from_camera_shader, load_preview_shader,
        update_init_from_camera_shader, update_preview_shader,
    },
    graphics_shader::{VisibilityPushConstants, load_visibility_shader},
    lights::Light,
    mesh::{GpuMesh, Mesh, VulkanAs, create_tlas},
    oidn::Oidn,
    texture2d::Texture2D,
    vulkan_context::{VulkanConfig, VulkanContext},
    window::{initialize_window, update_camera},
};

mod bindings;
mod buffer;
mod camera;
mod compute_shader;
mod graphics_shader;
mod lights;
mod math;
mod mesh;
mod oidn;
mod pack;
mod seams;
mod sh;
mod shader_bindings;
mod shaders;
mod skybox;
mod test;
mod texture2d;
mod vulkan_cmd;
mod vulkan_context;
mod vulkan_swapchain;
mod window;

pub struct Glim {
    pub config: GlimConfig,
    pub vk: VulkanContext,
    pub window: *mut GLFWwindow,

    pub opaque_mesh: Mesh,
    pub transparent_mesh: Mesh,
    pub cpu_lights: Vec<Light>,
    pub emissive_triangles: Vec<u32>,
    pub groups: Vec<LightmapGroup>,
    pub seams: Vec<Seam>,

    pub gpu_mesh: GpuMesh,
    pub gpu_lights: Buffer,
    pub emissive_triangles_buffer: Buffer,
    pub tlas: VulkanAs,

    pub camera: Camera,

    pub preview_shader: ComputeShader,
    pub init_from_camera_shader: ComputeShader,
    pub preview_initialized: bool,

    pub preview_push_constants: PreviewPushConstants,

    pub probes: Vec<SHProbeL2>,

    pub probes_buffer: Buffer,
    pub bake_probes_shader: ComputeShader,

    pub adjust_samples_shader: ComputeShader,

    pub staging_buffer: Buffer,

    pub render_target: RenderTarget,

    pub constants: SpecializationConstants,

    pub output_dir: PathBuf,

    pub skybox: Skybox,
}

impl Drop for Glim {
    fn drop(&mut self) {
        for group in &mut self.groups {
            group.destroy(&self.vk);
        }

        if !self.staging_buffer.buffer.is_null() {
            self.staging_buffer.destroy(&self.vk);
        }

        let rt = &mut self.render_target;

        if !rt.visibility.image().is_null() {
            rt.visibility.destroy(&self.vk);
        }
        if !rt.diffuse.image().is_null() {
            rt.diffuse.destroy(&self.vk);
        }

        if !self.preview_shader.pipeline.is_null() {
            self.preview_shader.destroy(&self.vk);
        }
        self.gpu_mesh.destroy(&self.vk);
        self.tlas.destroy(&self.vk);

        if !self.init_from_camera_shader.pipeline.is_null() {
            self.init_from_camera_shader.destroy(&self.vk);
        }

        if !self.gpu_lights.buffer.is_null() {
            self.gpu_lights.destroy(&self.vk);
        }

        if !self.probes_buffer.buffer.is_null() {
            self.probes_buffer.destroy(&self.vk);
            self.bake_probes_shader.destroy(&self.vk);
        }

        if !self.emissive_triangles_buffer.buffer.is_null() {
            self.emissive_triangles_buffer.destroy(&self.vk);
        }

        if !self.skybox.view().is_null() {
            self.skybox.destroy(&self.vk);
        }
    }
}

pub struct RenderTarget {
    visibility: Texture2D,
    diffuse: Texture2D,
}

pub struct LightmapGroup {
    pub settings: LightmapSettings,
    pub index: u32,

    pub albedo: Texture2D,
    pub emission: Texture2D,
    pub emission_pixels: Vec<f32>,

    pub lightmap_diffuse_final: Vec<f32>,
    pub lightmap_directional: Vec<f32>,
    pub lightmap_diffuse_previous_bounce: Vec<f32>,
}

#[inline]
pub fn as_bytes<T>(v: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v as *const T as *const u8, std::mem::size_of::<T>()) }
}

fn clamp_samples(samples: u32) -> u32 {
    const MAX_SAMPLES: u32 = 65536;
    samples.clamp(1, MAX_SAMPLES)
}

fn clamp_bounces(bounces: u32) -> u32 {
    const MAX_DIMENSIONS: u32 = 256;
    // 2 dimensions for direct + 2 per bounce
    const MAX_BOUNCES: u32 = (MAX_DIMENSIONS - 2) / 2;
    bounces.clamp(0, MAX_BOUNCES)
}

fn update_visibility_from_camera(app: &mut Glim, cmd: vk::CommandBuffer) {
    let width = app.config.preview_settings.width;
    let height = app.config.preview_settings.height;

    let vk = &mut app.vk;
    let shader = &app.init_from_camera_shader;

    let push = app.camera.make_push_constants();

    let constants_bytes = as_bytes(&push);

    let visibility = &mut app.render_target.visibility;

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

// main render function
fn initialize_render(app: &mut Glim) {
    assert!(app.opaque_mesh.vertices.len() > 0 || app.transparent_mesh.vertices.len() > 0);

    let total_triangles = (app.opaque_mesh.indices.len() + app.transparent_mesh.indices.len()) / 3;

    extract_emissive_triangles(app);

    let config = &app.config;

    app.constants = SpecializationConstants {
        use_camera: 0,
        light_falloff_type: config.light_falloff as u32,
        transparent_primitive_offset: (app.opaque_mesh.indices.len() / 3) as u32,
        emissive_triangles_count: app.emissive_triangles.len() as u32,
        multiple_importance_sampling: config.mis as u32,
        lightmap_group_count: app.groups.len() as u32,
        lightmap_mode: config.lightmap_mode as u32,
        coordinate_system: app.config.coordinate_system as u32,
    };

    app.preview_shader = load_preview_shader(&app.vk, &app.constants);

    if app.probes.len() > 0 {
        app.bake_probes_shader = load_bake_light_probes_shader(&app.vk, &app.constants);

        let flags = vk::BufferUsageFlags::TRANSFER_DST
            | vk::BufferUsageFlags::STORAGE_BUFFER
            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
            | vk::BufferUsageFlags::TRANSFER_SRC;

        app.probes_buffer = Buffer::new(
            &app.vk,
            String::from("Light Probes"),
            &app.probes,
            flags,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        );
    }

    // clamp samples and bounces to supported limits
    app.config.probe_samples = clamp_samples(app.config.probe_samples);
    app.config.probe_bounces = clamp_bounces(app.config.probe_bounces);

    app.config.direct_samples = clamp_samples(app.config.direct_samples);
    app.config.indirect_samples = clamp_samples(app.config.indirect_samples);
    app.config.bounce_count = clamp_bounces(app.config.bounce_count);

    // upload lights
    if app.cpu_lights.len() > 0 {
        app.gpu_lights = Buffer::new(
            &app.vk,
            String::from("Lights"),
            &app.cpu_lights,
            light_buffer_flags(),
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        );
    } else {
        app.gpu_lights = Buffer::empty(
            &app.vk,
            String::from("Lights"),
            std::mem::size_of::<Light>() as vk::DeviceSize,
            light_buffer_flags(),
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        );
    }

    app.gpu_mesh = GpuMesh::new(
        &app.vk,
        &app.opaque_mesh,
        &app.transparent_mesh,
        &app.groups,
    );
    let message = format!(
        "Created scene with Vertices: {} Triangles: {}",
        app.opaque_mesh.vertices.len() + app.transparent_mesh.vertices.len(),
        total_triangles,
    );

    (app.config.log_callback)(LogMessage::message(&message));

    let blas = match &app.gpu_mesh.acceleration_structure {
        mesh::AccelerationStructureType::RayQuery(blas) => blas,
        _ => panic!("Expected RayQuery variant"),
    };

    app.tlas = create_tlas(&app.vk, blas);

    if app.skybox.view().is_null() {
        app.skybox = Skybox::solid(&app.vk, 4, 4, Vector3::new(0.0, 0.0, 0.0));
    }

    if app.config.is_preview {
        render_preview(app);
    } else {
        render_lightmaps(app);
    }
    unsafe {
        app.vk.device.device_wait_idle().unwrap();
    }
}

fn render_preview(app: &mut Glim) {
    let albedos: Vec<vk::ImageView> = app.groups.iter().map(|x| x.albedo.view()).collect();
    let emissions: Vec<vk::ImageView> = app.groups.iter().map(|x| x.emission.view()).collect();

    let window = app.window;

    let preview_settings = app.config.preview_settings.clone();

    app.init_from_camera_shader = load_init_from_camera_shader(&app.vk, &app.constants);

    update_render_target(app, &preview_settings);

    let visibility = &mut app.render_target.visibility;
    let diffuse = &mut app.render_target.diffuse;

    update_preview_shader(
        &app.vk,
        &app.preview_shader,
        app.tlas.acceleration_structure(),
        visibility.view(),
        &albedos,
        &emissions,
        diffuse.view(),
        app.gpu_mesh.index_buffer.buffer,
        app.gpu_mesh.vertex_buffer.buffer,
        app.gpu_lights.buffer,
        app.emissive_triangles_buffer.buffer,
        app.skybox.view(),
        app.skybox.sampler(),
    );

    let mut previous_time = std::time::Instant::now();

    let mut bake_start_time = std::time::Instant::now();
    let mut bake_complete_printed = false;

    unsafe {
        while glfwWindowShouldClose(window) == 0 {
            glfwPollEvents();

            print!(
                "\rSample: {} / {}",
                app.preview_push_constants.sample_index, app.config.direct_samples
            );
            io::stdout().flush().unwrap();

            let now = std::time::Instant::now();

            if glfwGetKey(window, GLFW_KEY_ESCAPE) == GLFW_PRESS {
                glfwSetWindowShouldClose(window, 1);
            }

            let delta_time = now.duration_since(previous_time).as_secs_f32();

            update_camera(app, delta_time);

            if !app.preview_initialized {
                app.preview_push_constants.sample_index = 0;
                bake_start_time = std::time::Instant::now();
                bake_complete_printed = false;
            }

            // render finished
            if app.preview_push_constants.sample_index >= app.preview_push_constants.max_samples {
                std::thread::sleep(Duration::from_millis(16));
                if !bake_complete_printed {
                    io::stdout().flush().unwrap();
                    let bake_time = now.duration_since(bake_start_time).as_secs_f32();
                    println!("bake complete in {}s", bake_time);
                    bake_complete_printed = true;
                }
            }

            if !render_sample_camera(app) {
                // restart bake
                app.config.preview_settings.width = app.vk.swapchain.extent.width;
                app.config.preview_settings.height = app.vk.swapchain.extent.height;

                update_render_target(app, &preview_settings);

                let diffuse = &mut app.render_target.diffuse;
                let visibility = &mut app.render_target.visibility;

                update_preview_shader(
                    &app.vk,
                    &app.preview_shader,
                    app.tlas.acceleration_structure(),
                    visibility.view(),
                    &albedos,
                    &emissions,
                    diffuse.view(),
                    app.gpu_mesh.index_buffer.buffer,
                    app.gpu_mesh.vertex_buffer.buffer,
                    app.gpu_lights.buffer,
                    app.emissive_triangles_buffer.buffer,
                    app.skybox.view(),
                    app.skybox.sampler(),
                );

                continue;
            }

            if app.config.throttle_preview_ms > 0 {
                let target_duration_secs = app.config.throttle_preview_ms as f32 / 1000.0;
                let sleep_duration = target_duration_secs - delta_time;
                if sleep_duration > 0.0 {
                    std::thread::sleep(Duration::from_secs_f32(sleep_duration));
                }
            }

            previous_time = now;
        }
    }
}

fn render_sample_camera(app: &mut Glim) -> bool {
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

        if app.preview_push_constants.sample_index == 0 {
            update_visibility_from_camera(app, cmd);
            app.preview_initialized = true;
            let clear = vk::ClearColorValue {
                float32: [0.0, 0.0, 0.0, 0.0],
            };

            let diffuse = &mut app.render_target.diffuse;

            {
                let vk: &VulkanContext = &app.vk;
                let range = vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                };
                let vk = &vk.device;
                if diffuse.layout() != vk::ImageLayout::TRANSFER_DST_OPTIMAL {
                    let barrier = diffuse.barrier(
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
                }

                vk.cmd_clear_color_image(
                    cmd,
                    diffuse.image(),
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &clear,
                    &[range],
                );
            };
        }

        let vk = &app.vk.device;

        let diffuse = &mut app.render_target.diffuse;

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

        if app.preview_push_constants.sample_index < app.preview_push_constants.max_samples {
            let shader = &app.preview_shader;

            let constants_bytes = as_bytes(&app.preview_push_constants);

            let groups_x = (width + 7) / 8;
            let groups_y = (height + 7) / 8;

            vk.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, shader.pipeline);

            vk.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                shader.pipeline_layout,
                0,
                &[shader.descriptor_set],
                &[],
            );

            vk.cmd_push_constants(
                cmd,
                shader.pipeline_layout,
                vk::ShaderStageFlags::COMPUTE,
                0,
                &constants_bytes,
            );

            vk.cmd_dispatch(cmd, groups_x, groups_y, 1);

            app.preview_push_constants.sample_index += 1;
        }

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

fn update_render_target(app: &mut Glim, settings: &LightmapSettings) {
    let (width, height) = if app.config.is_preview {
        (
            app.config.preview_settings.width,
            app.config.preview_settings.height,
        )
    } else {
        (settings.width, settings.height)
    };

    let diffuse = &mut app.render_target.diffuse;
    let visibility = &mut app.render_target.visibility;

    if !diffuse.image().is_null() {
        diffuse.destroy(&app.vk);
    }
    if !visibility.image().is_null() {
        visibility.destroy(&app.vk);
    }

    let diffuse = Texture2D::new(
        &app.vk,
        width,
        height,
        vk::Format::R32G32B32A32_SFLOAT,
        vk::ImageUsageFlags::STORAGE
            | vk::ImageUsageFlags::TRANSFER_SRC
            | vk::ImageUsageFlags::TRANSFER_DST,
        String::from("RT Diffuse"),
    );

    let visibility = {
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
            String::from("RT Visibility"),
        );

        let albedos: Vec<vk::ImageView> = app.groups.iter().map(|x| x.albedo.view()).collect();

        update_init_from_camera_shader(
            vk,
            shader,
            app.tlas.acceleration_structure(),
            &visibility,
            app.gpu_mesh.index_buffer.buffer,
            app.gpu_mesh.vertex_buffer.buffer,
            &albedos,
        );
        visibility
    };

    app.render_target.diffuse = diffuse;
    app.render_target.visibility = visibility;

    {
        let max_samples = app.config.direct_samples;
        let bounce_count = app.config.bounce_count;
        app.preview_push_constants = PreviewPushConstants {
            lights_count: app.cpu_lights.len() as u32,
            sample_index: 0,
            width: width,
            height: height,
            max_samples,
            bounce_count,
        };
    };

    app.preview_initialized = false;
}

fn extract_emissive_triangles(app: &mut Glim) {
    // todo indices of both opaque and transparent
    let vertices = &app.opaque_mesh.vertices;
    let indices = &app.opaque_mesh.indices;
    let mut emissive_triangles: Vec<u32> = Vec::new();

    fn sample_emissive(uv: Vector2, w: u32, h: u32, pixels: &Vec<f32>) -> bool {
        let x = (uv.x * w as f32).clamp(0.0, (w - 1) as f32) as usize;
        let y = (uv.y * h as f32).clamp(0.0, (h - 1) as f32) as usize;
        let index = (y * w as usize + x) * 4;
        let emission = &pixels[index..index + 4];
        emission[0] > 0.0 || emission[1] > 0.0 || emission[2] > 0.0
    }

    if app.config.mis {
        for primitive_id in 0..(indices.len() / 3) {
            let i0 = indices[primitive_id * 3 + 0] as usize;
            let i1 = indices[primitive_id * 3 + 1] as usize;
            let i2 = indices[primitive_id * 3 + 2] as usize;

            let v0 = &vertices[i0];
            let v1 = &vertices[i1];
            let v2 = &vertices[i2];

            let emissive = (v0.flags & (1 << 17)) != 0;

            if !emissive {
                continue;
            }

            let uv0 = v0.uv;
            let uv1 = v1.uv;
            let uv2 = v2.uv;

            let group_index = (v0.flags & 0xFFFF) as usize;
            let group = &app.groups[group_index];
            let pixels = &group.emission_pixels;

            let w = group.settings.width;
            let h = group.settings.height;

            let center_uv = (uv0 + uv1 + uv2) / 3.0;

            if sample_emissive(center_uv, w, h, pixels) {
                emissive_triangles.push(primitive_id as u32);
                continue;
            }
            if sample_emissive(uv0, w, h, pixels) {
                emissive_triangles.push(primitive_id as u32);
                continue;
            }
            if sample_emissive(uv1, w, h, pixels) {
                emissive_triangles.push(primitive_id as u32);
                continue;
            }
            if sample_emissive(uv2, w, h, pixels) {
                emissive_triangles.push(primitive_id as u32);
                continue;
            }
        }
    }

    if app.config.mis {
        let message = format!(
            "Found {} emissive triangles for MIS",
            emissive_triangles.len()
        );
        (app.config.log_callback)(LogMessage::message(&message));
    }

    if emissive_triangles.len() > 0 {
        app.emissive_triangles_buffer = Buffer::new(
            &app.vk,
            String::from("Emissive Triangles"),
            &emissive_triangles,
            vk::BufferUsageFlags::TRANSFER_DST
                | vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        );
    } else {
        app.emissive_triangles_buffer = Buffer::empty(
            &app.vk,
            String::from("Emissive Triangles"),
            32,
            vk::BufferUsageFlags::TRANSFER_DST
                | vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        );
    }

    app.emissive_triangles = emissive_triangles;
}

impl LightmapGroup {
    fn new(
        app: &mut Glim,
        settings: LightmapSettings,
        albedo_pixels: &[u8],
        emission_pixels: &[f32],
        index: u32,
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
            format!("Albedo {}", index),
        );

        let mut emission = Texture2D::new(
            &app.vk,
            settings.width,
            settings.height,
            vk::Format::R32G32B32A32_SFLOAT,
            vk::ImageUsageFlags::SAMPLED
                | vk::ImageUsageFlags::TRANSFER_SRC
                | vk::ImageUsageFlags::TRANSFER_DST,
            format!("Emission {}", index),
        );

        // if emission_pixels.len() > 0 {
        emission.set_pixels(&app.vk, emission_pixels, &app.staging_buffer);
        // }

        // if albedo_pixels.len() > 0 {
        albedo.set_pixels(&app.vk, albedo_pixels, &app.staging_buffer);
        // }

        LightmapGroup {
            settings,
            albedo,
            emission,
            emission_pixels: emission_pixels.to_vec(),
            lightmap_diffuse_final: Vec::new(),
            lightmap_diffuse_previous_bounce: Vec::new(),
            index,
            lightmap_directional: Vec::new(),
        }
    }

    pub fn destroy(&mut self, vk: &VulkanContext) {
        if !self.albedo.image().is_null() {
            self.albedo.destroy(vk);
        }
        if !self.emission.image().is_null() {
            self.emission.destroy(vk);
        }
    }
}

impl Glim {
    pub fn new(config: GlimConfig, output_dir: PathBuf) -> Glim {
        let mut vulkan_config = VulkanConfig {
            enable_validation_layers: config.vulkan_validation_layers,
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

        let init_from_camera_shader = ComputeShader::null();

        let gpu_lights = Buffer::null();

        let preview_push_constants = PreviewPushConstants {
            lights_count: 0,
            sample_index: 0,
            width: 0,
            height: 0,
            max_samples: 0,
            bounce_count: 0,
        };

        let opaque_mesh = Mesh {
            vertices: Vec::new(),
            indices: Vec::new(),
        };

        let transparent_mesh = Mesh {
            vertices: Vec::new(),
            indices: Vec::new(),
        };

        let render_target = RenderTarget {
            visibility: Texture2D::null(),
            diffuse: Texture2D::null(),
        };

        let staging_width = 1024;
        let staging_height = 1024;

        // todo remove
        let staging_buffer = Buffer::empty(
            &vk,
            String::from("Staging Buffer"),
            staging_width
                * staging_height
                * 4
                * std::mem::size_of::<f32>() as u64 as vk::DeviceSize, // 16 MB
            vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        );

        let constants = SpecializationConstants {
            use_camera: 0,
            light_falloff_type: 0,
            transparent_primitive_offset: 0,
            emissive_triangles_count: 0,
            multiple_importance_sampling: 0,
            lightmap_group_count: 0,
            lightmap_mode: 0,
            coordinate_system: config.coordinate_system as u32,
        };

        let skybox = Skybox::null();

        Self {
            vk,
            opaque_mesh,
            transparent_mesh,
            window: window,
            config: config,
            cpu_lights: Vec::new(),
            preview_shader: ComputeShader::null(),
            gpu_mesh: GpuMesh::null(),
            tlas: VulkanAs::null(),
            groups: Vec::new(),
            camera,
            init_from_camera_shader,
            preview_initialized: false,
            gpu_lights,
            preview_push_constants,
            render_target,
            probes: Vec::new(),
            probes_buffer: Buffer::null(),
            bake_probes_shader: ComputeShader::null(),
            seams: Vec::new(),
            emissive_triangles: Vec::new(),
            emissive_triangles_buffer: Buffer::null(),
            staging_buffer,
            adjust_samples_shader: ComputeShader::null(),
            constants,
            output_dir,
            skybox,
        }
    }
}

fn save_tga(path: PathBuf, w: usize, h: usize, pixels: &[f32]) -> io::Result<()> {
    use std::fs::File;
    use std::io::BufWriter;

    let mut file = BufWriter::new(File::create(path)?);

    // TGA header (18 bytes)
    let header: [u8; 18] = [
        0, // ID length
        0, // Color map type
        2, // Image type: uncompressed true-color
        0,
        0,
        0,
        0,
        0, // Color map spec (unused)
        0,
        0, // X origin
        0,
        0, // Y origin
        (w & 0xFF) as u8,
        ((w >> 8) & 0xFF) as u8, // Width
        (h & 0xFF) as u8,
        ((h >> 8) & 0xFF) as u8, // Height
        32,                      // Bits per pixel (32 = RGBA)
        0x28,                    // Descriptor: 8 alpha bits, top-left origin (bit 5 set)
    ];
    file.write_all(&header)?;

    let mut row = vec![0u8; w * 4];
    for y in 0..h {
        for x in 0..w {
            let flipped_y = h - 1 - y;
            let index = (x + flipped_y * w) * 4;
            let r = (pixels[index + 0].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            let g = (pixels[index + 1].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            let b = (pixels[index + 2].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            let a = (pixels[index + 3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;

            let out = x * 4;
            row[out + 0] = b;
            row[out + 1] = g;
            row[out + 2] = r;
            row[out + 3] = a;
        }
        file.write_all(&row)?;
    }

    Ok(())
}

fn render_lightmaps(app: &mut Glim) {
    let mut max_resolution = (1, 1);
    let mut total_pixel_count = 0;
    for group in &app.groups {
        max_resolution.0 = u32::max(max_resolution.0, group.settings.width);
        max_resolution.1 = u32::max(max_resolution.1, group.settings.height);

        total_pixel_count += group.settings.width * group.settings.height;
    }

    // let max_pixel_count = max_resolution.0 * max_resolution.1;
    // let max_pixels_size = (max_pixel_count * std::mem::size_of::<f32>() as u32) as vk::DeviceSize;

    let mut visibility_expanded = Texture2D::new(
        &app.vk,
        max_resolution.0,
        max_resolution.1,
        vk::Format::R32G32_UINT,
        vk::ImageUsageFlags::STORAGE
            | vk::ImageUsageFlags::TRANSFER_SRC
            | vk::ImageUsageFlags::TRANSFER_DST
            | vk::ImageUsageFlags::SAMPLED
            | vk::ImageUsageFlags::COLOR_ATTACHMENT,
        String::from("Visibility Expanded"),
    );

    let mut visibility_shader_conservative =
        load_visibility_shader(&mut app.vk, &visibility_expanded, true, &app.constants);
    let mut visibility_shader_non_conservative =
        load_visibility_shader(&mut app.vk, &visibility_expanded, false, &app.constants);

    update_visibility_shader(
        &app.vk,
        &visibility_shader_conservative,
        app.gpu_mesh.index_buffer.buffer,
        app.gpu_mesh.vertex_buffer.buffer,
    );
    update_visibility_shader(
        &app.vk,
        &visibility_shader_non_conservative,
        app.gpu_mesh.index_buffer.buffer,
        app.gpu_mesh.vertex_buffer.buffer,
    );

    let mut compaction_shader = load_shader_compaction_mask(&app.vk, &app.constants);
    let mut compaction_buffer = Buffer::empty(
        &app.vk,
        "Compaction Mask".to_owned(),
        (total_pixel_count as u64 / 32) as u64 * std::mem::size_of::<u32>() as u64 * 2,
        vk::BufferUsageFlags::TRANSFER_DST
            | vk::BufferUsageFlags::STORAGE_BUFFER
            | vk::BufferUsageFlags::TRANSFER_SRC
            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    );
    update_shader_compaction_mask(
        &app.vk,
        &compaction_shader,
        visibility_expanded.view(),
        compaction_buffer.buffer,
    );

    let visibility_clear = [vk::ClearValue {
        color: vk::ClearColorValue {
            uint32: [u32::MAX, 0, 0, 0],
        },
    }];

    let mut expanded_groups_start = vec![0; app.groups.len()];
    let mut expanded_group_offset = 0;
    for group_index in 0..app.groups.len() {
        let group = &app.groups[group_index].settings;

        let mut render_pass_begin = vk::RenderPassBeginInfo {
            render_pass: visibility_shader_conservative.render_pass,
            framebuffer: visibility_shader_conservative.framebuffer,
            render_area: vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: vk::Extent2D {
                    width: group.width,
                    height: group.height,
                },
            },
            ..Default::default()
        };
        render_pass_begin = render_pass_begin.clear_values(&visibility_clear);

        let mesh = &app.gpu_mesh;

        let visibility_push = VisibilityPushConstants {
            width: group.width,
            height: group.height,
            group_index: group_index as u32,
            rt_width: visibility_expanded.width(),
            rt_height: visibility_expanded.height(),
            pad1: 0,
            pad2: 0,
            conservative: 1,
        };
        let visibility_push_bytes = as_bytes(&visibility_push);

        let compaction_push = CompactionPushConstants {
            width: group.width,
            height: group.height,
            offset: expanded_group_offset,
            compacted_count: 0,
            encode_type: 0,
            group_index: group_index as u32,
            dilate: 0,
            pad2: 0,
        };
        let compaction_push_bytes = as_bytes(&compaction_push);

        unsafe {
            let cmd = app.vk.begin_single_use_cmd();
            let vk = &app.vk.device;

            vk.cmd_begin_render_pass(cmd, &render_pass_begin, vk::SubpassContents::INLINE);
            vk.cmd_bind_pipeline(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                visibility_shader_conservative.pipeline,
            );
            vk.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                visibility_shader_conservative.pipeline_layout,
                0,
                &[visibility_shader_conservative.descriptor_set],
                &[],
            );
            vk.cmd_push_constants(
                cmd,
                visibility_shader_conservative.pipeline_layout,
                vk::ShaderStageFlags::FRAGMENT | vk::ShaderStageFlags::VERTEX,
                0,
                &visibility_push_bytes,
            );
            vk.cmd_draw(cmd, mesh.index_len * 3, 1, 0, 0);

            // non conservative
            let visibility_push = VisibilityPushConstants {
                width: group.width,
                height: group.height,
                group_index: group_index as u32,
                rt_width: visibility_expanded.width(),
                rt_height: visibility_expanded.height(),
                pad1: 0,
                pad2: 0,
                conservative: 0,
            };
            let visibility_push_bytes = as_bytes(&visibility_push);
            vk.cmd_bind_pipeline(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                visibility_shader_non_conservative.pipeline,
            );
            vk.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                visibility_shader_non_conservative.pipeline_layout,
                0,
                &[visibility_shader_non_conservative.descriptor_set],
                &[],
            );
            vk.cmd_push_constants(
                cmd,
                visibility_shader_non_conservative.pipeline_layout,
                vk::ShaderStageFlags::FRAGMENT | vk::ShaderStageFlags::VERTEX,
                0,
                &visibility_push_bytes,
            );
            vk.cmd_draw(cmd, mesh.index_len * 3, 25, 0, 0);

            vk.cmd_end_render_pass(cmd);
            // AttachmentDescription final_layout: vk::ImageLayout::GENERAL
            visibility_expanded.set_layout(vk::ImageLayout::GENERAL);

            // compaction
            vk.cmd_bind_pipeline(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                compaction_shader.pipeline,
            );
            vk.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                compaction_shader.pipeline_layout,
                0,
                &[compaction_shader.descriptor_set],
                &[],
            );
            vk.cmd_push_constants(
                cmd,
                compaction_shader.pipeline_layout,
                vk::ShaderStageFlags::COMPUTE,
                0,
                &compaction_push_bytes,
            );
            let groups_x = ((group.width * group.height) + 31) / 32;
            let groups_y = 1;
            vk.cmd_dispatch(cmd, groups_x, groups_y, 1);

            app.vk.end_single_use_cmd(cmd);
        };

        expanded_groups_start[group_index] = expanded_group_offset as usize;
        expanded_group_offset += (group.width * group.height) / 32;
    }

    compaction_shader.destroy(&app.vk);

    let mut compaction_buffer_cpu = vec![0u32; compaction_buffer.bytes as usize / 4];
    let mut staging_buffer_compaction = Buffer::empty(
        &app.vk,
        "Staging Buffer".to_owned(),
        compaction_buffer.bytes,
        vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::TRANSFER_SRC,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    );

    unsafe {
        let cmd = app.vk.begin_single_use_cmd();

        let regions = vk::BufferCopy {
            src_offset: 0,
            dst_offset: 0,
            size: compaction_buffer.bytes,
        };
        app.vk.device.cmd_copy_buffer(
            cmd,
            compaction_buffer.buffer,
            staging_buffer_compaction.buffer,
            &[regions],
        );

        app.vk.end_single_use_cmd(cmd);

        std::ptr::copy_nonoverlapping(
            staging_buffer_compaction.ptr as *const u8,
            compaction_buffer_cpu.as_mut_ptr() as *mut u8,
            regions.size as usize,
        );
    };

    let mut compacted_pixels_count = 0;
    for group_index in 0..app.groups.len() {
        let group_start = expanded_groups_start[group_index] * 2;

        let group = &app.groups[group_index].settings;
        let pixel_count = (group.width * group.height) as usize;

        for i in 0..pixel_count / 32 {
            let i = group_start + i * 2 + 1;

            let bits = compaction_buffer_cpu[i];
            compaction_buffer_cpu[i] = compacted_pixels_count;
            compacted_pixels_count += bits;
        }

        // const DEBUG_COMPACTION: bool = false;
        // if DEBUG_COMPACTION {
        //     let mut debug_pixels = vec![0.0; pixel_count * 4];

        //     for i in 0..pixel_count {
        //         let word = i / 32;
        //         let bit = i & 31;

        //         let mask = compaction_buffer_cpu[group_start + word * 2];
        //         let offset = compaction_buffer_cpu[group_start + word * 2 + 1];

        //         let active = (mask & (1 << bit)) != 0;

        //         let order = if active {
        //             let rank = (mask & ((1u32 << bit) - 1)).count_ones();
        //             let compact_index = offset + rank;

        //             (compact_index % 32) as f32 / 32.0
        //         } else {
        //             0.0
        //         };
        //         let visible = if active { 1.0 } else { 0.0 };

        //         let (x, y) = index_to_uv(i as u32);

        //         let pixel = (y * group.width + x) as usize;
        //         let dst = pixel * 4;

        //         debug_pixels[dst + 0] = order;
        //         debug_pixels[dst + 1] = order;
        //         debug_pixels[dst + 2] = order;
        //         debug_pixels[dst + 3] = visible;
        //     }

        //     let readback_data = LightmapReadbackData {
        //         group_index: group_index as u32,
        //         ty: 0,
        //         pixels: debug_pixels.as_ptr(),
        //         pixels_count: debug_pixels.len() as u32,
        //         width: group.width,
        //         height: group.height,
        //     };
        //     (app.config.lightmap_read_callback)(readback_data);
        // }
    }

    // copy back the compaction buffer with prefix sum calculated
    unsafe {
        std::ptr::copy_nonoverlapping(
            compaction_buffer_cpu.as_ptr() as *const u8,
            staging_buffer_compaction.ptr as *mut u8,
            compaction_buffer.bytes as usize,
        );
        drop(compaction_buffer_cpu);

        let cmd = app.vk.begin_single_use_cmd();

        let region = vk::BufferCopy {
            src_offset: 0,
            dst_offset: 0,
            size: compaction_buffer.bytes,
        };

        app.vk.device.cmd_copy_buffer(
            cmd,
            staging_buffer_compaction.buffer,
            compaction_buffer.buffer,
            &[region],
        );

        app.vk.end_single_use_cmd(cmd);
    }

    let log = app.config.log_callback;
    let reduction = 1.0 - (compacted_pixels_count as f32 / total_pixel_count as f32);
    let message = format!("Compaction: {}%", reduction * 100.0);
    (log)(LogMessage::message(&message));

    let mut compacted_visibility = Buffer::empty(
        &app.vk,
        "Compacted Visibility".to_owned(),
        compacted_pixels_count as u64 * (std::mem::size_of::<f32>() * 4) as u64,
        vk::BufferUsageFlags::TRANSFER_DST
            | vk::BufferUsageFlags::STORAGE_BUFFER
            | vk::BufferUsageFlags::TRANSFER_SRC
            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    );

    let mut compact_visibility_shader = load_shader_compact_visibility(&app.vk, &app.constants);
    update_shader_compact_visibility(
        &app.vk,
        &compact_visibility_shader,
        visibility_expanded.view(),
        compaction_buffer.buffer,
        compacted_visibility.buffer,
        app.gpu_mesh.index_buffer.buffer,
        app.gpu_mesh.vertex_buffer.buffer,
    );

    // render visibility again but this time compact
    for group_index in 0..app.groups.len() {
        let group = &app.groups[group_index].settings;

        let mut render_pass_begin = vk::RenderPassBeginInfo {
            render_pass: visibility_shader_conservative.render_pass,
            framebuffer: visibility_shader_conservative.framebuffer,
            render_area: vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: vk::Extent2D {
                    width: group.width,
                    height: group.height,
                },
            },
            ..Default::default()
        };
        render_pass_begin = render_pass_begin.clear_values(&visibility_clear);

        let mesh = &app.gpu_mesh;

        let visibility_push = VisibilityPushConstants {
            width: group.width,
            height: group.height,
            group_index: group_index as u32,
            rt_width: visibility_expanded.width(),
            rt_height: visibility_expanded.height(),
            pad1: 0,
            pad2: 0,
            conservative: 1,
        };
        let visibility_push_bytes = as_bytes(&visibility_push);

        let compaction_push = CompactionPushConstants {
            width: group.width,
            height: group.height,
            offset: expanded_groups_start[group_index] as u32,
            compacted_count: compacted_pixels_count,
            encode_type: 0,
            group_index: group_index as u32,
            dilate: 0,
            pad2: 0,
        };
        let compaction_push_bytes = as_bytes(&compaction_push);

        unsafe {
            let cmd = app.vk.begin_single_use_cmd();
            let vk = &app.vk.device;

            vk.cmd_begin_render_pass(cmd, &render_pass_begin, vk::SubpassContents::INLINE);
            vk.cmd_bind_pipeline(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                visibility_shader_conservative.pipeline,
            );
            vk.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                visibility_shader_conservative.pipeline_layout,
                0,
                &[visibility_shader_conservative.descriptor_set],
                &[],
            );
            vk.cmd_push_constants(
                cmd,
                visibility_shader_conservative.pipeline_layout,
                vk::ShaderStageFlags::FRAGMENT | vk::ShaderStageFlags::VERTEX,
                0,
                &visibility_push_bytes,
            );

            vk.cmd_draw(cmd, mesh.index_len * 3, 1, 0, 0);

            // non conservative
            let visibility_push = VisibilityPushConstants {
                width: group.width,
                height: group.height,
                group_index: group_index as u32,
                rt_width: visibility_expanded.width(),
                rt_height: visibility_expanded.height(),
                pad1: 0,
                pad2: 0,
                conservative: 0,
            };
            let visibility_push_bytes = as_bytes(&visibility_push);
            vk.cmd_bind_pipeline(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                visibility_shader_non_conservative.pipeline,
            );
            vk.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                visibility_shader_non_conservative.pipeline_layout,
                0,
                &[visibility_shader_non_conservative.descriptor_set],
                &[],
            );
            vk.cmd_push_constants(
                cmd,
                visibility_shader_non_conservative.pipeline_layout,
                vk::ShaderStageFlags::FRAGMENT | vk::ShaderStageFlags::VERTEX,
                0,
                &visibility_push_bytes,
            );
            vk.cmd_draw(cmd, mesh.index_len * 3, 25, 0, 0);

            vk.cmd_end_render_pass(cmd);
            // AttachmentDescription final_layout: vk::ImageLayout::GENERAL
            visibility_expanded.set_layout(vk::ImageLayout::GENERAL);

            let shader = &compact_visibility_shader;
            vk.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, shader.pipeline);
            vk.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                shader.pipeline_layout,
                0,
                &[shader.descriptor_set],
                &[],
            );
            vk.cmd_push_constants(
                cmd,
                shader.pipeline_layout,
                vk::ShaderStageFlags::COMPUTE,
                0,
                &compaction_push_bytes,
            );
            let groups_x = (group.width + 7) / 8;
            let groups_y = (group.height + 7) / 8;
            vk.cmd_dispatch(cmd, groups_x, groups_y, 1);

            app.vk.end_single_use_cmd(cmd);
        };
    }

    visibility_expanded.destroy(&app.vk);
    visibility_shader_conservative.destroy(&app.vk);
    visibility_shader_non_conservative.destroy(&app.vk);
    compact_visibility_shader.destroy(&app.vk);
    staging_buffer_compaction.destroy(&app.vk);
    drop(staging_buffer_compaction);

    let albedos: Vec<vk::ImageView> = app.groups.iter().map(|x| x.albedo.view()).collect();
    let emissions: Vec<vk::ImageView> = app.groups.iter().map(|x| x.emission.view()).collect();

    // adjust sample positions
    {
        let mut adjust_sample_shader = load_adjust_samples_shader(&app.vk, &app.constants);
        update_adjust_samples_shader(
            &app.vk,
            &adjust_sample_shader,
            app.tlas.acceleration_structure(),
            compacted_visibility.buffer,
            &albedos,
            app.gpu_mesh.index_buffer.buffer,
            app.gpu_mesh.vertex_buffer.buffer,
        );
        let push = BakeDirectPushConstants {
            compacted_count: compacted_pixels_count,
            sample_index: 0,
            max_samples: app.config.direct_samples,
            lights_count: app.cpu_lights.len() as u32,
        };
        let cmd = app.vk.begin_single_use_cmd();
        let vk = &app.vk.device;
        unsafe {
            let push_bytes = as_bytes(&push);
            let shader = &adjust_sample_shader;
            vk.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, shader.pipeline);
            vk.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                shader.pipeline_layout,
                0,
                &[shader.descriptor_set],
                &[],
            );
            vk.cmd_push_constants(
                cmd,
                shader.pipeline_layout,
                vk::ShaderStageFlags::COMPUTE,
                0,
                &push_bytes,
            );

            let groups_x = (compacted_pixels_count + 63) / 64;
            vk.cmd_dispatch(cmd, groups_x, 1, 1);
        }
        app.vk.end_single_use_cmd(cmd);
        adjust_sample_shader.destroy(&app.vk);
    }

    let mut lightmap_channels = match app.config.lightmap_mode {
        LightmapMode::NonDirectional => 3,
        LightmapMode::Directional => 6,
    };

    // todo this could definitely be moved into a separate buffer
    // so it can be freed before the last big texture is allocated for decompaction
    if app.config.bounce_count > 0 {
        lightmap_channels += 6;
    }

    // todo initialize
    let mut compacted_lightmap = Buffer::empty(
        &app.vk,
        "Diffuse Buffer".to_owned(),
        compacted_pixels_count as u64 * (std::mem::size_of::<f32>() * lightmap_channels) as u64,
        vk::BufferUsageFlags::TRANSFER_DST
            | vk::BufferUsageFlags::STORAGE_BUFFER
            | vk::BufferUsageFlags::TRANSFER_SRC
            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    );

    let mut bake_direct_shader = load_bake_direct_shader(&app.vk, &app.constants);
    update_bake_direct_shader(
        &app.vk,
        &bake_direct_shader,
        app.tlas.acceleration_structure(),
        &albedos,
        &emissions,
        app.gpu_mesh.index_buffer.buffer,
        app.gpu_mesh.vertex_buffer.buffer,
        app.gpu_lights.buffer,
        app.emissive_triangles_buffer.buffer,
        compacted_visibility.buffer,
        compacted_lightmap.buffer,
        app.skybox.view(),
        app.skybox.sampler(),
    );

    let mut bake_direct_push = BakeDirectPushConstants {
        compacted_count: compacted_pixels_count,
        sample_index: 0,
        max_samples: app.config.direct_samples,
        lights_count: app.cpu_lights.len() as u32,
    };

    let compacted_groups_x = (compacted_pixels_count + 63) / 64;

    let message = format!("Baking Direct");

    let mut progress = 0.0;
    let progress_max =
        app.config.direct_samples + app.config.indirect_samples * app.config.bounce_count;
    let progress_scale = 1.0 / progress_max as f32;

    for sample_index in 0..app.config.direct_samples {
        if is_cancelled() {
            break;
        }
        bake_direct_push.sample_index = sample_index;
        (log)(LogMessage::progress(&message, progress * progress_scale));
        progress += 1.0;

        let vk = &app.vk.device;
        let shader = &bake_direct_shader;
        let bake_direct_push_bytes = as_bytes(&bake_direct_push);

        let cmd = app.vk.begin_single_use_cmd();
        unsafe {
            vk.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, shader.pipeline);
            vk.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                shader.pipeline_layout,
                0,
                &[shader.descriptor_set],
                &[],
            );
            vk.cmd_push_constants(
                cmd,
                shader.pipeline_layout,
                vk::ShaderStageFlags::COMPUTE,
                0,
                &bake_direct_push_bytes,
            );

            vk.cmd_dispatch(cmd, compacted_groups_x, 1, 1);
        };
        app.vk.end_single_use_cmd(cmd);
    }
    bake_direct_shader.destroy(&app.vk);

    let mut group_info_buffer = {
        let mut infos = Vec::with_capacity(app.groups.len());
        for group_index in 0..app.groups.len() {
            let group = &app.groups[group_index].settings;
            infos.push(LightmapInfo {
                resolution: [group.width, group.height],
                compaction_offset: expanded_groups_start[group_index] as u32,
                pad: 0,
            });
        }

        let group_info_buffer = Buffer::empty(
            &app.vk,
            "Lightmap Info".into(),
            128 * std::mem::size_of::<LightmapInfo>() as u64,
            vk::BufferUsageFlags::UNIFORM_BUFFER,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        );
        unsafe {
            std::ptr::copy_nonoverlapping(
                infos.as_ptr() as *const u8,
                group_info_buffer.ptr as *mut u8,
                infos.len() * std::mem::size_of::<LightmapInfo>(),
            );
        }
        group_info_buffer
    };

    if app.config.bounce_count > 0 {
        let mut indirect_shader = load_bake_indirect_shader(&app.vk, &app.constants);

        let mut push = BakeIndirectPushConstants {
            compacted_count: compacted_pixels_count,
            sample_index: 0,
            max_samples: app.config.indirect_samples,
            bounce_index: 0,
        };

        update_bake_indirect_shader(
            &app.vk,
            &indirect_shader,
            app.tlas.acceleration_structure(),
            compacted_visibility.buffer,
            &albedos,
            app.gpu_mesh.index_buffer.buffer,
            app.gpu_mesh.vertex_buffer.buffer,
            compacted_lightmap.buffer,
            compaction_buffer.buffer,
            group_info_buffer.buffer,
        );

        'bounces: for bounce_index in 0..app.config.bounce_count {
            push.bounce_index = bounce_index;

            let message = format!("Baking Bounce {}", bounce_index + 1);

            for sample_index in 0..app.config.indirect_samples {
                if is_cancelled() {
                    break 'bounces;
                }
                push.sample_index = sample_index;
                (log)(LogMessage::progress(&message, progress * progress_scale));
                progress += 1.0;

                let vk = &app.vk.device;
                let shader = &indirect_shader;
                let push_bytes = as_bytes(&push);

                let cmd = app.vk.begin_single_use_cmd();
                unsafe {
                    vk.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, shader.pipeline);
                    vk.cmd_bind_descriptor_sets(
                        cmd,
                        vk::PipelineBindPoint::COMPUTE,
                        shader.pipeline_layout,
                        0,
                        &[shader.descriptor_set],
                        &[],
                    );
                    vk.cmd_push_constants(
                        cmd,
                        shader.pipeline_layout,
                        vk::ShaderStageFlags::COMPUTE,
                        0,
                        &push_bytes,
                    );

                    vk.cmd_dispatch(cmd, compacted_groups_x, 1, 1);
                };
                app.vk.end_single_use_cmd(cmd);
            }
        }

        indirect_shader.destroy(&app.vk);
        drop(indirect_shader);
    }

    let mut decompact_shader = load_shader_decompact(&app.vk, &app.constants);

    let mut staging_buffer_lightmap = Buffer::empty(
        &app.vk,
        "Staging Buffer Lightmap".to_owned(),
        (max_resolution.0 * max_resolution.1 * 4) as u64 * std::mem::size_of::<f32>() as u64,
        vk::BufferUsageFlags::TRANSFER_SRC | vk::BufferUsageFlags::STORAGE_BUFFER,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    );

    update_shader_decompact(
        &app.vk,
        &decompact_shader,
        compaction_buffer.buffer,
        staging_buffer_lightmap.buffer,
        compacted_lightmap.buffer,
        compacted_visibility.buffer,
        group_info_buffer.buffer,
    );

    let oidn = Oidn::load();
    let oidn = if oidn.is_err() {
        let err = oidn.err();
        match err {
            Some(err) => {
                let message = "Failed to load Open Image Denoise";
                (log)(LogMessage::message(&message));
                (log)(LogMessage::message(&err.to_string()))
            }
            None => {}
        };
        None
    } else {
        Some(oidn.unwrap())
    };

    let lightmaps_per_group = match app.config.lightmap_mode {
        LightmapMode::NonDirectional => 1,
        LightmapMode::Directional => 2,
    };
    let post_total = (app.groups.len() * lightmaps_per_group).max(1) as u32;

    let output_dir = &app.output_dir;

    let process_lightmap = |group_index: usize, lightmap_type: u32, post_step: u32| {
        let group = &app.groups[group_index].settings;

        (log)(LogMessage::progress(
            &format!("Denoising & Fixing Seams ({}/{})", post_step, post_total),
            post_step as f32 / post_total as f32,
        ));

        let mut compaction_push = CompactionPushConstants {
            width: group.width,
            height: group.height,
            offset: expanded_groups_start[group_index] as u32,
            compacted_count: compacted_pixels_count,
            encode_type: lightmap_type,
            group_index: group_index as u32,
            dilate: group.dilate as u32,
            pad2: 0,
        };
        let decompact_push_bytes = as_bytes(&compaction_push);

        unsafe {
            let cmd = app.vk.begin_single_use_cmd();
            let vk = &app.vk.device;

            let shader = &decompact_shader;
            vk.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, shader.pipeline);
            vk.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                shader.pipeline_layout,
                0,
                &[shader.descriptor_set],
                &[],
            );
            vk.cmd_push_constants(
                cmd,
                shader.pipeline_layout,
                vk::ShaderStageFlags::COMPUTE,
                0,
                &decompact_push_bytes,
            );
            let groups_x = (group.width + 7) / 8;
            let groups_y = (group.height + 7) / 8;
            vk.cmd_dispatch(cmd, groups_x, groups_y, 1);
            app.vk.end_single_use_cmd(cmd);
        };

        unsafe {
            let pixels: &mut [f32] = std::slice::from_raw_parts_mut(
                staging_buffer_lightmap.ptr as *mut f32,
                (group.width * group.height * 4) as usize,
            );

            if group.denoise {
                match &oidn {
                    Some(oidn) => {
                        let start_time = std::time::Instant::now();

                        let directional = lightmap_type == 1;

                        oidn.denoise(
                            pixels,
                            group.width as usize,
                            group.height as usize,
                            directional,
                        );

                        let now = std::time::Instant::now();
                        let elapsed = now.duration_since(start_time).as_secs_f32();

                        let message = format!("Denoise Complete {}s", elapsed);
                        (log)(LogMessage::message(&message));
                    }
                    None => {
                        let message = format!("<color=red>Denoise Failed</color>");
                        (log)(LogMessage::message(&message));
                    }
                }
            }

            // encode directional
            if lightmap_type == 1 {
                compaction_push.encode_type = 2;
                let decompact_push_bytes = as_bytes(&compaction_push);

                let cmd = app.vk.begin_single_use_cmd();
                let vk = &app.vk.device;
                let shader = &decompact_shader;
                vk.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, shader.pipeline);
                vk.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::COMPUTE,
                    shader.pipeline_layout,
                    0,
                    &[shader.descriptor_set],
                    &[],
                );
                vk.cmd_push_constants(
                    cmd,
                    shader.pipeline_layout,
                    vk::ShaderStageFlags::COMPUTE,
                    0,
                    &decompact_push_bytes,
                );
                let groups_x = (group.width + 7) / 8;
                let groups_y = (group.height + 7) / 8;
                vk.cmd_dispatch(cmd, groups_x, groups_y, 1);
                app.vk.end_single_use_cmd(cmd);
            }

            // todo this doesnt handle directional alpha
            if group.fix_seams {
                let start_time = std::time::Instant::now();

                fix_seams(
                    pixels,
                    group.width,
                    group.height,
                    &app.seams,
                    app.config.seams_debug,
                    group_index as u32,
                );

                let now = std::time::Instant::now();
                let elapsed = now.duration_since(start_time).as_secs_f32();

                let message = format!("Seam Fix Complete {}s", elapsed);
                (log)(LogMessage::message(&message));
            }

            dilate(pixels, group.width, group.height, 0.0);

            // for baking light probes the denoised lightmap could also be used
            // just by compacting it back here

            let w = group.width as usize;
            let h = group.height as usize;

            if lightmap_type == 0 {
                use exr::prelude::*;
                use half::f16;

                let full_path = output_dir.join(format!("Lightmap-{}_Diffuse.exr", group_index));

                let channels = SpecificChannels::rgb(|pos: Vec2<usize>| {
                    let x = pos.x();
                    let flipped_y = h - 1 - pos.y();
                    let index = x + flipped_y * w;

                    (
                        f16::from_f32(pixels[index * 4 + 0]),
                        f16::from_f32(pixels[index * 4 + 1]),
                        f16::from_f32(pixels[index * 4 + 2]),
                    )
                });

                let layer = Layer::new(
                    (w, h),
                    LayerAttributes::named("diffuse"),
                    Encoding::SMALL_LOSSLESS,
                    channels,
                );

                Image::from_layer(layer).write().to_file(full_path).unwrap();
            } else if lightmap_type == 1 {
                let full_path =
                    output_dir.join(format!("Lightmap-{}_Directional.tga", group_index));

                // unity doesnt seem to like rgba .exr files
                // use exr::image::write::write_rgba_file;
                // use half::f16;

                // write_rgba_file(full_path, w, h, |x, y| {
                //     let flipped_y = h - 1 - y;
                //     let index = x + flipped_y * w;
                //     (
                //         pixels[index * 4 + 0],
                //         pixels[index * 4 + 1],
                //         pixels[index * 4 + 2],
                //         pixels[index * 4 + 3],
                //     )
                // })
                // .unwrap();

                save_tga(full_path, w, h, pixels).unwrap();
            }
        };
    };

    let mut post_step = 0;

    for group_index in 0..app.groups.len() {
        if is_cancelled() {
            break;
        }
        match app.config.lightmap_mode {
            LightmapMode::NonDirectional => {
                post_step += 1;
                process_lightmap(group_index, 0, post_step);
            }
            LightmapMode::Directional => {
                post_step += 1;
                process_lightmap(group_index, 0, post_step);
                post_step += 1;
                process_lightmap(group_index, 1, post_step);
            }
        }
    }

    decompact_shader.destroy(&app.vk);
    compacted_visibility.destroy(&app.vk);
    staging_buffer_lightmap.destroy(&app.vk);
    drop(decompact_shader);
    drop(compacted_visibility);
    drop(staging_buffer_lightmap);

    // light probes
    if app.probes.len() > 0 && !is_cancelled() {
        let mut shader = load_bake_light_probes_shader(&app.vk, &app.constants);

        update_bake_light_probes_shader(
            &app.vk,
            &shader,
            app.tlas.acceleration_structure(),
            app.probes_buffer.buffer,
            &albedos,
            &emissions,
            compacted_lightmap.buffer,
            app.gpu_mesh.index_buffer.buffer,
            app.gpu_mesh.vertex_buffer.buffer,
            app.gpu_lights.buffer,
            compaction_buffer.buffer,
            group_info_buffer.buffer,
            app.skybox.view(),
            app.skybox.sampler(),
        );

        let mut push = BakeSHPushConstants {
            lights_count: app.cpu_lights.len() as u32,
            max_samples: app.config.probe_samples,
            sample_index: 0,
            probes_count: app.probes.len() as u32,
        };

        let probes_count = app.probes.len() as u32;
        let groups_x = (probes_count + 63) / 64;
        let vk = &app.vk.device;

        for sample_index in 0..app.config.probe_samples {
            push.sample_index = sample_index;
            let constants_bytes = as_bytes(&push);

            let cmd = app.vk.begin_single_use_cmd();
            unsafe {
                vk.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, shader.pipeline);

                vk.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::COMPUTE,
                    shader.pipeline_layout,
                    0,
                    &[shader.descriptor_set],
                    &[],
                );

                vk.cmd_push_constants(
                    cmd,
                    shader.pipeline_layout,
                    vk::ShaderStageFlags::COMPUTE,
                    0,
                    &constants_bytes,
                );

                vk.cmd_dispatch(cmd, groups_x, 1, 1);
            };
            app.vk.end_single_use_cmd(cmd);
        }

        let mut staging_buffer_light_probes = Buffer::empty(
            &app.vk,
            "Staging Buffer Light Probes".to_owned(),
            app.probes_buffer.bytes,
            vk::BufferUsageFlags::TRANSFER_DST,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        );

        let cmd = app.vk.begin_single_use_cmd();
        let region = vk::BufferCopy {
            src_offset: 0,
            dst_offset: 0,
            size: app.probes_buffer.bytes,
        };
        unsafe {
            app.vk.device.cmd_copy_buffer(
                cmd,
                app.probes_buffer.buffer,
                staging_buffer_light_probes.buffer,
                &[region],
            )
        };
        app.vk.end_single_use_cmd(cmd);

        let readback_data = LightprobesReadbackData {
            probes: staging_buffer_light_probes.ptr as *const SHProbeL2,
            pixels_count: app.probes.len() as u32,
        };

        (app.config.lightprobes_read_callback)(readback_data);
        shader.destroy(&app.vk);
        staging_buffer_light_probes.destroy(&app.vk);
    }

    compacted_lightmap.destroy(&app.vk);
    compaction_buffer.destroy(&app.vk);
    group_info_buffer.destroy(&app.vk);
    drop(compacted_lightmap);
    drop(compaction_buffer);
    drop(group_info_buffer);

    if is_cancelled() {
        (log)(LogMessage::message("Bake cancelled by user."));
    }
}
