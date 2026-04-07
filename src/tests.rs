#[cfg(test)]
mod tests {
    use crate::{
        vulkan_cmd::{begin_temp_graphics_cmd, end_temp_graphics_cmd},
        *,
    };

    #[test]
    fn test_initialize() {
        let config = StilbConfig {
            is_preview: 0,
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

        let mesh = StilbMesh {
            vertices: vertices.as_ptr(),
            normals: normals.as_ptr(),
            uvs: uvs.as_ptr(),
            vertices_length: vertices.len() as u32,
            indices: indices.as_ptr(),
            indices_length: indices.len() as u32,
        };

        add_mesh(stilb, mesh);

        let stilb_obj = unsafe { &*stilb };
        let vk = &stilb_obj.vk;

        let cmd = begin_temp_graphics_cmd(vk);

        end_temp_graphics_cmd(vk, cmd);

        deinitialize(stilb);
    }
}
