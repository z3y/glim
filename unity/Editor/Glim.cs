using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using UnityEditor;
using UnityEditor.SceneManagement;
using UnityEngine;
using UnityEngine.Experimental.Rendering;
using UnityEngine.Rendering;
using UnityEngine.SceneManagement;
using Debug = UnityEngine.Debug;
using Object = UnityEngine.Object;

namespace Glim
{
    public class BakeContextGroup
    {
        public Color32[] albedo;
        public byte[] emission;
        public Bindings.LightmapSettings settings;
        public GlimLightmapGroup groupAsset;

        public BakeContextGroup(GlimLightmapGroup group, IList<Renderer> renderers)
        {
            groupAsset = group;

            settings = new Bindings.LightmapSettings(group);

            using var metaAlbedo = new MetaTexture((int)settings.width, MetaTexture.AtlasType.Albedo);
            using var metaEmission = new MetaTexture((int)settings.width, MetaTexture.AtlasType.Emission);

            // The two atlases are independent, so issue both readbacks before blocking on
            // either and let them overlap rather than stalling on each in turn.
            var albedoRequest = metaAlbedo.CreateAtlas(renderers, MetaTexture.AtlasType.Albedo);
            var emissionRequest = metaEmission.CreateAtlas(renderers, MetaTexture.AtlasType.Emission);

            albedoRequest.WaitForCompletion();
            emissionRequest.WaitForCompletion();

            albedo = albedoRequest.GetData<Color32>().ToArray();
            emission = emissionRequest.GetData<byte>().ToArray();

            // var albedoAtlas = new Texture2D((int)settings.width, (int)settings.height, TextureFormat.ARGB32, 1, true);
            // albedoAtlas.SetPixels32(albedo);
            // AssetDatabase.CreateAsset(albedoAtlas, "Assets/AbledoAtlas.asset");
            // var emissionAtlas = new Texture2D((int)settings.width, (int)settings.height, TextureFormat.RGBAFloat, 1, true);
            // emissionAtlas.SetPixels(emission);
            // AssetDatabase.CreateAsset(emissionAtlas, "Assets/EmissionAtlas.asset");


            // var albedoAtlas = new Texture2D((int)settings.width, (int)settings.height, TextureFormat.ARGB32, 1, true);
            // albedoAtlas.SetPixels32(albedo);
            // var albedoBytes = albedoAtlas.EncodeToTGA();
            // File.WriteAllBytes("Assets/AbledoAtlas.tga", albedoBytes);

            // Debug.Log($"Group width: {settings.width}, height:{settings.height}");
        }

        public BakeContextGroup(GlimLightmapGroup group, Terrain terrain)
        {
            groupAsset = group;

            settings = new Bindings.LightmapSettings(group);

            using var metaAlbedo = new TerrainMetaTexture((int)settings.width);
            albedo = metaAlbedo.CreateAtlas(terrain);
            emission = new byte[group.Width * group.Height * 4];
        }

        public void ClearPixels()
        {
            albedo = new Color32[0];
            emission = new byte[0];
        }
    }

    public class LightProbeVolumeData
    {
        public int id;
        public int indexStart;
        public Vector3Int resolution;
    }

    public struct CookiePixelsData
    {
        public Color32[] pixels;
        public uint width;
        public uint height;
    }

    public struct AlphaPixelsData
    {
        public byte[] pixels;
        public uint width;
        public uint height;
    }

    public class BakeContext
    {
        public List<Bindings.Light> bakeLights = new();
        public List<CookiePixelsData> cookies = new();
        public List<AlphaPixelsData> alphas = new();
        public List<Glim.MeshData> sceneMesh = new();
        public List<BakeContextGroup> groups = new();

        public List<Vector4> probePositions = new();

        public List<LightProbeVolumeData> probeVolumes = new();

        public LightingDataAsset storage;
        public Scene scene;

        public bool reflectionProbesSuperSampling;
        public bool reflectionProbesSpecular;
        public bool bakeReflectionProbes;
        public string skyboxReflectionPath;

        public string outputDir;
        public LightmapMode lightmapMode;
        public MixedLightMode mixedMode;
        public bool isPreview;

        public Color[] skyboxPixels;

        private static int GetDepth(Transform t)
        {
            int depth = 0;
            while (t.parent != null) { t = t.parent; depth++; }
            return depth;
        }

        public BakeContext(GlimLightmapper lightmapper, Bindings.GlimConfig config)
        {
            this.lightmapMode = lightmapper.lightmapMode;
            this.reflectionProbesSuperSampling = lightmapper.reflectionProbesSuperSampling;
            this.reflectionProbesSpecular = lightmapper.reflectionProbesSpecular;
            this.isPreview = config.is_preview;
            this.bakeReflectionProbes = lightmapper.bakeReflectionProbes;

            this.skyboxPixels = SkyboxCapture.Capture(SceneManager.GetActiveScene());

            var bakerId = GlobalObjectId.GetGlobalObjectIdSlow(lightmapper);

            SerializedObject lda;
            if (!config.is_preview)
            {
                // scene reopened, references lost here
                storage = LightingData.CreateAsset(SceneManager.GetActiveScene());
                lda = new SerializedObject(storage);
                LightingData.InspectorModeObject.SetValue(lda, InspectorMode.DebugInternal);
            }
            else
            {
                lda = null;
            }

            scene = SceneManager.GetActiveScene();
            var rootObjects = scene.GetRootGameObjects().Where(x => x.activeInHierarchy);
            lightmapper = (GlimLightmapper)GlobalObjectId.GlobalObjectIdentifierToObjectSlow(bakerId);

            this.outputDir = Path.Combine(Path.GetDirectoryName(scene.path), scene.name);
            if (!AssetDatabase.IsValidFolder(this.outputDir))
            {
                AssetDatabase.CreateFolder(Path.GetDirectoryName(scene.path), scene.name);
            }

            this.skyboxReflectionPath = Path.Combine(this.outputDir, "SkyboxReflection.exr");
            SkyboxCapture.SaveAsCubemapAsset(skyboxPixels, this.skyboxReflectionPath);

            var ftraceLightmaps = rootObjects.FirstOrDefault(x => x.gameObject.name == "!ftraceLightmaps");
            if (ftraceLightmaps != null)
            {
                // bakery is breaking directional lightmaps, need to remove this from scene
                // bakery always creates this object just by having the "Render Lightmap" window open
                // so make sure to close it and reopen the scene
                bool confirmed = EditorUtility.DisplayDialog("Remove bakery script", "Remove the hidden Bakery !ftraceLightmaps GameObject to prevent conflicts.", "Continue");
                if (confirmed)
                {
                    GameObject.DestroyImmediate(ftraceLightmaps.gameObject);
                    EditorSceneManager.MarkSceneDirty(scene);
                    rootObjects = scene.GetRootGameObjects().Where(x => x.activeInHierarchy);
                }
            }

            var lights = rootObjects.SelectMany(x => x.GetComponentsInChildren<Light>(false)).ToArray();
            var builtIn = GraphicsSettings.currentRenderPipeline == null;

            var uniqueCookies = new List<Texture>();

            var addedLights = new List<Light>(lights.Length);
            foreach (var light in lights)
            {
                if (light.lightmapBakeType == LightmapBakeType.Realtime)
                {
                    continue;
                }

                bool mixed = light.lightmapBakeType == LightmapBakeType.Mixed;

                var gammaColor = light.color;
                if (light.useColorTemperature)
                {
                    Color temperature = Mathf.CorrelatedColorTemperatureToRGB(light.colorTemperature).gamma;
                    gammaColor *= temperature;
                }
                var linear = gammaColor.linear;

                var color = new Vector3(linear.r, linear.g, linear.b) * light.intensity;

                var lightType = Bindings.LightType.Directional;
                if (light.type == LightType.Directional)
                {
                    lightType = Bindings.LightType.Directional;
                }
                else if (light.type == LightType.Point)
                {
                    lightType = Bindings.LightType.Point;
                }
                else if (light.type == LightType.Rectangle)
                {
                    lightType = Bindings.LightType.Area;
                }
                else if (light.type == LightType.Disc)
                {
                    lightType = Bindings.LightType.Disc;
                }

                float radiusOrAngle = light.type == LightType.Directional ?
                    Mathf.Deg2Rad * light.shadowAngle : light.shadowRadius;

                var l = new Bindings.Light
                {
                    ty = lightType,
                    position = light.transform.position,
                    direction = light.transform.forward,
                    up = light.transform.up,
                    range = light.range,
                    color = color,
                    shadow_radius_or_angle = radiusOrAngle,
                    mixed = light.lightmapBakeType == LightmapBakeType.Mixed ? (byte)1 : (byte)0,
                    cookie = uint.MaxValue,
                    cast_shadows = light.shadows == LightShadows.None ? (byte)0 : (byte)1,
                    occlusion_channel = -1,
                };


#if UNITY_6000_1_OR_NEWER
                l.area_or_cookie_size = light.cookieSize2D;
#else
                l.area_or_cookie_size = new Vector2(light.cookieSize, light.cookieSize);
#endif

                if (light.type == LightType.Spot)
                {
                    l.spot_outer = light.spotAngle;
                    l.ty = Bindings.LightType.Spot;
                    l.direction = -light.transform.forward;
                    l.spot_inner_percent = 80;
                }
                if (light.type == LightType.Rectangle)
                {
                    l.direction = -light.transform.forward;
                    l.area_or_cookie_size = light.areaSize;
                }
                if (light.type == LightType.Disc)
                {
                    l.direction = -light.transform.forward;
                    l.area_or_cookie_size = new Vector2(light.areaSize.x, light.areaSize.x);
                }

                bool lightSupportsCookie = light.type == LightType.Spot ||
                    light.type == LightType.Directional ||
                    light.type == LightType.Point ||
                    light.type == LightType.Rectangle ||
                    light.type == LightType.Disc;

                if (light.cookie && lightSupportsCookie)
                {
                    if (uniqueCookies.Contains(light.cookie))
                    {
                        l.cookie = (uint)uniqueCookies.IndexOf(light.cookie);
                    }
                    else
                    {
                        uniqueCookies.Add(light.cookie);
                        l.cookie = (uint)uniqueCookies.Count - 1;
                    }
                }

                if (!builtIn)
                {
                    l.spot_inner_percent = light.innerSpotAngle / light.spotAngle * 100.0f;
                }

                var glimLight = light.GetComponent<GlimAdditionalLightSettings>();
                if (glimLight && glimLight.enabled)
                {
                    l.spot_inner_percent = light.innerSpotAngle / light.spotAngle * 100.0f;

                    if ((light.type == LightType.Rectangle || light.type == LightType.Disc) && glimLight.areaCookie)
                    {
                        if (uniqueCookies.Contains(glimLight.areaCookie))
                        {
                            l.cookie = (uint)uniqueCookies.IndexOf(glimLight.areaCookie);
                        }
                        else
                        {
                            uniqueCookies.Add(glimLight.areaCookie);
                            l.cookie = (uint)uniqueCookies.Count - 1;
                        }
                    }
                }


                addedLights.Add(light);
                bakeLights.Add(l);
            }

            foreach (var cookie in uniqueCookies)
            {
                if (cookie is Texture2D tex)
                {
                    var pixels = GetPixels32(tex);
                    cookies.Add(new CookiePixelsData()
                    {
                        pixels = pixels,
                        width = (uint)tex.width,
                        height = (uint)tex.height,
                    });
                }
                else if (cookie is Cubemap cube)
                {
                    var pixels = CubemapToOctahedralPixels(cube, out int size);
                    cookies.Add(new CookiePixelsData
                    {
                        pixels = pixels,
                        width = (uint)size,
                        height = (uint)size,
                    });
                }
            }


            this.mixedMode = lightmapper.mixedMode;
            var mixedMode = lightmapper.mixedMode switch
            {
                MixedLightMode.BakedIndirect => MixedLightingMode.IndirectOnly,
                // MixedLightMode.Subtractive => MixedLightingMode.Subtractive,
                MixedLightMode.Shadowmask => MixedLightingMode.Shadowmask,
                _ => MixedLightingMode.IndirectOnly,
            };

            var lightsArray = addedLights.ToArray();
            List<int> occlusionChannels = new();
            if (mixedMode == MixedLightingMode.Shadowmask)
            {
                occlusionChannels = ShadowMaskChannelAssigner.Assign(lightsArray);
            }

            if (!config.is_preview)
            {
                var lightsProp = lda.FindProperty("m_Lights");
                var lightsOutputsProp = lda.FindProperty("m_LightBakingOutputs");
                Debug.Assert(lightsProp != null);
                Debug.Assert(lightsOutputsProp != null);

                lightsProp.arraySize = lightsArray.Length;
                lightsOutputsProp.arraySize = lightsArray.Length;
                for (int lightIndex = 0; lightIndex < lightsArray.Length; lightIndex++)
                {
                    var light = lightsArray[lightIndex];
                    var outputElement = lightsOutputsProp.GetArrayElementAtIndex(lightIndex);
                    var ids = lightsProp.GetArrayElementAtIndex(lightIndex);

                    int occlusionChannel = -1;
                    if (occlusionChannels.Count > 0)
                    {
                        occlusionChannel = occlusionChannels[lightIndex];
                        var l = bakeLights[lightIndex];
                        l.occlusion_channel = (sbyte)occlusionChannel;
                        bakeLights[lightIndex] = l;
                    }

                    outputElement.FindPropertyRelative("probeOcclusionLightIndex").intValue = 0;
                    outputElement.FindPropertyRelative("occlusionMaskChannel").intValue = occlusionChannel;

                    var mixedLightingMode = mixedMode;

                    if (occlusionChannel == -1)
                    {
                        mixedLightingMode = MixedLightingMode.IndirectOnly;
                    }

                    var mode = outputElement.FindPropertyRelative("lightmapBakeMode");
                    mode.FindPropertyRelative("lightmapBakeType").intValue = (int)light.lightmapBakeType;
                    mode.FindPropertyRelative("mixedLightingMode").intValue = (int)mixedLightingMode;

                    outputElement.FindPropertyRelative("isBaked").boolValue = true;

                    var soi = LightingData.ObjectToSOI(light);

                    ids.Next(true);
                    ids.longValue = soi.MainLFID;
                    ids.Next(false);
                    ids.longValue = soi.PrefabLFID;
                }
            }


            var allSelectors = rootObjects
                .SelectMany(x => x.GetComponentsInChildren<GlimLightmapGroupSelector>(false))
                .Where(x => x.enabled)
                .ToArray();

            Array.Sort(allSelectors, (a, b) => GetDepth(b.transform).CompareTo(GetDepth(a.transform)));

            var groupMap = new Dictionary<GlimLightmapGroup, List<MeshRenderer>>();
            var claimed = new HashSet<MeshRenderer>();

            foreach (var selector in allSelectors)
            {
                if (selector.group == null) continue;

                var renderers = selector.GetComponentsInChildren<MeshRenderer>(false)
                    .Where(x => Glim.IsLightmapStatic(x));

                foreach (var r in renderers)
                {
                    if (claimed.Add(r))
                    {
                        if (!groupMap.TryGetValue(selector.group, out var list))
                        {
                            list = new List<MeshRenderer>();
                            groupMap[selector.group] = list;
                        }
                        list.Add(r);
                    }
                }
            }

            var allRenderers = rootObjects.SelectMany(x => x.GetComponentsInChildren<MeshRenderer>(false)).ToList();
            var allLodGroups = rootObjects.SelectMany(x => x.GetComponentsInChildren<LODGroup>(false));

            // keep only highest level LODs for bake
            foreach (var group in allLodGroups)
            {
                var lods = group.GetLODs();

                for (int i = 1; i < lods.Length; i++)
                {
                    LOD lod = lods[i];

                    foreach (var renderer in lods[i].renderers)
                    {
                        if (renderer is MeshRenderer mr)
                        {
                            allRenderers.Remove(mr);
                        }
                    }
                }

            }

            var unclaimedRenderers = new List<MeshRenderer>();
            foreach (var r in allRenderers)
            {
                if (claimed.Contains(r))
                {
                    continue;
                }

                if (Glim.IsLightmapStatic(r))
                {
                    unclaimedRenderers.Add(r);
                }
            }
            var globalGroup = lightmapper.group == null ? ScriptableObject.CreateInstance<GlimLightmapGroup>() : lightmapper.group;
            if (unclaimedRenderers.Count > 0)
            {
                groupMap[globalGroup] = unclaimedRenderers;
            }

            uint groupIndex = 0;
            int mrDataOffset = 0;
            foreach (var (lightmapGroup, renderers) in groupMap)
            {
                var rendererArray = renderers.ToArray();

                // var hashData = new uint[4];
                // hashData[0] = (uint)lightmapGroup.resolution;
                // hashData[1] = lightmapGroup.holeFilling ? 1u : 0u;
                // hashData[2] = lightmapGroup.packingIterations;
                // hashData[3] = (uint)BitConverter.SingleToInt32Bits(lightmapGroup.scaleExponent);

                // var newHash = MeshHash.FromLightmapUV(rendererArray, hashData);

                // bool changed = lightmapGroup.lightmapUVHash != newHash;

                // if (!lightmapper.enableUVCache)
                // {
                //     changed = true;
                // }

                if (true)
                {
                    if (lightmapGroup.packingType == UVPackingType.ScaleOffset || lightmapGroup.packingType == UVPackingType.SplitCharts)
                    {
                        var sw = new Stopwatch();
                        sw.Start();

                        bool bruteForce = lightmapGroup.bruteForce;
                        bool holeFilling = lightmapGroup.holeFilling;
                        float worldScaleExponent = lightmapGroup.scaleExponent;
                        bool split = lightmapGroup.packingType == UVPackingType.SplitCharts;
                        var packer = UVPacking.uvpacker_create(lightmapGroup.Width, lightmapGroup.Height, lightmapGroup.packingIterations, bruteForce, holeFilling, split, worldScaleExponent);
                        for (int rendererIndex = 0; rendererIndex < renderers.Count; rendererIndex++)
                        {
                            Renderer r = renderers[rendererIndex];
                            var mf = r.GetComponent<MeshFilter>();
                            var t = r.GetComponent<Transform>();

                            var mesh = mf.sharedMesh;

                            bool hasUv0 = mesh.HasVertexAttribute(VertexAttribute.TexCoord0);
                            bool hasUv1 = mesh.HasVertexAttribute(VertexAttribute.TexCoord1);

                            var positions = mesh.vertices;
                            t.TransformPoints(positions); // todo slow, verts are transformed again later
                            var uvs = hasUv1 ? mesh.uv2 : mesh.uv;
                            var indices = mesh.triangles;
                            float scale = 1.0f;
                            if (r is MeshRenderer mr)
                            {
                                scale = mr.scaleInLightmap;
                            }

                            unsafe
                            {
                                fixed (Vector3* p = positions)
                                fixed (Vector2* uv = uvs)
                                fixed (int* i = indices)
                                {
                                    UVPacking.uvpacker_add_mesh(packer, p, (uint)positions.Length, uv, (uint)uvs.Length, i, (uint)indices.Length, scale, (uint)rendererIndex);
                                }
                            }
                        }

                        bool success = UVPacking.uvpacker_pack(packer);

                        if (!success)
                        {
                            throw new Exception("UV Packing failed, try increasing lightmap resolution, packing iteration count or brute force mode or disable ensure padding");
                        }

                        sw.Stop();
                        var elapsed = sw.ElapsedMilliseconds;

                        for (int rendererIndex = 0; rendererIndex < renderers.Count; rendererIndex++)
                        {
                            Renderer r = renderers[rendererIndex];

                            if (r is MeshRenderer mr)
                            {
                                if (split)
                                {
                                    var uvs = UVPacking.GetMeshUvs(packer, (uint)rendererIndex);
                                    if (uvs != null)
                                    {
                                        var mf = r.GetComponent<MeshFilter>();
                                        var clone = Object.Instantiate(mf.sharedMesh);
                                        if (uvs.Length != clone.vertexCount)
                                        {
                                            Debug.LogError($"Uv packing failed for renderer {r.name}", r);
                                        }
                                        clone.name = "GlimUV_" + clone.name;
                                        clone.uv2 = uvs;
                                        mr.enlightenVertexStream = clone;
                                        mr.additionalVertexStreams = null;
                                        r.lightmapScaleOffset = new Vector4(1, 1, 0, 0);
                                    }
                                    else
                                    {
                                        Debug.LogError($"Uv packing failed for renderer {r.name}", r);
                                    }
                                }
                                else
                                {
                                    mr.enlightenVertexStream = null;
                                    mr.additionalVertexStreams = null;

                                    var so = UVPacking.uvpacker_get_scale_offset(packer, (uint)rendererIndex);
                                    mr.lightmapScaleOffset = so;
                                }
                                EditorUtility.SetDirty(mr);
                            }

                        }

                        float coverage = UVPacking.uvpacker_get_coverage(packer);
                        Bake.bakeMessages.AppendLine($"Group {groupIndex} UVs packed in {elapsed}ms with {coverage * 100.0f}% coverage");

                        // lightmapGroup.lightmapUVHash = newHash;
                        EditorUtility.SetDirty(lightmapper);

                        UVPacking.uvpacker_destroy(packer);
                    }
                    else
                    {
                        foreach (var r in renderers)
                        {
                            if (r is MeshRenderer mr)
                            {
                                mr.enlightenVertexStream = null;
                                mr.additionalVertexStreams = null;
                                EditorUtility.SetDirty(mr);
                            }
                            r.lightmapScaleOffset = new Vector4(1, 1, 0, 0);
                            EditorUtility.SetDirty(r);
                        }
                    }
                }
                // else
                // {
                //     Bake.bakeMessages.AppendLine($"Group {groupIndex} using cached lightmap UVs");
                // }

                bool saveGroup = !(config.is_preview || lightmapGroup.discard);

                if (saveGroup)
                {
                    var rendererDataIds = lda.FindProperty("m_LightmappedRendererDataIDs");
                    var rendererData = lda.FindProperty("m_LightmappedRendererData");
                    rendererDataIds.arraySize += rendererArray.Length;
                    rendererData.arraySize += rendererArray.Length;

                    for (int i = 0; i < rendererArray.Length; i++)
                    {
                        MeshRenderer mr = rendererArray[i];
                        var scaleOffset = mr.lightmapScaleOffset;

                        var ids = rendererDataIds.GetArrayElementAtIndex(mrDataOffset + i);
                        var lmData = rendererData.GetArrayElementAtIndex(mrDataOffset + i);

                        var soi = LightingData.ObjectToSOI(mr);

                        ids.Next(true);
                        ids.longValue = soi.MainLFID;
                        ids.Next(false);
                        ids.longValue = soi.PrefabLFID;

                        lmData.FindPropertyRelative("lightmapIndex").intValue = (int)groupIndex;
                        lmData.FindPropertyRelative("lightmapST").vector4Value = scaleOffset;
                        lmData.FindPropertyRelative("lightmapSTDynamic").vector4Value = new Vector4(1, 1, 0, 0);

                        lmData.FindPropertyRelative("terrainDynamicUVST").vector4Value = scaleOffset;
                        lmData.FindPropertyRelative("terrainChunkDynamicUVST").vector4Value = scaleOffset;

                        lmData.FindPropertyRelative("lightmapIndexDynamic").intValue = 65535;

                        var uvMeshProp = lmData.FindPropertyRelative("uvMesh");
                        if (lightmapGroup.packingType == UVPackingType.SplitCharts && mr.enlightenVertexStream)
                        {
                            Mesh uvMesh = mr.enlightenVertexStream;
                            AssetDatabase.AddObjectToAsset(uvMesh, lda.targetObject);
                            uvMeshProp.objectReferenceValue = uvMesh;
                        }
                        else
                        {
                            uvMeshProp.objectReferenceValue = null;
                        }
                    }

                    mrDataOffset = rendererData.arraySize;
                }

                groups.Add(new BakeContextGroup(lightmapGroup, rendererArray));
                sceneMesh.AddRange(Glim.ExtractMeshData(rendererArray, groupIndex));
                groupIndex++;
            }

            var terrains = rootObjects
                .SelectMany(x => x.GetComponentsInChildren<Terrain>(false))
                .Where(t => t.enabled && t.gameObject.activeInHierarchy
                    && GameObjectUtility.GetStaticEditorFlags(t.gameObject).HasFlag(StaticEditorFlags.ContributeGI));

            foreach (var terrain in terrains)
            {
                var data = terrain.terrainData;
                if (data == null) continue;

                Vector4 scaleOffset = new(1, 1, 0, 0);

                bool hasHoles = data.holesTexture != null;

                terrain.lightmapScaleOffset = scaleOffset;
                EditorUtility.SetDirty(terrain);

                Vector3 position = terrain.transform.position;

                var mesh = TerrainExporter.GenerateMesh(data, position, step: 4);

                GlimLightmapGroup terrainGroup = null;
                var selector = terrain.GetComponent<GlimLightmapGroupSelector>();
                if (selector && selector.group)
                {
                    terrainGroup = selector.group;
                }
                else
                {
                    terrainGroup = ScriptableObject.CreateInstance<GlimLightmapGroup>();
                }
                terrainGroup.packingType = UVPackingType.None;
                EditorUtility.SetDirty(terrainGroup);

                groups.Add(new BakeContextGroup(terrainGroup, terrain));

                var meshData = new Glim.MeshData
                {
                    vertices = mesh.vertices,
                    normals = mesh.normals,
                    lightmapUVs = mesh.uv,
                    mainUVs = mesh.uv,
                    indices = mesh.triangles,
                    groupIndex = groupIndex,
                    backfaceGI = false,
                    transparent = hasHoles,
                    emissive = false,
                    fixSeams = false,
                    alphaTexture = data.holesTexture,
                    alphaWrap = false,
                    cutoff = 0.5f,
                };
                sceneMesh.Add(meshData);
                GameObject.DestroyImmediate(mesh);

                if (!config.is_preview)
                {
                    var rendererDataIds = lda.FindProperty("m_LightmappedRendererDataIDs");
                    var rendererData = lda.FindProperty("m_LightmappedRendererData");
                    rendererDataIds.arraySize += 1;
                    rendererData.arraySize += 1;
                    int lastIndex = rendererData.arraySize - 1;

                    var ids = rendererDataIds.GetArrayElementAtIndex(lastIndex);
                    var lmData = rendererData.GetArrayElementAtIndex(lastIndex);

                    var soi = LightingData.ObjectToSOI(terrain);

                    ids.Next(true);
                    ids.longValue = soi.MainLFID;
                    ids.Next(false);
                    ids.longValue = soi.PrefabLFID;

                    lmData.FindPropertyRelative("lightmapIndex").intValue = (int)groupIndex;
                    lmData.FindPropertyRelative("lightmapST").vector4Value = scaleOffset;
                    lmData.FindPropertyRelative("lightmapSTDynamic").vector4Value = scaleOffset;
                    lmData.FindPropertyRelative("terrainDynamicUVST").vector4Value = scaleOffset;
                    lmData.FindPropertyRelative("terrainChunkDynamicUVST").vector4Value = scaleOffset;
                    lmData.FindPropertyRelative("lightmapIndexDynamic").intValue = 65535;
                    lmData.FindPropertyRelative("uvMesh").objectReferenceValue = null;
                }

                groupIndex++;
            }

            List<Texture> alphaTextures = new();
            var alphaLookup = new Dictionary<Texture, int>();
            foreach (var mesh in sceneMesh)
            {
                if (!mesh.alphaTexture)
                {
                    mesh.alphaTextureIndex = -1;
                    continue;
                }

                if (!alphaLookup.TryGetValue(mesh.alphaTexture, out int index))
                {
                    index = alphaTextures.Count;
                    alphaLookup[mesh.alphaTexture] = index;
                    alphaTextures.Add(mesh.alphaTexture);

                    alphas.Add(new AlphaPixelsData
                    {
                        pixels = GetAlphaPixels(mesh.alphaTexture),
                        width = (uint)mesh.alphaTexture.width,
                        height = (uint)mesh.alphaTexture.height,
                    });
                }

                mesh.alphaTextureIndex = index;
                mesh.alphaTexture = null; // drop the reference, only the index is needed from here on
            }


            if (groupIndex <= 0)
            {
                throw new InvalidOperationException("No static objects or lightmap groups found.");
            }

            if (!lightmapper.group)
            {
                ScriptableObject.DestroyImmediate(globalGroup);
            }

            float defaultProbeRadius = lightmapper.lightProbeRadius;

            if (!config.is_preview)
            {
                var lightProbesRef = lda.FindProperty("m_LightProbes").objectReferenceValue;
                using var probesSo = new SerializedObject(lightProbesRef);
                LightingData.InspectorModeObject.SetValue(probesSo, InspectorMode.DebugInternal);
                var probePositions = probesSo.FindProperty("m_Data").FindPropertyRelative("m_Positions");
                int probesCount = probePositions.arraySize;

                for (int i = 0; i < probesCount; i++)
                {
                    var element = probePositions.GetArrayElementAtIndex(i);
                    var probe = (Vector4)element.vector3Value;
                    probe.w = defaultProbeRadius;
                    this.probePositions.Add(probe);
                }

                lda.ApplyModifiedPropertiesWithoutUndo();
                lda.Dispose();
            }

            // Debug.Log($"Vertices: {sceneMesh.Sum(x => x.vertices.Length)}");
            // Debug.Log($"Indices: {sceneMesh.Sum(x => x.triangles.Length)}");
            // Debug.Log($"Lights: {sceneLights.Count}");
            // Debug.Log($"LightProbes: {this.probePositions.Count}");
        }

        public static Color32[] GetPixels32(Texture2D texture)
        {
            if (texture.isReadable)
            {
                return texture.GetPixels32();
            }

            RenderTexture render_texture = RenderTexture.GetTemporary(texture.width, texture.height, 0, RenderTextureFormat.ARGB32);

            Graphics.Blit(texture, render_texture);

            RenderTexture previous = RenderTexture.active;
            RenderTexture.active = render_texture;

            Texture2D readable = new Texture2D(texture.width, texture.height, TextureFormat.RGBA32, false);

            readable.ReadPixels(new Rect(0, 0, texture.width, texture.height), 0, 0);

            readable.Apply();

            Color32[] pixels = readable.GetPixels32();

            Object.DestroyImmediate(readable);

            RenderTexture.active = previous;
            RenderTexture.ReleaseTemporary(render_texture);

            return pixels;
        }

        // Keeps only the last channel: 1 comp -> R, 2 -> G, 4 -> A.
        // 3 comps (RGB) has no alpha, so it returns 1.0 (opaque).
        // Accepts RenderTexture too (e.g. terrain holes, R8 unorm where 1 = solid, 0 = hole).
        public static byte[] GetAlphaPixels(Texture texture)
        {
            int components = (int)GraphicsFormatUtility.GetComponentCount(texture.graphicsFormat);
            bool alpha8 = texture is Texture2D t2 && t2.format == TextureFormat.Alpha8; // samples as (1,1,1,a)

            Color[] pixels;
            if (texture is Texture2D { isReadable: true } readableTex)
            {
                pixels = readableTex.GetPixels();
            }
            else
            {
                // RenderTexture or non-readable Texture2D: blit to a float RT and read back
                var rt = RenderTexture.GetTemporary(texture.width, texture.height, 0, RenderTextureFormat.ARGBFloat, RenderTextureReadWrite.Linear);
                Graphics.Blit(texture, rt);

                var previous = RenderTexture.active;
                RenderTexture.active = rt;

                var readable = new Texture2D(texture.width, texture.height, TextureFormat.RGBAFloat, false, true);
                readable.ReadPixels(new Rect(0, 0, texture.width, texture.height), 0, 0);
                pixels = readable.GetPixels();

                Object.DestroyImmediate(readable);
                RenderTexture.active = previous;
                RenderTexture.ReleaseTemporary(rt);
            }

            var result = new byte[pixels.Length];
            for (int i = 0; i < pixels.Length; i++)
            {
                float a;
                if (alpha8) a = pixels[i].a;
                else switch (components)
                    {
                        case 1: a = pixels[i].r; break;
                        case 2: a = pixels[i].g; break;
                        case 4: a = pixels[i].a; break;
                        default: a = 1.0f; break;
                    }
                result[i] = (byte)Mathf.RoundToInt(Mathf.Clamp01(a) * 255f);
            }
            return result;
        }

        static Material cubeToOctahedralMat;
        public static Color32[] CubemapToOctahedralPixels(Cubemap cube, out int size)
        {
            if (!cubeToOctahedralMat)
            {
                cubeToOctahedralMat = AssetDatabase.LoadAssetAtPath<Material>("Packages/io.github.z3y.glim/Editor/CubemapToOctahedral.mat");
            }

            size = Mathf.Clamp(cube.width * 2, 64, 2048);

            cubeToOctahedralMat.SetTexture("_Cube", cube);

            RenderTexture rt = RenderTexture.GetTemporary(size, size, 0, RenderTextureFormat.ARGB32);
            Graphics.Blit(null, rt, cubeToOctahedralMat);

            RenderTexture previous = RenderTexture.active;
            RenderTexture.active = rt;

            Texture2D readable = new Texture2D(size, size, TextureFormat.RGBA32, false);
            readable.ReadPixels(new Rect(0, 0, size, size), 0, 0);
            readable.Apply();

            Color32[] pixels = readable.GetPixels32();

            Object.DestroyImmediate(readable);
            RenderTexture.active = previous;
            RenderTexture.ReleaseTemporary(rt);

            cubeToOctahedralMat.SetTexture("_Cube", null);

            return pixels;
        }
    }



    public class Glim
    {
        public static bool IsLightmapStatic(MeshRenderer renderer)
        {
            if (!renderer.enabled)
            {
                return false;
            }

            var gameObject = renderer.gameObject;

            if (gameObject.activeInHierarchy == false)
            {
                return false;
            }

            if (!GameObjectUtility.GetStaticEditorFlags(gameObject).HasFlag(StaticEditorFlags.ContributeGI))
            {
                return false;
            }

            if (renderer.receiveGI != ReceiveGI.Lightmaps)
            {
                return false;
            }

            if (gameObject.transform is RectTransform)
            {
                return false;
            }

            if (renderer.scaleInLightmap == 0)
            {
                Debug.LogWarning($"Scale In Lightmap 0 not supported <a href=\"https://github.com/z3y/glim/issues/19\">(Read Issue #19)</a> GameObject {renderer.name} will not be baked.", renderer.gameObject);
                return false;
            }

            var filter = renderer.GetComponent<MeshFilter>();

            if (!filter)
            {
                return false;
            }

            var mesh = filter.sharedMesh;

            if (mesh == null)
            {
                return false;
            }

            var vertices = mesh.vertexCount;

            if (vertices <= 0)
            {
                return false;
            }

            if (mesh.subMeshCount <= 0)
            {
                return false;
            }

            bool hasUv0 = mesh.HasVertexAttribute(VertexAttribute.TexCoord0);
            bool hasUv1 = mesh.HasVertexAttribute(VertexAttribute.TexCoord1);

            if (!(hasUv0 || hasUv1))
            {
                return false;
            }


            return true;
        }

        public class MeshData
        {
            public Vector3[] vertices;
            public Vector3[] normals;
            public Vector2[] lightmapUVs;
            public Vector2[] mainUVs;
            public int[] indices;
            public uint groupIndex;
            public bool backfaceGI;
            public bool transparent;
            public bool emissive;
            public bool fixSeams;
            public bool noShadows;

            public Texture alphaTexture;
            public int alphaTextureIndex = -1;
            public bool alphaWrap;
            public float cutoff;
        }

        public static List<MeshData> ExtractMeshData(Renderer[] renderers, uint groupIndex)
        {
            var datas = new List<MeshData>();

            var vertices = new List<Vector3>();
            var normals = new List<Vector3>();
            var lightmapUVs = new List<Vector2>();
            var mainUVs = new List<Vector2>();
            var indices = new List<int>();

            var localVertices = new List<Vector3>();
            var localNormals = new List<Vector3>();
            var localLightmapUvs = new List<Vector2>();
            var localMainUvs = new List<Vector2>();
            var localIndices = new List<int>();
            var remap = new Dictionary<int, int>();

            for (int rendererIndex = 0; rendererIndex < renderers.Length; rendererIndex++)
            {
                var mr = renderers[rendererIndex] as MeshRenderer;
                if (!mr)
                {
                    continue;
                }

                var filter = mr.GetComponent<MeshFilter>();
                if (!filter)
                {
                    continue;
                }

                var transform = mr.transform;

                Mesh mesh = filter.sharedMesh;
                if (!mesh)
                {
                    continue;
                }

                mesh.GetVertices(vertices);
                mesh.GetNormals(normals);

                if (mesh.HasVertexAttribute(VertexAttribute.TexCoord1))
                {
                    mesh.GetUVs(1, lightmapUVs);
                }
                else
                {
                    mesh.GetUVs(0, lightmapUVs);
                }

                mesh.GetUVs(0, mainUVs);

                var enlightenVertexStream = mr.enlightenVertexStream;
                var additionalVertexStreams = mr.additionalVertexStreams;
                if (enlightenVertexStream)
                {
                    if (enlightenVertexStream.HasVertexAttribute(VertexAttribute.TexCoord1))
                    {
                        enlightenVertexStream.GetUVs(1, lightmapUVs);
                    }
                }
                else if (additionalVertexStreams)
                {
                    if (additionalVertexStreams.HasVertexAttribute(VertexAttribute.TexCoord1))
                    {
                        additionalVertexStreams.GetUVs(1, lightmapUVs);
                    }
                }

                int subMeshCount = mesh.subMeshCount;
                var materials = mr.sharedMaterials;

                bool fixSeams = mr.stitchLightmapSeams;

                for (int i = 0; i < vertices.Count; i++)
                {
                    vertices[i] = transform.TransformPoint(vertices[i]);
                }

                Matrix4x4 normalMatrix = transform.localToWorldMatrix.inverse.transpose;
                for (int normalIndex = 0; normalIndex < normals.Count; normalIndex++)
                {
                    normals[normalIndex] = normalMatrix.MultiplyVector(normals[normalIndex]).normalized;
                }

                bool isNegativeScale = transform.localToWorldMatrix.determinant < 0.0f;


                Vector4 scaleOffset = mr.lightmapScaleOffset;
                Vector2 scale = new(scaleOffset.x, scaleOffset.y);
                Vector2 offset = new(scaleOffset.z, scaleOffset.w);
                for (int uvIndex = 0; uvIndex < lightmapUVs.Count; uvIndex++)
                {
                    lightmapUVs[uvIndex] = lightmapUVs[uvIndex] * scale + offset;
                }

                for (int submeshIndex = 0; submeshIndex < materials.Length; submeshIndex++)
                {
                    if (submeshIndex >= subMeshCount)
                    {
                        break;
                    }

                    Material material = materials[submeshIndex];
                    if (material == null)
                    {
                        continue;
                    }

                    var topology = mesh.GetTopology(submeshIndex);
                    if (topology != MeshTopology.Triangles)
                    {
                        Debug.LogError($"GameObject {mr.name} Mesh {mesh.name} topology != MeshTopology.Triangles", mr.gameObject);
                        continue;
                        // todo Quads
                    }

                    bool backfaceGI = material.doubleSidedGI;
                    bool transparent = MetaTexture.IsMaterialTransparent(material);
                    bool emissive = MetaTexture.IsMaterialEmissive(material);

                    mesh.GetIndices(indices, submeshIndex);

                    if (isNegativeScale)
                    {
                        for (int j = 0; j < indices.Count; j += 3)
                        {
                            (indices[j + 1], indices[j]) = (indices[j], indices[j + 1]);
                        }
                    }

                    remap.Clear();
                    localVertices.Clear();
                    localNormals.Clear();
                    localLightmapUvs.Clear();
                    localMainUvs.Clear();
                    localIndices.Clear();
                    localIndices.Capacity = indices.Count;

                    for (int j = 0; j < indices.Count; j++)
                    {
                        int globalIndex = indices[j];

                        if (!remap.TryGetValue(globalIndex, out int localIndex))
                        {
                            localIndex = localVertices.Count;
                            remap[globalIndex] = localIndex;

                            localVertices.Add(vertices[globalIndex]);
                            localNormals.Add(normals[globalIndex]);
                            localLightmapUvs.Add(lightmapUVs[globalIndex]);
                            localMainUvs.Add(mainUVs[globalIndex]);
                        }

                        localIndices.Add(localIndex);
                    }

                    if (localMainUvs.Count != localLightmapUvs.Count)
                    {
                        throw new Exception($"uv0 Count != uv1 Count on mesh {mesh.name} GameObject {mr.name}");
                    }
                    Texture2D alphaTexture = null;
                    bool alphaWrap = false;
                    float cutoff = 0.5f;
                    if (MetaTexture.IsMaterialTransparent(material))
                    {
                        alphaTexture = MetaTexture.GetMaterialAlpha(material);
                    }

                    if (alphaTexture != null)
                    {
                        Vector2 tiling;
                        Vector2 tilingOffset;
                        MetaTexture.GetMaterialAlbedoScaleOffset(material, out tiling, out tilingOffset);
                        for (int j = 0; j < localMainUvs.Count; j++)
                        {
                            localMainUvs[j] = localMainUvs[j] * tiling + tilingOffset;
                        }

                        alphaWrap = alphaTexture.wrapMode != TextureWrapMode.Clamp; // other modes can be supported too if needed
                        float baseCutoff = MetaTexture.GetMaterialCutoff(material);

                        float colorAlpha = MetaTexture.GetMaterialColorAlpha(material);
                        if (colorAlpha > 1e-5f)
                        {
                            cutoff = baseCutoff / colorAlpha;
                        }
                        else
                        {
                            cutoff = baseCutoff <= 0.0f ? 0.0f : 2.0f;
                        }
                    }

                    if (localVertices.Count != localNormals.Count ||
                        localVertices.Count != localLightmapUvs.Count ||
                        localVertices.Count != localMainUvs.Count ||
                        localIndices.Count % 3 != 0)
                    {
                        Debug.LogError($"GameObject {mr.name} with Mesh {mesh.name} has an invalid mesh.", mr.gameObject);
                        continue;
                    }

                    var data = new MeshData
                    {
                        vertices = localVertices.ToArray(),
                        normals = localNormals.ToArray(),
                        lightmapUVs = localLightmapUvs.ToArray(),
                        mainUVs = localMainUvs.ToArray(),
                        indices = localIndices.ToArray(),
                        groupIndex = groupIndex,
                        backfaceGI = backfaceGI,
                        transparent = transparent,
                        emissive = emissive,
                        fixSeams = fixSeams,
                        noShadows = mr.shadowCastingMode == ShadowCastingMode.Off,
                        alphaTexture = alphaTexture,
                        alphaWrap = alphaWrap,
                        cutoff = cutoff,
                    };

                    datas.Add(data);
                }
            }

            return datas;
        }
    }
}
