// https://jack.wrenn.fyi/blog/include-transmute/

macro_rules! include_transmute {
    ($file:expr, $type:ty) => {
        unsafe { core::mem::transmute(*include_bytes!($file)) }
    };
}

pub fn get_test_shader() -> &'static [u32] {
    const LEN: usize = include_bytes!(concat!(env!("OUT_DIR"), "/test.spv")).len() / 4;

    static SHADER: [u32; LEN] =
        include_transmute!(concat!(env!("OUT_DIR"), "/test.spv"), [u32; LEN]);

    &SHADER
}

pub fn get_bake_lights_shader() -> &'static [u32] {
    const LEN: usize = include_bytes!(concat!(env!("OUT_DIR"), "/bake_lights.spv")).len() / 4;

    static SHADER: [u32; LEN] =
        include_transmute!(concat!(env!("OUT_DIR"), "/bake_lights.spv"), [u32; LEN]);

    &SHADER
}

pub fn get_visibility_fragment_shader() -> &'static [u32] {
    const LEN: usize =
        include_bytes!(concat!(env!("OUT_DIR"), "/visibility_fragment.spv")).len() / 4;

    static SHADER: [u32; LEN] = include_transmute!(
        concat!(env!("OUT_DIR"), "/visibility_fragment.spv"),
        [u32; LEN]
    );

    &SHADER
}

pub fn get_visibility_vertex_shader() -> &'static [u32] {
    const LEN: usize = include_bytes!(concat!(env!("OUT_DIR"), "/visibility_vertex.spv")).len() / 4;

    static SHADER: [u32; LEN] = include_transmute!(
        concat!(env!("OUT_DIR"), "/visibility_vertex.spv"),
        [u32; LEN]
    );

    &SHADER
}

pub fn get_visibility_geometry_shader() -> &'static [u32] {
    const LEN: usize =
        include_bytes!(concat!(env!("OUT_DIR"), "/visibility_geometry.spv")).len() / 4;

    static SHADER: [u32; LEN] = include_transmute!(
        concat!(env!("OUT_DIR"), "/visibility_geometry.spv"),
        [u32; LEN]
    );

    &SHADER
}
