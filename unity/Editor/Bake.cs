using System;
using System.Collections.Generic;
using System.IO;
using System.Threading;
using UnityEditor;
using UnityEditor.SceneManagement;
using UnityEditorInternal;
using UnityEngine;
using UnityEngine.SceneManagement;

namespace stilb
{
    public class Bake
    {
        class ReadbackResult
        {
            public Bindings.ReadbackData data;
            public Color[] pixelsDiffuseCopy;
        }

        static List<ReadbackResult> _bakeResults = new();
        static volatile bool _isComplete = false;
        static volatile bool _running = false;

        [AOT.MonoPInvokeCallback(typeof(Bindings.ReadbackCallback))]
        public static void OnReadback(Bindings.ReadbackData data)
        {
            Debug.Log($"Received Group {data.group_index}: {data.width}x{data.height}");
            var pixels = data.GetPixels();

            _bakeResults.Add(new ReadbackResult()
            {
                data = data,
                pixelsDiffuseCopy = pixels,
            });
        }

        static void CheckBakeComplete()
        {
            if (!_isComplete)
            {
                return;
            }

            Debug.Log("Bake Complete");

            string lightmapFolder = "Assets/StilbLightmaps/";
            string lightmapFolderFull = Path.GetFullPath(lightmapFolder);

            if (!Directory.Exists(lightmapFolderFull))
            {
                Directory.CreateDirectory(lightmapFolderFull);
            }

            List<LightmapData> lightmapDatas = new();

            foreach (var item in _bakeResults)
            {
                var data = item.data;

                var diffuseTex = new Texture2D((int)data.width, (int)data.height, TextureFormat.RGBAFloat, 1, false);
                diffuseTex.SetPixels(item.pixelsDiffuseCopy);
                var fileName = $"Diffuse{data.group_index}";
                diffuseTex.name = fileName;

                var assets = new UnityEngine.Object[] { diffuseTex };
                var path = $"Assets/{fileName}.asset";
                InternalEditorUtility.SaveToSerializedFileAndForget(assets, path, false);
                // AssetDatabase.CreateAsset(texture, $"Assets/{fileName}.asset");


                AssetDatabase.ImportAsset(path);
                var loadedAsset = AssetDatabase.LoadAssetAtPath<Texture2D>(path);
                var lmData = new LightmapData
                {
                    lightmapColor = loadedAsset,
                    lightmapDir = null,
                    shadowMask = null
                };
                lightmapDatas.Add(lmData);
            }


            var scene = SceneManager.GetActiveScene();
            LightmapSettings.lightmaps = lightmapDatas.ToArray();
            LightmapSettings.lightmapsMode = LightmapsMode.NonDirectional;
            EditorSceneManager.MarkSceneDirty(scene);

            _bakeResults = new();
            _isComplete = false;
            _running = false;
        }

        public static void Start(Bindings.StilbConfig config)
        {
            if (_running)
            {
                Debug.LogError("Bake already running");
                return;
            }
            _isComplete = false;
            _bakeResults = new();
            EditorApplication.update -= CheckBakeComplete;
            EditorApplication.update += CheckBakeComplete;


            var ctx = new BakeContext();

            _running = true;

            var thread = new Thread(() =>
            {
                try
                {
                    var app = Bindings.app_new(config);


                    for (int i = 0; i < ctx.sceneMesh.Count; i++)
                    {
                        var data = ctx.sceneMesh[i];

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

                    foreach (var light in ctx.sceneLights)
                    {
                        Bindings.app_add_light(app, light);
                    }

                    foreach (var group in ctx.groups)
                    {
                        unsafe
                        {
                            fixed (Color32* albedoPtr = group.albedo)
                            fixed (Color* emissionsPtr = group.emission)
                            {
                                Bindings.app_add_lightmap_group(
                                    app,
                                    group.settings,
                                    (byte*)albedoPtr,
                                    (uint)(group.albedo.Length * 4),
                                    (float*)emissionsPtr,
                                    (uint)(group.emission.Length * 4)
                                );
                            }
                        }
                    }


                    Bindings.app_run(app);

                    Bindings.app_destroy(app);
                    _running = false;
                    _isComplete = true;
                }
                catch (Exception e)
                {
                    _running = false;
                    Debug.LogException(e);
                }
            });

            thread.SetApartmentState(ApartmentState.STA);
            thread.IsBackground = true;
            thread.Start();


        }

    }
}