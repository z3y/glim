use crate::math::{Vector2, Vector3};
use std::hash::{Hash, Hasher};

#[repr(u8)]
#[derive(Debug, PartialEq, Hash)]
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
    pub cast_shadows: bool,
    pub mixed: bool,
    pub occlusion_channel: u8,

    pub direction: Vector3,
    pub range: f32,

    pub color: Vector3,
    pub shadow_radius_or_angle: f32,

    pub spot_inner_percent: f32,
    pub spot_outer: f32,
    pub area_or_cookie_size: Vector2,

    pub up: Vector3,
    pub cookie: u32,
}

impl Hash for Light {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.position.x.to_bits().hash(state);
        self.position.y.to_bits().hash(state);
        self.position.z.to_bits().hash(state);

        self.direction.x.to_bits().hash(state);
        self.direction.y.to_bits().hash(state);
        self.direction.z.to_bits().hash(state);

        self.up.x.to_bits().hash(state);
        self.up.y.to_bits().hash(state);
        self.up.z.to_bits().hash(state);

        self.color.x.to_bits().hash(state);
        self.color.y.to_bits().hash(state);
        self.color.z.to_bits().hash(state);

        self.range.to_bits().hash(state);
        self.shadow_radius_or_angle.to_bits().hash(state);
        self.spot_inner_percent.to_bits().hash(state);
        self.spot_outer.to_bits().hash(state);

        self.area_or_cookie_size.x.to_bits().hash(state);
        self.area_or_cookie_size.y.to_bits().hash(state);

        self.cast_shadows.hash(state);
        self.ty.hash(state);
    }
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

            up: Vector3::UP,
            mixed: false,

            cookie: u32::MAX,
            cast_shadows: true,
            occlusion_channel: u8::MAX,
            area_or_cookie_size: Vector2::new(0.5, 0.5),
        }
    }
}
