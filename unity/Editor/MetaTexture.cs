using System;
using System.Collections.Generic;
using UnityEditor;
using UnityEngine;
using UnityEngine.Rendering;
using UnityEngine.Experimental.Rendering;

namespace Glim
{
    public class MetaTexture : IDisposable
    {
        RenderTexture _rt;

        int _resolution;

        public MetaTexture(int resolution, AtlasType type)
        {
            _resolution = resolution;

            // todo limit resolution
            var desc = new RenderTextureDescriptor
            {
                autoGenerateMips = false,
                width = resolution,
                height = resolution,
                useMipMap = false,
                mipCount = 1,
                colorFormat = type == AtlasType.Albedo ? RenderTextureFormat.ARGB32 : RenderTextureFormat.RGB111110Float,
                sRGB = false,
                volumeDepth = 1,
                msaaSamples = 1,
                dimension = TextureDimension.Tex2D
            };

            _rt = new RenderTexture(desc);
            _rt.filterMode = FilterMode.Point;
        }

        public enum AtlasType
        {
            Albedo,
            Emission,
        }

        // Expanded meshes depend on the source mesh, the renderer's lightmap scale and the atlas
        // resolution. Share one cache between the albedo and emission atlases of a group so each
        // mesh is only built once.
        public class ConservativeMeshCache : IDisposable
        {
            readonly int _resolution;
            readonly Dictionary<(Mesh, Vector2), Mesh> _meshes = new();

            public ConservativeMeshCache(int resolution)
            {
                _resolution = resolution;
            }

            public Mesh Get(Mesh src, Vector4 lightmapST)
            {
                var key = (src, new Vector2(Mathf.Abs(lightmapST.x), Mathf.Abs(lightmapST.y)));
                if (_meshes.TryGetValue(key, out var cached))
                {
                    return cached; // may be null for meshes that can't be expanded; don't retry
                }

                var mesh = BuildConservativeMesh(src, key.Item2, _resolution);
                _meshes[key] = mesh;
                return mesh;
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
        }

        public AsyncGPUReadbackRequest CreateAtlas(IList<Renderer> renderers, AtlasType type, ConservativeMeshCache conservativeMeshes)
        {
            using var cmd = new CommandBuffer();
            cmd.SetRenderTarget(_rt);

            if (type == AtlasType.Albedo)
            {
                cmd.ClearRenderTarget(true, true, Color.gray);
            }
            else
            {
                cmd.ClearRenderTarget(true, true, Color.black);
            }

            RenderMeta(renderers, type, cmd, conservativeMeshes);

            Graphics.ExecuteCommandBuffer(cmd);

            var format = type == AtlasType.Albedo ? GraphicsFormat.R8G8B8A8_UNorm : GraphicsFormat.B10G11R11_UFloatPack32;

            return AsyncGPUReadback.Request(_rt, 0, format);
        }

        static int _Cutoff = Shader.PropertyToID("_Cutoff");
        static int _MainTex = Shader.PropertyToID("_MainTex");
        static int _BaseMap = Shader.PropertyToID("_BaseMap");
        static int _Color = Shader.PropertyToID("_Color");
        static int _BaseColor = Shader.PropertyToID("_BaseColor");

        Color GetMaterialColor(Material mat)
        {
            if (mat.HasProperty(_Color))
            {
                return mat.GetColor(_Color);
            }
            if (mat.HasProperty(_BaseColor))
            {
                return mat.GetColor(_BaseColor);
            }
            return Color.white;
        }

        static Texture GetMaterialAlbedo(Material mat)
        {
            if (mat.HasProperty(_MainTex))
            {
                return mat.GetTexture(_MainTex);
            }
            if (mat.HasProperty(_BaseMap))
            {
                return mat.GetTexture(_BaseMap);
            }
            return Texture2D.whiteTexture;
        }

        public static Texture2D GetMaterialAlpha(Material mat)
        {
            Texture tex = GetMaterialAlbedo(mat);

            if (tex is Texture2D tex2d)
            {
                int components = (int)GraphicsFormatUtility.GetComponentCount(tex2d.graphicsFormat);
                bool alpha8 = tex2d.format == TextureFormat.Alpha8;

                if (alpha8 || components == 4)
                {
                    return tex2d;
                }
            }

            return Texture2D.whiteTexture;
        }

        Vector2 GetMaterialAlbedoScale(Material mat)
        {
            if (mat.HasProperty(_MainTex))
            {
                return mat.GetTextureScale(_MainTex);
            }
            if (mat.HasProperty(_BaseMap))
            {
                return mat.GetTextureScale(_BaseMap);
            }
            return Vector2.one;
        }

        Vector2 GetMaterialAlbedoOffset(Material mat)
        {
            if (mat.HasProperty(_MainTex))
            {
                return mat.GetTextureOffset(_MainTex);
            }
            if (mat.HasProperty(_BaseMap))
            {
                return mat.GetTextureOffset(_BaseMap);
            }
            return Vector2.zero;
        }

        public static float GetMaterialCutoff(Material source)
        {
            string renderType = source.GetTag("RenderType", false, "");
            if (source.HasProperty(_Cutoff) && renderType == "TransparentCutout")
            {
                return source.GetFloat(_Cutoff);
            }
            else
            {
                return 0.5f;
            }
        }

        void RenderMeta(IList<Renderer> renderers, AtlasType type, CommandBuffer cmd, ConservativeMeshCache conservativeMeshes)
        {
            float near = 0.01f;
            float far = 100f;

            // Ortho projection matrix
            Matrix4x4 proj = Matrix4x4.Ortho(0, 1, 0, 1, near, far);
            // View matrix (like a top-down or front view)
            Vector3 camPos = new Vector3(0, 0, -10);
            Vector3 target = Vector3.zero;
            Vector3 up = Vector3.up;
            Matrix4x4 view = Matrix4x4.LookAt(camPos, target, up);
            cmd.SetViewProjectionMatrices(view, proj);

            cmd.SetGlobalVector("unity_MetaVertexControl", new Vector4(1, 0, 0, 0));
            cmd.SetGlobalFloat("unity_OneOverOutputBoost", 1.0f);
            cmd.SetGlobalFloat("unity_UseLinearSpace", 1.0f);

            if (type == AtlasType.Albedo)
            {
                cmd.SetGlobalVector("unity_MetaFragmentControl", new Vector4(1, 0, 0, 0));
                cmd.SetGlobalFloat("unity_MaxOutputValue", 1.0f);
            }
            else if (type == AtlasType.Emission)
            {
                cmd.SetGlobalVector("unity_MetaFragmentControl", new Vector4(0, 1, 0, 0));
                cmd.SetGlobalFloat("unity_MaxOutputValue", 100.0f);
            }

            cmd.SetGlobalFloat("unity_VisualizationMode", -1);

            cmd.SetGlobalVector("unity_LightmapST", new Vector4(1f, 1f, 0, 0));

            // https://ndotl.wordpress.com/2018/08/29/baking-artifact-free-lightmaps/#raster
            Vector4[] uvOffset = new Vector4[]
            {
                    new (1f, 1f, -2, -2f),
                    new (1f, 1f, 2, -2f),
                    new (1f, 1f, -2, 2f),
                    new (1f, 1f, 2f, 2f),
                    new (1f, 1f, -1f, -2f),
                    new (1f, 1f, 1f, -2f),
                    new (1f, 1f, -2f, -1f),
                    new (1f, 1f, 2f, -1f),
                    new (1f, 1f, -2f, 1f),
                    new (1f, 1f, 2f, 1f),
                    new (1f, 1f, -1f, 2f),
                    new (1f, 1f, 1f, 2f),
                    new (1f, 1f, -2f, 0f),
                    new (1f, 1f, 2f, 0f),
                    new (1f, 1f, 0f, -2f),
                    new (1f, 1f, 0f, 2f),
                    new (1f, 1f, -1f, -1f),
                    new (1f, 1f, 1f, -1f),
                    new (1f, 1f, -1f, 0f),
                    new (1f, 1f, 1f, 0f),
                    new (1f, 1f, -1f, 1f),
                    new (1f, 1f, 1f, 1f),
                    new (1f, 1f, 0f, -1f),
                    new (1f, 1f, 0f, 1f),
                    new (1f, 1f, 0f, 0f)
            };

            float halfTexelSize = (1.0f / _resolution) * 0.5f;
            for (int i = 0; i < uvOffset.Length; i++)
            {
                uvOffset[i].z *= halfTexelSize;
                uvOffset[i].w *= halfTexelSize;
            }

            var unity_LightmapST = Shader.PropertyToID("unity_LightmapST");

            bool flipY = !SystemInfo.graphicsUVStartsAtTop;

            var items = new List<MetaDrawItem>(renderers.Count);

            for (int rendererIndex = 0; rendererIndex < renderers.Count; rendererIndex++)
            {
                var renderer = renderers[rendererIndex];
                var mesh = renderer.GetComponent<MeshFilter>().sharedMesh;

                if (!mesh)
                {
                    continue;
                }

                MaterialPropertyBlock mpb = new();
                renderer.GetPropertyBlock(mpb);
                if (renderer is MeshRenderer mr)
                {
                    var enlightenVertexStream = mr.enlightenVertexStream;
                    var additionalVertexStreams = mr.additionalVertexStreams;
                    if (enlightenVertexStream)
                    {
                        if (enlightenVertexStream.HasVertexAttribute(VertexAttribute.TexCoord1))
                        {
                            mesh = enlightenVertexStream;
                        }
                    }
                    else if (additionalVertexStreams)
                    {
                        if (additionalVertexStreams.HasVertexAttribute(VertexAttribute.TexCoord1))
                        {
                            mesh = additionalVertexStreams;
                        }
                    }
                }

                var materials = renderer.sharedMaterials;

                // Skip renderers that wouldn't draw anything in this atlas (e.g. no emissive
                // materials), so their expanded mesh is never built.
                if (!HasAnyMetaDraw(materials, mesh.subMeshCount, type))
                {
                    continue;
                }

                var lightmapSo = renderer.lightmapScaleOffset;
                var baseSo = lightmapSo;

                if (flipY)
                {
                    baseSo.y = -baseSo.y;
                    baseSo.w = 1.0f - baseSo.w;
                }

                items.Add(new MetaDrawItem
                {
                    mesh = mesh,
                    conservativeMesh = conservativeMeshes?.Get(mesh, lightmapSo),
                    localToWorld = renderer.localToWorldMatrix,
                    mpb = mpb,
                    materials = materials,
                    baseSo = baseSo,
                });
            }

            // Pass 1: conservatively expanded triangles for every renderer. This guarantees every
            // texel touched by a triangle gets at least one sample. It runs for all renderers before
            // any jittered pass, so it only survives in texels the regular passes never reach.
            for (int i = 0; i < items.Count; i++)
            {
                var item = items[i];
                if (!item.conservativeMesh)
                {
                    continue;
                }

                cmd.SetGlobalVector(unity_LightmapST, item.baseSo);
                DrawMetaSubmeshes(cmd, item.conservativeMesh, item, type);
            }

            // Pass 2: regular rasterization with jitter offsets, drawn on top.
            // Renderers are the outer loop so the per-renderer lookups happen once
            // instead of once per jitter offset. Each renderer owns a distinct atlas region
            // via lightmapScaleOffset and its own offsets still run in order.
            for (int i = 0; i < items.Count; i++)
            {
                var item = items[i];

                for (int offsetIndex = 0; offsetIndex < uvOffset.Length; offsetIndex++)
                {
                    var so = item.baseSo;
                    so.z += uvOffset[offsetIndex].z;
                    so.w += uvOffset[offsetIndex].w;

                    cmd.SetGlobalVector(unity_LightmapST, so);
                    DrawMetaSubmeshes(cmd, item.mesh, item, type);
                }
            }
        }

        struct MetaDrawItem
        {
            public Mesh mesh;
            public Mesh conservativeMesh;
            public Matrix4x4 localToWorld;
            public MaterialPropertyBlock mpb;
            public Material[] materials;
            public Vector4 baseSo;
        }

        // Returns the META pass index to draw for this material in this atlas, or -1 to skip it.
        static int GetMetaPass(Material mat, AtlasType type)
        {
            if (!mat)
            {
                return -1;
            }

            if (type == AtlasType.Emission && !IsMaterialEmissive(mat))
            {
                return -1;
            }

            return mat.FindPass("META");
        }

        static bool HasAnyMetaDraw(Material[] materials, int submeshCount, AtlasType type)
        {
            int count = Mathf.Min(materials.Length, submeshCount);
            for (int i = 0; i < count; i++)
            {
                if (GetMetaPass(materials[i], type) >= 0)
                {
                    return true;
                }
            }
            return false;
        }

        static void DrawMetaSubmeshes(CommandBuffer cmd, Mesh mesh, in MetaDrawItem item, AtlasType type)
        {
            var sharedMaterials = item.materials;
            int count = Mathf.Min(sharedMaterials.Length, mesh.subMeshCount);

            for (int submeshIndex = 0; submeshIndex < count; submeshIndex++)
            {
                var mat = sharedMaterials[submeshIndex];
                int meta = GetMetaPass(mat, type);
                if (meta >= 0)
                {
                    cmd.DrawMesh(mesh, item.localToWorld, mat, submeshIndex, meta, item.mpb);
                }
            }
        }

        // Builds a copy of src where every triangle is replaced by its exact conservative footprint in
        // lightmap texel space: the triangle grown by a half-texel square (Minkowski sum), i.e. the convex
        // hull of each vertex ± (0.5, 0.5). Rasterizing that polygon with regular point sampling hits every
        // texel whose square overlaps the triangle, which is what hardware conservative rasterization does,
        // and nothing more (at most 0.5 texel in x/y). All vertex attributes are linearly extrapolated with
        // the hull vertices' barycentrics, so the unmodified META shader sees the surface continuing past
        // the original edges.
        static Mesh BuildConservativeMesh(Mesh src, Vector2 lightmapScale, int resolution)
        {
            var uv1 = new List<Vector2>();
            src.GetUVs(1, uv1);
            if (uv1.Count == 0)
            {
                src.GetUVs(0, uv1);
            }
            if (uv1.Count == 0)
            {
                return null;
            }

            var pos = new List<Vector3>(); src.GetVertices(pos);
            var nrm = new List<Vector3>(); src.GetNormals(nrm);
            var tan = new List<Vector4>(); src.GetTangents(tan);
            var col = new List<Color>(); src.GetColors(col);
            var uvs = new List<Vector4>[8];
            for (int c = 0; c < uvs.Length; c++)
            {
                uvs[c] = new List<Vector4>();
                if (src.HasVertexAttribute(VertexAttribute.TexCoord0 + c))
                {
                    src.GetUVs(c, uvs[c]);
                }
            }

            Vector2 toTexel = lightmapScale * resolution;

            // Footprints are mostly 6-7 sided, so reserve ~7 vertices per source triangle up front
            // instead of letting the lists regrow.
            long indexCount = 0;
            for (int s = 0; s < src.subMeshCount; s++) indexCount += src.GetIndexCount(s);
            int vertexCapacity = (int)Math.Min(indexCount / 3 * 7, int.MaxValue);

            var oPos = new List<Vector3>(vertexCapacity);
            var oNrm = new List<Vector3>(nrm.Count > 0 ? vertexCapacity : 0);
            var oTan = new List<Vector4>(tan.Count > 0 ? vertexCapacity : 0);
            var oCol = new List<Color>(col.Count > 0 ? vertexCapacity : 0);
            var oUvs = new List<Vector4>[uvs.Length];
            for (int c = 0; c < oUvs.Length; c++) oUvs[c] = new List<Vector4>(uvs[c].Count > 0 ? vertexCapacity : 0);

            var subIndices = new List<int>[src.subMeshCount];
            var tris = new List<int>();
            var corners = new Vector2[12];
            var hull = new Vector2[24];

            for (int s = 0; s < src.subMeshCount; s++)
            {
                var outIdx = new List<int>((int)Math.Min(src.GetIndexCount(s) * 5L, int.MaxValue));
                subIndices[s] = outIdx;

                if (src.GetTopology(s) != MeshTopology.Triangles)
                {
                    continue;
                }

                src.GetTriangles(tris, s);

                for (int t = 0; t + 2 < tris.Count; t += 3)
                {
                    int i0 = tris[t], i1 = tris[t + 1], i2 = tris[t + 2];
                    Vector2 p0 = Vector2.Scale(uv1[i0], toTexel);
                    Vector2 p1 = Vector2.Scale(uv1[i1], toTexel);
                    Vector2 p2 = Vector2.Scale(uv1[i2], toTexel);

                    float area = (p1.x - p0.x) * (p2.y - p0.y) - (p2.x - p0.x) * (p1.y - p0.y);
                    if (Mathf.Abs(area) < 1e-8f)
                    {
                        continue; // degenerate in lightmap space
                    }
                    // Exact conservative footprint for point sampling at texel centers:
                    // triangle ⊕ [-0.5, 0.5]² = convex hull of each vertex's four half-texel corners.
                    AddHalfTexelCorners(corners, 0, p0);
                    AddHalfTexelCorners(corners, 4, p1);
                    AddHalfTexelCorners(corners, 8, p2);
                    int hullCount = ConvexHull(corners, hull);
                    if (hullCount < 3)
                    {
                        continue;
                    }

                    // hull is CCW; keep the original triangle's winding in case the pass culls
                    bool reverse = area < 0f;

                    int baseV = oPos.Count;
                    for (int k = 0; k < hullCount; k++)
                    {
                        Vector2 q = hull[reverse ? hullCount - 1 - k : k];

                        // barycentrics of q relative to the original triangle (negative = extrapolation)
                        float w1 = ((q.x - p0.x) * (p2.y - p0.y) - (p2.x - p0.x) * (q.y - p0.y)) / area;
                        float w2 = ((p1.x - p0.x) * (q.y - p0.y) - (q.x - p0.x) * (p1.y - p0.y)) / area;
                        float w0 = 1f - w1 - w2;

                        oPos.Add(pos[i0] * w0 + pos[i1] * w1 + pos[i2] * w2);
                        if (nrm.Count > 0)
                        {
                            var nn = nrm[i0] * w0 + nrm[i1] * w1 + nrm[i2] * w2;
                            oNrm.Add(nn.sqrMagnitude > 1e-12f ? nn.normalized : nrm[i0]);
                        }
                        if (tan.Count > 0)
                        {
                            var tt = tan[i0] * w0 + tan[i1] * w1 + tan[i2] * w2;
                            Vector3 t3 = tt;
                            t3 = t3.sqrMagnitude > 1e-12f ? t3.normalized : (Vector3)tan[i0];
                            oTan.Add(new Vector4(t3.x, t3.y, t3.z, tan[i0].w));
                        }
                        if (col.Count > 0)
                        {
                            oCol.Add(col[i0] * w0 + col[i1] * w1 + col[i2] * w2);
                        }
                        for (int c = 0; c < uvs.Length; c++)
                        {
                            if (uvs[c].Count > 0)
                            {
                                oUvs[c].Add(uvs[c][i0] * w0 + uvs[c][i1] * w1 + uvs[c][i2] * w2);
                            }
                        }
                    }

                    // fan-triangulate the convex footprint
                    for (int k = 1; k + 1 < hullCount; k++)
                    {
                        outIdx.Add(baseV);
                        outIdx.Add(baseV + k);
                        outIdx.Add(baseV + k + 1);
                    }
                }
            }

            if (oPos.Count == 0)
            {
                return null;
            }

            var dst = new Mesh
            {
                name = src.name + " (Conservative)",
                hideFlags = HideFlags.HideAndDontSave,
                indexFormat = IndexFormat.UInt32,
            };

            dst.SetVertices(oPos);
            if (oNrm.Count > 0) dst.SetNormals(oNrm);
            if (oTan.Count > 0) dst.SetTangents(oTan);
            if (oCol.Count > 0) dst.SetColors(oCol);
            for (int c = 0; c < oUvs.Length; c++)
            {
                if (oUvs[c].Count > 0) dst.SetUVs(c, oUvs[c]);
            }

            dst.subMeshCount = subIndices.Length;
            for (int s = 0; s < subIndices.Length; s++)
            {
                dst.SetTriangles(subIndices[s], s, false);
            }
            dst.bounds = src.bounds;
            dst.UploadMeshData(true);

            return dst;
        }

        static void AddHalfTexelCorners(Vector2[] corners, int start, Vector2 p)
        {
            corners[start + 0] = new Vector2(p.x - 0.5f, p.y - 0.5f);
            corners[start + 1] = new Vector2(p.x + 0.5f, p.y - 0.5f);
            corners[start + 2] = new Vector2(p.x - 0.5f, p.y + 0.5f);
            corners[start + 3] = new Vector2(p.x + 0.5f, p.y + 0.5f);
        }

        static float Cross(Vector2 o, Vector2 a, Vector2 b)
        {
            return (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
        }

        // Andrew's monotone chain. Sorts points in place, writes the CCW hull (no repeated last point)
        // into hull and returns its vertex count. hull must have room for 2 * points.Length.
        static int ConvexHull(Vector2[] points, Vector2[] hull)
        {
            // insertion sort by x then y (only 12 points)
            for (int i = 1; i < points.Length; i++)
            {
                var p = points[i];
                int j = i - 1;
                while (j >= 0 && (points[j].x > p.x || (points[j].x == p.x && points[j].y > p.y)))
                {
                    points[j + 1] = points[j];
                    j--;
                }
                points[j + 1] = p;
            }

            int k = 0;
            for (int i = 0; i < points.Length; i++)
            {
                while (k >= 2 && Cross(hull[k - 2], hull[k - 1], points[i]) <= 0f) k--;
                hull[k++] = points[i];
            }
            for (int i = points.Length - 2, lower = k + 1; i >= 0; i--)
            {
                while (k >= lower && Cross(hull[k - 2], hull[k - 1], points[i]) <= 0f) k--;
                hull[k++] = points[i];
            }
            return k - 1;
        }

        public static bool IsMaterialEmissive(Material mat)
        {
            if (!mat) return false;
            if (!mat.shader) return false;

            return mat.globalIlluminationFlags.HasFlag(MaterialGlobalIlluminationFlags.BakedEmissive);
        }


        public static readonly HashSet<string> TransparentTags = new()
        {
            "Transparent",
            "TransparentCutout",
            "TreeTransparentCutout",
            "GrassBillboard",
            "Grass",
            "TreeLeaf"
        };

        public static bool IsMaterialTransparent(Material mat)
        {
            if (!mat) return false;
            if (!mat.shader) return false;

            {
                string type = mat.GetTag("SurfaceType", false, "");
                if (TransparentTags.Contains(type)) return true;
            }

            {
                string type = mat.GetTag("RenderType", false, "");
                if (TransparentTags.Contains(type)) return true;
            }

            // if (mat.renderQueue >= (int)RenderQueue.AlphaTest)
            // {
            //     return true;
            // }

            return false;
        }

        // Transparent materials that are alpha blended rather than alpha tested (cutout, grass, leaves).
        // These get real partial shadows instead of a cutoff.
        public static bool IsMaterialAlphaBlended(Material mat)
        {
            if (!mat) return false;
            if (!mat.shader) return false;

            const string blended = "Transparent";

            if (mat.GetTag("SurfaceType", false, "") == blended) return true;
            if (mat.GetTag("RenderType", false, "") == blended) return true;

            return false;
        }

        public static int GetMaterialAlbedoProperty(Material mat)
        {
            if (mat.HasProperty(_MainTex)) return _MainTex;
            if (mat.HasProperty(_BaseMap)) return _BaseMap;
            return -1;
        }

        public static void GetMaterialAlbedoScaleOffset(Material mat, out Vector2 scale, out Vector2 offset)
        {
            int prop = GetMaterialAlbedoProperty(mat);
            if (prop >= 0)
            {
                scale = mat.GetTextureScale(prop);
                offset = mat.GetTextureOffset(prop);
            }
            else
            {
                scale = Vector2.one;
                offset = Vector2.zero;
            }
        }

        public static float GetMaterialColorAlpha(Material mat)
        {
            if (mat.HasProperty(_BaseColor)) return mat.GetColor(_BaseColor).a;
            if (mat.HasProperty(_Color)) return mat.GetColor(_Color).a;
            return 1.0f;
        }

        public void Dispose()
        {
            if (_rt)
            {
                Editor.DestroyImmediate(_rt);
            }
        }
    }
}