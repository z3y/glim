#if GLIM_BURST
using System;
using System.Collections.Generic;
using Unity.Burst;
using Unity.Collections;
using Unity.Collections.LowLevel.Unsafe;
using Unity.Jobs;
using Unity.Mathematics;
using UnityEditor;
using UnityEngine;
using UnityEngine.Rendering;

namespace Glim
{
    // Conservative rasterization without touching the material's shader.
    //
    // Every triangle is replaced by its exact conservative footprint in lightmap texel space: the triangle
    // grown by a half-texel square (Minkowski sum), i.e. the convex hull of each vertex ± (0.5, 0.5).
    // Rasterizing that polygon with regular point sampling hits every texel whose square overlaps the
    // triangle, which is what hardware conservative rasterization does, and nothing more (at most 0.5 texel
    // in x/y). All vertex attributes are linearly extrapolated with the hull vertices' barycentrics, so the
    // unmodified META shader sees the surface continuing past the original edges.
    //
    // Expanded meshes depend on the source mesh, the renderer's lightmap scale and the atlas resolution.
    // One cache is shared by the albedo and emission atlases of a group so each mesh is only built once.
    public class ConservativeMeshCache : IDisposable
    {
        // A triangle ⊕ square footprint has at most 3 + 4 = 7 sides; one spare slot for float noise.
        const int MaxHullVertices = 8;
        const int MaxHullIndices = (MaxHullVertices - 2) * 3;
        const int MaxUVChannels = 8;

        readonly int _resolution;
        readonly Dictionary<(Mesh, Vector2), Mesh> _meshes = new();

        public ConservativeMeshCache(int resolution)
        {
            _resolution = resolution;
        }

        static Vector2 ScaleKey(Vector4 lightmapST)
        {
            return new Vector2(Mathf.Abs(lightmapST.x), Mathf.Abs(lightmapST.y));
        }

        public Mesh Get(Mesh src, Vector4 lightmapST)
        {
            var key = (src, ScaleKey(lightmapST));
            if (!_meshes.TryGetValue(key, out var mesh))
            {
                Build(new List<(Mesh, Vector4)> { (src, lightmapST) });
                _meshes.TryGetValue(key, out mesh);
            }
            return mesh; // null for meshes that can't be expanded
        }

        // Builds all missing expanded meshes at once: source data is read once per mesh, then one
        // parallel job per (mesh, scale) runs over its triangles, and everything completes together.
        public void Build(IReadOnlyList<(Mesh mesh, Vector4 lightmapST)> requests)
        {
            var keys = new List<(Mesh, Vector2)>();
            var seen = new HashSet<(Mesh, Vector2)>();
            foreach (var (mesh, st) in requests)
            {
                if (!mesh) continue;
                var key = (mesh, ScaleKey(st));
                if (_meshes.ContainsKey(key) || !seen.Add(key)) continue;
                keys.Add(key);
            }

            if (keys.Count == 0)
            {
                return;
            }

            var sources = new Dictionary<Mesh, SourceData>();
            var outputs = new OutputData[keys.Count];
            var handles = new NativeArray<JobHandle>(keys.Count, Allocator.Temp);

            try
            {
                for (int i = 0; i < keys.Count; i++)
                {
                    var (mesh, scale) = keys[i];

                    if (!sources.TryGetValue(mesh, out var source))
                    {
                        source = SourceData.Read(mesh);
                        sources[mesh] = source;
                    }

                    if (!source.valid || source.triangleCount == 0)
                    {
                        continue;
                    }

                    var output = OutputData.Allocate(source);
                    outputs[i] = output;

                    var job = new ExpandJob
                    {
                        indices = source.indices,
                        lightmapUV = source.lightmapUV,
                        positions = source.positions,
                        normals = source.normals,
                        tangents = source.tangents,
                        colors = source.colors,
                        uvs = source.uvs,
                        vertexCount = source.vertexCount,
                        uvSlotCount = source.uvChannels.Length,
                        toTexel = new float2(scale.x, scale.y) * _resolution,

                        outPositions = output.positions,
                        outNormals = output.normals,
                        outTangents = output.tangents,
                        outColors = output.colors,
                        outUvs = output.uvs,
                        outIndices = output.indices,
                        outVertexCount = output.vertexCount,
                    };

                    handles[i] = job.Schedule(source.triangleCount, 64);
                }

                JobHandle.ScheduleBatchedJobs();
                JobHandle.CompleteAll(handles);

                for (int i = 0; i < keys.Count; i++)
                {
                    var (mesh, _) = keys[i];
                    var source = sources[mesh];
                    _meshes[keys[i]] = outputs[i].allocated ? CreateMesh(mesh, source, outputs[i]) : null;
                }
            }
            finally
            {
                // make sure nothing is still reading before freeing, even on an exception
                JobHandle.CompleteAll(handles);
                handles.Dispose();
                foreach (var output in outputs) output.Dispose();
                foreach (var source in sources.Values) source.Dispose();
            }
        }

        static Mesh CreateMesh(Mesh src, in SourceData source, in OutputData output)
        {
            var dst = new Mesh
            {
                name = src.name + " (Conservative)",
                hideFlags = HideFlags.HideAndDontSave,
                indexFormat = IndexFormat.UInt32,
            };

            int vc = output.vertexCount;
            dst.SetVertices(output.positions.Reinterpret<Vector3>());
            if (output.normals.Length > 0) dst.SetNormals(output.normals.Reinterpret<Vector3>());
            if (output.tangents.Length > 0) dst.SetTangents(output.tangents.Reinterpret<Vector4>());
            if (output.colors.Length > 0) dst.SetColors(output.colors.Reinterpret<Color>());
            for (int slot = 0; slot < source.uvChannels.Length; slot++)
            {
                dst.SetUVs(source.uvChannels[slot], output.uvs.GetSubArray(slot * vc, vc).Reinterpret<Vector4>());
            }

            dst.subMeshCount = source.submeshTriangleStart.Length;
            for (int s = 0; s < source.submeshTriangleStart.Length; s++)
            {
                dst.SetIndices(output.indices,
                    source.submeshTriangleStart[s] * MaxHullIndices,
                    source.submeshTriangleCount[s] * MaxHullIndices,
                    MeshTopology.Triangles, s, false);
            }

            dst.bounds = src.bounds;
            dst.UploadMeshData(true);
            return dst;
        }

        public void Dispose()
        {
            foreach (var mesh in _meshes.Values)
            {
                if (mesh)
                {
                    Editor.DestroyImmediate(mesh);
                }
            }
            _meshes.Clear();
        }

        struct SourceData : IDisposable
        {
            public bool valid;
            public int vertexCount;
            public int triangleCount;

            // triangle indices of all triangle-topology submeshes, concatenated
            public NativeArray<int> indices;
            public int[] submeshTriangleStart;
            public int[] submeshTriangleCount;

            public NativeArray<float2> lightmapUV;
            public NativeArray<float3> positions;
            public NativeArray<float3> normals;  // empty if missing
            public NativeArray<float4> tangents; // empty if missing
            public NativeArray<float4> colors;   // empty if missing
            public NativeArray<float4> uvs;      // [slot * vertexCount + v], one slot per present channel
            public int[] uvChannels;             // slot -> uv channel

            public static SourceData Read(Mesh mesh)
            {
                var result = new SourceData();

                using var dataArray = Mesh.AcquireReadOnlyMeshData(mesh);
                var data = dataArray[0];

                int lightmapChannel = data.HasVertexAttribute(VertexAttribute.TexCoord1) ? 1
                    : data.HasVertexAttribute(VertexAttribute.TexCoord0) ? 0 : -1;
                if (lightmapChannel < 0)
                {
                    return result;
                }

                int vc = data.vertexCount;
                result.vertexCount = vc;

                result.lightmapUV = new NativeArray<float2>(vc, Allocator.TempJob, NativeArrayOptions.UninitializedMemory);
                data.GetUVs(lightmapChannel, result.lightmapUV.Reinterpret<Vector2>());

                result.positions = new NativeArray<float3>(vc, Allocator.TempJob, NativeArrayOptions.UninitializedMemory);
                data.GetVertices(result.positions.Reinterpret<Vector3>());

                bool hasNormals = data.HasVertexAttribute(VertexAttribute.Normal);
                result.normals = new NativeArray<float3>(hasNormals ? vc : 0, Allocator.TempJob, NativeArrayOptions.UninitializedMemory);
                if (hasNormals) data.GetNormals(result.normals.Reinterpret<Vector3>());

                bool hasTangents = data.HasVertexAttribute(VertexAttribute.Tangent);
                result.tangents = new NativeArray<float4>(hasTangents ? vc : 0, Allocator.TempJob, NativeArrayOptions.UninitializedMemory);
                if (hasTangents) data.GetTangents(result.tangents.Reinterpret<Vector4>());

                bool hasColors = data.HasVertexAttribute(VertexAttribute.Color);
                result.colors = new NativeArray<float4>(hasColors ? vc : 0, Allocator.TempJob, NativeArrayOptions.UninitializedMemory);
                if (hasColors) data.GetColors(result.colors.Reinterpret<Color>());

                var channels = new List<int>(MaxUVChannels);
                for (int c = 0; c < MaxUVChannels; c++)
                {
                    if (data.HasVertexAttribute(VertexAttribute.TexCoord0 + c)) channels.Add(c);
                }
                result.uvChannels = channels.ToArray();
                result.uvs = new NativeArray<float4>(vc * channels.Count, Allocator.TempJob, NativeArrayOptions.UninitializedMemory);
                for (int slot = 0; slot < channels.Count; slot++)
                {
                    data.GetUVs(channels[slot], result.uvs.GetSubArray(slot * vc, vc).Reinterpret<Vector4>());
                }

                int submeshCount = data.subMeshCount;
                result.submeshTriangleStart = new int[submeshCount];
                result.submeshTriangleCount = new int[submeshCount];

                int totalIndices = 0;
                for (int s = 0; s < submeshCount; s++)
                {
                    var sm = data.GetSubMesh(s);
                    if (sm.topology == MeshTopology.Triangles)
                    {
                        totalIndices += sm.indexCount - sm.indexCount % 3;
                    }
                }

                result.indices = new NativeArray<int>(totalIndices, Allocator.TempJob, NativeArrayOptions.UninitializedMemory);

                int offset = 0;
                for (int s = 0; s < submeshCount; s++)
                {
                    var sm = data.GetSubMesh(s);
                    result.submeshTriangleStart[s] = offset / 3;

                    if (sm.topology != MeshTopology.Triangles)
                    {
                        continue;
                    }

                    int count = sm.indexCount - sm.indexCount % 3;
                    if (count != sm.indexCount)
                    {
                        // GetIndices writes the whole submesh; read into a temp and copy the full triangles
                        using var temp = new NativeArray<int>(sm.indexCount, Allocator.Temp, NativeArrayOptions.UninitializedMemory);
                        data.GetIndices(temp, s);
                        NativeArray<int>.Copy(temp, 0, result.indices, offset, count);
                    }
                    else
                    {
                        data.GetIndices(result.indices.GetSubArray(offset, count), s);
                    }

                    result.submeshTriangleCount[s] = count / 3;
                    offset += count;
                }

                result.triangleCount = totalIndices / 3;
                result.valid = true;
                return result;
            }

            public void Dispose()
            {
                if (indices.IsCreated) indices.Dispose();
                if (lightmapUV.IsCreated) lightmapUV.Dispose();
                if (positions.IsCreated) positions.Dispose();
                if (normals.IsCreated) normals.Dispose();
                if (tangents.IsCreated) tangents.Dispose();
                if (colors.IsCreated) colors.Dispose();
                if (uvs.IsCreated) uvs.Dispose();
            }
        }

        struct OutputData : IDisposable
        {
            public bool allocated;
            public int vertexCount;
            public NativeArray<float3> positions;
            public NativeArray<float3> normals;
            public NativeArray<float4> tangents;
            public NativeArray<float4> colors;
            public NativeArray<float4> uvs;
            public NativeArray<int> indices;

            public static OutputData Allocate(in SourceData source)
            {
                int vc = source.triangleCount * MaxHullVertices;

                // cleared memory: unused hull slots stay zeroed and are never referenced
                return new OutputData
                {
                    allocated = true,
                    vertexCount = vc,
                    positions = new NativeArray<float3>(vc, Allocator.TempJob),
                    normals = new NativeArray<float3>(source.normals.Length > 0 ? vc : 0, Allocator.TempJob),
                    tangents = new NativeArray<float4>(source.tangents.Length > 0 ? vc : 0, Allocator.TempJob),
                    colors = new NativeArray<float4>(source.colors.Length > 0 ? vc : 0, Allocator.TempJob),
                    uvs = new NativeArray<float4>(vc * source.uvChannels.Length, Allocator.TempJob),
                    indices = new NativeArray<int>(source.triangleCount * MaxHullIndices, Allocator.TempJob),
                };
            }

            public void Dispose()
            {
                if (positions.IsCreated) positions.Dispose();
                if (normals.IsCreated) normals.Dispose();
                if (tangents.IsCreated) tangents.Dispose();
                if (colors.IsCreated) colors.Dispose();
                if (uvs.IsCreated) uvs.Dispose();
                if (indices.IsCreated) indices.Dispose();
            }
        }

        // One triangle per iteration. Each triangle owns a fixed slot of MaxHullVertices vertices and
        // MaxHullIndices indices in the output, so iterations never touch each other's data.
        [BurstCompile(CompileSynchronously = true)]
        unsafe struct ExpandJob : IJobParallelFor
        {
            [ReadOnly] public NativeArray<int> indices;
            [ReadOnly] public NativeArray<float2> lightmapUV;
            [ReadOnly] public NativeArray<float3> positions;
            [ReadOnly] public NativeArray<float3> normals;
            [ReadOnly] public NativeArray<float4> tangents;
            [ReadOnly] public NativeArray<float4> colors;
            [ReadOnly] public NativeArray<float4> uvs;
            public int vertexCount;
            public int uvSlotCount;
            public float2 toTexel;

            [WriteOnly, NativeDisableParallelForRestriction] public NativeArray<float3> outPositions;
            [WriteOnly, NativeDisableParallelForRestriction] public NativeArray<float3> outNormals;
            [WriteOnly, NativeDisableParallelForRestriction] public NativeArray<float4> outTangents;
            [WriteOnly, NativeDisableParallelForRestriction] public NativeArray<float4> outColors;
            [WriteOnly, NativeDisableParallelForRestriction] public NativeArray<float4> outUvs;
            [WriteOnly, NativeDisableParallelForRestriction] public NativeArray<int> outIndices;
            public int outVertexCount;

            public void Execute(int tri)
            {
                int baseV = tri * MaxHullVertices;
                int baseI = tri * MaxHullIndices;

                // default to degenerate triangles; overwritten below for the real fan
                for (int k = 0; k < MaxHullIndices; k++)
                {
                    outIndices[baseI + k] = baseV;
                }

                int i0 = indices[tri * 3];
                int i1 = indices[tri * 3 + 1];
                int i2 = indices[tri * 3 + 2];

                float2 p0 = lightmapUV[i0] * toTexel;
                float2 p1 = lightmapUV[i1] * toTexel;
                float2 p2 = lightmapUV[i2] * toTexel;

                float area = Cross(p1 - p0, p2 - p0);
                if (math.abs(area) < 1e-8f)
                {
                    return; // degenerate in lightmap space
                }
                float invArea = 1f / area;

                float2* corners = stackalloc float2[12];
                float2* hull = stackalloc float2[24];

                AddHalfTexelCorners(corners, 0, p0);
                AddHalfTexelCorners(corners, 4, p1);
                AddHalfTexelCorners(corners, 8, p2);

                int hullCount = ConvexHull(corners, 12, hull);
                if (hullCount < 3)
                {
                    return;
                }
                hullCount = math.min(hullCount, MaxHullVertices);

                // hull is CCW; keep the original triangle's winding in case the pass culls
                bool reverse = area < 0f;

                for (int k = 0; k < hullCount; k++)
                {
                    float2 q = hull[reverse ? hullCount - 1 - k : k];

                    // barycentrics of q relative to the original triangle (negative = extrapolation)
                    float w1 = Cross(q - p0, p2 - p0) * invArea;
                    float w2 = Cross(p1 - p0, q - p0) * invArea;
                    float w0 = 1f - w1 - w2;

                    int v = baseV + k;

                    outPositions[v] = positions[i0] * w0 + positions[i1] * w1 + positions[i2] * w2;

                    if (normals.Length > 0)
                    {
                        float3 n = normals[i0] * w0 + normals[i1] * w1 + normals[i2] * w2;
                        outNormals[v] = math.normalizesafe(n, normals[i0]);
                    }

                    if (tangents.Length > 0)
                    {
                        float4 t = tangents[i0] * w0 + tangents[i1] * w1 + tangents[i2] * w2;
                        float3 t3 = math.normalizesafe(t.xyz, tangents[i0].xyz);
                        outTangents[v] = new float4(t3, tangents[i0].w);
                    }

                    if (colors.Length > 0)
                    {
                        outColors[v] = colors[i0] * w0 + colors[i1] * w1 + colors[i2] * w2;
                    }

                    for (int slot = 0; slot < uvSlotCount; slot++)
                    {
                        int src = slot * vertexCount;
                        outUvs[slot * outVertexCount + v] =
                            uvs[src + i0] * w0 + uvs[src + i1] * w1 + uvs[src + i2] * w2;
                    }
                }

                // fan-triangulate the convex footprint
                for (int k = 1; k + 1 < hullCount; k++)
                {
                    int o = baseI + (k - 1) * 3;
                    outIndices[o] = baseV;
                    outIndices[o + 1] = baseV + k;
                    outIndices[o + 2] = baseV + k + 1;
                }
            }

            static float Cross(float2 a, float2 b)
            {
                return a.x * b.y - a.y * b.x;
            }

            static void AddHalfTexelCorners(float2* corners, int start, float2 p)
            {
                corners[start + 0] = p + new float2(-0.5f, -0.5f);
                corners[start + 1] = p + new float2(0.5f, -0.5f);
                corners[start + 2] = p + new float2(-0.5f, 0.5f);
                corners[start + 3] = p + new float2(0.5f, 0.5f);
            }

            // Andrew's monotone chain. Sorts points in place, writes the CCW hull (no repeated last point)
            // and returns its vertex count. hull must have room for 2 * count points.
            static int ConvexHull(float2* points, int count, float2* hull)
            {
                for (int i = 1; i < count; i++)
                {
                    float2 p = points[i];
                    int j = i - 1;
                    while (j >= 0 && (points[j].x > p.x || (points[j].x == p.x && points[j].y > p.y)))
                    {
                        points[j + 1] = points[j];
                        j--;
                    }
                    points[j + 1] = p;
                }

                int k = 0;
                for (int i = 0; i < count; i++)
                {
                    while (k >= 2 && Cross(hull[k - 1] - hull[k - 2], points[i] - hull[k - 2]) <= 0f) k--;
                    hull[k++] = points[i];
                }
                for (int i = count - 2, lower = k + 1; i >= 0; i--)
                {
                    while (k >= lower && Cross(hull[k - 1] - hull[k - 2], points[i] - hull[k - 2]) <= 0f) k--;
                    hull[k++] = points[i];
                }
                return k - 1;
            }
        }
    }
}
#else
using System;
using System.Collections.Generic;
using UnityEngine;

namespace Glim
{
    // Without Burst the conservative pass is skipped entirely: building the expanded meshes in managed
    // code is too slow to be worth it. The meta atlases are still rendered with the jittered passes only.
    public class ConservativeMeshCache : IDisposable
    {
        bool _logged;

        public ConservativeMeshCache(int resolution)
        {
        }

        public void Build(IReadOnlyList<(Mesh mesh, Vector4 lightmapST)> requests)
        {
            if (_logged || requests.Count == 0)
            {
                return;
            }

            _logged = true;
            Debug.LogError("[Glim] Burst (com.unity.burst) is not installed. Skipping conservative rasterization of the meta atlases; thin triangles may leave holes in albedo and emission.");
        }

        public Mesh Get(Mesh src, Vector4 lightmapST)
        {
            return null;
        }

        public void Dispose()
        {
        }
    }
}
#endif
