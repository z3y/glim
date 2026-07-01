#if UNITY_EDITOR
using UnityEngine;

namespace stilb
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

    [CreateAssetMenu]
    public class LightmapGroup : ScriptableObject
    {
        public uint resolution = 512;
        public bool dilate = true;
        public bool denoise = true;
        public bool fixSeams = true;
        public UVPackingType uvPacking = UVPackingType.ScaleOffset;
        public bool packingBruteForce = false;
        [Range(5, 25)] public uint packingIterations = 5;
        public LightmapSaveFormat format = LightmapSaveFormat.EXR;
        public Texture2D.EXRFlags exrFlags = Texture2D.EXRFlags.OutputAsFloat | Texture2D.EXRFlags.CompressZIP;
    }
}
#endif