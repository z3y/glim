use std::ffi::CStr;

use ash::vk::{self, Handle};
use shaders::*;

use crate::{
    as_bytes, math::Vector3, shader_bindings::*, shaders::bake_direct::BakeDirectPushConstants,
    texture2d::Texture2D, vulkan_context::VulkanContext,
};

pub struct ComputeShader {
    module: vk::ShaderModule,
    pub pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
    pub descriptor_set: vk::DescriptorSet,
    set_layout: vk::DescriptorSetLayout,
}

impl ComputeShader {
    pub fn null() -> Self {
        Self {
            module: vk::ShaderModule::null(),
            pipeline: vk::Pipeline::null(),
            pipeline_layout: vk::PipelineLayout::null(),
            descriptor_set: vk::DescriptorSet::null(),
            set_layout: vk::DescriptorSetLayout::null(),
        }
    }

    pub fn new(
        vk: &VulkanContext,
        code: &[u32],
        bindings: &[vk::DescriptorSetLayoutBinding],
        push_constant_ranges: &[vk::PushConstantRange],
        specialization_info: &vk::SpecializationInfo,
    ) -> Self {
        let create_info = vk::DescriptorSetLayoutCreateInfo::default().bindings(bindings);

        let set_layout =
            unsafe { vk.device.create_descriptor_set_layout(&create_info, None) }.unwrap();

        let set_layouts = [set_layout];

        let create_info = vk::ShaderModuleCreateInfo::default().code(code);

        let module = unsafe { vk.device.create_shader_module(&create_info, None) }.unwrap();

        let create_info = vk::PipelineLayoutCreateInfo::default()
            .push_constant_ranges(push_constant_ranges)
            .set_layouts(&set_layouts);

        let pipeline_layout =
            unsafe { vk.device.create_pipeline_layout(&create_info, None) }.unwrap();

        const NAME: &CStr = c"main";

        let mut stage = vk::PipelineShaderStageCreateInfo {
            stage: vk::ShaderStageFlags::COMPUTE,
            module,
            p_name: NAME.as_ptr(),
            ..Default::default()
        };
        stage = stage.specialization_info(&specialization_info);

        let create_info = vk::ComputePipelineCreateInfo {
            stage,
            layout: pipeline_layout,
            ..Default::default()
        };

        let pipeline = unsafe {
            vk.device
                .create_compute_pipelines(vk::PipelineCache::null(), &[create_info], None)
                .unwrap()
        }[0];

        let mut allocate_info = vk::DescriptorSetAllocateInfo {
            descriptor_pool: vk.descriptor_pool,
            descriptor_set_count: 1,
            ..Default::default()
        };

        allocate_info = allocate_info.set_layouts(&set_layouts);

        let descriptor_set =
            unsafe { vk.device.allocate_descriptor_sets(&allocate_info) }.unwrap()[0];

        Self {
            module,
            pipeline_layout,
            pipeline,
            set_layout,
            descriptor_set,
        }
    }

    pub fn destroy(&mut self, vk: &VulkanContext) {
        assert!(!self.module.is_null());
        assert!(!self.pipeline.is_null());
        assert!(!self.pipeline_layout.is_null());
        assert!(!self.set_layout.is_null());

        unsafe {
            vk.device.destroy_shader_module(self.module, None);
            vk.device.destroy_pipeline(self.pipeline, None);
            vk.device
                .destroy_pipeline_layout(self.pipeline_layout, None);

            vk.device
                .destroy_descriptor_set_layout(self.set_layout, None);
        };

        self.module = vk::ShaderModule::null();
        self.pipeline = vk::Pipeline::null();
        self.pipeline_layout = vk::PipelineLayout::null();
        self.set_layout = vk::DescriptorSetLayout::null();
    }
}

#[repr(C)]
pub struct InitFromCameraPushConstants {
    pub camera_position: Vector3,
    pub fov_half_tan: f32,

    pub camera_direction: Vector3,
    pub pad: u32,
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

#[repr(C)]
pub struct BakeSHPushConstants {
    pub lights_count: u32,
    pub max_samples: u32,
    pub sample_index: u32,
    pub probes_count: u32,
}

#[repr(C)]
pub struct BakeBouncePushConstants {
    pub width: u32,
    pub height: u32,
    pub sample_index: u32,
    pub max_samples: u32,

    pub bounce_index: u32,
    pub pad0: u32,
    pub pad1: u32,
    pub pad2: u32,
}

pub fn create_specialization_map_entries() -> [vk::SpecializationMapEntry; 7] {
    let size = std::mem::size_of::<u32>();

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
    ]
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct LightmapInfo {
    pub resolution: [u32; 2],
    pub compaction_offset: u32,
    pub pad: u32,
}

pub struct SpecializationConstants {
    pub use_camera: u32, // unused
    pub light_falloff_type: u32,
    pub transparent_primitive_offset: u32,
    pub emissive_triangles_count: u32,
    pub multiple_importance_sampling: u32,
    pub lightmap_group_count: u32,
    pub lightmap_mode: u32,
}

// fn create_specialization_constants(
//     mis: bool,
//     light_falloff_type: LightFalloffType,
//     transparent_primitive_offset: u32,
//     emissive_triangles_count: u32,
// ) -> [u32; 5] {
//     let mis: u32 = if mis { 1 } else { 0 };
//     [
//         0,
//         light_falloff_type as u32,
//         transparent_primitive_offset,
//         emissive_triangles_count,
//         mis,
//     ]
// }

pub fn load_init_from_camera_shader(
    vk: &VulkanContext,
    constants: &SpecializationConstants,
) -> ComputeShader {
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
        size: std::mem::size_of::<InitFromCameraPushConstants>() as u32,
    }];

    ComputeShader::new(
        vk,
        get_init_from_camera_shader(),
        &bindings,
        &push_constant_ranges,
        &specialization_info,
    )
}

pub fn update_init_from_camera_shader(
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

pub fn load_preview_shader(
    vk: &VulkanContext,
    constants: &SpecializationConstants,
) -> ComputeShader {
    let mut bindings = Vec::new();

    bind_tlas(&mut bindings);
    bind_visibility(&mut bindings);
    bind_albedos(&mut bindings, constants.lightmap_group_count);
    bind_emissions(&mut bindings, constants.lightmap_group_count);
    bind_lightmap_diffuse(&mut bindings);
    bind_indices(&mut bindings);
    bind_vertices(&mut bindings);
    bind_lights(&mut bindings);
    bind_emissive_triangles(&mut bindings);

    let map_entries = create_specialization_map_entries();
    let data_bytes = as_bytes(constants);
    let specialization_info = vk::SpecializationInfo::default()
        .map_entries(&map_entries)
        .data(data_bytes);

    let push_constant_ranges = [vk::PushConstantRange {
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        offset: 0,
        size: std::mem::size_of::<PreviewPushConstants>() as u32,
    }];

    ComputeShader::new(
        vk,
        get_preview_shader(),
        &bindings,
        &push_constant_ranges,
        &specialization_info,
    )
}

pub fn load_adjust_samples_shader(
    vk: &VulkanContext,
    constants: &SpecializationConstants,
) -> ComputeShader {
    let mut bindings = Vec::new();

    bind_tlas(&mut bindings);
    bind_compacted_visibility_buffer(&mut bindings);
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
        size: std::mem::size_of::<BakeDirectPushConstants>() as u32,
    }];

    ComputeShader::new(
        vk,
        get_adjust_samples_shader(),
        &bindings,
        &push_constant_ranges,
        &specialization_info,
    )
}

pub fn load_bake_bounce_shader(
    vk: &VulkanContext,
    constants: &SpecializationConstants,
) -> ComputeShader {
    let mut bindings = Vec::new();

    bind_tlas(&mut bindings);
    bind_visibility(&mut bindings);
    bind_albedos(&mut bindings, constants.lightmap_group_count);
    bind_lightmap_diffuse(&mut bindings);
    bind_indices(&mut bindings);
    bind_vertices(&mut bindings);

    let map_entries = create_specialization_map_entries();
    let data_bytes = as_bytes(constants);
    let specialization_info = vk::SpecializationInfo::default()
        .map_entries(&map_entries)
        .data(data_bytes);

    let push_constant_ranges = [vk::PushConstantRange {
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        offset: 0,
        size: std::mem::size_of::<BakeBouncePushConstants>() as u32,
    }];

    ComputeShader::new(
        vk,
        get_bake_bounce_shader(),
        &bindings,
        &push_constant_ranges,
        &specialization_info,
    )
}

pub fn load_bake_light_probes_shader(
    vk: &VulkanContext,
    constants: &SpecializationConstants,
) -> ComputeShader {
    let mut bindings = Vec::new();

    bind_tlas(&mut bindings);
    bind_albedos(&mut bindings, constants.lightmap_group_count);
    bind_emissions(&mut bindings, constants.lightmap_group_count);
    bind_probe_sh(&mut bindings);
    bind_indices(&mut bindings);
    bind_vertices(&mut bindings);
    bind_lights(&mut bindings);
    bind_compacted_lightmap(&mut bindings);
    bind_compaction_buffer(&mut bindings);
    bind_lightmap_info(&mut bindings);

    let push_constant_ranges = [vk::PushConstantRange {
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        offset: 0,
        size: std::mem::size_of::<BakeSHPushConstants>() as u32,
    }];

    let map_entries = create_specialization_map_entries();
    let data_bytes = as_bytes(constants);
    let specialization_info = vk::SpecializationInfo::default()
        .map_entries(&map_entries)
        .data(data_bytes);

    ComputeShader::new(
        vk,
        get_bake_sh_shader(),
        &bindings,
        &push_constant_ranges,
        &specialization_info,
    )
}

pub fn update_bake_light_probes_shader(
    vk: &VulkanContext,
    shader: &ComputeShader,
    tlas: vk::AccelerationStructureKHR,
    probes: vk::Buffer,
    albedos: &[vk::ImageView],
    emissions: &[vk::ImageView],
    compacted_lightmap: vk::Buffer,
    indices: vk::Buffer,
    vertices: vk::Buffer,
    lights: vk::Buffer,
    compaction: vk::Buffer,
    lightmap_info: vk::Buffer,
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

    // ProbeSH
    let info = [vk::DescriptorBufferInfo {
        buffer: probes,
        offset: 0,
        range: vk::WHOLE_SIZE,
    }];
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: 7,
        descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
        ..Default::default()
    };
    write = write.buffer_info(&info);
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

    // CompactionBuffer
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

pub fn update_preview_shader(
    vk: &VulkanContext,
    shader: &ComputeShader,
    tlas: vk::AccelerationStructureKHR,
    target_visibility: vk::ImageView,
    albedos: &[vk::ImageView],
    emissions: &[vk::ImageView],
    target_diffuse: vk::ImageView,
    indices: vk::Buffer,
    vertices: vk::Buffer,
    lights: vk::Buffer,
    emissive_triangles: vk::Buffer,
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
        image_view: target_visibility,
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

    // LightmapDiffuse
    let info = [vk::DescriptorImageInfo {
        image_view: target_diffuse,
        image_layout: vk::ImageLayout::GENERAL,
        ..Default::default()
    }];
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: 4,
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

    unsafe { vk.device.update_descriptor_sets(&descriptor_writes, &[]) };
}

pub fn update_adjust_samples_shader(
    vk: &VulkanContext,
    shader: &ComputeShader,
    tlas: vk::AccelerationStructureKHR,
    compacted_visibility: vk::Buffer,
    albedos: &[vk::ImageView],
    indices: vk::Buffer,
    vertices: vk::Buffer,
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

pub fn update_bake_bounce_shader(
    vk: &VulkanContext,
    shader: &ComputeShader,
    tlas: vk::AccelerationStructureKHR,
    target_visibility: vk::ImageView,
    albedos: &[vk::ImageView],
    target_diffuse: vk::ImageView,
    indices: vk::Buffer,
    vertices: vk::Buffer,
    dominant_direction: vk::Buffer,
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
        image_view: target_visibility,
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

    // LightmapDiffuse
    let info = [vk::DescriptorImageInfo {
        image_view: target_diffuse,
        image_layout: vk::ImageLayout::GENERAL,
        ..Default::default()
    }];
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: 4,
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

    // DominantDirection
    let info = [vk::DescriptorBufferInfo {
        buffer: dominant_direction,
        offset: 0,
        range: vk::WHOLE_SIZE,
    }];
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: 14,
        descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
        ..Default::default()
    };
    write = write.buffer_info(&info);
    descriptor_writes.push(write);

    unsafe { vk.device.update_descriptor_sets(&descriptor_writes, &[]) };
}
