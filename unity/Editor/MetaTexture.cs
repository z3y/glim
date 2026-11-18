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
                    localToWorld = renderer.localToWorldMatrix,
                    mpb = mpb,
                    materials = materials,
                    lightmapSo = lightmapSo,
                    baseSo = baseSo,
                });
            }

            // Build every missing expanded mesh in one batch of parallel Burst jobs.
            if (conservativeMeshes != null)
            {
                var requests = new List<(Mesh, Vector4)>(items.Count);
                for (int i = 0; i < items.Count; i++)
                {
                    requests.Add((items[i].mesh, items[i].lightmapSo));
                }
                conservativeMeshes.Build(requests);
            }

            // Pass 1: conservatively expanded triangles for every renderer. This guarantees every
            // texel touched by a triangle gets at least one sample. It runs for all renderers before
            // any jittered pass, so it only survives in texels the regular passes never reach.
            for (int i = 0; conservativeMeshes != null && i < items.Count; i++)
            {
                var item = items[i];
                var conservativeMesh = conservativeMeshes.Get(item.mesh, item.lightmapSo);
                if (!conservativeMesh)
                {
                    continue;
                }

                cmd.SetGlobalVector(unity_LightmapST, item.baseSo);
                DrawMetaSubmeshes(cmd, conservativeMesh, item, type);
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
            public Matrix4x4 localToWorld;
            public MaterialPropertyBlock mpb;
            public Material[] materials;
            public Vector4 lightmapSo;
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
