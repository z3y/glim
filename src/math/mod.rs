mod vector2;
mod vector3;
mod vector4;

pub use vector2::Vector2;
pub use vector3::Vector3;
pub use vector4::Vector4;

#[inline]
pub const fn vec3(x: f32, y: f32, z: f32) -> Vector3 {
    Vector3 { x, y, z }
}

// #[inline]
// pub const fn vec2(x: f32, y: f32) -> Vector2 {
//     Vector2 { x, y }
// }

#[inline]
pub const fn vec4(x: f32, y: f32, z: f32, w: f32) -> Vector4 {
    Vector4 { x, y, z, w }
}
