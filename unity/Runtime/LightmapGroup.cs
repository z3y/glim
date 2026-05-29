#if UNITY_EDITOR
using UnityEngine;

namespace stilb
{

    public enum LightmapSaveFormat : int
    {
        EXR = 0,
        Asset = 1,
    }

    [CreateAssetMenu]
    public class LightmapGroup : ScriptableObject
    {
        public uint resolution = 512;
        public bool dilate = true;
        public bool denoise = true;
        public bool fixSeams = true;
        public LightmapSaveFormat format = LightmapSaveFormat.EXR;
        public Texture2D.EXRFlags exrFlags = Texture2D.EXRFlags.OutputAsFloat | Texture2D.EXRFlags.CompressZIP;
    }
}
#endif