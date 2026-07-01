using System;
using System.Collections.Generic;
using System.IO;
using System.Threading;
using UnityEditor;
using UnityEditor.SceneManagement;
using UnityEditorInternal;
using UnityEngine;
using UnityEngine.Rendering;

namespace stilb
{
    public class Bake
    {
        class ReadbackResult
        {
            public Bindings.LightmapReadbackData data;
            public Color[] pixelsDiffuseCopy;
        }

        static List<ReadbackResult> _bakeResults = new();
        static List<Bindings.SHProbe> _bakeProbesResults = new();
        static volatile bool _isComplete = false;
        static volatile bool _running = false;
        static int _progressID = -1;
        static BakeContext _context = null;

        [AOT.MonoPInvokeCallback(typeof(Bindings.LightmapReadCallback))]
        public static void OnReadbackLightmap(Bindings.LightmapReadbackData data)
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
        public static void OnReadbackLightprobes(Bindings.LightprobesReadbackData data)
        {
            Debug.Log($"Received Probes {data.probes_count}");
            var probes = data.GetProbes();

            _bakeProbesResults.AddRange(probes);
        }

        [AOT.MonoPInvokeCallback(typeof(Bindings.LogCallback))]
        public static void OnLogCalback(Bindings.LogData data)
        {
            if (data.ty == 0) // success
            {
                Debug.Log(data.message.ToString());
            }
            if (data.ty == 1) // error
            {
                throw new Exception(data.message.ToString());
            }
            if (data.ty == 2) // progress
            {
                Progress.Report(_progressID, data.progress, data.message.ToString());
            }
        }

        static double _bakeStartTime = 0.0;

        static void PollBakeComplete()
        {
            if (!_isComplete)
            {
                return;
            }

            if (_bakeResults.Count == 0)
            {
                ResetBake();
                return;
            }
            try
            {
                var now = Time.realtimeSinceStartupAsDouble;
                Debug.Log($"Bake Complete in {now - _bakeStartTime}");

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


                // apply light probes
                var lightProbesRef = lda.FindProperty("m_LightProbes").objectReferenceValue;

                // faster
                SphericalHarmonicsL2 sh = new();
                var obj = lightProbesRef as LightProbes;
                Debug.Assert(obj != null);
                if (obj != null)
                {
                    SphericalHarmonicsL2[] bakedProbesArray = obj.bakedProbes;
                    int bakedCoeffCount = bakedProbesArray.Length;

                    for (int i = 0; i < bakedCoeffCount; i++)
                    {
                        Bindings.SHProbe probeData = _bakeProbesResults[i];

                        sh[0, 0] = probeData.L0.x; sh[0, 1] = probeData.L1_1.x; sh[0, 2] = probeData.L10.x; sh[0, 3] = probeData.L11.x; sh[0, 4] = probeData.L2_2.x; sh[0, 5] = probeData.L2_1.x; sh[0, 6] = probeData.L20.x; sh[0, 7] = probeData.L21.x; sh[0, 8] = probeData.L22.x;
                        sh[1, 0] = probeData.L0.y; sh[1, 1] = probeData.L1_1.y; sh[1, 2] = probeData.L10.y; sh[1, 3] = probeData.L11.y; sh[1, 4] = probeData.L2_2.y; sh[1, 5] = probeData.L2_1.y; sh[1, 6] = probeData.L20.y; sh[1, 7] = probeData.L21.y; sh[1, 8] = probeData.L22.y;
                        sh[2, 0] = probeData.L0.z; sh[2, 1] = probeData.L1_1.z; sh[2, 2] = probeData.L10.z; sh[2, 3] = probeData.L11.z; sh[2, 4] = probeData.L2_2.z; sh[2, 5] = probeData.L2_1.z; sh[2, 6] = probeData.L20.z; sh[2, 7] = probeData.L21.z; sh[2, 8] = probeData.L22.z;

                        bakedProbesArray[i] = sh;
                    }

                    obj.bakedProbes = bakedProbesArray;
                    EditorUtility.SetDirty(obj);
                }

                // slow
                // using var probesSo = new SerializedObject(lightProbesRef);
                // LightingData.InspectorModeObject.SetValue(probesSo, InspectorMode.DebugInternal);
                // var bakedCoeff = probesSo.FindProperty("m_BakedCoefficients");
                // int bakedCoeffCount = bakedCoeff.arraySize;
                // for (int i = 0; i < bakedCoeffCount; i++)
                // {
                //     SerializedProperty prop = bakedCoeff.GetArrayElementAtIndex(i);

                //     Bindings.SHProbe probeData = _bakeProbesResults[i];

                //     float[] flatCoefficients = new float[27]
                //     {
                //         probeData.L0.x, probeData.L1_1.x, probeData.L10.x, probeData.L11.x, probeData.L2_2.x, probeData.L2_1.x, probeData.L20.x, probeData.L21.x, probeData.L22.x,
                //         probeData.L0.y, probeData.L1_1.y, probeData.L10.y, probeData.L11.y, probeData.L2_2.y, probeData.L2_1.y, probeData.L20.y, probeData.L21.y, probeData.L22.y,
                //         probeData.L0.z, probeData.L1_1.z, probeData.L10.z, probeData.L11.z, probeData.L2_2.z, probeData.L2_1.z, probeData.L20.z, probeData.L21.z, probeData.L22.z
                //     };

                //     prop.Next(true);

                //     for (int j = 0; j < flatCoefficients.Length; j++)
                //     {
                //         prop.floatValue = flatCoefficients[j];
                //         prop.Next(false);
                //     }

                // }
                // probesSo.ApplyModifiedPropertiesWithoutUndo();


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
            if (_progressID != -1)
            {
                Progress.Finish(_progressID, Progress.Status.Succeeded);
            }
            _progressID = -1;
        }

        public static void Start(LightmapBaker baker, Bindings.StilbConfig config)
        {
            if (_running)
            {
                Debug.LogError("Bake already running");
                return;
            }

            ResetBake();

            EditorApplication.update += PollBakeComplete;

            var ctx = new BakeContext(baker, config);
            _context = ctx;

            _running = true;

            if (!config.is_preview)
            {
                _progressID = Progress.Start("Baking Lightmaps", null, Progress.Options.None);
            }

            _bakeStartTime = Time.realtimeSinceStartupAsDouble;
            var thread = new Thread(() =>
            {
                try
                {
                    var app = Bindings.app_new(config);

                    if (app == null)
                    {
                        throw new Exception("failed to launch");
                    }


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
                                    backface_gi = data.backfaceGI,
                                    transparent = data.transparent,
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
                    _isComplete = true;
                    Debug.LogException(e);
                }
            });

            thread.SetApartmentState(ApartmentState.STA);
            thread.IsBackground = true;
            thread.Start();


        }

    }
}