using System;
using System.Collections.Generic;
using UnityEngine;


namespace Glim
{
    public class MeshHash
    {
        public static uint FromLightmapUV(IList<MeshRenderer> renderers, IList<uint> hashData)
        {
            var uvs = new List<Vector2>();

            uint hash = 2166136261;
            uint prime = 16777619;

            foreach (var data in hashData)
            {
                hash ^= data;
                hash *= prime;
            }

            foreach (var r in renderers)
            {
                if (!r)
                {
                    continue;
                }

                var mf = r.GetComponent<MeshFilter>();

                if (!mf)
                {
                    continue;
                }

                var mesh = mf.sharedMesh;

                if (!mesh)
                {
                    continue;
                }

                int channel = mesh.HasVertexAttribute(UnityEngine.Rendering.VertexAttribute.TexCoord1) ? 1 : 0;

                uvs.Clear();
                mesh.GetUVs(channel, uvs);

                hash ^= (uint)mesh.vertexCount;
                hash *= prime;

                hash ^= (uint)mesh.subMeshCount;
                hash *= prime;

                var scale = r.transform.lossyScale;

                hash ^= (uint)BitConverter.SingleToInt32Bits(scale.x);
                hash *= prime;
                hash ^= (uint)BitConverter.SingleToInt32Bits(scale.y);
                hash *= prime;
                hash ^= (uint)BitConverter.SingleToInt32Bits(scale.z);
                hash *= prime;

                hash ^= (uint)BitConverter.SingleToInt32Bits(r.scaleInLightmap);
                hash *= prime;

                hash ^= (uint)channel;
                hash *= prime;

                for (int i = 0; i < uvs.Count; i++)
                {
                    hash ^= (uint)BitConverter.SingleToInt32Bits(uvs[i].x);
                    hash *= prime;

                    hash ^= (uint)BitConverter.SingleToInt32Bits(uvs[i].y);
                    hash *= prime;
                }

                var triangles = mesh.triangles;

                hash ^= (uint)triangles.Length;
                hash *= prime;

                for (int i = 0; i < triangles.Length; i++)
                {
                    hash ^= (uint)triangles[i];
                    hash *= prime;
                }
            }

            return hash;
        }

    }

}
