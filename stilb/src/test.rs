#[cfg(test)]
mod tests {
    use ash::vk;

    use crate::{
        bmp::save_bmp,
        compute_shader::{load_shader_test, update_test_shader},
        graphics_shader::{VisibilityPushConstants, create_visibility_shader},
        math::*,
        mesh::{GpuMesh, Vertex},
        texture2d::Texture2D,
        *,
    };

    fn get_test_config() -> StilbConfig {
        StilbConfig {
            is_preview: true,
            preview_width: 512,
            preview_height: 512,
        }
    }

    fn get_test_mesh_moneky() -> Mesh {
        let bytes = include_bytes!("../../meshes/monkey.bin");

        assert_eq!(bytes.len() % std::mem::size_of::<Vertex>(), 0);

        let mut vertices: Vec<Vertex> = unsafe {
            let (prefix, mid, suffix) = bytes.align_to::<Vertex>();
            if !prefix.is_empty() || !suffix.is_empty() {
                // If it wasn't perfectly aligned, we have to copy it manually
                // or handle the offset. But for a test, we can just do this:
                let mut v = Vec::with_capacity(bytes.len() / std::mem::size_of::<Vertex>());
                std::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    v.as_mut_ptr() as *mut u8,
                    bytes.len(),
                );
                v.set_len(bytes.len() / std::mem::size_of::<Vertex>());
                v
            } else {
                mid.to_vec()
            }
        };

        for vert in &mut vertices {
            let temp = vert.position.y;
            vert.position.y = vert.position.z;
            vert.position.z = temp;
            vert.position.x = -vert.position.x;

            let temp = vert.normal.y;
            vert.normal.y = vert.normal.z;
            vert.normal.z = temp;
            vert.normal.x = -vert.normal.x;
        }

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
        let app = app_initialize(config);
        let app = unsafe { &mut *app };
        let vk = &mut app.vk;

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
        update_test_shader(vk, &test_shader, texture.view());

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
        }

        vk.end_single_use_cmd(cmd);

        let pixels_read = texture.read_pixels(vk);
        save_bmp(
            "../temp/read.bmp",
            texture.width(),
            texture.height(),
            &pixels_read,
        )
        .unwrap();

        texture.destroy(vk);
        test_shader.destroy(vk);

        app_deinitialize(app);
    }

    #[test]
    fn test_visibility_rasterize() {
        let config = get_test_config();
        let app = app_initialize(config);
        let app = unsafe { &mut *app };
        let vk = &mut app.vk;

        let mesh = get_test_mesh_moneky();

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

        let mut gpu_mesh = GpuMesh::new(vk, &mesh);

        let mut shader = create_visibility_shader(vk, &visibility);

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

        let push = VisibilityPushConstants {
            vertices: gpu_mesh.vertex_address(),
            indices: gpu_mesh.index_address(),
            width: visibility.width(),
            height: visibility.height(),
            pad0: 0,
            pad1: 0,
        };

        let constants_bytes = unsafe {
            std::slice::from_raw_parts(
                &push as *const VisibilityPushConstants as *const u8,
                std::mem::size_of::<VisibilityPushConstants>(),
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

        app_deinitialize(app);
    }

    #[test]
    fn test_headless_bake() {
        let config = StilbConfig {
            is_preview: false,
            preview_width: 512,
            preview_height: 512,
        };

        let app = app_initialize(config);
        let app = unsafe { &mut *app };

        let mesh = get_test_mesh_moneky();

        app.cpu_meshes.push(mesh);

        app.cpu_lights.push(Light {
            ty: lights::LightType::Point,
            position: Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
            direction: Vector3::ZERO,
            range: 5.0,
            color: Vector3::ONE,
            shadow_range_or_angle: 0.1,
        });

        app_run(app);

        app_deinitialize(app);
    }

    #[test]
    fn test_preview() {
        let config = StilbConfig {
            is_preview: true,
            preview_width: 512,
            preview_height: 512,
        };

        let app = app_initialize(config);
        let app = unsafe { &mut *app };

        let mesh = get_test_mesh_moneky();

        app.cpu_meshes.push(mesh);

        app.cpu_lights.push(Light {
            ty: lights::LightType::Point,
            position: Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
            direction: Vector3::ZERO,
            range: 5.0,
            color: Vector3::ONE,
            shadow_range_or_angle: 0.1,
        });

        app_run(app);

        app_deinitialize(app);
    }
}
