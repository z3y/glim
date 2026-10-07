using UnityEngine;

#if UNITY_EDITOR
using System.Linq;
using UnityEditor;
#endif

namespace Glim
{
    [DisallowMultipleComponent]
    [RequireComponent(typeof(Light))]
    public class GlimAdditionalLightSettings : MonoBehaviour
    {
#if UNITY_EDITOR

        void OnDrawGizmosSelected()
        {
            var light = GetComponent<Light>();
            if (light && light.type == LightType.Spot)
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
        }

        public override void OnInspectorGUI()
        {
            if (_light == null)
            {
                return;
            }

            _light.Update();

            bool isSpot = _type != null && (_type.hasMultipleDifferentValues || _type.intValue == 0);
            if (isSpot)
            {
                DrawSpotAngles();
            }

            _light.ApplyModifiedProperties();
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