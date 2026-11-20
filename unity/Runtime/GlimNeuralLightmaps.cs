using System;
using UnityEngine;

namespace Glim
{
    // Binds the neural lightmap textures to every lightmapped renderer with a MaterialPropertyBlock,
    // Unity has no slots for them. Created and filled by the bake.
    [ExecuteAlways]
    public class GlimNeuralLightmaps : MonoBehaviour
    {
        [Serializable]
        public class Group
        {
            public Texture2D[] features = new Texture2D[4];
        }

        public Texture2D weights;
        public Group[] groups = Array.Empty<Group>();

        static readonly int WeightsID = Shader.PropertyToID("_GlimNeuralWeights");
        static readonly int[] FeatureIDs =
        {
            Shader.PropertyToID("_GlimNeural0"),
            Shader.PropertyToID("_GlimNeural1"),
            Shader.PropertyToID("_GlimNeural2"),
            Shader.PropertyToID("_GlimNeural3"),
        };

        void OnEnable() => Apply();

        void Start() => Apply();

        [ContextMenu("Apply")]
        public void Apply()
        {
            if (!weights)
            {
                return;
            }

            var block = new MaterialPropertyBlock();
#pragma warning disable CS0618
            var renderers = FindObjectsOfType<Renderer>();
#pragma warning restore CS0618
            foreach (var renderer in renderers)
            {
                int index = renderer.lightmapIndex;
                if (index < 0 || index >= groups.Length || groups[index] == null)
                {
                    continue;
                }

                renderer.GetPropertyBlock(block);
                block.SetTexture(WeightsID, weights);
                var features = groups[index].features;
                for (int i = 0; i < FeatureIDs.Length && i < features.Length; i++)
                {
                    if (features[i])
                    {
                        block.SetTexture(FeatureIDs[i], features[i]);
                    }
                }
                renderer.SetPropertyBlock(block);
            }
        }
    }
}
