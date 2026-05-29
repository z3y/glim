#if UNITY_EDITOR
using UnityEditor;
using UnityEngine;

namespace stilb
{
    public enum LightFalloffType
    {
        Auto = 0,
        InverseSquare = 1,
        UnityBuiltIn = 2,
    }

    public class LightmapBaker : MonoBehaviour
    {

        public enum TextureSamplerFilter : uint
        {
            Nearest = 0,
            Linear = 1,
        }

        public TextureSamplerFilter filter = TextureSamplerFilter.Nearest;

        [Header("Bake Settings")]
        public uint directSamples = 512;
        public uint indirectSamples = 1024;
        public uint lightProbeSamples = 4096;
        public uint bounces = 5;

        [Header("Preview Settings")]
        public uint previewWidth = 1024;
        public uint previewHeight = 1024;
        public uint previewThrottle = 2;

        [Header("Bake Settings")]
        public LightmapGroup globalGroup;
        public LightFalloffType lightFalloff = LightFalloffType.Auto;
    }
}
#endif