using UnityEngine;
using UnityEngine.SceneManagement;
using UnityEngine.Experimental.Rendering;

namespace Glim
{
    public static class SkyboxCapture
    {
        public const int RESOLUTION = 128;

        public static Color[] Capture(Scene scene)
        {
            var cubemap = new Cubemap(
                RESOLUTION,
                TextureFormat.RGBAHalf,
                false,
                true
            );

            var cameraGO = new GameObject("Skybox Capture Camera");
            SceneManager.MoveGameObjectToScene(cameraGO, scene);

            var camera = cameraGO.AddComponent<Camera>();
            camera.enabled = false;
            camera.clearFlags = CameraClearFlags.Skybox;
            camera.cullingMask = 0;
            camera.backgroundColor = Color.black;
            camera.allowHDR = true;

            camera.RenderToCubemap(cubemap);

            Color[] pixels = new Color[RESOLUTION * RESOLUTION * 6];

            for (int faceIndex = 0; faceIndex < 6; faceIndex++)
            {
                var colors = cubemap.GetPixels((CubemapFace)faceIndex);
                colors.CopyTo(pixels, faceIndex * RESOLUTION * RESOLUTION);
            }

            Object.DestroyImmediate(cameraGO);
            Object.DestroyImmediate(cubemap);

            return pixels;
        }
    }
}