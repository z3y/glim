use ash::vk::{self, Handle};
use std::io::{self, Write};
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
use crate::seams::{Seam, fix_seams, inpaint};
use crate::sh::SHProbe;
use crate::{
    camera::Camera,
    compute_shader::{
        ComputeShader, PreviewPushConstants, load_init_from_camera_shader, load_preview_shader,
        update_init_from_camera_shader, update_preview_shader,
    },
    graphics_shader::{VisibilityPushConstants, create_visibility_shader},
    lights::Light,
    math::Vector3,
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
mod seams;
mod sh;
mod shader_bindings;
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

    pub texture_sampler: vk::Sampler,

    pub preview_push_constants: PreviewPushConstants,

    pub probes: Vec<SHProbe>,

    pub probes_buffer: Buffer,
    pub bake_probes_shader: ComputeShader,

    pub staging_buffer: Buffer,

    pub render_target: RenderTarget,
}

impl Drop for Stilb {
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

        unsafe { self.vk.device.destroy_sampler(self.texture_sampler, None) };
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

fn render_visibility_from_lightmap(app: &mut Stilb, width: u32, height: u32, group_index: u32) {
    let vk = &mut app.vk;
    let cmd = vk.begin_single_use_cmd();
    let mesh = &app.gpu_mesh;

    let visibility = &mut app.render_target.visibility;

    if visibility.width() != width || visibility.height() != height {
        if !visibility.image().is_null() {
            visibility.destroy(vk);
        }

        app.render_target.visibility = Texture2D::new(
            vk,
            width,
            height,
            vk::Format::R32G32B32A32_SFLOAT,
            vk::ImageUsageFlags::STORAGE
                | vk::ImageUsageFlags::TRANSFER_SRC
                | vk::ImageUsageFlags::TRANSFER_DST
                | vk::ImageUsageFlags::SAMPLED
                | vk::ImageUsageFlags::COLOR_ATTACHMENT,
            String::from("RT Visibility"),
        );
    }

    let visibility = &app.render_target.visibility;

    // todo create shader once
    let mut shader = create_visibility_shader(vk, visibility, false);
    let mut shader_convervative = create_visibility_shader(vk, visibility, true);

    update_visibility_shader(
        vk,
        &shader,
        app.gpu_mesh.index_buffer.buffer,
        app.gpu_mesh.vertex_buffer.buffer,
    );
    update_visibility_shader(
        vk,
        &shader_convervative,
        app.gpu_mesh.index_buffer.buffer,
        app.gpu_mesh.vertex_buffer.buffer,
    );

    let clear_values = [vk::ClearValue {
        color: vk::ClearColorValue {
            float32: [0.0, 0.0, 0.0, 0.0],
        },
    }];

    let mut render_pass_begin = vk::RenderPassBeginInfo {
        render_pass: shader_convervative.render_pass,
        framebuffer: shader_convervative.framebuffer,
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

    unsafe {
        vk.device
            .cmd_begin_render_pass(cmd, &render_pass_begin, vk::SubpassContents::INLINE);
        vk.device.cmd_bind_pipeline(
            cmd,
            vk::PipelineBindPoint::GRAPHICS,
            shader_convervative.pipeline,
        );

        vk.device.cmd_bind_descriptor_sets(
            cmd,
            vk::PipelineBindPoint::GRAPHICS,
            shader_convervative.pipeline_layout,
            0,
            &[shader_convervative.descriptor_set],
            &[],
        );

        let push = VisibilityPushConstants {
            width: visibility.width(),
            height: visibility.height(),
            group_index,
            convervative: 1,
        };
        let constants_bytes = as_bytes(&push);
        vk.device.cmd_push_constants(
            cmd,
            shader_convervative.pipeline_layout,
            vk::ShaderStageFlags::GEOMETRY
                | vk::ShaderStageFlags::FRAGMENT
                | vk::ShaderStageFlags::VERTEX,
            0,
            &constants_bytes,
        );

        vk.device.cmd_draw(cmd, mesh.index_len, 1, 0, 0);

        // vk.device.cmd_end_render_pass(cmd);

        // non conservative

        // vk.device
        //     .cmd_begin_render_pass(cmd, &render_pass_begin2, vk::SubpassContents::INLINE);
        vk.device
            .cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, shader.pipeline);

        vk.device.cmd_bind_descriptor_sets(
            cmd,
            vk::PipelineBindPoint::GRAPHICS,
            shader.pipeline_layout,
            0,
            &[shader.descriptor_set],
            &[],
        );

        let push = VisibilityPushConstants {
            width: visibility.width(),
            height: visibility.height(),
            group_index,
            convervative: 0,
        };
        let constants_bytes = as_bytes(&push);
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
    shader_convervative.destroy(vk);
}

fn render_visibility_from_camera(app: &mut Stilb, width: u32, height: u32) -> Texture2D {
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
        app.texture_sampler,
    );
    visibility
}

fn update_visibility_from_camera(app: &mut Stilb, cmd: vk::CommandBuffer) {
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
        if texture.layout() != vk::ImageLayout::TRANSFER_DST_OPTIMAL {
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
        }

        vk.cmd_clear_color_image(
            cmd,
            texture.image(),
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &clear,
            &[range],
        );
    }
}

// main render function
fn initialize_render(app: &mut Stilb) {
    assert!(app.opaque_mesh.vertices.len() > 0 || app.transparent_mesh.vertices.len() > 0);

    let total_triangles = (app.opaque_mesh.indices.len() + app.transparent_mesh.indices.len()) / 3;

    extract_emissive_triangles(app);

    app.preview_shader = load_preview_shader(
        &app.vk,
        app.config.is_preview,
        app.config.light_falloff,
        app.groups.len() as u32,
        (app.opaque_mesh.indices.len() / 3) as u32,
        app.emissive_triangles.len() as u32,
        app.config.mis,
    );

    if app.probes.len() > 0 {
        app.bake_probes_shader =
            load_bake_sh_shader(&app.vk, app.groups.len() as u32, app.config.light_falloff);

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
        let dummy_buffer = [Light {
            position: Vector3::ZERO,
            ty: lights::LightType::Directional,
            direction: Vector3::ZERO,
            range: 0.0,
            color: Vector3::ZERO,
            shadow_radius_or_angle: 0.0,
        }];
        app.gpu_lights = Buffer::new(
            &app.vk,
            String::from("Lights"),
            &dummy_buffer,
            light_buffer_flags(),
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        );
    }

    app.gpu_mesh = GpuMesh::new(&app.vk, &app.opaque_mesh, &app.transparent_mesh);
    let message = format!(
        "Created scene with Vertices: {} Triangles: {}",
        app.opaque_mesh.vertices.len() + app.transparent_mesh.vertices.len(),
        total_triangles,
    );

    (app.config.log_callback)(LogMessage::message(&message));

    let mesh::AccelerationStructureType::RayQuery(blas) = &app.gpu_mesh.acceleration_structure
    else {
        panic!("Expected RayQuery variant");
    };

    app.tlas = create_tlas(&app.vk, blas);

    if app.config.is_preview {
        render_preview(app);
    } else {
        render_lightmaps(app);
    }
    unsafe {
        app.vk.device.device_wait_idle().unwrap();
    }
}

fn initialize_preview_push_constants(
    app: &mut Stilb,
    width: u32,
    height: u32,
    max_samples: u32,
    bounce_count: u32,
) {
    app.preview_push_constants = PreviewPushConstants {
        lights_count: app.cpu_lights.len() as u32,
        sample_index: 0,
        width: width,
        height: height,
        max_samples,
        bounce_count,
    };
}

fn copy_image(vk: &VulkanContext, src: &mut Texture2D, dst: &mut Texture2D) {
    unsafe { vk.device.queue_wait_idle(vk.compute_queue).unwrap() }

    let cmd = vk.begin_single_use_cmd();

    let region = vk::ImageCopy {
        src_subresource: vk::ImageSubresourceLayers {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        },
        src_offset: vk::Offset3D { x: 0, y: 0, z: 0 },
        dst_subresource: vk::ImageSubresourceLayers {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        },
        dst_offset: vk::Offset3D { x: 0, y: 0, z: 0 },
        extent: vk::Extent3D {
            width: src.width(),
            height: src.height(),
            depth: 1,
        },
        ..Default::default()
    };

    unsafe {
        if src.layout() != vk::ImageLayout::TRANSFER_SRC_OPTIMAL {
            let barrier = src.barrier(
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                vk::AccessFlags::default(),
                vk::AccessFlags::TRANSFER_READ,
            );
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

        if dst.layout() != vk::ImageLayout::TRANSFER_DST_OPTIMAL {
            let barrier = dst.barrier(
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::AccessFlags::default(),
                vk::AccessFlags::TRANSFER_WRITE,
            );
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

        vk.device.cmd_copy_image(
            cmd,
            src.image(),
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            dst.image(),
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &[region],
        );

        let barrier = dst.barrier(
            vk::ImageLayout::GENERAL,
            vk::AccessFlags::TRANSFER_WRITE,
            vk::AccessFlags::SHADER_READ,
        );
        vk.device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[barrier],
        );
    };

    vk.end_single_use_cmd(cmd);
}

fn render_preview(app: &mut Stilb) {
    let albedos: Vec<vk::ImageView> = app.groups.iter().map(|x| x.albedo.view()).collect();
    let emissions: Vec<vk::ImageView> = app.groups.iter().map(|x| x.emission.view()).collect();

    let window = app.window;

    let preview_settings = app.config.preview_settings.clone();

    app.init_from_camera_shader = load_init_from_camera_shader(
        &app.vk,
        app.groups.len() as u32,
        (app.opaque_mesh.indices.len() / 3) as u32,
    );

    update_render_target(app, &preview_settings, 0);

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
        app.texture_sampler,
        app.gpu_mesh.index_buffer.buffer,
        app.gpu_mesh.vertex_buffer.buffer,
        app.gpu_lights.buffer,
        app.emissive_triangles_buffer.buffer,
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

            if !render_sample_camera(app, &preview_settings) {
                // restart bake
                app.config.preview_settings.width = app.vk.swapchain.extent.width;
                app.config.preview_settings.height = app.vk.swapchain.extent.height;

                update_render_target(app, &preview_settings, 0);

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
                    app.texture_sampler,
                    app.gpu_mesh.index_buffer.buffer,
                    app.gpu_mesh.vertex_buffer.buffer,
                    app.gpu_lights.buffer,
                    app.emissive_triangles_buffer.buffer,
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

fn render_lightmaps(app: &mut Stilb) {
    let albedos: Vec<vk::ImageView> = app.groups.iter().map(|x| x.albedo.view()).collect();
    let emissions: Vec<vk::ImageView> = app.groups.iter().map(|x| x.emission.view()).collect();

    let any_denoise = app.groups.iter().any(|x| x.settings.denoise);

    let oidn = if any_denoise {
        Some(Oidn::load().expect("failed to load oidn"))
    } else {
        None
    };

    let mut bake_direct_shader = load_bake_direct_shader(
        &app.vk,
        app.config.light_falloff,
        app.groups.len() as u32,
        (app.opaque_mesh.indices.len() / 3) as u32,
        app.emissive_triangles.len() as u32,
        app.config.mis,
    );

    let lights_count = app.cpu_lights.len() as u32;

    let mut previous_diffuses = Vec::new();

    let bounce_count = app.config.bounce_count;

    let has_bounces = bounce_count > 0;

    if has_bounces {
        for i in 0..app.groups.len() {
            let group = &app.groups[i];
            let settings = group.settings.clone();

            let diffuse = Texture2D::new(
                &app.vk,
                settings.width,
                settings.height,
                vk::Format::R32G32B32A32_SFLOAT,
                vk::ImageUsageFlags::SAMPLED
                    | vk::ImageUsageFlags::TRANSFER_SRC
                    | vk::ImageUsageFlags::TRANSFER_DST,
                String::from("Diffuse Copy"),
            );

            previous_diffuses.push(diffuse);
        }
    }

    let log = app.config.log_callback;

    let mut progress = 0.0;
    let progress_max = app.groups.len() as u32 * app.config.direct_samples
        + app.groups.len() as u32 * app.config.indirect_samples * app.config.bounce_count;

    let progress_scale = 1.0 / progress_max as f32;

    for i in 0..app.groups.len() {
        let message = format!("Baking Direct (Group {}/{})", i + 1, app.groups.len());

        let group = &app.groups[i];

        let settings = group.settings.clone();

        let width = group.settings.width;
        let height = group.settings.height;

        let mut push = BakeDirectPushConstants {
            width,
            height,
            sample_index: 0,
            max_samples: app.config.direct_samples,
            lights_count,
            pad0: 0,
            pad1: 0,
            pad2: 0,
        };

        update_render_target(app, &settings, group.index);

        let visibility = &mut app.render_target.visibility;
        let diffuse = &mut app.render_target.diffuse;

        let shader = &bake_direct_shader;

        update_bake_direct_shader(
            &app.vk,
            shader,
            app.tlas.acceleration_structure(),
            visibility.view(),
            &albedos,
            &emissions,
            diffuse.view(),
            app.texture_sampler,
            app.gpu_mesh.index_buffer.buffer,
            app.gpu_mesh.vertex_buffer.buffer,
            app.gpu_lights.buffer,
            app.emissive_triangles_buffer.buffer,
        );

        let cmd = app.vk.command_buffer;
        let vk = &app.vk.device;

        let groups_x = (width + 7) / 8;
        let groups_y = (height + 7) / 8;

        let begin_info = vk::CommandBufferBeginInfo {
            flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
            ..Default::default()
        };

        loop {
            (log)(LogMessage::progress(&message, progress * progress_scale));

            unsafe {
                vk.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())
                    .unwrap();

                vk.begin_command_buffer(cmd, &begin_info).unwrap();

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

                vk.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, shader.pipeline);

                vk.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::COMPUTE,
                    shader.pipeline_layout,
                    0,
                    &[shader.descriptor_set],
                    &[],
                );

                let constants_bytes = as_bytes(&push);

                vk.cmd_push_constants(
                    cmd,
                    shader.pipeline_layout,
                    vk::ShaderStageFlags::COMPUTE,
                    0,
                    &constants_bytes,
                );

                vk.cmd_dispatch(cmd, groups_x, groups_y, 1);

                let cmds = [cmd];
                let submit = vk::SubmitInfo::default().command_buffers(&cmds);

                vk.end_command_buffer(cmd).unwrap();

                vk.queue_submit(app.vk.compute_queue, &[submit], vk::Fence::null())
                    .unwrap();

                vk.queue_wait_idle(app.vk.compute_queue).unwrap()
            };

            push.sample_index += 1;

            if push.sample_index >= push.max_samples {
                break;
            } else {
                progress += 1.0;
            }
        }

        if has_bounces {
            copy_image(&app.vk, diffuse, &mut previous_diffuses[i]);
        }

        diffuse.read_pixels(
            &app.vk,
            &mut app.groups[i].lightmap_diffuse_final,
            &app.staging_buffer,
        );
    }

    bake_direct_shader.destroy(&app.vk);

    let mut bake_bounce_shader = load_bake_bounce_shader(
        &app.vk,
        app.config.light_falloff,
        app.groups.len() as u32,
        (app.opaque_mesh.indices.len() / 3) as u32,
    );

    for group in &mut app.groups {
        group.emission.destroy(&app.vk);
    }

    for bounce_index in 0..bounce_count {
        let previous: Vec<vk::ImageView> = previous_diffuses.iter().map(|x| x.view()).collect();

        for i in 0..app.groups.len() {
            let message = format!(
                "Baking Bounce {}/{} (Group {}/{})",
                bounce_index + 1,
                bounce_count,
                i + 1,
                app.groups.len(),
            );

            let group = &app.groups[i];

            let settings = group.settings.clone();

            let width = group.settings.width;
            let height = group.settings.height;

            let mut push = BakeBouncePushConstants {
                width,
                height,
                sample_index: 0,
                max_samples: app.config.indirect_samples,
                bounce_index: bounce_index as u32,
                pad0: 0,
                pad1: 0,
                pad2: 0,
            };

            update_render_target(app, &settings, group.index);

            let visibility = &mut app.render_target.visibility;
            let diffuse = &mut app.render_target.diffuse;

            let shader = &bake_bounce_shader;

            update_bake_bounce_shader(
                &app.vk,
                shader,
                app.tlas.acceleration_structure(),
                visibility.view(),
                &albedos,
                &previous,
                diffuse.view(),
                app.texture_sampler,
                app.gpu_mesh.index_buffer.buffer,
                app.gpu_mesh.vertex_buffer.buffer,
            );

            let cmd = app.vk.command_buffer;
            let vk = &app.vk.device;

            let groups_x = (width + 7) / 8;
            let groups_y = (height + 7) / 8;

            let begin_info = vk::CommandBufferBeginInfo {
                flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
                ..Default::default()
            };

            loop {
                (log)(LogMessage::progress(&message, progress * progress_scale));

                unsafe {
                    vk.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())
                        .unwrap();

                    vk.begin_command_buffer(cmd, &begin_info).unwrap();

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

                    vk.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, shader.pipeline);

                    vk.cmd_bind_descriptor_sets(
                        cmd,
                        vk::PipelineBindPoint::COMPUTE,
                        shader.pipeline_layout,
                        0,
                        &[shader.descriptor_set],
                        &[],
                    );

                    let constants_bytes = as_bytes(&push);

                    vk.cmd_push_constants(
                        cmd,
                        shader.pipeline_layout,
                        vk::ShaderStageFlags::COMPUTE,
                        0,
                        &constants_bytes,
                    );

                    vk.cmd_dispatch(cmd, groups_x, groups_y, 1);

                    let cmds = [cmd];
                    let submit = vk::SubmitInfo::default().command_buffers(&cmds);

                    vk.end_command_buffer(cmd).unwrap();

                    vk.queue_submit(app.vk.compute_queue, &[submit], vk::Fence::null())
                        .unwrap();

                    vk.queue_wait_idle(app.vk.compute_queue).unwrap()
                };

                push.sample_index += 1;

                if push.sample_index >= push.max_samples {
                    break;
                } else {
                    progress += 1.0;
                }
            }

            unsafe { app.vk.device.queue_wait_idle(app.vk.compute_queue).unwrap() }

            diffuse.read_pixels(
                &app.vk,
                &mut app.groups[i].lightmap_diffuse_previous_bounce,
                &app.staging_buffer,
            );

            let group = &mut app.groups[i];
            let src = &group.lightmap_diffuse_previous_bounce;
            let dst = &mut group.lightmap_diffuse_final;

            for (d, s) in dst.iter_mut().zip(src) {
                *d += s;
            }
        }

        let last_bounce = bounce_index == bounce_count - 1;

        if !last_bounce {
            for i in 0..app.groups.len() {
                let group = &mut app.groups[i];
                let pixels = &group.lightmap_diffuse_previous_bounce;
                previous_diffuses[i].set_pixels(&app.vk, pixels, &app.staging_buffer);
            }
        }
    }

    bake_bounce_shader.destroy(&app.vk);

    for tex in &mut previous_diffuses {
        tex.destroy(&app.vk);
    }

    for i in 0..app.groups.len() {
        let group = &mut app.groups[i];
        let group_index = group.index;
        let mut pixels = &mut group.lightmap_diffuse_final;
        let settings = group.settings.clone();

        let width = group.settings.width;
        let height = group.settings.height;

        if settings.dilate {
            let start_time = std::time::Instant::now();
            let backface_threshold = 0.9;

            inpaint(pixels, width, height, backface_threshold, 32);

            let now = std::time::Instant::now();
            let elapsed = now.duration_since(start_time).as_secs_f32();

            let message = format!("Dilation complete {}s", elapsed);
            (log)(LogMessage::message(&message));
        }

        if settings.denoise {
            let start_time = std::time::Instant::now();

            match &oidn {
                Some(oidn) => {
                    oidn.denoise(&mut pixels, width as usize, height as usize);
                }
                None => {}
            }

            let now = std::time::Instant::now();
            let elapsed = now.duration_since(start_time).as_secs_f32();

            let message = format!("Denoise Complete {}s", elapsed);
            (log)(LogMessage::message(&message));
        }

        if settings.fix_seams {
            let start_time = std::time::Instant::now();

            fix_seams(
                &mut pixels,
                width,
                height,
                &app.seams,
                app.config.seams_debug,
                group_index,
            );

            let now = std::time::Instant::now();
            let elapsed = now.duration_since(start_time).as_secs_f32();

            let message = format!("Seam Fix Complete {}s", elapsed);
            (log)(LogMessage::message(&message));
        }

        let readback_data = LightmapReadbackData {
            group_index,
            ty: 0,
            pixels: pixels.as_ptr(),
            pixels_count: pixels.len() as u32,
            width,
            height,
        };

        (app.config.lightmap_read_callback)(readback_data);
    }
}

// fn _bake_lightmaps(app: &mut Stilb) {
//     let albedos: Vec<vk::ImageView> = app.groups.iter().map(|x| x.albedo.view()).collect();
//     let emissions: Vec<vk::ImageView> = app.groups.iter().map(|x| x.emission.view()).collect();

//     if app.config.is_preview {
//         render_preview(app);
//     } else {
//         let any_denoise = app.groups.iter().any(|x| x.settings.denoise);

//         let oidn = if any_denoise {
//             Some(Oidn::load().expect("failed to load oidn"))
//         } else {
//             None
//         };

//         let radiosity_iteration = true;
//         if radiosity_iteration {
//             let mut bake_direct_shader = load_bake_direct_shader(
//                 &app.vk,
//                 app.config.light_falloff,
//                 app.groups.len() as u32,
//                 (app.opaque_mesh.indices.len() / 3) as u32,
//             );

//             let group = &app.groups[0];
//             let width = group.settings.width;
//             let height = group.settings.height;
//             app.push.sample_index = 0;
//             let settings = group.settings.clone();
//             update_render_target(app, &settings, 0);

//             let RenderTarget::NonDirectional {
//                 visibility,
//                 diffuse,
//             } = &mut app.render_target
//             else {
//                 unreachable!()
//             };

//             let shader = &bake_direct_shader;

//             update_bake_direct_shader(
//                 &app.vk,
//                 shader,
//                 app.tlas.acceleration_structure(),
//                 visibility.view(),
//                 &albedos,
//                 &emissions,
//                 diffuse.view(),
//                 app.texture_sampler,
//                 app.gpu_mesh.index_buffer.buffer,
//                 app.gpu_mesh.vertex_buffer.buffer,
//                 app.gpu_lights.buffer,
//             );

//             let cmd = app.vk.command_buffer;
//             let vk = &app.vk.device;

//             let groups_x = (width + 7) / 8;
//             let groups_y = (height + 7) / 8;

//             let mut push = BakeDirectPushConstants {
//                 width,
//                 height,
//                 sample_index: 0,
//                 max_samples: settings.max_samples,
//                 lights_count: app.cpu_lights.len() as u32,
//                 pad0: 0,
//                 pad1: 0,
//                 pad2: 0,
//             };

//             let begin_info = vk::CommandBufferBeginInfo {
//                 flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
//                 ..Default::default()
//             };

//             loop {
//                 unsafe {
//                     vk.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())
//                         .unwrap();

//                     vk.begin_command_buffer(cmd, &begin_info).unwrap();

//                     if diffuse.layout() != vk::ImageLayout::GENERAL {
//                         let barrier = diffuse.barrier(
//                             vk::ImageLayout::GENERAL,
//                             vk::AccessFlags::default(),
//                             vk::AccessFlags::SHADER_WRITE,
//                         );
//                         vk.cmd_pipeline_barrier(
//                             cmd,
//                             vk::PipelineStageFlags::TOP_OF_PIPE,
//                             vk::PipelineStageFlags::COMPUTE_SHADER,
//                             vk::DependencyFlags::empty(),
//                             &[],
//                             &[],
//                             &[barrier],
//                         );
//                     }

//                     vk.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, shader.pipeline);

//                     vk.cmd_bind_descriptor_sets(
//                         cmd,
//                         vk::PipelineBindPoint::COMPUTE,
//                         shader.pipeline_layout,
//                         0,
//                         &[shader.descriptor_set],
//                         &[],
//                     );

//                     let constants_bytes = as_bytes(&push);

//                     vk.cmd_push_constants(
//                         cmd,
//                         shader.pipeline_layout,
//                         vk::ShaderStageFlags::COMPUTE,
//                         0,
//                         &constants_bytes,
//                     );

//                     vk.cmd_dispatch(cmd, groups_x, groups_y, 1);

//                     let cmds = [cmd];
//                     let submit = vk::SubmitInfo::default().command_buffers(&cmds);

//                     vk.end_command_buffer(cmd).unwrap();

//                     vk.queue_submit(app.vk.compute_queue, &[submit], vk::Fence::null())
//                         .unwrap();

//                     vk.queue_wait_idle(app.vk.compute_queue).unwrap()
//                 };

//                 push.sample_index += 1;

//                 if push.sample_index >= push.max_samples {
//                     break;
//                 }
//             }

//             bake_direct_shader.destroy(&app.vk);

//             unsafe {
//                 app.vk.device.device_wait_idle().unwrap();
//             }

//             let pixels_direct = diffuse.read_pixels(&app.vk);

//             let mut previous_bounce = Texture2D::new(
//                 &app.vk,
//                 width,
//                 height,
//                 vk::Format::R32G32B32A32_SFLOAT,
//                 vk::ImageUsageFlags::STORAGE
//                     | vk::ImageUsageFlags::TRANSFER_SRC
//                     | vk::ImageUsageFlags::TRANSFER_DST,
//             );
//             previous_bounce.set_pixels(&app.vk, &pixels_direct);

//             let mut bake_bounce_shader = load_bake_bounce_shader(
//                 &app.vk,
//                 app.config.light_falloff,
//                 app.groups.len() as u32,
//                 (app.opaque_mesh.indices.len() / 3) as u32,
//             );

//             let shader = &bake_bounce_shader;

//             update_bake_bounce_shader(
//                 &app.vk,
//                 shader,
//                 app.tlas.acceleration_structure(),
//                 visibility.view(),
//                 &albedos,
//                 &emissions,
//                 diffuse.view(),
//                 previous_bounce.view(),
//                 app.texture_sampler,
//                 app.gpu_mesh.index_buffer.buffer,
//                 app.gpu_mesh.vertex_buffer.buffer,
//             );

//             let cmd = app.vk.command_buffer;
//             let vk = &app.vk.device;

//             let mut push = BakeBouncePushConstants {
//                 width,
//                 height,
//                 sample_index: 0,
//                 max_samples: settings.max_samples,
//                 bounce_index: 0,
//                 pad0: 0,
//                 pad1: 0,
//                 pad2: 0,
//             };

//             loop {
//                 unsafe {
//                     vk.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())
//                         .unwrap();

//                     vk.begin_command_buffer(cmd, &begin_info).unwrap();

//                     if diffuse.layout() != vk::ImageLayout::GENERAL {
//                         let barrier = diffuse.barrier(
//                             vk::ImageLayout::GENERAL,
//                             vk::AccessFlags::default(),
//                             vk::AccessFlags::SHADER_WRITE,
//                         );
//                         vk.cmd_pipeline_barrier(
//                             cmd,
//                             vk::PipelineStageFlags::TOP_OF_PIPE,
//                             vk::PipelineStageFlags::COMPUTE_SHADER,
//                             vk::DependencyFlags::empty(),
//                             &[],
//                             &[],
//                             &[barrier],
//                         );
//                     }

//                     vk.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, shader.pipeline);

//                     vk.cmd_bind_descriptor_sets(
//                         cmd,
//                         vk::PipelineBindPoint::COMPUTE,
//                         shader.pipeline_layout,
//                         0,
//                         &[shader.descriptor_set],
//                         &[],
//                     );

//                     let constants_bytes = as_bytes(&push);

//                     vk.cmd_push_constants(
//                         cmd,
//                         shader.pipeline_layout,
//                         vk::ShaderStageFlags::COMPUTE,
//                         0,
//                         &constants_bytes,
//                     );

//                     vk.cmd_dispatch(cmd, groups_x, groups_y, 1);

//                     let cmds = [cmd];
//                     let submit = vk::SubmitInfo::default().command_buffers(&cmds);

//                     vk.end_command_buffer(cmd).unwrap();

//                     vk.queue_submit(app.vk.compute_queue, &[submit], vk::Fence::null())
//                         .unwrap();

//                     vk.queue_wait_idle(app.vk.compute_queue).unwrap()
//                 };

//                 push.sample_index += 1;

//                 if push.sample_index >= push.max_samples {
//                     break;
//                 }
//             }

//             unsafe {
//                 app.vk.device.device_wait_idle().unwrap();
//             }

//             let mut bounce_pixels = diffuse.read_pixels(&app.vk);

//             previous_bounce.set_pixels(&app.vk, &bounce_pixels);

//             let mut push = BakeBouncePushConstants {
//                 width,
//                 height,
//                 sample_index: 0,
//                 max_samples: settings.max_samples,
//                 bounce_index: 1,
//                 pad0: 0,
//                 pad1: 0,
//                 pad2: 0,
//             };

//             loop {
//                 unsafe {
//                     vk.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())
//                         .unwrap();

//                     vk.begin_command_buffer(cmd, &begin_info).unwrap();

//                     if diffuse.layout() != vk::ImageLayout::GENERAL {
//                         let barrier = diffuse.barrier(
//                             vk::ImageLayout::GENERAL,
//                             vk::AccessFlags::default(),
//                             vk::AccessFlags::SHADER_WRITE,
//                         );
//                         vk.cmd_pipeline_barrier(
//                             cmd,
//                             vk::PipelineStageFlags::TOP_OF_PIPE,
//                             vk::PipelineStageFlags::COMPUTE_SHADER,
//                             vk::DependencyFlags::empty(),
//                             &[],
//                             &[],
//                             &[barrier],
//                         );
//                     }

//                     vk.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, shader.pipeline);

//                     vk.cmd_bind_descriptor_sets(
//                         cmd,
//                         vk::PipelineBindPoint::COMPUTE,
//                         shader.pipeline_layout,
//                         0,
//                         &[shader.descriptor_set],
//                         &[],
//                     );

//                     let constants_bytes = as_bytes(&push);

//                     vk.cmd_push_constants(
//                         cmd,
//                         shader.pipeline_layout,
//                         vk::ShaderStageFlags::COMPUTE,
//                         0,
//                         &constants_bytes,
//                     );

//                     vk.cmd_dispatch(cmd, groups_x, groups_y, 1);

//                     let cmds = [cmd];
//                     let submit = vk::SubmitInfo::default().command_buffers(&cmds);

//                     vk.end_command_buffer(cmd).unwrap();

//                     vk.queue_submit(app.vk.compute_queue, &[submit], vk::Fence::null())
//                         .unwrap();

//                     vk.queue_wait_idle(app.vk.compute_queue).unwrap()
//                 };

//                 push.sample_index += 1;

//                 if push.sample_index >= push.max_samples {
//                     break;
//                 }
//             }

//             unsafe {
//                 app.vk.device.device_wait_idle().unwrap();
//             }

//             let callback = app.config.callback;

//             let mut bounce_pixels2 = diffuse.read_pixels(&app.vk);

//             for (i, p) in bounce_pixels.iter().enumerate() {
//                 bounce_pixels2[i] += p + pixels_direct[i];
//             }

//             let readback_data = ReadbackData {
//                 group_index: 0,
//                 ty: 0,
//                 pixels: bounce_pixels2.as_ptr(),
//                 pixels_count: bounce_pixels2.len() as u32,
//                 width,
//                 height,
//             };

//             callback(readback_data);

//             bake_bounce_shader.destroy(&app.vk);
//         } else {
//             for i in 0..app.groups.len() {
//                 let group_index = i as u32;
//                 let bake_start_time = std::time::Instant::now();

//                 let group = &app.groups[i];
//                 let width = group.settings.width;
//                 let height = group.settings.height;
//                 app.push.sample_index = 0;
//                 let settings = group.settings.clone();
//                 update_render_target(app, &settings, group_index);

//                 let RenderTarget::NonDirectional {
//                     visibility,
//                     diffuse,
//                 } = &mut app.render_target
//                 else {
//                     unreachable!()
//                 };

//                 update_bake_shader(
//                     &app.vk,
//                     &app.bake_shader,
//                     app.tlas.acceleration_structure(),
//                     visibility.view(),
//                     &albedos,
//                     &emissions,
//                     diffuse.view(),
//                     app.texture_sampler,
//                     app.gpu_mesh.index_buffer.buffer,
//                     app.gpu_mesh.vertex_buffer.buffer,
//                     app.gpu_lights.buffer,
//                     app.emissive_triangles_buffer.buffer,
//                 );

//                 loop {
//                     render_sample_bake(app, &settings);
//                     if app.push.sample_index >= settings.max_samples {
//                         break;
//                     }
//                 }

//                 let now = std::time::Instant::now();
//                 let bake_time = now.duration_since(bake_start_time).as_secs_f32();
//                 println!("bake complete in {}s", bake_time);

//                 unsafe {
//                     app.vk.device.device_wait_idle().unwrap();
//                 }

//                 let RenderTarget::NonDirectional {
//                     visibility: _visibility,
//                     diffuse,
//                 } = &mut app.render_target
//                 else {
//                     unreachable!()
//                 };

//                 let callback = app.config.callback;

//                 let mut pixels = diffuse.read_pixels(&app.vk);

//                 if settings.dilate {
//                     let start_time = std::time::Instant::now();
//                     let backface_threshold = 0.9;

//                     inpaint(&mut pixels, width, height, backface_threshold, 32);

//                     let now = std::time::Instant::now();
//                     let elapsed = now.duration_since(start_time).as_secs_f32();
//                     println!("dilated in {}s", elapsed);
//                 }

//                 if settings.denoise {
//                     let start_time = std::time::Instant::now();

//                     match &oidn {
//                         Some(oidn) => {
//                             oidn.denoise(&mut pixels, width as usize, height as usize);
//                         }
//                         None => {}
//                     }

//                     let now = std::time::Instant::now();
//                     let elapsed = now.duration_since(start_time).as_secs_f32();
//                     println!("denoised in {}s", elapsed);
//                 }

//                 if settings.fix_seams {
//                     let start_time = std::time::Instant::now();

//                     fix_seams(
//                         &mut pixels,
//                         width,
//                         height,
//                         &app.seams,
//                         app.config.seams_debug,
//                         group_index,
//                     );

//                     let now = std::time::Instant::now();
//                     let elapsed = now.duration_since(start_time).as_secs_f32();
//                     println!("fixed seams in {}s", elapsed);
//                 }

//                 let readback_data = ReadbackData {
//                     group_index,
//                     ty: 0,
//                     pixels: pixels.as_ptr(),
//                     pixels_count: pixels.len() as u32,
//                     width,
//                     height,
//                 };

//                 callback(readback_data);
//             }
//         }

//         if app.probes.len() > 0 {
//             update_bake_sh_shader(
//                 &app.vk,
//                 &app.bake_probes_shader,
//                 app.tlas.acceleration_structure(),
//                 app.probes_buffer.buffer,
//                 &albedos,
//                 &emissions,
//                 app.texture_sampler,
//                 app.gpu_mesh.index_buffer.buffer,
//                 app.gpu_mesh.vertex_buffer.buffer,
//                 app.gpu_lights.buffer,
//             );

//             let probes_samples = app.config.probe_samples;
//             let probe_bounces = app.config.probe_bounces;
//             initialize_bake_sh_push_constants(app, probes_samples, probe_bounces);

//             loop {
//                 render_sample_bake_probes(app);
//                 if app.push_probes.sample_index >= probes_samples {
//                     break;
//                 }
//             }

//             println!("light probes baked");

//             unsafe {
//                 app.vk.device.device_wait_idle().unwrap();
//             }

//             // println!("Probes:\n{:#?}", &app.probes);
//             app.vk
//                 .download_buffer(app.probes_buffer.buffer, &mut app.probes);

//             for probe in &mut app.probes {
//                 probe.normalize(probes_samples);
//             }
//             let readback_data = ReadbackProbesData {
//                 probes: app.probes.as_ptr(),
//                 pixels_count: app.probes.len() as u32,
//             };

//             (app.config.probes_callback)(readback_data);
//         }
//     }

//     unsafe {
//         app.vk.device.device_wait_idle().unwrap();
//     }
// }

// fn render_sample_bake(app: &mut Stilb, settings: &LightmapSettings) {
//     let width = settings.width;
//     let height = settings.height;

//     let vk = &app.vk.device;

//     let cmd = app.vk.command_buffer;

//     let begin_info = vk::CommandBufferBeginInfo {
//         flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
//         ..Default::default()
//     };

//     unsafe {
//         vk.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())
//             .unwrap();

//         vk.begin_command_buffer(cmd, &begin_info).unwrap();

//         let RenderTarget::NonDirectional {
//             visibility: _,
//             diffuse,
//         } = &mut app.render_target
//         else {
//             unreachable!()
//         };

//         if diffuse.layout() != vk::ImageLayout::GENERAL {
//             let barrier = diffuse.barrier(
//                 vk::ImageLayout::GENERAL,
//                 vk::AccessFlags::default(),
//                 vk::AccessFlags::SHADER_WRITE,
//             );
//             vk.cmd_pipeline_barrier(
//                 cmd,
//                 vk::PipelineStageFlags::TOP_OF_PIPE,
//                 vk::PipelineStageFlags::COMPUTE_SHADER,
//                 vk::DependencyFlags::empty(),
//                 &[],
//                 &[],
//                 &[barrier],
//             );
//         }

//         if app.push.sample_index < settings.max_samples {
//             render_sample(app, cmd, width, height);
//             app.push.sample_index += 1;
//         }
//         let vk = &app.vk.device;

//         let cmds = [cmd];
//         let submit = vk::SubmitInfo::default().command_buffers(&cmds);

//         vk.end_command_buffer(cmd).unwrap();

//         vk.queue_submit(app.vk.compute_queue, &[submit], vk::Fence::null())
//             .unwrap();

//         vk.queue_wait_idle(app.vk.compute_queue).unwrap()
//     };
// }

// fn render_sample_bake_probes(app: &mut Stilb) {
//     let vk = &app.vk.device;

//     let cmd = app.vk.command_buffer;

//     let begin_info = vk::CommandBufferBeginInfo {
//         flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
//         ..Default::default()
//     };

//     let shader = &app.bake_probes_shader;

//     let probes_count = app.probes.len() as u32;

//     let groups_x = (probes_count + 63) / 64;

//     let constants_bytes = as_bytes(&app.push_probes);

//     unsafe {
//         vk.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())
//             .unwrap();

//         vk.begin_command_buffer(cmd, &begin_info).unwrap();

//         vk.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, shader.pipeline);

//         vk.cmd_bind_descriptor_sets(
//             cmd,
//             vk::PipelineBindPoint::COMPUTE,
//             shader.pipeline_layout,
//             0,
//             &[shader.descriptor_set],
//             &[],
//         );

//         vk.cmd_push_constants(
//             cmd,
//             shader.pipeline_layout,
//             vk::ShaderStageFlags::COMPUTE,
//             0,
//             &constants_bytes,
//         );

//         vk.cmd_dispatch(cmd, groups_x, 1, 1);

//         let vk = &app.vk.device;

//         let cmds = [cmd];
//         let submit = vk::SubmitInfo::default().command_buffers(&cmds);

//         vk.end_command_buffer(cmd).unwrap();

//         vk.queue_submit(app.vk.compute_queue, &[submit], vk::Fence::null())
//             .unwrap();

//         vk.queue_wait_idle(app.vk.compute_queue).unwrap()
//     };

//     app.push_probes.sample_index += 1;
// }

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

        if app.preview_push_constants.sample_index == 0 {
            update_visibility_from_camera(app, cmd);
            app.preview_initialized = true;
            let clear = vk::ClearColorValue {
                float32: [0.0, 0.0, 0.0, 0.0],
            };

            let diffuse = &mut app.render_target.diffuse;

            clear_texture(&app.vk, diffuse, cmd, clear);
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

fn update_render_target(app: &mut Stilb, settings: &LightmapSettings, group_index: u32) {
    let (width, height) = if app.config.is_preview {
        (
            app.config.preview_settings.width,
            app.config.preview_settings.height,
        )
    } else {
        (settings.width, settings.height)
    };

    if app.config.is_preview {
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

        let visibility = render_visibility_from_camera(app, width, height);

        app.render_target.diffuse = diffuse;
        app.render_target.visibility = visibility;
    } else {
        render_visibility_from_lightmap(app, width, height, group_index);

        let diffuse = &mut app.render_target.diffuse;

        if diffuse.width() != width || diffuse.height() != height {
            if !diffuse.image().is_null() {
                diffuse.destroy(&app.vk);
            }

            app.render_target.diffuse = Texture2D::new(
                &app.vk,
                width,
                height,
                vk::Format::R32G32B32A32_SFLOAT,
                vk::ImageUsageFlags::STORAGE
                    | vk::ImageUsageFlags::TRANSFER_SRC
                    | vk::ImageUsageFlags::TRANSFER_DST,
                String::from("Lightmap Diffuse"),
            );
        }
    }

    initialize_preview_push_constants(
        app,
        width,
        height,
        app.config.direct_samples,
        app.config.bounce_count,
    );

    app.preview_initialized = false;
}

#[inline]
fn edge_side(ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32) -> f32 {
    (px - ax) * (by - ay) - (py - ay) * (bx - ax)
}

fn extract_emissive_triangles(app: &mut Stilb) {
    // todo indices of both opaque and transparent
    let vertices = &app.opaque_mesh.vertices;
    let indices = &app.opaque_mesh.indices;
    let mut emissive_triangles = Vec::new();

    if app.config.mis {
        for (primitive_id, chunk) in indices.chunks(3).enumerate() {
            if chunk.len() < 3 {
                break;
            }

            let v0 = &vertices[chunk[0] as usize];
            let v1 = &vertices[chunk[1] as usize];
            let v2 = &vertices[chunk[2] as usize];

            let uv0 = v0.uv;
            let uv1 = v1.uv;
            let uv2 = v2.uv;

            let group_index = (v0.flags & 0xFFFF) as usize;
            let group = &app.groups[group_index];
            let pixels = &group.emission_pixels;

            let min_u = uv0.x.min(uv1.x).min(uv2.x).clamp(0.0, 1.0);
            let max_u = uv0.x.max(uv1.x).max(uv2.x).clamp(0.0, 1.0);
            let min_v = uv0.y.min(uv1.y).min(uv2.y).clamp(0.0, 1.0);
            let max_v = uv0.y.max(uv1.y).max(uv2.y).clamp(0.0, 1.0);

            let width = group.settings.width;
            let height = group.settings.height;

            let tex_w = width as f32;
            let tex_h = height as f32;

            let start_x = ((min_u * tex_w).floor() as u32).min(width - 1);
            let end_x = ((max_u * tex_w).ceil() as u32).min(width - 1);
            let start_y = ((min_v * tex_h).floor() as u32).min(height - 1);
            let end_y = ((max_v * tex_h).ceil() as u32).min(height - 1);

            let mut is_emissive = false;
            'pixel_search: for y in start_y..=end_y {
                for x in start_x..=end_x {
                    let p_u = (x as f32 + 0.5) / tex_w;
                    let p_v = (y as f32 + 0.5) / tex_h;

                    let w0 = edge_side(uv1.x, uv1.y, uv2.x, uv2.y, p_u, p_v);
                    let w1 = edge_side(uv2.x, uv2.y, uv0.x, uv0.y, p_u, p_v);
                    let w2 = edge_side(uv0.x, uv0.y, uv1.x, uv1.y, p_u, p_v);

                    let is_inside = (w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0)
                        || (w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0);

                    if is_inside {
                        let emissive_r = pixels[((y * width + x) * 4 + 0) as usize];
                        let emissive_g = pixels[((y * width + x) * 4 + 1) as usize];
                        let emissive_b = pixels[((y * width + x) * 4 + 2) as usize];

                        if emissive_r > 0.0 || emissive_g > 0.0 || emissive_b > 0.0 {
                            is_emissive = true;
                            break 'pixel_search;
                        }
                    }
                }
            }

            if is_emissive {
                emissive_triangles.push(primitive_id as u32);
            }
        }

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
        let dummy = [0u32];
        app.emissive_triangles_buffer = Buffer::new(
            &app.vk,
            String::from("Emissive Triangles"),
            &dummy,
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
        app: &mut Stilb,
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

impl Stilb {
    pub fn new(config: StilbConfig) -> Stilb {
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

        let filter = if config.texture_filter == TextureSamplerFilter::Linear {
            vk::Filter::LINEAR
        } else {
            vk::Filter::NEAREST
        };

        let sampler_info = vk::SamplerCreateInfo::default()
            .mag_filter(filter)
            .min_filter(filter)
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

        let texture_sampler = unsafe { vk.device.create_sampler(&sampler_info, None).unwrap() };

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
            texture_sampler,
            preview_push_constants,
            render_target,
            probes: Vec::new(),
            probes_buffer: Buffer::null(),
            bake_probes_shader: ComputeShader::null(),
            seams: Vec::new(),
            emissive_triangles: Vec::new(),
            emissive_triangles_buffer: Buffer::null(),
            staging_buffer,
        }
    }
}
