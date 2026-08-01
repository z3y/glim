using System;
using System.Collections.Generic;
using UnityEngine;


namespace Glim
{
    public class MeshHash
    {
        public static uint FromLightmapUV(IEnumerable<MeshRenderer> renderers, uint resolution)
        {
            var uvs = new List<Vector2>();

            uint hash = 2166136261;
            uint prime = 16777619;

            hash ^= resolution;
            hash *= prime;

            foreach (var r in renderers)
            {
                var mesh = r.GetComponent<MeshFilter>().sharedMesh;

                int channel = mesh.HasVertexAttribute(UnityEngine.Rendering.VertexAttribute.TexCoord1) ? 1 : 0;

                uvs.Clear();
                mesh.GetUVs(channel, uvs);


                hash ^= (uint)mesh.vertexCount;
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
            }

            return hash;
        }

    }

}
