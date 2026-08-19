use ash::vk::{self, Handle};

use crate::{as_bytes, compute_shader::ComputeShader, vulkan_context::VulkanContext};

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
    pub use_camera: u32,                   // 0
    pub light_falloff_type: u32,           // 1
    pub transparent_primitive_offset: u32, // 2
    pub emissive_triangles_count: u32,     // 3

    pub multiple_importance_sampling: u32, // 4
    pub lightmap_group_count: u32,         // 5
    pub lightmap_mode: u32,                // 6
    pub coordinate_system: u32,            // 7

    pub skybox_intensity: f32,     // 8
    pub indirect_intensity: f32,   // 9
    pub lightprobe_deringing: f32, // 10
    pub pad0: u32,                 // 11

    pub vertex_address: u64,  // 12 13
    pub indices_address: u64, // 14 15

    pub emissive_triangles_address: u64, // 16 17
    pub compacted_lightmap_address: u64, // 18 19

    pub lightmaps_info_address: u64,      // 20 21
    pub compacted_visiblity_address: u64, // 22 23

    pub lights_address: u64,            // 24 25
    pub compaction_buffer_address: u64, // 26 27
}

pub fn create_specialization_map_entries() -> Vec<vk::SpecializationMapEntry> {
    let entry_size = size_of::<u32>();
    let entries_total = size_of::<SpecializationConstants>();
    let entry_len = entries_total / entry_size;

    let mut entries = Vec::with_capacity(entry_len);

    for id in 0..entry_len {
        entries.push(vk::SpecializationMapEntry {
            constant_id: id as u32,
            offset: (id * entry_size) as u32,
            size: entry_size,
        });
    }

    entries
}

pub enum ShaderBinding {
    Tlas = 0,
    Visibility = 2,
    PreviewDiffuse = 4,
    Emissions = 5,
    Albedos = 3,
    Skybox = 20,
    SkyboxSampler = 21,
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
                    binding: ShaderBinding::Tlas as u32,
                    descriptor_type: vk::DescriptorType::ACCELERATION_STRUCTURE_KHR,
                    descriptor_count: 1,
                    stage_flags: vk::ShaderStageFlags::COMPUTE,
                    ..Default::default()
                });
            }
            ShaderBinding::Visibility => {
                bindings.push(vk::DescriptorSetLayoutBinding {
                    binding: ShaderBinding::Visibility as u32,
                    descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
                    descriptor_count: 1,
                    stage_flags: vk::ShaderStageFlags::COMPUTE,
                    ..Default::default()
                });
            }
            ShaderBinding::PreviewDiffuse => {
                bindings.push(vk::DescriptorSetLayoutBinding {
                    binding: ShaderBinding::PreviewDiffuse as u32,
                    descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
                    descriptor_count: 1,
                    stage_flags: vk::ShaderStageFlags::COMPUTE,
                    ..Default::default()
                });
            }
            ShaderBinding::Emissions => {
                bindings.push(vk::DescriptorSetLayoutBinding {
                    binding: ShaderBinding::Emissions as u32,
                    descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
                    descriptor_count: lightmap_group_count,
                    stage_flags: vk::ShaderStageFlags::COMPUTE,
                    ..Default::default()
                });
            }
            ShaderBinding::Albedos => {
                bindings.push(vk::DescriptorSetLayoutBinding {
                    binding: ShaderBinding::Albedos as u32,
                    descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
                    descriptor_count: lightmap_group_count,
                    stage_flags: vk::ShaderStageFlags::COMPUTE,
                    ..Default::default()
                });
            }
            ShaderBinding::Skybox => {
                bindings.push(vk::DescriptorSetLayoutBinding {
                    binding: ShaderBinding::Skybox as u32,
                    descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
                    descriptor_count: 1,
                    stage_flags: vk::ShaderStageFlags::COMPUTE,
                    ..Default::default()
                });
            }
            ShaderBinding::SkyboxSampler => {
                bindings.push(vk::DescriptorSetLayoutBinding {
                    binding: ShaderBinding::SkyboxSampler as u32,
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

pub fn update_compute_shader(
    vk: &VulkanContext,
    shader: &ComputeShader,
    tlas: vk::AccelerationStructureKHR,
    albedos: &[vk::ImageView],
    emissions: &[vk::ImageView],
    skybox: vk::ImageView,
    skybox_sampler: vk::Sampler,
    expanded_visibility: vk::ImageView,
    preview_diffuse: vk::ImageView,
) {
    let mut descriptor_writes = Vec::new();

    // TopLevelAS
    let tlases = [tlas];
    let mut info =
        vk::WriteDescriptorSetAccelerationStructureKHR::default().acceleration_structures(&tlases);
    let write = vk::WriteDescriptorSet::default()
        .push_next(&mut info)
        .dst_set(shader.descriptor_set)
        .dst_binding(ShaderBinding::Tlas as u32)
        .descriptor_type(vk::DescriptorType::ACCELERATION_STRUCTURE_KHR)
        .descriptor_count(1);
    if !tlas.is_null() {
        descriptor_writes.push(write);
    }

    // Albedo
    let infos: Vec<vk::DescriptorImageInfo> = albedos
        .iter()
        .map(|tex| vk::DescriptorImageInfo {
            image_view: *tex,
            image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            ..Default::default()
        })
        .collect();
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: ShaderBinding::Albedos as u32,
        dst_array_element: 0,
        descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
        ..Default::default()
    };
    write = write.image_info(&infos);
    if albedos.len() > 0 {
        descriptor_writes.push(write);
    }

    // Emission
    let infos: Vec<vk::DescriptorImageInfo> = emissions
        .iter()
        .map(|tex| vk::DescriptorImageInfo {
            image_view: *tex,
            image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            ..Default::default()
        })
        .collect();
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: ShaderBinding::Emissions as u32,
        dst_array_element: 0,
        descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
        ..Default::default()
    };
    write = write.image_info(&infos);
    if emissions.len() > 0 {
        descriptor_writes.push(write);
    }

    // Skybox
    let info = [vk::DescriptorImageInfo {
        image_view: skybox,
        image_layout: vk::ImageLayout::READ_ONLY_OPTIMAL,
        ..Default::default()
    }];
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: ShaderBinding::Skybox as u32,
        descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
        ..Default::default()
    };
    write = write.image_info(&info);
    if !skybox.is_null() {
        descriptor_writes.push(write);
    }

    // SkyboxSampler
    let info = [vk::DescriptorImageInfo {
        sampler: skybox_sampler,
        ..Default::default()
    }];
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: ShaderBinding::SkyboxSampler as u32,
        descriptor_type: vk::DescriptorType::SAMPLER,
        ..Default::default()
    };
    write = write.image_info(&info);
    if !skybox_sampler.is_null() {
        descriptor_writes.push(write);
    }

    // VisibilityBuffer
    let info = [vk::DescriptorImageInfo {
        image_view: expanded_visibility,
        image_layout: vk::ImageLayout::GENERAL,
        ..Default::default()
    }];
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: ShaderBinding::Visibility as u32,
        descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
        ..Default::default()
    };
    write = write.image_info(&info);
    if !expanded_visibility.is_null() {
        descriptor_writes.push(write);
    }

    // LightmapDiffuse
    let info = [vk::DescriptorImageInfo {
        image_view: preview_diffuse,
        image_layout: vk::ImageLayout::GENERAL,
        ..Default::default()
    }];
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: ShaderBinding::PreviewDiffuse as u32,
        descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
        ..Default::default()
    };
    write = write.image_info(&info);
    if !preview_diffuse.is_null() {
        descriptor_writes.push(write);
    }

    unsafe { vk.device.update_descriptor_sets(&descriptor_writes, &[]) };
}

#[repr(C)]
pub struct PreviewPushConstants {
    pub lights_count: u32,
    pub max_samples: u32,

    pub sample_index: u32,
    pub width: u32,
    pub height: u32,
    pub bounce_count: u32,
}
