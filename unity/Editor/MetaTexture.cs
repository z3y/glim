using System;
using System.Collections.Generic;
using UnityEditor;
using UnityEngine;
using UnityEngine.Rendering;

namespace Glim
{
    public class MetaTexture : IDisposable
    {
        RenderTexture _rt;

        int _resolution;
        Material _metaAlphaMat;

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
                colorFormat = type == AtlasType.Albedo ? RenderTextureFormat.ARGB32 : RenderTextureFormat.ARGBHalf,
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

            if (type == AtlasType.Albedo)
            {
                cmd.ClearRenderTarget(true, true, Color.gray);
            }
            else
            {
                cmd.ClearRenderTarget(true, true, Color.black);
            }

            RenderMeta(renderers, type, cmd);
            if (type == AtlasType.Albedo)
            {
                RenderMeta(renderers, AtlasType.Alpha, cmd);
            }

            Graphics.ExecuteCommandBuffer(cmd);

            var format = type == AtlasType.Albedo ? TextureFormat.RGBA32 : TextureFormat.RGBAFloat;

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

        Texture GetMaterialAlbedo(Material mat)
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


        Material GetAlphaMaterial(Material source)
        {
            if (_alphaMatVariants.TryGetValue(source, out var variant))
            {
                return variant;
            }

            variant = new Material(_metaAlphaMat) { hideFlags = HideFlags.HideAndDontSave };

            variant.SetTexture(_MainTex, GetMaterialAlbedo(source));
            variant.SetColor(_Color, GetMaterialColor(source));

            string renderType = source.GetTag("RenderType", false, "");
            if (variant.HasProperty(_Cutoff) && renderType == "TransparentCutout")
            {
                variant.SetFloat(_Cutoff, source.GetFloat(_Cutoff));
            }
            else
            {
                variant.SetFloat(_Cutoff, 0.5f);
            }

            variant.SetTextureScale(_MainTex, GetMaterialAlbedoScale(source));
            variant.SetTextureOffset(_MainTex, GetMaterialAlbedoOffset(source));

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
                                if (!IsMaterialEmissive(mat))
                                {
                                    continue;
                                }
                            }
                            int meta = mat.FindPass("META");
                            if (meta >= 0)
                            {
                                cmd.DrawRenderer(renderer, mat, submeshIndex, meta);
                            }
                        }
                    }
                }
            }
        }

        public static bool IsMaterialEmissive(Material mat)
        {
            if (!mat) return false;
            if (!mat.shader) return false;

            return mat.globalIlluminationFlags.HasFlag(MaterialGlobalIlluminationFlags.BakedEmissive);
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