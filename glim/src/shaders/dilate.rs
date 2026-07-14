use ash::vk;
use shaders::load_shader_bytes;

use crate::{as_bytes, compute_shader::*, vulkan_context::VulkanContext};

#[repr(C)]
pub struct DilatePushConstants {
    pub width: u32,
    pub height: u32,
    pub pad0: u32,
    pub pad1: u32,
}

pub fn load_shader_dilate(
    vk: &VulkanContext,
    constants: &SpecializationConstants,
) -> ComputeShader {
    let mut bindings = Vec::new();

    bindings.push(vk::DescriptorSetLayoutBinding {
        binding: 0,
        descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        ..Default::default()
    });

    let map_entries = create_specialization_map_entries();
    let data_bytes = as_bytes(constants);
    let specialization_info = vk::SpecializationInfo::default()
        .map_entries(&map_entries)
        .data(data_bytes);

    let push_constant_ranges = [vk::PushConstantRange {
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        offset: 0,
        size: std::mem::size_of::<DilatePushConstants>() as u32,
    }];

    let bytes = load_shader_bytes(shaders::ShaderName::Dilate);

    ComputeShader::new(
        vk,
        &bytes,
        &bindings,
        &push_constant_ranges,
        &specialization_info,
    )
}

pub fn update_shader_dilate(vk: &VulkanContext, shader: &ComputeShader, tex: vk::Buffer) {
    let mut descriptor_writes = Vec::new();

    let info = [vk::DescriptorBufferInfo {
        buffer: tex,
        offset: 0,
        range: vk::WHOLE_SIZE,
    }];
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: 0,
        descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
        ..Default::default()
    };
    write = write.buffer_info(&info);
    descriptor_writes.push(write);

    unsafe { vk.device.update_descriptor_sets(&descriptor_writes, &[]) };
}
