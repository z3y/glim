use ash::vk;
use shaders::get_bake_direct_shader;

use crate::{as_bytes, compute_shader::*, shader_bindings::*, vulkan_context::VulkanContext};

#[repr(C)]
pub struct BakeDirectPushConstants {
    pub compacted_count: u32,
    pub sample_index: u32,
    pub max_samples: u32,
    pub lights_count: u32,
}

pub fn load_bake_direct_shader(
    vk: &VulkanContext,
    constants: &SpecializationConstants,
) -> ComputeShader {
    let mut bindings = Vec::new();

    bind_tlas(&mut bindings);
    bind_albedos(&mut bindings, constants.lightmap_group_count);
    bind_emissions(&mut bindings, constants.lightmap_group_count);
    bind_indices(&mut bindings);
    bind_vertices(&mut bindings);
    bind_lights(&mut bindings);
    bind_emissive_triangles(&mut bindings);
    bind_compacted_visibility_buffer(&mut bindings);
    bind_compacted_lightmap(&mut bindings);

    let map_entries = create_specialization_map_entries();
    let data_bytes = as_bytes(constants);
    let specialization_info = vk::SpecializationInfo::default()
        .map_entries(&map_entries)
        .data(data_bytes);

    let push_constant_ranges = [vk::PushConstantRange {
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        offset: 0,
        size: std::mem::size_of::<BakeDirectPushConstants>() as u32,
    }];

    ComputeShader::new(
        vk,
        get_bake_direct_shader(),
        &bindings,
        &push_constant_ranges,
        &specialization_info,
    )
}

pub fn update_bake_direct_shader(
    vk: &VulkanContext,
    shader: &ComputeShader,
    tlas: vk::AccelerationStructureKHR,
    albedos: &[vk::ImageView],
    emissions: &[vk::ImageView],
    indices: vk::Buffer,
    vertices: vk::Buffer,
    lights: vk::Buffer,
    emissive_triangles: vk::Buffer,
    compacted_visibility: vk::Buffer,
    compacted_lightmap: vk::Buffer,
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
        dst_binding: 5,
        dst_array_element: 0,
        descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
        ..Default::default()
    };
    write = write.image_info(&infos);
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

    // Lights
    let info = [vk::DescriptorBufferInfo {
        buffer: lights,
        offset: 0,
        range: vk::WHOLE_SIZE,
    }];
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: 10,
        descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
        ..Default::default()
    };
    write = write.buffer_info(&info);
    descriptor_writes.push(write);

    // EmissiveTriangles
    let info = [vk::DescriptorBufferInfo {
        buffer: emissive_triangles,
        offset: 0,
        range: vk::WHOLE_SIZE,
    }];
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: 12,
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

    unsafe { vk.device.update_descriptor_sets(&descriptor_writes, &[]) };
}
