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

namespace glim
{
    [CustomEditor(typeof(LightmapBaker))]
    public class LightmapBakerEditor : Editor
    {
        SerializedObject _nestedSO;

        public override VisualElement CreateInspectorGUI()
        {
            var root = new VisualElement();

            var baker = target as LightmapBaker;

            InspectorElement.FillDefaultInspector(root, serializedObject, this);

            var nestedContainer = new VisualElement();
            root.Add(nestedContainer);

            void RebuildNested()
            {
                nestedContainer.Clear();

                _nestedSO?.Dispose();
                _nestedSO = null;

                if (baker.group)
                {
                    _nestedSO = new SerializedObject(baker.group);
                    VisualElement nestedInspector = CreateNestedInspector(_nestedSO, this);
                    nestedContainer.Add(nestedInspector);
                }
            }

            RebuildNested();

            var globalGroupProp = serializedObject.FindProperty(nameof(baker.group));
            root.TrackPropertyValue(globalGroupProp, _ => RebuildNested());

            {
                VisualElement element = new()
                {
                    style =
                    {
                        height = 20
                    }
                };
                root.Add(element);
            }




            {
                Button button = new()
                {
                    text = "Open Preview Window",
                    style =
                    {
                        height = 25
                    }
                };
                button.clicked += () =>
                {
                    var camera = SceneView.lastActiveSceneView.camera;

                    var previewSettings = new Bindings.LightmapSettings(
                        baker.previewWidth, baker.previewHeight, false, false, false);

                    var config = new Bindings.GlimConfig(
                        Bindings.CoordinateSystem.Unity,
                        baker.previewSamples,
                        baker.previewSamples,
                        baker.previewBounces,
                        true,
                        baker.previewThrottle,
                        previewSettings,
                        camera.transform.position,
                        camera.transform.forward,
                        baker.lightProbeSamples,
                        baker.lightFalloff,
                        baker.multipleImportanceSampling,
                        baker.lightmapMode
                    );

                    Bake.Start(baker, config);
                };
                root.Add(button);
            }

            {
                Button button = new()
                {
                    text = "Bake Reflection Probes",
                    style =
                    {
                        height = 25
                    }
                };
                button.clicked += () =>
                {
                    BakeAllReflectionProbesSnapshots(EditorSceneManager.GetActiveScene(), baker.reflectionProbesSuperSampling ? 2 : 1, baker.reflectionProbesSpecular);
                };
                root.Add(button);
            }

            {
                Button button = new()
                {
                    text = "Clear Lighting Data",
                    style =
                    {
                        height = 25
                    }
                };
                button.clicked += () =>
                {
                    Lightmapping.lightingDataAsset = null;
                    EditorSceneManager.MarkSceneDirty(EditorSceneManager.GetActiveScene());
                };
                root.Add(button);
            }


            {
                VisualElement element = new()
                {
                    style =
                    {
                        height = 20
                    }
                };
                root.Add(element);
            }

            {
                Button button = new()
                {
                    text = "Generate Lighting",
                    style =
                    {
                        height = 35
                    }
                };
                button.clicked += () =>
                {
                    var previewSettings = new Bindings.LightmapSettings();

                    var config = new Bindings.GlimConfig(
                        Bindings.CoordinateSystem.Unity,
                        baker.directSamples,
                        baker.indirectSamples,
                        baker.bounces,
                        false,
                        0,
                        previewSettings,
                        Vector3.zero,
                        Vector3.zero,
                        baker.lightProbeSamples,
                        baker.lightFalloff,
                        baker.multipleImportanceSampling,
                        baker.lightmapMode
                    );
                    Bake.Start(baker, config);
                };
                root.Add(button);

                ProgressBar progressBar = new()
                {
                    style =
                    {
                        height = 20,
                        display = DisplayStyle.None
                    }
                };
                root.Add(progressBar);

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
                    var last = Bake.LoadReport(baker.gameObject.scene.path);
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
                    button.SetEnabled(!running);

                    if (running)
                    {
                        progressBar.value = Mathf.Clamp01(Bake.BakeProgress) * 100f;
                        progressBar.title = Bake.BakeMessage;
                    }

                    if (seenReport != Bake.ReportVersion)
                    {
                        seenReport = Bake.ReportVersion;
                        RefreshReport();
                    }
                }).Every(100);
            }

            return root;
        }

        public static VisualElement CreateNestedInspector(SerializedObject so, Editor editor)
        {
            VisualElement nestedInspector = new();
            InspectorElement.FillDefaultInspector(nestedInspector, so, editor);
            nestedInspector.Bind(so);
            nestedInspector.Q<PropertyField>("PropertyField:m_Script").style.display = DisplayStyle.None;
            return nestedInspector;
        }

        public static void BakeAllReflectionProbesSnapshots(Scene scene, int supersampling, bool specularProbes)
        {
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
