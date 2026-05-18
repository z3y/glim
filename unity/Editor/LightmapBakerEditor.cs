using UnityEditor;
using UnityEditor.SceneManagement;
using UnityEditor.UIElements;
using UnityEngine;
using UnityEngine.UIElements;

namespace stilb
{
    [CustomEditor(typeof(LightmapBaker))]
    public class LightmapBakerEditor : Editor
    {
        public override VisualElement CreateInspectorGUI()
        {
            var root = new VisualElement();

            var baker = target as LightmapBaker;

            InspectorElement.FillDefaultInspector(root, serializedObject, this);


            if (baker.globalGroup)
            {
                // root.Add(new Label("<b>Global Settings</b>") { style = { marginTop = 20 } });

                VisualElement nestedInspector = new VisualElement();
                SerializedObject so = new(baker.globalGroup);
                InspectorElement.FillDefaultInspector(nestedInspector, so, this);
                nestedInspector.Bind(so);
                nestedInspector.Q<PropertyField>("PropertyField:m_Script").style.display = DisplayStyle.None;

                root.Add(nestedInspector);
            }

            Button startBakeButton = new()
            {
                text = "Bake",
                style =
                {
                    height = 25
                }
            };
            startBakeButton.clicked += () =>
            {
                var previewSettings = new Bindings.LightmapSettings();

                var config = new Bindings.StilbConfig(
                    Bindings.CoordinateSystem.Unity,
                    false,
                    0,
                    previewSettings,
                    Vector3.zero,
                    Vector3.zero,
                    (Bindings.TextureSamplerFilter)baker.filter,
                    baker.probeSamples,
                    baker.probeBounces,
                    baker.lightFalloff
                );
                Bake.Start(baker, config);
            };
            root.Add(startBakeButton);

            Button clearButton = new()
            {
                text = "Clear Lighting Data",
                style =
                {
                    height = 25
                }
            };
            clearButton.clicked += () =>
            {
                Lightmapping.lightingDataAsset = null;
                EditorSceneManager.MarkSceneDirty(EditorSceneManager.GetActiveScene());
            };
            root.Add(clearButton);


            root.Add(new Label("<b>Preview Settings</b>") { style = { marginTop = 20 } });

            Button startPreviewButton = new Button
            {
                text = "Open Preview",
                style =
                {
                    height = 25
                }
            };
            startPreviewButton.clicked += () =>
            {
                var camera = SceneView.lastActiveSceneView.camera;

                var previewSettings = new Bindings.LightmapSettings
                {
                    width = baker.previewWidth,
                    height = baker.previewHeight,
                    max_samples = baker.previewSamples,
                    bounce_count = baker.previewBounces,
                };

                var config = new Bindings.StilbConfig(
                    Bindings.CoordinateSystem.Unity,
                    true,
                    baker.previewThrottle,
                    previewSettings,
                    camera.transform.position,
                    camera.transform.forward,
                    (Bindings.TextureSamplerFilter)baker.filter,
                    baker.probeSamples,
                    baker.probeBounces,
                    baker.lightFalloff
                );

                Bake.Start(baker, config);
            };

            root.Add(startPreviewButton);

            return root;
        }
    }
}