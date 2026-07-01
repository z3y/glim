using System.IO;
using UnityEditor;
using UnityEngine;
using UnityEngine.SceneManagement;
using UnityEditor.SceneManagement;

namespace stilb
{
    public class LightingData
    {
        public const string TempScenePath = "Packages/io.github.z3y.stilb/Editor/Scene/Temp.unity";
        public const string TempLightingDataPath = "Packages/io.github.z3y.stilb/Editor/Scene/Temp/LightingData.asset";

        // todo https://github.com/pema99/GITweaks/blob/master/Editor/GITweaksLightingDataAssetEditor.cs

        public static System.Reflection.PropertyInfo InspectorModeObject =
                    typeof(SerializedObject).GetProperty("inspectorMode", System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Instance);

        public struct SerializedObjectID : System.IEquatable<SerializedObjectID>
        {
            public long MainLFID; // If prefab, LFID in MeshRenderer in prefab stage, else LFID of object
            public long PrefabLFID; // If prefab, LFID of "Prefab instance" object, points to prefab

            public SerializedObjectID(long main, long prefab)
            {
                MainLFID = main;
                PrefabLFID = prefab;
            }

            public bool Equals(SerializedObjectID other) => other.MainLFID == MainLFID && other.PrefabLFID == PrefabLFID;
            public override bool Equals(object obj) => obj is SerializedObjectID id && Equals(id);
            public static bool operator ==(SerializedObjectID a, SerializedObjectID b) => a.Equals(b);
            public static bool operator !=(SerializedObjectID a, SerializedObjectID b) => !(a == b);
            public override int GetHashCode() => System.HashCode.Combine(MainLFID, PrefabLFID);
        }

        public static SerializedObjectID ObjectToSOI(Object obj)
        {
            using var mainSO = new SerializedObject(obj);
            InspectorModeObject.SetValue(mainSO, InspectorMode.DebugInternal);
            long lfid = mainSO.FindProperty("m_LocalIdentfierInFile").longValue;

            var prefabInstance = mainSO.FindProperty("m_PrefabInstance");
            if (prefabInstance.objectReferenceValue != null)
            {
                using var prefabInstanceSO = new SerializedObject(prefabInstance.objectReferenceValue);
                InspectorModeObject.SetValue(prefabInstanceSO, InspectorMode.DebugInternal);

                using var correspondingSO = new SerializedObject(mainSO.FindProperty("m_CorrespondingSourceObject").objectReferenceValue);
                InspectorModeObject.SetValue(correspondingSO, InspectorMode.DebugInternal);

                long sourceLFID = correspondingSO.FindProperty("m_LocalIdentfierInFile").longValue;
                long prefabLFID = prefabInstanceSO.FindProperty("m_LocalIdentfierInFile").longValue;

                return new SerializedObjectID(sourceLFID, prefabLFID);
            }
            else
            {
                return new SerializedObjectID(lfid, 0);
            }
        }

        // [MenuItem("Test/ZeroL2Coefficients")]
        public static void ZeroL2Coefficients()
        {
            var lightingDataAsset = Lightmapping.lightingDataAsset;
            using var lda = new SerializedObject(lightingDataAsset);
            InspectorModeObject.SetValue(lda, InspectorMode.DebugInternal);

            var lightProbesRef = lda.FindProperty("m_LightProbes").objectReferenceValue;
            using var probesSo = new SerializedObject(lightProbesRef);
            InspectorModeObject.SetValue(probesSo, InspectorMode.DebugInternal);

            var bakedCoeff = probesSo.FindProperty("m_BakedCoefficients");

            for (int i = 0; i < bakedCoeff.arraySize; i++)
            {
                SerializedProperty prop = bakedCoeff.GetArrayElementAtIndex(i);
                Debug.Log($"Probe {i}:");

                prop.Next(true);

                for (int j = 0; j < 27; j++)
                {
                    // L2 starts at coefficient 4 for each color channel
                    // RGB blocks:
                    // 0-8   = R
                    // 9-17  = G
                    // 18-26 = B

                    int channelCoeff = j % 9;

                    if (channelCoeff >= 4)
                    {
                        prop.floatValue = 0f;
                    }
                    else
                    {
                        Debug.Log($" [{j}] = {prop.floatValue}");
                    }

                    prop.Next(false);
                }
            }

            probesSo.ApplyModifiedPropertiesWithoutUndo();
            Lightmapping.lightingDataAsset = lightingDataAsset;
        }

        public static LightingDataAsset CreateAsset(Scene targetScene)
        {
            bool saved = EditorSceneManager.SaveCurrentModifiedScenesIfUserWantsTo();

            if (!saved)
            {
                throw new System.Exception("Bake failed");
            }

            var scenePath = targetScene.path;

            Scene tempScene = EditorSceneManager.OpenScene(TempScenePath, OpenSceneMode.Additive);

            foreach (GameObject obj in targetScene.GetRootGameObjects())
            {
                if (!obj.activeInHierarchy)
                {
                    continue;
                }

                var probes = obj.GetComponentsInChildren<LightProbeGroup>(false);
                foreach (var probe in probes)
                {
                    if (!probe.enabled)
                    {
                        continue;
                    }

                    GameObject copyObj = new($"Copy_{probe.gameObject.name}");
                    copyObj.transform.SetPositionAndRotation(probe.transform.position, probe.transform.rotation);
                    copyObj.transform.localScale = probe.transform.lossyScale;
                    var targetGroup = copyObj.AddComponent<LightProbeGroup>();

                    // todo maybe deduplicate probe positions here
                    targetGroup.probePositions = probe.probePositions;

                    SceneManager.MoveGameObjectToScene(copyObj, tempScene);
                }
            }

            EditorSceneManager.CloseScene(targetScene, true);
            EditorSceneManager.SaveScene(tempScene);
            EditorSceneManager.SetActiveScene(tempScene);

            if (!Lightmapping.Bake())
            {
                throw new System.Exception("Bake failed");
            }


            foreach (GameObject obj in tempScene.GetRootGameObjects())
            {
                Object.DestroyImmediate(obj);
            }
            Lightmapping.lightingDataAsset = null;
            EditorSceneManager.SaveScene(tempScene);

            targetScene = EditorSceneManager.OpenScene(scenePath, OpenSceneMode.Single);
            EditorSceneManager.SetActiveScene(targetScene);
            EditorSceneManager.CloseScene(tempScene, true);

            var lightingDataAsset = AssetDatabase.LoadAssetAtPath<LightingDataAsset>(TempLightingDataPath);
            using var lda = new SerializedObject(lightingDataAsset);
            InspectorModeObject.SetValue(lda, InspectorMode.DebugInternal);

            var sceneAsset = AssetDatabase.LoadAssetAtPath<SceneAsset>(targetScene.path);

            var sceneProp = lda.FindProperty("m_Scene");
            Debug.Assert(sceneProp != null);
            sceneProp.objectReferenceValue = sceneAsset;
            lda.ApplyModifiedPropertiesWithoutUndo();

            return lightingDataAsset;
        }
    }
}
