using System;
using System.Collections.Generic;
using UnityEditor;
using UnityEngine;
using UnityEngine.Rendering;

namespace glim
{
    public class MetaTexture : IDisposable
    {
        RenderTexture _rt;

        int _resolution;
        Material _metaAlphaMat;

        // CommandBuffer.DrawRenderer only records a material reference, so the shared
        // AlphaMeta asset cannot be re-configured per draw - every draw would resolve to
        // whatever was set last. Keep one configured variant per source material instead.
        readonly Dictionary<Material, Material> _alphaMatVariants = new();

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
                colorFormat = type == AtlasType.Albedo ? RenderTextureFormat.ARGB32 : RenderTextureFormat.ARGBFloat,
                sRGB = false,
                volumeDepth = 1,
                msaaSamples = 1,
                dimension = TextureDimension.Tex2D
            };

            _metaAlphaMat = AssetDatabase.LoadAssetAtPath<Material>("Packages/io.github.z3y.glim/Editor/AlphaMeta.mat");
            _rt = new RenderTexture(desc);
            _rt.filterMode = FilterMode.Point;
        }

        public enum AtlasType
        {
            Albedo,
            Emission,
            Alpha,
        }

        public AsyncGPUReadbackRequest CreateAtlas(IList<Renderer> renderers, AtlasType type)
        {
            using var cmd = new CommandBuffer();
            cmd.SetRenderTarget(_rt);

            cmd.ClearRenderTarget(true, true, Color.black);
            RenderMeta(renderers, type, cmd);
            if (type == AtlasType.Albedo)
            {
                RenderMeta(renderers, AtlasType.Alpha, cmd);
            }

            // Executed once here rather than at the end of each RenderMeta: the buffer is
            // cumulative, so executing per pass replayed the clear and every albedo draw a
            // second time and threw the first pass's output away.
            Graphics.ExecuteCommandBuffer(cmd);

            var format = type == AtlasType.Albedo ? TextureFormat.RGBA32 : TextureFormat.RGBAFloat;

            return AsyncGPUReadback.Request(_rt, 0, format);
        }

        static int _Cutoff = Shader.PropertyToID("_Cutoff");
        static int _MainTex = Shader.PropertyToID("_MainTex");
        static int _Color = Shader.PropertyToID("_Color");

        Material GetAlphaMaterial(Material source)
        {
            if (_alphaMatVariants.TryGetValue(source, out var variant))
            {
                return variant;
            }

            variant = new Material(_metaAlphaMat) { hideFlags = HideFlags.HideAndDontSave };
            variant.SetTexture(_MainTex, source.mainTexture);
            variant.SetColor(_Color, source.color);

            string renderType = source.GetTag("RenderType", false, "");
            if (variant.HasProperty(_Cutoff) && renderType == "TransparentCutout")
            {
                variant.SetFloat(_Cutoff, source.GetFloat(_Cutoff));
            }
            else
            {
                variant.SetFloat(_Cutoff, 0.5f);
            }

            variant.SetTextureOffset(_MainTex, source.mainTextureOffset);
            variant.SetTextureScale(_MainTex, source.mainTextureScale);

            _alphaMatVariants.Add(source, variant);
            return variant;
        }

        void RenderMeta(IList<Renderer> renderers, AtlasType type, CommandBuffer cmd)
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


            // Renderers are the outer loop so the per-renderer lookups below happen once
            // instead of once per jitter offset. Each renderer owns a distinct atlas region
            // via lightmapScaleOffset and its own offsets still run in order, so the draw
            // results are unchanged.
            for (int rendererIndex = 0; rendererIndex < renderers.Count; rendererIndex++)
            {
                var renderer = renderers[rendererIndex];
                var mesh = renderer.GetComponent<MeshFilter>().sharedMesh;

                if (!mesh)
                {
                    continue;
                }

                // Each access to .sharedMaterials allocates a new array.
                var sharedMaterials = renderer.sharedMaterials;

                var baseSo = renderer.lightmapScaleOffset;

                if (flipY)
                {
                    baseSo.y = -baseSo.y;
                    baseSo.w = 1.0f - baseSo.w;
                }

                for (int offsetIndex = 0; offsetIndex < uvOffset.Length; offsetIndex++)
                {
                    var so = baseSo;
                    so.z += uvOffset[offsetIndex].z;
                    so.w += uvOffset[offsetIndex].w;

                    cmd.SetGlobalVector(unity_LightmapST, so);

                    for (int submeshIndex = 0; submeshIndex < mesh.subMeshCount; submeshIndex++)
                    {
                        var mat = sharedMaterials[submeshIndex];

                        if (type == AtlasType.Alpha)
                        {
                            if (!IsMaterialTransparent(mat))
                            {
                                continue;
                            }

                            cmd.DrawRenderer(renderer, GetAlphaMaterial(mat), submeshIndex, 0);
                        }
                        else
                        {


                            if (type == AtlasType.Emission)
                            {
                                if (!mat.globalIlluminationFlags.HasFlag(MaterialGlobalIlluminationFlags.BakedEmissive))
                                {
                                    continue;
                                }
                            }
                            int meta = mat.FindPass("META");
                            cmd.DrawRenderer(renderer, mat, submeshIndex, meta);
                        }
                    }
                }
            }
        }

        public static bool IsMaterialTransparent(Material mat)
        {
            if (!mat) return false;
            if (!mat.shader) return false;

            string surfaceType = mat.GetTag("SurfaceType", false, "");
            if (surfaceType == "Transparent" || surfaceType == "TransparentCutout") return true;

            string renderType = mat.GetTag("RenderType", false, "");
            if (renderType == "Transparent" || renderType == "TransparentCutout") return true;

            // if (mat.renderQueue >= (int)RenderQueue.AlphaTest)
            // {
            //     return true;
            // }

            return false;
        }

        public void Dispose()
        {
            if (_rt)
            {
                Editor.DestroyImmediate(_rt);
            }

            foreach (var variant in _alphaMatVariants.Values)
            {
                if (variant)
                {
                    Editor.DestroyImmediate(variant);
                }
            }
            _alphaMatVariants.Clear();
        }
    }
}