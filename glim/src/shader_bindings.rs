use ash::vk;

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

pub fn bind_probe_sh(bindings: &mut Vec<vk::DescriptorSetLayoutBinding<'_>>) {
    bindings.push(vk::DescriptorSetLayoutBinding {
        binding: 7,
        descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        ..Default::default()
    });
}

pub fn bind_indices(bindings: &mut Vec<vk::DescriptorSetLayoutBinding<'_>>) {
    bindings.push(vk::DescriptorSetLayoutBinding {
        binding: 8,
        descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        ..Default::default()
    });
}

pub fn bind_vertices(bindings: &mut Vec<vk::DescriptorSetLayoutBinding<'_>>) {
    bindings.push(vk::DescriptorSetLayoutBinding {
        binding: 9,
        descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        ..Default::default()
    });
}

pub fn bind_lights(bindings: &mut Vec<vk::DescriptorSetLayoutBinding<'_>>) {
    bindings.push(vk::DescriptorSetLayoutBinding {
        binding: 10,
        descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        ..Default::default()
    });
}

pub fn bind_emissive_triangles(bindings: &mut Vec<vk::DescriptorSetLayoutBinding<'_>>) {
    bindings.push(vk::DescriptorSetLayoutBinding {
        binding: 12,
        descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        ..Default::default()
    });
}

pub fn bind_compaction_buffer(bindings: &mut Vec<vk::DescriptorSetLayoutBinding<'_>>) {
    bindings.push(vk::DescriptorSetLayoutBinding {
        binding: 15,
        descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        ..Default::default()
    });
}

pub fn bind_compacted_visibility_buffer(bindings: &mut Vec<vk::DescriptorSetLayoutBinding<'_>>) {
    bindings.push(vk::DescriptorSetLayoutBinding {
        binding: 16,
        descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        ..Default::default()
    });
}

pub fn bind_decompact_target(bindings: &mut Vec<vk::DescriptorSetLayoutBinding<'_>>) {
    bindings.push(vk::DescriptorSetLayoutBinding {
        binding: 17,
        descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        ..Default::default()
    });
}

pub fn bind_compacted_lightmap(bindings: &mut Vec<vk::DescriptorSetLayoutBinding<'_>>) {
    bindings.push(vk::DescriptorSetLayoutBinding {
        binding: 18,
        descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        ..Default::default()
    });
}

pub fn bind_lightmap_info(bindings: &mut Vec<vk::DescriptorSetLayoutBinding<'_>>) {
    bindings.push(vk::DescriptorSetLayoutBinding {
        binding: 19,
        descriptor_type: vk::DescriptorType::UNIFORM_BUFFER,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        ..Default::default()
    });
}
