#[cfg(test)]
mod tests {
    use crate::bindings::*;
    use crate::bmp::save_bmp;
    use crate::mesh::FfiMesh;
    use crate::{lights::LightType, math::*, *};

    #[test]
    fn test_preview() {
        let mut config = make_config();
        config.is_preview = true;
        test_render(config);
    }

    #[test]
    fn test_bake() {
        let mut config = make_config();
        config.is_preview = false;
        test_render(config);
    }

    fn test_render(mut config: StilbConfig) {
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

        // let mut offset = 0.0;
        // for _ in 0..1 {
        // {
        add_mesh(app, "..\\meshes\\monkey.glb").expect("failed to load mesh");
        // }

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

        //     offset += 5.0;
        //     for m in &mut mesh.vertices {
        //         m.position.x += 5.0;
        //     }
        // }

        // app_add_light(
        //     app,
        //     Light {
        //         ty: LightType::Point,
        //         position: Vector3 {
        //             x: 1.5,
        //             y: -0.3,
        //             z: -1.5,
        //         },
        //         direction: Vector3::ZERO,
        //         range: 100.0,
        //         color: Vector3::new(1.0, 1.0, 1.0),
        //         shadow_radius_or_angle: 0.0,
        //     },
        // );

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
        //         shadow_radius_or_angle: 0.01,
        //     },
        // );

        let (w, h, emission_pixels) = load_tga("..\\textures\\emission_cute.tga").unwrap();
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

        let test_probes = false;
        if test_probes {
            let mut offset = 0.1;
            for _ in 0..5 {
                app_add_probe(app, Vector3::new(0.0, offset, 0.0));
                app_add_probe(app, Vector3::new(0.0, offset, 0.0));
                app_add_probe(app, Vector3::new(0.0, offset, 0.0));
                app_add_probe(app, Vector3::new(0.0, offset, 0.0));
                offset += 0.1;
            }
        }

        app_run(app);

        app_destroy(app);
    }

    fn make_config() -> StilbConfig {
        let preview_settings = LightmapSettings {
            width: 1024,
            height: 1024,
            max_samples: 512,
            bounce_count: 3,
            denoise: false,
        };

        let config = StilbConfig {
            coordinate_system: CoordinateSystem::Default,
            is_preview: true,
            camera_position: Vector3::new(0.0, 0.0, 5.0),
            camera_forward: Vector3::FORWARD,
            preview_settings,
            throttle_preview_ms: 2,
            callback: test_save_callback,
            probes_callback: test_probes_callback,
            texture_filter: TextureSamplerFilter::Linear,
            probe_samples: 4096,
            probe_bounces: 3,
        };
        config
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

    pub fn add_mesh(app: *mut Stilb, path: &str) -> std::io::Result<()> {
        let (document, buffers, _) =
            gltf::import(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        for mesh in document.meshes() {
            for primitive in mesh.primitives() {
                let mut vertices = Vec::<Vector3>::new();
                let mut normals = Vec::<Vector3>::new();
                let mut uvs = Vec::<Vector2>::new();
                let mut indices = Vec::<u32>::new();

                let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()].0));

                if let Some(iter) = reader.read_positions() {
                    for p in iter {
                        let v = Vector3::new(p[0], p[1], p[2]);
                        vertices.push(v);
                    }
                }

                if let Some(iter) = reader.read_normals() {
                    for n in iter {
                        let v = Vector3::new(n[0], n[1], n[2]);
                        normals.push(v);
                    }
                }

                if let Some(iter) = reader.read_tex_coords(0) {
                    for uv in iter.into_f32() {
                        uvs.push(Vector2::new(uv[0], uv[1]));
                    }
                }

                if let Some(iter) = reader.read_indices() {
                    indices.extend(iter.into_u32());
                }

                let mesh = FfiMesh {
                    vertices: vertices.as_ptr(),
                    normals: normals.as_ptr(),
                    uvs: uvs.as_ptr(),
                    indices: indices.as_ptr(),
                    vertices_length: vertices.len() as u32,
                    indices_length: indices.len() as u32,
                    lightmap_group: 0,
                    backface_gi: false,
                };
                app_add_mesh(app, mesh);
            }
        }

        Ok(())
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn test_save_callback(data: ReadbackData) {
        let pixels = unsafe { std::slice::from_raw_parts(data.pixels, data.pixels_count as usize) };

        let file_name = format!("../temp/diffuse_lightmap{}.bmp", data.group_index);
        save_bmp(file_name.as_str(), data.width, data.height, &pixels).unwrap();
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn test_probes_callback(data: ReadbackProbesData) {
        let probes = unsafe { std::slice::from_raw_parts(data.probes, data.pixels_count as usize) };

        println!("Baked Probes:\n {:#?}", &probes);
    }
}
