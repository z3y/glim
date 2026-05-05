using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using UnityEditor;
using UnityEngine;
using UnityEngine.SceneManagement;

namespace stilb
{
    public class Preview
    {
        [MenuItem("Stilb/Start Preview")]
        public static void StartPreview()
        {
            var scene = SceneManager.GetActiveScene();

            var camera = SceneView.lastActiveSceneView.camera;

            var rootObjects = scene.GetRootGameObjects().Where(x => x.activeInHierarchy);

            var groups = rootObjects.SelectMany(x => x.GetComponentsInChildren<LightmapGroup>(false)).ToArray();

            var allRenderers = groups.SelectMany(x => x.GetComponentsInChildren<MeshRenderer>(false));

            var staticRenderers = allRenderers.Where(x => Stilb.IsLightmapStatic(x)).ToArray();

            var lights = rootObjects.SelectMany(x => x.GetComponentsInChildren<Light>(false)).ToArray();


            var lightmapSettings = new Bindings.LightmapSettings
            {
                width = 512,
                height = 512,
                max_samples = 256,
                bounce_count = 3,
                denoise = false,
            };

            var config = new Bindings.StilbConfig
            {
                is_preview = true,
                preview_height = 512,
                preview_width = 512,
                camera_position = camera.transform.position,
                camera_forward = camera.transform.forward,
            };

            var meshes = new List<Mesh>();

            int pixels = (int)(lightmapSettings.width * lightmapSettings.height);

            // var albedo = new Color32[pixels];
            // for (int i = 0; i < pixels; i++)
            // {
            //     albedo[i] = new Color32(255, 255, 255, 255);
            // }
            // var emission = new Color[pixels];

            var meshData = Stilb.ExtractMeshData(staticRenderers, 0);

            var lightsData = new List<Bindings.Light>();

            foreach (var light in lights)
            {
                // todo color temperature
                var color = new Vector3(light.color.r, light.color.g, light.color.b) * light.intensity;

                var l = new Bindings.Light
                {
                    ty = Bindings.LightType.Point,
                    position = light.transform.position,
                    direction = light.transform.forward,
                    range = light.range,
                    color = color,
                    shadow_radius_or_angle = light.shadowRadius,
                };

                lightsData.Add(l);
            }

            var meta = new MetaTexture((int)lightmapSettings.width);
            var albedoAtlas = meta.CreateAtlas(staticRenderers, MetaTexture.AtlasType.Albedo);
            var albedo = albedoAtlas.GetPixels32();
            Editor.DestroyImmediate(albedoAtlas);

            var emissionAtlas = meta.CreateAtlas(staticRenderers, MetaTexture.AtlasType.Emission);
            var emission = emissionAtlas.GetPixels();
            Editor.DestroyImmediate(emissionAtlas);

            var thread = new Thread(() =>
            {
                try
                {
                    var app = Bindings.app_new(config);


                    for (int i = 0; i < meshData.Count; i++)
                    {
                        var data = meshData[i];

                        unsafe
                        {
                            fixed (Vector3* vPtr = data.vertices)
                            fixed (Vector3* nPtr = data.normals)
                            fixed (Vector2* uPtr = data.uvs)
                            fixed (int* iPtr = data.triangles)
                            {
                                var exportedMesh = new Bindings.Mesh
                                {
                                    vertices = vPtr,
                                    normals = nPtr,
                                    uvs = uPtr,
                                    indices = (uint*)iPtr,
                                    vertices_length = (uint)data.vertices.Length,
                                    indices_length = (uint)data.triangles.Length,
                                    lightmap_group = data.groupIndex,
                                };

                                Bindings.app_add_mesh(app, exportedMesh);
                            }
                        }
                    }

                    foreach (var light in lightsData)
                    {
                        Bindings.app_add_light(app, light);
                    }

                    unsafe
                    {
                        fixed (Color32* albedoPtr = albedo)
                        fixed (Color* emissionsPtr = emission)
                        {
                            Bindings.app_add_lightmap_group(
                                app,
                                lightmapSettings,
                                (byte*)albedoPtr,
                                (uint)(albedo.Length * 4),
                                (float*)emissionsPtr,
                                (uint)(emission.Length * 4)
                            );
                        }
                    }

                    Bindings.app_run(app);

                    Bindings.app_destroy(app);

                }
                catch (Exception e)
                {
                    Debug.LogException(e);
                }
            });

            thread.SetApartmentState(ApartmentState.STA);
            thread.IsBackground = true;
            thread.Start();


        }

    }
}