#![allow(unsafe_op_in_unsafe_fn)]

use ash::vk::{self, Handle};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{self, Write};
use std::mem;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Instant;
use std::time::Duration;

use crate::bindings::*;
use crate::buffer::Buffer;
use crate::camera::InitializePreviewPushConstants;
use crate::lights::LightType;
use crate::mesh::GEOMETRIES_VARIANTS;
use crate::meta::{MetaType, write_unity_texture_meta};
use crate::raster::{
    Texels, block_average_fill, build_compaction, decompact_combined_sh, decompact_diffuse,
    decompact_directional, decompact_shadowmask, dilate_shadowmask, dilate_texture,
    encode_directional, luminance, rasterize_texels, render_normals,
};
use crate::seams::{Seam, fix_seams};
use crate::sh::SHProbeL2;
use glam::{Vec2, Vec3, Vec4};

use crate::light_tree::LightTriangle;
use crate::shader::{
    PreviewPushConstants, RayTracingData, ShaderBindings, ShaderName, SpecializationConstants,
    load_compute_shader, update_compute_shader,
};
use crate::skybox::Skybox;
use crate::texture_array::{TextureArray, TextureDescriptor};
use crate::{
    camera::Camera,
    compute_shader::ComputeShader,
    lights::Light,
    mesh::{GpuMesh, Mesh, VulkanAs, create_tlas},
    oidn::Oidn,
    preview::{PreviewHost, PreviewStats, apply_host_camera, update_camera},
    preview_external::ExternalHost,
    texture2d::Texture2D,
    vulkan_context::{VulkanConfig, VulkanContext},
};

mod bindings;
mod buffer;
mod camera;
mod compute_shader;
mod cwbvh;
mod light_tree;
mod lights;
mod mesh;
mod meta;
mod neural;
mod oidn;
mod pack;
mod preview;
mod preview_external;
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
#[cfg(any(test, feature = "glfw"))]
mod window;

/// Where the interactive preview is displayed
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PreviewBackend {
    /// frames are read back and polled by the host application (Unity)
    External,
    /// standalone GLFW window with a swapchain
    #[cfg(any(test, feature = "glfw"))]
    Window,
}

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
    pub preview_host: Option<Box<dyn PreviewHost>>,

    pub meshes: [Mesh; GEOMETRIES_VARIANTS],

    pub cpu_lights: Vec<Light>,
    // cpu_lights[i] was added as light cpu_lights_source_index[i], empty if not sorted
    pub cpu_lights_source_index: Vec<u32>,
    pub cpu_lights_hash: u64,
    pub light_cookies: Vec<TexturePixelsData>,
    pub alphas: Vec<TexturePixelsData>,
    pub emissive_triangles_count: u32,
    pub groups: Vec<LightmapGroup>,
    pub seams: Vec<Seam>,

    pub gpu_mesh: GpuMesh,
    pub gpu_lights: Buffer,
    pub gpu_lights_staging: Buffer,

    pub tlas: VulkanAs,
    pub bvh_nodes: Buffer,
    pub bvh_triangles: Buffer,

    pub camera: Camera,

    pub preview_shader: ComputeShader,
    pub init_from_camera_shader: ComputeShader,
    pub preview_initialized: bool,

    pub probes: Vec<SHProbeL2>,

    pub render_target: RenderTarget,
    pub compacted_visibility: Buffer,

    pub constants: SpecializationConstants,

    pub output_dir: PathBuf,

    pub skybox: Skybox,

    pub albedo_array: TextureArray,
    pub emission_array: TextureArray,
    pub cookie_array: TextureArray,
    pub alphas_array: TextureArray,

    pub linear_sampler: vk::Sampler,

    pub benchmarks: Vec<(&'static str, f32)>,

    pub light_tree_nodes: Buffer,
    pub primitive_to_leaf: Buffer,
}

impl Drop for Glim {
    fn drop(&mut self) {
        if let Some(mut host) = self.preview_host.take() {
            unsafe { self.vk.device.device_wait_idle().unwrap() };
            host.destroy(&mut self.vk);
        }

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

        if !self.gpu_lights_staging.buffer.is_null() {
            self.gpu_lights_staging.destroy(&self.vk);
        }

        if !self.light_tree_nodes.buffer.is_null() {
            self.light_tree_nodes.destroy(&self.vk);
        }
        if !self.primitive_to_leaf.buffer.is_null() {
            self.primitive_to_leaf.destroy(&self.vk);
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
    pub diffuse_guid: String,
    pub directional_guid: String,
    pub shadowmask_guid: String,
    pub albedo_pixels: Vec<u8>,
    pub emission_pixels: Vec<u8>,
}

#[repr(C, align(16))]
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
    samples.clamp(0, MAX_SAMPLES)
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

    let shader = &app.init_from_camera_shader;

    let push = InitializePreviewPushConstants {
        rt: app.rt_data(),
        camera_position: app.camera.position,
        fov: app.camera.view_scale(),
        camera_direction: app.camera.forward,
        orthographic: app.camera.orthographic() as u32,
        camera_up: app.camera.up,
        pad0: 0,
        resolution: [
            app.config.preview_settings.width,
            app.config.preview_settings.height,
        ],
        pad1: 0,
        pad2: 0,
    };

    let push_bytes = as_bytes(&push);

    let visibility = &mut app.render_target.visibility;
    let diffuse = &mut app.render_target.diffuse;

    let vk = &mut app.vk;
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

        if diffuse.layout() != vk::ImageLayout::GENERAL {
            let barrier = diffuse.barrier(
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
            &push_bytes,
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
        if !app.config.is_preview {
            // first other lights, then area, then mixed
            // keep the source index, probe occlusion light indices are remapped back to it
            let count = app.cpu_lights.len();

            let mut source_index: Vec<u32> = Vec::with_capacity(count);
            for i in 0..count {
                source_index.push(i as u32);
            }

            let cpu_lights = &app.cpu_lights;
            source_index.sort_unstable_by_key(|&i| light_sort_order(&cpu_lights[i as usize]));

            let mut lights: Vec<Light> = Vec::with_capacity(count);
            for i in 0..count {
                lights.push(app.cpu_lights[source_index[i] as usize]);
            }

            app.cpu_lights = lights;
            app.cpu_lights_source_index = source_index;
        }

        app.gpu_lights_staging = Buffer::empty(
            &app.vk,
            String::from("Lights Staging"),
            (app.cpu_lights.len() * size_of::<Light>()) as u64,
            vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        );

        app.gpu_lights = Buffer::empty(
            &app.vk,
            String::from("Lights"),
            (app.cpu_lights.len() * size_of::<Light>()) as u64,
            vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        );

        app.upload_lights();
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
        app.skybox = Skybox::solid(&app.vk, 4, 4, Vec3::new(0.0, 0.0, 0.0));
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
        jitter_barycentrics: app.config.antialiasing as u32,
        emissive_triangles_count: app.emissive_triangles_count,
        pad0: 0,
        lightmap_group_count: app.groups.len() as u32,
        lightmap_mode: config.lightmap_mode as u32,
        coordinate_system: app.config.coordinate_system as u32,
        skybox_intensity: config.skybox_intensity,
        indirect_intensity: config.indirect_intensity,
        lightprobe_deringing: config.lightprobe_deringing,
        emissive_multiplier: app.config.emissive_multiplier,
        ao_intensity: config.ao_intensity,
        ao_range: config.ao_range,
        backface_texel_start: u32::MAX,
        pad1: 0,
        geometries_0_offset: geometries_offsets[0],
        geometries_1_offset: geometries_offsets[1],
        geometries_2_offset: geometries_offsets[2],
        geometries_3_offset: geometries_offsets[3],
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

fn bind_preview_shader(app: &mut Glim) {
    let shader_bindings = ShaderBindings {
        tlas: app.tlas.acceleration_structure,
        albedos: &app.albedo_array.views(),
        emissions: &app.emission_array.views(),
        skybox: app.skybox.view(),
        skybox_sampler: app.skybox.sampler(),
        visibility: app.render_target.visibility.view,
        preview_diffuse: app.render_target.diffuse.view(),
        cookies: &app.cookie_array.views(),
        alphas: &app.alphas_array.views(),
        linear_sampler: app.linear_sampler,
    };

    if app.preview_shader.pipeline.is_null() {
        app.preview_shader = load_compute_shader(
            &app.vk,
            ShaderName::Preview,
            &app.constants,
            &shader_bindings,
            size_of::<PreviewPushConstants>(),
        );
    }
    update_compute_shader(&app.vk, &app.preview_shader, &shader_bindings);
}

fn resize_preview(app: &mut Glim, push: &mut PreviewPushConstants, width: u32, height: u32) {
    unsafe { app.vk.device.device_wait_idle().unwrap() };

    app.config.preview_settings.width = width;
    app.config.preview_settings.height = height;
    push.resolution = [width, height];

    let preview_settings = app.config.preview_settings.clone();
    update_render_target(app, &preview_settings);
    bind_preview_shader(app);
}

fn render_preview(app: &mut Glim) {
    let Some(mut host) = app.preview_host.take() else {
        Log::msg("preview has no host");
        return;
    };

    let (width, height) = host.size(&app.vk);
    app.config.preview_settings.width = width.max(1);
    app.config.preview_settings.height = height.max(1);

    let preview_settings = app.config.preview_settings.clone();
    update_render_target(app, &preview_settings);
    bind_preview_shader(app);

    let mut push = PreviewPushConstants {
        rt: app.rt_data(),
        sample_index: 0,
        sample_count: app.config.direct_emission_samples,
        light_count: app.cpu_lights.len() as u32,
        bounce_count: app.config.bounce_count,
        resolution: [
            app.config.preview_settings.width,
            app.config.preview_settings.height,
        ],
        lights: app.gpu_lights.gpu_address,
    };

    let mut previous_time = std::time::Instant::now();

    let mut bake_start_time = std::time::Instant::now();
    let mut bake_complete_printed = false;

    let hardware_rt = app.config.hardware_rt && app.vk.as_device.is_some();

    LIGHT_UPDATE_READY.store(true, Ordering::Release);

    loop {
        let frame = host.begin_frame();
        if frame.close {
            break;
        }

        Log::progress(push.sample_index as f32 / push.sample_count as f32);

        let now = std::time::Instant::now();
        let delta_time = now.duration_since(previous_time).as_secs_f32();

        if let Some(settings) = frame.settings {
            app.config.throttle_preview_ms = settings.throttle_ms;

            let sample_count = clamp_samples(settings.sample_count.max(1));
            let bounce_count = clamp_bounces(settings.bounce_count);
            if (sample_count, bounce_count) != (push.sample_count, push.bounce_count) {
                app.config.direct_emission_samples = sample_count;
                app.config.bounce_count = bounce_count;
                push.sample_count = sample_count;
                push.bounce_count = bounce_count;
                app.preview_initialized = false;
            }
        }

        match &frame.camera {
            Some(camera) => apply_host_camera(app, camera),
            None => update_camera(app, &frame.input, delta_time),
        }

        if LIGHT_UPDATE_REQUESTED.load(Ordering::Acquire) {
            let changed = app.upload_lights();
            if changed {
                app.preview_initialized = false;
            }
            push.light_count = app.cpu_lights.len() as u32;
            push.lights = app.gpu_lights.gpu_address;
        }

        let (width, height) = host.size(&app.vk);
        if width == 0 || height == 0 {
            // minimized
            std::thread::sleep(Duration::from_millis(16));
            previous_time = now;
            continue;
        }

        if (width, height)
            != (
                app.config.preview_settings.width,
                app.config.preview_settings.height,
            )
        {
            resize_preview(app, &mut push, width, height);
        }

        if !app.preview_initialized {
            push.sample_index = 0;
            bake_start_time = std::time::Instant::now();
            bake_complete_printed = false;
        }

        // render finished
        if push.sample_index >= push.sample_count {
            std::thread::sleep(Duration::from_millis(16));
            if !bake_complete_printed {
                io::stdout().flush().unwrap();
                let bake_time = now.duration_since(bake_start_time).as_secs_f32();
                println!("bake complete in {}s", bake_time);
                bake_complete_printed = true;
            }
        }

        if !render_sample_camera(app, host.as_mut(), &mut push) {
            // output out of date, the size is queried again next iteration
            continue;
        }

        host.end_frame(&PreviewStats {
            sample_index: push.sample_index,
            sample_count: push.sample_count,
            width: app.config.preview_settings.width,
            height: app.config.preview_settings.height,
            light_count: push.light_count,
            hardware_rt,
        });

        // sleep for whatever is left of this iteration's frame time, measured from its own start
        if app.config.throttle_preview_ms > 0 {
            let deadline = now + Duration::from_millis(app.config.throttle_preview_ms as u64);
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if !remaining.is_zero() {
                std::thread::sleep(remaining);
            }
        }

        previous_time = now;
    }

    unsafe { app.vk.device.device_wait_idle().unwrap() };
    host.destroy(&mut app.vk);
}

fn render_sample_camera(
    app: &mut Glim,
    host: &mut dyn PreviewHost,
    push: &mut PreviewPushConstants,
) -> bool {
    let width = app.config.preview_settings.width;
    let height = app.config.preview_settings.height;

    let Some(cmd) = host.acquire(&mut app.vk, width, height) else {
        return false;
    };

    let begin_info = vk::CommandBufferBeginInfo {
        flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
        ..Default::default()
    };

    let mut changed = false;

    unsafe {
        let vk = &app.vk.device;
        vk.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())
            .unwrap();

        vk.begin_command_buffer(cmd, &begin_info).unwrap();

        if push.sample_index == 0 {
            update_visibility_from_camera(app, cmd);
            app.preview_initialized = true;
            changed = true;

            let diffuse = &mut app.render_target.diffuse;

            let vk = &app.vk.device;
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

        if push.sample_index < push.sample_count {
            let shader = &app.preview_shader;

            let push_bytes = as_bytes(push);

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
                &push_bytes,
            );
            vk.cmd_dispatch(cmd, groups_x, groups_y, 1);
            push.sample_index += 1;
            changed = true;
        }

        let barrier = diffuse.barrier(
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            vk::AccessFlags::SHADER_WRITE,
            vk::AccessFlags::TRANSFER_READ,
        );
        vk.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[barrier],
        );
    }

    host.record_output(
        &app.vk,
        cmd,
        app.render_target.diffuse.image(),
        width,
        height,
        changed,
    );

    unsafe { app.vk.device.end_command_buffer(cmd).unwrap() };

    host.submit(&mut app.vk, cmd, width, height)
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

    app.preview_initialized = false;
}

fn decode_r11g11b10(packed: u32) -> Vec3 {
    use half::f16;
    let decode = |bits: u32| {
        let v = f16::from_bits(bits as u16).to_f32();
        if v.is_finite() { v } else { 0.0 }
    };
    Vec3::new(
        decode((packed & 0x7ff) << 4),
        decode(((packed >> 11) & 0x7ff) << 4),
        decode(((packed >> 22) & 0x3ff) << 5),
    )
}

fn extract_emissive_triangles(app: &mut Glim) {
    fn sample_emission(uv: Vec2, w: u32, h: u32, pixels: &[u8]) -> Vec3 {
        let x = (uv.x * w as f32).clamp(0.0, (w - 1) as f32) as usize;
        let y = (uv.y * h as f32).clamp(0.0, (h - 1) as f32) as usize;
        let index = (y * w as usize + x) * 4;

        let packed = u32::from_le_bytes([
            pixels[index],
            pixels[index + 1],
            pixels[index + 2],
            pixels[index + 3],
        ]);

        decode_r11g11b10(packed)
    }

    let mut light_triangles: Vec<LightTriangle> = Vec::new();
    let mut primitive_base: u32 = 0;

    if app.config.light_tree {
        for mesh in &app.meshes {
            let triangle_count = mesh.indices.len() / 3;

            for (local_id, tri) in mesh.indices.chunks_exact(3).enumerate() {
                let v0 = &mesh.vertices[tri[0] as usize];
                let v1 = &mesh.vertices[tri[1] as usize];
                let v2 = &mesh.vertices[tri[2] as usize];

                if !v0.emissive() {
                    continue;
                }

                let group = &app.groups[v0.group() as usize];
                let (w, h) = (group.settings.width, group.settings.height);
                let pixels = &group.emission_pixels;

                let (uv0, uv1, uv2) = (v0.uv, v1.uv, v2.uv);
                let uvs = [
                    (uv0 + uv1 + uv2) / 3.0,
                    uv0,
                    uv1,
                    uv2,
                    (uv0 + uv1) * 0.5,
                    (uv1 + uv2) * 0.5,
                    (uv2 + uv0) * 0.5,
                ];
                let average: Vec3 = uvs
                    .iter()
                    .map(|&uv| sample_emission(uv, w, h, pixels))
                    .sum::<Vec3>()
                    / uvs.len() as f32
                    * mesh.emission_multiplier(local_id);

                light_triangles.extend(LightTriangle::new(
                    primitive_base + local_id as u32,
                    [v0.position, v1.position, v2.position],
                    luminance(average),
                    v0.backface_gi(),
                ));
            }

            primitive_base += triangle_count as u32;
        }
    }

    let tree = light_tree::build(&light_triangles, primitive_base as usize);

    if app.config.light_tree {
        Log::msg(&format!(
            "Light tree: {} emissive triangles, {} nodes",
            light_triangles.len(),
            tree.nodes.len()
        ));
    }

    let light_tree_nodes = upload_buffer_or_empty(&app.vk, "Light Tree Nodes", &tree.nodes, 64);
    let primitive_to_leaf = upload_buffer_or_empty(
        &app.vk,
        "Light Tree Primitive To Leaf",
        &tree.primitive_to_leaf,
        32,
    );

    app.light_tree_nodes = light_tree_nodes;
    app.primitive_to_leaf = primitive_to_leaf;
    app.emissive_triangles_count = light_triangles.len() as u32;
}

fn upload_buffer_or_empty<T>(
    vk: &VulkanContext,
    name: &str,
    data: &[T],
    empty_size: vk::DeviceSize,
) -> Buffer {
    let usage = vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS;
    let local = vk::MemoryPropertyFlags::DEVICE_LOCAL;
    if data.is_empty() {
        Buffer::empty(vk, name.to_string(), empty_size, usage, local)
    } else {
        Buffer::new(vk, name.to_string(), data, usage, local)
    }
}

impl LightmapGroup {
    fn new(
        settings: LightmapSettings,
        albedo_pixels: &[u8],   // RGB32
        emission_pixels: &[u8], // RGB111110Float
        diffuse_guid: String,
        directional_guid: String,
        shadowmask_guid: String,
    ) -> LightmapGroup {
        LightmapGroup {
            settings,
            albedo_pixels: albedo_pixels.to_vec(),
            emission_pixels: emission_pixels.to_vec(),
            id: 0,
            diffuse_guid,
            directional_guid,
            shadowmask_guid,
        }
    }
}

impl Glim {
    pub fn new(config: GlimConfig, output_dir: PathBuf, backend: PreviewBackend) -> Glim {
        unsafe { LOG_CALLBACK = config.log_callback };

        let preview_width = config.preview_settings.width;
        let preview_height = config.preview_settings.height;

        #[cfg(any(test, feature = "glfw"))]
        let use_window = config.is_preview && backend == PreviewBackend::Window;
        #[cfg(not(any(test, feature = "glfw")))]
        let use_window = {
            let _ = backend;
            false
        };

        #[cfg_attr(not(any(test, feature = "glfw")), allow(unused_mut))]
        let mut vulkan_config = VulkanConfig {
            enable_validation_layers: config.vulkan_validation_layers,
            enable_window: use_window,
            window_extensions: Vec::new(),
        };

        #[cfg(any(test, feature = "glfw"))]
        let window = use_window
            .then(|| window::GlfwHost::new(preview_width, preview_height, &mut vulkan_config));

        let create_surface_callback = |_instance: &ash::Instance| -> vk::SurfaceKHR {
            #[cfg(any(test, feature = "glfw"))]
            if let Some(window) = &window {
                return window.create_surface(_instance);
            }
            unreachable!("surface requested without a window");
        };

        #[cfg_attr(not(any(test, feature = "glfw")), allow(unused_mut))]
        let mut vk = VulkanContext::new(&vulkan_config, create_surface_callback);
        println!("Vulkan Initialized");

        let mut preview_host: Option<Box<dyn PreviewHost>> = None;

        #[cfg(any(test, feature = "glfw"))]
        if let Some(window) = window {
            vk.create_swapchain(preview_width, preview_height);
            preview_host = Some(Box::new(window));
        }

        if config.is_preview && preview_host.is_none() {
            preview_host = Some(Box::new(ExternalHost::new(
                &vk,
                preview_width,
                preview_height,
            )));
        }

        let mut pos = config.camera_position;
        config.coordinate_system.transform(&mut pos);
        let mut camera = Camera {
            position: pos,
            forward: Vec3::Z,
            up: Vec3::Y,
            fov: 60.0,
            ortho_size: 0.0,
        };
        let mut fwd = config.camera_forward;
        config.coordinate_system.transform(&mut fwd);
        camera.set_forward(fwd);

        let init_from_camera_shader = ComputeShader::null();

        let gpu_lights = Buffer::null();

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
                tangents: Vec::new(),
                main_uvs: Vec::new(),
                original_normals: Vec::new(),
                transparent: false,
                no_shadows: false,
            },
            Mesh {
                vertices: Vec::new(),
                indices: Vec::new(),
                tangents: Vec::new(),
                main_uvs: Vec::new(),
                original_normals: Vec::new(),
                transparent: true,
                no_shadows: false,
            },
            Mesh {
                vertices: Vec::new(),
                indices: Vec::new(),
                tangents: Vec::new(),
                main_uvs: Vec::new(),
                original_normals: Vec::new(),
                transparent: false,
                no_shadows: true,
            },
            Mesh {
                vertices: Vec::new(),
                indices: Vec::new(),
                tangents: Vec::new(),
                main_uvs: Vec::new(),
                original_normals: Vec::new(),
                transparent: true,
                no_shadows: true,
            },
        ];

        Self {
            vk,
            meshes,
            preview_host,
            config: config,
            cpu_lights: Vec::new(),
            cpu_lights_source_index: Vec::new(),
            preview_shader: ComputeShader::null(),
            gpu_mesh: GpuMesh::null(),
            tlas: VulkanAs::null(),
            groups: Vec::new(),
            camera,
            init_from_camera_shader,
            preview_initialized: false,
            gpu_lights,
            render_target,
            probes: Vec::new(),
            seams: Vec::new(),
            emissive_triangles_count: 0,
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
            alphas: Vec::new(),
            alphas_array: TextureArray::null(),
            compacted_visibility: Buffer::null(),
            benchmarks: Vec::with_capacity(32),
            gpu_lights_staging: Buffer::null(),
            cpu_lights_hash: 0,
            light_tree_nodes: Buffer::null(),
            primitive_to_leaf: Buffer::null(),
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

    fn upload_lights(&mut self) -> bool {
        if self.cpu_lights.len() == 0 {
            LIGHT_UPDATE_REQUESTED.store(false, Ordering::Relaxed);
            return false;
        }

        let mut h = DefaultHasher::new();
        self.cpu_lights.hash(&mut h);
        let fingerprint = h.finish();

        if self.cpu_lights_hash == fingerprint {
            LIGHT_UPDATE_REQUESTED.store(false, Ordering::Relaxed);
            return false;
        }

        let capacity = self.gpu_lights_staging.bytes as usize / size_of::<Light>();
        assert!(capacity >= self.cpu_lights.len());

        // if capacity < self.cpu_lights.len() {
        //     Log::msg("Resizing Lights Buffer");
        //     self.gpu_lights_staging.destroy(&self.vk);
        //     self.gpu_lights_staging = Buffer::empty(
        //         &self.vk,
        //         String::from("Lights Staging"),
        //         (self.cpu_lights.len() * size_of::<Light>()) as u64,
        //         vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::TRANSFER_SRC,
        //         vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        //     );
        // }

        let lights: &mut [Light] = unsafe {
            std::slice::from_raw_parts_mut(
                self.gpu_lights_staging.ptr as *mut Light,
                self.cpu_lights.len(),
            )
        };

        unsafe {
            std::ptr::copy_nonoverlapping(
                self.cpu_lights.as_ptr(),
                lights.as_mut_ptr(),
                lights.len(),
            )
        };

        let cmd = self.vk.begin_single_use_cmd();
        unsafe {
            let region = vk::BufferCopy {
                src_offset: 0,
                dst_offset: 0,
                size: self.gpu_lights_staging.bytes,
            };
            self.vk.device.cmd_copy_buffer(
                cmd,
                self.gpu_lights_staging.buffer,
                self.gpu_lights.buffer,
                &[region],
            );
        }
        self.vk.end_single_use_cmd(cmd);

        self.cpu_lights_hash = fingerprint;

        LIGHT_UPDATE_REQUESTED.store(false, Ordering::Relaxed);
        true
    }

    pub fn rt_data(&self) -> RayTracingData {
        RayTracingData {
            indices: self.gpu_mesh.index_buffer.gpu_address,
            vertices: self.gpu_mesh.vertex_buffer.gpu_address,
            bvh_nodes: self.bvh_nodes.gpu_address,
            bvh_triangles: self.bvh_triangles.gpu_address,
            main_uv: self.gpu_mesh.main_uv_buffer.gpu_address,
            visibility: self.compacted_visibility.gpu_address,
        }
    }
}

fn save_tga(path: &PathBuf, w: usize, h: usize, pixels: &[Vec4], alpha: bool) -> io::Result<()> {
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
                let index = x + flipped_y * w;
                let r = (pixels[index].x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                let g = (pixels[index].y.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                let b = (pixels[index].z.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                let a = (pixels[index].w.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;

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
                let index = x + flipped_y * w;
                let r = (pixels[index].x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                let g = (pixels[index].y.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                let b = (pixels[index].z.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;

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

fn bake_shadowmasks(app: &mut Glim, texels: &Texels, buffer: &mut Vec<Vec4>) {
    if is_cancelled() {
        return;
    }

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

    let visibility_texels_count = texels.front_count;

    let mut shadowmask_buffer = Buffer::empty(
        &app.vk,
        "Indirect Buffer".to_owned(),
        (visibility_texels_count * size_of::<f32>() * 4) as u64,
        vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
            | vk::BufferUsageFlags::TRANSFER_SRC
            | vk::BufferUsageFlags::TRANSFER_DST,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    );
    zero_buffer(&app.vk, &shadowmask_buffer);

    Log::msg("Baking Shadowmasks");

    // hard shadows only need enough samples for anti-aliasing, soft shadows sample the
    // light radius/angle and are not denoised, so use at least the direct light sample count
    const SHADOWMASK_MIN_SAMPLES: u32 = 32;
    let soft_shadows = app.cpu_lights.iter().any(|l| {
        l.occlusion_channel != u8::MAX && l.cast_shadows && l.shadow_radius_or_angle > 0.0
    });
    let shadowmask_samples = if soft_shadows {
        app.config.direct_light_samples.max(SHADOWMASK_MIN_SAMPLES)
    } else {
        SHADOWMASK_MIN_SAMPLES
    };

    #[repr(C, align(16))]
    pub struct PushConstants {
        pub rt: RayTracingData,

        pub lights_start: u32,
        pub lights_count: u32,
        pub compacted_count: u32,
        pub pad0: u32,

        pub pixels: vk::DeviceAddress,
        pub sample_index: u32,
        pub sample_count: u32,

        pub lights: vk::DeviceAddress,
        pub pad1: u32,
        pub pad2: u32,
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

    let groups_x = (visibility_texels_count as u32 + 63) / 64;

    let mut push = PushConstants {
        rt: app.rt_data(),
        lights_start: 0,
        lights_count: app.cpu_lights.len() as u32,
        compacted_count: visibility_texels_count as u32,
        pad0: 0,
        pixels: shadowmask_buffer.gpu_address,
        sample_index: 0,
        sample_count: shadowmask_samples,
        lights: app.gpu_lights.gpu_address,
        pad1: 0,
        pad2: 0,
    };

    let start = Instant::now();

    for sample_index in 0..shadowmask_samples {
        push.sample_index = sample_index;
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
            vk.cmd_dispatch(cmd, groups_x, 1, 1);
        }
        app.vk.end_single_use_cmd(cmd);
    }

    let elapsed = start.elapsed().as_secs_f32();
    app.benchmarks.push(("Shadow Mask Bake", elapsed));

    shader.destroy(&app.vk);

    let mut readback_buffer = Buffer::empty(
        &app.vk,
        "Readback Buffer".to_owned(),
        shadowmask_buffer.bytes,
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
            size: shadowmask_buffer.bytes,
        };

        app.vk.device.cmd_copy_buffer(
            cmd,
            shadowmask_buffer.buffer,
            readback_buffer.buffer,
            &[region],
        );

        app.vk.end_single_use_cmd(cmd);
    }
    shadowmask_buffer.destroy(&app.vk);
    drop(shadowmask_buffer);

    let readback_buffer_slice: &[f32] = unsafe {
        std::slice::from_raw_parts(
            readback_buffer.ptr as *const f32,
            visibility_texels_count * 4,
        )
    };

    let mut max_resolution = 1;
    for g in &app.groups {
        max_resolution = max_resolution.max(g.settings.width as usize);
    }

    let mut buffer_coverage = vec![0.0f32; max_resolution * max_resolution];

    for group_index in 0..app.groups.len() {
        let group = &app.groups[group_index];
        let settings = &group.settings;
        let resolution = settings.width as usize;

        if settings.discard {
            continue;
        }

        decompact_shadowmask(
            buffer,
            &mut buffer_coverage,
            &texels,
            &readback_buffer_slice,
            &app.gpu_mesh.merged_mesh,
            &group,
        );

        dilate_shadowmask(buffer, &mut buffer_coverage, resolution, 8);

        let path = app
            .output_dir
            .join(format!("Lightmap-{}_ShadowMask.tga", group_index));

        if let Err(err) = save_tga(
            &path,
            resolution as usize,
            resolution as usize,
            buffer,
            true,
        ) {
            Log::msg(&format!("Failed to save shadowmask: {}", err));
        }

        write_unity_texture_meta(MetaType::Shadowmask, &path, &group.shadowmask_guid);
    }
    readback_buffer.destroy(&app.vk);
    drop(readback_buffer);
}

fn zero_buffer(vk: &VulkanContext, buffer: &Buffer) {
    let cmd = vk.begin_single_use_cmd();

    unsafe {
        vk.device
            .cmd_fill_buffer(cmd, buffer.buffer, 0, buffer.bytes, 0);
    }

    vk.end_single_use_cmd(cmd);
}

fn init_samples(app: &Glim, visibility_texels_count: usize) {
    #[repr(C, align(16))]
    pub struct PushConstants {
        pub rt: RayTracingData,

        pub visibility: vk::DeviceAddress,
        pub compacted_count: u32,
        pub leak_detection: u32,
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

    let mut shader = load_compute_shader(
        &app.vk,
        ShaderName::AdjustSamples,
        &app.constants,
        &shader_bindings,
        size_of::<PushConstants>(),
    );
    update_compute_shader(&app.vk, &shader, &shader_bindings);

    let push = PushConstants {
        rt: app.rt_data(),
        visibility: app.compacted_visibility.gpu_address,
        compacted_count: visibility_texels_count as u32,
        leak_detection: app.config.leak_detection as u32,
    };

    let groups_x = (visibility_texels_count as u32 + 63) / 64;

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

    shader.destroy(&app.vk);
}

fn bake_direct_emission(app: &mut Glim, visibility_texels_count: usize, lightmap: &Buffer) {
    if is_cancelled() {
        return;
    }

    if app.config.direct_emission_samples == 0 {
        return;
    }

    let message = format!("Baking Direct Emission");
    Log::msg(&message);

    #[repr(C, align(16))]
    pub struct PushConstants {
        pub rt: RayTracingData,

        pub compacted_count: u32,
        pub sample_index: u32,
        pub max_samples: u32,
        pub pad0: u32,

        pub lightmap: vk::DeviceAddress,
        pub light_tree: vk::DeviceAddress,
        pub primitive_to_leaf: vk::DeviceAddress,
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
        rt: app.rt_data(),
        compacted_count: visibility_texels_count as u32,
        sample_index: 0,
        max_samples: app.config.direct_emission_samples,
        pad0: 0,
        lightmap: lightmap.gpu_address,
        light_tree: app.light_tree_nodes.gpu_address,
        primitive_to_leaf: app.primitive_to_leaf.gpu_address,
    };

    let groups_x = (visibility_texels_count as u32 + 63) / 64;
    let shader = &bake_direct_emission_shader;

    let start = Instant::now();

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

    let elapsed = start.elapsed().as_secs_f32();
    app.benchmarks.push(("Direct Emission Bake", elapsed));

    bake_direct_emission_shader.destroy(&app.vk);
    drop(bake_direct_emission_shader);
}

fn bake_direct_lights(
    app: &mut Glim,
    visibility_texels_count: usize,
    lights_start: usize,
    lights_count: usize,
    samples: u32,
    mixed: bool,
    lightmap: &Buffer,
) {
    if lights_count == 0 {
        return;
    }

    if samples == 0 {
        return;
    }

    #[repr(C, align(16))]
    pub struct PushConstants {
        pub rt: RayTracingData,

        pub texel_count: u32,
        pub sample_index: u32,
        pub max_samples: u32,
        pub mixed: u32,

        pub lightmap: vk::DeviceAddress,
        pub pad0: u32,
        pub pad1: u32,

        pub lights: vk::DeviceAddress,
        pub lights_start: u32,
        pub lights_end: u32,
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
        rt: app.rt_data(),
        texel_count: visibility_texels_count as u32,
        sample_index: 0,
        max_samples: samples,
        mixed: mixed as u32,
        lightmap: lightmap.gpu_address,
        pad0: 0,
        pad1: 0,
        lights: app.gpu_lights.gpu_address,
        lights_start: lights_start as u32,
        lights_end: (lights_start + lights_count) as u32,
    };

    let groups_x = (visibility_texels_count as u32 + 63) / 64;
    let shader = &bake_direct_light_shader;

    let start = Instant::now();

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

    let elapsed = start.elapsed().as_secs_f32();
    app.benchmarks.push(("Direct Lights Bake", elapsed));

    bake_direct_light_shader.destroy(&app.vk);
    drop(bake_direct_light_shader);
}

fn bake_indirect(
    app: &mut Glim,
    lightmap: &Buffer,
    visibility_texels_count: usize,
    mixed_start: usize,
    mixed_count: usize,
    compaction_buffer: &Buffer,
    lightmaps_info: &Buffer,
) {
    if app.config.bounce_count == 0 {
        return;
    }

    if app.config.indirect_samples == 0 {
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
        dst_offset: 0,
        size: (size_of::<f32>() * 3 * visibility_texels_count) as u64,
    };
    unsafe {
        app.vk
            .device
            .cmd_copy_buffer(cmd, lightmap.buffer, indirect_buffer.buffer, &[region])
    };
    app.vk.end_single_use_cmd(cmd);

    Log::msg(&format!("Baking {} Mixed Lights", mixed_count));
    bake_direct_lights(
        app,
        visibility_texels_count,
        mixed_start,
        mixed_count,
        app.config.direct_light_samples,
        true,
        &indirect_buffer,
    );

    #[repr(C, align(16))]
    pub struct PushConstants {
        pub rt: RayTracingData,

        pub compacted_count: u32,
        pub sample_index: u32,
        pub sample_count: u32,
        pub bounce_index: u32,

        pub bounce_buffer: vk::DeviceAddress,
        pub compaction: vk::DeviceAddress,

        pub lightmaps_info: vk::DeviceAddress,
        pub lightmap: vk::DeviceAddress,
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
        rt: app.rt_data(),
        compacted_count: visibility_texels_count as u32,
        sample_index: 0,
        sample_count: app.config.indirect_samples,
        bounce_index: 0,
        bounce_buffer: indirect_buffer.gpu_address,
        compaction: compaction_buffer.gpu_address,
        lightmaps_info: lightmaps_info.gpu_address,
        lightmap: lightmap.gpu_address,
    };

    let bounce_samples: Vec<u32> = (0..app.config.bounce_count)
        .map(|bounce_index| bounce_sample_count(app.config.indirect_samples, bounce_index))
        .collect();
    let total_steps = bounce_samples.iter().sum::<u32>() as f32;
    let mut steps_done = 0u32;

    let start = Instant::now();

    'bounces: for bounce_index in 0..app.config.bounce_count {
        let samples = bounce_samples[bounce_index as usize];
        push.bounce_index = bounce_index;
        push.sample_count = samples;

        let message = format!("Baking Bounce {} ({} samples)", bounce_index + 1, samples);
        Log::msg(&message);

        for sample_index in 0..samples {
            if is_cancelled() {
                break 'bounces;
            }
            push.sample_index = sample_index;
            let push_bytes = as_bytes(&push);

            Log::progress(steps_done as f32 / total_steps);
            steps_done += 1;

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

    let elapsed = start.elapsed().as_secs_f32();
    app.benchmarks.push(("Bounces Bake", elapsed));

    indirect_shader.destroy(&app.vk);
    indirect_buffer.destroy(&app.vk);
}

/// Later bounces carry less energy and are lower frequency, so each one after the first
/// gets half the samples of the previous, never going below `MIN_BOUNCE_SAMPLES`
/// (or the configured count if that is lower).
fn bounce_sample_count(indirect_samples: u32, bounce_index: u32) -> u32 {
    const MIN_BOUNCE_SAMPLES: u32 = 64;
    let floor = indirect_samples.min(MIN_BOUNCE_SAMPLES);
    indirect_samples
        .checked_shr(bounce_index)
        .unwrap_or(0)
        .max(floor)
}

fn light_sort_order(light: &Light) -> u8 {
    if light.ty == LightType::Area || light.ty == LightType::Disc {
        1
    } else if light.mixed {
        2
    } else {
        0
    }
}

fn remap_light_index(source_index: &[u32], index: i32) -> i32 {
    if index < 0 {
        return -1;
    }

    source_index[index as usize] as i32
}

// moves probes out of geometry and disables jitter near surfaces, runs once before baking
fn adjust_light_probes(
    app: &Glim,
    shader_bindings: &ShaderBindings,
    probes_buffer: &Buffer,
    probes_count: u32,
    groups_x: u32,
) {
    #[repr(C, align(16))]
    pub struct PushConstants {
        pub rt: RayTracingData,

        pub sh_probes: vk::DeviceAddress,
        pub probes_count: u32,
    }

    let mut shader = load_compute_shader(
        &app.vk,
        ShaderName::AdjustProbes,
        &app.constants,
        shader_bindings,
        size_of::<PushConstants>(),
    );
    update_compute_shader(&app.vk, &shader, shader_bindings);

    let push = PushConstants {
        rt: app.rt_data(),
        sh_probes: probes_buffer.gpu_address,
        probes_count,
    };

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

    shader.destroy(&app.vk);
}

fn bake_light_probes(
    app: &mut Glim,
    mixed_start: usize,
    mixed_count: usize,
    compaction: &Buffer,
    lightmap_info: &Buffer,
    lightmap: &Buffer,
) {
    if app.probes.len() == 0 {
        return;
    }

    if app.config.probe_samples == 0 {
        return;
    }

    if is_cancelled() {
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

    #[repr(C, align(16))]
    pub struct PushConstants {
        pub rt: RayTracingData,

        pub lights_count: u32,
        pub max_samples: u32,
        pub sample_index: u32,
        pub probes_count: u32,

        pub sh_probes: vk::DeviceAddress,
        pub mixed_start: u32,
        pub mixed_end: u32,

        pub compaction: vk::DeviceAddress,
        pub lightmap_info: vk::DeviceAddress,

        pub lights: vk::DeviceAddress,
        pub lightmap: vk::DeviceAddress,

        pub light_tree: vk::DeviceAddress,
        pub primitive_to_leaf: vk::DeviceAddress,
    }

    assert!(size_of::<PushConstants>() <= 128);

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
        rt: app.rt_data(),
        lights_count: app.cpu_lights.len() as u32,
        max_samples: app.config.probe_samples,
        sample_index: 0,
        probes_count: app.probes.len() as u32,
        sh_probes: probes_buffer.gpu_address,
        mixed_start: mixed_start as u32,
        mixed_end: (mixed_start + mixed_count) as u32,
        compaction: compaction.gpu_address,
        lightmap_info: lightmap_info.gpu_address,
        lights: app.gpu_lights.gpu_address,
        lightmap: lightmap.gpu_address,
        light_tree: app.light_tree_nodes.gpu_address,
        primitive_to_leaf: app.primitive_to_leaf.gpu_address,
    };

    let probes_count = app.probes.len() as u32;
    let groups_x = (probes_count + 63) / 64;

    if app.config.leak_detection {
        adjust_light_probes(app, &shader_bindings, &probes_buffer, probes_count, groups_x);
    }

    let vk = &app.vk.device;

    let start = Instant::now();

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

    let elapsed = start.elapsed().as_secs_f32();
    app.benchmarks.push(("Light Probes Bake", elapsed));

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

    // probe occlusion light indices point into the sorted gpu lights, remap them to the source light order
    if !app.cpu_lights_source_index.is_empty() {
        let probes = unsafe {
            std::slice::from_raw_parts_mut(
                staging_buffer_light_probes.ptr as *mut SHProbeL2,
                app.probes.len(),
            )
        };
        let source_index = &app.cpu_lights_source_index;

        for i in 0..probes.len() {
            let probe = &mut probes[i];
            probe.occlusion_light0 = remap_light_index(source_index, probe.occlusion_light0);
            probe.occlusion_light1 = remap_light_index(source_index, probe.occlusion_light1);
            probe.occlusion_light2 = remap_light_index(source_index, probe.occlusion_light2);
            probe.occlusion_light3 = remap_light_index(source_index, probe.occlusion_light3);
        }
    }

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

    let texels = rasterize_texels(
        &app.gpu_mesh.merged_mesh,
        &app.groups,
        app.config.coverage_min_threshold.clamp(0.0, 1.0),
        app.config.coverage_max_samples.clamp(1, 32),
    );
    let texels_count = texels.visibility.len();

    let compaction = build_compaction(&texels, &app.gpu_mesh.merged_mesh, &app.groups);

    app.constants.backface_texel_start = texels.front_count as u32;

    let mut lightmaps_info = {
        let group_count = app.groups.len();
        let mut infos = Vec::with_capacity(group_count * 2);
        for i in 0..group_count * 2 {
            let group = &app.groups[i % group_count].settings;
            infos.push(LightmapInfo {
                resolution: [group.width, group.height],
                compaction_offset: compaction.offsets[i],
                table_base: compaction.table_bases[i],
            });
        }

        let lightmaps_info = Buffer::new(
            &app.vk,
            "Lightmap Info".into(),
            &infos,
            vk::BufferUsageFlags::TRANSFER_DST
                | vk::BufferUsageFlags::TRANSFER_SRC
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        );

        lightmaps_info
    };

    let mut compaction_buffer = Buffer::new(
        &app.vk,
        "Compaction Buffer".to_owned(),
        &compaction.words,
        vk::BufferUsageFlags::TRANSFER_DST
            | vk::BufferUsageFlags::TRANSFER_SRC
            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    );

    drop(compaction);

    Log::msg(&format!(
        "Created {} texels for baking ({} front, {} back face).",
        texels_count,
        texels.front_count,
        texels_count - texels.front_count
    ));

    let visibility_buffer = Buffer::new(
        &app.vk,
        String::from("Visibility Buffer"),
        &texels.visibility,
        vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    );
    app.compacted_visibility = visibility_buffer;

    let lightmap_texels_channels = match app.config.lightmap_mode {
        LightmapMode::NonDirectional => 3,
        LightmapMode::DominantDirection => 6,
        LightmapMode::CombinedSH => 9,
        LightmapMode::Neural => 3,
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
    zero_buffer(&app.vk, &lightmap_texels);

    init_samples(app, texels_count);

    bake_direct_emission(app, texels_count, &lightmap_texels);

    let export_debug_textures = false;

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
        &lightmap_texels,
    );

    Log::msg(&format!("Baking {} Area Lights", area_count));
    bake_direct_lights(
        app,
        texels_count,
        area_start,
        area_count,
        app.config.direct_emission_samples,
        false,
        &lightmap_texels,
    );

    bake_indirect(
        app,
        &lightmap_texels,
        texels_count,
        mixed_start,
        mixed_count,
        &compaction_buffer,
        &lightmaps_info,
    );

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

    let readback_buffer_slice: &[f32] = unsafe {
        std::slice::from_raw_parts(
            readback_buffer.ptr as *const f32,
            lightmap_texels_channels * texels_count,
        )
    };

    if app.config.lightmap_mode == LightmapMode::Neural && !is_cancelled() {
        Log::msg("Training Neural Lightmaps");
        let result = neural::train_neural(
            app,
            &texels,
            &lightmap_texels,
            readback_buffer_slice,
            &compaction_buffer,
            &lightmaps_info,
            mixed_start,
            mixed_count,
        );
        if let Some(result) = result {
            if !is_cancelled() {
                let debug_images = std::env::var("GLIM_NN_NO_DEBUG").is_err();
                neural::export_neural(app, &result, debug_images);
            }
        }
    }

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

    let mut buffer = vec![Vec4::ZERO; max_resolution * max_resolution];
    let mut mask = vec![false; max_resolution * max_resolution];
    let mut buffer_copy = vec![Vec4::ZERO; max_resolution * max_resolution];

    for group_index in 0..app.groups.len() {
        let group = &app.groups[group_index];
        let settings = &group.settings;
        let resolution = group.settings.width as usize;

        if settings.discard {
            continue;
        }

        if is_cancelled() {
            continue;
        }

        Log::msg(&format!(
            "Processing Lightmap {}/{}",
            group_index + 1,
            app.groups.len()
        ));
        Log::progress(group_index as f32 / app.groups.len() as f32);

        let start = Instant::now();
        decompact_diffuse(
            &mut buffer,
            &texels,
            &readback_buffer_slice,
            &app.gpu_mesh.merged_mesh,
            &group,
            app.config.lightmap_mode == LightmapMode::CombinedSH,
        );
        let elapsed = start.elapsed().as_secs_f32();
        app.benchmarks.push(("Diffuse Decompact", elapsed));

        const DILATE_DENOISE: usize = 1;
        const DILATE_FINAL: usize = 1;
        const BLOCK_FILL: bool = true;

        if settings.denoise {
            // fix denoiser artifacts on edges
            if settings.dilate {
                dilate_texture(&mut buffer, &mut mask, resolution, DILATE_DENOISE);
            }
            match &oidn {
                Some(oidn) => {
                    let start = Instant::now();
                    oidn.denoise(&mut buffer, resolution, resolution, false);
                    let elapsed = start.elapsed().as_secs_f32();
                    app.benchmarks.push(("Denoise Diffuse", elapsed));
                }
                None => {
                    let message = format!("<color=red>Denoise Failed</color>");
                    Log::msg(&message);
                }
            }
        }

        let start = Instant::now();
        if settings.dilate {
            dilate_texture(&mut buffer, &mut mask, resolution, DILATE_FINAL);
        }
        let elapsed = start.elapsed().as_secs_f32();
        app.benchmarks.push(("Diffuse Dilate", elapsed));

        if settings.fix_seams {
            let channels = 3;
            let start = Instant::now();
            fix_seams(
                &mut buffer,
                resolution as u32,
                resolution as u32,
                &app.seams,
                app.config.seams_debug,
                group_index as u32,
                channels,
            );
            let elapsed = start.elapsed().as_secs_f32();
            app.benchmarks.push(("Diffuse Fix Seams", elapsed));
        }

        if BLOCK_FILL {
            let start = Instant::now();
            block_average_fill(&mut buffer, &mut buffer_copy, &mask, resolution);
            let elapsed = start.elapsed().as_secs_f32();
            app.benchmarks.push(("Diffuse Block Fill", elapsed));
        }

        use exr::prelude::*;
        use half::f16;

        let full_path = app
            .output_dir
            .join(format!("Lightmap-{}_Diffuse.exr", group_index));

        let start = Instant::now();
        let channels = SpecificChannels::rgb(|pos: Vec2<usize>| {
            let x = pos.x();
            let flipped_y = resolution - 1 - pos.y();
            let index = x + flipped_y * resolution;

            (
                f16::from_f32(buffer[index].x),
                f16::from_f32(buffer[index].y),
                f16::from_f32(buffer[index].z),
            )
        });

        let layer = Layer::new(
            (resolution, resolution),
            LayerAttributes::named("diffuse"),
            Encoding::FAST_LOSSLESS,
            channels,
        );

        Image::from_layer(layer)
            .write()
            .to_file(&full_path)
            .unwrap();

        write_unity_texture_meta(MetaType::Diffuse, &full_path, &group.diffuse_guid);

        let elapsed = start.elapsed().as_secs_f32();
        app.benchmarks.push(("Diffuse Save", elapsed));

        if app.config.lightmap_mode == LightmapMode::DominantDirection {
            let tangent_space = false;
            decompact_directional(
                &mut buffer,
                &texels,
                &readback_buffer_slice,
                &app.gpu_mesh.merged_mesh,
                &group,
                tangent_space,
            );

            if settings.denoise {
                // fix denoiser artifacts on edges
                if settings.dilate {
                    dilate_texture(&mut buffer, &mut mask, resolution, DILATE_DENOISE);
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

            render_normals(&mut buffer_copy, &texels, &app.gpu_mesh.merged_mesh, &group);

            if settings.dilate {
                dilate_texture(&mut buffer, &mut mask, resolution, DILATE_FINAL);
                dilate_texture(&mut buffer_copy, &mut mask, resolution, DILATE_FINAL);
            }

            if export_debug_textures {
                let full_path = app
                    .output_dir
                    .join(format!("Lightmap-{}_Normal.tga", group_index));

                save_tga(&full_path, resolution, resolution, &buffer_copy, false).unwrap();
            }

            encode_directional(&mut buffer, &buffer_copy, tangent_space);

            if settings.fix_seams {
                let channels = if tangent_space { 3 } else { 4 };
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

            if BLOCK_FILL {
                block_average_fill(&mut buffer, &mut buffer_copy, &mask, resolution);
            }

            let alpha = if tangent_space { false } else { true };

            let full_path = app
                .output_dir
                .join(format!("Lightmap-{}_Directional.tga", group_index));
            save_tga(&full_path, resolution, resolution, &buffer, alpha).unwrap();
            write_unity_texture_meta(MetaType::Directional, &full_path, &group.directional_guid);
        }

        if app.config.lightmap_mode == LightmapMode::CombinedSH {
            decompact_combined_sh(
                &mut buffer,
                &texels,
                &readback_buffer_slice,
                &app.gpu_mesh.merged_mesh,
                &group,
            );

            if settings.denoise {
                // fix denoiser artifacts on edges
                if settings.dilate {
                    dilate_texture(&mut buffer, &mut mask, resolution, DILATE_DENOISE);
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
                dilate_texture(&mut buffer, &mut mask, resolution, DILATE_FINAL);
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

            if BLOCK_FILL {
                block_average_fill(&mut buffer, &mut buffer_copy, &mask, resolution);
            }

            let full_path = app
                .output_dir
                .join(format!("Lightmap-{}_Directional.tga", group_index));
            save_tga(&full_path, resolution, resolution, &buffer, false).unwrap();
            write_unity_texture_meta(MetaType::Directional, &full_path, &group.directional_guid);
        }
    }
    drop(buffer_copy);
    readback_buffer.destroy(&app.vk);
    drop(readback_buffer);

    bake_shadowmasks(app, &texels, &mut buffer);

    drop(buffer);

    bake_light_probes(
        app,
        mixed_start,
        mixed_count,
        &compaction_buffer,
        &lightmaps_info,
        &lightmap_texels,
    );

    app.compacted_visibility.destroy(&app.vk);

    compaction_buffer.destroy(&app.vk);
    drop(compaction_buffer);
    lightmaps_info.destroy(&app.vk);
    drop(lightmaps_info);
    lightmap_texels.destroy(&app.vk);
    drop(lightmap_texels);

    Log::msg(&format!("Benchmarks: {:?}", app.benchmarks));
}
