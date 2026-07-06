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
                string sceneName = _context.scene.name;
                string outputFolder = Path.Combine(Path.GetDirectoryName(scenePath), sceneName);

                if (!AssetDatabase.IsValidFolder(outputFolder))
                {
                    AssetDatabase.CreateFolder(Path.GetDirectoryName(scenePath), sceneName);
                }


                foreach (var result in _bakeResults)
                {
                    var data = result.data;
                    var groupAsset = _context.groups[(int)data.group_index].groupAsset;

                    var diffuseTex = new Texture2D((int)data.width, (int)data.height, TextureFormat.RGBAFloat, false, true);
                    diffuseTex.wrapMode = TextureWrapMode.Clamp;
                    diffuseTex.SetPixels(result.pixelsDiffuseCopy);
                    var fileName = $"Lightmap-{data.group_index}_comp_light";
                    diffuseTex.name = fileName;

                    var assets = new UnityEngine.Object[] { diffuseTex };
                    string path;
                    if (groupAsset.format == LightmapSaveFormat.EXR)
                    {
                        string metaPath = Path.Combine(outputFolder, $"{fileName}.exr.meta");
                        if (!File.Exists(metaPath))
                        {
                            var guid = GUID.Generate().ToString();
                            var yaml = CreateTextureImporterMeta(guid);
                            File.WriteAllText(metaPath, yaml);
                        }
                        path = Path.Combine(outputFolder, $"{fileName}.exr");
                        var bytes = diffuseTex.EncodeToEXR(groupAsset.exrFlags);
                        File.WriteAllBytes(path, bytes);
                    }
                    else // asset
                    {
                        path = Path.Combine(outputFolder, $"{fileName}.asset");
                        InternalEditorUtility.SaveToSerializedFileAndForget(assets, path, false);
                    }


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


                lda.ApplyModifiedPropertiesWithoutUndo();
                string ldaName = "LightingData";

                // move 
                string destPath = Path.Combine(outputFolder, $"{ldaName}.asset").Replace("\\", "/");
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

                LightmapBakerEditor.BakeAllReflectionProbesSnapshots(_context.scene, _context.reflectionProbesSuperSampling ? 2 : 1, _context.reflectionProbesSpecular);
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

        public static string CreateTextureImporterMeta(string guid)
        {
            string yaml = $@"
fileFormatVersion: 2
guid: {guid}
TextureImporter:
  internalIDToNameTable: []
  externalObjects: {{}}
  serializedVersion: 13
  mipmaps:
    mipMapMode: 0
    enableMipMap: 0
    sRGBTexture: 0
    linearTexture: 1
    fadeOut: 0
    borderMipMap: 0
    mipMapsPreserveCoverage: 0
    alphaTestReferenceValue: 0.5
    mipMapFadeDistanceStart: 1
    mipMapFadeDistanceEnd: 3
  bumpmap:
    convertToNormalMap: 0
    externalNormalMap: 0
    heightScale: 0.25
    normalMapFilter: 0
    flipGreenChannel: 0
  isReadable: 0
  streamingMipmaps: 0
  streamingMipmapsPriority: 0
  vTOnly: 0
  ignoreMipmapLimit: 0
  grayScaleToAlpha: 0
  generateCubemap: 6
  cubemapConvolution: 0
  seamlessCubemap: 0
  textureFormat: 1
  maxTextureSize: 2048
  textureSettings:
    serializedVersion: 2
    filterMode: 1
    aniso: 1
    mipBias: 0
    wrapU: 1
    wrapV: 1
    wrapW: 0
  nPOTScale: 1
  lightmap: 0
  compressionQuality: 50
  spriteMode: 0
  spriteExtrude: 1
  spriteMeshType: 1
  alignment: 0
  spritePivot: {{x: 0.5, y: 0.5}}
  spritePixelsToUnits: 100
  spriteBorder: {{x: 0, y: 0, z: 0, w: 0}}
  spriteGenerateFallbackPhysicsShape: 1
  alphaUsage: 0
  alphaIsTransparency: 0
  spriteTessellationDetail: -1
  textureType: 6
  textureShape: 1
  singleChannelComponent: 0
  flipbookRows: 1
  flipbookColumns: 1
  maxTextureSizeSet: 0
  compressionQualitySet: 0
  textureFormatSet: 0
  ignorePngGamma: 0
  applyGammaDecoding: 0
  swizzle: 50462976
  cookieLightType: 0
  platformSettings:
  - serializedVersion: 4
    buildTarget: DefaultTexturePlatform
    maxTextureSize: 8192
    resizeAlgorithm: 0
    textureFormat: -1
    textureCompression: 2
    compressionQuality: 50
    crunchedCompression: 0
    allowsAlphaSplitting: 0
    overridden: 0
    ignorePlatformSupport: 0
    androidETC2FallbackOverride: 0
    forceMaximumCompressionQuality_BC6H_BC7: 1
  - serializedVersion: 4
    buildTarget: Standalone
    maxTextureSize: 8192
    resizeAlgorithm: 0
    textureFormat: 24
    textureCompression: 1
    compressionQuality: 50
    crunchedCompression: 0
    allowsAlphaSplitting: 0
    overridden: 1
    ignorePlatformSupport: 0
    androidETC2FallbackOverride: 0
    forceMaximumCompressionQuality_BC6H_BC7: 1
  - serializedVersion: 4
    buildTarget: Android
    maxTextureSize: 8192
    resizeAlgorithm: 0
    textureFormat: 68
    textureCompression: 1
    compressionQuality: 50
    crunchedCompression: 0
    allowsAlphaSplitting: 0
    overridden: 1
    ignorePlatformSupport: 0
    androidETC2FallbackOverride: 0
    forceMaximumCompressionQuality_BC6H_BC7: 1
  spriteSheet:
    serializedVersion: 2
    sprites: []
    outline: []
    customData: 
    physicsShape: []
    bones: []
    spriteID: 
    internalID: 0
    vertices: []
    indices: 
    edges: []
    weights: []
    secondaryTextures: []
    spriteCustomMetadata:
      entries: []
    nameFileIdTable: {{}}
  mipmapLimitGroupName: 
  pSDRemoveMatte: 0
  userData: 
  assetBundleName: 
  assetBundleVariant:
";
            return yaml;
        }
    }
}