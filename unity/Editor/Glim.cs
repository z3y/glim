using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using UnityEditor;
using UnityEditor.SceneManagement;
using UnityEngine;
using UnityEngine.Rendering;
using UnityEngine.SceneManagement;
using Debug = UnityEngine.Debug;

namespace Glim
{
    public class BakeContextGroup
    {
        public Color32[] albedo;
        public Color[] emission;
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
            emission = emissionRequest.GetData<Color>().ToArray();


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
            emission = new Color[group.Width * group.Height];
        }

        public void ClearPixels()
        {
            albedo = new Color32[0];
            emission = new Color[0];
        }
    }

    public class LightProbeVolumeData
    {
        public int id;
        public int indexStart;
        public Vector3Int resolution;
    }

    public class BakeContext
    {
        public List<Bindings.Light> sceneLights = new();
        public List<Glim.MeshData> sceneMesh = new();
        public List<BakeContextGroup> groups = new();

        public List<Vector4> probePositions = new();

        public List<LightProbeVolumeData> probeVolumes = new();

        public LightingDataAsset storage;
        public Scene scene;

        public bool reflectionProbesSuperSampling;
        public bool reflectionProbesSpecular;
        public bool bakeReflectionProbes;

        public string outputDir;
        public LightmapMode lightmapMode;
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
                    // TODO
                    lightType = Bindings.LightType.Area;
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
                    mixed = light.lightmapBakeType == LightmapBakeType.Mixed ? 1u : 0u
                };

                if (light.type == LightType.Spot)
                {
                    l.spot_outer = light.spotAngle;
                    l.spot_inner_percent = light.innerSpotAngle;
                    l.ty = Bindings.LightType.Spot;
                    l.direction = -light.transform.forward;

                    if (builtIn)
                    {
                        l.spot_inner_percent = 80; // todo this doesnt match built in spot lights exactly
                    }
                }
                if (light.type == LightType.Rectangle)
                {
                    l.direction = -light.transform.forward;
                    l.area_size = light.areaSize;
                }
                if (light.type == LightType.Disc)
                {
                    l.direction = -light.transform.forward;
                    l.area_size = new Vector2(light.areaSize.x, light.areaSize.x);
                }

                addedLights.Add(light);
                sceneLights.Add(l);
            }


            if (!config.is_preview)
            {
                var lightsArray = addedLights.ToArray();
                var lightsProp = lda.FindProperty("m_Lights");
                var lightsOutputsProp = lda.FindProperty("m_LightBakingOutputs");
                Debug.Assert(lightsProp != null);
                Debug.Assert(lightsOutputsProp != null);

                lightsProp.arraySize = lightsArray.Length;
                lightsOutputsProp.arraySize = lightsArray.Length;
                for (int i = 0; i < lightsArray.Length; i++)
                {
                    var light = lightsArray[i];
                    var outputElement = lightsOutputsProp.GetArrayElementAtIndex(i);
                    var ids = lightsProp.GetArrayElementAtIndex(i);

                    outputElement.FindPropertyRelative("probeOcclusionLightIndex").intValue = 0;
                    outputElement.FindPropertyRelative("occlusionMaskChannel").intValue = -1;

                    var mixedMode = lightmapper.mixedMode switch
                    {
                        MixedLightMode.BakedIndirect => MixedLightingMode.IndirectOnly,
                        // MixedLightMode.Subtractive => MixedLightingMode.Subtractive,
                        // MixedLightMode.Shadowmask => MixedLightingMode.Shadowmask,
                        _ => MixedLightingMode.IndirectOnly,
                    };

                    var mode = outputElement.FindPropertyRelative("lightmapBakeMode");
                    mode.FindPropertyRelative("lightmapBakeType").intValue = (int)light.lightmapBakeType;
                    mode.FindPropertyRelative("mixedLightingMode").intValue = (int)mixedMode;

                    outputElement.FindPropertyRelative("isBaked").boolValue = true;

                    var soi = LightingData.ObjectToSOI(light);

                    ids.Next(true);
                    ids.longValue = soi.MainLFID;
                    ids.Next(false);
                    ids.longValue = soi.PrefabLFID;
                }
            }


            var allSelectors = rootObjects
                .SelectMany(x => x.GetComponentsInChildren<LightmapGroupSelector>(false))
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

                var hashData = new uint[4];
                hashData[0] = (uint)lightmapGroup.resolution;
                hashData[1] = lightmapGroup.holeFilling ? 1u : 0u;
                hashData[2] = lightmapGroup.packingIterations;
                hashData[3] = (uint)BitConverter.SingleToInt32Bits(lightmapGroup.scaleExponent);

                var newHash = MeshHash.FromLightmapUV(rendererArray, hashData);

                bool changed = lightmapGroup.lightmapUVHash != newHash;

                if (!lightmapper.enableUVCache)
                {
                    changed = true;
                }

                if (changed)
                {

                    if (lightmapGroup.packingType == UVPackingType.ScaleOffset)
                    {
                        var sw = new Stopwatch();
                        sw.Start();

                        bool bruteForce = lightmapGroup.bruteForce;
                        bool holeFilling = lightmapGroup.holeFilling;
                        float worldScaleExponent = lightmapGroup.scaleExponent;
                        var packer = UVPacking.uvpacker_create(lightmapGroup.Width, lightmapGroup.Height, lightmapGroup.packingIterations, bruteForce, holeFilling, worldScaleExponent);
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

                            var so = UVPacking.uvpacker_get_scale_offset(packer, (uint)rendererIndex);
                            r.lightmapScaleOffset = so;
                            EditorUtility.SetDirty(r);
                        }

                        float coverage = UVPacking.uvpacker_get_coverage(packer);
                        Bake.bakeMessages.AppendLine($"Group {groupIndex} UVs packed in {elapsed}ms with {coverage * 100.0f}% coverage");

                        lightmapGroup.lightmapUVHash = newHash;
                        EditorUtility.SetDirty(lightmapper);

                        UVPacking.uvpacker_destroy(packer);
                    }
                    else
                    {
                        foreach (var r in renderers)
                        {
                            r.lightmapScaleOffset = new Vector4(1, 1, 0, 0);
                        }
                    }
                }
                else
                {
                    Bake.bakeMessages.AppendLine($"Group {groupIndex} using cached lightmap UVs");
                }

                if (!config.is_preview)
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

                        // lmData.FindPropertyRelative("uvMesh");
                        lmData.FindPropertyRelative("terrainDynamicUVST").vector4Value = scaleOffset;
                        lmData.FindPropertyRelative("terrainChunkDynamicUVST").vector4Value = scaleOffset;

                        lmData.FindPropertyRelative("lightmapIndexDynamic").intValue = 65535;

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

                var terrainGroup = ScriptableObject.CreateInstance<GlimLightmapGroup>();
                terrainGroup.packingType = UVPackingType.None;

                groups.Add(new BakeContextGroup(terrainGroup, terrain));

                var meshData = new Glim.MeshData
                {
                    vertices = mesh.vertices,
                    normals = mesh.normals,
                    uvs = mesh.uv,
                    indices = mesh.triangles,
                    groupIndex = groupIndex,
                    backfaceGI = false,
                    transparent = hasHoles ? true : false,
                    emissive = false,
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
                }

                groupIndex++;
            }


            if (groupIndex <= 0)
            {
                throw new InvalidOperationException("No lightmap groups found.");
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

            if (renderer.scaleInLightmap == 0)
            {
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
            public Vector2[] uvs;
            public int[] indices;
            public uint groupIndex;
            public bool backfaceGI;
            public bool transparent;
            public bool emissive;
        }

        public static List<MeshData> ExtractMeshData(Renderer[] renderers, uint groupIndex)
        {
            var datas = new List<MeshData>();

            var vertices = new List<Vector3>();
            var normals = new List<Vector3>();
            var uvs = new List<Vector2>();
            var indices = new List<int>();

            var localVertices = new List<Vector3>();
            var localNormals = new List<Vector3>();
            var localUvs = new List<Vector2>();
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
                    mesh.GetUVs(1, uvs);
                }
                else
                {
                    mesh.GetUVs(0, uvs);
                }

                var enlightenVertexStream = mr.enlightenVertexStream;
                var additionalVertexStreams = mr.additionalVertexStreams;
                if (enlightenVertexStream)
                {
                    if (enlightenVertexStream.HasVertexAttribute(VertexAttribute.TexCoord1))
                    {
                        enlightenVertexStream.GetUVs(1, uvs);
                    }
                }
                else if (additionalVertexStreams)
                {
                    if (additionalVertexStreams.HasVertexAttribute(VertexAttribute.TexCoord1))
                    {
                        additionalVertexStreams.GetUVs(1, uvs);
                    }
                }

                int subMeshCount = mesh.subMeshCount;
                var materials = mr.sharedMaterials;

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
                for (int uvIndex = 0; uvIndex < uvs.Count; uvIndex++)
                {
                    uvs[uvIndex] = uvs[uvIndex] * scale + offset;
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
                    localUvs.Clear();
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
                            localUvs.Add(uvs[globalIndex]);
                        }

                        localIndices.Add(localIndex);
                    }


                    var data = new MeshData
                    {
                        vertices = localVertices.ToArray(),
                        normals = localNormals.ToArray(),
                        uvs = localUvs.ToArray(),
                        indices = localIndices.ToArray(),
                        groupIndex = groupIndex,
                        backfaceGI = backfaceGI,
                        transparent = transparent,
                        emissive = emissive,
                    };

                    datas.Add(data);

                }
            }

            return datas;
        }
    }
}
