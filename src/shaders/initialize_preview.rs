use ash::vk;

use crate::{
    as_bytes,
    compute_shader::*,
    math::Vector3,
    shaders::{SpecializationConstants, create_specialization_map_entries, *},
    texture2d::Texture2D,
    vulkan_context::VulkanContext,
};

#[repr(C)]
pub struct PushConstants {
    pub camera_position: Vector3,
    pub fov_half_tan: f32,

    pub camera_direction: Vector3,
    pub pad: u32,
}

pub fn load_shader(vk: &VulkanContext, constants: &SpecializationConstants) -> ComputeShader {
    let mut bindings = Vec::new();

    bind_visibility(&mut bindings);
    bind_tlas(&mut bindings);
    bind_indices(&mut bindings);
    bind_vertices(&mut bindings);
    bind_albedos(&mut bindings, constants.lightmap_group_count);

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

    ComputeShader::new(
        vk,
        &load_shader_bytes(ShaderName::InitializePreview),
        &bindings,
        &push_constant_ranges,
        &specialization_info,
    )
}

pub fn update_shader(
    vk: &VulkanContext,
    shader: &ComputeShader,
    tlas: vk::AccelerationStructureKHR,
    visibility: &Texture2D,
    indices: vk::Buffer,
    vertices: vk::Buffer,
    albedos: &[vk::ImageView],
) {
    let mut descriptor_writes = Vec::new();

    // TopLevelAS
    let tlas = [tlas];
    let mut info =
        vk::WriteDescriptorSetAccelerationStructureKHR::default().acceleration_structures(&tlas);
    let write = vk::WriteDescriptorSet::default()
        .push_next(&mut info)
        .dst_set(shader.descriptor_set)
        .dst_binding(0)
        .descriptor_type(vk::DescriptorType::ACCELERATION_STRUCTURE_KHR)
        .descriptor_count(1);
    descriptor_writes.push(write);

    // VisibilityBuffer
    let info = [vk::DescriptorImageInfo {
        image_view: visibility.view(),
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

    // Indices
    let info = [vk::DescriptorBufferInfo {
        buffer: indices,
        offset: 0,
        range: vk::WHOLE_SIZE,
    }];
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: 8,
        descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
        ..Default::default()
    };
    write = write.buffer_info(&info);
    descriptor_writes.push(write);

    // Vertices
    let info = [vk::DescriptorBufferInfo {
        buffer: vertices,
        offset: 0,
        range: vk::WHOLE_SIZE,
    }];
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: 9,
        descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
        ..Default::default()
    };
    write = write.buffer_info(&info);
    descriptor_writes.push(write);

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
        dst_binding: 3,
        dst_array_element: 0,
        descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
        ..Default::default()
    };
    write = write.image_info(&infos);
    descriptor_writes.push(write);

    unsafe { vk.device.update_descriptor_sets(&descriptor_writes, &[]) };
}
