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

pub fn get_preview_shader() -> &'static [u32] {
    const LEN: usize = include_bytes!(concat!(env!("OUT_DIR"), "/preview.spv")).len() / 4;

    static SHADER: [u32; LEN] =
        include_transmute!(concat!(env!("OUT_DIR"), "/preview.spv"), [u32; LEN]);

    &SHADER
}

pub fn get_bake_direct_shader() -> &'static [u32] {
    const LEN: usize = include_bytes!(concat!(env!("OUT_DIR"), "/bake_direct.spv")).len() / 4;

    static SHADER: [u32; LEN] =
        include_transmute!(concat!(env!("OUT_DIR"), "/bake_direct.spv"), [u32; LEN]);

    &SHADER
}

pub fn get_adjust_samples_shader() -> &'static [u32] {
    const LEN: usize = include_bytes!(concat!(env!("OUT_DIR"), "/adjust_samples.spv")).len() / 4;

    static SHADER: [u32; LEN] =
        include_transmute!(concat!(env!("OUT_DIR"), "/adjust_samples.spv"), [u32; LEN]);

    &SHADER
}

pub fn get_bake_bounce_shader() -> &'static [u32] {
    const LEN: usize = include_bytes!(concat!(env!("OUT_DIR"), "/bake_bounce.spv")).len() / 4;

    static SHADER: [u32; LEN] =
        include_transmute!(concat!(env!("OUT_DIR"), "/bake_bounce.spv"), [u32; LEN]);

    &SHADER
}

pub fn get_bake_sh_shader() -> &'static [u32] {
    const LEN: usize = include_bytes!(concat!(env!("OUT_DIR"), "/bake_sh.spv")).len() / 4;

    static SHADER: [u32; LEN] =
        include_transmute!(concat!(env!("OUT_DIR"), "/bake_sh.spv"), [u32; LEN]);

    &SHADER
}

pub fn get_init_from_camera_shader() -> &'static [u32] {
    const LEN: usize = include_bytes!(concat!(env!("OUT_DIR"), "/init_from_camera.spv")).len() / 4;

    static SHADER: [u32; LEN] = include_transmute!(
        concat!(env!("OUT_DIR"), "/init_from_camera.spv"),
        [u32; LEN]
    );

    &SHADER
}

pub fn get_init_from_bake_fragment_shader() -> &'static [u32] {
    const LEN: usize =
        include_bytes!(concat!(env!("OUT_DIR"), "/init_from_bake_fragment.spv")).len() / 4;

    static SHADER: [u32; LEN] = include_transmute!(
        concat!(env!("OUT_DIR"), "/init_from_bake_fragment.spv"),
        [u32; LEN]
    );

    &SHADER
}

pub fn get_init_from_bake_vertex_shader() -> &'static [u32] {
    const LEN: usize =
        include_bytes!(concat!(env!("OUT_DIR"), "/init_from_bake_vertex.spv")).len() / 4;

    static SHADER: [u32; LEN] = include_transmute!(
        concat!(env!("OUT_DIR"), "/init_from_bake_vertex.spv"),
        [u32; LEN]
    );

    &SHADER
}

pub enum ShaderName {
    CompactionMask,
    CompactVisibility,
    Decompact,
    Dilate,
}

const COMPACTION_MASK_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/compaction_mask.spv"));
const COMPACT_VISIBILITY_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/compact_visibility.spv"));
const DECOMPACT_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/decompact.spv"));

const DILATE_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/dilate.spv"));

pub fn load_shader_bytes(name: ShaderName) -> Vec<u32> {
    let bytes = match name {
        ShaderName::CompactionMask => COMPACTION_MASK_BYTES,
        ShaderName::CompactVisibility => COMPACT_VISIBILITY_BYTES,
        ShaderName::Decompact => DECOMPACT_BYTES,
        ShaderName::Dilate => DILATE_BYTES,
    };

    let aligned = bytes
        .chunks_exact(4)
        .map(|b| u32::from_ne_bytes(b.try_into().unwrap()))
        .collect();

    aligned
}
