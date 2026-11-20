use glam::Vec3;
use std::{
    any::Any,
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    ptr::{null, null_mut},
    slice,
    sync::atomic::{AtomicBool, Ordering},
};

use crate::{
    Glim, LOG_CALLBACK, LightmapGroup, PreviewBackend, initialize_render,
    lights::{LIGHT_SPECTRUM_NONE, Light},
    mesh::{FfiMesh, Mesh},
    spectral::LightSpectrum,
    sh::SHProbeL2,
    skybox::Skybox,
};

#[repr(C)]
#[derive(Clone)]
pub struct GlimConfig {
    pub coordinate_system: CoordinateSystem,

    pub is_preview: bool,
    pub vulkan_validation_layers: bool,
    pub seams_debug: bool,
    pub hardware_rt: bool,
    pub throttle_preview_ms: u32,
    pub preview_settings: LightmapSettings,

    pub camera_position: Vec3,
    pub camera_forward: Vec3,

    pub log_callback: LogCallback,
    pub lightprobes_read_callback: LightprobesReadCallback,

    pub probe_samples: u32,
    pub light_falloff: LightFalloffType,

    pub light_tree: bool,

    pub direct_light_samples: u32,
    pub direct_emission_samples: u32,
    pub indirect_samples: u32,
    pub bounce_count: u32,
    pub lightmap_mode: LightmapMode,
    pub skybox_intensity: f32,
    pub indirect_intensity: f32,
    pub lightprobe_deringing: f32,
    pub emissive_multiplier: f32,

    pub ao_intensity: f32,
    pub ao_range: f32,

    pub coverage_max_samples: u32,
    pub coverage_min_threshold: f32,
    pub antialiasing: bool,
    pub leak_detection: bool,
    /// spectral products of colors (bounce albedo, tinted transparency), see spectral.rs
    pub spectral_rendering: bool,
    /// lightmaps are shaded with albedo + offset, see SpectralAlbedoOffset in spectral.slang
    pub spectral_albedo_offset: bool,
}

#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LightmapMode {
    NonDirectional = 0,
    DominantDirection = 1,
    CombinedSH = 3,
}

#[repr(u32)]
pub enum LogType {
    Info = 0,
    Error = 1,
    Progress = 2,
}

#[repr(C)]
#[derive(Clone, Copy)]
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

    pub const EMPTY: FfiString = FfiString {
        raw: "".as_ptr(),
        length: 0,
    };

    pub fn to_string(self) -> String {
        if self.raw.is_null() {
            return String::new();
        }

        unsafe {
            let slice = std::slice::from_raw_parts(self.raw, self.length as usize);
            String::from_utf8_lossy(slice).into_owned()
        }
    }
}

#[repr(C)]
pub struct Log {
    pub ty: LogType,
    pub progress: f32,
    pub message: FfiString,
}

impl Log {
    pub fn msg(message: &str) {
        let msg = Self {
            ty: LogType::Info,
            progress: -1.0,
            message: FfiString::new(message),
        };
        (unsafe { LOG_CALLBACK })(msg);
    }

    pub fn progress(progress: f32) {
        let msg = Self {
            ty: LogType::Progress,
            progress,
            message: FfiString::EMPTY,
        };
        (unsafe { LOG_CALLBACK })(msg);
    }

    pub fn error(message: &str) {
        let msg = Self {
            ty: LogType::Error,
            progress: -1.0,
            message: FfiString::new(message),
        };
        (unsafe { LOG_CALLBACK })(msg);
    }
}

pub type LogCallback = extern "C" fn(data: Log);
pub type LightprobesReadCallback = extern "C" fn(data: LightprobesReadbackData);

#[repr(C)]
pub struct LightprobesReadbackData {
    pub probes: *const SHProbeL2,
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

    pub discard: bool,
}

#[repr(u32)]
#[derive(Clone, Copy, PartialEq)]
pub enum CoordinateSystem {
    Default = 0,
    Unity = 1,
}

impl CoordinateSystem {
    #[inline]
    pub fn transform(self, v: &mut Vec3) {
        match self {
            CoordinateSystem::Default => {}
            CoordinateSystem::Unity => v.z = -v.z,
        }
    }
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
    let data = Log {
        ty: LogType::Error,
        progress: -1.0,
        message,
    };

    (log_callback)(data);
}

// Atomic bool so compiler can be happy
static CANCEL_REQUESTED: AtomicBool = AtomicBool::new(false);

pub static LIGHT_UPDATE_REQUESTED: AtomicBool = AtomicBool::new(false);

pub static LIGHT_UPDATE_READY: AtomicBool = AtomicBool::new(false);

pub fn is_cancelled() -> bool {
    CANCEL_REQUESTED.load(Ordering::Relaxed)
}

#[unsafe(no_mangle)]
pub extern "C" fn app_request_cancel() {
    CANCEL_REQUESTED.store(true, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
pub extern "C" fn app_new(config: GlimConfig, output_dir: FfiString) -> *mut Glim {
    app_new_with_backend(config, output_dir, PreviewBackend::External)
}

/// Same as `app_new` but the preview opens in its own GLFW window
#[cfg(any(test, feature = "glfw"))]
#[unsafe(no_mangle)]
pub extern "C" fn app_new_windowed(config: GlimConfig, output_dir: FfiString) -> *mut Glim {
    app_new_with_backend(config, output_dir, PreviewBackend::Window)
}

fn app_new_with_backend(
    config: GlimConfig,
    output_dir: FfiString,
    backend: PreviewBackend,
) -> *mut Glim {
    CANCEL_REQUESTED.store(false, Ordering::Relaxed);
    LIGHT_UPDATE_READY.store(false, Ordering::Relaxed);
    LIGHT_UPDATE_REQUESTED.store(false, Ordering::Relaxed);

    let result = catch_unwind(AssertUnwindSafe(|| {
        let output_dir = PathBuf::from(output_dir.to_string());
        let app = Glim::new(config.clone(), output_dir, backend);
        Box::into_raw(Box::new(app))
    }));

    match result {
        Ok(val) => val,
        Err(err) => {
            handle_unwind_error(config.log_callback, err);
            null_mut() as *mut Glim
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn app_add_mesh(app: *mut Glim, mesh: FfiMesh) {
    if app.is_null() {
        return;
    }

    let app = unsafe { &mut *app };
    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut index = 0;
        if mesh.transparent {
            index += 1;
        }
        if mesh.no_shadows {
            index += 2;
        }

        let target_mesh = &mut app.meshes[index];

        // a zero multiplier removes the mesh from the light tree and emission lookups entirely
        let emissive = mesh.emissive && mesh.emission_multiplier > 0.0;

        let mut fix_seams = mesh.fix_seams;

        if app.config.is_preview {
            fix_seams = false;
        }

        Mesh::append_ffi_mesh(
            target_mesh,
            mesh,
            app.config.coordinate_system,
            &app.groups,
            fix_seams,
            emissive,
        );
    }));

    if let Err(err) = result {
        handle_unwind_error(app.config.log_callback, err);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn app_add_light(app: *mut Glim, mut light: Light) {
    if app.is_null() {
        return;
    }

    let app = unsafe { &mut *app };

    let result = catch_unwind(AssertUnwindSafe(|| {
        let system = app.config.coordinate_system;
        system.transform(&mut light.position);
        system.transform(&mut light.direction);
        system.transform(&mut light.up);

        light.direction = Vec3::ZERO - light.direction; // ??
        light.shadow_radius_or_angle = light.shadow_radius_or_angle.max(0.0);

        // a spectrum sets the hue, the light color only its brightness (luminance)
        let spectrum_rgb = app
            .light_spectra
            .get(light.spectrum as usize)
            .and_then(|spectrum| spectrum.rgb);
        match spectrum_rgb {
            Some(rgb) => {
                let luminance = light.color.dot(Vec3::new(0.2126, 0.7152, 0.0722));
                light.color = Vec3::from(rgb) * luminance;
            }
            None => light.spectrum = LIGHT_SPECTRUM_NONE,
        }

        app.cpu_lights.push(light);
    }));

    if let Err(err) = result {
        handle_unwind_error(app.config.log_callback, err);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn app_clear_lights(app: *mut Glim) -> bool {
    if app.is_null() {
        return false;
    }

    if !LIGHT_UPDATE_READY.load(Ordering::Acquire) {
        return false;
    }

    if LIGHT_UPDATE_REQUESTED.load(Ordering::Acquire) {
        return false;
    }

    let app = unsafe { &mut *app };

    let result = catch_unwind(AssertUnwindSafe(|| {
        app.cpu_lights.clear();
    }));

    if let Err(err) = result {
        handle_unwind_error(app.config.log_callback, err);
    }

    true
}

#[unsafe(no_mangle)]
pub extern "C" fn app_upload_lights(app: *mut Glim) {
    if app.is_null() {
        return;
    }

    LIGHT_UPDATE_REQUESTED.store(true, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
pub extern "C" fn app_add_lightmap_group(
    app: *mut Glim,
    settings: LightmapSettings,
    albedo_pixels: *const u8,
    albedo_pixels_length: u32,
    emission_pixels: *const u8,
    emission_pixels_length: u32,
    diffuse_guid: FfiString,
    directional_guid: FfiString,
    shadowmask_guid: FfiString,
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

        let diffuse_guid = diffuse_guid.to_string();
        let directional_guid = directional_guid.to_string();
        let shadowmask_guid = shadowmask_guid.to_string();

        let mut group = LightmapGroup::new(
            settings,
            albedo_pixels,
            emission_pixels,
            diffuse_guid,
            directional_guid,
            shadowmask_guid,
        );
        group.id = app.groups.len() as u32;
        app.groups.push(group);
    }));

    if let Err(err) = result {
        handle_unwind_error(app.config.log_callback, err);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn app_add_light_cookie(
    app: *mut Glim,
    pixels: *const u8,
    pixels_length: u32,
    width: u32,
    height: u32,
) {
    if app.is_null() {
        return;
    }

    let app = unsafe { &mut *app };

    let result = catch_unwind(AssertUnwindSafe(|| {
        let pixels = unsafe { slice::from_raw_parts(pixels, pixels_length as usize) };
        app.add_light_cookie(pixels, width, height);
    }));

    if let Err(err) = result {
        handle_unwind_error(app.config.log_callback, err);
    }
}

/// Relative spectral power of a light, samples evenly spaced over [360, 830] nm.
/// Lights reference it by the order it was added in (Light::spectrum), add spectra before lights.
#[unsafe(no_mangle)]
pub extern "C" fn app_add_light_spectrum(app: *mut Glim, samples: *const f32, samples_length: u32) {
    if app.is_null() || samples.is_null() {
        return;
    }

    let app = unsafe { &mut *app };

    let result = catch_unwind(AssertUnwindSafe(|| {
        let samples = unsafe { slice::from_raw_parts(samples, samples_length as usize) };
        app.light_spectra.push(LightSpectrum::from_samples(samples));
    }));

    if let Err(err) = result {
        handle_unwind_error(app.config.log_callback, err);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn app_add_alpha_texture(
    app: *mut Glim,
    pixels: *const u8,
    pixels_length: u32,
    width: u32,
    height: u32,
) {
    if app.is_null() {
        return;
    }

    let app = unsafe { &mut *app };

    let result = catch_unwind(AssertUnwindSafe(|| {
        let pixels = unsafe { slice::from_raw_parts(pixels, pixels_length as usize) };
        app.add_alpha_texture(pixels, width, height);
    }));

    if let Err(err) = result {
        handle_unwind_error(app.config.log_callback, err);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn app_run(app: *mut Glim) {
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
pub extern "C" fn app_add_probe(app: *mut Glim, mut position: Vec3, radius: f32) {
    if app.is_null() {
        return;
    }

    let app = unsafe { &mut *app };

    let result = catch_unwind(AssertUnwindSafe(|| {
        let system = app.config.coordinate_system;
        system.transform(&mut position);

        let probe = SHProbeL2 {
            position,
            radius,
            ..Default::default()
        };

        app.probes.push(probe);
    }));

    if let Err(err) = result {
        handle_unwind_error(app.config.log_callback, err);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn app_set_skybox(
    app: *mut Glim,
    pixels: *const f32,
    pixels_length: u32,
    width: u32,
    height: u32,
) {
    if app.is_null() {
        return;
    }

    let app = unsafe { &mut *app };

    let result = catch_unwind(AssertUnwindSafe(|| {
        let pixels = unsafe { slice::from_raw_parts(pixels, pixels_length as usize) };
        app.skybox = Skybox::new(&app.vk, width, height, pixels);
    }));

    if let Err(err) = result {
        handle_unwind_error(app.config.log_callback, err);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn app_destroy(app: *mut Glim) {
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
