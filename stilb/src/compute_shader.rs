use std::ffi::CStr;

use ash::vk::{self, Handle};
use shaders::{get_bake_shader, get_init_from_camera_shader, get_test_shader};

use crate::{math::Vector3, texture2d::Texture2D, vulkan_context::VulkanContext};

pub struct ComputeShader {
    module: vk::ShaderModule,
    pub pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
    pub descriptor_set: vk::DescriptorSet,
    set_layout: vk::DescriptorSetLayout,
}

impl ComputeShader {
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

pub fn load_init_from_camera_shader(vk: &VulkanContext) -> ComputeShader {
    let mut bindings = Vec::new();

    // VisibilityBuffer
    bindings.push(vk::DescriptorSetLayoutBinding {
        binding: 2,
        descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        ..Default::default()
    });

    // TopLevelAS
    bindings.push(vk::DescriptorSetLayoutBinding {
        binding: 0,
        descriptor_type: vk::DescriptorType::ACCELERATION_STRUCTURE_KHR,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        ..Default::default()
    });

    let specialization_info = vk::SpecializationInfo::default();

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

    unsafe { vk.device.update_descriptor_sets(&descriptor_writes, &[]) };
}

pub fn load_shader_test(vk: &VulkanContext) -> ComputeShader {
    let mut bindings = Vec::new();

    bindings.push(vk::DescriptorSetLayoutBinding {
        binding: 0,
        descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        ..Default::default()
    });

    let specialization_info = vk::SpecializationInfo::default();

    ComputeShader::new(vk, get_test_shader(), &bindings, &[], &specialization_info)
}

pub fn update_test_shader(vk: &VulkanContext, shader: &ComputeShader, binding0: vk::ImageView) {
    let image_info = [vk::DescriptorImageInfo {
        image_view: binding0,
        image_layout: vk::ImageLayout::GENERAL,
        ..Default::default()
    }];

    let mut image_write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: 0,
        descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
        ..Default::default()
    };
    image_write = image_write.image_info(&image_info);

    let descriptor_writes = [image_write];

    unsafe { vk.device.update_descriptor_sets(&descriptor_writes, &[]) };
}

// pub struct BakePushConstants
// {
//     Vertex *vertices;
//     uint *indices;

//     Light *lights;
//     uint lightCount;
//     float pad0;

//     uint sampleIndex;
//     uint width;
//     uint height;
//     float pad1;
// };

#[repr(C)]
pub struct BakePushConstants {
    pub vertices: vk::DeviceAddress,
    pub indices: vk::DeviceAddress,

    pub lights: vk::DeviceAddress,
    pub lights_count: u32,
    pub pad0: u32,

    pub sample_index: u32,
    pub width: u32,
    pub height: u32,
    pub pad1: u32,
}

pub fn load_bake_lights_shader(vk: &VulkanContext) -> ComputeShader {
    let mut bindings = Vec::new();

    // TopLevelAS
    bindings.push(vk::DescriptorSetLayoutBinding {
        binding: 0,
        descriptor_type: vk::DescriptorType::ACCELERATION_STRUCTURE_KHR,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        ..Default::default()
    });

    // VisibilityBuffer
    bindings.push(vk::DescriptorSetLayoutBinding {
        binding: 2,
        descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        ..Default::default()
    });

    // Albedo
    bindings.push(vk::DescriptorSetLayoutBinding {
        binding: 3,
        descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        ..Default::default()
    });

    // Target0
    bindings.push(vk::DescriptorSetLayoutBinding {
        binding: 4,
        descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
        descriptor_count: 1,
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        ..Default::default()
    });

    let specialization_info = vk::SpecializationInfo::default();

    let push_constant_ranges = [vk::PushConstantRange {
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        offset: 0,
        size: std::mem::size_of::<BakePushConstants>() as u32,
    }];

    ComputeShader::new(
        vk,
        get_bake_shader(),
        &bindings,
        &push_constant_ranges,
        &specialization_info,
    )
}

pub fn update_bake_lights_shader(
    vk: &VulkanContext,
    shader: &ComputeShader,
    tlas: vk::AccelerationStructureKHR,
    visibility: &Texture2D,
    albedo: &Texture2D,
    lightmap_diffuse: &Texture2D,
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

    // Albedo
    let info = [vk::DescriptorImageInfo {
        image_view: albedo.view(),
        image_layout: vk::ImageLayout::GENERAL,
        ..Default::default()
    }];
    let mut write = vk::WriteDescriptorSet {
        dst_set: shader.descriptor_set,
        dst_binding: 3,
        descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
        ..Default::default()
    };
    write = write.image_info(&info);
    descriptor_writes.push(write);

    // LightmapDiffuse
    let info = [vk::DescriptorImageInfo {
        image_view: lightmap_diffuse.view(),
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

    unsafe { vk.device.update_descriptor_sets(&descriptor_writes, &[]) };
}
