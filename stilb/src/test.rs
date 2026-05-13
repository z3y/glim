#[cfg(test)]
mod tests {
    use crate::bindings::*;
    use crate::bmp::save_bmp;
    use crate::{lights::LightType, math::*, mesh::Vertex, *};

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
            let pos = vert.position;
            vert.position.x = pos.x;
            vert.position.y = pos.z;
            vert.position.z = -pos.y;

            let norm = vert.normal;
            vert.normal.x = norm.x;
            vert.normal.y = norm.z;
            vert.normal.z = -norm.y;

            vert.uv_y = 1.0 - vert.uv_y;
        }

        let indices: Vec<u32> = (0..vertices.len() as u32).collect();

        Mesh { vertices, indices }
    }

    pub fn load_tga(path: &str) -> std::io::Result<(u32, u32, Vec<f32>)> {
        let img = image::open(path)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
            .to_rgba8();

        let width = img.width();
        let height = img.height();
        let pixels = img.into_raw().iter().map(|&b| b as f32 / 255.0).collect();

        Ok((width, height, pixels))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn test_save_callback(data: ReadbackData) {
        let pixels = unsafe { std::slice::from_raw_parts(data.pixels, data.pixels_count as usize) };

        let file_name = format!("../temp/diffuse_lightmap{}.bmp", data.group_index);
        save_bmp(file_name.as_str(), data.width, data.height, &pixels).unwrap();
    }

    #[test]
    fn test_render() {
        let preview_settings = LightmapSettings {
            width: 1024,
            height: 1024,
            max_samples: 1024,
            bounce_count: 3,
            denoise: false,
        };

        let mut config = StilbConfig {
            coordinate_system: CoordinateSystem::Default,
            is_preview: true,
            camera_position: Vector3::new(0.0, 0.0, 5.0),
            camera_forward: Vector3::FORWARD,
            preview_settings,
            throttle_preview_ms: 0,
            callback: test_save_callback,
        };

        config.camera_forward = Vector3 {
            x: -0.42446527,
            y: -0.4595601,
            z: -0.7801498,
        };

        config.camera_position = Vector3 {
            x: 1.890761,
            y: 0.4439021,
            z: 1.685063,
        };

        let app = app_new(config);
        // app
        let mut mesh = get_test_mesh_moneky();

        let mut offset = 0.0;
        for _ in 0..1 {
            {
                let app = unsafe { &mut *app };
                app.cpu_mesh.merge_mesh(&mesh);
            }

            // app_add_light(
            //     app,
            //     Light {
            //         ty: LightType::Point,
            //         position: Vector3 {
            //             x: 0.0 + offset,
            //             y: 1.0,
            //             z: 0.0,
            //         },
            //         direction: Vector3::ZERO,
            //         range: 5.0,
            //         color: Vector3::new(1.0, 1.0, 1.0) * 1.0,
            //         shadow_radius_or_angle: 0.1,
            //     },
            // );

            offset += 5.0;
            for m in &mut mesh.vertices {
                m.position.x += 5.0;
            }
        }

        // app_add_light(
        //     app,
        //     Light {
        //         ty: lights::LightType::Directional,
        //         position: Vector3 {
        //             x: 0.0,
        //             y: 1.0,
        //             z: 0.0,
        //         },
        //         direction: Vector3::new(0.5, -1.0, 0.5).normalize(),
        //         range: 0.0,
        //         color: Vector3::new(1.0, 1.0, 1.0),
        //         shadow_radius_or_angle: 0.1,
        //     },
        // );

        let (w, h, emission_pixels) = load_tga("..\\textures\\emission.tga").unwrap();
        let albedo_pixels = vec![255; (w * h * 4) as usize];
        // let emission_pixels = vec![0.0; (w * h * 4) as usize];

        let settings = LightmapSettings {
            width: w,
            height: h,
            bounce_count: 2,
            max_samples: 256,
            denoise: true,
        };

        app_add_lightmap_group(
            app,
            settings,
            albedo_pixels.as_ptr(),
            albedo_pixels.len() as u32,
            emission_pixels.as_ptr(),
            emission_pixels.len() as u32,
        );

        app_run(app);

        app_destroy(app);
    }
}
