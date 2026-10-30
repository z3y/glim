#![allow(unsafe_op_in_unsafe_fn)]

use ash::vk::{self, Handle};
use std::io::{self, Write};
use std::mem;
use std::path::PathBuf;
use std::{ptr, time::Duration};

use glfw_sys::{
    GLFW_KEY_ESCAPE, GLFW_PRESS, GLFWwindow, glfwCreateWindowSurface, glfwGetKey,
    glfwGetWindowSize, glfwPollEvents, glfwSetWindowShouldClose, glfwSetWindowTitle,
    glfwWindowShouldClose,
};

use crate::bindings::*;
use crate::buffer::Buffer;
use crate::camera::InitializePreviewPushConstants;
use crate::lights::LightType;
use crate::math::{Vector2, Vector3};
use crate::mesh::{GEOMETRIES_VARIANTS, Vertex};
use crate::raster::{
    build_compaction, decompact_combined_sh, decompact_diffuse, decompact_directional,
    dilate_texture, encode_directional, rasterize_texels, render_normals,
};
use crate::seams::{Seam, fix_seams};
use crate::sh::SHProbeL2;

use crate::shader::{
    PreviewPushConstants, ShaderBindings, ShaderName, SpecializationConstants, load_compute_shader,
    update_compute_shader,
};
use crate::skybox::Skybox;
use crate::texture_array::{TextureArray, TextureDescriptor};
use crate::{
    camera::Camera,
    compute_shader::ComputeShader,
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
mod cwbvh;
mod lights;
mod math;
mod mesh;
mod oidn;
mod pack;
mod raster;
mod seams;
mod sh;
mod shader;
mod skybox;
mod test;
mod texture2d;
mod texture_array;
mod vulkan_cmd;
mod vulkan_context;
mod vulkan_swapchain;
mod window;

#[unsafe(no_mangle)]
pub extern "C" fn default_logger(data: Log) {
    match data.ty {
        LogType::Info => println!("Message: {}", data.message.from()),
        LogType::Error => panic!("Error: {}", data.message.from()),
        LogType::Progress => {
            use std::io::{self, Write};
            print!("\rProgress: {:.1}%\x1B[K", data.progress * 100.0);
            let _ = io::stdout().flush();
        }
    }
}

pub static mut LOG_CALLBACK: LogCallback = default_logger;

pub struct Glim {
    pub config: GlimConfig,
    pub vk: VulkanContext,
    pub window: *mut GLFWwindow,

    pub meshes: [Mesh; GEOMETRIES_VARIANTS],

    pub cpu_lights: Vec<Light>,
    pub light_cookies: Vec<TexturePixelsData>,
    pub alphas: Vec<TexturePixelsData>,
    pub emissive_triangles: Vec<u32>,
    pub groups: Vec<LightmapGroup>,
    pub seams: Vec<Seam>,

    pub gpu_mesh: GpuMesh,
    pub gpu_lights: Buffer,
    pub emissive_triangles_buffer: Buffer,

    pub tlas: VulkanAs,
    pub bvh_nodes: Buffer,
    pub bvh_triangles: Buffer,

    pub camera: Camera,

    pub preview_shader: ComputeShader,
    pub init_from_camera_shader: ComputeShader,
    pub preview_initialized: bool,

    pub preview_push_constants: PreviewPushConstants,

    pub probes: Vec<SHProbeL2>,

    pub render_target: RenderTarget,

    pub constants: SpecializationConstants,

    pub output_dir: PathBuf,

    pub skybox: Skybox,

    pub albedo_array: TextureArray,
    pub emission_array: TextureArray,
    pub cookie_array: TextureArray,
    pub alphas_array: TextureArray,

    pub linear_sampler: vk::Sampler,
    pub window_title: String,
}

impl Drop for Glim {
    fn drop(&mut self) {
        self.albedo_array.destroy(&self.vk);
        self.emission_array.destroy(&self.vk);
        self.cookie_array.destroy(&self.vk);
        self.alphas_array.destroy(&self.vk);

        unsafe { self.vk.device.destroy_sampler(self.linear_sampler, None) };

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

        if !self.bvh_nodes.buffer.is_null() {
            self.bvh_nodes.destroy(&self.vk);
        }
        if !self.bvh_triangles.buffer.is_null() {
            self.bvh_triangles.destroy(&self.vk);
        }

        if !self.tlas.acceleration_structure.is_null() {
            self.tlas.destroy(&self.vk);
        }

        if !self.init_from_camera_shader.pipeline.is_null() {
            self.init_from_camera_shader.destroy(&self.vk);
        }

        if !self.gpu_lights.buffer.is_null() {
            self.gpu_lights.destroy(&self.vk);
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

pub struct TexturePixelsData {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub struct LightmapGroup {
    pub settings: LightmapSettings,
    pub id: u32,
    pub albedo_pixels: Vec<u8>,
    pub emission_pixels: Vec<u8>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct LightmapInfo {
    pub resolution: [u32; 2],
    pub compaction_offset: u32,
    pub table_base: u32,
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
    for mesh in &app.meshes {
        let message = format!(
            "Geometries Group Transparent: {}, Cast Shadows: {}, Vertices: {} Triangles: {}",
            mesh.transparent,
            !mesh.no_shadows,
            mesh.vertices.len(),
            mesh.indices.len() / 3,
        );
        Log::msg(&message);
    }

    extract_emissive_triangles(app);

    // clamp samples and bounces to supported limits
    app.config.probe_samples = clamp_samples(app.config.probe_samples);
    app.config.direct_emission_samples = clamp_samples(app.config.direct_emission_samples);
    app.config.direct_light_samples = clamp_samples(app.config.direct_light_samples);
    app.config.indirect_samples = clamp_samples(app.config.indirect_samples);
    app.config.bounce_count = clamp_bounces(app.config.bounce_count);

    // upload lights
    if app.cpu_lights.len() > 0 {
        // first other lights, then area, then mixed
        app.cpu_lights.sort_unstable_by_key(|x| {
            if x.ty == LightType::Area || x.ty == LightType::Disc {
                1u8
            } else if x.mixed {
                2u8
            } else {
                0u8
            }
        });

        app.gpu_lights = Buffer::new(
            &app.vk,
            String::from("Lights"),
            &app.cpu_lights,
            vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        );
    } else {
        app.gpu_lights = Buffer::empty(
            &app.vk,
            String::from("Lights"),
            std::mem::size_of::<Light>() as vk::DeviceSize,
            vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        );
    }

    let use_ray_query = app.config.hardware_rt && app.vk.as_device.is_some();

    app.gpu_mesh = GpuMesh::new(
        &app.vk,
        &app.meshes,
        &app.groups,
        use_ray_query,
        &mut app.seams,
        &app.config,
    );

    match &app.gpu_mesh.acceleration_structure {
        mesh::AccelerationStructureType::RayQuery(vulkan_as) => {
            app.tlas = create_tlas(&app.vk, &vulkan_as)
        }
        mesh::AccelerationStructureType::BVH { cwbvh } => {
            app.bvh_nodes = Buffer::new(
                &app.vk,
                String::from("BVH Nodes"),
                &cwbvh.nodes,
                vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
            );
            app.bvh_triangles = Buffer::new(
                &app.vk,
                String::from("BVH Triangles"),
                &cwbvh.triangles,
                vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
            );
        }
    }

    if app.skybox.view().is_null() {
        app.skybox = Skybox::solid(&app.vk, 4, 4, Vector3::new(0.0, 0.0, 0.0));
    }

    // create meta textures

    let mut albedos = Vec::with_capacity(app.groups.len());
    let mut emissions = Vec::with_capacity(app.groups.len());

    for index in 0..app.groups.len() {
        let group = &app.groups[index];

        let desc = TextureDescriptor {
            width: group.settings.width,
            height: group.settings.height,
            format: vk::Format::R8G8B8A8_UNORM,
            usage: vk::ImageUsageFlags::SAMPLED
                | vk::ImageUsageFlags::TRANSFER_SRC
                | vk::ImageUsageFlags::TRANSFER_DST,
            name: format!("Albedo {}", index),
        };
        albedos.push(desc);

        let desc = TextureDescriptor {
            width: group.settings.width,
            height: group.settings.height,
            format: vk::Format::B10G11R11_UFLOAT_PACK32,
            usage: vk::ImageUsageFlags::SAMPLED
                | vk::ImageUsageFlags::TRANSFER_SRC
                | vk::ImageUsageFlags::TRANSFER_DST,
            name: format!("Emission {}", index),
        };
        emissions.push(desc);
    }

    let mut albedo_array = TextureArray::new(&app.vk, albedos);
    let mut emission_array = TextureArray::new(&app.vk, emissions);

    const STAGING_RESOLUTION: u64 = 1024;
    let mut staging_buffer = Buffer::empty(
        &app.vk,
        String::from("Staging Buffer"),
        STAGING_RESOLUTION
            * STAGING_RESOLUTION
            * 4
            * std::mem::size_of::<f32>() as u64 as vk::DeviceSize, // 16 MB
        vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::TRANSFER_SRC,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    );

    for index in 0..app.groups.len() {
        let group = &app.groups[index];

        albedo_array.textures[index].set_pixels(&app.vk, &group.albedo_pixels, &staging_buffer);
        emission_array.textures[index].set_pixels(&app.vk, &group.emission_pixels, &staging_buffer);
    }

    // free
    for index in 0..app.groups.len() {
        let group = &mut app.groups[index];
        group.albedo_pixels = Vec::new();
        group.emission_pixels = Vec::new();
    }

    app.albedo_array = albedo_array;
    app.emission_array = emission_array;

    if app.light_cookies.len() == 0 {
        let dummy_cookie = TexturePixelsData {
            pixels: vec![0u8; 16],
            width: 2,
            height: 2,
        };
        app.light_cookies.push(dummy_cookie);
    }
    let mut cookies = Vec::with_capacity(app.light_cookies.len());
    for (index, cookie) in app.light_cookies.iter().enumerate() {
        let desc = TextureDescriptor {
            width: cookie.width,
            height: cookie.height,
            format: vk::Format::R8G8B8A8_UNORM,
            usage: vk::ImageUsageFlags::SAMPLED
                | vk::ImageUsageFlags::TRANSFER_SRC
                | vk::ImageUsageFlags::TRANSFER_DST,
            name: format!("Cookie {}", index),
        };
        cookies.push(desc);
    }
    let mut cookie_array = TextureArray::new(&app.vk, cookies);
    for (index, cookie) in app.light_cookies.iter().enumerate() {
        cookie_array.textures[index].set_pixels(&app.vk, &cookie.pixels, &staging_buffer);
    }
    app.cookie_array = cookie_array;
    // free
    for cookie in &mut app.light_cookies {
        cookie.pixels = Vec::new();
    }

    if app.alphas.len() == 0 {
        let dummy_alpha = TexturePixelsData {
            pixels: vec![0u8; 4],
            width: 2,
            height: 2,
        };
        app.alphas.push(dummy_alpha);
    }
    let mut alphas = Vec::with_capacity(app.alphas.len());
    for (index, alpha) in app.alphas.iter().enumerate() {
        let desc = TextureDescriptor {
            width: alpha.width,
            height: alpha.height,
            format: vk::Format::R8_UNORM,
            usage: vk::ImageUsageFlags::SAMPLED
                | vk::ImageUsageFlags::TRANSFER_SRC
                | vk::ImageUsageFlags::TRANSFER_DST,
            name: format!("Alpha {}", index),
        };
        alphas.push(desc);
    }
    let mut alphas_array = TextureArray::new(&app.vk, alphas);
    for (index, alpha) in app.alphas.iter().enumerate() {
        alphas_array.textures[index].set_pixels(&app.vk, &alpha.pixels, &staging_buffer);
    }
    app.alphas_array = alphas_array;
    // free
    for a in &mut app.alphas {
        a.pixels = Vec::new();
    }

    staging_buffer.destroy(&app.vk);
    drop(staging_buffer);

    let config = &app.config;

    let mut geometries_offsets = [0; GEOMETRIES_VARIANTS];
    let mut offset = 0;
    for i in 0..GEOMETRIES_VARIANTS {
        geometries_offsets[i] = offset as u32;
        let tri_count = app.meshes[i].indices.len() / 3;
        offset += tri_count;
    }

    app.constants = SpecializationConstants {
        hardware_rt: use_ray_query as u32,
        light_falloff_type: config.light_falloff as u32,
        emissive_triangles_count: app.emissive_triangles.len() as u32,
        multiple_importance_sampling: config.mis as u32,
        lightmap_group_count: app.groups.len() as u32,
        lightmap_mode: config.lightmap_mode as u32,
        coordinate_system: app.config.coordinate_system as u32,
        skybox_intensity: config.skybox_intensity,
        indirect_intensity: config.indirect_intensity,
        lightprobe_deringing: config.lightprobe_deringing,
        vertex_address: app.gpu_mesh.vertex_buffer.gpu_address,
        indices_address: app.gpu_mesh.index_buffer.gpu_address,
        emissive_triangles_address: app.emissive_triangles_buffer.gpu_address,
        compacted_lightmap_address: 0,
        lightmaps_info_address: 0,
        compacted_visibility_address: 0,
        lights_address: app.gpu_lights.gpu_address,
        compaction_buffer_address: 0,
        bvh_nodes_address: app.bvh_nodes.gpu_address,
        bvh_triangles_address: app.bvh_triangles.gpu_address,
        emissive_multiplier: app.config.emissive_multiplier,
        ao_intensity: config.ao_intensity,
        ao_range: config.ao_range,
        geometries_0_offset: geometries_offsets[0],
        geometries_1_offset: geometries_offsets[1],
        geometries_2_offset: geometries_offsets[2],
        geometries_3_offset: geometries_offsets[3],
        main_uv_address: app.gpu_mesh.main_uv_buffer.gpu_address,
        jitter_barycentrics: app.config.antialiasing as u32,
    };

    if app.config.is_preview {
        render_preview(app);
    } else {
        render_lightmaps(app)
    }
    unsafe {
        app.vk.device.device_wait_idle().unwrap();
    }
}

fn format_window_title(
    title: &mut String,
    sample_index: u32,
    max_samples: u32,
    light_count: usize,
    hardware_rt: bool,
    w: i32,
    h: i32,
) {
    use std::fmt::Write;
    title.clear();

    let _ = write!(
        title,
        "Preview | Hardware RT: {} | {}x{} | Lights {} | Sample {}/{}\0",
        hardware_rt, w, h, light_count, sample_index, max_samples
    );
}

fn render_preview(app: &mut Glim) {
    let window = app.window;

    let preview_settings = app.config.preview_settings.clone();

    update_render_target(app, &preview_settings);

    let visibility = &mut app.render_target.visibility;
    let diffuse = &mut app.render_target.diffuse;

    let albedos = app.albedo_array.views();
    let emissions = app.emission_array.views();

    let shader_bindings = ShaderBindings {
        tlas: app.tlas.acceleration_structure,
        albedos: &albedos,
        emissions: &emissions,
        skybox: app.skybox.view(),
        skybox_sampler: app.skybox.sampler(),
        visibility: visibility.view,
        preview_diffuse: diffuse.view(),
        cookies: &app.cookie_array.views(),
        alphas: &app.alphas_array.views(),
        linear_sampler: app.linear_sampler,
    };

    app.preview_shader = load_compute_shader(
        &app.vk,
        ShaderName::Preview,
        &app.constants,
        &shader_bindings,
        size_of::<PreviewPushConstants>(),
    );
    update_compute_shader(&app.vk, &app.preview_shader, &shader_bindings);

    let mut previous_time = std::time::Instant::now();

    let mut bake_start_time = std::time::Instant::now();
    let mut bake_complete_printed = false;

    unsafe {
        while glfwWindowShouldClose(window) == 0 {
            glfwPollEvents();

            Log::progress(
                app.preview_push_constants.sample_index as f32
                    / app.config.direct_emission_samples as f32,
            );

            {
                use std::ffi::c_char;

                let hardwar_rt = app.config.hardware_rt && app.vk.as_device.is_some();

                let (mut w, mut h) = (0, 0);
                glfwGetWindowSize(window, &mut w, &mut h);

                format_window_title(
                    &mut app.window_title,
                    app.preview_push_constants.sample_index,
                    app.config.direct_emission_samples,
                    app.cpu_lights.len(),
                    hardwar_rt,
                    w,
                    h,
                );

                glfwSetWindowTitle(window, app.window_title.as_ptr().cast::<c_char>());
            }

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

                let shader_bindings = ShaderBindings {
                    tlas: app.tlas.acceleration_structure,
                    albedos: &app.albedo_array.views(),
                    emissions: &app.emission_array.views(),
                    skybox: app.skybox.view(),
                    skybox_sampler: app.skybox.sampler(),
                    visibility: visibility.view,
                    preview_diffuse: diffuse.view(),
                    alphas: &app.alphas_array.views(),
                    cookies: &app.cookie_array.views(),
                    linear_sampler: app.linear_sampler,
                };
                update_compute_shader(&app.vk, &app.preview_shader, &shader_bindings);

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

    unsafe {
        glfwPollEvents();
        glfw_sys::glfwDestroyWindow(window);
        // glfw_sys::glfwTerminate();
    };
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

        let shader_bindings = ShaderBindings {
            tlas: app.tlas.acceleration_structure,
            albedos: &app.albedo_array.views(),
            emissions: &app.emission_array.views(),
            skybox: app.skybox.view(),
            skybox_sampler: app.skybox.sampler(),
            visibility: visibility.view,
            preview_diffuse: diffuse.view(),
            alphas: &app.alphas_array.views(),
            cookies: &app.cookie_array.views(),
            linear_sampler: app.linear_sampler,
        };

        if app.init_from_camera_shader.pipeline.is_null() {
            app.init_from_camera_shader = load_compute_shader(
                &app.vk,
                ShaderName::InitializePreview,
                &app.constants,
                &shader_bindings,
                size_of::<InitializePreviewPushConstants>(),
            );
        }

        update_compute_shader(&app.vk, &app.init_from_camera_shader, &shader_bindings);

        visibility
    };

    app.render_target.diffuse = diffuse;
    app.render_target.visibility = visibility;

    {
        let max_samples = app.config.direct_emission_samples;
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
    let mut emissive_triangles: Vec<u32> = Vec::new();

    fn sample_emissive(uv: Vector2, w: u32, h: u32, pixels: &[u8]) -> bool {
        let x = (uv.x * w as f32).clamp(0.0, (w - 1) as f32) as usize;
        let y = (uv.y * h as f32).clamp(0.0, (h - 1) as f32) as usize;
        let index = (y * w as usize + x) * 4;

        let packed = u32::from_le_bytes([
            pixels[index],
            pixels[index + 1],
            pixels[index + 2],
            pixels[index + 3],
        ]);

        let r = packed & 0x7ff;
        let g = (packed >> 11) & 0x7ff;
        let b = (packed >> 22) & 0x3ff;

        r != 0 || g != 0 || b != 0
    }

    if app.config.mis {
        let mut extract = |vertices: &[Vertex], indices: &[u32]| {
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
        };

        for mesh in &app.meshes {
            extract(&mesh.vertices, &mesh.indices);
        }
    }

    if app.config.mis {
        let message = format!(
            "Found {} emissive triangles for MIS",
            emissive_triangles.len()
        );
        Log::msg(&message);
    }

    if emissive_triangles.len() > 0 {
        app.emissive_triangles_buffer = Buffer::new(
            &app.vk,
            String::from("Emissive Triangles"),
            &emissive_triangles,
            vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        );
    } else {
        app.emissive_triangles_buffer = Buffer::empty(
            &app.vk,
            String::from("Emissive Triangles"),
            32,
            vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        );
    }

    app.emissive_triangles = emissive_triangles;
}

impl LightmapGroup {
    fn new(
        settings: LightmapSettings,
        albedo_pixels: &[u8],   // RGB32
        emission_pixels: &[u8], // RGB111110Float
    ) -> LightmapGroup {
        LightmapGroup {
            settings,
            albedo_pixels: albedo_pixels.to_vec(),
            emission_pixels: emission_pixels.to_vec(),
            id: 0,
        }
    }
}

impl Glim {
    pub fn new(config: GlimConfig, output_dir: PathBuf) -> Glim {
        unsafe { LOG_CALLBACK = config.log_callback };

        let mut vulkan_config = VulkanConfig {
            enable_validation_layers: config.vulkan_validation_layers,
            enable_window: config.is_preview,
            window_extensions: Vec::new(),
        };

        let window_title = String::from("Preview\0");
        let window = initialize_window(&config, &mut vulkan_config, &window_title);

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

        let render_target = RenderTarget {
            visibility: Texture2D::null(),
            diffuse: Texture2D::null(),
        };

        let constants = SpecializationConstants::default();

        let sampler_info = vk::SamplerCreateInfo::default()
            .mag_filter(vk::Filter::LINEAR)
            .min_filter(vk::Filter::LINEAR)
            .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .mip_lod_bias(0.0)
            .anisotropy_enable(false)
            .max_anisotropy(1.0)
            .compare_enable(false)
            .compare_op(vk::CompareOp::ALWAYS)
            .min_lod(0.0)
            .max_lod(0.0)
            .unnormalized_coordinates(false);

        let linear_sampler = unsafe { vk.device.create_sampler(&sampler_info, None).unwrap() };

        let meshes = [
            Mesh {
                vertices: Vec::new(),
                indices: Vec::new(),
                main_uvs: Vec::new(),
                transparent: false,
                no_shadows: false,
            },
            Mesh {
                vertices: Vec::new(),
                indices: Vec::new(),
                main_uvs: Vec::new(),
                transparent: true,
                no_shadows: false,
            },
            Mesh {
                vertices: Vec::new(),
                indices: Vec::new(),
                main_uvs: Vec::new(),
                transparent: false,
                no_shadows: true,
            },
            Mesh {
                vertices: Vec::new(),
                indices: Vec::new(),
                main_uvs: Vec::new(),
                transparent: true,
                no_shadows: true,
            },
        ];

        Self {
            vk,
            meshes,
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
            seams: Vec::new(),
            emissive_triangles: Vec::new(),
            emissive_triangles_buffer: Buffer::null(),
            constants,
            output_dir,
            skybox: Skybox::null(),
            albedo_array: TextureArray::null(),
            emission_array: TextureArray::null(),
            bvh_nodes: Buffer::null(),
            bvh_triangles: Buffer::null(),
            light_cookies: Vec::new(),
            cookie_array: TextureArray::null(),
            linear_sampler,
            window_title,
            alphas: Vec::new(),
            alphas_array: TextureArray::null(),
        }
    }

    pub fn add_light_cookie(&mut self, pixels: &[u8], width: u32, height: u32) {
        let cookie = TexturePixelsData {
            pixels: pixels.to_owned(),
            width,
            height,
        };

        self.light_cookies.push(cookie);
    }

    pub fn add_alpha_texture(&mut self, pixels: &[u8], width: u32, height: u32) {
        let alpha = TexturePixelsData {
            pixels: pixels.to_owned(),
            width,
            height,
        };

        self.alphas.push(alpha);
    }
}

fn save_tga(path: PathBuf, w: usize, h: usize, pixels: &[f32], alpha: bool) -> io::Result<()> {
    use std::fs::File;
    use std::io::BufWriter;

    let mut file = BufWriter::new(File::create(path)?);

    let bits = match alpha {
        true => 32,
        false => 24,
    };

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
        ((h >> 8) & 0xFF) as u8,         // Height
        bits,                            // Bits per pixel (32 = RGBA)
        if alpha { 0x28 } else { 0x20 }, // top-left origin
    ];
    file.write_all(&header)?;

    if alpha {
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
    } else {
        let mut row = vec![0u8; w * 3];
        for y in 0..h {
            for x in 0..w {
                let flipped_y = h - 1 - y;
                let index = (x + flipped_y * w) * 4;
                let r = (pixels[index + 0].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                let g = (pixels[index + 1].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                let b = (pixels[index + 2].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;

                let out = x * 3;
                row[out + 0] = b;
                row[out + 1] = g;
                row[out + 2] = r;
            }
            file.write_all(&row)?;
        }
    }

    Ok(())
}

fn bake_shadowmask(app: &mut Glim, pixel_buffer: &Buffer, lightmap_index: usize) {
    let mut requires_shadowmask = false;
    for l in &app.cpu_lights {
        if l.occlusion_channel != u8::MAX {
            requires_shadowmask = true;
            break;
        }
    }

    if !requires_shadowmask {
        return;
    }

    let group = &app.groups[lightmap_index].settings;
    if group.discard {
        return;
    }

    let resolution = group.width;

    Log::msg(&format!(
        "Baking Shadowmask ({}/{})",
        lightmap_index + 1,
        app.groups.len()
    ));

    #[repr(C)]
    pub struct PushConstants {
        pub lights_start: u32,
        pub lights_count: u32,
        pub resolution: u32,
        pub lightmap_group: u32,

        pub pixels_address: u64,
    }

    let albedos = app.albedo_array.views();
    let emissions = app.emission_array.views();
    let cookies = app.cookie_array.views();

    let shader_bindings = ShaderBindings {
        tlas: app.tlas.acceleration_structure,
        albedos: &albedos,
        emissions: &emissions,
        skybox: app.skybox.view(),
        skybox_sampler: app.skybox.sampler(),
        visibility: vk::ImageView::null(),
        preview_diffuse: vk::ImageView::null(),
        cookies: &cookies,
        alphas: &app.alphas_array.views(),
        linear_sampler: app.linear_sampler,
    };

    let mut shader = load_compute_shader(
        &app.vk,
        ShaderName::ShadowMask,
        &app.constants,
        &shader_bindings,
        size_of::<PushConstants>(),
    );
    update_compute_shader(&app.vk, &shader, &shader_bindings);

    let push = PushConstants {
        lights_start: 0,
        lights_count: app.cpu_lights.len() as u32,
        resolution: resolution,
        lightmap_group: lightmap_index as u32,
        pixels_address: pixel_buffer.gpu_address,
    };
    let push_bytes = as_bytes(&push);

    let cmd = app.vk.begin_single_use_cmd();
    unsafe {
        let vk = &app.vk.device;
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
        vk.cmd_dispatch(cmd, resolution / 8, resolution / 8, 1);
    }
    app.vk.end_single_use_cmd(cmd);

    shader.destroy(&app.vk);

    let pixels: &[f32] = unsafe {
        std::slice::from_raw_parts(
            pixel_buffer.ptr as *const f32,
            (resolution * resolution * 4) as usize,
        )
    };

    let path = app
        .output_dir
        .join(format!("Lightmap-{}_ShadowMask.tga", lightmap_index));

    if let Err(err) = save_tga(path, resolution as usize, resolution as usize, pixels, true) {
        Log::msg(&format!("Failed to save shadowmask: {}", err));
    }
}

fn zero_buffer(vk: &VulkanContext, buffer: &Buffer) {
    let cmd = vk.begin_single_use_cmd();

    unsafe {
        vk.device
            .cmd_fill_buffer(cmd, buffer.buffer, 0, buffer.bytes, 0);
    }

    vk.end_single_use_cmd(cmd);
}

fn bake_direct_emission(app: &Glim, visibility_texels_count: usize) {
    let message = format!("Baking Direct Emission");
    Log::msg(&message);

    #[repr(C)]
    pub struct PushConstants {
        pub compacted_count: u32,
        pub sample_index: u32,
        pub max_samples: u32,
        pub pad0: u32,
        // pub texel_validity_address: u64,
        // pub pad1: u32,
        // pub pad2: u32,
    }

    let shader_bindings = ShaderBindings {
        tlas: app.tlas.acceleration_structure,
        albedos: &app.albedo_array.views(),
        emissions: &&app.emission_array.views(),
        skybox: app.skybox.view(),
        skybox_sampler: app.skybox.sampler(),
        visibility: vk::ImageView::null(),
        preview_diffuse: vk::ImageView::null(),
        alphas: &app.alphas_array.views(),
        cookies: &app.cookie_array.views(),
        linear_sampler: app.linear_sampler,
    };

    let mut bake_direct_emission_shader = load_compute_shader(
        &app.vk,
        ShaderName::BakeDirectEmission,
        &app.constants,
        &shader_bindings,
        size_of::<PushConstants>(),
    );
    update_compute_shader(&app.vk, &bake_direct_emission_shader, &shader_bindings);

    let mut push = PushConstants {
        compacted_count: visibility_texels_count as u32,
        sample_index: 0,
        max_samples: app.config.direct_emission_samples,
        pad0: 0,
        // texel_validity_address: texel_validity.gpu_address,
        // pad1: 0,
        // pad2: 0,
    };

    let groups_x = (visibility_texels_count as u32 + 63) / 64;
    let shader = &bake_direct_emission_shader;

    for sample_index in 0..push.max_samples {
        if is_cancelled() {
            break;
        }
        push.sample_index = sample_index;

        Log::progress(push.sample_index as f32 / push.max_samples as f32);

        let vk = &app.vk.device;
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

            vk.cmd_dispatch(cmd, groups_x, 1, 1);
        };
        app.vk.end_single_use_cmd(cmd);
    }
    bake_direct_emission_shader.destroy(&app.vk);
    drop(bake_direct_emission_shader);
}

fn bake_direct_lights(
    app: &Glim,
    visibility_texels_count: usize,
    lights_start: usize,
    lights_count: usize,
    samples: u32,
    mixed: bool,
) {
    if lights_count == 0 {
        return;
    }

    #[repr(C)]
    pub struct PushConstants {
        pub texel_count: u32,
        pub sample_index: u32,
        pub max_samples: u32,
        pub mixed: u32,

        pub lights_start: u32,
        pub lights_count: u32,
        pub pad1: u32,
        pub pad2: u32,
    }

    let shader_bindings = ShaderBindings {
        tlas: app.tlas.acceleration_structure,
        albedos: &app.albedo_array.views(),
        emissions: &[],
        skybox: app.skybox.view(),
        skybox_sampler: app.skybox.sampler(),
        visibility: vk::ImageView::null(),
        preview_diffuse: vk::ImageView::null(),
        alphas: &app.alphas_array.views(),
        cookies: &app.cookie_array.views(),
        linear_sampler: app.linear_sampler,
    };

    // todo cache
    let mut bake_direct_light_shader = load_compute_shader(
        &app.vk,
        ShaderName::BakeDirectLight,
        &app.constants,
        &shader_bindings,
        size_of::<PushConstants>(),
    );
    update_compute_shader(&app.vk, &bake_direct_light_shader, &shader_bindings);

    let mut push = PushConstants {
        texel_count: visibility_texels_count as u32,
        sample_index: 0,
        max_samples: samples,
        mixed: mixed as u32,
        lights_start: lights_start as u32,
        lights_count: lights_count as u32,
        pad1: 0,
        pad2: 0,
    };

    let groups_x = (visibility_texels_count as u32 + 63) / 64;
    let shader = &bake_direct_light_shader;

    for sample_index in 0..samples {
        if is_cancelled() {
            break;
        }
        push.sample_index = sample_index;
        let push_bytes = as_bytes(&push);

        let vk = &app.vk.device;

        Log::progress(push.sample_index as f32 / push.max_samples as f32);

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
            vk.cmd_dispatch(cmd, groups_x, 1, 1);
        };
        app.vk.end_single_use_cmd(cmd);
    }

    bake_direct_light_shader.destroy(&app.vk);
    drop(bake_direct_light_shader);
}

fn bake_indirect(app: &Glim, direct_buffer: &Buffer, visibility_texels_count: usize) {
    if app.config.bounce_count == 0 {
        return;
    }

    // 2 buffers to keep previous and next bounce diffuse
    let indirect_texels_count = visibility_texels_count * 2;

    let mut indirect_buffer = Buffer::empty(
        &app.vk,
        "Indirect Buffer".to_owned(),
        (indirect_texels_count * size_of::<f32>() * 3) as u64,
        vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS | vk::BufferUsageFlags::TRANSFER_DST,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    );
    zero_buffer(&app.vk, &indirect_buffer);

    let cmd = app.vk.begin_single_use_cmd();
    let region = vk::BufferCopy {
        src_offset: 0,
        dst_offset: (size_of::<f32>() * 3 * visibility_texels_count) as u64,
        size: (size_of::<f32>() * 3 * visibility_texels_count) as u64,
    };
    unsafe {
        app.vk
            .device
            .cmd_copy_buffer(cmd, direct_buffer.buffer, indirect_buffer.buffer, &[region])
    };
    app.vk.end_single_use_cmd(cmd);

    // todo
    // if mixed_count > 0 {
    //     render_direct_light(
    //         lights_count as u32 - mixed_count,
    //         lights_count as u32,
    //         app.config.direct_light_samples,
    //         true,
    //         &mut progress,
    //     );
    // }

    #[repr(C)]
    pub struct PushConstants {
        pub compacted_count: u32,
        pub sample_index: u32,
        pub max_samples: u32,
        pub bounce_index: u32,

        pub indirect_address: u64,
        pub pad0: u32,
        pub pad1: u32,
    }

    let shader_bindings = ShaderBindings {
        tlas: app.tlas.acceleration_structure,
        albedos: &app.albedo_array.views(),
        emissions: &app.emission_array.views(),
        skybox: app.skybox.view(),
        skybox_sampler: app.skybox.sampler(),
        visibility: vk::ImageView::null(),
        preview_diffuse: vk::ImageView::null(),
        cookies: &app.cookie_array.views(),
        alphas: &app.alphas_array.views(),
        linear_sampler: app.linear_sampler,
    };

    let mut indirect_shader = load_compute_shader(
        &app.vk,
        ShaderName::BakeIndirect,
        &app.constants,
        &shader_bindings,
        size_of::<PushConstants>(),
    );
    update_compute_shader(&app.vk, &indirect_shader, &shader_bindings);

    let groups_x = (visibility_texels_count as u32 + 63) / 64;

    let mut push = PushConstants {
        compacted_count: visibility_texels_count as u32,
        sample_index: 0,
        max_samples: app.config.indirect_samples,
        bounce_index: 0,
        indirect_address: indirect_buffer.gpu_address,
        pad0: 0,
        pad1: 0,
    };

    let total_steps = (app.config.bounce_count * app.config.indirect_samples) as f32;

    'bounces: for bounce_index in 0..app.config.bounce_count {
        push.bounce_index = bounce_index;

        let message = format!("Baking Bounce {}", bounce_index + 1);
        Log::msg(&message);

        // let last_sample = app.config.indirect_samples - 1;
        for sample_index in 0..app.config.indirect_samples {
            if is_cancelled() {
                break 'bounces;
            }
            push.sample_index = sample_index;
            let push_bytes = as_bytes(&push);

            Log::progress(
                (bounce_index * app.config.indirect_samples + sample_index) as f32 / total_steps,
            );

            let vk = &app.vk.device;
            let shader = &indirect_shader;

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
                vk.cmd_dispatch(cmd, groups_x, 1, 1);
            };
            app.vk.end_single_use_cmd(cmd);
        }
    }

    indirect_shader.destroy(&app.vk);
    indirect_buffer.destroy(&app.vk);
}

fn bake_light_probes(app: &mut Glim) {
    if app.probes.len() == 0 || is_cancelled() {
        return;
    }

    Log::msg(&format!("Baking Light Probes"));

    let mut probes_buffer = Buffer::new(
        &app.vk,
        String::from("Light Probes"),
        &app.probes,
        vk::BufferUsageFlags::TRANSFER_DST
            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
            | vk::BufferUsageFlags::TRANSFER_SRC,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    );

    #[repr(C)]
    pub struct PushConstants {
        pub lights_count: u32,
        pub max_samples: u32,
        pub sample_index: u32,
        pub probes_count: u32,

        pub sh_probes_address: u64,
        pub pad0: u64,
    }

    let shader_bindings = ShaderBindings {
        tlas: app.tlas.acceleration_structure,
        albedos: &app.albedo_array.views(),
        emissions: &app.emission_array.views(),
        skybox: app.skybox.view(),
        skybox_sampler: app.skybox.sampler(),
        visibility: vk::ImageView::null(),
        preview_diffuse: vk::ImageView::null(),
        cookies: &app.cookie_array.views(),
        alphas: &app.alphas_array.views(),
        linear_sampler: app.linear_sampler,
    };

    let mut shader = load_compute_shader(
        &app.vk,
        ShaderName::BakeLightProbes,
        &app.constants,
        &shader_bindings,
        size_of::<PushConstants>(),
    );
    update_compute_shader(&app.vk, &shader, &shader_bindings);

    let mut push = PushConstants {
        lights_count: app.cpu_lights.len() as u32,
        max_samples: app.config.probe_samples,
        sample_index: 0,
        probes_count: app.probes.len() as u32,
        sh_probes_address: probes_buffer.gpu_address,
        pad0: 0,
    };

    let probes_count = app.probes.len() as u32;
    let groups_x = (probes_count + 63) / 64;
    let vk = &app.vk.device;

    for sample_index in 0..app.config.probe_samples {
        if is_cancelled() {
            break;
        }
        push.sample_index = sample_index;
        let constants_bytes = as_bytes(&push);

        Log::progress(push.sample_index as f32 / push.max_samples as f32);

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
        }
        app.vk.end_single_use_cmd(cmd);
    }

    let mut staging_buffer_light_probes = Buffer::empty(
        &app.vk,
        "Staging Buffer Light Probes".to_owned(),
        probes_buffer.bytes,
        vk::BufferUsageFlags::TRANSFER_DST,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    );

    let cmd = app.vk.begin_single_use_cmd();
    let region = vk::BufferCopy {
        src_offset: 0,
        dst_offset: 0,
        size: probes_buffer.bytes,
    };
    unsafe {
        app.vk.device.cmd_copy_buffer(
            cmd,
            probes_buffer.buffer,
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
    probes_buffer.destroy(&app.vk);
}

fn render_lightmaps(app: &mut Glim) {
    let mut max_resolution = 1;
    for g in &app.groups {
        max_resolution = max_resolution.max(g.settings.width as usize);
    }

    let mut mesh = app.meshes[0].clone();
    for i in 1..app.meshes.len() {
        mesh.merge_mesh(&app.meshes[i]);
    }

    let texels = rasterize_texels(
        &mesh,
        &app.groups,
        app.config.coverage_min_threshold.clamp(0.0, 1.0),
        app.config.coverage_max_samples.clamp(1, 32),
    );
    let texels_count = texels.visibility.len();

    let (comaction_buffer_data, offsets, table_base) =
        build_compaction(&texels, &mesh, &app.groups);

    let mut lightmaps_info = {
        let mut infos = Vec::with_capacity(app.groups.len());
        for group_index in 0..app.groups.len() {
            let group = &app.groups[group_index].settings;
            infos.push(LightmapInfo {
                resolution: [group.width, group.height],
                compaction_offset: offsets[group_index],
                table_base: table_base,
            });
        }

        let lightmaps_info = Buffer::empty(
            &app.vk,
            "Lightmap Info".into(),
            (infos.len() * std::mem::size_of::<LightmapInfo>()) as u64,
            vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT, // todo device local
        );
        unsafe {
            std::ptr::copy_nonoverlapping(
                infos.as_ptr() as *const u8,
                lightmaps_info.ptr as *mut u8,
                infos.len() * std::mem::size_of::<LightmapInfo>(),
            );
        }
        lightmaps_info
    };
    app.constants.lightmaps_info_address = lightmaps_info.gpu_address;

    let mut compaction_buffer = Buffer::new(
        &app.vk,
        "Compaction Buffer".to_owned(),
        &comaction_buffer_data,
        vk::BufferUsageFlags::TRANSFER_DST
            | vk::BufferUsageFlags::TRANSFER_SRC
            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    );
    app.constants.compaction_buffer_address = compaction_buffer.gpu_address;

    Log::msg(&format!("Created {} texels for baking.", texels_count));

    let mut visibility_buffer = Buffer::new(
        &app.vk,
        String::from("Visibility Buffer"),
        &texels.visibility,
        vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    );
    app.constants.compacted_visibility_address = visibility_buffer.gpu_address;

    let lightmap_texels_channels = match app.config.lightmap_mode {
        LightmapMode::NonDirectional => 3,
        LightmapMode::DominantDirection => 6,
        LightmapMode::CombinedSH => 9,
    };

    let mut lightmap_texels = Buffer::empty(
        &app.vk,
        "Lightmap Texels".to_owned(),
        (texels_count * mem::size_of::<f32>() * lightmap_texels_channels) as u64,
        vk::BufferUsageFlags::TRANSFER_DST
            | vk::BufferUsageFlags::TRANSFER_SRC
            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    );
    app.constants.compacted_lightmap_address = lightmap_texels.gpu_address;
    zero_buffer(&app.vk, &lightmap_texels);

    bake_direct_emission(app, texels_count);

    // todo area lights with more samples
    let mut mixed_start = 0;
    let mut area_start = 0;
    let mut area_count = 0;
    let mut mixed_count = 0;

    for i in 0..app.cpu_lights.len() {
        let light = &app.cpu_lights[i];

        if light.ty == LightType::Area || light.ty == LightType::Disc {
            if area_count == 0 {
                area_start = i;
            }
            area_count += 1;
        }

        if light.mixed {
            if mixed_count == 0 {
                mixed_start = i;
            }
            mixed_count += 1;
        }
    }

    let count = app.cpu_lights.len() - area_count - mixed_count;
    Log::msg(&format!("Baking {} Direct Lights", count));
    bake_direct_lights(
        app,
        texels_count,
        0,
        count,
        app.config.direct_light_samples,
        false,
    );

    Log::msg(&format!("Baking {} Area Lights", area_count));
    bake_direct_lights(
        app,
        texels_count,
        area_start,
        area_count,
        app.config.direct_emission_samples,
        false,
    );

    Log::msg(&format!("Baking {} Mixed Lights", mixed_count));
    // todo mixed
    // bake_direct_lights(
    //     app,
    //     texels_count,
    //     mixed_start,
    //     mixed_count,
    //     app.config.direct_light_samples,
    //     true,
    // );

    bake_indirect(app, &lightmap_texels, texels_count);

    let mut readback_buffer = Buffer::empty(
        &app.vk,
        "Readback Buffer".to_owned(),
        lightmap_texels.bytes,
        vk::BufferUsageFlags::TRANSFER_SRC
            | vk::BufferUsageFlags::TRANSFER_DST
            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    );

    unsafe {
        let cmd = app.vk.begin_single_use_cmd();

        let region = vk::BufferCopy {
            src_offset: 0,
            dst_offset: 0,
            size: lightmap_texels.bytes,
        };

        app.vk.device.cmd_copy_buffer(
            cmd,
            lightmap_texels.buffer,
            readback_buffer.buffer,
            &[region],
        );

        app.vk.end_single_use_cmd(cmd);
    }

    let readback_buffer_slice: &mut [f32] = unsafe {
        std::slice::from_raw_parts_mut(
            readback_buffer.ptr as *mut f32,
            readback_buffer.bytes as usize,
        )
    };

    let oidn = Oidn::load();
    let mut oidn = if oidn.is_err() {
        let err = oidn.err();
        match err {
            Some(err) => {
                let message = "Failed to load Open Image Denoise";
                Log::msg(&message);
                Log::msg(&err.to_string());
            }
            None => {}
        };
        None
    } else {
        Some(oidn.unwrap())
    };

    if let Some(oidn) = oidn.as_mut() {
        Log::msg(&format!(
            "Open Image Denoise 2.0 loaded with device {:?}",
            oidn.device_type()
        ));

        let size = max_resolution * max_resolution * 4 * size_of::<f32>();

        if !oidn.reserve_buffer(size) {
            Log::msg(&format!("OIDN: failed to reserve {} bytes", size));
        }
    }

    let mut buffer = vec![0.0; max_resolution * max_resolution * 4];
    let mut buffer_normal = if app.config.lightmap_mode == LightmapMode::DominantDirection {
        vec![0.0; max_resolution * max_resolution * 4]
    } else {
        Vec::new()
    };

    for group_index in 0..app.groups.len() {
        let group = &app.groups[group_index];
        let settings = &group.settings;
        let resolution = group.settings.width as usize;

        Log::msg(&format!(
            "Processing Lightmap {}/{}",
            group_index + 1,
            app.groups.len()
        ));
        Log::progress(group_index as f32 / app.groups.len() as f32);

        decompact_diffuse(
            &mut buffer,
            &texels.visibility,
            &texels.coverage,
            &readback_buffer_slice,
            &mesh,
            &group,
            app.config.lightmap_mode == LightmapMode::CombinedSH,
        );

        const DILATE_DENOISE: usize = 2;
        const DILATE_FINAL: usize = 8;

        if settings.denoise {
            // fix denoiser artifacts on edges
            if settings.dilate {
                dilate_texture(&mut buffer, resolution, DILATE_DENOISE);
            }
            match &oidn {
                Some(oidn) => {
                    oidn.denoise(&mut buffer, resolution, resolution, false);
                }
                None => {
                    let message = format!("<color=red>Denoise Failed</color>");
                    Log::msg(&message);
                }
            }
        }

        if settings.dilate {
            dilate_texture(&mut buffer, resolution, DILATE_FINAL);
        }

        if settings.fix_seams {
            let channels = 3;
            fix_seams(
                &mut buffer,
                resolution as u32,
                resolution as u32,
                &app.seams,
                app.config.seams_debug,
                group_index as u32,
                channels,
            );
        }

        use exr::prelude::*;
        use half::f16;

        let full_path = app
            .output_dir
            .join(format!("Lightmap-{}_Diffuse.exr", group_index));

        let channels = SpecificChannels::rgb(|pos: Vec2<usize>| {
            let x = pos.x();
            let flipped_y = resolution - 1 - pos.y();
            let index = x + flipped_y * resolution;

            (
                f16::from_f32(buffer[index * 4 + 0]),
                f16::from_f32(buffer[index * 4 + 1]),
                f16::from_f32(buffer[index * 4 + 2]),
            )
        });

        let layer = Layer::new(
            (resolution, resolution),
            LayerAttributes::named("diffuse"),
            Encoding::FAST_LOSSLESS,
            channels,
        );

        Image::from_layer(layer).write().to_file(full_path).unwrap();

        if app.config.lightmap_mode == LightmapMode::DominantDirection {
            decompact_directional(
                &mut buffer,
                &texels.visibility,
                &texels.coverage,
                &readback_buffer_slice,
                &mesh,
                &group,
            );

            if settings.denoise {
                // fix denoiser artifacts on edges
                if settings.dilate {
                    dilate_texture(&mut buffer, resolution, DILATE_DENOISE);
                }
                match &oidn {
                    Some(oidn) => {
                        oidn.denoise(&mut buffer, resolution, resolution, true);
                    }
                    None => {
                        let message = format!("<color=red>Denoise Failed</color>");
                        Log::msg(&message);
                    }
                }
            }

            render_normals(
                &mut buffer_normal,
                &texels.visibility,
                &texels.coverage,
                &mesh,
                &group,
            );

            if settings.dilate {
                dilate_texture(&mut buffer, resolution, DILATE_FINAL);
                dilate_texture(&mut buffer_normal, resolution, DILATE_FINAL);
            }

            // let full_path = app
            //     .output_dir
            //     .join(format!("Lightmap-{}_Normal.tga", group_index));
            // save_tga(full_path, resolution, resolution, &buffer_normal, false).unwrap();

            encode_directional(&mut buffer, &buffer_normal);

            if settings.fix_seams {
                let channels = 4;
                fix_seams(
                    &mut buffer,
                    resolution as u32,
                    resolution as u32,
                    &app.seams,
                    app.config.seams_debug,
                    group_index as u32,
                    channels,
                );
            }

            let full_path = app
                .output_dir
                .join(format!("Lightmap-{}_Directional.tga", group_index));
            save_tga(full_path, resolution, resolution, &buffer, true).unwrap();
        }

        if app.config.lightmap_mode == LightmapMode::CombinedSH {
            decompact_combined_sh(
                &mut buffer,
                &texels.visibility,
                &texels.coverage,
                &readback_buffer_slice,
                &mesh,
                &group,
            );

            if settings.denoise {
                // fix denoiser artifacts on edges
                if settings.dilate {
                    dilate_texture(&mut buffer, resolution, DILATE_DENOISE);
                }
                match &oidn {
                    Some(oidn) => {
                        oidn.denoise(&mut buffer, resolution, resolution, false);
                    }
                    None => {
                        let message = format!("<color=red>Denoise Failed</color>");
                        Log::msg(&message);
                    }
                }
            }

            if settings.dilate {
                dilate_texture(&mut buffer, resolution, DILATE_FINAL);
            }

            if settings.fix_seams {
                let channels = 3;
                fix_seams(
                    &mut buffer,
                    resolution as u32,
                    resolution as u32,
                    &app.seams,
                    app.config.seams_debug,
                    group_index as u32,
                    channels,
                );
            }

            let full_path = app
                .output_dir
                .join(format!("Lightmap-{}_Directional.tga", group_index));
            save_tga(full_path, resolution, resolution, &buffer, false).unwrap();
        }
    }

    drop(buffer);
    drop(buffer_normal);
    readback_buffer.destroy(&app.vk);
    drop(readback_buffer);

    bake_light_probes(app);

    visibility_buffer.destroy(&app.vk);
    drop(visibility_buffer);
    compaction_buffer.destroy(&app.vk);
    drop(compaction_buffer);
    lightmaps_info.destroy(&app.vk);
    drop(lightmaps_info);
    lightmap_texels.destroy(&app.vk);
    drop(lightmap_texels);
}
