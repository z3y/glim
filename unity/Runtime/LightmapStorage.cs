#if UNITY_EDITOR
using UnityEditor;
using UnityEditor.SceneManagement;
using UnityEngine;
using UnityEngine.SceneManagement;

namespace stilb
{
    [CreateAssetMenu]
    public class LightmapStorage : ScriptableObject
    {
        [System.Serializable]
        public struct RendererInfo
        {
            public int lightmapIndex;
            public Vector4 lightmapScaleOffset;
            public string gameObjectPath;
        }

        public LightmapData[] lightmapDatas;
        public LightmapsMode lightmapsMode;
        public RendererInfo[] renderers;

        public void ApplyLightmaps()
        {
            var scene = SceneManager.GetActiveScene();
            LightmapSettings.lightmaps = lightmapDatas;
            LightmapSettings.lightmapsMode = lightmapsMode;

            foreach (var info in renderers)
            {
                var go = GameObject.Find(info.gameObjectPath);
                if (go == null) continue;

                var mr = go.GetComponent<MeshRenderer>();
                if (mr == null) continue;

                mr.lightmapIndex = info.lightmapIndex;
                mr.lightmapScaleOffset = info.lightmapScaleOffset;
                EditorUtility.SetDirty(mr);
            }

            EditorSceneManager.MarkSceneDirty(scene);
        }
    }
}
#endif