#[cfg(test)]
mod tests {
    use ash::vk;
    use shaders::{
        get_visibility_fragment_shader, get_visibility_geometry_shader,
        get_visibility_vertex_shader,
    };

    use crate::{
        bmp::save_bmp,
        compute_shader::{load_shader_test, update_test_shader},
        graphics_shader::GraphicsShader,
        math::*,
        mesh::{GpuMesh, Vertex},
        texture2d::Texture2D,
        *,
    };

    fn get_test_config() -> StilbConfig {
        let preview = false;

        StilbConfig {
            is_preview: if preview { 1 } else { 0 },
            preview_width: 512,
            preview_height: 512,
        }
    }

    fn get_test_mesh_moneky() -> Mesh {
        let bytes = include_bytes!("../../meshes/monkey.bin");

        assert_eq!(bytes.len() % std::mem::size_of::<Vertex>(), 0);

        let vertices: Vec<Vertex> = unsafe {
            let ptr = bytes.as_ptr() as *const Vertex;
            let len = bytes.len() / std::mem::size_of::<Vertex>();
            std::slice::from_raw_parts(ptr, len).to_vec()
        };

        let indices: Vec<u32> = (0..vertices.len() as u32).collect();

        Mesh { vertices, indices }
    }

    fn get_test_mesh() -> Mesh {
        let vertices = [
            Vector3::new(-0.5, 0.0, -0.5),
            Vector3::new(0.5, 0.0, -0.5),
            Vector3::new(0.5, 0.0, 0.5),
            Vector3::new(-0.5, 0.0, 0.5),
        ];

        let normals = [
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        ];

        let uvs = [
            Vector2::new(0.0, 0.0),
            Vector2::new(1.0, 0.0),
            Vector2::new(1.0, 1.0),
            Vector2::new(0.0, 1.0),
        ];

        let indices = [0, 2, 1, 2, 0, 3];

        assert!(uvs.len() == vertices.len());
        assert!(normals.len() == vertices.len());

        let mesh = FfiMesh {
            vertices: vertices.as_ptr(),
            normals: normals.as_ptr(),
            uvs: uvs.as_ptr(),
            indices: indices.as_ptr(),
            vertices_length: vertices.len() as u32,
            indices_length: indices.len() as u32,
        };

        Mesh::from_ffi_mesh(mesh)
    }

    #[test]
    fn test_initialize() {
        let config = get_test_config();

        let stilb = initialize(config);

        let stilb_obj = unsafe { &mut *stilb };
        let vk = &mut stilb_obj.vk;

        stilb_obj.meshes.push(get_test_mesh());
        let mesh = &stilb_obj.meshes[0];

        let mut texture2 = Texture2D::new(
            vk,
            2,
            2,
            vk::Format::R32G32B32A32_SFLOAT,
            vk::ImageUsageFlags::STORAGE
                | vk::ImageUsageFlags::TRANSFER_SRC
                | vk::ImageUsageFlags::TRANSFER_DST
                | vk::ImageUsageFlags::SAMPLED,
        );

        #[rustfmt::skip]
        let pixels: [f32; 16] = [
            1.0, 0.0, 0.0, 1.0,
            0.0, 1.0, 0.0, 1.0,
            0.0, 0.0, 1.0, 1.0,
            1.0, 1.0, 0.0, 1.0,
        ];

        texture2.set_pixels(vk, &pixels);

        let pixels_read = texture2.read_pixels(vk);

        save_bmp(
            "../temp/read2.bmp",
            texture2.width(),
            texture2.height(),
            &pixels_read,
        )
        .unwrap();

        texture2.destroy(vk);

        let mut gpu_mesh = GpuMesh::new(vk, mesh);

        let mut texture = Texture2D::new(
            vk,
            config.preview_width,
            config.preview_height,
            vk::Format::R32G32B32A32_SFLOAT,
            vk::ImageUsageFlags::STORAGE
                | vk::ImageUsageFlags::TRANSFER_SRC
                | vk::ImageUsageFlags::TRANSFER_DST
                | vk::ImageUsageFlags::SAMPLED,
        );

        let mut test_shader = load_shader_test(vk);
        update_test_shader(vk, &test_shader, texture.view);

        let cmd = vk.begin_single_use_cmd();

        unsafe {
            let barrier = texture.barrier(
                vk::ImageLayout::GENERAL,
                vk::AccessFlags::default(),
                vk::AccessFlags::SHADER_WRITE,
            );

            vk.device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            );

            vk.device
                .cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, test_shader.pipeline);

            vk.device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                test_shader.pipeline_layout,
                0,
                &[test_shader.descriptor_set],
                &[],
            );

            let groups_x = (texture.width() + 7) / 8;
            let groups_y = (texture.height() + 7) / 8;
            vk.device.cmd_dispatch(cmd, groups_x, groups_y, 1);

            vk.end_single_use_cmd(cmd);
        }

        let pixels_read = texture.read_pixels(vk);
        save_bmp(
            "../temp/read.bmp",
            texture.width(),
            texture.height(),
            &pixels_read,
        )
        .unwrap();

        gpu_mesh.destroy(vk);
        texture.destroy(vk);
        test_shader.destroy(vk);

        // run(stilb);

        deinitialize(stilb);
    }

    #[test]
    fn test_visibility_rasterize() {
        let config = get_test_config();
        let stilb = initialize(config);

        let stilb_obj = unsafe { &mut *stilb };
        let vk = &mut stilb_obj.vk;

        stilb_obj.meshes.push(get_test_mesh_moneky());
        let mesh = &stilb_obj.meshes[0];

        let mut visibility = Texture2D::new(
            vk,
            512,
            512,
            vk::Format::R32G32B32A32_SFLOAT,
            vk::ImageUsageFlags::STORAGE
                | vk::ImageUsageFlags::TRANSFER_SRC
                | vk::ImageUsageFlags::TRANSFER_DST
                | vk::ImageUsageFlags::SAMPLED
                | vk::ImageUsageFlags::COLOR_ATTACHMENT,
        );

        let mut gpu_mesh = GpuMesh::new(vk, mesh);

        #[repr(C)]
        struct PushConstants {
            vertices: *const Vertex,
            indices: *const u32,
            width: u32,
            height: u32,
            padding0: f32,
            padding1: f32,
        }

        let push_constant_ranges = [vk::PushConstantRange {
            stage_flags: vk::ShaderStageFlags::GEOMETRY
                | vk::ShaderStageFlags::FRAGMENT
                | vk::ShaderStageFlags::VERTEX,
            offset: 0,
            size: std::mem::size_of::<PushConstants>() as u32,
        }];

        let mut shader = GraphicsShader::new(
            vk,
            Some(get_visibility_vertex_shader()),
            Some(get_visibility_fragment_shader()),
            Some(get_visibility_geometry_shader()),
            &[],
            &push_constant_ranges,
            &vk::SpecializationInfo::default(),
            &visibility,
        );

        let cmd = vk.begin_single_use_cmd();

        let clear_values = [vk::ClearValue {
            color: vk::ClearColorValue {
                float32: [0.0, 0.0, 0.0, 0.0],
            },
        }];

        let mut render_pass_begin = vk::RenderPassBeginInfo {
            render_pass: shader.render_pass,
            framebuffer: shader.framebuffer,
            render_area: vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: vk::Extent2D {
                    width: visibility.width(),
                    height: visibility.height(),
                },
            },
            ..Default::default()
        };
        render_pass_begin = render_pass_begin.clear_values(&clear_values);

        let push = PushConstants {
            vertices: gpu_mesh.vertex_address() as _,
            indices: gpu_mesh.index_address() as _,
            width: visibility.width(),
            height: visibility.height(),
            padding0: 0.0,
            padding1: 0.0,
        };

        let constants_bytes = unsafe {
            std::slice::from_raw_parts(
                &push as *const PushConstants as *const u8,
                std::mem::size_of::<PushConstants>(),
            )
        };

        unsafe {
            vk.device
                .cmd_begin_render_pass(cmd, &render_pass_begin, vk::SubpassContents::INLINE);
            vk.device
                .cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, shader.pipeline);

            vk.device.cmd_push_constants(
                cmd,
                shader.pipeline_layout,
                vk::ShaderStageFlags::GEOMETRY
                    | vk::ShaderStageFlags::FRAGMENT
                    | vk::ShaderStageFlags::VERTEX,
                0,
                &constants_bytes,
            );

            vk.device.cmd_draw(cmd, mesh.indices.len() as u32, 25, 0, 0);

            vk.device.cmd_end_render_pass(cmd);
        }
        vk.end_single_use_cmd(cmd);

        let pixels_read = visibility.read_pixels(vk);
        save_bmp(
            "../temp/visibility.bmp",
            visibility.width(),
            visibility.height(),
            &pixels_read,
        )
        .unwrap();

        shader.destroy(vk);
        visibility.destroy(vk);
        gpu_mesh.destroy(vk);

        deinitialize(stilb);
    }
}
