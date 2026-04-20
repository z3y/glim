#[cfg(test)]
mod tests {
    use ash::vk;
    use shaders::get_test_shader;

    use crate::{bmp::save_bmp, math::*, mesh::GpuMesh, shader::Shader, texture2d::Texture2D, *};

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
            256,
            256,
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

        // save_bmp("../temp/write.bmp", 2, 2, &pixels).unwrap();
        // texture.set_pixels(vk, &pixels);
        // let pixels_read = texture.read_pixels(vk);
        // save_bmp("../temp/read.bmp", 2, 2, &pixels_read).unwrap();

        let mesh = &stilb_obj.meshes[0];

        let mut gpu_mesh = GpuMesh::new(vk, mesh);

        let mut bindings = Vec::new();

        bindings.push(vk::DescriptorSetLayoutBinding {
            binding: 0,
            descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
            descriptor_count: 1,
            stage_flags: vk::ShaderStageFlags::COMPUTE,
            ..Default::default()
        });

        let specialization_info = vk::SpecializationInfo::default();

        let mut shader = Shader::new(vk, get_test_shader(), &bindings, &[], &specialization_info);

        let mut descriptor_writes = Vec::new();

        let image_info = [vk::DescriptorImageInfo {
            image_view: texture.view,
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

        descriptor_writes.push(image_write);

        let cmd = vk.compute_cmd;
        unsafe {
            vk.device.update_descriptor_sets(&descriptor_writes, &[]);

            vk.device
                .reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())
                .unwrap();

            let begin_info = vk::CommandBufferBeginInfo {
                flags: vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
                ..Default::default()
            };

            vk.device.begin_command_buffer(cmd, &begin_info).unwrap();

            let subresource_range = vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            };

            let barrier = vk::ImageMemoryBarrier {
                dst_access_mask: vk::AccessFlags::SHADER_WRITE,
                old_layout: texture.layout(),
                new_layout: vk::ImageLayout::GENERAL,
                image: texture.image,
                subresource_range,
                ..Default::default()
            };

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
                .cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, shader.pipeline);

            vk.device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                shader.pipeline_layout,
                0,
                &[shader.descriptor_set],
                &[],
            );

            let groups_x = (texture.width() + 7) / 8;
            let groups_y = (texture.height() + 7) / 8;
            vk.device.cmd_dispatch(cmd, groups_x, groups_y, 1);

            vk.device.end_command_buffer(cmd).unwrap();

            let cmds = [cmd];

            let submit_info = vk::SubmitInfo::default().command_buffers(&cmds);

            vk.device
                .queue_submit(vk.compute_queue, &[submit_info], vk::Fence::null())
                .unwrap();

            vk.device.queue_wait_idle(vk.compute_queue).unwrap();
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
        shader.destroy(vk);

        run(stilb);

        deinitialize(stilb);
    }
}
