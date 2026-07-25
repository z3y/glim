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
}