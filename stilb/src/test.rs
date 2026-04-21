#[cfg(test)]
mod tests {
    use ash::vk;

    use crate::{
        bmp::save_bmp,
        math::*,
        mesh::GpuMesh,
        shader::{load_shader_test, update_test_shader},
        texture2d::Texture2D,
        *,
    };

    fn get_test_config() -> StilbConfig {
        let preview = true;

        StilbConfig {
            is_preview: if preview { 1 } else { 0 },
            preview_width: 512,
            preview_height: 512,
        }
    }

    fn get_test_mesh() -> Mesh {
        let vertices = vec![
            Vector3::new(-0.5, 0.0, -0.5),
            Vector3::new(0.5, 0.0, -0.5),
            Vector3::new(0.5, 0.0, 0.5),
            Vector3::new(-0.5, 0.0, 0.5),
        ];

        let normals = vec![
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        ];

        let uvs = vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(1.0, 0.0),
            Vector2::new(1.0, 1.0),
            Vector2::new(0.0, 1.0),
        ];

        let indices: Vec<u32> = vec![0, 1, 2, 2, 3, 0];

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

        // #[rustfmt::skip]
        // let pixels: [f32; 16] = [
        //     1.0, 0.0, 0.0, 1.0,
        //     0.0, 1.0, 0.0, 1.0,
        //     0.0, 0.0, 1.0, 1.0,
        //     1.0, 1.0, 0.0, 1.0,
        // ];

        let mesh = &stilb_obj.meshes[0];

        let mut gpu_mesh = GpuMesh::new(vk, mesh);

        let mut test_shader = load_shader_test(vk);
        update_test_shader(vk, &test_shader, texture.view);

        let cmd = vk.begin_temp_cmd();

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

            vk.end_temp_cmd(cmd);
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

        run(stilb);

        deinitialize(stilb);
    }
}
