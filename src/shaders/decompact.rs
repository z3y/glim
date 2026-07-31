use ash::vk;

use crate::{
    as_bytes,
    compute_shader::*,
    shaders::*,
    shaders::{SpecializationConstants, create_specialization_map_entries},
    vulkan_context::VulkanContext,
};

#[repr(C)]
pub struct PushConstants {
    pub width: u32,
    pub height: u32,
    pub offset: u32,
    pub compacted_count: u32,

    pub encode_type: u32,
    pub group_index: u32,
    pub dilate: u32,
    pub pad2: u32,
}

pub fn load_shader(vk: &VulkanContext, constants: &SpecializationConstants) -> ComputeShader {
    let mut bindings = Vec::new();

    bind_compaction_buffer(&mut bindings);
    bind_decompact_target(&mut bindings);
    bind_compacted_lightmap(&mut bindings);
    bind_compacted_visibility_buffer(&mut bindings);
    bind_lightmap_info(&mut bindings);

    let map_entries = create_specialization_map_entries();
    let data_bytes = as_bytes(constants);
    let specialization_info = vk::SpecializationInfo::default()
        .map_entries(&map_entries)
        .data(data_bytes);

    let push_constant_ranges = [vk::PushConstantRange {
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        offset: 0,
        size: std::mem::size_of::<PushConstants>() as u32,
    }];

    let bytes = load_shader_bytes(ShaderName::Decompact);

    ComputeShader::new(
        vk,
        &bytes,
        &bindings,
        &push_constant_ranges,
        &specialization_info,
    )
}

pub fn update_shader(
    vk: &VulkanContext,
    shader: &ComputeShader,
    compaction: vk::Buffer,
    decompact_target: vk::Buffer,
    compacted_lightmap: vk::Buffer,
    compacted_visibility: vk::Buffer,
    lightmap_info: vk::Buffer,
) {
    let mut descriptor_writes = Vec::new();

    // CompactionBuffer
    let info = [vk::DescriptorBufferInfo {
        buffer: compaction,
        offset: 0,
        range: vk::WHOLE_SIZE,
    }];
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: 15,
        descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
        ..Default::default()
    };
    write = write.buffer_info(&info);
    descriptor_writes.push(write);

    // DecompactTarget
    let info = [vk::DescriptorBufferInfo {
        buffer: decompact_target,
        offset: 0,
        range: vk::WHOLE_SIZE,
    }];
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: 17,
        descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
        ..Default::default()
    };
    write = write.buffer_info(&info);
    descriptor_writes.push(write);

    // CompactedLightmap
    let info = [vk::DescriptorBufferInfo {
        buffer: compacted_lightmap,
        offset: 0,
        range: vk::WHOLE_SIZE,
    }];
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: 18,
        descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
        ..Default::default()
    };
    write = write.buffer_info(&info);
    descriptor_writes.push(write);

    // CompactedVisibility
    let info = [vk::DescriptorBufferInfo {
        buffer: compacted_visibility,
        offset: 0,
        range: vk::WHOLE_SIZE,
    }];
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: 16,
        descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
        ..Default::default()
    };
    write = write.buffer_info(&info);
    descriptor_writes.push(write);

    // LightmapInfo
    let info = [vk::DescriptorBufferInfo {
        buffer: lightmap_info,
        offset: 0,
        range: vk::WHOLE_SIZE,
    }];
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: 19,
        descriptor_type: vk::DescriptorType::UNIFORM_BUFFER,
        ..Default::default()
    };
    write = write.buffer_info(&info);
    descriptor_writes.push(write);

    unsafe { vk.device.update_descriptor_sets(&descriptor_writes, &[]) };
}
