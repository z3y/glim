use crate::math::{Vector2, Vector3};

#[repr(u32)]
#[derive(Debug, PartialEq)]
pub enum LightType {
    Directional = 0,
    Point = 1,
    Spot = 2,
    Area = 3,
    Disc = 4,
}

#[repr(C)]
#[derive(Debug)]
pub struct Light {
    pub position: Vector3,
    pub ty: LightType,

    pub direction: Vector3,
    pub range: f32,

    pub color: Vector3,
    pub shadow_radius_or_angle: f32,

    pub spot_inner_percent: f32,
    pub spot_outer: f32,
    pub area_size_or_disc_radius: Vector2,

    pub up: Vector3,
    pub mixed: u32,

    pub cookie_size: Vector2,
    pub cookie: u32,
    pub cast_shadows: u32,
}

impl Default for Light {
    fn default() -> Self {
        Self {
            position: Vector3::ZERO,
            ty: LightType::Directional,

            direction: Vector3::FORWARD,
            range: 0.0,

            color: Vector3::ONE,
            shadow_radius_or_angle: 0.0,

            spot_inner_percent: 0.0,
            spot_outer: 0.0,
            area_size_or_disc_radius: Vector2::ZERO,

            up: Vector3::UP,
            mixed: 0,

            cookie: u32::MAX,
            cookie_size: Vector2::new(0.5, 0.5),
            cast_shadows: 1,
        }
    }
}
