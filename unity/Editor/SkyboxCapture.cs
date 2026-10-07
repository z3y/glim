using UnityEngine;
using UnityEngine.Experimental.Rendering;
using UnityEngine.SceneManagement;

namespace Glim
{
    public static class SkyboxCapture
    {
        const GraphicsFormat FORMAT = GraphicsFormat.R16G16B16A16_SFloat;

        public static Color[] Capture(Scene scene)
        {
            var resolution = RenderSettings.defaultReflectionResolution;
            Debug.Log(resolution);

            var rtDesc = new RenderTextureDescriptor(resolution, resolution)
            {
                dimension = UnityEngine.Rendering.TextureDimension.Cube,
                graphicsFormat = FORMAT,
                depthBufferBits = 24,
                msaaSamples = 1
            };

            var rt = new RenderTexture(rtDesc);
            rt.Create();

            var cameraGO = new GameObject("Skybox Capture Camera");
            SceneManager.MoveGameObjectToScene(cameraGO, scene);

            var camera = cameraGO.AddComponent<Camera>();
            camera.enabled = false;
            camera.clearFlags = CameraClearFlags.Skybox;
            camera.cullingMask = 0;
            camera.backgroundColor = Color.black;
            camera.allowHDR = true;

            camera.RenderToCubemap(rt);

            Color[] pixels = new Color[resolution * resolution * 6];
            var face = new Texture2D(resolution, resolution, FORMAT, TextureCreationFlags.None);

            for (int faceIndex = 0; faceIndex < 6; faceIndex++)
            {
                Graphics.SetRenderTarget(rt, 0, (CubemapFace)faceIndex);

                face.ReadPixels(new Rect(0, 0, resolution, resolution), 0, 0);
                face.Apply(false);

                var colors = face.GetPixels();
                colors.CopyTo(pixels, faceIndex * resolution * resolution);
            }

            Graphics.SetRenderTarget(null);

            Object.DestroyImmediate(cameraGO);
            Object.DestroyImmediate(face);
            rt.Release();
            Object.DestroyImmediate(rt);

            return pixels;
        }
    }
}