use std::slice;

use ash::vk::Handle;

use crate::{
    LightmapGroup, LightmapSettings, RenderTarget, Stilb, StilbConfig,
    lights::Light,
    math::Vector3,
    mesh::{FfiMesh, Mesh},
    sh::SHProbe,
    start_bake,
};

#[unsafe(no_mangle)]
pub extern "C" fn app_new(config: StilbConfig) -> *mut Stilb {
    let app = Stilb::new(config);
    Box::into_raw(Box::new(app))
}

#[unsafe(no_mangle)]
pub extern "C" fn app_add_mesh(app: *mut Stilb, mesh: FfiMesh) {
    let app = unsafe { &mut *app };
    Mesh::append_ffi_mesh(&mut app.cpu_mesh, mesh, app.config.coordinate_system);
}

#[unsafe(no_mangle)]
pub extern "C" fn app_add_light(app: *mut Stilb, mut light: Light) {
    let app = unsafe { &mut *app };

    let system = app.config.coordinate_system;
    light.position.transform_space(system);
    light.direction.transform_space(system);

    light.direction = Vector3::ZERO - light.direction;

    // todo:
    light.shadow_radius_or_angle = light.shadow_radius_or_angle.max(0.001);

    app.cpu_lights.push(light);
}

#[unsafe(no_mangle)]
pub extern "C" fn app_add_lightmap_group(
    app: *mut Stilb,
    settings: LightmapSettings,
    albedo_pixels: *const u8,
    albedo_pixels_length: u32,
    emission_pixels: *const f32,
    emission_pixels_length: u32,
) {
    let app = unsafe { &mut *app };

    let emission_pixels =
        unsafe { slice::from_raw_parts(emission_pixels, emission_pixels_length as usize) };

    let albedo_pixels =
        unsafe { slice::from_raw_parts(albedo_pixels, albedo_pixels_length as usize) };

    let group = LightmapGroup::new(app, settings, albedo_pixels, emission_pixels);
    app.groups.push(group);
}

#[unsafe(no_mangle)]
pub extern "C" fn app_run(app: *mut Stilb) {
    let app = unsafe { &mut *app };
    start_bake(app);
}

#[unsafe(no_mangle)]
pub extern "C" fn app_add_probe(app: *mut Stilb, mut position: Vector3) {
    let app = unsafe { &mut *app };

    let system = app.config.coordinate_system;
    position.transform_space(system);

    let probe = SHProbe {
        l0: Vector3::ZERO,
        pad0: 0,
        l1x: Vector3::ZERO,
        position_x: position.x,
        l1y: Vector3::ZERO,
        position_y: position.y,
        l1z: Vector3::ZERO,
        position_z: position.z,
    };

    app.probes.push(probe);
}

#[unsafe(no_mangle)]
pub extern "C" fn app_destroy(app: *mut Stilb) {
    if !app.is_null() {
        // Take ownership back from the pointer and let Box drop it
        let mut app = unsafe { Box::from_raw(app) };

        for group in &mut app.groups {
            group.destroy(&app.vk);
        }

        if let RenderTarget::NonDirectional {
            visibility,
            diffuse,
        } = &mut app.render_target
        {
            visibility.destroy(&app.vk);
            diffuse.destroy(&app.vk);
        };

        if !app.bake_shader.pipeline.is_null() {
            app.bake_shader.destroy(&app.vk);
        }
        app.gpu_mesh.destroy(&app.vk);
        app.tlas.destroy(&app.vk);

        if !app.init_from_camera_shader.pipeline.is_null() {
            app.init_from_camera_shader.destroy(&app.vk);
        }

        if !app.gpu_lights.buffer.is_null() {
            app.gpu_lights.destroy(&app.vk);
        }

        if !app.probes_buffer.buffer.is_null() {
            app.probes_buffer.destroy(&app.vk);
            app.bake_probes_shader.destroy(&app.vk);
        }

        unsafe {
            app.vk
                .device
                .destroy_sampler(app.sampler_linear_clamp, None)
        };

        println!("App destroyed");
    }
}
