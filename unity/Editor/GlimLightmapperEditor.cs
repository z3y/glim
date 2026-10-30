using System;
using System.Collections.Generic;
using System.Linq;
using System.Reflection;
using UnityEditor;
using UnityEditor.SceneManagement;
using UnityEditor.UIElements;
using UnityEngine;
using UnityEngine.SceneManagement;
using UnityEngine.UIElements;

namespace Glim
{
    [CustomEditor(typeof(GlimLightmapper))]
    public class GlimLightmapperEditor : Editor
    {
        Editor _groupEditor;
        bool _groupExpanded = true;

        void OnDisable()
        {
            if (_groupEditor != null)
                DestroyImmediate(_groupEditor);
        }

        static Button CreateIconButton(string iconName, string text, string tooltip, int height)
        {
            Button button = new()
            {
                tooltip = tooltip,
                style =
                {
                    height = height,
                    flexDirection = FlexDirection.Row,
                    justifyContent = Justify.Center,
                    alignItems = Align.Center,
                    flexGrow = 1,
                    flexBasis = 0,
                }
            };

            Texture icon = EditorGUIUtility.IconContent(iconName).image;
            if (icon != null)
            {
                button.Add(new Image
                {
                    image = icon,
                    style = { width = 16, height = 16, marginRight = 4 }
                });
            }

            button.Add(new Label(text));
            return button;
        }

        public override VisualElement CreateInspectorGUI()
        {
            var root = new VisualElement();

            var lightmapper = target as GlimLightmapper;

            var horizontal = new VisualElement()
            {
                style =
                {
                    flexDirection = FlexDirection.Row,
                    flexGrow = 1,
                    marginBottom = 5,
                    marginTop = 5,
                }
            };
            var bakeButton = CreateIconButton("Lighting", "Bake", "Bake Lightmaps", 35);
            bakeButton.clicked += () =>
            {
                var previewSettings = new Bindings.LightmapSettings();

                float w = Mathf.Lerp(8.0f, 2.0f, lightmapper.deringingIntensity);
                if (!lightmapper.lightProbeDeringing || lightmapper.deringingIntensity <= 0.0f)
                {
                    w = 0.0f;
                }

                var config = new Bindings.GlimConfig(
                    Bindings.CoordinateSystem.Unity,
                    lightmapper.directLightSamples,
                    lightmapper.directEmissionSamples,
                    lightmapper.indirectSamples,
                    lightmapper.bounces,
                    false,
                    lightmapper.hardwareRayTracing,
                    0,
                    previewSettings,
                    Vector3.zero,
                    Vector3.zero,
                    lightmapper.lightProbeSamples,
                    lightmapper.lightFalloff,
                    lightmapper.multipleImportanceSampling,
                    lightmapper.lightmapMode,
                    RenderSettings.ambientIntensity,
                    lightmapper.indirectMultiplier,
                    w,
                    lightmapper.emissiveMultiplier,
                    lightmapper.ambientOcclusion,
                    lightmapper.ambientOcclusionRange,
                    lightmapper.coverageMaxSamples,
                    lightmapper.coverageMinThreshold,
                    lightmapper.antiAliasing
                );
                Bake.Start(lightmapper, config);
            };
            horizontal.Add(bakeButton);

            var previewButton = CreateIconButton("animationvisibilitytoggleon", "Preview", "Opens the path-traced preview window", 35);
            previewButton.clicked += () =>
            {
                var camera = SceneView.lastActiveSceneView.camera;

                var previewSettings = new Bindings.LightmapSettings(
                    lightmapper.previewWidth, lightmapper.previewHeight, false, false, false, false);

                var config = new Bindings.GlimConfig(
                    Bindings.CoordinateSystem.Unity,
                    lightmapper.previewSamples,
                    lightmapper.previewSamples,
                    lightmapper.previewSamples,
                    lightmapper.previewBounces,
                    true,
                    lightmapper.hardwareRayTracing,
                    lightmapper.previewThrottle,
                    previewSettings,
                    camera.transform.position,
                    camera.transform.forward,
                    lightmapper.lightProbeSamples,
                    lightmapper.lightFalloff,
                    lightmapper.multipleImportanceSampling,
                    lightmapper.lightmapMode,
                    RenderSettings.ambientIntensity,
                    lightmapper.indirectMultiplier,
                    0.0f,
                    lightmapper.emissiveMultiplier,
                    lightmapper.ambientOcclusion,
                    lightmapper.ambientOcclusionRange,
                    lightmapper.coverageMaxSamples,
                    lightmapper.coverageMinThreshold,
                    lightmapper.antiAliasing
                );

                Bake.Start(lightmapper, config);
            };
            horizontal.Add(previewButton);

            var bakeReflectionButton = CreateIconButton("ReflectionProbe Icon", "Bake Reflections", "Render Reflection Probes", 35);
            bakeReflectionButton.clicked += () =>
            {
                BakeAllReflectionProbesSnapshots(EditorSceneManager.GetActiveScene(), lightmapper.reflectionProbesSuperSampling ? 2 : 1, lightmapper.reflectionProbesSpecular);
            };
            horizontal.Add(bakeReflectionButton);

            var clearButton = CreateIconButton("TreeEditor.Trash", "Clear", "Clear baked lighting data", 35);
            clearButton.clicked += () =>
            {
                Lightmapping.lightingDataAsset = null;

                var scene = EditorSceneManager.GetActiveScene();
                var selectors = scene.GetRootGameObjects().SelectMany(x => x.GetComponentsInChildren<GlimLightmapGroupSelector>(true));

                foreach (var selector in selectors)
                {
                    if (selector.group == null)
                    {
                        continue;
                    }
                    selector.group.lightmapUVHash = 0;
                    EditorUtility.SetDirty(selector.group);
                }

                if (lightmapper.group)
                {
                    lightmapper.group.lightmapUVHash = 0;
                    EditorUtility.SetDirty(lightmapper.group);
                }

                EditorSceneManager.MarkSceneDirty(scene);
            };
            horizontal.Add(clearButton);
            root.Add(horizontal);

            ProgressBar progressBar = new()
            {
                title = "Baking",
                style =
                {
                    display = DisplayStyle.None,
                }
            };
            root.Add(progressBar);

            Button cancelButton = new()
            {
                text = "Cancel Bake",
                style =
                {
                    height = 30,
                    display = DisplayStyle.None,
                    backgroundColor = new Color(0.5f, 0.2f, 0.2f, 1.0f)
                }
            };
            cancelButton.clicked += () =>
            {
                if (EditorUtility.DisplayDialog("Cancel Bake",
                        "Are you sure you want to cancel the current bake?",
                        "Cancel Bake", "Keep Baking"))
                {
                    Bake.Cancel();
                }
            };
            root.Add(cancelButton);

            InspectorElement.FillDefaultInspector(root, serializedObject, this);
            root.Add(new IMGUIContainer(() =>
            {
                var group = lightmapper.group;
                if (group == null)
                    return;

                _groupExpanded = EditorGUILayout.InspectorTitlebar(_groupExpanded, group);
                if (_groupExpanded)
                {
                    CreateCachedEditor(group, null, ref _groupEditor);

                    EditorGUILayout.BeginVertical(EditorStyles.helpBox);
                    var so = _groupEditor.serializedObject;
                    so.Update();
                    DrawPropertiesExcluding(so, "m_Script");
                    so.ApplyModifiedProperties();
                    EditorGUILayout.EndVertical();
                }

                EditorGUILayout.Space();
            }));

            Label report = new()
            {
                style =
                    {
                        whiteSpace = WhiteSpace.Normal,
                        marginTop = 8
                    }
            };
            root.Add(report);

            void RefreshReport()
            {
                var last = Bake.LoadReport(lightmapper.gameObject.scene.path);
                report.style.display = last == null ? DisplayStyle.None : DisplayStyle.Flex;

                if (last == null)
                {
                    return;
                }

                var finishedAt = DateTime.Parse(last.finishedAt).ToString("HH:mm:ss");
                var took = TimeSpan.FromSeconds(last.bakeTime).ToString(@"hh\:mm\:ss");

                report.text =
                    $"Bake finished at {finishedAt} and took {took}\n" +
                    $"Lightmaps: {last.lightmapCount} ({EditorUtility.FormatBytes(last.lightmapBytes)} on disk, " +
                    $"{EditorUtility.FormatBytes(last.lightmapMemoryBytes)} compressed)\n" +
                    $"Lighting Data: {EditorUtility.FormatBytes(last.lightingDataBytes)}\n" +
                    $"Light Probes: {last.probeCount}";
            }

            RefreshReport();

            int seenReport = Bake.ReportVersion;
            progressBar.schedule.Execute(() =>
            {
                bool running = Bake.IsBaking;
                progressBar.style.display = running ? DisplayStyle.Flex : DisplayStyle.None;
                cancelButton.style.display = running ? DisplayStyle.Flex : DisplayStyle.None;
                bakeButton.SetEnabled(!running);
                previewButton.SetEnabled(!running);

                if (running)
                {
                    progressBar.value = Mathf.Clamp01(Bake.BakeProgress) * 100f;
                    progressBar.title = Bake.BakeMessage;

                    bool cancelling = Bake.IsCancelling;
                    cancelButton.SetEnabled(!cancelling);
                    cancelButton.text = cancelling ? "Cancelling…" : "Cancel Bake";
                }

                if (seenReport != Bake.ReportVersion)
                {
                    seenReport = Bake.ReportVersion;
                    RefreshReport();
                }
            }).Every(200);

            return root;
        }

        public static void BakeAllReflectionProbesSnapshots(Scene scene, int supersampling, bool specularProbes)
        {
            if (Lightmapping.lightingDataAsset == null)
            {
                Debug.LogError("Lightmaps not baked");
                return;
            }

            var root = scene.GetRootGameObjects();

            var probes = root.SelectMany(x => x.GetComponentsInChildren<ReflectionProbe>(false)).ToArray();

            var speculars = new List<GameObject>();
            if (specularProbes)
            {
                var lights = root.SelectMany(x => x.GetComponentsInChildren<Light>(true))
                    .Where(l => l.enabled && l.gameObject.activeInHierarchy)
                    .Distinct()
                    .ToArray();

                var lightMeshMat = AssetDatabase.LoadAssetAtPath<Material>("Packages/io.github.z3y.glim/Editor/LightMesh.mat");

                foreach (var l in lights)
                {
                    GameObject go = GameObject.CreatePrimitive(PrimitiveType.Cube);
                    go.name = "Light Mesh";
                    speculars.Add(go);

                    go.transform.position = l.transform.position;
                    go.transform.forward = l.transform.forward;

                    GameObjectUtility.SetStaticEditorFlags(go, StaticEditorFlags.ReflectionProbeStatic);

                    var mr = go.GetComponent<MeshRenderer>();
                    mr.sharedMaterial = lightMeshMat;

                    var mpb = new MaterialPropertyBlock();

                    mpb.SetColor("_LightColor", l.color);//todo temperature
                    mpb.SetFloat("_LightIntensity", l.intensity);

                    float r = Mathf.Max(l.shadowRadius, 0.002f) * 2.0f;

                    if (l.type == LightType.Point)
                    {
                        mpb.SetInt("_LightType", 0);
                        go.transform.localScale = new Vector3(r, r, r);
                    }
                    else if (l.type == LightType.Spot)
                    {
                        mpb.SetInt("_LightType", 1);
                        mpb.SetFloat("_LightSpotAngle", l.spotAngle);
                        go.transform.localScale = new Vector3(r, r, r);
                    }
                    else if (l.type == LightType.Directional)
                    {
                        mpb.SetInt("_LightType", 2);
                        mpb.SetFloat("_LightDirectionalAngle", l.shadowAngle);
                        go.transform.localScale = new Vector3(999, 999, 999); // todo this needs to be visible from all reflection probes and still not get culled
                    }
                    else if (l.type == LightType.Rectangle)
                    {
                        mpb.SetInt("_LightType", 3);
                        go.transform.localScale = new Vector3(l.areaSize.x, l.areaSize.y, 0.01f);
                    }

                    mr.SetPropertyBlock(mpb);
                }
            }

            if (supersampling > 1)
            {
                foreach (var probe in probes)
                {
                    probe.resolution *= supersampling;
                }
            }

            try
            {
                MethodInfo bakeMethod = typeof(Lightmapping).GetMethod(
                    "BakeAllReflectionProbesSnapshots",
                    BindingFlags.Static | BindingFlags.NonPublic
                );

                bool success = (bool)bakeMethod.Invoke(null, null);
            }
            finally
            {
                if (supersampling > 1)
                {
                    foreach (var probe in probes)
                    {
                        probe.resolution /= supersampling;

                        var path = AssetDatabase.GetAssetPath(probe.bakedTexture);
                        TextureImporter textureImporter = AssetImporter.GetAtPath(path) as TextureImporter;
                        if (textureImporter == null)
                        {
                            continue;
                        }

                        textureImporter.maxTextureSize = probe.resolution;
                        textureImporter.SaveAndReimport();
                    }
                }

                foreach (var go in speculars)
                {
                    DestroyImmediate(go);
                }
            }
        }
    }
}
