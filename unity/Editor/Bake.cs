using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Threading;
using UnityEditor;
using UnityEditor.SceneManagement;
using UnityEngine;
using UnityEngine.Rendering;

namespace Glim
{
    [Serializable]
    public class BakeReport
    {
        public double bakeTime;
        public string finishedAt;
        public int lightmapCount;
        public long lightmapBytes;
        public long lightmapMemoryBytes;
        public long lightingDataBytes;
        public int probeCount;
    }

    public class Bake
    {
        static List<Bindings.SHProbe> _bakeProbesResults = new();
        static volatile bool _isComplete = false;
        static volatile bool _running = false;
        static int _progressID = -1;
        static BakeContext _context = null;

        static volatile float _progress = 0f;
        static volatile string _progressMessage = "";
        static volatile bool _isPreview = false;
        static volatile bool _cancelRequested = false;

        public static bool IsBaking => _running && !_isPreview;
        public static bool IsCancelling => _cancelRequested && _running;
        public static float BakeProgress => _progress;
        public static string BakeMessage => _progressMessage;

        public static void Cancel()
        {
            if (!_running || _isPreview || _cancelRequested)
            {
                return;
            }
            _cancelRequested = true;
            Bindings.app_request_cancel();
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
                _progress = data.progress;
                _progressMessage = data.message.ToString();
            }
        }

        static double _bakeStartTime = 0.0;

        static readonly MethodInfo StorageMemorySize = ResolveStorageMemorySize();

        static MethodInfo ResolveStorageMemorySize()
        {
            var type = AppDomain.CurrentDomain.GetAssemblies()
                .Select(a => a.GetType("UnityEditor.TextureUtil", false))
                .FirstOrDefault(t => t != null);

            return type?.GetMethod("GetStorageMemorySizeLong",
                BindingFlags.Static | BindingFlags.Public | BindingFlags.NonPublic);
        }

        static long GetCompressedTextureBytes(Texture2D texture)
        {
            if (texture == null || StorageMemorySize == null)
            {
                return 0;
            }

            return (long)StorageMemorySize.Invoke(null, new object[] { texture });
        }

        static string BakeReportPath(string scenePath)
        {
            var dir = Path.GetDirectoryName(scenePath);
            var sceneName = Path.GetFileNameWithoutExtension(scenePath);
            return Path.Combine(dir, sceneName, "bakeReport.json");
        }

        public static BakeReport LoadReport(string scenePath)
        {
            if (string.IsNullOrEmpty(scenePath))
            {
                return null;
            }

            var path = BakeReportPath(scenePath);
            return File.Exists(path) ? JsonUtility.FromJson<BakeReport>(File.ReadAllText(path)) : null;
        }

        public static int ReportVersion { get; private set; }

        static void SaveReport(string scenePath, BakeReport report)
        {
            if (string.IsNullOrEmpty(scenePath))
            {
                return;
            }

            var path = BakeReportPath(scenePath);
            Directory.CreateDirectory(Path.GetDirectoryName(path));
            File.WriteAllText(path, JsonUtility.ToJson(report));
            ReportVersion++;
        }

        const string BakingTitle = "Baking Lightmaps";
        const string DenoisingTitle = "Denoising & Fixing Seams";

        static string _progressTitle = "";

        static void ReportProgress()
        {
            var message = _progressMessage;

            // redraw since we can't edit titles for progress
            var title = message.StartsWith(DenoisingTitle) ? DenoisingTitle : BakingTitle;

            if (title != _progressTitle)
            {
                Progress.Finish(_progressID, Progress.Status.Succeeded);
                _progressID = Progress.Start(title, null, Progress.Options.None);
                Progress.RegisterCancelCallback(_progressID, () => { Cancel(); return true; });
                _progressTitle = title;
            }

            Progress.Report(_progressID, _progress, message);
        }

        static void PollBakeComplete()
        {
            if (!_isComplete)
            {
                if (_progressID != -1)
                {
                    ReportProgress();
                }
                return;
            }

            if (_context.isPreview)
            {
                ResetBake();
                return;
            }

            if (_cancelRequested)
            {
                Debug.Log("Bake cancelled");
                if (_progressID != -1)
                {
                    Progress.Finish(_progressID, Progress.Status.Canceled);
                    _progressID = -1;
                }
                ResetBake();
                return;
            }
            try
            {
                var now = Time.realtimeSinceStartupAsDouble;
                Debug.Log($"Bake Complete in {now - _bakeStartTime}");

                List<LightmapData> lightmapDatas = new();
                for (int i = 0; i < _context.groups.Count; i++)
                {
                    var lmData = new LightmapData
                    {
                        lightmapColor = null,
                        lightmapDir = null,
                        shadowMask = null
                    };
                    lightmapDatas.Add(lmData);
                }

                var scenePath = _context.scene.path;
                string sceneName = _context.scene.name;

                long lightmapBytes = 0;
                long lightmapMemoryBytes = 0;

                // hard coded paths for now in rust
                for (int groupIndex = 0; groupIndex < _context.groups.Count; groupIndex++)
                {
                    BakeContextGroup group = _context.groups[groupIndex];

                    var diffuseName = $"Lightmap-{groupIndex}_Diffuse.exr";
                    var directionalName = $"Lightmap-{groupIndex}_Directional.tga";

                    {
                        string metaPath = Path.Combine(_context.outputDir, $"{diffuseName}.meta");
                        if (!File.Exists(metaPath))
                        {
                            var guid = GUID.Generate().ToString();
                            var yaml = CreateTextureImporterMeta(guid, false);
                            File.WriteAllText(metaPath, yaml);
                        }
                    }

                    if (_context.lightmapMode == LightmapMode.Directional)
                    {
                        string metaPath = Path.Combine(_context.outputDir, $"{directionalName}.meta");
                        if (!File.Exists(metaPath))
                        {
                            var guid = GUID.Generate().ToString();
                            var yaml = CreateTextureImporterMeta(guid, true);
                            File.WriteAllText(metaPath, yaml);
                        }
                    }
                }

                AssetDatabase.Refresh();

                for (int groupIndex = 0; groupIndex < _context.groups.Count; groupIndex++)
                {
                    BakeContextGroup group = _context.groups[groupIndex];

                    var groupAsset = _context.groups[groupIndex].groupAsset;
                    var diffuseName = $"Lightmap-{groupIndex}_Diffuse.exr";
                    var directionalName = $"Lightmap-{groupIndex}_Directional.tga";

                    {
                        var path = Path.Combine(_context.outputDir, diffuseName);
                        var loadedAsset = AssetDatabase.LoadAssetAtPath<Texture2D>(path);
                        lightmapMemoryBytes += GetCompressedTextureBytes(loadedAsset);
                        lightmapDatas[groupIndex].lightmapColor = loadedAsset;
                        lightmapBytes += new FileInfo(path).Length;
                    }

                    if (_context.lightmapMode == LightmapMode.Directional)
                    {
                        var path = Path.Combine(_context.outputDir, directionalName);
                        var loadedAsset = AssetDatabase.LoadAssetAtPath<Texture2D>(path);
                        lightmapMemoryBytes += GetCompressedTextureBytes(loadedAsset);
                        lightmapDatas[groupIndex].lightmapDir = loadedAsset;
                        lightmapBytes += new FileInfo(path).Length;
                    }
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

                lda.FindProperty("m_LightmapsMode").intValue = _context.lightmapMode == LightmapMode.Directional ?
                    (int)LightmapsMode.CombinedDirectional : (int)LightmapsMode.NonDirectional;

                // apply light probes
                var lightProbesRef = lda.FindProperty("m_LightProbes").objectReferenceValue;
                SphericalHarmonicsL2 sh = new();
                var obj = lightProbesRef as LightProbes;
                Debug.Assert(obj != null);
                if (obj != null && _bakeProbesResults.Count > 0)
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
                string destPath = Path.Combine(_context.outputDir, $"{ldaName}.asset").Replace("\\", "/");
                if (AssetDatabase.LoadMainAssetAtPath(destPath) != null)
                {
                    AssetDatabase.DeleteAsset(destPath);
                }
                AssetDatabase.MoveAsset(LightingData.TempLightingDataPath, destPath);

#if VRC_LIGHT_VOLUMES
                CreateLightVolumeTextures(_context, _context.outputDir);
#endif

                // apply new asset
                var newLda = AssetDatabase.LoadAssetAtPath<LightingDataAsset>(destPath);
                using var lda2 = new SerializedObject(newLda);
                lda2.FindProperty("m_Name").stringValue = ldaName;
                lda2.ApplyModifiedPropertiesWithoutUndo();
                Lightmapping.lightingDataAsset = newLda;
                EditorSceneManager.MarkSceneDirty(_context.scene);

                LightmapSettings.lightmaps = lightmapDatas.ToArray();
                LightmapSettings.lightmapsMode = _context.lightmapMode == LightmapMode.Directional ? LightmapsMode.CombinedDirectional : LightmapsMode.NonDirectional;

                SaveReport(scenePath, new BakeReport
                {
                    bakeTime = now - _bakeStartTime,
                    finishedAt = DateTime.Now.ToString("o"),
                    lightmapCount = lightmapDatas.Count,
                    lightmapBytes = lightmapBytes,
                    lightmapMemoryBytes = lightmapMemoryBytes,
                    lightingDataBytes = new FileInfo(destPath).Length,
                    probeCount = _bakeProbesResults.Count,
                });

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
            _bakeProbesResults = new();
            _isComplete = false;
            _running = false;
            _context = null;
            _progress = 0f;
            _progressMessage = "";
            _progressTitle = "";
            _isPreview = false;
            _cancelRequested = false;
            if (_progressID != -1)
            {
                Progress.Finish(_progressID, Progress.Status.Succeeded);
            }
            _progressID = -1;
        }

        public static Vector4[] GenerateProbeVolume(Vector3 center, Vector3 size, Vector3Int resolution)
        {
            Vector4[] positions = new Vector4[resolution.x * resolution.y * resolution.z];

            Vector3 texelSize = new(
                size.x / resolution.x,
                size.y / resolution.y,
                size.z / resolution.z
            );

            Vector3 origin = center - size * 0.5f;

            origin += texelSize / 2.0f;

            float radius = Mathf.Min(texelSize.x, texelSize.y, texelSize.z) / 2.0f;

            int i = 0;
            for (int z = 0; z < resolution.z; z++)
                for (int y = 0; y < resolution.y; y++)
                    for (int x = 0; x < resolution.x; x++)
                    {
                        Vector4 probe = origin + Vector3.Scale(new Vector3(x, y, z), texelSize);
                        probe.w = radius;
                        positions[i++] = probe;
                    }

            return positions;
        }

#if VRC_LIGHT_VOLUMES
        static void AddLightProbeVolumes(LightmapBaker baker, BakeContext ctx)
        {
            var vrclv = ctx.scene.GetRootGameObjects().SelectMany(x => x.GetComponentsInChildren<VRCLightVolumes.LightVolume>(false)).ToArray();

            for (int i = 0; i < vrclv.Length; i++)
            {
                var lv = vrclv[i];
                var lvData = new LightProbeVolumeData
                {
                    indexStart = ctx.probePositions.Count,
                    id = i,
                    resolution = lv.Resolution,
                };

                ctx.probeVolumes.Add(lvData);
                var volume = GenerateProbeVolume(lv.transform.position, lv.transform.lossyScale, lv.Resolution);
                ctx.probePositions.AddRange(volume);
            }
        }

        static void CreateLightVolumeTextures(BakeContext ctx, string directory)
        {
            var lvs = ctx.probeVolumes;
            var vrclv = ctx.scene.GetRootGameObjects().SelectMany(x => x.GetComponentsInChildren<VRCLightVolumes.LightVolume>(false)).ToArray();

            for (int volumeIndex = 0; volumeIndex < lvs.Count; volumeIndex++)
            {
                var data = lvs[volumeIndex];

                int w = data.resolution.x;
                int h = data.resolution.y;
                int d = data.resolution.z;

                int probeCount = w * h * d;

                TextureFormat format = TextureFormat.RGBAHalf;
                Texture3D tex0 = new(w, h, d, format, false) { wrapMode = TextureWrapMode.Clamp, filterMode = FilterMode.Bilinear };
                Texture3D tex1 = new(w, h, d, format, false) { wrapMode = TextureWrapMode.Clamp, filterMode = FilterMode.Bilinear };
                Texture3D tex2 = new(w, h, d, format, false) { wrapMode = TextureWrapMode.Clamp, filterMode = FilterMode.Bilinear };

                Color[] tex0Col = new Color[probeCount];
                Color[] tex1Col = new Color[probeCount];
                Color[] tex2Col = new Color[probeCount];

                float coeff = 1.0f;// todo
                // float coeff = 1.7699115f;// todo

                int pixelIndex = 0;
                for (int i = data.indexStart; i < data.indexStart + probeCount; i++)
                {
                    var probe = _bakeProbesResults[i];

                    var L0 = probe.L0;
                    var L1x = probe.L11;
                    var L1y = probe.L1_1;
                    var L1z = probe.L10;

                    var L1r = new Vector3(L1x.x, L1y.x, L1z.x);
                    var L1g = new Vector3(L1x.y, L1y.y, L1z.y);
                    var L1b = new Vector3(L1x.z, L1y.z, L1z.z);

                    tex0Col[pixelIndex] = new Color(L0.x, L0.y, L0.z, L1r.z * coeff);
                    tex1Col[pixelIndex] = new Color(L1r.x * coeff, L1g.x * coeff, L1b.x * coeff, L1g.z * coeff);
                    tex2Col[pixelIndex] = new Color(L1r.y * coeff, L1g.y * coeff, L1b.y * coeff, L1b.z * coeff);

                    pixelIndex++;
                }

                tex0.SetPixels(tex0Col);
                tex1.SetPixels(tex1Col);
                tex2.SetPixels(tex2Col);

                AssetDatabase.CreateAsset(tex0, Path.Combine(directory, $"LightProbeVolume_{volumeIndex}-0.asset"));
                AssetDatabase.CreateAsset(tex1, Path.Combine(directory, $"LightProbeVolume_{volumeIndex}-1.asset"));
                AssetDatabase.CreateAsset(tex2, Path.Combine(directory, $"LightProbeVolume_{volumeIndex}-2.asset"));

                var lv = vrclv[volumeIndex];
                lv.Texture0 = tex0;
                lv.Texture1 = tex1;
                lv.Texture2 = tex2;
                EditorUtility.SetDirty(lv);
            }

            var lvSetup = ctx.scene.GetRootGameObjects().SelectMany(x => x.GetComponentsInChildren<VRCLightVolumes.LightVolumeSetup>(false)).FirstOrDefault();
            if (lvSetup)
            {
                lvSetup.GenerateAtlas();
            }

        }
#endif

        // Refocus the window for QoL
        static void RestoreSelection()
        {
            var baker = UnityEngine.Object.FindAnyObjectByType<LightmapBaker>();

            if (baker != null)
            {
                Selection.activeGameObject = baker.gameObject;
            }
        }

        public static void Start(LightmapBaker baker, Bindings.GlimConfig config)
        {
            if (_running)
            {
                Debug.LogError("Bake already running");
                return;
            }

            ResetBake();

            EditorApplication.update += PollBakeComplete;

            var ctx = new BakeContext(baker, config);

#if VRC_LIGHT_VOLUMES
            AddLightProbeVolumes(baker, ctx);
#endif

            _context = ctx;

            _running = true;
            _isPreview = config.is_preview;

            if (!config.is_preview)
            {
                _progressID = Progress.Start(BakingTitle, null, Progress.Options.None);
                Progress.RegisterCancelCallback(_progressID, () => { Cancel(); return true; });
                _progressTitle = BakingTitle;
                RestoreSelection();
            }

            _bakeStartTime = Time.realtimeSinceStartupAsDouble;
            var thread = new Thread(() =>
            {
                try
                {
                    var output_dir = Bindings.FfiString.FromString(ctx.outputDir);
                    var app = Bindings.app_new(config, output_dir);

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
                                    emissive = data.emissive,
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
                        Vector3 p = (Vector3)position;
                        float r = position.w;
                        Bindings.app_add_probe(app, p, r);
                    }

                    uint size = (uint)SkyboxCapture.RESOLUTION;
                    Bindings.app_set_skybox(app, _context.skyboxPixels, (uint)_context.skyboxPixels.Length * 4, size, size);

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

        public static string CreateTextureImporterMeta(string guid, bool directional)
        {
            int alphaUsage = directional ? 1 : 0;
            int textureType = directional ? 12 : 6;
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
    aniso: 0
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
  alphaUsage: {alphaUsage}
  alphaIsTransparency: 0
  spriteTessellationDetail: -1
  textureType: {textureType}
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
