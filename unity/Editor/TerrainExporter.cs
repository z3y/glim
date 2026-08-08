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
        public static Mesh GenerateMesh(TerrainData data, Vector3 position, int step = 2)
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
                    vertices[idx] = new Vector3(worldX, worldY, worldZ) + position;
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
            // mesh.RecalculateBounds();
            return mesh;
        }
    }

    public class TerrainMetaTexture : IDisposable
    {
        RenderTexture _rt;
        static Material _metaMat;
        static Mesh _quad;
        static Mesh QuadMesh => _quad ??= BuildQuad();

        static readonly int _MainTex = Shader.PropertyToID("_MainTex");
        static readonly int _MainTexST = Shader.PropertyToID("_MainTex_ST");
        static readonly int _Splat = Shader.PropertyToID("_Splat");
        static readonly int _SplatChannel = Shader.PropertyToID("_SplatChannel");

        public TerrainMetaTexture(int resolution)
        {
            var desc = new RenderTextureDescriptor
            {
                autoGenerateMips = false,
                width = resolution,
                height = resolution,
                useMipMap = false,
                mipCount = 1,
                colorFormat = RenderTextureFormat.ARGB32,
                sRGB = false,
                volumeDepth = 1,
                msaaSamples = 1,
                dimension = TextureDimension.Tex2D
            };
            _rt = new RenderTexture(desc) { filterMode = FilterMode.Point };

            if (_metaMat == null)
            {
                var shader = Shader.Find("Hidden/Glim/TerrainMeta");
                if (shader == null)
                {
                    throw new Exception("Hidden/Glim/TerrainMeta shader not found.");
                }
                _metaMat = new Material(shader) { hideFlags = HideFlags.HideAndDontSave };
            }
        }

        static Mesh BuildQuad()
        {
            var mesh = new Mesh
            {
                name = "GlimTerrainMetaQuad",
                hideFlags = HideFlags.HideAndDontSave,
                vertices = new[]
                {
                    new Vector3(0, 0, 0), new Vector3(1, 0, 0),
                    new Vector3(0, 1, 0), new Vector3(1, 1, 0),
                },
                uv = new[]
                {
                    new Vector2(0, 0), new Vector2(1, 0),
                    new Vector2(0, 1), new Vector2(1, 1),
                },
                triangles = new[] { 0, 2, 1, 1, 2, 3 }
            };
            mesh.RecalculateBounds();
            return mesh;
        }

        public Color32[] CreateAtlas(Terrain terrain)
        {
            var data = terrain.terrainData;
            var layers = data.terrainLayers;
            Vector3 size = data.size;

            using (var cmd = new CommandBuffer())
            {
                cmd.SetRenderTarget(_rt);
                cmd.ClearRenderTarget(true, true, Color.clear);

                for (int layerIndex = 0; layerIndex < layers.Length; layerIndex++)
                {
                    var layer = layers[layerIndex];
                    if (layer.diffuseTexture == null) continue;

                    int alphamapIndex = layerIndex / 4;
                    int channel = layerIndex % 4;
                    if (alphamapIndex >= data.alphamapTextureCount) continue;

                    Vector2 tileSize = layer.tileSize;
                    if (tileSize.x <= 0f) tileSize.x = 0.001f;
                    if (tileSize.y <= 0f) tileSize.y = 0.001f;

                    var mpb = new MaterialPropertyBlock();
                    mpb.SetTexture(_MainTex, layer.diffuseTexture);
                    mpb.SetVector(_MainTexST, new Vector4(
                        size.x / tileSize.x,
                        size.z / tileSize.y,
                        layer.tileOffset.x / tileSize.x,
                        layer.tileOffset.y / tileSize.y));
                    mpb.SetTexture(_Splat, data.GetAlphamapTexture(alphamapIndex));
                    mpb.SetInt(_SplatChannel, channel);

                    cmd.DrawMesh(QuadMesh, Matrix4x4.identity, _metaMat, 0, 0, mpb);
                }

                Graphics.ExecuteCommandBuffer(cmd);
            }

            var request = AsyncGPUReadback.Request(_rt, 0, TextureFormat.RGBA32);
            request.WaitForCompletion();
            return request.GetData<Color32>().ToArray();
        }

        public void Dispose()
        {
            if (_rt) UnityEngine.Object.DestroyImmediate(_rt);
        }
    }
}