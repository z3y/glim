#[cfg(test)]
mod tests {
    use std::f32;
    use std::fs::File;
    use std::io::BufWriter;

    use image::codecs::openexr::OpenExrEncoder;
    use image::{ExtendedColorType, ImageEncoder};

    use crate::bindings::*;
    use crate::lights::LightType;
    use crate::math::*;
    use crate::mesh::FfiMesh;
    use crate::pack::UVPacker;
    use crate::*;

    fn make_config() -> StilbConfig {
        let preview_settings = LightmapSettings {
            width: 1024,
            height: 1024,
            denoise: false,
            dilate: false,
            fix_seams: false,
        };

        let config = StilbConfig {
            coordinate_system: CoordinateSystem::Default,
            is_preview: true,
            camera_position: Vector3::new(0.0, 0.0, 5.0),
            camera_forward: Vector3::FORWARD,
            preview_settings,
            throttle_preview_ms: 2,
            lightmap_read_callback: test_save_callback,
            lightprobes_read_callback: test_probes_callback,
            texture_filter: TextureSamplerFilter::Nearest,
            probe_samples: 4096,
            probe_bounces: 3,
            light_falloff: LightFalloffType::InverseSquare,
            vulkan_validation_layers: true,
            seams_debug: false,
            direct_samples: 64,
            indirect_samples: 64,
            bounce_count: 5,
            log_callback: log_callback,
            mis: false,
        };
        config
    }

    #[test]
    fn test_preview() {
        let mut config = make_config();
        config.is_preview = true;
        test_render(config, false);
    }

    #[test]
    fn test_bake() {
        let mut config = make_config();
        config.is_preview = false;
        test_render(config, false);
    }

    fn test_render(mut config: StilbConfig, test_probes: bool) {
        // config.camera_forward = Vector3 {
        //     x: -0.42446527,
        //     y: -0.4595601,
        //     z: -0.7801498,
        // };

        // config.camera_position = Vector3 {
        //     x: 1.890761,
        //     y: 0.4439021,
        //     z: 1.685063,
        // };

        // noisy
        config.camera_position = Vector3 {
            x: 1.829,
            y: 1.11498,
            z: 0.195829,
        };
        config.camera_forward = Vector3 {
            x: -0.8777,
            y: -0.4029,
            z: 0.2595,
        };

        let app = app_new(config);

        // let mut offset = 0.0;
        // for _ in 0..1 {
        // {
        // add_mesh(
        //     app,
        //     "../meshes/monkey.glb",
        //     false,
        //     false,
        //     Vector3::ZERO,
        //     0,
        //     false,
        // )
        // .expect("failed to load mesh");
        // add_mesh(
        //     app,
        //     "../meshes/random.glb",
        //     false,
        //     false,
        //     Vector3::ZERO,
        //     0,
        //     false,
        // )
        // .expect("failed to load mesh");
        // let (w, h, emission_pixels) = load_tga_f32("../textures/emission_cute.tga").unwrap();
        // let (w, h, emission_pixels) =
        //     load_tga_f32("../textures/emission_cute_wall_only.tga").unwrap();
        // let w = 512;
        // let h = 512;
        // let emission_pixels = vec![0.0; (w * h * 4) as usize];
        // let albedo_pixels = vec![255; (w * h * 4) as usize];
        // }

        // app_add_light(
        //     app,
        //     Light {
        //         ty: LightType::Point,
        //         position: Vector3 {
        //             x: 0.0,
        //             y: 1.0,
        //             z: 0.0,
        //         },
        //         direction: Vector3::ZERO,
        //         range: 5.0,
        //         color: Vector3::new(1.0, 1.0, 1.0) * 3.0,
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
        //         range: 5.0,
        //         color: Vector3::new(1.0, 1.0, 1.0),
        //         shadow_radius_or_angle: 0.0,
        //     },
        // );

        // app_add_light(
        //     app,
        //     Light {
        //         ty: LightType::Point,
        //         position: Vector3 {
        //             x: 0.5,
        //             y: 0.3,
        //             z: -0.5,
        //         },
        //         direction: Vector3::ZERO,
        //         range: 5.0,
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

        // let settings = LightmapSettings {
        //     width: w,
        //     height: h,
        //     bounce_count: 5,
        //     max_samples: 2048,
        //     denoise: true,
        //     dilate: true,
        //     fix_seams: false,
        // };

        // app_add_lightmap_group(
        //     app,
        //     settings.clone(),
        //     albedo_pixels.as_ptr(),
        //     albedo_pixels.len() as u32,
        //     emission_pixels.as_ptr(),
        //     emission_pixels.len() as u32,
        // );

        // flower
        // add_mesh(
        //     app,
        //     "../meshes/flower.glb",
        //     true,
        //     true,
        //     Vector3::new(0.0, -1.05, 0.0),
        //     1,
        //     true,
        // )
        // .expect("failed to load mesh");
        // let (w, h, albedo_pixels) = load_tga_u8("../textures/flower.tga").unwrap();
        // let emission_pixels = vec![0.0; (w * h * 4) as usize];
        // app_add_lightmap_group(
        //     app,
        //     settings,
        //     albedo_pixels.as_ptr(),
        //     albedo_pixels.len() as u32,
        //     emission_pixels.as_ptr(),
        //     emission_pixels.len() as u32,
        // );

        // noisy
        add_mesh(
            app,
            "../meshes/noisy.glb",
            false,
            false,
            Vector3::new(0.0, 0.0, 0.0),
            0,
            false,
        )
        .expect("failed to load mesh");
        let (w, h, mut emission_pixels) = load_tga_f32("../textures/noisy.tga").unwrap();
        for pixel in &mut emission_pixels {
            *pixel *= f32::consts::PI;
        }
        // let w = 1024 * 2;
        // let h = 1024 * 2;
        // let emission_pixels = vec![0.0; (w * h * 4) as usize];

        let albedo_pixels = vec![255; (w * h * 4) as usize];
        let settings = LightmapSettings {
            width: w,
            height: h,
            denoise: false,
            dilate: false,
            fix_seams: false,
        };
        app_add_lightmap_group(
            app,
            settings,
            albedo_pixels.as_ptr(),
            albedo_pixels.len() as u32,
            emission_pixels.as_ptr(),
            emission_pixels.len() as u32,
        );

        if test_probes {
            let mut offset = 0.1;
            for _ in 0..5 {
                app_add_probe(app, Vector3::new(0.0, offset, 0.0));
                offset += 0.1;
            }
        }

        app_run(app);

        app_destroy(app);
    }

    #[allow(dead_code)]
    pub fn load_tga_f32(path: &str) -> std::io::Result<(u32, u32, Vec<f32>)> {
        let img = image::open(path)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
            .to_rgba8();

        let width = img.width();
        let height = img.height();
        let pixels = img.into_raw().iter().map(|&b| b as f32 / 255.0).collect();

        Ok((width, height, pixels))
    }

    #[allow(dead_code)]
    pub fn save_exr_f32(pixels: &[f32], width: u32, height: u32, stride: u32, path: &str) {
        let color_type = match stride {
            3 => ExtendedColorType::Rgb32F,
            4 => ExtendedColorType::Rgba32F,
            _ => panic!("Unsupported stride: {}. Must be 1, 2, 3, or 4.", stride),
        };

        let file = File::create(path).expect("Failed to create EXR output file");
        let writer = BufWriter::new(file);

        let byte_buffer: &[u8] = unsafe {
            std::slice::from_raw_parts(
                pixels.as_ptr() as *const u8,
                pixels.len() * std::mem::size_of::<f32>(),
            )
        };

        let encoder = OpenExrEncoder::new(writer);
        encoder
            .write_image(byte_buffer, width, height, color_type)
            .expect("Failed to encode and save OpenEXR image data");
    }

    #[allow(dead_code)]
    pub fn load_tga_u8(path: &str) -> std::io::Result<(u32, u32, Vec<u8>)> {
        let img = image::open(path)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
            .to_rgba8();

        let width = img.width();
        let height = img.height();
        let pixels = img.into_raw();

        Ok((width, height, pixels))
    }

    pub fn add_mesh(
        app: *mut Stilb,
        path: &str,
        flip_uv: bool,
        transparent: bool,
        position_offset: Vector3,
        group: u32,
        backface_gi: bool,
    ) -> std::io::Result<()> {
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
                        let v = Vector3::new(p[0], p[1], p[2]) + position_offset;
                        vertices.push(v);
                    }
                }

                if let Some(iter) = reader.read_normals() {
                    for n in iter {
                        let v = Vector3::new(n[0], n[1], n[2]);
                        normals.push(v);
                    }
                }

                if let Some(iter) = reader.read_tex_coords(1) {
                    for uv in iter.into_f32() {
                        if flip_uv {
                            uvs.push(Vector2::new(uv[0], 1.0 - uv[1]));
                        } else {
                            uvs.push(Vector2::new(uv[0], uv[1]));
                        }
                    }
                } else {
                    if let Some(iter) = reader.read_tex_coords(0) {
                        for uv in iter.into_f32() {
                            if flip_uv {
                                uvs.push(Vector2::new(uv[0], 1.0 - uv[1]));
                            } else {
                                uvs.push(Vector2::new(uv[0], uv[1]));
                            }
                        }
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
                    lightmap_group: group,
                    backface_gi,
                    transparent,
                };
                app_add_mesh(app, mesh);
            }
        }

        Ok(())
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn test_save_callback(data: LightmapReadbackData) {
        let pixels = unsafe { std::slice::from_raw_parts(data.pixels, data.pixels_count as usize) };

        let file_name = format!("../temp/diffuse_lightmap{}.exr", data.group_index);

        println!("saving lightmap {}", file_name);

        save_exr_f32(&pixels, data.width, data.height, 4, file_name.as_str());
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn log_callback(data: LogMessage) {
        match data.ty {
            LogMessageType::Success => println!("Message: {}", data.message.from()),
            LogMessageType::Error => panic!("Error: {}", data.message.from()),
            LogMessageType::Progress => {
                use std::io::{self, Write};
                print!(
                    "\r{}: {:.1}%\x1B[K",
                    data.message.from(),
                    data.progress * 100.0
                );
                let _ = io::stdout().flush();
            }
        }
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn test_probes_callback(data: LightprobesReadbackData) {
        let probes = unsafe { std::slice::from_raw_parts(data.probes, data.pixels_count as usize) };

        println!("Baked Probes:\n {:?}", &probes);
    }

    #[test]
    fn test_uv_packer() -> std::io::Result<()> {
        let path = "../meshes/packuv.glb";
        // let path = "../meshes/plane.glb";

        let mut packer = UVPacker::new(512, 512, 5, true);

        let (document, buffers, _) =
            gltf::import(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        let flip_uv = false;
        let scale_multiplier = 1.0;

        let mut mesh_id = 0;
        for mesh in document.meshes() {
            for primitive in mesh.primitives() {
                let mut positions = Vec::<Vector3>::new();
                let mut uvs = Vec::<Vector2>::new();
                let mut indices = Vec::<u32>::new();

                let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()].0));

                if let Some(iter) = reader.read_positions() {
                    for p in iter {
                        let v = Vector3::new(p[0], p[1], p[2]);
                        positions.push(v);
                    }
                }

                if let Some(iter) = reader.read_tex_coords(1) {
                    for uv in iter.into_f32() {
                        if flip_uv {
                            uvs.push(Vector2::new(uv[0], 1.0 - uv[1]));
                        } else {
                            uvs.push(Vector2::new(uv[0], uv[1]));
                        }
                    }
                } else {
                    if let Some(iter) = reader.read_tex_coords(0) {
                        for uv in iter.into_f32() {
                            if flip_uv {
                                uvs.push(Vector2::new(uv[0], 1.0 - uv[1]));
                            } else {
                                uvs.push(Vector2::new(uv[0], uv[1]));
                            }
                        }
                    }
                }

                if let Some(iter) = reader.read_indices() {
                    indices.extend(iter.into_u32());
                }

                packer.add_mesh(&positions, &uvs, &indices, scale_multiplier, mesh_id);

                mesh_id += 1;
            }
        }

        packer.pack();

        for (i, chart) in packer.charts().iter().enumerate() {
            let file_name = format!("../temp/char{}.bmp", i);
            println!("scale_offset {:#?}", packer.get_scale_offset(i));
            chart.bitmap().save_bmp(&file_name);
        }

        match packer.target {
            Some(bm) => {
                let file_name = "../temp/atlas.bmp";
                bm.save_bmp(&file_name);
            }
            None => {}
        }

        Ok(())
    }
}
