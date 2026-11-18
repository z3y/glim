#if UNITY_EDITOR
using UnityEngine;

namespace Glim
{
    public enum GlimDoubleSidedGI
    {
        [Tooltip("Use the material Double Sided Global Illumination setting")]
        Material = 0,
        On = 1,
        Off = 2,
    }

    // Per renderer bake settings, only used by the bake. Renderers without this component use the defaults below.
    [DisallowMultipleComponent]
    [RequireComponent(typeof(MeshRenderer))]
    public class GlimRendererSettings : MonoBehaviour
    {
        [Tooltip(
@"Back faces receive and bounce light instead of being treated as black.
Material uses the Double Sided Global Illumination setting of each material.")]
        public GlimDoubleSidedGI doubleSidedGI = GlimDoubleSidedGI.Material;

        [Tooltip(
@"Scales the baked emission of this renderer, on top of the global emissive multiplier.
Only affects materials with baked emission. 0 removes it as a light source.")]
        [Min(0.0f)] public float emissionMultiplier = 1.0f;

        [Tooltip(
@"How much light passes through the opaque part of alpha blended (Transparent) materials, tinted by their albedo.
0: shadows are only reduced by the material alpha (1 - alpha).
1: the covered part transmits the full albedo color, giving colored shadows like tinted glass.
Alpha tested (Cutout) and opaque materials are not affected.")]
        [Range(0.0f, 1.0f)] public float transmission = 0.0f;

        void Start() { }
    }
}
#endif
