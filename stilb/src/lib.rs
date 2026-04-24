use std::ptr;

use ash::vk::{self, Handle};

use glfw_sys::{GLFWwindow, glfwCreateWindowSurface};

use crate::{
    bmp::save_bmp,
    compute_shader::{
        BakePushConstants, ComputeShader, load_bake_lights_shader, update_bake_lights_shader,
    },
    graphics_shader::{VisibilityPushConstants, create_visibility_shader},
    lights::Light,
    mesh::{FfiMesh, GpuMesh, Mesh, VulkanAs, create_tlas},
    texture2d::Texture2D,
    vulkan_core::{VulkanConfig, VulkanContext},
    window::{initialize_window, platform_loop},
};

mod bmp;
mod compute_shader;
mod graphics_shader;
mod lights;
mod math;
mod mesh;
mod test;
mod texture2d;
mod vulkan_cmd;
mod vulkan_core;
mod vulkan_swapchain;
mod window;

pub struct Stilb {
    pub config: StilbConfig,
    pub vk: VulkanContext,
    pub window: *mut GLFWwindow,

    pub groups: Vec<LightmapSettings>,
    pub cpu_meshes: Vec<Mesh>,
    pub cpu_lights: Vec<Light>,

    pub gpu_mesh: GpuMesh,
    pub tlas: VulkanAs,

    pub bake_lights_shader: ComputeShader,
    // pub bake_init_shader: ComputeShader,
}

pub struct LightmapSettings {
    pub width: u32,
    pub height: u32,

    pub max_samples: u32,
    pub bounces: u32,

    pub denoise: bool,
}

pub struct LightmapGroup {
    pub settings: LightmapSettings,

    pub albedo: Texture2D,
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

pub fn as_bytes<T>(v: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v as *const T as *const u8, std::mem::size_of::<T>()) }
}

pub fn blit_with_shader(vk: &VulkanContext, cmd: vk::CommandBuffer, image: vk::ImageView) {

    // vk.device.bindre
    // transition to general
}

fn start_preview_bake(app: &mut Stilb) {}

fn rasterize_visibility(
    vk: &mut VulkanContext,
    mesh: &GpuMesh,
    width: u32,
    height: u32,
) -> Texture2D {
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

// fn initialize_rays(app: &mut Stilb) {}

fn start_headless_bake(app: &mut Stilb) {
    assert!(app.cpu_meshes.len() > 0);

    app.gpu_mesh = GpuMesh::new(&app.vk, &app.cpu_meshes[0]);

    let settings = LightmapSettings {
        width: 512,
        height: 512,
        bounces: 2,
        max_samples: 256,
        denoise: false,
    };

    let mesh::AccelerationStructureType::RayQuery(blas) = &app.gpu_mesh.acceleration_structure
    else {
        panic!("Expected RayQuery variant");
    };

    app.tlas = create_tlas(&app.vk, blas);

    let visibility =
        rasterize_visibility(&mut app.vk, &app.gpu_mesh, settings.width, settings.height);

    let albedo = Texture2D::new(
        &app.vk,
        settings.width,
        settings.height,
        vk::Format::R32G32B32A32_SFLOAT,
        vk::ImageUsageFlags::SAMPLED
            | vk::ImageUsageFlags::TRANSFER_SRC
            | vk::ImageUsageFlags::TRANSFER_DST,
    );

    let diffuse_lightmap = Texture2D::new(
        &app.vk,
        settings.width,
        settings.height,
        vk::Format::R32G32B32A32_SFLOAT,
        vk::ImageUsageFlags::STORAGE
            | vk::ImageUsageFlags::TRANSFER_SRC
            | vk::ImageUsageFlags::TRANSFER_DST,
    );

    let mut group = LightmapGroup {
        settings,
        visibility,
        albedo,
        diffuse_lightmap,
    };

    update_bake_lights_shader(
        &app.vk,
        &app.bake_lights_shader,
        app.tlas.acceleration_structure(),
        &group.visibility,
        &group.albedo,
        &group.diffuse_lightmap,
    );

    let vk = &app.vk;
    let cmd = vk.begin_single_use_cmd();

    let push = BakePushConstants {
        vertices: app.gpu_mesh.vertex_address(),
        indices: app.gpu_mesh.index_address(),
        lights: 0,
        lights_count: 0,
        pad0: 0,
        sampled_index: 0,
        width: group.settings.width,
        height: group.settings.height,
        pad1: 0,
    };

    let constants_bytes = as_bytes(&push);

    unsafe {
        let barrier = group.diffuse_lightmap.barrier(
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

        vk.device.cmd_bind_pipeline(
            cmd,
            vk::PipelineBindPoint::COMPUTE,
            app.bake_lights_shader.pipeline,
        );

        vk.device.cmd_bind_descriptor_sets(
            cmd,
            vk::PipelineBindPoint::COMPUTE,
            app.bake_lights_shader.pipeline_layout,
            0,
            &[app.bake_lights_shader.descriptor_set],
            &[],
        );

        vk.device.cmd_push_constants(
            cmd,
            app.bake_lights_shader.pipeline_layout,
            vk::ShaderStageFlags::COMPUTE,
            0,
            &constants_bytes,
        );

        let groups_x = (group.diffuse_lightmap.width() + 7) / 8;
        let groups_y = (group.diffuse_lightmap.height() + 7) / 8;
        vk.device.cmd_dispatch(cmd, groups_x, groups_y, 1);
    }

    vk.end_single_use_cmd(cmd);

    let pixels_read = group.diffuse_lightmap.read_pixels(&app.vk);
    save_bmp(
        "../temp/diffuse_lightmap.bmp",
        group.diffuse_lightmap.width(),
        group.diffuse_lightmap.height(),
        &pixels_read,
    )
    .unwrap();

    destroy_group(&app.vk, &mut group);
}

fn destroy_group(vk: &VulkanContext, group: &mut LightmapGroup) {
    group.albedo.destroy(vk);
    group.diffuse_lightmap.destroy(vk);
    group.visibility.destroy(vk);
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

    let bake_lights_shader = load_bake_lights_shader(&vk);

    let stilb = Stilb {
        vk,
        cpu_meshes: Vec::new(),
        window: window,
        config: app_config,
        cpu_lights: Vec::new(),
        bake_lights_shader,
        gpu_mesh: GpuMesh::null(),
        tlas: VulkanAs::null(),
        groups: Vec::new(),
    };

    Box::into_raw(Box::new(stilb))
}

#[unsafe(no_mangle)]
pub extern "C" fn add_mesh(stilb: *mut Stilb, raw: FfiMesh) {
    unsafe {
        let stilb_obj = &mut *stilb;
        let mesh = Mesh::from_ffi_mesh(raw);
        // println!("Added mesh: {:#?}", mesh);
        stilb_obj.cpu_meshes.push(mesh);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn app_run(stilb: *mut Stilb) {
    let app = unsafe { &mut *stilb };

    if app.config.is_preview {
        platform_loop(app.window);
    } else {
        start_headless_bake(app);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn app_deinitialize(stilb: *mut Stilb) {
    if !stilb.is_null() {
        // Take ownership back from the pointer and let Box drop it
        let mut stilb = unsafe { Box::from_raw(stilb) };

        stilb.bake_lights_shader.destroy(&stilb.vk);
        stilb.gpu_mesh.destroy(&stilb.vk);
        stilb.tlas.destroy(&stilb.vk);

        println!("Stilb destroyed");
    }
}
