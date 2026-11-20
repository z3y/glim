use ash::vk;
use glam::Vec3;

use crate::{Glim, vulkan_context::VulkanContext};

pub mod keys {
    pub const FORWARD: u32 = 1 << 0;
    pub const BACK: u32 = 1 << 1;
    pub const LEFT: u32 = 1 << 2;
    pub const RIGHT: u32 = 1 << 3;
    pub const DOWN: u32 = 1 << 4;
    pub const UP: u32 = 1 << 5;
    pub const FAST: u32 = 1 << 6;
    pub const SLOW: u32 = 1 << 7;
    pub const RESTART: u32 = 1 << 8;
    pub const PRINT_CAMERA: u32 = 1 << 9;
}

/// Input for one preview frame, independent of where it came from (GLFW or Unity)
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PreviewInput {
    /// `keys::*` bitflags of currently held keys
    pub keys: u32,
    /// cursor movement in pixels since the last frame, y down
    pub look_delta_x: f32,
    pub look_delta_y: f32,
    /// mouse look is active (right mouse button held)
    pub looking: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PreviewStats {
    pub sample_index: u32,
    pub sample_count: u32,
    pub width: u32,
    pub height: u32,
    pub light_count: u32,
    pub hardware_rt: bool,
}

/// Settings that can change while the preview is running
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PreviewSettings {
    pub sample_count: u32,
    pub bounce_count: u32,
    pub throttle_ms: u32,
}

/// Camera driven by the host (Unity scene view), in the host's coordinate system
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct PreviewCamera {
    pub position: Vec3,
    pub forward: Vec3,
    pub up: Vec3,
    /// vertical field of view in degrees
    pub fov: f32,
    /// half of the vertical view size, orthographic when > 0
    pub ortho_size: f32,
}

pub struct HostFrame {
    pub input: PreviewInput,
    pub close: bool,
    /// latest settings sent by the host, if any
    pub settings: Option<PreviewSettings>,
    /// camera set by the host, replaces the free fly input when present
    pub camera: Option<PreviewCamera>,
}

/// Where the preview gets its input from and where the rendered image goes.
pub trait PreviewHost {
    /// Polls events and input, called once per preview loop iteration
    fn begin_frame(&mut self) -> HostFrame;

    /// Size the preview should render at, (0, 0) when there's nothing to render to (minimized)
    fn size(&self, vk: &VulkanContext) -> (u32, u32);

    /// Waits until a frame slot is free and returns its command buffer.
    /// None means the output is out of date and the size has to be queried again.
    fn acquire(
        &mut self,
        vk: &mut VulkanContext,
        width: u32,
        height: u32,
    ) -> Option<vk::CommandBuffer>;

    /// Records copying `src` (TRANSFER_SRC_OPTIMAL, R32G32B32A32_SFLOAT) to the output.
    /// `changed` is true when the image content changed this frame.
    fn record_output(
        &mut self,
        vk: &VulkanContext,
        cmd: vk::CommandBuffer,
        src: vk::Image,
        width: u32,
        height: u32,
        changed: bool,
    );

    /// Submits the recorded command buffer, false if the output went out of date
    fn submit(
        &mut self,
        vk: &mut VulkanContext,
        cmd: vk::CommandBuffer,
        width: u32,
        height: u32,
    ) -> bool;

    fn end_frame(&mut self, stats: &PreviewStats);

    /// Frees host resources, the device must be idle
    fn destroy(&mut self, vk: &mut VulkanContext);
}

/// Matches the host camera exactly, restarts the accumulation only when something changed
pub fn apply_host_camera(app: &mut Glim, host_camera: &PreviewCamera) {
    let system = app.config.coordinate_system;
    let mut position = host_camera.position;
    let mut forward = host_camera.forward;
    let mut up = host_camera.up;
    system.transform(&mut position);
    system.transform(&mut forward);
    system.transform(&mut up);

    let camera = &mut app.camera;
    let (old_position, old_forward, old_up) = (camera.position, camera.forward, camera.up);
    let (old_fov, old_ortho) = (camera.fov, camera.ortho_size);

    camera.position = position;
    camera.set_basis(forward, up);
    if host_camera.fov > 0.0 {
        camera.fov = host_camera.fov.clamp(0.01, 179.0);
    }
    camera.ortho_size = host_camera.ortho_size.max(0.0);

    let changed = camera.position != old_position
        || camera.forward != old_forward
        || camera.up != old_up
        || camera.fov != old_fov
        || camera.ortho_size != old_ortho;

    if changed {
        app.preview_initialized = false;
    }
}

pub fn update_camera(app: &mut Glim, input: &PreviewInput, delta_time: f32) {
    let camera = &mut app.camera;
    let held = |k: u32| input.keys & k != 0;

    let mut move_speed = 5.0 * delta_time;
    let mouse_sensitivity = 0.0025;

    let mut camera_moved = false;
    let restart = held(keys::RESTART);

    if held(keys::FAST) {
        move_speed *= 4.0;
    }
    if held(keys::SLOW) {
        move_speed *= 0.25;
    }

    if input.looking {
        let dx = input.look_delta_x * mouse_sensitivity;
        let dy = -input.look_delta_y * mouse_sensitivity;

        if dx != 0.0 || dy != 0.0 {
            let yaw = camera.yaw() - dx;
            let pitch = (camera.pitch() + dy).clamp(-1.55334, 1.55334);
            camera.set_yaw_pitch(yaw, pitch);
            camera_moved = true;
        }
    }

    let forward = camera.get_forward().normalize_or_zero();
    let right = forward.cross(Vec3::Y).normalize_or_zero();

    let mut direction = Vec3::ZERO;
    let mut add = |k: u32, dir: Vec3| {
        if held(k) {
            direction += dir;
            camera_moved = true;
        }
    };
    add(keys::FORWARD, forward);
    add(keys::BACK, -forward);
    add(keys::RIGHT, right);
    add(keys::LEFT, -right);
    add(keys::UP, Vec3::Y);
    add(keys::DOWN, -Vec3::Y);

    if camera_moved {
        camera.position += direction.normalize_or_zero() * move_speed;
    }

    if held(keys::PRINT_CAMERA) {
        println!(
            "Camera Forward: {:#?} Camera Position: {:#?}",
            camera.get_forward(),
            camera.position
        );
    }

    if camera_moved || restart {
        app.preview_initialized = false;
    }
}
