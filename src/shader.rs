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

pub struct ShaderBindings<'a> {
    pub tlas: vk::AccelerationStructureKHR,
    pub albedos: &'a [vk::ImageView],
    pub emissions: &'a [vk::ImageView],
    pub skybox: vk::ImageView,
    pub skybox_sampler: vk::Sampler,
    pub visibility: vk::ImageView,
    pub preview_diffuse: vk::ImageView,
}

impl<'a> Default for ShaderBindings<'a> {
    fn default() -> Self {
        Self {
            tlas: vk::AccelerationStructureKHR::null(),
            albedos: &[],
            emissions: &[],
            skybox: vk::ImageView::null(),
            skybox_sampler: vk::Sampler::null(),
            visibility: vk::ImageView::null(),
            preview_diffuse: vk::ImageView::null(),
        }
    }
}

#[repr(C)]
#[derive(Default)]
pub struct SpecializationConstants {
    pub hardware_rt: u32,                  // 0
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

    pub lightmaps_info_address: u64,       // 20 21
    pub compacted_visibility_address: u64, // 22 23

    pub lights_address: u64,            // 24 25
    pub compaction_buffer_address: u64, // 26 27

    pub bvh_nodes_address: u64,     // 28 29
    pub bvh_triangles_address: u64, // 30 31
}

pub const SPECIALIZATION_MAP_ENTRIES_LEN: usize =
    size_of::<SpecializationConstants>() / size_of::<u32>();

pub const SPECIALIZATION_MAP_ENTRIES: [vk::SpecializationMapEntry; SPECIALIZATION_MAP_ENTRIES_LEN] = {
    let entry_size = size_of::<u32>();

    let mut entries = [vk::SpecializationMapEntry {
        constant_id: 0,
        offset: 0,
        size: entry_size,
    }; SPECIALIZATION_MAP_ENTRIES_LEN];

    let mut i = 0;
    while i < SPECIALIZATION_MAP_ENTRIES_LEN {
        entries[i] = vk::SpecializationMapEntry {
            constant_id: i as u32,
            offset: (i * entry_size) as u32,
            size: entry_size,
        };
        i += 1;
    }

    entries
};

pub struct ShaderBindingID {}
impl ShaderBindingID {
    pub const TLAS: u32 = 0;
    pub const VISIBILITY: u32 = 2;
    pub const PREVIEW_DIFFUSE: u32 = 4;
    pub const EMISSIONS: u32 = 5;
    pub const ALBEDOS: u32 = 3;
    pub const SKYBOX: u32 = 20;
    pub const SKYBOX_SAMPLER: u32 = 21;
}

pub fn load_compute_shader(
    vk: &VulkanContext,
    shader_name: ShaderName,
    constants: &SpecializationConstants,
    bindings: &ShaderBindings,
    push_constants_size: usize,
) -> ComputeShader {
    let mut layout_bindings = Vec::new();

    let lightmap_group_count = constants.lightmap_group_count;

    if !bindings.tlas.is_null() {
        layout_bindings.push(vk::DescriptorSetLayoutBinding {
            binding: ShaderBindingID::TLAS,
            descriptor_type: vk::DescriptorType::ACCELERATION_STRUCTURE_KHR,
            descriptor_count: 1,
            stage_flags: vk::ShaderStageFlags::COMPUTE,
            ..Default::default()
        });
    }

    if !bindings.visibility.is_null() {
        layout_bindings.push(vk::DescriptorSetLayoutBinding {
            binding: ShaderBindingID::VISIBILITY,
            descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
            descriptor_count: 1,
            stage_flags: vk::ShaderStageFlags::COMPUTE,
            ..Default::default()
        });
    }

    if !bindings.preview_diffuse.is_null() {
        layout_bindings.push(vk::DescriptorSetLayoutBinding {
            binding: ShaderBindingID::PREVIEW_DIFFUSE,
            descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
            descriptor_count: 1,
            stage_flags: vk::ShaderStageFlags::COMPUTE,
            ..Default::default()
        });
    }

    if bindings.emissions.len() > 0 {
        layout_bindings.push(vk::DescriptorSetLayoutBinding {
            binding: ShaderBindingID::EMISSIONS,
            descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
            descriptor_count: lightmap_group_count,
            stage_flags: vk::ShaderStageFlags::COMPUTE,
            ..Default::default()
        });
    }

    if bindings.albedos.len() > 0 {
        layout_bindings.push(vk::DescriptorSetLayoutBinding {
            binding: ShaderBindingID::ALBEDOS,
            descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
            descriptor_count: lightmap_group_count,
            stage_flags: vk::ShaderStageFlags::COMPUTE,
            ..Default::default()
        });
    }

    if !bindings.skybox.is_null() {
        layout_bindings.push(vk::DescriptorSetLayoutBinding {
            binding: ShaderBindingID::SKYBOX,
            descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
            descriptor_count: 1,
            stage_flags: vk::ShaderStageFlags::COMPUTE,
            ..Default::default()
        });
    }

    if !bindings.skybox_sampler.is_null() {
        layout_bindings.push(vk::DescriptorSetLayoutBinding {
            binding: ShaderBindingID::SKYBOX_SAMPLER,
            descriptor_type: vk::DescriptorType::SAMPLER,
            descriptor_count: 1,
            stage_flags: vk::ShaderStageFlags::COMPUTE,
            ..Default::default()
        });
    }

    let data_bytes = as_bytes(constants);
    let specialization_info = vk::SpecializationInfo::default()
        .map_entries(&SPECIALIZATION_MAP_ENTRIES)
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
        &layout_bindings,
        &push_constant_ranges,
        &specialization_info,
    )
}

pub fn update_compute_shader(
    vk: &VulkanContext,
    shader: &ComputeShader,
    bindings: &ShaderBindings,
) {
    let mut descriptor_writes = Vec::new();

    // TopLevelAS
    let tlases = [bindings.tlas];
    let mut info =
        vk::WriteDescriptorSetAccelerationStructureKHR::default().acceleration_structures(&tlases);
    let write = vk::WriteDescriptorSet::default()
        .push_next(&mut info)
        .dst_set(shader.descriptor_set)
        .dst_binding(ShaderBindingID::TLAS)
        .descriptor_type(vk::DescriptorType::ACCELERATION_STRUCTURE_KHR)
        .descriptor_count(1);
    if !bindings.tlas.is_null() {
        descriptor_writes.push(write);
    }

    // Albedo
    let infos: Vec<vk::DescriptorImageInfo> = bindings
        .albedos
        .iter()
        .map(|tex| vk::DescriptorImageInfo {
            image_view: *tex,
            image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            ..Default::default()
        })
        .collect();
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: ShaderBindingID::ALBEDOS,
        dst_array_element: 0,
        descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
        ..Default::default()
    };
    write = write.image_info(&infos);
    if bindings.albedos.len() > 0 {
        descriptor_writes.push(write);
    }

    // Emission
    let infos: Vec<vk::DescriptorImageInfo> = bindings
        .emissions
        .iter()
        .map(|tex| vk::DescriptorImageInfo {
            image_view: *tex,
            image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            ..Default::default()
        })
        .collect();
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: ShaderBindingID::EMISSIONS,
        dst_array_element: 0,
        descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
        ..Default::default()
    };
    write = write.image_info(&infos);
    if bindings.emissions.len() > 0 {
        descriptor_writes.push(write);
    }

    // Skybox
    let info = [vk::DescriptorImageInfo {
        image_view: bindings.skybox,
        image_layout: vk::ImageLayout::READ_ONLY_OPTIMAL,
        ..Default::default()
    }];
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: ShaderBindingID::SKYBOX,
        descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
        ..Default::default()
    };
    write = write.image_info(&info);
    if !bindings.skybox.is_null() {
        descriptor_writes.push(write);
    }

    // SkyboxSampler
    let info = [vk::DescriptorImageInfo {
        sampler: bindings.skybox_sampler,
        ..Default::default()
    }];
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: ShaderBindingID::SKYBOX_SAMPLER,
        descriptor_type: vk::DescriptorType::SAMPLER,
        ..Default::default()
    };
    write = write.image_info(&info);
    if !bindings.skybox_sampler.is_null() {
        descriptor_writes.push(write);
    }

    // VisibilityBuffer
    let info = [vk::DescriptorImageInfo {
        image_view: bindings.visibility,
        image_layout: vk::ImageLayout::GENERAL,
        ..Default::default()
    }];
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: ShaderBindingID::VISIBILITY,
        descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
        ..Default::default()
    };
    write = write.image_info(&info);
    if !bindings.visibility.is_null() {
        descriptor_writes.push(write);
    }

    // LightmapDiffuse
    let info = [vk::DescriptorImageInfo {
        image_view: bindings.preview_diffuse,
        image_layout: vk::ImageLayout::GENERAL,
        ..Default::default()
    }];
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: ShaderBindingID::PREVIEW_DIFFUSE,
        descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
        ..Default::default()
    };
    write = write.image_info(&info);
    if !bindings.preview_diffuse.is_null() {
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
