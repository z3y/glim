#if UNITY_EDITOR
using UnityEngine;

namespace Glim
{
    // Per renderer bake settings. Renderers without this component use the defaults below.
    [DisallowMultipleComponent]
    [RequireComponent(typeof(MeshRenderer))]
    public class GlimRendererSettings : MonoBehaviour
    {
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
