using System;
using System.Collections.Generic;
using UnityEngine;
using UnityEngine.Rendering;

namespace Glim
{
    public static class TerrainExporter
    {
        /// <summary>
        /// Builds a world-relative mesh from a Terrain's heightmap.
        /// step=1 uses full heightmap resolution (can be huge, e.g. 513x513 -> ~263k verts).
        /// Increase step to downsample (2, 4, 8...) for lightmap-baking purposes.
        /// </summary>
        public static Mesh GenerateMesh(TerrainData data, int step = 2)
        {
            int hmRes = data.heightmapResolution; // e.g. 513
            var heights = data.GetHeights(0, 0, hmRes, hmRes); // indexed [z, x]
            var holes = data.GetHoles(0, 0, data.holesResolution, data.holesResolution); // indexed [z, x], true = solid

            int vertsX = (hmRes - 1) / step + 1;
            int vertsZ = (hmRes - 1) / step + 1;
            Vector3 size = data.size;

            var vertices = new Vector3[vertsX * vertsZ];
            var uvs = new Vector2[vertsX * vertsZ];

            for (int z = 0; z < vertsZ; z++)
            {
                int hz = Mathf.Min(z * step, hmRes - 1);
                for (int x = 0; x < vertsX; x++)
                {
                    int hx = Mathf.Min(x * step, hmRes - 1);
                    float worldX = (float)hx / (hmRes - 1) * size.x;
                    float worldZ = (float)hz / (hmRes - 1) * size.z;
                    float worldY = heights[hz, hx] * size.y;

                    int idx = z * vertsX + x;
                    vertices[idx] = new Vector3(worldX, worldY, worldZ);
                    // This matches the UV space terrain shaders already use (splatmap/control-texture space),
                    // which is convenient: it's already a 0..1 footprint over the whole terrain.
                    uvs[idx] = new Vector2(worldX / size.x, worldZ / size.z);
                }
            }

            var triangles = new List<int>((vertsX - 1) * (vertsZ - 1) * 6);
            int holeRes = data.holesResolution;

            for (int z = 0; z < vertsZ - 1; z++)
            {
                for (int x = 0; x < vertsX - 1; x++)
                {
                    // Skip quads that fall in a terrain hole
                    if (holes != null && holeRes > 1)
                    {
                        float u0 = (float)x / (vertsX - 1), u1 = (float)(x + 1) / (vertsX - 1);
                        float v0 = (float)z / (vertsZ - 1), v1 = (float)(z + 1) / (vertsZ - 1);
                        int hx0 = Mathf.Clamp(Mathf.RoundToInt(u0 * (holeRes - 1)), 0, holeRes - 1);
                        int hx1 = Mathf.Clamp(Mathf.RoundToInt(u1 * (holeRes - 1)), 0, holeRes - 1);
                        int hz0 = Mathf.Clamp(Mathf.RoundToInt(v0 * (holeRes - 1)), 0, holeRes - 1);
                        int hz1 = Mathf.Clamp(Mathf.RoundToInt(v1 * (holeRes - 1)), 0, holeRes - 1);

                        bool anyHole = !holes[hz0, hx0] || !holes[hz0, hx1] || !holes[hz1, hx0] || !holes[hz1, hx1];
                        if (anyHole) continue;
                    }

                    int i0 = z * vertsX + x;
                    int i1 = i0 + 1;
                    int i2 = i0 + vertsX;
                    int i3 = i2 + 1;

                    triangles.Add(i0); triangles.Add(i2); triangles.Add(i1);
                    triangles.Add(i1); triangles.Add(i2); triangles.Add(i3);
                }
            }

            var mesh = new Mesh();
            if (vertices.Length > 65535)
            {
                mesh.indexFormat = IndexFormat.UInt32;
            }
            mesh.vertices = vertices;
            mesh.uv = uvs;
            mesh.triangles = triangles.ToArray();
            mesh.RecalculateNormals();
            mesh.RecalculateBounds();
            return mesh;
        }
    }

    public class TerrainMetaTexture : IDisposable
    {
        RenderTexture _rt;

        static Mesh _quad;
        static Mesh QuadMesh => _quad ??= BuildQuad();

        static readonly int _Control = Shader.PropertyToID("_Control");
        static readonly int[] _Splat =
        {
            Shader.PropertyToID("_Splat0"),
            Shader.PropertyToID("_Splat1"),
            Shader.PropertyToID("_Splat2"),
            Shader.PropertyToID("_Splat3"),
        };

        public TerrainMetaTexture(int resolution, MetaTexture.AtlasType type)
        {
            var desc = new RenderTextureDescriptor
            {
                autoGenerateMips = false,
                width = resolution,
                height = resolution,
                useMipMap = false,
                mipCount = 1,
                colorFormat = type == MetaTexture.AtlasType.Albedo ? RenderTextureFormat.ARGB32 : RenderTextureFormat.ARGBHalf,
                sRGB = false,
                volumeDepth = 1,
                msaaSamples = 1,
                dimension = TextureDimension.Tex2D
            };
            _rt = new RenderTexture(desc) { filterMode = FilterMode.Point };
        }

        static Mesh BuildQuad()
        {
            var mesh = new Mesh
            {
                name = "GlimTerrainMetaQuad",
                hideFlags = HideFlags.HideAndDontSave,
                vertices = new[]
                {
                    new Vector3(0, 0, 0),
                    new Vector3(1, 0, 0),
                    new Vector3(0, 1, 0),
                    new Vector3(1, 1, 0),
                },
                uv = new[]
                {
                    new Vector2(0, 0),
                    new Vector2(1, 0),
                    new Vector2(0, 1),
                    new Vector2(1, 1),
                },
                triangles = new[] { 0, 2, 1, 1, 2, 3 }
            };
            mesh.RecalculateNormals();
            mesh.RecalculateBounds();
            return mesh;
        }

        static readonly int _MainTex_ST = Shader.PropertyToID("_MainTex_ST");
        static readonly int[] _SplatST =
        {
            Shader.PropertyToID("_Splat0_ST"),
            Shader.PropertyToID("_Splat1_ST"),
            Shader.PropertyToID("_Splat2_ST"),
            Shader.PropertyToID("_Splat3_ST"),
        };

        /// <summary>
        /// Mirrors what Unity's internal terrain renderer sets up per-frame: bind each alphamap
        /// (control texture) and its up-to-4 associated layer diffuse textures onto the shared
        /// terrain material for this draw only, via property block (no shared-material mutation).
        /// </summary>
        static MaterialPropertyBlock BuildPropertyBlock(Terrain terrain)
        {
            var mpb = new MaterialPropertyBlock();
            var data = terrain.terrainData;
            var layers = data.terrainLayers;
            int alphamaps = data.alphamapTextureCount;
            Vector3 size = data.size;

            // Force identity - this gets applied to tc.xy in the META pass before splat sampling,
            // we don't want any unexpected transform on our raw 0..1 quad UVs.
            mpb.SetVector(_MainTex_ST, new Vector4(1, 1, 0, 0));

            for (int i = 0; i < alphamaps; i++)
            {
                int controlId = i == 0 ? _Control : Shader.PropertyToID($"_Control{i}");
                mpb.SetTexture(controlId, data.GetAlphamapTexture(i));

                for (int s = 0; s < 4; s++)
                {
                    int layerIndex = i * 4 + s;
                    if (layerIndex >= layers.Length) continue;

                    var layer = layers[layerIndex];
                    mpb.SetTexture(_Splat[s], layer.diffuseTexture);

                    Vector2 tileSize = layer.tileSize;
                    if (tileSize.x <= 0f) tileSize.x = 0.001f;
                    if (tileSize.y <= 0f) tileSize.y = 0.001f;

                    var st = new Vector4(
                        size.x / tileSize.x,
                        size.z / tileSize.y,
                        layer.tileOffset.x / tileSize.x,
                        layer.tileOffset.y / tileSize.y);

                    mpb.SetVector(_SplatST[s], st);
                }
            }

            return mpb;
        }

        public AsyncGPUReadbackRequest CreateAtlas(Terrain terrain, MetaTexture.AtlasType type)
        {
            var material = terrain.materialTemplate;
            int meta = material != null ? material.FindPass("META") : -1;

            using var cmd = new CommandBuffer();
            cmd.SetRenderTarget(_rt);
            cmd.ClearRenderTarget(true, true, type == MetaTexture.AtlasType.Albedo ? Color.gray : Color.black);

            Matrix4x4 proj = Matrix4x4.Ortho(0, 1, 0, 1, 0.01f, 100f);
            Matrix4x4 view = Matrix4x4.LookAt(new Vector3(0, 0, -10), Vector3.zero, Vector3.up);
            cmd.SetViewProjectionMatrices(view, proj);

            cmd.SetGlobalVector("unity_MetaVertexControl", new Vector4(1, 0, 0, 0));
            cmd.SetGlobalFloat("unity_OneOverOutputBoost", 1.0f);
            cmd.SetGlobalFloat("unity_UseLinearSpace", 1.0f);
            cmd.SetGlobalFloat("unity_VisualizationMode", -1);

            var scaleOffset = new Vector4(1f, 1f, 0f, 0f);
            bool flipY = !SystemInfo.graphicsUVStartsAtTop;
            if (flipY)
            {
                scaleOffset.y = -scaleOffset.y;
                scaleOffset.w = 1.0f - scaleOffset.w;
            }

            cmd.SetGlobalVector("unity_LightmapST", scaleOffset);

            if (type == MetaTexture.AtlasType.Albedo)
            {
                cmd.SetGlobalVector("unity_MetaFragmentControl", new Vector4(1, 0, 0, 0));
                cmd.SetGlobalFloat("unity_MaxOutputValue", 1.0f);
            }
            else
            {
                cmd.SetGlobalVector("unity_MetaFragmentControl", new Vector4(0, 1, 0, 0));
                cmd.SetGlobalFloat("unity_MaxOutputValue", 100.0f);
            }

            if (meta >= 0)
            {
                var mpb = BuildPropertyBlock(terrain);
                cmd.DrawMesh(QuadMesh, Matrix4x4.identity, material, 0, meta, mpb);
            }

            Graphics.ExecuteCommandBuffer(cmd);

            var format = type == MetaTexture.AtlasType.Albedo ? TextureFormat.RGBA32 : TextureFormat.RGBAFloat;
            return AsyncGPUReadback.Request(_rt, 0, format);
        }

        public void Dispose()
        {
            if (_rt) UnityEngine.Object.DestroyImmediate(_rt);
        }
    }
}