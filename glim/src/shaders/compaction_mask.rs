use ash::vk;
use shaders::load_shader_bytes;

use crate::{as_bytes, compute_shader::*, shader_bindings::*, vulkan_context::VulkanContext};

#[repr(C)]
pub struct CompactionPushConstants {
    pub width: u32,
    pub height: u32,
    pub offset: u32,
    pub compacted_count: u32,

    pub lightmap_type: u32,
    pub pad0: u32,
    pub pad1: u32,
    pub pad2: u32,
}

pub fn load_shader_compaction_mask(
    vk: &VulkanContext,
    constants: &SpecializationConstants,
) -> ComputeShader {
    let mut bindings = Vec::new();

    bind_visibility(&mut bindings);
    bind_compaction_buffer(&mut bindings);

    let map_entries = create_specialization_map_entries();
    let data_bytes = as_bytes(constants);
    let specialization_info = vk::SpecializationInfo::default()
        .map_entries(&map_entries)
        .data(data_bytes);

    let push_constant_ranges = [vk::PushConstantRange {
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        offset: 0,
        size: std::mem::size_of::<CompactionPushConstants>() as u32,
    }];

    let bytes = load_shader_bytes(shaders::ShaderName::CompactionMask);

    ComputeShader::new(
        vk,
        &bytes,
        &bindings,
        &push_constant_ranges,
        &specialization_info,
    )
}

pub fn update_shader_compaction_mask(
    vk: &VulkanContext,
    shader: &ComputeShader,
    visibility: vk::ImageView,
    compaction: vk::Buffer,
) {
    let mut descriptor_writes = Vec::new();

    // VisibilityBuffer
    let info = [vk::DescriptorImageInfo {
        image_view: visibility,
        image_layout: vk::ImageLayout::GENERAL,
        ..Default::default()
    }];
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: 2,
        descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
        ..Default::default()
    };
    write = write.image_info(&info);
    descriptor_writes.push(write);

    // CompactionMaskBuffer
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

    unsafe { vk.device.update_descriptor_sets(&descriptor_writes, &[]) };
}
