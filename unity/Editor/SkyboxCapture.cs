using System;
using System.IO;
using UnityEditor;
using UnityEngine;
using UnityEngine.Experimental.Rendering;
using UnityEngine.SceneManagement;
using Object = UnityEngine.Object;

namespace Glim
{
    public static class SkyboxCapture
    {
        const GraphicsFormat FORMAT = GraphicsFormat.R16G16B16A16_SFloat;

        public static Color[] Capture(Scene scene)
        {
            var resolution = RenderSettings.defaultReflectionResolution;

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

        public static void SaveAsCubemapAsset(Color[] pixels, string assetPath)
        {
            int res = (int)Mathf.Sqrt(pixels.Length / 6);

            var strip = new Texture2D(res * 6, res, FORMAT, TextureCreationFlags.None);
            var tmp = new Color[res * res];

            for (int f = 0; f < 6; f++)
            {
                int src = f;
                int srcOffset = src * res * res;

                for (int y = 0; y < res; y++)
                {
                    Array.Copy(pixels, srcOffset + (res - 1 - y) * res, tmp, y * res, res);
                }

                strip.SetPixels(f * res, 0, res, res, tmp);
            }
            // strip.Apply(false);

            File.WriteAllBytes(assetPath, strip.EncodeToEXR(Texture2D.EXRFlags.None));
            UnityEngine.Object.DestroyImmediate(strip);

            var metaPath = assetPath + ".meta";

            if (!File.Exists(metaPath))
            {
                var yaml = CreateSkyboxYaml();
                File.WriteAllText(metaPath, yaml);
            }

            AssetDatabase.ImportAsset(assetPath);
        }

        static string CreateSkyboxYaml()
        {
            var guid = GUID.Generate().ToString();

            return $@"fileFormatVersion: 2
guid: {guid}
TextureImporter:
  internalIDToNameTable: []
  externalObjects: {{}}
  serializedVersion: 13
  mipmaps:
    mipMapMode: 0
    enableMipMap: 1
    sRGBTexture: 0
    linearTexture: 0
    fadeOut: 0
    borderMipMap: 0
    mipMapsPreserveCoverage: 0
    alphaTestReferenceValue: 0.5
    mipMapFadeDistanceStart: 1
    mipMapFadeDistanceEnd: 3
  bumpmap:
    convertToNormalMap: 0
    externalNormalMap: 0
    heightScale: 0.25
    normalMapFilter: 0
    flipGreenChannel: 0
  isReadable: 0
  streamingMipmaps: 0
  streamingMipmapsPriority: 0
  vTOnly: 0
  ignoreMipmapLimit: 0
  grayScaleToAlpha: 0
  generateCubemap: 6
  cubemapConvolution: 1
  seamlessCubemap: 1
  textureFormat: 1
  maxTextureSize: 2048
  textureSettings:
    serializedVersion: 2
    filterMode: 2
    aniso: 0
    mipBias: 0
    wrapU: 1
    wrapV: 1
    wrapW: 1
  nPOTScale: 1
  lightmap: 0
  compressionQuality: 50
  spriteMode: 0
  spriteExtrude: 1
  spriteMeshType: 1
  alignment: 0
  spritePivot: {{x: 0.5, y: 0.5}}
  spritePixelsToUnits: 100
  spriteBorder: {{x: 0, y: 0, z: 0, w: 0}}
  spriteGenerateFallbackPhysicsShape: 1
  alphaUsage: 1
  alphaIsTransparency: 0
  spriteTessellationDetail: -1
  textureType: 0
  textureShape: 2
  singleChannelComponent: 0
  flipbookRows: 1
  flipbookColumns: 1
  maxTextureSizeSet: 0
  compressionQualitySet: 0
  textureFormatSet: 0
  ignorePngGamma: 0
  applyGammaDecoding: 0
  swizzle: 50462976
  cookieLightType: 0
  platformSettings:
  - serializedVersion: 4
    buildTarget: DefaultTexturePlatform
    maxTextureSize: 2048
    resizeAlgorithm: 0
    textureFormat: -1
    textureCompression: 1
    compressionQuality: 100
    crunchedCompression: 0
    allowsAlphaSplitting: 0
    overridden: 0
    ignorePlatformSupport: 0
    androidETC2FallbackOverride: 0
    forceMaximumCompressionQuality_BC6H_BC7: 0
  - serializedVersion: 4
    buildTarget: Standalone
    maxTextureSize: 2048
    resizeAlgorithm: 0
    textureFormat: -1
    textureCompression: 1
    compressionQuality: 50
    crunchedCompression: 0
    allowsAlphaSplitting: 0
    overridden: 0
    ignorePlatformSupport: 0
    androidETC2FallbackOverride: 0
    forceMaximumCompressionQuality_BC6H_BC7: 0
  spriteSheet:
    serializedVersion: 2
    sprites: []
    outline: []
    customData: 
    physicsShape: []
    bones: []
    spriteID: 
    internalID: 0
    vertices: []
    indices: 
    edges: []
    weights: []
    secondaryTextures: []
    spriteCustomMetadata:
      entries: []
    nameFileIdTable: {{}}
  mipmapLimitGroupName: 
  pSDRemoveMatte: 0
  userData: 
  assetBundleName: 
  assetBundleVariant: 
";
        }
    }
}