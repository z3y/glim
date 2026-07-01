using System;
using System.Collections.Generic;
using UnityEditor;
using UnityEngine;
using UnityEngine.Rendering;

namespace stilb
{
    public class MetaTexture : IDisposable
    {
        RenderTexture _rt;

        int _resolution;
        Material _metaAlphaMat;
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

            _metaAlphaMat = AssetDatabase.LoadAssetAtPath<Material>("Packages/io.github.z3y.stilb/Editor/AlphaMeta.mat");
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

            var format = type == AtlasType.Albedo ? TextureFormat.RGBA32 : TextureFormat.RGBAFloat;

            var request = AsyncGPUReadback.Request(_rt, 0, format);
            request.WaitForCompletion();
            return request;
        }

        static int _Cutoff = Shader.PropertyToID("_Cutoff");

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

            var _MainTex = Shader.PropertyToID("_MainTex");
            var _Color = Shader.PropertyToID("_Color");
            var unity_LightmapST = Shader.PropertyToID("unity_LightmapST");

            for (int offsetIndex = 0; offsetIndex < uvOffset.Length; offsetIndex++)
            {
                for (int rendererIndex = 0; rendererIndex < renderers.Count; rendererIndex++)
                {
                    var renderer = renderers[rendererIndex];
                    var mesh = renderer.GetComponent<MeshFilter>().sharedMesh;

                    if (!mesh)
                    {
                        continue;
                    }

                    var so = renderer.lightmapScaleOffset;
                    so.z += uvOffset[offsetIndex].z;
                    so.w += uvOffset[offsetIndex].w;

                    cmd.SetGlobalVector(unity_LightmapST, so);

                    for (int submeshIndex = 0; submeshIndex < mesh.subMeshCount; submeshIndex++)
                    {
                        var mat = renderer.sharedMaterials[submeshIndex];

                        if (type == AtlasType.Alpha)
                        {
                            if (!IsMaterialTransparent(mat))
                            {
                                continue;
                            }

                            _metaAlphaMat.SetTexture(_MainTex, mat.mainTexture);
                            _metaAlphaMat.SetColor(_Color, mat.color);
                            string renderType = mat.GetTag("RenderType", false, "");
                            if (_metaAlphaMat.HasProperty(_Cutoff) && renderType == "TransparentCutout")
                            {
                                _metaAlphaMat.SetFloat(_Cutoff, mat.GetFloat(_Cutoff));
                            }
                            else
                            {
                                _metaAlphaMat.SetFloat(_Cutoff, 0.5f);
                            }
                            _metaAlphaMat.SetTextureOffset(_MainTex, mat.mainTextureOffset);
                            _metaAlphaMat.SetTextureScale(_MainTex, mat.mainTextureScale);
                            cmd.DrawRenderer(renderer, _metaAlphaMat, submeshIndex, 0);
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

            Graphics.ExecuteCommandBuffer(cmd);
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

        }
    }
}