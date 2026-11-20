// Neural lightmaps, see shaders/includes/neural.slang
//
// After the regular bake, a small MLP shared by the scene and NN_FEATURES latent features per lightmap pixel
// are trained on the GPU to output the prefiltered incoming radiance for a direction and lobe width.
// Targets are fresh noisy ray traced estimates every step.
//
// Outputs per lightmap group: Lightmap-{g}_Neural{0..3}.exr, 16 features as 4 RGBA half textures
// and NeuralWeights.exr for the whole scene, NN_WEIGHT_COUNT floats as RGBA float, 64 texels per row.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::time::Instant;

use ash::vk;

use crate::bindings::{Log, is_cancelled};
use crate::buffer::Buffer;
use crate::raster::Texels;
use crate::shader::{RayTracingData, ShaderBindings, ShaderName, load_compute_shader, update_compute_shader};
use crate::{Glim, as_bytes, zero_buffer};

pub const NN_FEATURES: usize = 16;
pub const NN_DIR_ENCODING: usize = 25;
pub const NN_LOBE_ENCODING: usize = 7;
pub const NN_IN: usize = NN_FEATURES + NN_DIR_ENCODING + NN_LOBE_ENCODING;
pub const NN_HIDDEN: usize = 64;
pub const NN_OUT: usize = 4;

pub const NN_W1: usize = 0;
pub const NN_B1: usize = NN_W1 + NN_HIDDEN * NN_IN;
pub const NN_W2: usize = NN_B1 + NN_HIDDEN;
pub const NN_B2: usize = NN_W2 + NN_HIDDEN * NN_HIDDEN;
pub const NN_W3: usize = NN_B2 + NN_HIDDEN;
pub const NN_B3: usize = NN_W3 + NN_OUT * NN_HIDDEN;
pub const NN_WEIGHT_COUNT: usize = NN_B3 + NN_OUT;

pub const NN_BATCH_STRIDE: usize = NN_IN + 4 * NN_HIDDEN + NN_OUT;

pub const NN_WEIGHTS_TEXTURE_WIDTH: usize = 64;

const _: () = assert!(NN_IN == 48 && NN_WEIGHT_COUNT == 7556 && NN_BATCH_STRIDE == 308);

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct NeuralPixel {
    pub texel_start: u32,
    pub texel_count: u32,
    pub group: u32,
    pub index: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NeuralTexel {
    index: u32,
    coverage: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NeuralData {
    weights: vk::DeviceAddress,
    weight_m: vk::DeviceAddress,
    weight_v: vk::DeviceAddress,
    partials: vk::DeviceAddress,

    features: vk::DeviceAddress,
    feature_m: vk::DeviceAddress,
    feature_v: vk::DeviceAddress,
    visits: vk::DeviceAddress,

    batch: vk::DeviceAddress,
    loss: vk::DeviceAddress,
    eval: vk::DeviceAddress,
    lightmap: vk::DeviceAddress,

    pixels: vk::DeviceAddress,
    texels: vk::DeviceAddress,
    order: vk::DeviceAddress,
    lights: vk::DeviceAddress,

    pixel_count: u32,
    light_count: u32,
    mixed_start: u32,
    mixed_end: u32,
}

pub struct NeuralSettings {
    pub steps: u32,
    pub batch_size: u32,
    pub target_rays: u32,
    pub weights_lr: f32,
    pub features_lr: f32,
    pub reference_rays: u32,
    pub debug_sharpness_t: f32,
}

fn env_or<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

impl NeuralSettings {
    /// defaults, overridable with environment variables for experiments
    pub fn from_env() -> Self {
        Self {
            steps: env_or("GLIM_NN_STEPS", 8000),
            batch_size: env_or("GLIM_NN_BATCH", 65536),
            target_rays: env_or("GLIM_NN_TARGET_RAYS", 4),
            weights_lr: env_or("GLIM_NN_LR", 2e-3),
            features_lr: env_or("GLIM_NN_FEATURE_LR", 1e-2),
            reference_rays: env_or("GLIM_NN_REFERENCE_RAYS", 256),
            debug_sharpness_t: env_or("GLIM_NN_DEBUG_T", 0.4),
        }
    }
}

pub struct NeuralResult {
    pub pixels: Vec<NeuralPixel>,
    pub features: Vec<f32>,
    pub weights: Vec<f32>,
    pub eval: Vec<f32>,
}

struct Rng(u64);

impl Rng {
    fn next_u32(&mut self) -> u32 {
        // splitmix64
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        ((z ^ (z >> 31)) >> 32) as u32
    }

    fn uniform(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / 16777216.0
    }

    fn normal(&mut self) -> f32 {
        let u1 = self.uniform().max(1e-7);
        let u2 = self.uniform();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f32::consts::PI * u2).cos()
    }
}

fn device_buffer<T>(app: &Glim, name: &str, data: &[T]) -> Buffer {
    Buffer::new(
        &app.vk,
        name.to_owned(),
        data,
        vk::BufferUsageFlags::TRANSFER_DST
            | vk::BufferUsageFlags::TRANSFER_SRC
            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    )
}

fn empty_buffer(app: &Glim, name: &str, bytes: usize, host: bool) -> Buffer {
    let properties = if host {
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT
    } else {
        vk::MemoryPropertyFlags::DEVICE_LOCAL
    };
    let buffer = Buffer::empty(
        &app.vk,
        name.to_owned(),
        bytes.max(16) as u64,
        vk::BufferUsageFlags::TRANSFER_DST
            | vk::BufferUsageFlags::TRANSFER_SRC
            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
        properties,
    );
    zero_buffer(&app.vk, &buffer);
    buffer
}

fn read_buffer<T: Copy + Default>(app: &Glim, buffer: &Buffer, count: usize) -> Vec<T> {
    let bytes = (count * size_of::<T>()).max(16);
    let mut staging = empty_buffer(app, "Neural Readback", bytes, true);

    let cmd = app.vk.begin_single_use_cmd();
    unsafe {
        app.vk.device.cmd_copy_buffer(
            cmd,
            buffer.buffer,
            staging.buffer,
            &[vk::BufferCopy {
                src_offset: 0,
                dst_offset: 0,
                size: (count * size_of::<T>()) as u64,
            }],
        );
    }
    app.vk.end_single_use_cmd(cmd);

    let mut out = vec![T::default(); count];
    unsafe {
        std::ptr::copy_nonoverlapping(staging.ptr as *const T, out.as_mut_ptr(), count);
    }
    staging.destroy(&app.vk);
    out
}

fn compute_barrier(app: &Glim, cmd: vk::CommandBuffer) {
    let barrier = vk::MemoryBarrier {
        src_access_mask: vk::AccessFlags::SHADER_WRITE,
        dst_access_mask: vk::AccessFlags::SHADER_READ | vk::AccessFlags::SHADER_WRITE,
        ..Default::default()
    };
    unsafe {
        app.vk.device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::DependencyFlags::empty(),
            &[barrier],
            &[],
            &[],
        );
    }
}

fn dispatch<P>(
    app: &Glim,
    cmd: vk::CommandBuffer,
    shader: &crate::compute_shader::ComputeShader,
    push: &P,
    threads: usize,
) {
    let vk = &app.vk.device;
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
            as_bytes(push),
        );
        vk.cmd_dispatch(cmd, threads.div_ceil(64) as u32, 1, 1);
    }
}

/// pixels of all groups that have front face texels, with the texels covering them
fn build_pixels(app: &Glim, texels: &Texels) -> (Vec<NeuralPixel>, Vec<NeuralTexel>) {
    let mesh = &app.gpu_mesh.merged_mesh;

    let mut group_maps: Vec<Vec<u32>> = app
        .groups
        .iter()
        .map(|g| vec![u32::MAX; (g.settings.width * g.settings.width) as usize])
        .collect();

    let mut pixels: Vec<NeuralPixel> = Vec::new();
    let mut texel_pixel = vec![u32::MAX; texels.front_count];

    for compacted_index in 0..texels.front_count {
        let texel = &texels.visibility[compacted_index];
        if texel.primitive_id == u32::MAX {
            continue;
        }
        let i0 = mesh.indices[texel.primitive_id as usize * 3] as usize;
        let group = mesh.vertices[i0].group() as usize;
        if group >= app.groups.len() || app.groups[group].settings.discard {
            continue;
        }
        let resolution = app.groups[group].settings.width as usize;
        let index = texel.index(resolution);

        let map = &mut group_maps[group];
        if map[index] == u32::MAX {
            map[index] = pixels.len() as u32;
            pixels.push(NeuralPixel {
                texel_start: 0,
                texel_count: 0,
                group: group as u32,
                index: index as u32,
            });
        }
        let pixel = map[index];
        pixels[pixel as usize].texel_count += 1;
        texel_pixel[compacted_index] = pixel;
    }

    let mut start = 0;
    for p in pixels.iter_mut() {
        p.texel_start = start;
        start += p.texel_count;
        p.texel_count = 0;
    }

    let mut list = vec![NeuralTexel::default(); start as usize];
    for compacted_index in 0..texels.front_count {
        let pixel = texel_pixel[compacted_index];
        if pixel == u32::MAX {
            continue;
        }
        let p = &mut pixels[pixel as usize];
        list[(p.texel_start + p.texel_count) as usize] = NeuralTexel {
            index: compacted_index as u32,
            coverage: texels.coverage[compacted_index].max(1e-6),
        };
        p.texel_count += 1;
    }

    (pixels, list)
}

fn init_weights(rng: &mut Rng, mean_radiance: f32) -> Vec<f32> {
    let mut w = vec![0.0f32; NN_WEIGHT_COUNT];

    let std1 = (2.0 / NN_IN as f32).sqrt();
    for i in 0..NN_HIDDEN * NN_IN {
        w[NN_W1 + i] = rng.normal() * std1;
    }
    let std2 = (2.0 / NN_HIDDEN as f32).sqrt();
    for i in 0..NN_HIDDEN * NN_HIDDEN {
        w[NN_W2 + i] = rng.normal() * std2;
    }
    // small output layer, starts close to the average radiance of the scene
    for c in 0..3 {
        for j in 0..NN_HIDDEN {
            w[NN_W3 + c * NN_HIDDEN + j] = rng.normal() * 0.01;
        }
        w[NN_B3 + c] = mean_radiance.max(1e-3).ln();
    }
    w
}

pub fn train_neural(
    app: &mut Glim,
    texels: &Texels,
    lightmap_texels: &Buffer,
    lightmap_readback: &[f32],
    compaction: &Buffer,
    lightmaps_info: &Buffer,
    mixed_start: usize,
    mixed_count: usize,
) -> Option<NeuralResult> {
    let settings = NeuralSettings::from_env();
    let start = Instant::now();

    let (pixels, pixel_texels) = build_pixels(app, texels);
    let pixel_count = pixels.len();
    if pixel_count == 0 {
        return None;
    }

    let mut rng = Rng(0x5EED_1234_ABCD);

    // average diffuse, initializes the output bias
    let mut mean = 0.0f64;
    let mut n = 0usize;
    for i in 0..texels.front_count {
        let v = (lightmap_readback[i * 3] + lightmap_readback[i * 3 + 1] + lightmap_readback[i * 3 + 2]) / 3.0;
        if v.is_finite() {
            mean += v as f64;
            n += 1;
        }
    }
    let mean = if n > 0 { (mean / n as f64) as f32 } else { 0.1 };

    let weights = init_weights(&mut rng, mean);
    let features: Vec<f32> = (0..pixel_count * NN_FEATURES)
        .map(|_| (rng.uniform() * 2.0 - 1.0) * 0.1)
        .collect();

    let mut order: Vec<u32> = (0..pixel_count as u32).collect();
    for i in (1..order.len()).rev() {
        let j = (rng.next_u32() as usize) % (i + 1);
        order.swap(i, j);
    }

    let batch_size = (settings.batch_size as usize).min(pixel_count).max(1);
    const CHUNK_SIZE: usize = 256;
    let chunk_count = batch_size.div_ceil(CHUNK_SIZE);

    Log::msg(&format!(
        "Neural: {} pixels, {} weights, batch {}, {} steps, {} target rays",
        pixel_count, NN_WEIGHT_COUNT, batch_size, settings.steps, settings.target_rays
    ));

    let f32s = size_of::<f32>();
    let mut b_weights = device_buffer(app, "Neural Weights", &weights);
    let mut b_weight_m = empty_buffer(app, "Neural Weights M", NN_WEIGHT_COUNT * f32s, false);
    let mut b_weight_v = empty_buffer(app, "Neural Weights V", NN_WEIGHT_COUNT * f32s, false);
    let mut b_partials = empty_buffer(app, "Neural Partials", chunk_count * NN_WEIGHT_COUNT * f32s, false);
    let mut b_features = device_buffer(app, "Neural Features", &features);
    let mut b_feature_m = empty_buffer(app, "Neural Features M", features.len() * f32s, false);
    let mut b_feature_v = empty_buffer(app, "Neural Features V", features.len() * f32s, false);
    let mut b_visits = empty_buffer(app, "Neural Visits", pixel_count * 4, false);
    let mut b_batch = empty_buffer(app, "Neural Batch", batch_size * NN_BATCH_STRIDE * f32s, false);
    let mut b_loss = empty_buffer(app, "Neural Loss", batch_size * f32s, true);
    let mut b_eval = empty_buffer(app, "Neural Eval", pixel_count * 16 * f32s, false);
    let mut b_pixels = device_buffer(app, "Neural Pixels", &pixels);
    let mut b_texels = device_buffer(app, "Neural Texels", &pixel_texels);
    let mut b_order = device_buffer(app, "Neural Order", &order);

    let data = NeuralData {
        weights: b_weights.gpu_address,
        weight_m: b_weight_m.gpu_address,
        weight_v: b_weight_v.gpu_address,
        partials: b_partials.gpu_address,
        features: b_features.gpu_address,
        feature_m: b_feature_m.gpu_address,
        feature_v: b_feature_v.gpu_address,
        visits: b_visits.gpu_address,
        batch: b_batch.gpu_address,
        loss: b_loss.gpu_address,
        eval: b_eval.gpu_address,
        lightmap: lightmap_texels.gpu_address,
        pixels: b_pixels.gpu_address,
        texels: b_texels.gpu_address,
        order: b_order.gpu_address,
        lights: app.gpu_lights.gpu_address,
        pixel_count: pixel_count as u32,
        light_count: app.cpu_lights.len() as u32,
        mixed_start: mixed_start as u32,
        mixed_end: (mixed_start + mixed_count) as u32,
    };
    let mut b_data = device_buffer(app, "Neural Data", std::slice::from_ref(&data));

    #[repr(C, align(16))]
    struct TrainPush {
        rt: RayTracingData,
        compaction: vk::DeviceAddress,
        lightmaps_info: vk::DeviceAddress,
        data: vk::DeviceAddress,
        step: u32,
        batch_offset: u32,
        batch_size: u32,
        target_rays: u32,
        feature_lr: f32,
        loss_scale: f32,
    }

    #[repr(C, align(16))]
    struct GradPush {
        batch: vk::DeviceAddress,
        partials: vk::DeviceAddress,
        batch_size: u32,
        chunk_size: u32,
        chunk_count: u32,
    }

    #[repr(C, align(16))]
    struct AdamPush {
        weights: vk::DeviceAddress,
        weight_m: vk::DeviceAddress,
        weight_v: vk::DeviceAddress,
        partials: vk::DeviceAddress,
        chunk_count: u32,
        batch_size: u32,
        learning_rate: f32,
        step: f32,
        weight_decay: f32,
    }

    #[repr(C, align(16))]
    struct EvalPush {
        rt: RayTracingData,
        compaction: vk::DeviceAddress,
        lightmaps_info: vk::DeviceAddress,
        data: vk::DeviceAddress,
        reference_rays: u32,
        debug_sharpness_t: f32,
        pixel_offset: u32,
        pixel_count: u32,
    }

    let albedos = app.albedo_array.views();
    let emissions = app.emission_array.views();
    let cookies = app.cookie_array.views();
    let alphas = app.alphas_array.views();
    let bindings = ShaderBindings {
        tlas: app.tlas.acceleration_structure,
        albedos: &albedos,
        emissions: &emissions,
        skybox: app.skybox.view(),
        skybox_sampler: app.skybox.sampler(),
        visibility: vk::ImageView::null(),
        preview_diffuse: vk::ImageView::null(),
        cookies: &cookies,
        alphas: &alphas,
        linear_sampler: app.linear_sampler,
    };

    let load = |name: ShaderName, push_size: usize| {
        let shader = load_compute_shader(&app.vk, name, &app.constants, &bindings, push_size);
        update_compute_shader(&app.vk, &shader, &bindings);
        shader
    };
    let mut train_shader = load(ShaderName::NeuralTrain, size_of::<TrainPush>());
    let mut grad_shader = load(ShaderName::NeuralGrad, size_of::<GradPush>());
    let mut adam_shader = load(ShaderName::NeuralAdam, size_of::<AdamPush>());
    let mut eval_shader = load(ShaderName::NeuralEval, size_of::<EvalPush>());

    let rt = app.rt_data();
    let make_rt = || RayTracingData { ..rt };

    const STEPS_PER_SUBMIT: u32 = 4;
    const WARMUP: u32 = 200;
    const LOG_EVERY: u32 = 250;

    let lr_at = |base: f32, step: u32| -> f32 {
        let warmup = ((step + 1) as f32 / WARMUP as f32).min(1.0);
        let progress = step as f32 / settings.steps.max(1) as f32;
        let cosine = 0.5 * (1.0 + (std::f32::consts::PI * progress).cos());
        base * warmup * (0.02 + 0.98 * cosine)
    };

    let mut step = 0u32;
    let mut loss_history = Vec::new();
    while step < settings.steps {
        if is_cancelled() {
            break;
        }

        let cmd = app.vk.begin_single_use_cmd();
        let last = (step + STEPS_PER_SUBMIT).min(settings.steps);
        for s in step..last {
            let train = TrainPush {
                rt: make_rt(),
                compaction: compaction.gpu_address,
                lightmaps_info: lightmaps_info.gpu_address,
                data: b_data.gpu_address,
                step: s,
                batch_offset: ((s as u64 * batch_size as u64) % pixel_count as u64) as u32,
                batch_size: batch_size as u32,
                target_rays: settings.target_rays,
                feature_lr: lr_at(settings.features_lr, s),
                loss_scale: 1.0,
            };
            dispatch(app, cmd, &train_shader, &train, batch_size);
            compute_barrier(app, cmd);

            let grad = GradPush {
                batch: b_batch.gpu_address,
                partials: b_partials.gpu_address,
                batch_size: batch_size as u32,
                chunk_size: CHUNK_SIZE as u32,
                chunk_count: chunk_count as u32,
            };
            dispatch(app, cmd, &grad_shader, &grad, NN_WEIGHT_COUNT * chunk_count);
            compute_barrier(app, cmd);

            let adam = AdamPush {
                weights: b_weights.gpu_address,
                weight_m: b_weight_m.gpu_address,
                weight_v: b_weight_v.gpu_address,
                partials: b_partials.gpu_address,
                chunk_count: chunk_count as u32,
                batch_size: batch_size as u32,
                learning_rate: lr_at(settings.weights_lr, s),
                step: (s + 1) as f32,
                weight_decay: 0.0,
            };
            dispatch(app, cmd, &adam_shader, &adam, NN_WEIGHT_COUNT);
            compute_barrier(app, cmd);
        }
        app.vk.end_single_use_cmd(cmd);

        let previous = step;
        step = last;
        Log::progress(step as f32 / settings.steps as f32);

        if step / LOG_EVERY != previous / LOG_EVERY || step == settings.steps {
            let loss = unsafe { std::slice::from_raw_parts(b_loss.ptr as *const f32, batch_size) };
            let mean_loss = loss.iter().map(|&v| v as f64).sum::<f64>() / batch_size as f64;
            loss_history.push((step, mean_loss));
            Log::msg(&format!(
                "Neural step {}/{} loss {:.4} ({:.1}s)",
                step,
                settings.steps,
                mean_loss,
                start.elapsed().as_secs_f32()
            ));
        }
    }

    let train_time = start.elapsed().as_secs_f32();
    app.benchmarks.push(("Neural Training", train_time));

    // evaluation for the debug images, in chunks to keep each submit short
    let eval_start = Instant::now();
    const EVAL_CHUNK: usize = 16384;
    let mut offset = 0;
    while offset < pixel_count && !is_cancelled() {
        let cmd = app.vk.begin_single_use_cmd();
        let push = EvalPush {
            rt: make_rt(),
            compaction: compaction.gpu_address,
            lightmaps_info: lightmaps_info.gpu_address,
            data: b_data.gpu_address,
            reference_rays: settings.reference_rays,
            debug_sharpness_t: settings.debug_sharpness_t,
            pixel_offset: offset as u32,
            pixel_count: pixel_count as u32,
        };
        dispatch(app, cmd, &eval_shader, &push, EVAL_CHUNK.min(pixel_count - offset));
        app.vk.end_single_use_cmd(cmd);
        offset += EVAL_CHUNK;
    }
    app.benchmarks.push(("Neural Eval", eval_start.elapsed().as_secs_f32()));

    let features = read_buffer::<f32>(app, &b_features, pixel_count * NN_FEATURES);
    let weights = read_buffer::<f32>(app, &b_weights, NN_WEIGHT_COUNT);
    let eval = read_buffer::<f32>(app, &b_eval, pixel_count * 16);

    // relative error of the network diffuse against the traced reference, a quick quality number
    let mut diffuse_err = 0.0f64;
    let mut specular_err = 0.0f64;
    let mut count = 0usize;
    for p in 0..pixel_count {
        let e = &eval[p * 16..p * 16 + 16];
        let rel = |a: f32, b: f32| ((a - b).abs() / (b.abs() + 0.05)) as f64;
        let d = (rel(e[0], e[12]) + rel(e[1], e[13]) + rel(e[2], e[14])) / 3.0;
        let s = (rel(e[4], e[8]) + rel(e[5], e[9]) + rel(e[6], e[10])) / 3.0;
        if d.is_finite() && s.is_finite() {
            diffuse_err += d;
            specular_err += s;
            count += 1;
        }
    }
    if count > 0 && settings.reference_rays > 0 {
        Log::msg(&format!(
            "Neural trained in {:.1}s, mean relative error vs {} ray reference: diffuse {:.3}, specular {:.3}",
            train_time,
            settings.reference_rays,
            diffuse_err / count as f64,
            specular_err / count as f64
        ));
    }

    for shader in [&mut train_shader, &mut grad_shader, &mut adam_shader, &mut eval_shader] {
        shader.destroy(&app.vk);
    }
    for buffer in [
        &mut b_weights,
        &mut b_weight_m,
        &mut b_weight_v,
        &mut b_partials,
        &mut b_features,
        &mut b_feature_m,
        &mut b_feature_v,
        &mut b_visits,
        &mut b_batch,
        &mut b_loss,
        &mut b_eval,
        &mut b_pixels,
        &mut b_texels,
        &mut b_order,
        &mut b_data,
    ] {
        buffer.destroy(&app.vk);
    }

    Some(NeuralResult {
        pixels,
        features,
        weights,
        eval,
    })
}

fn stable_guid(path: &PathBuf) -> String {
    let mut a = DefaultHasher::new();
    ("glim-neural-a", path.to_string_lossy()).hash(&mut a);
    let mut b = DefaultHasher::new();
    ("glim-neural-b", path.to_string_lossy()).hash(&mut b);
    format!("{:016x}{:016x}", a.finish(), b.finish())
}

/// uncompressed linear data texture, point filtered, no mips
fn write_data_texture_meta(texture_path: &PathBuf, float32: bool) {
    let mut os = texture_path.as_os_str().to_owned();
    os.push(".meta");
    let path = PathBuf::from(os);
    if path.exists() {
        return;
    }
    let guid = stable_guid(texture_path);
    // RGBAHalf 17, RGBAFloat 20
    let format = if float32 { 20 } else { 17 };

    let yaml = format!(
        "fileFormatVersion: 2
guid: {guid}
TextureImporter:
  internalIDToNameTable: []
  externalObjects: {{}}
  serializedVersion: 13
  mipmaps:
    mipMapMode: 0
    enableMipMap: 0
    sRGBTexture: 0
    linearTexture: 1
  bumpmap:
    convertToNormalMap: 0
  isReadable: 0
  streamingMipmaps: 0
  textureFormat: 1
  maxTextureSize: 16384
  textureSettings:
    serializedVersion: 2
    filterMode: 0
    aniso: 0
    mipBias: 0
    wrapU: 1
    wrapV: 1
    wrapW: 1
  nPOTScale: 0
  lightmap: 0
  compressionQuality: 50
  alphaUsage: 1
  alphaIsTransparency: 0
  textureType: 0
  textureShape: 1
  singleChannelComponent: 0
  maxTextureSizeSet: 0
  compressionQualitySet: 0
  textureFormatSet: 0
  ignorePngGamma: 0
  applyGammaDecoding: 0
  platformSettings:
  - serializedVersion: 4
    buildTarget: DefaultTexturePlatform
    maxTextureSize: 16384
    resizeAlgorithm: 0
    textureFormat: {format}
    textureCompression: 0
    compressionQuality: 50
    crunchedCompression: 0
    allowsAlphaSplitting: 0
    overridden: 0
    ignorePlatformSupport: 0
    androidETC2FallbackOverride: 0
    forceMaximumCompressionQuality_BC6H_BC7: 0
  userData:
  assetBundleName:
  assetBundleVariant:
"
    );
    let _ = std::fs::write(&path, yaml);
}

/// pixels are indexed like the lightmap buffers, row 0 is the bottom, written flipped like the other lightmaps
fn write_exr_rgba_half(path: &PathBuf, width: usize, height: usize, pixels: &[[f32; 4]]) {
    use exr::prelude::*;
    let channels = SpecificChannels::rgba(|pos: Vec2<usize>| {
        let p = pixels[pos.x() + (height - 1 - pos.y()) * width];
        (
            f16::from_f32(p[0]),
            f16::from_f32(p[1]),
            f16::from_f32(p[2]),
            f16::from_f32(p[3]),
        )
    });
    let layer = Layer::new((width, height), LayerAttributes::named("neural"), Encoding::FAST_LOSSLESS, channels);
    Image::from_layer(layer).write().to_file(path).unwrap();
}

fn write_exr_rgba_f32(path: &PathBuf, width: usize, height: usize, pixels: &[[f32; 4]]) {
    use exr::prelude::*;
    let channels = SpecificChannels::rgba(|pos: Vec2<usize>| {
        let p = pixels[pos.x() + (height - 1 - pos.y()) * width];
        (p[0], p[1], p[2], p[3])
    });
    let layer = Layer::new((width, height), LayerAttributes::named("weights"), Encoding::UNCOMPRESSED, channels);
    Image::from_layer(layer).write().to_file(path).unwrap();
}

/// fills empty pixels from their neighbours, so bilinear and 4 tap lookups at chart borders read valid features
fn dilate_features(features: &mut [[f32; NN_FEATURES]], valid: &mut [bool], resolution: usize, passes: usize) {
    for _ in 0..passes {
        let snapshot_valid = valid.to_vec();
        let mut changed = false;
        for y in 0..resolution {
            for x in 0..resolution {
                let i = x + y * resolution;
                if snapshot_valid[i] {
                    continue;
                }
                let mut sum = [0.0f32; NN_FEATURES];
                let mut n = 0;
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        let nx = x as i32 + dx;
                        let ny = y as i32 + dy;
                        if nx < 0 || ny < 0 || nx >= resolution as i32 || ny >= resolution as i32 {
                            continue;
                        }
                        let j = nx as usize + ny as usize * resolution;
                        if snapshot_valid[j] {
                            for k in 0..NN_FEATURES {
                                sum[k] += features[j][k];
                            }
                            n += 1;
                        }
                    }
                }
                if n > 0 {
                    for k in 0..NN_FEATURES {
                        features[i][k] = sum[k] / n as f32;
                    }
                    valid[i] = true;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
}

pub fn export_neural(app: &Glim, result: &NeuralResult, debug_images: bool) {
    // weights, 4 per texel
    let texel_count = NN_WEIGHT_COUNT.div_ceil(4);
    let height = texel_count.div_ceil(NN_WEIGHTS_TEXTURE_WIDTH);
    let mut texels = vec![[0.0f32; 4]; NN_WEIGHTS_TEXTURE_WIDTH * height];
    for (i, w) in result.weights.iter().enumerate() {
        texels[i / 4][i % 4] = *w;
    }
    let path = app.output_dir.join("NeuralWeights.exr");
    write_exr_rgba_f32(&path, NN_WEIGHTS_TEXTURE_WIDTH, height, &texels);
    write_data_texture_meta(&path, true);

    for (group_index, group) in app.groups.iter().enumerate() {
        if group.settings.discard {
            continue;
        }
        let resolution = group.settings.width as usize;
        let count = resolution * resolution;

        let mut features = vec![[0.0f32; NN_FEATURES]; count];
        let mut valid = vec![false; count];
        let mut debug = vec![[[0.0f32; 4]; 4]; if debug_images { count } else { 0 }];

        for (p, pixel) in result.pixels.iter().enumerate() {
            if pixel.group as usize != group_index {
                continue;
            }
            let i = pixel.index as usize;
            features[i].copy_from_slice(&result.features[p * NN_FEATURES..(p + 1) * NN_FEATURES]);
            valid[i] = true;
            if debug_images {
                for k in 0..4 {
                    let e = &result.eval[p * 16 + k * 4..p * 16 + k * 4 + 4];
                    debug[i][k] = [e[0], e[1], e[2], 1.0];
                }
            }
        }

        dilate_features(&mut features, &mut valid, resolution, 16);

        for t in 0..NN_FEATURES / 4 {
            let pixels: Vec<[f32; 4]> = features
                .iter()
                .map(|f| [f[t * 4], f[t * 4 + 1], f[t * 4 + 2], f[t * 4 + 3]])
                .collect();
            let path = app.output_dir.join(format!("Lightmap-{}_Neural{}.exr", group_index, t));
            write_exr_rgba_half(&path, resolution, resolution, &pixels);
            write_data_texture_meta(&path, false);
        }

        if debug_images {
            let names = ["DiffuseNN", "SpecularNN", "SpecularReference", "DiffuseReference"];
            for (k, name) in names.iter().enumerate() {
                let pixels: Vec<[f32; 4]> = debug.iter().map(|d| d[k]).collect();
                let path = app
                    .output_dir
                    .join(format!("Lightmap-{}_NeuralDebug_{}.exr", group_index, name));
                write_exr_rgba_half(&path, resolution, resolution, &pixels);
            }
        }
    }
}
