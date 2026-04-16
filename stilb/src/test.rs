#[cfg(test)]
mod tests {
    use ash::vk;
    use shaders::TEST_COMPUTE;

    use crate::{math::*, texture2d::Texture2D, *};

    #[test]
    fn test_initialize() {
        let preview = true;

        println!("Loaded shader bytes: {} bytes", TEST_COMPUTE.len());
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

        let mesh = RawMesh {
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
            vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::TRANSFER_SRC,
        );

        #[rustfmt::skip]
        let pixels: [f32; 16] = [
            0.0, 0.0, 0.0, 0.0,
            1.0, 1.0, 1.0, 1.0,
            0.0, 0.0, 0.0, 0.0,
            1.0, 1.0, 1.0, 1.0,
        ];

        texture.set_pixels(vk, &pixels);

        texture.destroy(vk);

        deinitialize(stilb);
    }
}
