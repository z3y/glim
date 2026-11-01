using UnityEditor;
using UnityEngine;

namespace Glim
{
    public static class LightmapTexelGrid
    {
        static bool _enabled;

        public static bool IsEnabled => _enabled;

        public static void Toggle()
        {
            var shader = Shader.Find("Hidden/Glim/LightmapTexelGrid");
            if (shader == null)
            {
                Debug.LogError("Hidden/Glim/LightmapTexelGrid shader not found");
                return;
            }

            _enabled = !_enabled;

            foreach (SceneView sceneView in SceneView.sceneViews)
            {
                sceneView.SetSceneViewShaderReplace(_enabled ? shader : null, null);
            }

            SceneView.RepaintAll();
        }
    }
}
