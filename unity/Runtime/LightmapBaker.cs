#if UNITY_EDITOR
using System;
using System.IO;
using System.Linq;
using UnityEditor;
using UnityEngine;
using UnityEngine.SceneManagement;

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

        [NonSerialized] public TextureSamplerFilter filter = TextureSamplerFilter.Nearest;

        [Header("Bake Settings")]
        public uint directSamples = 512;
        public uint indirectSamples = 1024;
        public uint bounces = 5;
        public LightFalloffType lightFalloff = LightFalloffType.Auto;

        public uint lightProbeSamples = 4096;
        public float lightProbeRadius = 0.0f;

        [NonSerialized] public bool multipleImportanceSampling = false;
        public bool reflectionProbesSuperSampling = true;
        public bool reflectionProbesSpecular = false;


        [Header("Preview Settings")]
        public uint previewWidth = 1024;
        public uint previewHeight = 1024;
        public uint previewThrottle = 2;
        public uint previewSamples = 512;
        public uint previewBounces = 2;

        [Header("Default Group")]
        public LightmapGroup group;

        [MenuItem("Stilb/Bake")]
        public static void CreateLightmapBaker()
        {
            var scene = SceneManager.GetActiveScene();
            var roots = scene.GetRootGameObjects();

            var baker = roots.SelectMany(x => x.GetComponentsInChildren<LightmapBaker>()).FirstOrDefault();
            if (!baker)
            {
                var go = new GameObject("Stilb Lightmap Baker")
                {
                    tag = "EditorOnly"
                };

                go.transform.SetSiblingIndex(0);

                baker = go.AddComponent<LightmapBaker>();

                var group = ScriptableObject.CreateInstance<LightmapGroup>();
                baker.group = group;
                EditorUtility.SetDirty(baker);

                var scenePath = scene.path;
                string sceneName = scene.name;
                string outputFolder = Path.Combine(Path.GetDirectoryName(scenePath), sceneName);

                string assetPath = Path.Combine(outputFolder, $"{scene.name} Lightmap Group.asset");

                if (!AssetDatabase.IsValidFolder(outputFolder))
                {
                    AssetDatabase.CreateFolder(Path.GetDirectoryName(scenePath), sceneName);
                }


                AssetDatabase.CreateAsset(group, assetPath);
            }

            Selection.activeGameObject = baker.gameObject;
        }
    }


}
#endif