use std::{
    any::Any,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::{null, null_mut},
    slice,
};

use crate::{
    LightmapGroup, Stilb, initialize_render,
    lights::Light,
    math::Vector3,
    mesh::{FfiMesh, Mesh},
    sh::SHProbe,
};

#[repr(C)]
#[derive(Clone)]
pub struct StilbConfig {
    pub coordinate_system: CoordinateSystem,

    pub is_preview: bool,
    pub vulkan_validation_layers: bool,
    pub seams_debug: bool,
    pub throttle_preview_ms: u32,
    pub preview_settings: LightmapSettings,

    pub camera_position: Vector3,
    pub camera_forward: Vector3,

    pub log_callback: LogCallback,
    pub lightmap_read_callback: LightmapReadCallback,
    pub lightprobes_read_callback: LightprobesReadCallback,

    pub texture_filter: TextureSamplerFilter,
    pub probe_samples: u32,
    pub probe_bounces: u32,
    pub light_falloff: LightFalloffType,

    pub mis: bool,

    pub direct_samples: u32,
    pub indirect_samples: u32,
    pub bounce_count: u32,
}

#[repr(u32)]
pub enum LogMessageType {
    Success = 0,
    Error = 1,
    Progress = 2,
}

#[repr(C)]
pub struct FfiString {
    pub raw: *const u8,
    pub length: u32,
}

impl FfiString {
    pub fn new(value: &str) -> Self {
        Self {
            raw: value.as_ptr(),
            length: value.len() as u32,
        }
    }

    pub fn null() -> Self {
        Self {
            raw: null(),
            length: 0,
        }
    }

    pub fn from(self) -> &'static str {
        if self.raw.is_null() {
            return "";
        }
        unsafe {
            let slice = std::slice::from_raw_parts(self.raw, self.length as usize);
            std::str::from_utf8_unchecked(slice)
        }
    }
}

#[repr(C)]
pub struct LogMessage {
    pub ty: LogMessageType,
    pub progress: f32,
    pub message: FfiString,
}

impl LogMessage {
    pub fn message(message: &str) -> Self {
        Self {
            ty: LogMessageType::Success,
            progress: -1.0,
            message: FfiString::new(message),
        }
    }

    pub fn progress(message: &str, progress: f32) -> Self {
        Self {
            ty: LogMessageType::Progress,
            progress,
            message: FfiString::new(message),
        }
    }

    pub fn error(message: &str) -> Self {
        Self {
            ty: LogMessageType::Error,
            progress: -1.0,
            message: FfiString::new(message),
        }
    }
}

type LogCallback = extern "C" fn(data: LogMessage);
type LightmapReadCallback = extern "C" fn(data: LightmapReadbackData);
type LightprobesReadCallback = extern "C" fn(data: LightprobesReadbackData);

#[repr(C)]
pub struct LightmapReadbackData {
    pub group_index: u32,
    pub ty: u32,
    pub width: u32,
    pub height: u32,
    pub pixels: *const f32,
    pub pixels_count: u32,
}

#[repr(C)]
pub struct LightprobesReadbackData {
    pub probes: *const SHProbe,
    pub pixels_count: u32,
}

#[repr(C)]
#[derive(Clone, Debug)]
pub struct LightmapSettings {
    pub width: u32,
    pub height: u32,

    pub dilate: bool,
    pub denoise: bool,
    pub fix_seams: bool,
}

#[repr(u32)]
#[derive(Clone, Copy, PartialEq)]
pub enum CoordinateSystem {
    Default = 0,
    Unity = 1,
}

#[repr(u32)]
#[derive(Clone, Copy, PartialEq)]
pub enum TextureSamplerFilter {
    Nearest = 0,
    Linear = 1,
}

#[repr(u32)]
#[derive(Clone, Copy, PartialEq)]
pub enum LightFalloffType {
    InverseSquare = 0,
    UnityBuiltIn = 1,
}

fn handle_unwind_error(log_callback: LogCallback, err: Box<dyn Any + Send>) {
    let error_msg: &str = if let Some(s) = err.downcast_ref::<&str>() {
        *s
    } else if let Some(s) = err.downcast_ref::<String>() {
        s.as_str()
    } else {
        "Unknown panic payload type"
    };

    let message = FfiString::new(error_msg);
    let data = LogMessage {
        ty: LogMessageType::Error,
        progress: -1.0,
        message,
    };

    (log_callback)(data);
}

#[unsafe(no_mangle)]
pub extern "C" fn app_new(config: StilbConfig) -> *mut Stilb {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let app = Stilb::new(config.clone());
        Box::into_raw(Box::new(app))
    }));

    match result {
        Ok(val) => val,
        Err(err) => {
            handle_unwind_error(config.log_callback, err);
            null_mut() as *mut Stilb
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn app_add_mesh(app: *mut Stilb, mesh: FfiMesh) {
    if app.is_null() {
        return;
    }

    let app = unsafe { &mut *app };
    let result = catch_unwind(AssertUnwindSafe(|| {
        let target_mesh = if mesh.transparent {
            &mut app.transparent_mesh
        } else {
            &mut app.opaque_mesh
        };

        Mesh::append_ffi_mesh(
            target_mesh,
            mesh,
            app.config.coordinate_system,
            &mut app.seams,
            // todo add seams per renderer
            !app.config.is_preview,
        );
    }));

    if let Err(err) = result {
        handle_unwind_error(app.config.log_callback, err);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn app_add_light(app: *mut Stilb, mut light: Light) {
    if app.is_null() {
        return;
    }

    let app = unsafe { &mut *app };

    let result = catch_unwind(AssertUnwindSafe(|| {
        let system = app.config.coordinate_system;
        light.position.transform_space(system);
        light.direction.transform_space(system);

        light.direction = Vector3::ZERO - light.direction;

        // todo:
        light.shadow_radius_or_angle = light.shadow_radius_or_angle.max(0.001);

        app.cpu_lights.push(light);
    }));

    if let Err(err) = result {
        handle_unwind_error(app.config.log_callback, err);
    }
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
    if app.is_null() {
        return;
    }

    let app = unsafe { &mut *app };

    let result = catch_unwind(AssertUnwindSafe(|| {
        let emission_pixels =
            unsafe { slice::from_raw_parts(emission_pixels, emission_pixels_length as usize) };

        let albedo_pixels =
            unsafe { slice::from_raw_parts(albedo_pixels, albedo_pixels_length as usize) };

        let index = app.groups.len() as u32;
        let group = LightmapGroup::new(app, settings, albedo_pixels, emission_pixels, index);
        app.groups.push(group);
    }));

    if let Err(err) = result {
        handle_unwind_error(app.config.log_callback, err);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn app_run(app: *mut Stilb) {
    if app.is_null() {
        return;
    }

    let app = unsafe { &mut *app };

    let result = catch_unwind(AssertUnwindSafe(|| {
        initialize_render(app);
    }));

    if let Err(err) = result {
        handle_unwind_error(app.config.log_callback, err);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn app_add_probe(app: *mut Stilb, mut position: Vector3) {
    if app.is_null() {
        return;
    }

    let app = unsafe { &mut *app };

    let result = catch_unwind(AssertUnwindSafe(|| {
        let system = app.config.coordinate_system;
        position.transform_space(system);

        let probe = SHProbe {
            position,
            pad0: 0,
            l0: Vector3::ZERO,
            pad1: 0,
            l1_1: Vector3::ZERO,
            pad2: 0,
            l10: Vector3::ZERO,
            pad3: 0,
            l11: Vector3::ZERO,
            pad4: 0,
            l2_2: Vector3::ZERO,
            pad5: 0,
            l2_1: Vector3::ZERO,
            pad6: 0,
            l20: Vector3::ZERO,
            pad7: 0,
            l21: Vector3::ZERO,
            pad8: 0,
            l22: Vector3::ZERO,
            pad9: 0,
        };

        app.probes.push(probe);
    }));

    if let Err(err) = result {
        handle_unwind_error(app.config.log_callback, err);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn app_destroy(app: *mut Stilb) {
    if app.is_null() {
        return;
    }

    let app = unsafe { &mut *app };

    let callback = app.config.log_callback;

    let result = catch_unwind(AssertUnwindSafe(|| {
        // Take ownership back from the pointer and let Box drop it
        let mut _app = unsafe { Box::from_raw(app) };
    }));

    if let Err(err) = result {
        handle_unwind_error(callback, err);
    }
}
