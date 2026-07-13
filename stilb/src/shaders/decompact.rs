use ash::vk;
use shaders::load_shader_bytes;

use crate::{
    as_bytes, compute_shader::*, shader_bindings::*,
    shaders::compact_visibility::CompactPushConstants, vulkan_context::VulkanContext,
};

pub fn load_shader_decompact(
    vk: &VulkanContext,
    constants: &SpecializationConstants,
) -> ComputeShader {
    let mut bindings = Vec::new();

    bind_compaction_buffer(&mut bindings);
    bind_decompact_target(&mut bindings);

    let map_entries = create_specialization_map_entries();
    let data_bytes = as_bytes(constants);
    let specialization_info = vk::SpecializationInfo::default()
        .map_entries(&map_entries)
        .data(data_bytes);

    let push_constant_ranges = [vk::PushConstantRange {
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        offset: 0,
        size: std::mem::size_of::<CompactPushConstants>() as u32,
    }];

    let bytes = load_shader_bytes(shaders::ShaderName::Decompact);

    ComputeShader::new(
        vk,
        &bytes,
        &bindings,
        &push_constant_ranges,
        &specialization_info,
    )
}

pub fn update_shader_decompact(
    vk: &VulkanContext,
    shader: &ComputeShader,
    compaction: vk::Buffer,
    decompact_target: vk::Buffer,
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

    unsafe { vk.device.update_descriptor_sets(&descriptor_writes, &[]) };
}
