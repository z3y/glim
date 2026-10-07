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
        LegacyBIRP = 2,
    }

    public enum LightmapMode : uint
    {
        NonDirectional = 0,
        DominantDirection = 1,
        MonoSH = 3,
    }

    public enum MixedLightMode : uint
    {
        BakedIndirect = 0,
        // Subtractive = 1, // TODO
        // Shadowmask = 2, // TODO
    }

    public class GlimLightmapper : MonoBehaviour
    {
        [Header("Bake Settings")]

        [Tooltip(
@"- Non-Directional
Bakes a single diffuse lightmap texture.

- Dominant Direction
Bakes an additional directional lightmap that stores the dominant incoming light direction. Supports normal maps and improves directional lighting.

- Mono SH
Bakes two textures (L0 and Monochromatic luminance of L1). Produces higher-quality directional lighting than Dominant Direction, but requires a shader that supports it."
)]
        public LightmapMode lightmapMode = LightmapMode.NonDirectional;
        public MixedLightMode mixedMode = MixedLightMode.BakedIndirect;

        [Tooltip(
@"Distance falloff mode for point and spot lights. Directional lights have no falloff and are unaffected.

When using mixed lights this falloff should match the real time light for the current render pipeline (Auto), otherwise feel free to modify it.

- Auto
Picks the mode from the active render pipeline: InverseSquare when a render pipeline asset (URP) is assigned, LegacyBIRP when using the Built-In pipeline.

- Inverse Square
Physically based 1 / d² falloff, as used by URP. Intensity drops with the square of distance, and Range only acts as a smooth cutoff near the edge, so it doesn't change how bright the light looks.

- Legacy BIRP
The Built-In pipeline's older falloff: a hyperbolic curve of (distance / Range)², faded linearly to zero over the last stretch before Range. Range scales the whole curve, so a larger Range makes the light noticeably brighter than with inverse-square."
)]
        public LightFalloffType lightFalloff = LightFalloffType.Auto;

        [Tooltip(
@"Use fast hardware accelerated ray tracing if the GPU supports it (Vulkan RayQueries).
Automatically fallbacks to software CWBVH ray tracing when not avaliable.")]
        public bool hardwareRayTracing = true;

        [Tooltip(
@"Enables multiple importance sampling (MIS) for emissive meshes,
reducing direct light noise by combining light sampling and BSDF sampling, at the cost of slightly longer bake times.
Affects lightmaps, light probes and light volumes.")]
        public bool multipleImportanceSampling = false;

        [Tooltip("Number of direct samples, affects direct light from point, spot and directional lights")]
        public uint directLightSamples = 64;

        [Tooltip("Number of direct samples, affects emissive materials, area lights, multiple importance sampling and skybox")]
        public uint directEmissionSamples = 512;

        [Tooltip("Only affects bounced light")]
        public uint indirectSamples = 256;
        public uint bounces = 5;


        [Space]
        [Range(0.0f, 5.0f)] public float indirectMultiplier = 1.0f;
        [Range(0.0f, 1.0f)] public float ambientOcclusion = 0.0f;
        public float ambientOcclusionRange = 1.0f;
        public float emissiveMultiplier = 1.0f;

        [Space]
        public uint lightProbeSamples = 4096;
        [Tooltip(
@"Jitters light probe samples by defined radius in meters.
Probe positions are also moved outside of geometry defined by this radius.
Automatically applied to light volumes based on texel size."
)]
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
        public GlimLightmapGroup group;

        [Tooltip("(Experimental)\nHashes mesh data in order to skip uv packing if no changes are detected.")]
        public bool enableUVCache = false;

        [MenuItem("Tools/Glim/Bake")]
        public static void CreateLightmapBaker()
        {
            var scene = SceneManager.GetActiveScene();
            var roots = scene.GetRootGameObjects();

            var baker = roots.SelectMany(x => x.GetComponentsInChildren<GlimLightmapper>()).FirstOrDefault();
            if (!baker)
            {
                var go = new GameObject("Glim Lightmapper")
                {
                    tag = "EditorOnly"
                };

                go.transform.SetSiblingIndex(0);

                baker = go.AddComponent<GlimLightmapper>();

                var group = ScriptableObject.CreateInstance<GlimLightmapGroup>();
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
