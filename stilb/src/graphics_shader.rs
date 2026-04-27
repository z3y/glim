use std::ffi::CStr;

use ash::vk::{self, Handle};
use shaders::{
    get_init_from_bake_fragment_shader, get_init_from_bake_geometry_shader,
    get_init_from_bake_vertex_shader,
};

use crate::{mesh::Vertex, texture2d::Texture2D, vulkan_context::VulkanContext};

pub struct GraphicsShader {
    vertex_module: vk::ShaderModule,
    fragment_module: vk::ShaderModule,
    geometry_module: vk::ShaderModule,
    pub pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
    pub descriptor_set: vk::DescriptorSet,
    set_layout: vk::DescriptorSetLayout,
    pub framebuffer: vk::Framebuffer,
    pub render_pass: vk::RenderPass,
}

impl GraphicsShader {
    pub fn new(
        vk: &VulkanContext,
        vertex_spv: Option<&[u32]>,
        fragment_spv: Option<&[u32]>,
        geometry_spv: Option<&[u32]>,
        bindings: &[vk::DescriptorSetLayoutBinding],
        push_constant_ranges: &[vk::PushConstantRange],
        specialization_info: &vk::SpecializationInfo,
        target: &Texture2D,
    ) -> Self {
        // let create_info = vk::DescriptorSetLayoutCreateInfo::default().bindings(bindings);

        // let set_layout =
        //     unsafe { vk.device.create_descriptor_set_layout(&create_info, None) }.unwrap();

        // let set_layouts = [set_layout];

        let vertex_module = if let Some(spv) = vertex_spv {
            let create_info = vk::ShaderModuleCreateInfo::default().code(spv);
            unsafe { vk.device.create_shader_module(&create_info, None) }.unwrap()
        } else {
            vk::ShaderModule::null()
        };

        let fragment_module = if let Some(spv) = fragment_spv {
            let create_info = vk::ShaderModuleCreateInfo::default().code(spv);
            unsafe { vk.device.create_shader_module(&create_info, None) }.unwrap()
        } else {
            vk::ShaderModule::null()
        };

        let geometry_module = if let Some(spv) = geometry_spv {
            let create_info = vk::ShaderModuleCreateInfo::default().code(spv);
            unsafe { vk.device.create_shader_module(&create_info, None) }.unwrap()
        } else {
            vk::ShaderModule::null()
        };

        const ENTRY: &CStr = c"main";

        let vertex_stage = vk::PipelineShaderStageCreateInfo {
            stage: vk::ShaderStageFlags::VERTEX,
            module: vertex_module,
            p_name: ENTRY.as_ptr(),
            p_specialization_info: specialization_info,
            ..Default::default()
        };

        let fragment_stage = vk::PipelineShaderStageCreateInfo {
            stage: vk::ShaderStageFlags::FRAGMENT,
            module: fragment_module,
            p_name: ENTRY.as_ptr(),
            p_specialization_info: specialization_info,
            ..Default::default()
        };

        let geometry_stage = vk::PipelineShaderStageCreateInfo {
            stage: vk::ShaderStageFlags::GEOMETRY,
            module: geometry_module,
            p_name: ENTRY.as_ptr(),
            p_specialization_info: specialization_info,
            ..Default::default()
        };

        let mut stages = Vec::new();

        if vertex_spv.is_some() {
            stages.push(vertex_stage);
        }
        if fragment_spv.is_some() {
            stages.push(fragment_stage);
        }
        if geometry_spv.is_some() {
            stages.push(geometry_stage);
        }

        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();

        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo {
            topology: vk::PrimitiveTopology::TRIANGLE_LIST,
            ..Default::default()
        };

        let viewports = [vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: target.width() as f32,
            height: target.height() as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        }];

        let scissors = [vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: vk::Extent2D {
                width: target.width(),
                height: target.height(),
            },
        }];

        let viewport_state = vk::PipelineViewportStateCreateInfo::default()
            .scissors(&scissors)
            .viewports(&viewports);

        let rasterizer = vk::PipelineRasterizationStateCreateInfo {
            polygon_mode: vk::PolygonMode::FILL,
            cull_mode: vk::CullModeFlags::NONE,
            front_face: vk::FrontFace::COUNTER_CLOCKWISE,
            line_width: 1.0,
            ..Default::default()
        };

        let multisampling = vk::PipelineMultisampleStateCreateInfo {
            rasterization_samples: vk::SampleCountFlags::TYPE_1,
            ..Default::default()
        };

        let blend_attachments = [vk::PipelineColorBlendAttachmentState {
            color_write_mask: vk::ColorComponentFlags::R
                | vk::ColorComponentFlags::G
                | vk::ColorComponentFlags::B
                | vk::ColorComponentFlags::A,
            ..Default::default()
        }];

        let color_blending =
            vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend_attachments);

        let pipeline_layout =
            vk::PipelineLayoutCreateInfo::default().push_constant_ranges(&push_constant_ranges);
        // .set_layouts(&set_layouts);

        let pipeline_layout = unsafe {
            vk.device
                .create_pipeline_layout(&pipeline_layout, None)
                .unwrap()
        };

        let color_attachments = [vk::AttachmentDescription {
            format: vk::Format::R32G32B32A32_SFLOAT,
            samples: vk::SampleCountFlags::TYPE_1,
            load_op: vk::AttachmentLoadOp::CLEAR,
            store_op: vk::AttachmentStoreOp::STORE,
            initial_layout: vk::ImageLayout::UNDEFINED,
            final_layout: vk::ImageLayout::GENERAL,
            ..Default::default()
        }];

        let color_reference = [vk::AttachmentReference {
            attachment: 0,
            layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        }];

        let subpasses = [vk::SubpassDescription::default()
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .color_attachments(&color_reference)];

        let create_info = vk::RenderPassCreateInfo::default()
            .attachments(&color_attachments)
            .subpasses(&subpasses);

        let render_pass = unsafe { vk.device.create_render_pass(&create_info, None).unwrap() };

        let mut framebuffer_create = vk::FramebufferCreateInfo {
            render_pass,
            width: target.width(),
            height: target.height(),
            layers: 1,
            ..Default::default()
        };
        let attachments = [target.view()];
        framebuffer_create = framebuffer_create.attachments(&attachments);

        let framebuffer = unsafe {
            vk.device
                .create_framebuffer(&framebuffer_create, None)
                .unwrap()
        };

        let pipeline_infos = [vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&vertex_input)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_state)
            .rasterization_state(&rasterizer)
            .multisample_state(&multisampling)
            .color_blend_state(&color_blending)
            .layout(pipeline_layout)
            .render_pass(render_pass)];

        let pipeline = unsafe {
            vk.device
                .create_graphics_pipelines(vk::PipelineCache::null(), &pipeline_infos, None)
                .unwrap()[0]
        };

        // let mut allocate_info = vk::DescriptorSetAllocateInfo {
        //     descriptor_pool: vk.descriptor_pool,
        //     descriptor_set_count: 1,
        //     ..Default::default()
        // };
        // allocate_info = allocate_info.set_layouts(&set_layouts);

        // let descriptor_set =
        //     unsafe { vk.device.allocate_descriptor_sets(&allocate_info) }.unwrap()[0];

        let descriptor_set = vk::DescriptorSet::null();
        let set_layout = vk::DescriptorSetLayout::null();

        Self {
            vertex_module,
            fragment_module,
            geometry_module,
            pipeline,
            pipeline_layout,
            descriptor_set,
            set_layout,
            framebuffer,
            render_pass,
        }
    }

    pub fn destroy(&mut self, vk: &VulkanContext) {
        assert!(!self.pipeline.is_null());
        assert!(!self.pipeline_layout.is_null());
        assert!(!self.render_pass.is_null());
        assert!(!self.framebuffer.is_null());

        unsafe {
            if !self.vertex_module.is_null() {
                vk.device.destroy_shader_module(self.vertex_module, None);
            }
            if !self.fragment_module.is_null() {
                vk.device.destroy_shader_module(self.fragment_module, None);
            }
            if !self.geometry_module.is_null() {
                vk.device.destroy_shader_module(self.geometry_module, None);
            }
            vk.device.destroy_pipeline(self.pipeline, None);
            vk.device.destroy_framebuffer(self.framebuffer, None);
            vk.device.destroy_render_pass(self.render_pass, None);
            vk.device
                .destroy_pipeline_layout(self.pipeline_layout, None);

            if !self.set_layout.is_null() {
                vk.device
                    .destroy_descriptor_set_layout(self.set_layout, None);
            }
        };

        self.fragment_module = vk::ShaderModule::null();
        self.vertex_module = vk::ShaderModule::null();
        self.geometry_module = vk::ShaderModule::null();
        self.pipeline = vk::Pipeline::null();
        self.pipeline_layout = vk::PipelineLayout::null();
        self.set_layout = vk::DescriptorSetLayout::null();
        self.framebuffer = vk::Framebuffer::null();
        self.render_pass = vk::RenderPass::null();
    }
}

#[repr(C)]
pub struct VisibilityPushConstants {
    pub vertices: vk::DeviceAddress,
    pub indices: vk::DeviceAddress,

    pub width: u32,
    pub height: u32,
    pub pad0: u32,
    pub pad1: u32,
}

pub fn create_visibility_shader(vk: &mut VulkanContext, visibility: &Texture2D) -> GraphicsShader {
    let push_constant_ranges = [vk::PushConstantRange {
        stage_flags: vk::ShaderStageFlags::GEOMETRY
            | vk::ShaderStageFlags::FRAGMENT
            | vk::ShaderStageFlags::VERTEX,
        offset: 0,
        size: std::mem::size_of::<VisibilityPushConstants>() as u32,
    }];

    let shader = GraphicsShader::new(
        vk,
        Some(get_init_from_bake_vertex_shader()),
        Some(get_init_from_bake_fragment_shader()),
        Some(get_init_from_bake_geometry_shader()),
        &[],
        &push_constant_ranges,
        &vk::SpecializationInfo::default(),
        visibility,
    );
    shader
}
