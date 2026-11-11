#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::BufWriter;
    use std::{f32, panic};

    use image::codecs::openexr::OpenExrEncoder;
    use image::{ExtendedColorType, ImageEncoder};

    use crate::bindings::*;
    use crate::lights::LightType;
    use crate::math::*;
    use crate::mesh::FfiMesh;
    use crate::pack::UVPacker;
    use crate::texture2d::encode_r11g11b10;
    use crate::*;

    const TEST_NAME: &str = "raster_test";

    const DIRECT_LIGHT_SAMPLES: u32 = 256 * 4;
    const DIRECT_EMISSION_SAMPLES: u32 = 1024 * 4;
    const INDIRECT_SAMPLES: u32 = 256;
    const LIGHT_PROBE_SAMPLES: u32 = 4096;
    const BOUNCE_COUNT: u32 = 5;

    const DENOISE: bool = false;
    const DILATE: bool = true;
    const FIX_SEAMS: bool = false;

    const MIS: bool = true;
    const HARDWARE_RT: bool = true;

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

    fn make_config() -> GlimConfig {
        let preview_settings = LightmapSettings {
            width: 1024,
            height: 1024,
            denoise: false,
            dilate: false,
            fix_seams: false,
            discard: false,
        };

        let config = GlimConfig {
            coordinate_system: CoordinateSystem::Default,
            is_preview: true,
            vulkan_validation_layers: true,
            seams_debug: false,
            hardware_rt: HARDWARE_RT,
            throttle_preview_ms: 0,
            preview_settings,
            camera_position: Vector3::new(0.0, 0.0, 5.0),
            camera_forward: Vector3::FORWARD,
            log_callback: default_logger,
            lightprobes_read_callback: test_probes_callback,
            probe_samples: LIGHT_PROBE_SAMPLES,
            light_falloff: LightFalloffType::InverseSquare,
            mis: MIS,
            direct_light_samples: DIRECT_LIGHT_SAMPLES,
            direct_emission_samples: DIRECT_EMISSION_SAMPLES,
            indirect_samples: INDIRECT_SAMPLES,
            bounce_count: BOUNCE_COUNT,
            lightmap_mode: LightmapMode::DominantDirection,
            skybox_intensity: 1.0,
            indirect_intensity: 1.0,
            lightprobe_deringing: 4.0,
            emissive_multiplier: 1.0,
            ao_intensity: 0.0,
            ao_range: 1.0,
            coverage_max_samples: 4,
            coverage_min_threshold: 0.1,
            antialiasing: true,
            leak_detection: true,
        };
        config
    }

    fn test_render(config: GlimConfig) {
        let output_dir = FfiString::new("temp");
        let app = app_new(config, output_dir);

        let mesh_path = format!("tests/{}.glb", TEST_NAME);

        let emission_path = format!("tests/{}_emission.tga", TEST_NAME);
        let albedo_path = format!("tests/{}_albedo.tga", TEST_NAME);

        let (_, _, emission_pixels) = load_tga_f32(&emission_path).unwrap();
        let (w, h, albedo_pixels) = load_tga_u8(&albedo_path).unwrap();

        let settings = LightmapSettings {
            width: w,
            height: h,
            denoise: DENOISE,
            dilate: DILATE,
            fix_seams: FIX_SEAMS,
            discard: false,
        };

        let packed_emission_pixels: Vec<u8> = emission_pixels
            .chunks_exact(4)
            .flat_map(|rgba| encode_r11g11b10(rgba[0], rgba[1], rgba[2]))
            .collect();

        drop(emission_pixels);

        app_add_lightmap_group(
            app,
            settings,
            albedo_pixels.as_ptr(),
            albedo_pixels.len() as u32,
            packed_emission_pixels.as_ptr(),
            packed_emission_pixels.len() as u32,
        );

        load_gltf(
            app,
            &mesh_path,
            false,
            false,
            false,
            Vector3::new(0.0, 0.0, 0.0),
            0,
            false,
        )
        .expect("failed to load mesh");

        {
            let app = unsafe { &mut *app };
            app.skybox = Skybox::solid(&app.vk, 8, 8, Vector3::new(0.00, 0.00, 0.00));
        }

        app_add_probe(app, Vector3::new(0.0, 1.0, 0.0), 0.0);

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

    fn quaternion_rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
        let qv = [q[0], q[1], q[2]];
        let w = q[3];

        let t = [
            2.0 * (qv[1] * v[2] - qv[2] * v[1]),
            2.0 * (qv[2] * v[0] - qv[0] * v[2]),
            2.0 * (qv[0] * v[1] - qv[1] * v[0]),
        ];

        [
            v[0] + w * t[0] + (qv[1] * t[2] - qv[2] * t[1]),
            v[1] + w * t[1] + (qv[2] * t[0] - qv[0] * t[2]),
            v[2] + w * t[2] + (qv[0] * t[1] - qv[1] * t[0]),
        ]
    }

    pub fn load_gltf(
        app: *mut Glim,
        path: &str,
        flip_uv: bool,
        transparent: bool,
        no_shadows: bool,
        position_offset: Vector3,
        group: u32,
        backface_gi: bool,
    ) -> std::io::Result<()> {
        let (document, buffers, _) =
            gltf::import(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        for node in document.nodes() {
            let (t, r, _s) = node.transform().decomposed();

            let position = Vector3::new(t[0], t[1], t[2]);
            let fwd = quaternion_rotate(r, [0.0, 0.0, -1.0]);
            let up = quaternion_rotate(r, [0.0, 1.0, 0.0]);

            if let Some(gltf_light) = node.light() {
                let c = gltf_light.color();
                let intensity = gltf_light.intensity() / 1000.0; // todo

                // intensity *= 1.0 / f32::consts::PI;

                let mut light = Light {
                    position: position,
                    direction: Vector3::new(-fwd[0], -fwd[1], -fwd[2]),
                    up: Vector3::new(up[0], up[1], up[2]),
                    range: gltf_light.range().unwrap_or(25.0),
                    color: Vector3::new(c[0] * intensity, c[1] * intensity, c[2] * intensity),
                    shadow_radius_or_angle: 0.0,
                    ..Default::default()
                };

                match gltf_light.kind() {
                    gltf::khr_lights_punctual::Kind::Directional => {
                        light.ty = LightType::Directional;
                    }
                    gltf::khr_lights_punctual::Kind::Point => {
                        light.ty = LightType::Point;
                    }
                    gltf::khr_lights_punctual::Kind::Spot {
                        inner_cone_angle,
                        outer_cone_angle,
                    } => {
                        let inner_percent = if outer_cone_angle > 0.0 {
                            inner_cone_angle / outer_cone_angle
                        } else {
                            100.0
                        };
                        light.ty = LightType::Spot;
                        light.spot_inner_percent = inner_percent * 100.0;
                        light.spot_outer = outer_cone_angle.to_degrees() * 2.0;
                    }
                };

                // println!("Creating Light:\n{:#?}", light);
                app_add_light(app, light);
            }

            if let Some(_camera) = node.camera() {
                {
                    let app = unsafe { &mut *app };
                    app.camera.set_forward(Vector3::new(fwd[0], fwd[1], fwd[2]));
                    app.camera.position = position;
                }
            }
        }

        for mesh in document.meshes() {
            for primitive in mesh.primitives() {
                let mut vertices = Vec::<Vector3>::new();
                let mut normals = Vec::<Vector3>::new();
                let mut tangents = Vec::<[f32; 4]>::new();
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

                if let Some(iter) = reader.read_tangents() {
                    for t in iter {
                        tangents.push(t);
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

                if tangents.is_empty() && !uvs.is_empty() && !normals.is_empty() {
                    tangents = generate_tangents(&vertices, &normals, &uvs, &indices);
                }

                assert!(tangents.len() == vertices.len());
                assert!(uvs.len() == vertices.len());
                assert!(normals.len() == vertices.len());

                let mesh = FfiMesh {
                    vertices: vertices.as_ptr(),
                    normals: normals.as_ptr(),
                    lightmap_uvs: uvs.as_ptr(),
                    main_uvs: uvs.as_ptr(),
                    tangents: tangents.as_ptr(),
                    indices: indices.as_ptr(),
                    vertices_length: vertices.len() as u32,
                    indices_length: indices.len() as u32,
                    lightmap_group: group,
                    backface_gi,
                    emissive: true,
                    fix_seams: true,
                    transparent,
                    no_shadows,
                    alpha_index: -1,
                    alpha_wrap: true,
                    cutoff: 0.5,
                };
                app_add_mesh(app, mesh);
            }
        }

        Ok(())
    }

    fn generate_tangents(
        positions: &[Vector3],
        normals: &[Vector3],
        uvs: &[Vector2],
        indices: &[u32],
    ) -> Vec<[f32; 4]> {
        let n = positions.len();
        let mut tan = vec![[0.0f32; 3]; n];
        let mut bitan = vec![[0.0f32; 3]; n];

        for tri in indices.chunks_exact(3) {
            let (i0, i1, i2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);

            let (p0, p1, p2) = (positions[i0], positions[i1], positions[i2]);
            let (uv0, uv1, uv2) = (uvs[i0], uvs[i1], uvs[i2]);

            let e1 = [p1.x - p0.x, p1.y - p0.y, p1.z - p0.z];
            let e2 = [p2.x - p0.x, p2.y - p0.y, p2.z - p0.z];

            let du1 = uv1.x - uv0.x;
            let dv1 = uv1.y - uv0.y;
            let du2 = uv2.x - uv0.x;
            let dv2 = uv2.y - uv0.y;

            let det = du1 * dv2 - du2 * dv1;
            if det.abs() < 1e-12 {
                continue; // degenerate UVs, skip this triangle
            }
            let r = 1.0 / det;

            let t = [
                (e1[0] * dv2 - e2[0] * dv1) * r,
                (e1[1] * dv2 - e2[1] * dv1) * r,
                (e1[2] * dv2 - e2[2] * dv1) * r,
            ];
            let b = [
                (e2[0] * du1 - e1[0] * du2) * r,
                (e2[1] * du1 - e1[1] * du2) * r,
                (e2[2] * du1 - e1[2] * du2) * r,
            ];

            for &i in &[i0, i1, i2] {
                for k in 0..3 {
                    tan[i][k] += t[k];
                    bitan[i][k] += b[k];
                }
            }
        }

        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let nrm = [normals[i].x, normals[i].y, normals[i].z];
            let t = tan[i];

            // Gram-Schmidt: t' = normalize(t - n * dot(n, t))
            let d = nrm[0] * t[0] + nrm[1] * t[1] + nrm[2] * t[2];
            let mut o = [t[0] - nrm[0] * d, t[1] - nrm[1] * d, t[2] - nrm[2] * d];
            let len = (o[0] * o[0] + o[1] * o[1] + o[2] * o[2]).sqrt();

            if len > 1e-8 {
                o = [o[0] / len, o[1] / len, o[2] / len];
            } else {
                // fallback: any vector perpendicular to the normal
                let axis = if nrm[0].abs() < 0.9 {
                    [1.0, 0.0, 0.0]
                } else {
                    [0.0, 1.0, 0.0]
                };
                let d = nrm[0] * axis[0] + nrm[1] * axis[1] + nrm[2] * axis[2];
                o = [
                    axis[0] - nrm[0] * d,
                    axis[1] - nrm[1] * d,
                    axis[2] - nrm[2] * d,
                ];
                let l = (o[0] * o[0] + o[1] * o[1] + o[2] * o[2]).sqrt();
                o = [o[0] / l, o[1] / l, o[2] / l];
            }

            // handedness: sign of dot(cross(n, t), bitangent)
            let c = [
                nrm[1] * o[2] - nrm[2] * o[1],
                nrm[2] * o[0] - nrm[0] * o[2],
                nrm[0] * o[1] - nrm[1] * o[0],
            ];
            let b = bitan[i];
            let w = if c[0] * b[0] + c[1] * b[1] + c[2] * b[2] < 0.0 {
                -1.0
            } else {
                1.0
            };

            out.push([o[0], o[1], o[2], w]);
        }
        out
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn test_probes_callback(data: LightprobesReadbackData) {
        let probes = unsafe { std::slice::from_raw_parts(data.probes, data.pixels_count as usize) };

        println!("Baked Probes:\n {:?}", &probes);
    }

    #[test]
    fn test_uv_packer() -> std::io::Result<()> {
        let path = "tests/packuv_split.glb";

        let mut packer = UVPacker::new(1024, 1024, 25, true, true, false, 1.0);

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
            let file_name = format!("temp/char{}.bmp", i);
            println!("scale_offset {:#?}", packer.get_scale_offset(i));
            chart.bitmap().save_bmp(&file_name);
        }

        match packer.target {
            Some(bm) => {
                let file_name = "temp/atlas.bmp";
                bm.save_bmp(&file_name);
            }
            None => {
                panic!("Packing Failed");
            }
        }

        Ok(())
    }
}
