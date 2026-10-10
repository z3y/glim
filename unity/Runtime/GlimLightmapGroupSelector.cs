#if UNITY_EDITOR
using System.IO;
using UnityEditor;
using UnityEngine;

namespace Glim
{
    public class GlimLightmapGroupSelector : MonoBehaviour
    {
        public GlimLightmapGroup group;

        void Start() { }
    }

    [CustomEditor(typeof(GlimLightmapGroupSelector))]
    public class GlimLightmapGroupSelectorEditor : Editor
    {
        SerializedProperty groupProp;
        Editor groupEditor;
        bool expanded = true;

        void OnEnable()
        {
            groupProp = serializedObject.FindProperty("group");
        }

        void OnDisable()
        {
            if (groupEditor != null)
                DestroyImmediate(groupEditor);
        }

        public override void OnInspectorGUI()
        {
            serializedObject.Update();

            EditorGUILayout.BeginHorizontal();
            EditorGUILayout.PropertyField(groupProp);
            if (GUILayout.Button("New", GUILayout.Width(50)))
            {
                var created = CreateGroupInSceneFolder();
                if (created != null)
                    groupProp.objectReferenceValue = created;
            }
            EditorGUILayout.EndHorizontal();

            serializedObject.ApplyModifiedProperties();

            var group = groupProp.objectReferenceValue;
            if (group == null)
            {
                EditorGUILayout.HelpBox("No group assigned. Assign one or press New.", MessageType.Info);
                return;
            }

            expanded = EditorGUILayout.InspectorTitlebar(expanded, group);
            if (expanded)
            {
                CreateCachedEditor(group, null, ref groupEditor);

                EditorGUILayout.BeginVertical(EditorStyles.helpBox);
                groupEditor.OnInspectorGUI();
                EditorGUILayout.EndVertical();
            }
        }

        GlimLightmapGroup CreateGroupInSceneFolder()
        {
            var go = ((Component)target).gameObject;
            var scene = go.scene;

            string folder = "Assets";
            string name = go.name;

            if (!string.IsNullOrEmpty(scene.path))
            {
                folder = Path.GetDirectoryName(scene.path).Replace('\\', '/');
            }
            else
            {
                Debug.LogWarning("Scene hasn't been saved yet, creating the group in Assets/ instead.");
            }

            string path = AssetDatabase.GenerateUniqueAssetPath($"{folder}/{name}_LightmapGroup.asset");

            var asset = CreateInstance<GlimLightmapGroup>();
            AssetDatabase.CreateAsset(asset, path);
            AssetDatabase.SaveAssets();

            EditorGUIUtility.PingObject(asset);
            return asset;
        }
    }
}
#endif
