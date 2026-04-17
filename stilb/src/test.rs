#[cfg(test)]
mod tests {
    use ash::vk;
    use shaders::get_test_shader;

    use crate::{bmp::save_bmp, math::*, mesh::GpuMesh, shader::Shader, texture2d::Texture2D, *};

    #[test]
    fn test_initialize() {
        let preview = true;

        let config = StilbConfig {
            is_preview: if preview { 1 } else { 0 },
            preview_width: 512,
            preview_height: 512,
        };

        let stilb = initialize(config);

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

        add_mesh(stilb, mesh);

        let stilb_obj = unsafe { &*stilb };
        let vk = &stilb_obj.vk;

        let cmd = vk.begin_temp_graphics_cmd();

        vk.end_temp_graphics_cmd(cmd);

        let mut texture = Texture2D::new(
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

        save_bmp("../temp/write.bmp", 2, 2, &pixels).unwrap();
        texture.set_pixels(vk, &pixels);
        let pixels_read = texture.read_pixels(vk);
        save_bmp("../temp/read.bmp", 2, 2, &pixels_read).unwrap();

        texture.destroy(vk);

        let mesh = &stilb_obj.meshes[0];

        let mut gpu_mesh = GpuMesh::new(vk, mesh);

        gpu_mesh.destroy(vk);

        let mut bindings = Vec::new();

        bindings.push(vk::DescriptorSetLayoutBinding {
            binding: 0,
            descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
            descriptor_count: 1,
            stage_flags: vk::ShaderStageFlags::COMPUTE,
            ..Default::default()
        });

        let push_constant_ranges = vk::PushConstantRange::default();
        let specialization_info = vk::SpecializationInfo::default();

        let mut shader = Shader::new(
            vk,
            get_test_shader(),
            &bindings,
            &[push_constant_ranges],
            &specialization_info,
        );

        shader.destroy(vk);

        deinitialize(stilb);
    }
}
