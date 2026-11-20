using UnityEngine;

#if UNITY_EDITOR
using System.Linq;
using UnityEditor;
#endif

namespace Glim
{
    public enum LightSpectrumPreset
    {
        Custom,
        Incandescent,
        Halogen,
        LowPressureSodium,
        HighPressureSodium,
        MercuryVapor,
        FluorescentCoolWhite,
        FluorescentTriBand,
        LedCoolWhite,
        LedWarmWhite,
    }

    // Light spectra for spectral rendering, relative spectral power over wavelength in nm.
    // Presets are approximations of typical lamps (lines and bands as gaussians), not measurements.
    public static class LightSpectra
    {
        // must match src/spectral.rs
        public const float LambdaMin = 360.0f;
        public const float LambdaMax = 830.0f;
        public const int SampleCount = 471; // 1 nm

        static float Gauss(float lambda, float mean, float sigma)
        {
            float t = (lambda - mean) / sigma;
            return Mathf.Exp(-0.5f * t * t);
        }

        static float Planck(float lambda, float kelvin)
        {
            const float c2 = 1.4387769e7f; // nm K
            return Mathf.Pow(560.0f / lambda, 5.0f) * (Mathf.Exp(c2 / (560.0f * kelvin)) - 1.0f) / (Mathf.Exp(c2 / (lambda * kelvin)) - 1.0f);
        }

        public static float Evaluate(LightSpectrumPreset preset, float l)
        {
            switch (preset)
            {
                case LightSpectrumPreset.Incandescent:
                    return Planck(l, 2856.0f); // CIE illuminant A
                case LightSpectrumPreset.Halogen:
                    return Planck(l, 3200.0f);
                case LightSpectrumPreset.LowPressureSodium:
                    // sodium D lines at 589.0 / 589.6 nm
                    return Gauss(l, 589.3f, 2.0f) + 0.01f * Gauss(l, 568.8f, 2.0f);
                case LightSpectrumPreset.HighPressureSodium:
                    // pressure broadened around the self reversed D lines, a few sodium and mercury lines
                    return Mathf.Max(0.0f, Gauss(l, 595.0f, 30.0f) - 0.85f * Gauss(l, 589.0f, 5.0f))
                        + 0.3f * Gauss(l, 568.8f, 3.0f) + 0.2f * Gauss(l, 498.0f, 3.0f) + 0.15f * Gauss(l, 466.0f, 3.0f);
                case LightSpectrumPreset.MercuryVapor:
                    return 0.35f * Gauss(l, 404.7f, 2.0f) + 0.65f * Gauss(l, 435.8f, 2.0f) + Gauss(l, 546.1f, 2.0f)
                        + 0.7f * Gauss(l, 578.0f, 3.0f) + 0.03f * Gauss(l, 560.0f, 80.0f);
                case LightSpectrumPreset.FluorescentCoolWhite:
                    // halophosphate band with mercury lines, similar to CIE F2
                    return 0.9f * Gauss(l, 580.0f, 48.0f) + 0.25f * Gauss(l, 404.7f, 2.0f) + 0.55f * Gauss(l, 435.8f, 2.0f)
                        + 0.6f * Gauss(l, 546.1f, 2.0f) + 0.25f * Gauss(l, 578.0f, 2.0f);
                case LightSpectrumPreset.FluorescentTriBand:
                    // rare earth phosphors, similar to CIE F11
                    return 0.55f * Gauss(l, 435.8f, 2.5f) + 0.2f * Gauss(l, 487.0f, 4.0f) + Gauss(l, 544.0f, 3.5f)
                        + 0.35f * Gauss(l, 588.0f, 4.0f) + 0.95f * Gauss(l, 611.0f, 3.0f) + 0.15f * Gauss(l, 631.0f, 4.0f)
                        + 0.08f * Gauss(l, 707.0f, 4.0f) + 0.04f * Gauss(l, 500.0f, 60.0f);
                case LightSpectrumPreset.LedCoolWhite:
                    // blue die and yellow phosphor
                    return Gauss(l, 450.0f, 10.0f) + 0.62f * Gauss(l, 560.0f, 50.0f);
                case LightSpectrumPreset.LedWarmWhite:
                    return 0.45f * Gauss(l, 450.0f, 10.0f) + Gauss(l, 605.0f, 55.0f);
                default:
                    return 1.0f;
            }
        }

        // curve with keys every 2 nm, peak normalized to 1
        public static AnimationCurve CreateCurve(LightSpectrumPreset preset)
        {
            const float step = 2.0f;
            int count = Mathf.FloorToInt((LambdaMax - LambdaMin) / step) + 1;

            var values = new float[count];
            float peak = 0.0f;
            for (int i = 0; i < count; i++)
            {
                values[i] = Mathf.Max(0.0f, Evaluate(preset, LambdaMin + i * step));
                peak = Mathf.Max(peak, values[i]);
            }

            var keys = new Keyframe[count];
            for (int i = 0; i < count; i++)
            {
                float v = peak > 0.0f ? values[i] / peak : 0.0f;
                float prev = peak > 0.0f ? values[Mathf.Max(i - 1, 0)] / peak : 0.0f;
                float next = peak > 0.0f ? values[Mathf.Min(i + 1, count - 1)] / peak : 0.0f;
                float inTangent = (v - prev) / step;
                float outTangent = (next - v) / step;
                keys[i] = new Keyframe(LambdaMin + i * step, v, inTangent, outTangent);
            }
            return new AnimationCurve(keys);
        }

        // samples evenly spaced over [LambdaMin, LambdaMax], what app_add_light_spectrum takes
        public static float[] Sample(AnimationCurve curve)
        {
            var samples = new float[SampleCount];
            for (int i = 0; i < SampleCount; i++)
            {
                float lambda = Mathf.Lerp(LambdaMin, LambdaMax, i / (float)(SampleCount - 1));
                samples[i] = Mathf.Max(0.0f, curve.Evaluate(lambda));
            }
            return samples;
        }

        // CIE 1931 fit of Wyman, Sloan and Shirley 2013, same as src/spectral.rs
        static Vector3 CieXyz(float l)
        {
            float G(float mu, float s1, float s2)
            {
                float t = (l - mu) / (l < mu ? s1 : s2);
                return Mathf.Exp(-0.5f * t * t);
            }
            float x = 1.056f * G(599.8f, 37.9f, 31.0f) + 0.362f * G(442.0f, 16.0f, 26.7f) - 0.065f * G(501.1f, 20.4f, 26.2f);
            float y = 0.821f * G(568.8f, 46.9f, 40.5f) + 0.286f * G(530.9f, 16.3f, 31.1f);
            float z = 1.217f * G(437.0f, 11.8f, 36.0f) + 0.681f * G(459.0f, 26.0f, 13.8f);
            return new Vector3(x, y, z);
        }

        static Vector3 CieRgb(float l)
        {
            Vector3 c = CieXyz(l);
            return new Vector3(
                3.2406255f * c.x - 1.5372080f * c.y - 0.4986286f * c.z,
                -0.9689307f * c.x + 1.8757561f * c.y + 0.0415175f * c.z,
                0.0557101f * c.x - 0.2040211f * c.y + 1.0569959f * c.z);
        }

        // CIE D65, 360 to 830 nm in 10 nm steps, same as src/spectral.rs
        static readonly float[] D65 =
        {
            46.6383f, 52.0891f, 49.9755f, 54.6482f, 82.7549f, 91.486f, 93.4318f, 86.6823f, 104.865f, 117.008f,
            117.812f, 114.861f, 115.923f, 108.811f, 109.354f, 107.802f, 104.79f, 107.689f, 104.405f, 104.046f, 100.0f,
            96.3342f, 95.788f, 88.6856f, 90.0062f, 89.5991f, 87.6987f, 83.2886f, 83.6992f, 80.0268f, 80.2146f,
            82.2778f, 78.2842f, 69.7213f, 71.6091f, 74.349f, 61.604f, 69.8856f, 75.087f, 63.5927f, 46.4182f, 66.8054f,
            63.3828f, 64.304f, 59.4519f, 51.959f, 57.4406f, 60.3125f,
        };

        static float D65At(float l)
        {
            float t = Mathf.Clamp((l - LambdaMin) / 10.0f, 0.0f, D65.Length - 1);
            int i = Mathf.Min((int)t, D65.Length - 2);
            return Mathf.Lerp(D65[i], D65[i + 1], t - i);
        }

        // Linear sRGB of a spectrum, white balanced so D65 is (1, 1, 1), luminance normalized to 1.
        // Matches the color the baker uses for the light's hue.
        public static Color LinearColor(float[] samples)
        {
            Vector3 rgb = Vector3.zero;
            Vector3 white = Vector3.zero;
            for (int i = 0; i < samples.Length; i++)
            {
                float lambda = Mathf.Lerp(LambdaMin, LambdaMax, i / (float)(samples.Length - 1));
                Vector3 cmf = CieRgb(lambda);
                rgb += samples[i] * cmf;
                white += D65At(lambda) * cmf;
            }
            rgb = new Vector3(rgb.x / white.x, rgb.y / white.y, rgb.z / white.z);

            float luminance = 0.2126f * rgb.x + 0.7152f * rgb.y + 0.0722f * rgb.z;
            if (!(luminance > 0.0f))
            {
                return Color.black;
            }
            rgb /= luminance;
            return new Color(Mathf.Max(rgb.x, 0.0f), Mathf.Max(rgb.y, 0.0f), Mathf.Max(rgb.z, 0.0f), 1.0f);
        }
    }

    [DisallowMultipleComponent]
    [RequireComponent(typeof(Light))]
    public class GlimAdditionalLightSettings : MonoBehaviour
    {
        public Texture2D areaCookie;

        [Tooltip("Bake this light with a spectral power distribution instead of an RGB color. The spectrum sets the hue, the light color and intensity only the brightness. Needs Spectral Rendering on the Glim Lightmapper for colors to interact spectrally.")]
        public bool useSpectrum = false;

        public LightSpectrumPreset spectrumPreset = LightSpectrumPreset.Incandescent;

        [Tooltip("Relative spectral power over wavelength in nm (360 - 830)")]
        public AnimationCurve spectrum = LightSpectra.CreateCurve(LightSpectrumPreset.Incandescent);

#if UNITY_EDITOR
        void OnDrawGizmosSelected()
        {
            var light = GetComponent<Light>();

            if (!light)
            {
                return;
            }

            if (light.type == LightType.Spot)
            {
                float range = light.range;
                float radius = range * Mathf.Tan(light.innerSpotAngle * 0.5f * Mathf.Deg2Rad);

                Gizmos.matrix = Matrix4x4.TRS(transform.position, transform.rotation, Vector3.one);
                Gizmos.color = new Color(1f, 0.9f, 0.3f, 1f);

                const int seg = 32;
                Vector3 prev = new Vector3(radius, 0f, range);
                for (int i = 1; i <= seg; i++)
                {
                    float a = i / (float)seg * Mathf.PI * 2f;
                    Vector3 next = new Vector3(Mathf.Cos(a) * radius, Mathf.Sin(a) * radius, range);
                    Gizmos.DrawLine(prev, next);
                    prev = next;
                }

                Gizmos.DrawLine(Vector3.zero, new Vector3(radius, 0f, range));
                Gizmos.DrawLine(Vector3.zero, new Vector3(-radius, 0f, range));
                Gizmos.DrawLine(Vector3.zero, new Vector3(0f, radius, range));
                Gizmos.DrawLine(Vector3.zero, new Vector3(0f, -radius, range));
            }

            if ((light.type == LightType.Spot || light.type == LightType.Point) && light.shadowRadius > 0.0f)
            {
                Gizmos.color = Color.gray;
                Gizmos.DrawWireSphere(transform.position, light.shadowRadius);
            }

        }
#endif
    }

#if UNITY_EDITOR

    [CanEditMultipleObjects]
    [CustomEditor(typeof(GlimAdditionalLightSettings))]
    public class GlimAdditionalLightSettingsEditor : Editor
    {
        SerializedObject _light;

        SerializedProperty _type;
        SerializedProperty _outerAngle;
        SerializedProperty _innerAngle;

        SerializedProperty _areaCookie;

        SerializedProperty _useSpectrum;
        SerializedProperty _spectrumPreset;
        SerializedProperty _spectrum;

        void OnEnable()
        {
            var lights = targets
                .Select(t => ((Component)t).GetComponent<Light>())
                .Where(l => l != null)
                .ToArray();

            if (lights.Length == 0) return;

            _light = new SerializedObject(lights);

            _type = _light.FindProperty("m_Type");
            _outerAngle = _light.FindProperty("m_SpotAngle");
            _innerAngle = _light.FindProperty("m_InnerSpotAngle");

            _areaCookie = serializedObject.FindProperty(nameof(GlimAdditionalLightSettings.areaCookie));

            _useSpectrum = serializedObject.FindProperty(nameof(GlimAdditionalLightSettings.useSpectrum));
            _spectrumPreset = serializedObject.FindProperty(nameof(GlimAdditionalLightSettings.spectrumPreset));
            _spectrum = serializedObject.FindProperty(nameof(GlimAdditionalLightSettings.spectrum));
        }

        public override void OnInspectorGUI()
        {
            if (_light == null)
            {
                return;
            }

            serializedObject.Update();
            _light.Update();

            if (!_type.hasMultipleDifferentValues)
            {
                if (_type.intValue == (int)LightType.Spot)
                {
                    DrawSpotAngles();
                }

                if (_type.intValue == (int)LightType.Rectangle || _type.intValue == (int)LightType.Disc)
                {
                    EditorGUILayout.PropertyField(_areaCookie, new GUIContent("Area Cookie"));
                }
            }

            DrawSpectrum();

            _light.ApplyModifiedProperties();
            serializedObject.ApplyModifiedProperties();
        }

        void DrawSpectrum()
        {
            EditorGUILayout.PropertyField(_useSpectrum, new GUIContent("Spectrum"));
            if (!_useSpectrum.boolValue && !_useSpectrum.hasMultipleDifferentValues)
            {
                return;
            }

            EditorGUI.indentLevel++;

            EditorGUI.BeginChangeCheck();
            EditorGUILayout.PropertyField(_spectrumPreset, new GUIContent("Preset"));
            if (EditorGUI.EndChangeCheck() && !_spectrumPreset.hasMultipleDifferentValues)
            {
                var preset = (LightSpectrumPreset)_spectrumPreset.enumValueIndex;
                if (preset != LightSpectrumPreset.Custom)
                {
                    serializedObject.ApplyModifiedProperties();
                    foreach (var t in targets.Cast<GlimAdditionalLightSettings>())
                    {
                        Undo.RecordObject(t, "Light Spectrum Preset");
                        t.spectrum = LightSpectra.CreateCurve(preset);
                        EditorUtility.SetDirty(t);
                    }
                    serializedObject.Update();
                }
            }

            var ranges = new Rect(LightSpectra.LambdaMin, 0.0f, LightSpectra.LambdaMax - LightSpectra.LambdaMin, 1.0f);
            EditorGUI.BeginChangeCheck();
            EditorGUILayout.CurveField(_spectrum, new Color(1.0f, 0.8f, 0.3f), ranges, new GUIContent("Power (nm)"), GUILayout.Height(60));
            if (EditorGUI.EndChangeCheck())
            {
                // hand edited, no longer the preset
                _spectrumPreset.enumValueIndex = (int)LightSpectrumPreset.Custom;
            }

            if (targets.Length == 1)
            {
                var settings = (GlimAdditionalLightSettings)target;
                if (settings.spectrum != null)
                {
                    Color linear = LightSpectra.LinearColor(LightSpectra.Sample(settings.spectrum));
                    float max = Mathf.Max(linear.r, Mathf.Max(linear.g, linear.b));
                    Color display = max > 0.0f ? (linear / max).gamma : Color.black;
                    display.a = 1.0f;

                    using (new EditorGUI.DisabledScope(true))
                    {
                        EditorGUILayout.ColorField(new GUIContent("Color", "sRGB color of the spectrum, the hue the light bakes with"), display, false, false, false);
                    }

                    if (GUILayout.Button(new GUIContent("Apply Color To Light", "Sets the Unity light color to the spectrum's color so realtime lighting matches the hue. Baked brightness follows the light color's luminance times intensity.")))
                    {
                        var light = settings.GetComponent<Light>();
                        Undo.RecordObject(light, "Apply Spectrum Color");
                        light.useColorTemperature = false;
                        light.color = display;
                        EditorUtility.SetDirty(light);
                    }
                }
            }

            EditorGUI.indentLevel--;
        }

        void DrawSpotAngles()
        {
            Rect row = EditorGUI.PrefixLabel(EditorGUILayout.GetControlRect(), new GUIContent("Inner / Outer Spot Angle"));

            int indent = EditorGUI.indentLevel;
            EditorGUI.indentLevel = 0;

            const float w = 40f, gap = 4f;
            var innerRect = new Rect(row.x, row.y, w, row.height);
            var outerRect = new Rect(row.xMax - w, row.y, w, row.height);
            var sliderRect = new Rect(innerRect.xMax + gap, row.y, outerRect.x - innerRect.xMax - gap * 2f, row.height);

            float inner = _innerAngle.floatValue;
            float outer = _outerAngle.floatValue;

            EditorGUI.BeginChangeCheck();
            inner = EditorGUI.FloatField(innerRect, inner);
            EditorGUI.MinMaxSlider(sliderRect, ref inner, ref outer, 1f, 179f);
            outer = EditorGUI.FloatField(outerRect, outer);
            if (EditorGUI.EndChangeCheck())
            {
                outer = Mathf.Clamp(outer, 1f, 179f);
                inner = Mathf.Clamp(inner, 1f, outer);
                _innerAngle.floatValue = inner;
                _outerAngle.floatValue = outer;
            }

            EditorGUI.indentLevel = indent;
        }

    }

#endif
}
