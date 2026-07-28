pub enum ShaderName {
    CompactionMask,
    CompactVisibility,
    Decompact,
    InitFromBakeVertex,
    InitFromBakeFragment,
    InitFromCamera,
    BakeLightProbes,
    BakeDirect,
    BakeIndirect,
    AdjustSamples,
    Preview,
}

pub fn load_shader_bytes(name: ShaderName) -> Vec<u32> {
    #[rustfmt::skip]
    let bytes: &[u8] = match name {
        ShaderName::CompactionMask => include_bytes!(concat!(env!("OUT_DIR"), "/compaction_mask.spv")),
        ShaderName::CompactVisibility => include_bytes!(concat!(env!("OUT_DIR"), "/compact_visibility.spv")),
        ShaderName::Decompact => include_bytes!(concat!(env!("OUT_DIR"), "/decompact.spv")),
        ShaderName::InitFromBakeVertex => include_bytes!(concat!(env!("OUT_DIR"), "/init_from_bake_vertex.spv")),
        ShaderName::InitFromBakeFragment => include_bytes!(concat!(env!("OUT_DIR"), "/init_from_bake_fragment.spv")),
        ShaderName::InitFromCamera => include_bytes!(concat!(env!("OUT_DIR"), "/init_from_camera.spv")),
        ShaderName::BakeLightProbes => include_bytes!(concat!(env!("OUT_DIR"), "/bake_sh.spv")),
        ShaderName::BakeIndirect => include_bytes!(concat!(env!("OUT_DIR"), "/bake_indirect.spv")),
        ShaderName::AdjustSamples => include_bytes!(concat!(env!("OUT_DIR"), "/adjust_samples.spv")),
        ShaderName::Preview => include_bytes!(concat!(env!("OUT_DIR"), "/preview.spv")),
        ShaderName::BakeDirect => include_bytes!(concat!(env!("OUT_DIR"), "/bake_direct.spv")),
    };

    let aligned = bytes
        .chunks_exact(4)
        .map(|b| u32::from_ne_bytes(b.try_into().unwrap()))
        .collect();

    aligned
}
