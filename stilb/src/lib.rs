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
use crate::math::Vector2;
use crate::seams::{Seam, fix_seams};
use crate::sh::SHProbeL2;
use crate::shaders::bake_direct::{
    BakeDirectPushConstants, load_bake_direct_shader, update_bake_direct_shader,
};
use crate::shaders::compact_visibility::{
    load_shader_compact_visibility, update_shader_compact_visibility,
};
use crate::shaders::compaction_mask::{
    CompactionPushConstants, load_shader_compaction_mask, update_shader_compaction_mask,
};
use crate::shaders::decompact::{load_shader_decompact, update_shader_decompact};
use crate::shaders::dilate::{DilatePushConstants, load_shader_dilate, update_shader_dilate};
use crate::{
    camera::Camera,
    compute_shader::{
        ComputeShader, PreviewPushConstants, load_init_from_camera_shader, load_preview_shader,
        update_init_from_camera_shader, update_preview_shader,
    },
    graphics_shader::{VisibilityPushConstants, load_visibility_shader},
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
mod pack;
mod seams;
mod sh;
mod shader_bindings;
mod shaders;
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

    pub probes: Vec<SHProbeL2>,

    pub probes_buffer: Buffer,
    pub bake_probes_shader: ComputeShader,

    pub adjust_samples_shader: ComputeShader,

    pub staging_buffer: Buffer,

    pub render_target: RenderTarget,

    pub constants: SpecializationConstants,
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
    let mut shader = load_visibility_shader(vk, visibility, false, &app.constants);
    let mut shader_convervative = load_visibility_shader(vk, visibility, true, &app.constants);

    let albedos: Vec<vk::ImageView> = app.groups.iter().map(|x| x.albedo.view()).collect();

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
            rt_width: visibility.width(),
            rt_height: visibility.height(),
            pad1: 0,
            pad2: 0,
            conservative: 1,
        };
        let constants_bytes = as_bytes(&push);
        vk.device.cmd_push_constants(
            cmd,
            shader_convervative.pipeline_layout,
            vk::ShaderStageFlags::FRAGMENT | vk::ShaderStageFlags::VERTEX,
            0,
            &constants_bytes,
        );

        vk.device.cmd_draw(cmd, mesh.index_len * 3, 1, 0, 0);

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
            rt_width: visibility.width(),
            rt_height: visibility.height(),
            pad1: 0,
            pad2: 0,
            conservative: 0,
        };
        let constants_bytes = as_bytes(&push);
        vk.device.cmd_push_constants(
            cmd,
            shader.pipeline_layout,
            vk::ShaderStageFlags::FRAGMENT | vk::ShaderStageFlags::VERTEX,
            0,
            &constants_bytes,
        );

        vk.device.cmd_draw(cmd, mesh.index_len * 3, 25, 0, 0);

        vk.device.cmd_end_render_pass(cmd);

        // let shader = &app.adjust_samples_shader;

        // let albedos: Vec<vk::ImageView> = app.groups.iter().map(|x| x.albedo.view()).collect();

        // adjust sample positions
        // update_adjust_samples_shader(
        //     &vk,
        //     shader,
        //     app.tlas.acceleration_structure(),
        //     visibility.view(),
        //     &albedos,
        //     app.gpu_mesh.index_buffer.buffer,
        //     app.gpu_mesh.vertex_buffer.buffer,
        // );

        // vk.device
        //     .cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, shader.pipeline);

        // vk.device.cmd_bind_descriptor_sets(
        //     cmd,
        //     vk::PipelineBindPoint::COMPUTE,
        //     shader.pipeline_layout,
        //     0,
        //     &[shader.descriptor_set],
        //     &[],
        // );

        // let groups_x = (width + 7) / 8;
        // let groups_y = (height + 7) / 8;

        // vk.device.cmd_dispatch(cmd, groups_x, groups_y, 1);
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

    app.constants = SpecializationConstants {
        use_camera: 0,
        light_falloff_type: app.config.light_falloff as u32,
        transparent_primitive_offset: (app.opaque_mesh.indices.len() / 3) as u32,
        emissive_triangles_count: app.emissive_triangles.len() as u32,
        multiple_importance_sampling: app.config.mis as u32,
        lightmap_group_count: app.groups.len() as u32,
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
        let dummy_buffer = [Light {
            position: Vector3::ZERO,
            ty: lights::LightType::Directional,
            direction: Vector3::ZERO,
            range: 0.0,
            color: Vector3::ZERO,
            shadow_radius_or_angle: 0.0,
            spot_inner_percent: 0.0,
            spot_outer: 0.0,
            pad0: 0,
            pad1: 0,
        }];
        app.gpu_lights = Buffer::new(
            &app.vk,
            String::from("Lights"),
            &dummy_buffer,
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

    let mesh::AccelerationStructureType::RayQuery(blas) = &app.gpu_mesh.acceleration_structure
    else {
        panic!("Expected RayQuery variant");
    };

    app.tlas = create_tlas(&app.vk, blas);

    if app.config.is_preview {
        render_preview(app);
    } else {
        render_lightmaps3(app);
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

    app.init_from_camera_shader = load_init_from_camera_shader(&app.vk, &app.constants);

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

fn render_lightmaps3(app: &mut Stilb) {
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
            pad0: 0,
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
            pad0: 0,
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

    let mut compacted_diffuse = Buffer::empty(
        &app.vk,
        "Diffuse Buffer".to_owned(),
        compacted_pixels_count as u64 * (std::mem::size_of::<f32>() * 3) as u64,
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
        compacted_diffuse.buffer,
    );

    let mut bake_direct_push = BakeDirectPushConstants {
        compacted_count: compacted_pixels_count,
        sample_index: 0,
        max_samples: app.config.direct_samples,
        lights_count: app.cpu_lights.len() as u32,
    };

    let groups_x = (compacted_pixels_count + 63) / 64;

    let message = format!("Baking Direct");

    let mut progress = 0.0;
    let progress_max =
        app.config.direct_samples + app.config.indirect_samples * app.config.bounce_count;
    let progress_scale = 1.0 / progress_max as f32;

    for sample_index in 0..app.config.direct_samples {
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

            vk.cmd_dispatch(cmd, groups_x, 1, 1);
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

    // todo bake bounces

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
        compacted_diffuse.buffer,
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

    let mut dilate_shader = load_shader_dilate(&app.vk, &app.constants);

    for group_index in 0..app.groups.len() {
        let group = &app.groups[group_index].settings;

        let compaction_push = CompactionPushConstants {
            width: group.width,
            height: group.height,
            offset: expanded_groups_start[group_index] as u32,
            pad0: 0,
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

            if group.dilate {
                let start_time = std::time::Instant::now();

                let dilate_push = DilatePushConstants {
                    width: group.width,
                    height: group.height,
                    pad0: 0,
                    pad1: 0,
                };
                let dilate_push_bytes = as_bytes(&dilate_push);
                let groups_x = (group.width + 7) / 8;
                let groups_y = (group.height + 7) / 8;

                update_shader_dilate(&app.vk, &dilate_shader, staging_buffer_lightmap.buffer);

                let vk_dev = &app.vk.device;
                let shader = &dilate_shader;
                let cmd = app.vk.begin_single_use_cmd();
                vk_dev.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, shader.pipeline);
                vk_dev.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::COMPUTE,
                    shader.pipeline_layout,
                    0,
                    &[shader.descriptor_set],
                    &[],
                );
                vk_dev.cmd_push_constants(
                    cmd,
                    shader.pipeline_layout,
                    vk::ShaderStageFlags::COMPUTE,
                    0,
                    &dilate_push_bytes,
                );
                vk_dev.cmd_dispatch(cmd, groups_x, groups_y, 1);
                app.vk.end_single_use_cmd(cmd);

                let elapsed = std::time::Instant::now()
                    .duration_since(start_time)
                    .as_secs_f32();
                (log)(LogMessage::message(&format!(
                    "Dilation complete {}s",
                    elapsed
                )));
            }

            if group.denoise {
                let start_time = std::time::Instant::now();

                match &oidn {
                    Some(oidn) => {
                        oidn.denoise(pixels, group.width as usize, group.height as usize, false);
                    }
                    None => {}
                }

                let now = std::time::Instant::now();
                let elapsed = now.duration_since(start_time).as_secs_f32();

                let message = format!("Denoise Complete {}s", elapsed);
                (log)(LogMessage::message(&message));
            }

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

            let readback_data = LightmapReadbackData {
                group_index: group_index as u32,
                ty: 0,
                pixels: pixels.as_ptr(),
                pixels_count: pixels.len() as u32,
                width: group.width,
                height: group.height,
            };
            (app.config.lightmap_read_callback)(readback_data);
        };
    }

    dilate_shader.destroy(&app.vk);
    decompact_shader.destroy(&app.vk);
    compacted_visibility.destroy(&app.vk);
    staging_buffer_lightmap.destroy(&app.vk);

    // light probes
    if app.probes.len() > 0 {
        let mut shader = load_bake_light_probes_shader(&app.vk, &app.constants);

        update_bake_light_probes_shader(
            &app.vk,
            &shader,
            app.tlas.acceleration_structure(),
            app.probes_buffer.buffer,
            &albedos,
            &emissions,
            compacted_diffuse.buffer,
            app.gpu_mesh.index_buffer.buffer,
            app.gpu_mesh.vertex_buffer.buffer,
            app.gpu_lights.buffer,
            compaction_buffer.buffer,
            group_info_buffer.buffer,
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

        // println!("Probes:\n{:#?}", &app.probes);
        // app.vk
        //     .download_buffer(app.probes_buffer.buffer, &mut app.probes);

        // std::ptr::copy_nonoverlapping(
        //     compaction_buffer_cpu.as_ptr() as *const u8,
        //     staging_buffer_compaction.ptr as *mut u8,
        //     compaction_buffer.bytes as usize,
        // );

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

    compacted_diffuse.destroy(&app.vk);
    compaction_buffer.destroy(&app.vk);
    group_info_buffer.destroy(&app.vk);
}

// fn deinterleave_with_zero(mut word: u32) -> u32 {
//     word &= 0x5555_5555;
//     word = (word | (word >> 1)) & 0x3333_3333;
//     word = (word | (word >> 2)) & 0x0f0f_0f0f;
//     word = (word | (word >> 4)) & 0x00ff_00ff;
//     word = (word | (word >> 8)) & 0x0000_ffff;
//     word
// }

// fn index_to_uv(index: u32) -> (u32, u32) {
//     (
//         deinterleave_with_zero(index),
//         deinterleave_with_zero(index >> 1),
//     )
// }
// fn render_lightmaps(app: &mut Stilb) {
//     let albedos: Vec<vk::ImageView> = app.groups.iter().map(|x| x.albedo.view()).collect();
//     let emissions: Vec<vk::ImageView> = app.groups.iter().map(|x| x.emission.view()).collect();

//     let any_denoise = app.groups.iter().any(|x| x.settings.denoise);
//     let log = app.config.log_callback;

//     let oidn = if any_denoise {
//         let oidn = Oidn::load();
//         if oidn.is_err() {
//             let err = oidn.err();
//             match err {
//                 Some(err) => {
//                     let message = "Failed to load Open Image Denoise";
//                     (log)(LogMessage::message(&message));
//                     (log)(LogMessage::message(&err.to_string()))
//                 }
//                 None => {}
//             };
//             None
//         } else {
//             Some(oidn.unwrap())
//         }
//     } else {
//         None
//     };

//     app.adjust_samples_shader = load_adjust_samples_shader(&app.vk, &app.constants);

//     let mut max_resolution = (1, 1);
//     for group in &app.groups {
//         max_resolution.0 = u32::max(max_resolution.0, group.settings.width);
//         max_resolution.1 = u32::max(max_resolution.1, group.settings.height);
//     }
//     let dominant_direction_pixels_size =
//         std::mem::size_of::<f32>() as u32 * max_resolution.0 * max_resolution.1 * 4;

//     let usage = vk::BufferUsageFlags::TRANSFER_DST
//         | vk::BufferUsageFlags::STORAGE_BUFFER
//         | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
//         | vk::BufferUsageFlags::TRANSFER_SRC;

//     // todo diffuse should probably be switched to a buffer as well to not waste the alpha and for easier copy

//     let dominant_direction_buffer = Buffer::empty(
//         &app.vk,
//         "Dominant Direction".to_owned(),
//         dominant_direction_pixels_size as vk::DeviceSize,
//         usage,
//         vk::MemoryPropertyFlags::DEVICE_LOCAL,
//     );
//     let mut directional_pixels_temp =
//         vec![0.0f32; (max_resolution.0 * max_resolution.1 * 4) as usize];
//     let usage = vk::BufferUsageFlags::TRANSFER_DST;
//     let properties = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
//     let (staging_buffer, staging_memory, _) = app.vk.create_buffer(
//         dominant_direction_pixels_size as vk::DeviceSize,
//         usage,
//         properties,
//     );

//     let mut bake_direct_shader = load_bake_direct_shader(&app.vk, &app.constants);

//     let lights_count = app.cpu_lights.len() as u32;

//     let mut previous_diffuses = Vec::new();

//     let bounce_count = app.config.bounce_count;

//     for i in 0..app.groups.len() {
//         let group = &app.groups[i];
//         let settings = group.settings.clone();

//         let diffuse = Texture2D::new(
//             &app.vk,
//             settings.width,
//             settings.height,
//             vk::Format::R32G32B32A32_SFLOAT,
//             vk::ImageUsageFlags::SAMPLED
//                 | vk::ImageUsageFlags::TRANSFER_SRC
//                 | vk::ImageUsageFlags::TRANSFER_DST,
//             String::from("Diffuse Copy"),
//         );

//         previous_diffuses.push(diffuse);
//     }

//     let log = app.config.log_callback;

//     let mut progress = 0.0;
//     let progress_max = app.groups.len() as u32 * app.config.direct_samples
//         + app.groups.len() as u32 * app.config.indirect_samples * app.config.bounce_count;

//     let progress_scale = 1.0 / progress_max as f32;

//     for i in 0..app.groups.len() {
//         let message = format!("Baking Direct (Group {}/{})", i + 1, app.groups.len());

//         let group = &app.groups[i];

//         let settings = group.settings.clone();

//         let width = group.settings.width;
//         let height = group.settings.height;

//         let mut push = BakeDirectPushConstants {
//             width,
//             height,
//             sample_index: 0,
//             max_samples: app.config.direct_samples,
//             lights_count,
//             pad0: 0,
//             pad1: 0,
//             pad2: 0,
//         };

//         update_render_target(app, &settings, group.index);

//         let visibility = &mut app.render_target.visibility;
//         let diffuse = &mut app.render_target.diffuse;

//         let shader = &bake_direct_shader;

//         update_bake_direct_shader(
//             &app.vk,
//             shader,
//             app.tlas.acceleration_structure(),
//             visibility.view(),
//             &albedos,
//             &emissions,
//             diffuse.view(),
//             app.texture_sampler,
//             app.gpu_mesh.index_buffer.buffer,
//             app.gpu_mesh.vertex_buffer.buffer,
//             app.gpu_lights.buffer,
//             app.emissive_triangles_buffer.buffer,
//             dominant_direction_buffer.buffer,
//         );

//         let cmd = app.vk.command_buffer;
//         let vk = &app.vk.device;

//         let groups_x = (width + 7) / 8;
//         let groups_y = (height + 7) / 8;

//         let begin_info = vk::CommandBufferBeginInfo {
//             flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
//             ..Default::default()
//         };

//         loop {
//             (log)(LogMessage::progress(&message, progress * progress_scale));

//             unsafe {
//                 vk.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())
//                     .unwrap();

//                 vk.begin_command_buffer(cmd, &begin_info).unwrap();

//                 if diffuse.layout() != vk::ImageLayout::GENERAL {
//                     let barrier = diffuse.barrier(
//                         vk::ImageLayout::GENERAL,
//                         vk::AccessFlags::default(),
//                         vk::AccessFlags::SHADER_WRITE,
//                     );
//                     vk.cmd_pipeline_barrier(
//                         cmd,
//                         vk::PipelineStageFlags::TOP_OF_PIPE,
//                         vk::PipelineStageFlags::COMPUTE_SHADER,
//                         vk::DependencyFlags::empty(),
//                         &[],
//                         &[],
//                         &[barrier],
//                     );
//                 }

//                 vk.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, shader.pipeline);

//                 vk.cmd_bind_descriptor_sets(
//                     cmd,
//                     vk::PipelineBindPoint::COMPUTE,
//                     shader.pipeline_layout,
//                     0,
//                     &[shader.descriptor_set],
//                     &[],
//                 );

//                 let constants_bytes = as_bytes(&push);

//                 vk.cmd_push_constants(
//                     cmd,
//                     shader.pipeline_layout,
//                     vk::ShaderStageFlags::COMPUTE,
//                     0,
//                     &constants_bytes,
//                 );

//                 vk.cmd_dispatch(cmd, groups_x, groups_y, 1);

//                 let cmds = [cmd];
//                 let submit = vk::SubmitInfo::default().command_buffers(&cmds);

//                 vk.end_command_buffer(cmd).unwrap();

//                 vk.queue_submit(app.vk.compute_queue, &[submit], vk::Fence::null())
//                     .unwrap();

//                 vk.queue_wait_idle(app.vk.compute_queue).unwrap()
//             };

//             push.sample_index += 1;

//             if push.sample_index >= push.max_samples {
//                 break;
//             } else {
//                 progress += 1.0;
//             }
//         }

//         if bounce_count > 0 {
//             copy_image(&app.vk, diffuse, &mut previous_diffuses[i]);
//         }

//         diffuse.read_pixels(
//             &app.vk,
//             &mut app.groups[i].lightmap_diffuse_final,
//             &app.staging_buffer,
//         );

//         app.groups[i].lightmap_directional = vec![0.0; app.groups[i].lightmap_diffuse_final.len()];

//         let regions = vk::BufferCopy {
//             src_offset: 0,
//             dst_offset: 0,
//             size: (app.groups[i].lightmap_directional.len() * std::mem::size_of::<f32>())
//                 as vk::DeviceSize,
//         };
//         app.vk.download_buffer_with_staging(
//             dominant_direction_buffer.buffer,
//             &mut directional_pixels_temp,
//             staging_buffer,
//             staging_memory,
//             regions,
//         );
//         let dir = &mut app.groups[i].lightmap_directional;
//         for i in 0..dir.len() {
//             dir[i] = directional_pixels_temp[i];
//         }
//     }

//     bake_direct_shader.destroy(&app.vk);

//     let mut bake_bounce_shader = load_bake_bounce_shader(&app.vk, &app.constants);

//     for bounce_index in 0..bounce_count {
//         let previous: Vec<vk::ImageView> = previous_diffuses.iter().map(|x| x.view()).collect();

//         for i in 0..app.groups.len() {
//             let message = format!(
//                 "Baking Bounce {}/{} (Group {}/{})",
//                 bounce_index + 1,
//                 bounce_count,
//                 i + 1,
//                 app.groups.len(),
//             );

//             let group = &app.groups[i];

//             let settings = group.settings.clone();

//             let width = group.settings.width;
//             let height = group.settings.height;

//             let mut push = BakeBouncePushConstants {
//                 width,
//                 height,
//                 sample_index: 0,
//                 max_samples: app.config.indirect_samples,
//                 bounce_index: bounce_index as u32,
//                 pad0: 0,
//                 pad1: 0,
//                 pad2: 0,
//             };

//             update_render_target(app, &settings, group.index);

//             let visibility = &mut app.render_target.visibility;
//             let diffuse = &mut app.render_target.diffuse;

//             let shader = &bake_bounce_shader;

//             update_bake_bounce_shader(
//                 &app.vk,
//                 shader,
//                 app.tlas.acceleration_structure(),
//                 visibility.view(),
//                 &albedos,
//                 &previous,
//                 diffuse.view(),
//                 app.texture_sampler,
//                 app.gpu_mesh.index_buffer.buffer,
//                 app.gpu_mesh.vertex_buffer.buffer,
//                 dominant_direction_buffer.buffer,
//             );

//             let cmd = app.vk.command_buffer;
//             let vk = &app.vk.device;

//             let groups_x = (width + 7) / 8;
//             let groups_y = (height + 7) / 8;

//             let begin_info = vk::CommandBufferBeginInfo {
//                 flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
//                 ..Default::default()
//             };

//             loop {
//                 (log)(LogMessage::progress(&message, progress * progress_scale));

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
//                 } else {
//                     progress += 1.0;
//                 }
//             }

//             unsafe { app.vk.device.queue_wait_idle(app.vk.compute_queue).unwrap() }

//             diffuse.read_pixels(
//                 &app.vk,
//                 &mut app.groups[i].lightmap_diffuse_previous_bounce,
//                 &app.staging_buffer,
//             );

//             let group = &mut app.groups[i];
//             let src = &group.lightmap_diffuse_previous_bounce;
//             let dst = &mut group.lightmap_diffuse_final;

//             for (d, s) in dst.iter_mut().zip(src) {
//                 *d += s;
//             }

//             let regions = vk::BufferCopy {
//                 src_offset: 0,
//                 dst_offset: 0,
//                 size: (group.lightmap_directional.len() * std::mem::size_of::<f32>())
//                     as vk::DeviceSize,
//             };
//             app.vk.download_buffer_with_staging(
//                 dominant_direction_buffer.buffer,
//                 &mut directional_pixels_temp,
//                 staging_buffer,
//                 staging_memory,
//                 regions,
//             );
//             let src = &directional_pixels_temp;
//             let dst = &mut group.lightmap_directional;

//             // .w stores normal so skip that
//             for i in 0..(group.settings.width * group.settings.height) as usize {
//                 let i = i * 4;

//                 let x = i + 0;
//                 let y = i + 1;
//                 let z = i + 2;

//                 dst[x] += src[x];
//                 dst[y] += src[y];
//                 dst[z] += src[z];
//             }
//         }

//         let last_bounce = bounce_index == bounce_count - 1;

//         if !last_bounce {
//             for i in 0..app.groups.len() {
//                 let group = &mut app.groups[i];
//                 let pixels = &group.lightmap_diffuse_previous_bounce;
//                 previous_diffuses[i].set_pixels(&app.vk, pixels, &app.staging_buffer);
//             }
//         }
//     }

//     bake_bounce_shader.destroy(&app.vk);
//     app.adjust_samples_shader.destroy(&app.vk);

//     unsafe {
//         app.vk.device.destroy_buffer(staging_buffer, None);
//         app.vk.device.free_memory(staging_memory, None);
//     };

//     for i in 0..app.groups.len() {
//         let group = &mut app.groups[i];
//         let group_index = group.index;
//         let pixels = &mut group.lightmap_diffuse_final;
//         let direction_pixels = &mut group.lightmap_directional;

//         let settings = group.settings.clone();

//         let width = group.settings.width;
//         let height = group.settings.height;

//         if settings.dilate {
//             let start_time = std::time::Instant::now();
//             let backface_threshold = 0.0;

//             inpaint(pixels, width, height, backface_threshold, 16);

//             let now = std::time::Instant::now();
//             let elapsed = now.duration_since(start_time).as_secs_f32();

//             let message = format!("Dilation complete {}s", elapsed);
//             (log)(LogMessage::message(&message));
//         }

//         if settings.denoise {
//             let start_time = std::time::Instant::now();

//             match &oidn {
//                 Some(oidn) => {
//                     oidn.denoise(pixels, width as usize, height as usize, false);
//                 }
//                 None => {}
//             }

//             let now = std::time::Instant::now();
//             let elapsed = now.duration_since(start_time).as_secs_f32();

//             let message = format!("Denoise Complete {}s", elapsed);
//             (log)(LogMessage::message(&message));
//         }

//         if settings.fix_seams {
//             let start_time = std::time::Instant::now();

//             fix_seams(
//                 pixels,
//                 width,
//                 height,
//                 &app.seams,
//                 app.config.seams_debug,
//                 group_index,
//             );

//             let now = std::time::Instant::now();
//             let elapsed = now.duration_since(start_time).as_secs_f32();

//             let message = format!("Seam Fix Complete {}s", elapsed);
//             (log)(LogMessage::message(&message));
//         }

//         encode_directional_lightmap_rgb(pixels, direction_pixels);
//         if settings.denoise {
//             match &oidn {
//                 Some(oidn) => {
//                     oidn.denoise(direction_pixels, width as usize, height as usize, true);
//                 }
//                 None => {}
//             }
//         }
//         if settings.fix_seams {
//             fix_seams(
//                 direction_pixels,
//                 width,
//                 height,
//                 &app.seams,
//                 app.config.seams_debug,
//                 group_index,
//             );
//         }
//         encode_directional_lightmap_alpha(pixels, direction_pixels);

//         let readback_data = LightmapReadbackData {
//             group_index,
//             ty: 0,
//             pixels: pixels.as_ptr(),
//             pixels_count: pixels.len() as u32,
//             width,
//             height,
//         };

//         (app.config.lightmap_read_callback)(readback_data);

//         let readback_data = LightmapReadbackData {
//             group_index,
//             ty: 1,
//             pixels: direction_pixels.as_ptr(),
//             pixels_count: direction_pixels.len() as u32,
//             width,
//             height,
//         };

//         (app.config.lightmap_read_callback)(readback_data);

//         // todo emissions and previous diffuse can be merged here for light probes
//         // but one is flipped for some reason
//         previous_diffuses[i].set_pixels(&app.vk, pixels, &app.staging_buffer);
//     }

//     if app.probes.len() > 0 {
//         let shader = &app.bake_probes_shader;

//         let diffuses: Vec<vk::ImageView> = previous_diffuses.iter().map(|x| x.view()).collect();

//         update_bake_sh_shader(
//             &app.vk,
//             shader,
//             app.tlas.acceleration_structure(),
//             app.probes_buffer.buffer,
//             &albedos,
//             &emissions,
//             &diffuses,
//             app.texture_sampler,
//             app.gpu_mesh.index_buffer.buffer,
//             app.gpu_mesh.vertex_buffer.buffer,
//             app.gpu_lights.buffer,
//         );

//         let mut push = BakeSHPushConstants {
//             lights_count,
//             max_samples: app.config.probe_samples,
//             sample_index: 0,
//             probes_count: app.probes.len() as u32,
//         };

//         loop {
//             let vk = &app.vk.device;

//             let cmd = app.vk.command_buffer;

//             let begin_info = vk::CommandBufferBeginInfo {
//                 flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
//                 ..Default::default()
//             };

//             let probes_count = app.probes.len() as u32;

//             let groups_x = (probes_count + 63) / 64;

//             let constants_bytes = as_bytes(&push);

//             unsafe {
//                 vk.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())
//                     .unwrap();

//                 vk.begin_command_buffer(cmd, &begin_info).unwrap();

//                 vk.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, shader.pipeline);

//                 vk.cmd_bind_descriptor_sets(
//                     cmd,
//                     vk::PipelineBindPoint::COMPUTE,
//                     shader.pipeline_layout,
//                     0,
//                     &[shader.descriptor_set],
//                     &[],
//                 );

//                 vk.cmd_push_constants(
//                     cmd,
//                     shader.pipeline_layout,
//                     vk::ShaderStageFlags::COMPUTE,
//                     0,
//                     &constants_bytes,
//                 );

//                 vk.cmd_dispatch(cmd, groups_x, 1, 1);

//                 let vk = &app.vk.device;

//                 let cmds = [cmd];
//                 let submit = vk::SubmitInfo::default().command_buffers(&cmds);

//                 vk.end_command_buffer(cmd).unwrap();

//                 vk.queue_submit(app.vk.compute_queue, &[submit], vk::Fence::null())
//                     .unwrap();

//                 vk.queue_wait_idle(app.vk.compute_queue).unwrap()
//             };

//             push.sample_index += 1;
//             if push.sample_index >= push.max_samples {
//                 break;
//             }
//         }

//         println!("light probes baked");

//         unsafe {
//             app.vk.device.device_wait_idle().unwrap();
//         }

//         // println!("Probes:\n{:#?}", &app.probes);
//         app.vk
//             .download_buffer(app.probes_buffer.buffer, &mut app.probes);

//         let readback_data = LightprobesReadbackData {
//             probes: app.probes.as_ptr(),
//             pixels_count: app.probes.len() as u32,
//         };

//         (app.config.lightprobes_read_callback)(readback_data);
//     }

//     for group in &mut app.groups {
//         group.emission.destroy(&app.vk);
//     }

//     for tex in &mut previous_diffuses {
//         tex.destroy(&app.vk);
//     }
// }

fn octahedron_wrap(v: Vector2) -> Vector2 {
    let sign_x = if v.x >= 0.0 { 1.0 } else { -1.0 };
    let sign_y = if v.y >= 0.0 { 1.0 } else { -1.0 };

    Vector2 {
        x: (1.0 - v.y.abs()) * sign_x,
        y: (1.0 - v.x.abs()) * sign_y,
    }
}

// fn decode_normal_octahedron(e: Vector2) -> Vector3 {
//     let mut v = Vector3 {
//         x: e.x,
//         y: e.y,
//         z: 1.0 - e.x.abs() - e.y.abs(),
//     };

//     if v.z < 0.0 {
//         let xy = octahedron_wrap(Vector2 { x: v.x, y: v.y });
//         v.x = xy.x;
//         v.y = xy.y;
//     }

//     v.normalize()
// }

// fn unpack_normal_octahedron(packed: f32) -> Vector3 {
//     let bits = packed.to_bits();

//     let x = bits >> 16;
//     let y = bits & 0xFFFF;

//     let mut oct = Vector2 {
//         x: (x as f32) / 65535.0,
//         y: (y as f32) / 65535.0,
//     };

//     oct.x = oct.x * 2.0 - 1.0;
//     oct.y = oct.y * 2.0 - 1.0;

//     decode_normal_octahedron(oct)
// }

// fn encode_directional_lightmap_rgb(diffuse: &[f32], dir: &mut [f32]) {
//     for i in 0..(diffuse.len() / 4) {
//         let index = i * 4;
//         let diffuse_r = diffuse[index + 0];
//         let diffuse_g = diffuse[index + 1];
//         let diffuse_b = diffuse[index + 2];
//         let diffuse_a = diffuse[index + 3];

//         if diffuse_a == 0.0 {
//             continue;
//         }

//         let diffuse = Vector3::new(diffuse_r, diffuse_g, diffuse_b);
//         let luminance = diffuse.luminance();

//         let dir_x = dir[index + 0];
//         let dir_y = dir[index + 1];
//         let dir_z = dir[index + 2];

//         let v = Vector3::new(dir_x, dir_y, dir_z);

//         let normalized_dir = (v / luminance).normalize();

//         dir[index + 0] = normalized_dir.x * 0.5 + 0.5;
//         dir[index + 1] = normalized_dir.y * 0.5 + 0.5;
//         dir[index + 2] = normalized_dir.z * 0.5 + 0.5;
//     }
// }

// fn encode_directional_lightmap_alpha(diffuse: &[f32], dir: &mut [f32]) {
//     for i in 0..(diffuse.len() / 4) {
//         let index = i * 4;
//         let diffuse_a = diffuse[index + 3];

//         if diffuse_a == 0.0 {
//             continue;
//         }

//         let dir_w = dir[index + 3];

//         let normal = unpack_normal_octahedron(dir_w);

//         let normalized_dir = Vector3 {
//             x: (dir[index + 0] - 0.5) * 2.0,
//             y: (dir[index + 1] - 0.5) * 2.0,
//             z: (dir[index + 2] - 0.5) * 2.0,
//         };

//         dir[index + 3] = normal.dot(normalized_dir).clamp(0.0, 1.0) * 0.5 + 0.5;
//     }
// }

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
        } else {
            let cmd = app.vk.begin_single_use_cmd();
            clear_texture(
                &app.vk,
                &mut app.render_target.diffuse,
                cmd,
                vk::ClearColorValue {
                    float32: [0.0, 0.0, 0.0, 0.0],
                },
            );
            app.vk.end_single_use_cmd(cmd);
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
        };

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
            adjust_samples_shader: ComputeShader::null(),
            constants,
        }
    }
}
