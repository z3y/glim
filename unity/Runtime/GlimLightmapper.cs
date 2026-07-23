#if UNITY_EDITOR
using System;
using System.IO;
using System.Linq;
using UnityEditor;
using UnityEngine;
using UnityEngine.SceneManagement;

namespace Glim
{
    public enum LightFalloffType : uint
    {
        Auto = 0,
        InverseSquare = 1,
        UnityBuiltIn = 2,
    }

    public enum LightmapMode : uint
    {
        NonDirectional = 0,
        DominantDirection = 1,
        CombinedSH = 3,
    }

    public class GlimLightmapper : MonoBehaviour
    {
        [Header("Bake Settings")]

        [Tooltip(
@"Lightmap Modes

• Non-Directional
  Bakes a single diffuse lightmap texture.

• Dominant Direction
  Bakes an additional directional lightmap that stores the dominant incoming light direction.
  Supports normal maps and improves directional lighting.

• Combined SH
  Bakes two textures:
  - L0: L0 Spherical Harmonics
  - L1: Monochromatic luminance encoded into L1 Spherical Harmonics.
  Produces higher-quality directional lighting than Dominant Direction, but requires a shader that supports it."
)]
        public LightmapMode lightmapMode = LightmapMode.NonDirectional;

        public LightFalloffType lightFalloff = LightFalloffType.Auto;
        [Tooltip("Enables multiple importance sampling (MIS) for emissive meshes, reducing direct light noise by combining light sampling and BSDF sampling, at the cost of slightly longer bake times.")]
        public bool multipleImportanceSampling = false;
        public uint directSamples = 512;
        public uint indirectSamples = 256;
        public uint bounces = 5;
        [Range(0.0f, 5.0f)] public float indirectMultiplier = 1.0f;

        [Space]
        public uint lightProbeSamples = 4096;
        [Tooltip("Offset each light probe sample in random direction by defined radius in meters, instead of baking it at its exact world position to prevent accidental intersections with geometry. Keep this range low < 0.1 ")]
        public float lightProbeRadius = 0.0f;
        [Tooltip("Applies Lanczos windowing function to light probes to reduce ringing")]
        public bool lightProbeDeringing = false;
        [Range(0.0f, 1.0f)] public float deringingIntensity = 0.5f;

        [Space]
        [Tooltip("Automatically bake reflection probes too after the bake is complete")]
        public bool bakeReflectionProbes = true;
        [Tooltip("Temporarly increases reflection probe resolution by 2x and downsamples on the imported cubemap")]
        public bool reflectionProbesSuperSampling = false;
        [Tooltip("Creates a mesh for each light visible in reflection probes, based on the shadow radius, area size or directional angle")]
        public bool reflectionProbesSpecular = false; // todo URP Shader

        [Header("Preview Settings")]
        public uint previewWidth = 1024;
        public uint previewHeight = 1024;
        public uint previewThrottle = 2;
        public uint previewSamples = 512;
        public uint previewBounces = 2;

        [Header("Default Group")]
        public LightmapGroup group;

        [MenuItem("Glim/Bake")]
        public static void CreateLightmapBaker()
        {
            var scene = SceneManager.GetActiveScene();
            var roots = scene.GetRootGameObjects();

            var baker = roots.SelectMany(x => x.GetComponentsInChildren<GlimLightmapper>()).FirstOrDefault();
            if (!baker)
            {
                var go = new GameObject("Glim Baker")
                {
                    tag = "EditorOnly"
                };

                go.transform.SetSiblingIndex(0);

                baker = go.AddComponent<GlimLightmapper>();

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
