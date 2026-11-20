use glam::{Vec2, Vec3};
use std::hash::{Hash, Hasher};

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Hash)]
pub enum LightType {
    Directional = 0,
    Point = 1,
    Spot = 2,
    Area = 3,
    Disc = 4,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Light {
    pub position: Vec3,
    pub ty: LightType,
    pub cast_shadows: bool,
    pub mixed: bool,
    pub occlusion_channel: u8,

    pub direction: Vec3,
    pub range: f32,

    pub color: Vec3,
    pub shadow_radius_or_angle: f32,

    pub spot_inner_percent: f32,
    pub spot_outer: f32,
    pub area_or_cookie_size: Vec2,

    pub up: Vec3,
    pub cookie: u32,

    /// index into the light spectra (app_add_light_spectrum), u32::MAX = RGB light
    pub spectrum: u32,
    pub pad: [u32; 3],
}

pub const LIGHT_SPECTRUM_NONE: u32 = u32::MAX;

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
        self.spectrum.hash(state);
    }
}

impl Default for Light {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            ty: LightType::Directional,

            direction: Vec3::NEG_Z,
            range: 0.0,

            color: Vec3::ONE,
            shadow_radius_or_angle: 0.0,

            spot_inner_percent: 0.0,
            spot_outer: 0.0,

            up: Vec3::Y,
            mixed: false,

            cookie: u32::MAX,
            cast_shadows: true,
            occlusion_channel: u8::MAX,
            area_or_cookie_size: Vec2::new(0.5, 0.5),
            spectrum: LIGHT_SPECTRUM_NONE,
            pad: [0; 3],
        }
    }
}
