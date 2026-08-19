use ash::vk;

use crate::{as_bytes, compute_shader::ComputeShader, vulkan_context::VulkanContext};

pub mod bake_direct;
pub mod bake_indirect;
pub mod compact_visibility;
pub mod compaction_mask;
pub mod decompact;
pub mod initialize_preview;
pub mod preview;

pub enum ShaderName {
    CompactionMask,
    CompactVisibility,
    Decompact,
    RasterizeVertex,
    RasterizeFragment,
    InitializePreview,
    BakeLightProbes,
    BakeDirectLight,
    BakeIndirect,
    BakeDirectEmission,
    AdjustSamples,
    Preview,
}

pub fn load_shader_bytes(name: ShaderName) -> Vec<u32> {
    #[rustfmt::skip]
    let bytes: &'static [u8] = match name {
        ShaderName::CompactionMask => include_bytes!(concat!(env!("OUT_DIR"), "/compaction_mask.spv")),
        ShaderName::CompactVisibility => include_bytes!(concat!(env!("OUT_DIR"), "/compact_visibility.spv")),
        ShaderName::Decompact => include_bytes!(concat!(env!("OUT_DIR"), "/decompact.spv")),
        ShaderName::RasterizeVertex => include_bytes!(concat!(env!("OUT_DIR"), "/rasterize_vertex.spv")),
        ShaderName::RasterizeFragment => include_bytes!(concat!(env!("OUT_DIR"), "/rasterize_fragment.spv")),
        ShaderName::InitializePreview => include_bytes!(concat!(env!("OUT_DIR"), "/initialize_preview.spv")),
        ShaderName::BakeLightProbes => include_bytes!(concat!(env!("OUT_DIR"), "/bake_sh.spv")),
        ShaderName::BakeIndirect => include_bytes!(concat!(env!("OUT_DIR"), "/bake_indirect.spv")),
        ShaderName::AdjustSamples => include_bytes!(concat!(env!("OUT_DIR"), "/adjust_samples.spv")),
        ShaderName::Preview => include_bytes!(concat!(env!("OUT_DIR"), "/preview.spv")),
        ShaderName::BakeDirectLight => include_bytes!(concat!(env!("OUT_DIR"), "/bake_direct_light.spv")),
        ShaderName::BakeDirectEmission => include_bytes!(concat!(env!("OUT_DIR"), "/bake_direct_emission.spv")),
    };

    let aligned = bytes
        .chunks_exact(4)
        .map(|b| u32::from_ne_bytes(b.try_into().unwrap()))
        .collect();

    aligned
}

#[repr(C)]
pub struct SpecializationConstants {
    pub use_camera: u32, // unused
    pub light_falloff_type: u32,
    pub transparent_primitive_offset: u32,
    pub emissive_triangles_count: u32,

    pub multiple_importance_sampling: u32,
    pub lightmap_group_count: u32,
    pub lightmap_mode: u32,
    pub coordinate_system: u32,

    pub skybox_intensity: f32,
    pub indirect_intensity: f32,
    pub lightprobe_deringing: f32,
    pub pad0: u32,

    pub vertex_address: u64,
    pub indices_address: u64,

    pub emissive_triangles_address: u64,
    pub compacted_lightmap_address: u64,

    pub lightmaps_info_address: u64,
    pub compacted_visiblity_address: u64,

    pub lights_address: u64,
    pub compaction_buffer_address: u64,
}

pub fn create_specialization_map_entries() -> [vk::SpecializationMapEntry; 19] {
    let size = size_of::<u32>();

    [
        vk::SpecializationMapEntry {
            constant_id: 0,
            offset: 0 * size as u32,
            size,
        },
        vk::SpecializationMapEntry {
            constant_id: 1,
            offset: 1 * size as u32,
            size,
        },
        vk::SpecializationMapEntry {
            constant_id: 2,
            offset: 2 * size as u32,
            size,
        },
        vk::SpecializationMapEntry {
            constant_id: 3,
            offset: 3 * size as u32,
            size,
        },
        vk::SpecializationMapEntry {
            constant_id: 4,
            offset: 4 * size as u32,
            size,
        },
        vk::SpecializationMapEntry {
            constant_id: 5,
            offset: 5 * size as u32,
            size,
        },
        vk::SpecializationMapEntry {
            constant_id: 6,
            offset: 6 * size as u32,
            size,
        },
        vk::SpecializationMapEntry {
            constant_id: 7,
            offset: 7 * size as u32,
            size,
        },
        vk::SpecializationMapEntry {
            constant_id: 8,
            offset: 8 * size as u32,
            size,
        },
        vk::SpecializationMapEntry {
            constant_id: 9,
            offset: 9 * size as u32,
            size,
        },
        vk::SpecializationMapEntry {
            constant_id: 10,
            offset: 10 * size as u32,
            size,
        },
        vk::SpecializationMapEntry {
            constant_id: 11,
            offset: 12 * size as u32,
            size: size_of::<u64>(),
        },
        vk::SpecializationMapEntry {
            constant_id: 12,
            offset: 14 * size as u32,
            size: size_of::<u64>(),
        },
        vk::SpecializationMapEntry {
            constant_id: 13,
            offset: 16 * size as u32,
            size: size_of::<u64>(),
        },
        vk::SpecializationMapEntry {
            constant_id: 14,
            offset: 18 * size as u32,
            size: size_of::<u64>(),
        },
        vk::SpecializationMapEntry {
            constant_id: 15,
            offset: 20 * size as u32,
            size: size_of::<u64>(),
        },
        vk::SpecializationMapEntry {
            constant_id: 16,
            offset: 22 * size as u32,
            size: size_of::<u64>(),
        },
        vk::SpecializationMapEntry {
            constant_id: 17,
            offset: 24 * size as u32,
            size: size_of::<u64>(),
        },
        vk::SpecializationMapEntry {
            constant_id: 18,
            offset: 26 * size as u32,
            size: size_of::<u64>(),
        },
    ]
}

pub enum ShaderBinding {
    Tlas,
    PreviewVisibility,
    PreviewDiffuse,
    Emissions,
    Albedos,
    Skybox,
}

pub fn load_compute_shader(
    vk: &VulkanContext,
    shader_name: ShaderName,
    constants: &SpecializationConstants,
    push_constants_size: usize,
    required_bindigns: &[ShaderBinding],
) -> ComputeShader {
    let mut bindings = Vec::new();

    let lightmap_group_count = constants.lightmap_group_count;

    for binding in required_bindigns {
        match binding {
            ShaderBinding::Tlas => {
                bindings.push(vk::DescriptorSetLayoutBinding {
                    binding: 0,
                    descriptor_type: vk::DescriptorType::ACCELERATION_STRUCTURE_KHR,
                    descriptor_count: 1,
                    stage_flags: vk::ShaderStageFlags::COMPUTE,
                    ..Default::default()
                });
            }
            ShaderBinding::PreviewVisibility => {
                bindings.push(vk::DescriptorSetLayoutBinding {
                    binding: 2,
                    descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
                    descriptor_count: 1,
                    stage_flags: vk::ShaderStageFlags::COMPUTE,
                    ..Default::default()
                });
            }
            ShaderBinding::PreviewDiffuse => {
                bindings.push(vk::DescriptorSetLayoutBinding {
                    binding: 4,
                    descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
                    descriptor_count: 1,
                    stage_flags: vk::ShaderStageFlags::COMPUTE,
                    ..Default::default()
                });
            }
            ShaderBinding::Emissions => {
                bindings.push(vk::DescriptorSetLayoutBinding {
                    binding: 5,
                    descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
                    descriptor_count: lightmap_group_count,
                    stage_flags: vk::ShaderStageFlags::COMPUTE,
                    ..Default::default()
                });
            }
            ShaderBinding::Albedos => {
                bindings.push(vk::DescriptorSetLayoutBinding {
                    binding: 3,
                    descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
                    descriptor_count: lightmap_group_count,
                    stage_flags: vk::ShaderStageFlags::COMPUTE,
                    ..Default::default()
                });
            }
            ShaderBinding::Skybox => {
                bindings.push(vk::DescriptorSetLayoutBinding {
                    binding: 20,
                    descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
                    descriptor_count: 1,
                    stage_flags: vk::ShaderStageFlags::COMPUTE,
                    ..Default::default()
                });
                bindings.push(vk::DescriptorSetLayoutBinding {
                    binding: 21,
                    descriptor_type: vk::DescriptorType::SAMPLER,
                    descriptor_count: 1,
                    stage_flags: vk::ShaderStageFlags::COMPUTE,
                    ..Default::default()
                });
            }
        }
    }

    let map_entries = create_specialization_map_entries();
    let data_bytes = as_bytes(constants);
    let specialization_info = vk::SpecializationInfo::default()
        .map_entries(&map_entries)
        .data(data_bytes);

    let push_constant_ranges = [vk::PushConstantRange {
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        offset: 0,
        size: push_constants_size as u32,
    }];

    let bytes = load_shader_bytes(shader_name);

    ComputeShader::new(
        vk,
        &bytes,
        &bindings,
        &push_constant_ranges,
        &specialization_info,
    )
}

pub fn bind_tlas(bindings: &mut Vec<vk::DescriptorSetLayoutBinding<'_>>) {
    bindings.push(vk::DescriptorSetLayoutBinding {
        binding: 0,
        descriptor_type: vk::DescriptorType::ACCELERATION_STRUCTURE_KHR,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        ..Default::default()
    });
}

pub fn bind_visibility(bindings: &mut Vec<vk::DescriptorSetLayoutBinding<'_>>) {
    bindings.push(vk::DescriptorSetLayoutBinding {
        binding: 2,
        descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        ..Default::default()
    });
}

pub fn bind_albedos(
    bindings: &mut Vec<vk::DescriptorSetLayoutBinding<'_>>,
    lightmap_group_count: u32,
) {
    bindings.push(vk::DescriptorSetLayoutBinding {
        binding: 3,
        descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
        descriptor_count: lightmap_group_count,
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        ..Default::default()
    });
}

pub fn bind_lightmap_diffuse(bindings: &mut Vec<vk::DescriptorSetLayoutBinding<'_>>) {
    bindings.push(vk::DescriptorSetLayoutBinding {
        binding: 4,
        descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        ..Default::default()
    });
}

pub fn bind_emissions(
    bindings: &mut Vec<vk::DescriptorSetLayoutBinding<'_>>,
    lightmap_group_count: u32,
) {
    bindings.push(vk::DescriptorSetLayoutBinding {
        binding: 5,
        descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
        descriptor_count: lightmap_group_count,
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        ..Default::default()
    });
}

pub fn bind_skybox(bindings: &mut Vec<vk::DescriptorSetLayoutBinding<'_>>) {
    bindings.push(vk::DescriptorSetLayoutBinding {
        binding: 20,
        descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        ..Default::default()
    });
    bindings.push(vk::DescriptorSetLayoutBinding {
        binding: 21,
        descriptor_type: vk::DescriptorType::SAMPLER,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        ..Default::default()
    });
}
