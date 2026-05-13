using UnityEditor;
using UnityEditor.SceneManagement;
using UnityEditor.SearchService;
using UnityEditor.UIElements;
using UnityEngine;
using UnityEngine.UIElements;

namespace stilb
{
    [CustomEditor(typeof(LightmapBaker))]
    public class LightmapBakerEditor : Editor
    {
        Bindings.StilbConfig _config;
        Bindings.LightmapSettings _previewSettings;

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
                var config = new Bindings.StilbConfig
                {
                    is_preview = false,
                    coordinate_system = Bindings.CoordinateSystem.Unity,
                };
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


            _config = new Bindings.StilbConfig
            {
                coordinate_system = Bindings.CoordinateSystem.Unity,
                is_preview = true,
                preview_settings = _previewSettings,
                throttle_preview_ms = 10,
            };

            _previewSettings = new Bindings.LightmapSettings
            {
                width = 1024,
                height = 1024,
                max_samples = 512,
                bounce_count = 3,
            };

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
                _config.camera_position = camera.transform.position;
                _config.camera_forward = camera.transform.forward;
                _config.preview_settings = _previewSettings;
                Bake.Start(baker, _config);
            };

            var width = new UnsignedIntegerField("Width") { value = _previewSettings.width };
            width.RegisterValueChangedCallback(evt => _previewSettings.width = evt.newValue);
            root.Add(width);

            var height = new UnsignedIntegerField("Height") { value = _previewSettings.height };
            height.RegisterValueChangedCallback(evt => _previewSettings.height = evt.newValue);
            root.Add(height);

            var maxSamples = new UnsignedIntegerField("Max Samples") { value = _previewSettings.max_samples };
            maxSamples.RegisterValueChangedCallback(evt => _previewSettings.max_samples = evt.newValue);
            root.Add(maxSamples);

            var bounceCount = new UnsignedIntegerField("Bounces") { value = _previewSettings.bounce_count };
            bounceCount.RegisterValueChangedCallback(evt => _previewSettings.bounce_count = evt.newValue);
            root.Add(bounceCount);

            var throttle = new UnsignedIntegerField("Throttle Preview (ms)") { value = _config.throttle_preview_ms };
            throttle.RegisterValueChangedCallback(evt => _config.throttle_preview_ms = evt.newValue);
            root.Add(throttle);

            root.Add(startPreviewButton);

            return root;
        }
    }
}