using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
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
        static List<Bindings.SHProbe> _bakeProbesResults = new();
        static volatile bool _isComplete = false;
        static volatile bool _running = false;
        static BakeContext _context = null;

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

        [AOT.MonoPInvokeCallback(typeof(Bindings.ReadbackProbesCallback))]
        public static void OnReadbackProbes(Bindings.ReadbackProbeData data)
        {
            Debug.Log($"Received Probes {data.probes_count}");
            var probes = data.GetProbes();

            _bakeProbesResults.AddRange(probes);
        }

        static void PollBakeComplete()
        {
            if (!_isComplete)
            {
                return;
            }

            if (_bakeResults.Count == 0)
            {
                return;
            }
            try
            {
                Debug.Log("Bake Complete");

                List<LightmapData> lightmapDatas = new();

                var scenePath = _context.scene.path;
                string sceneDirectory = Path.GetDirectoryName(scenePath);

                foreach (var result in _bakeResults)
                {
                    var data = result.data;
                    var groupAsset = _context.groups[(int)data.group_index].groupAsset;

                    var diffuseTex = new Texture2D((int)data.width, (int)data.height, TextureFormat.RGBAFloat, false, true);
                    diffuseTex.SetPixels(result.pixelsDiffuseCopy);
                    var fileName = $"{_context.scene.name} LightmapDiffuse_{data.group_index}";
                    diffuseTex.name = fileName;

                    var assets = new UnityEngine.Object[] { diffuseTex };
                    string path;
                    if (groupAsset.format == LightmapSaveFormat.EXR)
                    {
                        // todo disable mip maps and change other import settings
                        path = Path.Combine(sceneDirectory, $"{fileName}.exr");
                        var bytes = diffuseTex.EncodeToEXR(groupAsset.exrFlags);
                        File.WriteAllBytes(path, bytes);
                    }
                    else // asset
                    {
                        path = Path.Combine(sceneDirectory, $"{fileName}.asset");
                        InternalEditorUtility.SaveToSerializedFileAndForget(assets, path, false);
                    }
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

                // EditorUtility.SetDirty(_context.baker);

                using var lda = new SerializedObject(_context.storage);
                LightingData.InspectorModeObject.SetValue(lda, InspectorMode.DebugInternal);

                Debug.Assert(_context.storage != null);

                var lightmapsProp = lda.FindProperty("m_Lightmaps");
                Debug.Assert(lightmapsProp != null);

                lightmapsProp.arraySize = lightmapDatas.Count;
                for (int i = 0; i < lightmapDatas.Count; i++)
                {
                    var element = lightmapsProp.GetArrayElementAtIndex(i);

                    element.FindPropertyRelative("m_Lightmap").objectReferenceValue = lightmapDatas[i].lightmapColor;
                    element.FindPropertyRelative("m_DirLightmap").objectReferenceValue = lightmapDatas[i].lightmapDir;
                    element.FindPropertyRelative("m_ShadowMask").objectReferenceValue = lightmapDatas[i].shadowMask;
                }


                lda.FindProperty("m_LightmapsMode").intValue = (int)LightmapsMode.NonDirectional;

                var storagePath = Path.Combine(sceneDirectory, $"{_context.scene.name} LightmapStorage.asset");

                lda.ApplyModifiedPropertiesWithoutUndo();
                string ldaName = _context.scene.name + " LightingData";

                // move 
                string destPath = Path.Combine(Path.GetDirectoryName(scenePath), $"{ldaName}.asset").Replace("\\", "/");
                if (AssetDatabase.LoadMainAssetAtPath(destPath) != null)
                {
                    AssetDatabase.DeleteAsset(destPath);
                }
                AssetDatabase.MoveAsset(LightingData.TempLightingDataPath, destPath);

                // apply new asset
                var newLda = AssetDatabase.LoadAssetAtPath<LightingDataAsset>(destPath);
                using var lda2 = new SerializedObject(newLda);
                lda2.FindProperty("m_Name").stringValue = ldaName;
                lda2.ApplyModifiedPropertiesWithoutUndo();
                Lightmapping.lightingDataAsset = newLda;
                EditorSceneManager.MarkSceneDirty(_context.scene);

                // cursed
                // EditorSceneManager.SaveScene(_context.scene);
                // Scene tempScene = EditorSceneManager.OpenScene(LightingData.TempScenePath, OpenSceneMode.Additive);
                // EditorSceneManager.CloseScene(_context.scene, false);
                // EditorSceneManager.OpenScene(scenePath, OpenSceneMode.Single);
                // EditorSceneManager.CloseScene(tempScene, true);
            }
            finally
            {
                ResetBake();
            }
        }

        static void ResetBake()
        {
            EditorApplication.update -= PollBakeComplete;
            _bakeResults = new();
            _bakeProbesResults = new();
            _isComplete = false;
            _running = false;
            _context = null;
        }

        public static void Start(LightmapBaker baker, Bindings.StilbConfig config)
        {
            if (_running)
            {
                Debug.LogError("Bake already running");
                return;
            }

            config.callback = OnReadback;
            config.probes_callback = OnReadbackProbes;

            ResetBake();

            EditorApplication.update += PollBakeComplete;

            var ctx = new BakeContext(baker, config);
            _context = ctx;

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
                    // free
                    ctx.sceneMesh = new();

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
                        group.ClearPixels();
                    }

                    foreach (var position in ctx.probePositions)
                    {
                        Bindings.app_add_probe(app, position);
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