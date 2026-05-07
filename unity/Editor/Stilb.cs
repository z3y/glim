using System;
using System.Collections.Generic;
using System.Linq;
using Unity.Collections;
using UnityEditor;
using UnityEngine;
using UnityEngine.Rendering;
using UnityEngine.SceneManagement;

namespace stilb
{
    public class BakeContextGroup
    {
        public Color32[] albedo;
        public Color[] emission;
        public Bindings.LightmapSettings settings;

        public BakeContextGroup(LightmapGroup group, IList<Renderer> renderers)
        {
            settings = new Bindings.LightmapSettings
            {
                width = group.resolution,
                height = group.resolution,
                max_samples = group.maxSamples,
                bounce_count = group.bounceCount,
                denoise = group.denoise,
            };

            using var metaAlbedo = new MetaTexture((int)settings.width, MetaTexture.AtlasType.Albedo);
            using var metaEmission = new MetaTexture((int)settings.width, MetaTexture.AtlasType.Emission);

            albedo = metaAlbedo
                .CreateAtlas(renderers, MetaTexture.AtlasType.Albedo)
                .GetData<Color32>().ToArray();

            emission = metaEmission
                .CreateAtlas(renderers, MetaTexture.AtlasType.Emission)
                .GetData<Color>().ToArray();


            // var albedoAtlas = new Texture2D((int)settings.width, (int)settings.height, TextureFormat.ARGB32, 1, true);
            // albedoAtlas.SetPixels32(albedo);
            // AssetDatabase.CreateAsset(albedoAtlas, "Assets/AbledoAtlas.asset");
            // var emissionAtlas = new Texture2D((int)settings.width, (int)settings.height, TextureFormat.RGBAFloat, 1, true);
            // emissionAtlas.SetPixels(emission);
            // AssetDatabase.CreateAsset(emissionAtlas, "Assets/EmissionAtlas.asset");

            Debug.Log($"Group width: {settings.width}, height:{settings.height}");
        }
    }

    public class BakeContext
    {
        public List<Bindings.Light> sceneLights = new();
        public List<Stilb.MeshData> sceneMesh = new();
        public List<BakeContextGroup> groups = new();

        private static int GetDepth(Transform t)
        {
            int depth = 0;
            while (t.parent != null) { t = t.parent; depth++; }
            return depth;
        }

        public BakeContext()
        {
            var scene = SceneManager.GetActiveScene();

            var rootObjects = scene.GetRootGameObjects().Where(x => x.activeInHierarchy);

            var lights = rootObjects.SelectMany(x => x.GetComponentsInChildren<Light>(false)).ToArray();
            foreach (var light in lights)
            {
                // todo mixed
                if (light.lightmapBakeType != LightmapBakeType.Baked)
                {
                    continue;
                }

                light.bakingOutput = new LightBakingOutput
                {
                    isBaked = true,
                    lightmapBakeType = LightmapBakeType.Baked,
                    mixedLightingMode = MixedLightingMode.IndirectOnly
                };
                EditorUtility.SetDirty(light);

                // todo color temperature
                var linear = light.color.linear;
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

                float radiusOrAngle = lightType == Bindings.LightType.Directional ?
                    Mathf.Deg2Rad * light.shadowAngle : light.shadowRadius;

                var l = new Bindings.Light
                {
                    ty = lightType,
                    position = light.transform.position,
                    direction = light.transform.forward,
                    range = light.range,
                    color = color,
                    shadow_radius_or_angle = radiusOrAngle,
                };


                sceneLights.Add(l);
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
                    .Where(x => Stilb.IsLightmapStatic(x));

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

                if (Stilb.IsLightmapStatic(r))
                {
                    unclaimedRenderers.Add(r);
                }
            }
            var globalGroup = ScriptableObject.CreateInstance<LightmapGroup>();
            if (unclaimedRenderers.Count > 0)
            {
                groupMap[globalGroup] = unclaimedRenderers;
            }

            uint groupIndex = 0;
            foreach (var (lightmapGroup, renderers) in groupMap)
            {
                var rendererArray = renderers.ToArray();

                foreach (var mr in rendererArray)
                {
                    mr.lightmapIndex = (int)groupIndex;
                    // todo scale and offset packing
                    mr.lightmapScaleOffset = new Vector4(1, 1, 0, 0);
                    EditorUtility.SetDirty(mr);
                }

                this.groups.Add(new BakeContextGroup(lightmapGroup, rendererArray));
                sceneMesh.AddRange(Stilb.ExtractMeshData(rendererArray, groupIndex));
                groupIndex++;
            }

            if (groupIndex <= 0)
            {
                throw new InvalidOperationException("No lightmap groups found.");
            }

            ScriptableObject.DestroyImmediate(globalGroup);

            Debug.Log($"Vertices: {sceneMesh.Sum(x => x.vertices.Length)}");
            Debug.Log($"Indices: {sceneMesh.Sum(x => x.triangles.Length)}");
            Debug.Log($"Lights: {sceneLights.Count}");
        }
    }

    public class Stilb
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

                // maybe find a way to use the actual mesh
                // layout without copy using Mesh.AcquireReadOnlyMeshData()

                var vertices = mesh.vertices;
                var normals = mesh.normals;
                var triangles = mesh.triangles;
                var uvs = mesh.HasVertexAttribute(VertexAttribute.TexCoord1) ? mesh.uv2 : mesh.uv;


                transform.TransformPoints(vertices);
                transform.TransformDirections(normals);

                // todo move to rust
                // Vector4 scaleOffset = renderers[i].lightmapScaleOffset;
                // Vector2 scale = new(scaleOffset.x, scaleOffset.y);
                // Vector2 offset = new(scaleOffset.z, scaleOffset.w);
                // for (int j = 0; j < uvs.Length; j++)
                // {
                //     uvs[j] = uvs[j] + scale + offset;
                // }

                var data = new MeshData
                {
                    vertices = vertices,
                    normals = normals,
                    uvs = uvs,
                    triangles = triangles,
                    groupIndex = groupIndex,
                };

                datas.Add(data);
            }

            return datas;
        }
    }
}

// namespace stilb
// {

//     public static class Stilb
//     {
//         [MenuItem("Stilb/Bake Scene")]
//         public static void BakeActiveScene()
//         {
//             var scene = SceneManager.GetActiveScene();
//             BakeScene(scene);
//         }

//         public static void BakeScene(Scene scene)
//         {
//             var rootObjects = scene.GetRootGameObjects();

//             var allRenderers = rootObjects.SelectMany(x => x.GetComponentsInChildren<MeshRenderer>(false));

//             var staticRenderers = allRenderers.Where(x => IsLightmapStatic(x)).ToArray();

//             Directory.CreateDirectory(wd);
//             ExportSceneMeshes(wd, staticRenderers);

//             var settings = new StilbConfig
//             {
//                 version = 1,
//                 height = 1024,
//                 width = 1024,
//                 bounceCount = 3,
//                 denoise = true,
//                 useCamera = false,
//                 maxSamples = 256,
//                 disableHardwareRt = false
//             };

//             ExportSettings(settings, wd);

//             RenderMetaTextures(wd, staticRenderers, (int)settings.width);

//             LaunchStilb(scene, staticRenderers, settings);
//         }

//         static void ExportSettings(StilbConfig settings, string path)
//         {
//             path = Path.Combine(path, "settings.bin");
//             using (BinaryWriter writer = new(File.Open(path, FileMode.Create)))
//             {
//                 writer.Write(settings.version);
//                 writer.Write(settings.width);
//                 writer.Write(settings.height);
//                 writer.Write(settings.maxSamples);
//                 writer.Write(settings.bounceCount);
//                 writer.Write(settings.denoise);
//                 writer.Write(settings.useCamera);
//                 writer.Write(settings.disableHardwareRt);
//             }
//         }

//         static void ApplyLightmap(Renderer[] renderers, string name)
//         {
//             string lightmapFolder = "Assets/StilbLightmaps";
//             string lightmapFolderFull = Path.GetFullPath(lightmapFolder);

//             if (!Directory.Exists(lightmapFolderFull))
//             {
//                 Directory.CreateDirectory(lightmapFolderFull);
//             }

//             string tempFolder = Path.Combine(Application.dataPath, "../Temp/StilbExport");

//             string lightmapFileName = name + ".tga";

//             string assetDestination = Path.Combine(lightmapFolder, lightmapFileName);

//             string destinationFull = Path.GetFullPath(assetDestination);

//             if (File.Exists(destinationFull))
//             {
//                 File.Delete(destinationFull);
//             }
//             File.Move(Path.Combine(tempFolder, lightmapFileName), destinationFull);

//             AssetDatabase.ImportAsset(assetDestination);

//             var lightmapTexture = AssetDatabase.LoadAssetAtPath<Texture2D>(assetDestination);

//             ApplyLightmapSettings(lightmapTexture, null, renderers);
//         }

//         static void RenderMetaTextures(string path, MeshRenderer[] renderers, int resolution)
//         {
//             using var meta = new MetaTexture(resolution);

//             meta.CreateAtlas(renderers, MetaTexture.AtlasType.Albedo, path);
//             meta.CreateAtlas(renderers, MetaTexture.AtlasType.Emission, path);
//         }

//         static void LaunchStilb(Scene scene, Renderer[] renderers, StilbConfig settings)
//         {
//             string stilbPath = "/run/media/z3y/SSD/Dev/stilb/build/stilb";

//             ProcessStartInfo startInfo = new ProcessStartInfo
//             {
//                 FileName = stilbPath,
//                 Arguments = $"",
//                 UseShellExecute = false,
//                 RedirectStandardOutput = true,
//                 RedirectStandardError = true,
//                 CreateNoWindow = false,
//                 WorkingDirectory = wd
//             };

//             var process = new Process()
//             {
//                 StartInfo = startInfo,
//                 EnableRaisingEvents = true,
//             };
//             //process.StartInfo.WindowStyle = ProcessWindowStyle.Hidden;

//             // bool baked = false;

//             // int progressId = Progress.Start("Baking", null);
//             // var sw = new Stopwatch();

//             // Progress.RegisterCancelCallback(progressId, () =>
//             // {
//             //     process.Close();
//             //     sw.Stop();
//             //     return true;
//             // });

//             // Progress.RegisterCancelCallback(progressId, () =>
//             // {
//             //     process.Close();
//             // });

//             process.OutputDataReceived += (sender, e) =>
//             {
//                 Debug.Log(e.Data);
//             };

//             process.ErrorDataReceived += (sender, e) =>
//             {
//                 Debug.LogError(e.Data);
//             };

//             process.Exited += (sender, e) =>
//             {
//                 EditorApplication.delayCall += () =>
//                 {
//                     if (!settings.useCamera)
//                     {
//                         ApplyLightmap(renderers, "output");
//                     }
//                 };
//             };

//             process.Start();
//             process.BeginOutputReadLine();
//             process.BeginErrorReadLine();
//         }

//         static void ApplyLightmapSettings(Texture2D lightmapTexture, Texture2D lightmapDirTexture, Renderer[] renderers)
//         {
//             LightmapData lightmapData = new()
//             {
//                 lightmapColor = lightmapTexture,
//                 lightmapDir = lightmapDirTexture
//             };

//             LightmapSettings.lightmaps = new LightmapData[] { lightmapData };
//             LightmapSettings.lightmapsMode = lightmapDirTexture ? LightmapsMode.CombinedDirectional : LightmapsMode.NonDirectional;

//             foreach (var renderer in renderers)
//             {
//                 renderer.lightmapIndex = 0;
//                 renderer.lightmapScaleOffset = new Vector4(1, 1, 0, 0);
//             }

//         }

//         static void ExportSceneMeshes(string path, MeshRenderer[] renderers)
//         {
//             var meshFilters = renderers.Select(x => x.GetComponent<MeshFilter>()).ToArray();
//             var meshes = meshFilters.Select(x => x.sharedMesh).ToArray();

//             int totalVertexCount = meshes.Sum(x => x.vertexCount);
//             int totalIndexCount = meshes.Sum(x => x.triangles.Length);

//             var allVertices = new List<Vector3>(totalVertexCount);
//             var allNormals = new List<Vector3>(totalVertexCount);
//             var allUvs = new List<Vector2>(totalVertexCount);
//             var allTriangles = new List<int>(totalIndexCount);

//             // TODO: speed up
//             foreach (var filter in meshFilters)
//             {
//                 int vertexOffset = allVertices.Count;
//                 var transform = filter.transform;
//                 var mesh = filter.sharedMesh;

//                 var vertices = mesh.vertices;
//                 var normals = mesh.normals;
//                 var triangles = mesh.triangles;
//                 var uvs = mesh.HasVertexAttribute(VertexAttribute.TexCoord1) ? mesh.uv2 : mesh.uv;

//                 Matrix4x4 matrix = filter.transform.localToWorldMatrix;

//                 // 1. Position with Z Flip
//                 Vector3[] meshVertices = mesh.vertices;
//                 for (int i = 0; i < meshVertices.Length; i++)
//                 {
//                     Vector3 worldPt = matrix.MultiplyPoint3x4(meshVertices[i]);
//                     // FLIP Z HERE
//                     allVertices.Add(new Vector3(worldPt.x, worldPt.y, -worldPt.z));
//                 }

//                 // 2. Normals with Z Flip
//                 Vector3[] meshNormals = mesh.normals;
//                 Matrix4x4 normalMatrix = matrix.inverse.transpose;
//                 for (int i = 0; i < meshNormals.Length; i++)
//                 {
//                     Vector3 worldNormal = normalMatrix.MultiplyVector(meshNormals[i]).normalized;
//                     // FLIP Z HERE
//                     allNormals.Add(new Vector3(worldNormal.x, worldNormal.y, -worldNormal.z));
//                 }

//                 // 3. UVs (No changes needed)
//                 allUvs.AddRange(mesh.uv);

//                 // 4. SWAP WINDING ORDER (Otherwise it's inside out)
//                 int[] meshTriangles = mesh.triangles;
//                 for (int i = 0; i < meshTriangles.Length; i += 3)
//                 {
//                     // Swap index 1 and 2 to reverse the face direction
//                     allTriangles.Add(meshTriangles[i + 0] + vertexOffset);
//                     allTriangles.Add(meshTriangles[i + 2] + vertexOffset);
//                     allTriangles.Add(meshTriangles[i + 1] + vertexOffset);
//                 }

//                 // transform.TransformPoints(vertices);
//                 // allVertices.AddRange(vertices);

//                 // transform.TransformDirections(normals);
//                 // allNormals.AddRange(normals);

//                 // Vector3[] meshVertices = mesh.vertices;
//                 // for (int i = 0; i < meshVertices.Length; i++)
//                 // {
//                 //     allVertices.Add(transform.TransformPoint(meshVertices[i]));
//                 // }

//                 // Vector3[] meshNormals = mesh.normals;
//                 // for (int i = 0; i < meshNormals.Length; i++)
//                 // {
//                 //     allNormals.Add(transform.TransformDirection(meshNormals[i]));
//                 // }

//                 // allUvs.AddRange(uvs);

//                 // foreach (var tri in triangles)
//                 // {
//                 //     allTriangles.Add(tri + vertexOffset);
//                 // }
//             }

//             Debug.Log($"Exporting Renderers: {renderers.Length} Vertices: {allVertices.Count}, Triangles: {allTriangles.Count}");


//             {
//                 using var bin = File.Open(Path.Combine(path, "vertices.bin"), FileMode.Create);
//                 using var w = new BinaryWriter(bin);
//                 w.Write(allVertices.Count);
//                 foreach (var vertex in allVertices)
//                 {
//                     w.Write(vertex.x);
//                     w.Write(vertex.y);
//                     w.Write(vertex.z);
//                 }
//             }
//             {
//                 using var bin = File.Open(Path.Combine(path, "normals.bin"), FileMode.Create);
//                 using var w = new BinaryWriter(bin);
//                 w.Write(allNormals.Count);
//                 foreach (var normal in allNormals)
//                 {
//                     w.Write(normal.x);
//                     w.Write(normal.y);
//                     w.Write(normal.z);
//                 }
//             }
//             {
//                 using var bin = File.Open(Path.Combine(path, "uvs.bin"), FileMode.Create);
//                 using var w = new BinaryWriter(bin);
//                 w.Write(allUvs.Count);
//                 foreach (var uv in allUvs)
//                 {
//                     w.Write(uv.x);
//                     w.Write(uv.y);
//                 }
//             }
//             {
//                 using var bin = File.Open(Path.Combine(path, "triangles.bin"), FileMode.Create);
//                 using var w = new BinaryWriter(bin);
//                 w.Write(allTriangles.Count);
//                 foreach (var tri in allTriangles)
//                 {
//                     w.Write(tri);
//                 }
//             }

//         }

//         public static bool IsLightmapStatic(MeshRenderer renderer)
//         {
//             var gameObject = renderer.gameObject;
//             if (gameObject.activeInHierarchy == false)
//             {
//                 return false;
//             }

//             if (!GameObjectUtility.GetStaticEditorFlags(gameObject).HasFlag(StaticEditorFlags.ContributeGI))
//             {
//                 return false;
//             }

//             var filter = renderer.GetComponent<MeshFilter>();

//             if (!filter)
//             {
//                 return false;
//             }

//             if (renderer.receiveGI != ReceiveGI.Lightmaps)
//             {
//                 return false;
//             }

//             if (renderer.scaleInLightmap == 0)
//             {
//                 return false;
//             }

//             if (filter.sharedMesh == null)
//             {
//                 return false;
//             }

//             var mesh = filter.sharedMesh;

//             var vertices = mesh.vertices;

//             if (vertices == null)
//             {
//                 return false;
//             }

//             if (!mesh.HasVertexAttribute(VertexAttribute.TexCoord0))
//             {
//                 return false;
//             }

//             var uv = mesh.HasVertexAttribute(VertexAttribute.TexCoord1) ? mesh.uv2 : mesh.uv;

//             if (uv.Length != vertices.Length)
//             {
//                 return false;
//             }


//             return true;
//         }
//     }

// }
