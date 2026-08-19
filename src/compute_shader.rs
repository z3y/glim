use std::ffi::CStr;

use ash::vk::{self, Handle};

use crate::{as_bytes, shaders::*, vulkan_context::VulkanContext};

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
