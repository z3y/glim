#if GLIM_APV
using System.Collections.Generic;
using Unity.Collections;
using UnityEditor;
using UnityEngine;
using UnityEngine.Rendering;

namespace Glim
{
    // Plugged into Unity's APV bake as the lighting backend.
    // Unity does placement and calls Initialize with the probe positions synchronously inside BakeAsync,
    // Glim bakes those positions together with the rest of the probes, then hands the SH back here.
    // Unity keeps polling Step on the main thread and finishes the cells/serialization itself.
    class GlimAPVLightingBaker : AdaptiveProbeVolumes.LightingBaker
    {
        public Vector3[] positions;
        public bool initialized;
        public bool disposed;

        NativeArray<SphericalHarmonicsL2> _irradiance;
        NativeArray<float> _validity;
        NativeArray<Vector4> _occlusion;

        bool _bakeOcclusion;
        bool _resultsReady;
        bool _failed;
        ulong _currentStep;

        public override bool isThreadSafe => false;
        public override ulong currentStep => _currentStep;
        public override ulong stepCount => positions == null ? 0ul : (ulong)positions.Length;
        public override NativeArray<SphericalHarmonicsL2> irradiance => _irradiance;
        public override NativeArray<float> validity => _validity;
        public override NativeArray<Vector4> occlusion => _occlusion;

        public override void Initialize(bool bakeProbeOcclusion, NativeArray<Vector3> probePositions)
        {
            // includes reflection probe normalization positions at the end, bake them all
            positions = probePositions.ToArray();
            int count = positions.Length;

            _irradiance = new NativeArray<SphericalHarmonicsL2>(count, Allocator.Persistent);
            _validity = new NativeArray<float>(count, Allocator.Persistent);

            _bakeOcclusion = bakeProbeOcclusion;
            if (bakeProbeOcclusion)
            {
                _occlusion = new NativeArray<Vector4>(count, Allocator.Persistent);
            }

            _currentStep = 0;
            initialized = true;
        }

        public override void Initialize(bool bakeProbeOcclusion, NativeArray<Vector3> probePositions, NativeArray<uint> bakedRenderingLayerMasks)
        {
            Initialize(bakeProbeOcclusion, probePositions);
        }

        public override bool Step()
        {
            if (_failed)
            {
                return false;
            }

            if (_resultsReady)
            {
                _currentStep = stepCount;
            }

            return true;
        }

        public void SetResults(List<Bindings.SHProbe> results, int indexStart)
        {
            SphericalHarmonicsL2 sh = new();

            for (int i = 0; i < positions.Length; i++)
            {
                Bindings.SHProbe p = results[indexStart + i];

                sh[0, 0] = p.L0.x; sh[0, 1] = p.L1_1.x; sh[0, 2] = p.L10.x; sh[0, 3] = p.L11.x; sh[0, 4] = p.L2_2.x; sh[0, 5] = p.L2_1.x; sh[0, 6] = p.L20.x; sh[0, 7] = p.L21.x; sh[0, 8] = p.L22.x;
                sh[1, 0] = p.L0.y; sh[1, 1] = p.L1_1.y; sh[1, 2] = p.L10.y; sh[1, 3] = p.L11.y; sh[1, 4] = p.L2_2.y; sh[1, 5] = p.L2_1.y; sh[1, 6] = p.L20.y; sh[1, 7] = p.L21.y; sh[1, 8] = p.L22.y;
                sh[2, 0] = p.L0.z; sh[2, 1] = p.L1_1.z; sh[2, 2] = p.L10.z; sh[2, 3] = p.L11.z; sh[2, 4] = p.L2_2.z; sh[2, 5] = p.L2_1.z; sh[2, 6] = p.L20.z; sh[2, 7] = p.L21.z; sh[2, 8] = p.L22.z;

                _irradiance[i] = sh;
                _validity[i] = 0.0f; // 0 = valid, glim has no validity output yet

                if (_bakeOcclusion)
                {
                    // indexed by shadowmask channel, same as the light probe occlusion
                    Vector4 occ = Vector4.zero;
                    for (int channel = 0; channel < 4; channel++)
                    {
                        occ[channel] = p.GetOcclusionLight(channel) == -1 ? 0.0f : Mathf.Clamp01(p.GetOcclusion(channel));
                    }
                    _occlusion[i] = occ;
                }
            }

            _resultsReady = true;
        }

        public void Fail()
        {
            _failed = true;
        }

        public override void Dispose()
        {
            if (disposed)
            {
                return;
            }
            disposed = true;

            if (_irradiance.IsCreated) _irradiance.Dispose();
            if (_validity.IsCreated) _validity.Dispose();
            if (_occlusion.IsCreated) _occlusion.Dispose();

            // don't leave glim hooked into regular unity bakes
            if (AdaptiveProbeVolumes.GetLightingBakerOverride() == this)
            {
                AdaptiveProbeVolumes.SetLightingBakerOverride(null);
            }
        }
    }

    static class AdaptiveProbeVolumeBake
    {
        static GlimAPVLightingBaker s_Baker;
        static int s_IndexStart;

        // Starts the unity APV bake with glim as the lighting backend and appends the placed probes to the glim bake.
        public static void AddProbes(BakeContext ctx, float radius)
        {
            Cancel();

            if (Lightmapping.isRunning || AdaptiveProbeVolumes.isRunning)
            {
                Debug.LogWarning("Glim: another bake is running, Adaptive Probe Volumes will not be baked.");
                return;
            }

            var baker = new GlimAPVLightingBaker();
            AdaptiveProbeVolumes.SetLightingBakerOverride(baker);

            bool started = false;
            try
            {
                // returns false when APV is disabled or there are no probe volumes in the scene
                started = AdaptiveProbeVolumes.BakeAsync();
            }
            finally
            {
                if (!started)
                {
                    baker.Dispose();
                }
            }

            if (!started)
            {
                return;
            }

            if (!baker.initialized)
            {
                AdaptiveProbeVolumes.Cancel();
                baker.Dispose();
                return;
            }

            s_Baker = baker;
            s_IndexStart = ctx.probePositions.Count;

            foreach (var p in baker.positions)
            {
                ctx.probePositions.Add(new Vector4(p.x, p.y, p.z, radius));
            }

            Bake.bakeMessages.AppendLine($"Adaptive Probe Volumes: {baker.positions.Length} probes");
        }

        // Hands the baked probes to unity, it finishes writing the APV data on the following editor updates.
        public static void Complete(List<Bindings.SHProbe> results)
        {
            var baker = s_Baker;
            if (baker == null)
            {
                return;
            }

            if (baker.disposed)
            {
                // canceled from unity's side
                s_Baker = null;
                return;
            }

            if (results.Count < s_IndexStart + baker.positions.Length)
            {
                Debug.LogError("Glim: missing probe results, Adaptive Probe Volumes bake canceled.");
                Cancel();
                return;
            }

            baker.SetResults(results, s_IndexStart);
            s_Baker = null;
        }

        // Cancels the unity APV bake if it is still waiting for glim results.
        public static void Cancel()
        {
            var baker = s_Baker;
            s_Baker = null;

            if (baker == null || baker.disposed)
            {
                return;
            }

            baker.Fail();
            AdaptiveProbeVolumes.Cancel();
            baker.Dispose();
        }
    }
}
#endif
