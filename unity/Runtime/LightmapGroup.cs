#if UNITY_EDITOR
using System;
using UnityEngine;

namespace Glim
{
    public enum LightmapSaveFormat : int
    {
        EXR = 0,
        Asset = 1,
    }

    public enum UVPackingType : int
    {
        None = 0,
        ScaleOffset = 1,
    }

    public enum Resolution : uint
    {
        _64 = 64,
        _128 = 128,
        _256 = 256,
        _512 = 512,
        _1024 = 1024,
        _2048 = 2048,
        _4096 = 4096,
        _8192 = 8192
    }

    [CreateAssetMenu(menuName = "Lightmap Group (Glim)")]
    public class LightmapGroup : ScriptableObject
    {
        public Resolution resolution = Resolution._2048;
        public uint Width => (uint)resolution;
        public uint Height => (uint)resolution;

        public UVPackingType packingType = UVPackingType.ScaleOffset;
        public bool bruteForce = false;

        [Tooltip("Scales up smaller charts to ensure there is enough padding, however the lightmap packing will fail if there is not enough resolution to satify all constraints, and resolution of other objects will decrease")]
        public bool ensurePadding = false;

        [Range(5, 25)] public uint packingIterations = 5;

        // public bool dilate = true;
        [NonSerialized] public bool dilate = false; // disabled for now
        public bool denoise = true;
        public bool fixSeams = true;
    }
}
#endif
