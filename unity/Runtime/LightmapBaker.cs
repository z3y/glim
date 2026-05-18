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

        public TextureSamplerFilter filter = TextureSamplerFilter.Linear;

        [Header("Light Probes")]
        public uint probeSamples = 4096;
        public uint probeBounces = 5;


        [Header("Preview Settings")]
        public uint previewWidth = 1024;
        public uint previewHeight = 1024;
        public uint previewSamples = 512;
        public uint previewBounces = 3;
        public uint previewThrottle = 2;

        [Header("Bake Settings")]
        public LightmapGroup globalGroup;
        public LightFalloffType lightFalloff = LightFalloffType.Auto;
    }
}
#endif