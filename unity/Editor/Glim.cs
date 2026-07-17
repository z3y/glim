using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using UnityEditor;
using UnityEditor.SceneManagement;
using UnityEngine;
using UnityEngine.Rendering;
using UnityEngine.SceneManagement;

namespace glim
{
    public class BakeContextGroup
    {
        public Color32[] albedo;
        public Color[] emission;
        public Bindings.LightmapSettings settings;
        public LightmapGroup groupAsset;

        public BakeContextGroup(LightmapGroup group, IList<Renderer> renderers)
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

        private static int GetDepth(Transform t)
        {
            int depth = 0;
            while (t.parent != null) { t = t.parent; depth++; }
            return depth;
        }

        public BakeContext(LightmapBaker baker, Bindings.GlimConfig config)
        {
            this.reflectionProbesSuperSampling = baker.reflectionProbesSuperSampling;
            this.reflectionProbesSpecular = baker.reflectionProbesSpecular;

            SerializedObject lda;
            if (!config.is_preview)
            {
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

            var ftraceLightmaps = rootObjects.FirstOrDefault(x => x.gameObject.name == "!ftraceLightmaps");
            if (ftraceLightmaps != null)
            {
                // bakery is breaking directional lightmaps, need to remove this from scene
                // bakery always creates this object just by having the "Render Lightmap" window open
                // so make sure to close it and reopen the scene
                Debug.Log("Removing Bakery !ftraceLightmap GameObject");
                GameObject.DestroyImmediate(ftraceLightmaps.gameObject);
                EditorSceneManager.MarkSceneDirty(scene);
                rootObjects = scene.GetRootGameObjects().Where(x => x.activeInHierarchy);
            }

            var lights = rootObjects.SelectMany(x => x.GetComponentsInChildren<Light>(false)).ToArray();
            var builtIn = GraphicsSettings.currentRenderPipeline == null;

            var addedLights = new List<Light>();
            foreach (var light in lights)
            {
                // todo mixed
                if (light.lightmapBakeType != LightmapBakeType.Baked)
                {
                    continue;
                }

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
                    var outputElement = lightsOutputsProp.GetArrayElementAtIndex(i);
                    var ids = lightsProp.GetArrayElementAtIndex(i);

                    outputElement.FindPropertyRelative("probeOcclusionLightIndex").intValue = 0;
                    outputElement.FindPropertyRelative("occlusionMaskChannel").intValue = -1;

                    var mode = outputElement.FindPropertyRelative("lightmapBakeMode");
                    mode.FindPropertyRelative("lightmapBakeType").intValue = (int)LightmapBakeType.Baked;
                    mode.FindPropertyRelative("mixedLightingMode").intValue = (int)MixedLightingMode.Shadowmask;

                    outputElement.FindPropertyRelative("isBaked").boolValue = true;

                    var soi = LightingData.ObjectToSOI(lightsArray[i]);

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

            var groupMap = new Dictionary<LightmapGroup, List<MeshRenderer>>();
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

            // todo move to ui
            var allRenderers = rootObjects.SelectMany(x => x.GetComponentsInChildren<MeshRenderer>(false));
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
            var globalGroup = baker.group == null ? ScriptableObject.CreateInstance<LightmapGroup>() : baker.group;
            if (unclaimedRenderers.Count > 0)
            {
                groupMap[globalGroup] = unclaimedRenderers;
            }

            uint groupIndex = 0;
            int mrDataOffset = 0;
            foreach (var (lightmapGroup, renderers) in groupMap)
            {
                var rendererArray = renderers.ToArray();

                if (lightmapGroup.packingType == UVPackingType.ScaleOffset)
                {
                    bool bruteForce = lightmapGroup.bruteForce;
                    var packer = UVPacking.uvpacker_create(lightmapGroup.Width, lightmapGroup.Height, lightmapGroup.packingIterations, bruteForce);
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
                        throw new Exception("UV Packing failed, try increasing resolution or packing iteration count");
                    }

                    for (int rendererIndex = 0; rendererIndex < renderers.Count; rendererIndex++)
                    {
                        Renderer r = renderers[rendererIndex];

                        var so = UVPacking.uvpacker_get_scale_offset(packer, (uint)rendererIndex);
                        r.lightmapScaleOffset = so;
                        EditorUtility.SetDirty(r);
                    }

                    UVPacking.uvpacker_destroy(packer);
                }
                else
                {
                    foreach (var r in renderers)
                    {
                        r.lightmapScaleOffset = new Vector4(1, 1, 0, 0);
                    }
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
                        var ids = rendererDataIds.GetArrayElementAtIndex(mrDataOffset + i);
                        var lmData = rendererData.GetArrayElementAtIndex(mrDataOffset + i);

                        var soi = LightingData.ObjectToSOI(mr);

                        ids.Next(true);
                        ids.longValue = soi.MainLFID;
                        ids.Next(false);
                        ids.longValue = soi.PrefabLFID;

                        lmData.FindPropertyRelative("lightmapIndex").intValue = (int)groupIndex;
                        var scaleOffset = mr.lightmapScaleOffset;
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

            if (groupIndex <= 0)
            {
                throw new InvalidOperationException("No lightmap groups found.");
            }

            if (!baker.group)
            {
                ScriptableObject.DestroyImmediate(globalGroup);
            }

            float defaultProbeRadius = baker.lightProbeRadius;

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

            // var uv = mesh.HasVertexAttribute(VertexAttribute.TexCoord1) ? mesh.uv2 : mesh.uv;

            // if (uv.Length != vertices)
            // {
            //     return false;
            // }


            return true;
        }

        public class MeshData
        {
            public Vector3[] vertices;
            public Vector3[] normals;
            public Vector2[] uvs;
            public int[] triangles;
            public uint groupIndex;
            public bool backfaceGI;
            public bool transparent;
        }

        public static List<MeshData> ExtractMeshData(Renderer[] renderers, uint groupIndex)
        {
            var datas = new List<MeshData>();

            for (int i = 0; i < renderers.Length; i++)
            {
                var filter = renderers[i].GetComponent<MeshFilter>();
                if (!filter)
                {
                    continue;
                }

                var transform = filter.transform;
                var mesh = filter.sharedMesh;

                var vertices = mesh.vertices;
                var normals = mesh.normals;
                var triangles = mesh.triangles;

                var uvs = mesh.HasVertexAttribute(VertexAttribute.TexCoord1) ? mesh.uv2 : mesh.uv;
                bool backfaceGI = false;
                bool transparent = false;
                if (renderers[i] is MeshRenderer mr)
                {
                    var evs = mr.enlightenVertexStream;
                    var avs = mr.additionalVertexStreams;

                    if (evs && evs.HasVertexAttribute(VertexAttribute.TexCoord1))
                    {
                        uvs = evs.uv2;
                    }
                    else if (avs && avs.HasVertexAttribute(VertexAttribute.TexCoord1))
                    {
                        uvs = avs.uv2;
                    }

                    var mats = mr.sharedMaterials;
                    // todo backfacegi and transparent per submesh instead of entire mesh
                    foreach (var mat in mats)
                    {
                        if (mat == null)
                        {
                            continue;
                        }

                        if (mat.doubleSidedGI)
                        {
                            backfaceGI = true;
                        }

                        if (MetaTexture.IsMaterialTransparent(mat))
                        {
                            transparent = true;
                        }
                    }
                }


                transform.TransformPoints(vertices);

                // todo move to rust
                Matrix4x4 normalMatrix = transform.localToWorldMatrix.inverse.transpose;

                for (int j = 0; j < normals.Length; j++)
                {
                    normals[j] = normalMatrix.MultiplyVector(normals[j]).normalized;
                }

                bool isNegativeScale = transform.localToWorldMatrix.determinant < 0.0f;
                if (isNegativeScale)
                {

                    for (int j = 0; j < triangles.Length; j += 3)
                    {
                        (triangles[j + 1], triangles[j]) = (triangles[j], triangles[j + 1]);
                    }
                }


                Vector4 scaleOffset = renderers[i].lightmapScaleOffset;
                Vector2 scale = new(scaleOffset.x, scaleOffset.y);
                Vector2 offset = new(scaleOffset.z, scaleOffset.w);
                for (int j = 0; j < uvs.Length; j++)
                {
                    uvs[j] = uvs[j] * scale + offset;
                }

                var data = new MeshData
                {
                    vertices = vertices,
                    normals = normals,
                    uvs = uvs,
                    triangles = triangles,
                    groupIndex = groupIndex,
                    backfaceGI = backfaceGI,
                    transparent = transparent
                };

                datas.Add(data);
            }

            return datas;
        }
    }
}