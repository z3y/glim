use crate::math::Vector3;

#[repr(C)]
struct SHProbe {
    l0: Vector3,
    sample_count: f32,

    l1x: Vector3,
    position_x: f32,

    l1y: Vector3,
    position_y: f32,

    l1z: Vector3,
    position_z: f32,
}
