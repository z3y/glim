#define LIGHTPROBE_SH

#include "includes/core.slang"
#include "includes/sobol_burley.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    uint lightCount;
    uint maxSamples;
    uint sampleIndex;
    uint probeCount;
};

[shader("compute")]
    [numthreads(64, 1, 1)] void
    main(uint3 threadID : SV_DispatchThreadID)
{
    uint probeIndex = threadID.x;

    if (probeIndex >= Push.probeCount)
    {
        return;
    }

    SHProbeL2 probe = SHProbes[probeIndex];
    float3 probePosition = probe.position;

    uint sampleIndex = Push.sampleIndex;
    uint sampleCount = Push.maxSamples;
    uint lightCount = Push.lightCount;

    float sampleScale = 1.0 / float(sampleCount);

    uint sobolIndexMask = ~0u;
    SamplingPattern pattern = SamplingPattern::BlueNoisePure;
    uint pixelSeed = RNGPixelInit1D(probeIndex);
    uint3 indexing = BlueNoiseIndexing(pixelSeed, sampleIndex, sobolIndexMask, sampleCount, pattern);
    uint dimension = 0;

    float2 xiLight = SobolBurleySample2D(indexing.x, 0, indexing.y, indexing.z);

    float probesRadius = probe.radius;
    if (probesRadius > 0.0)
    {
        probePosition = RandomPointOnSphere(probePosition, probesRadius, xiLight);
    }

    float3 throughput = float3(1.0);

    for (uint lightIndex = 0; lightIndex < lightCount; lightIndex++)
    {
        float3 L;
        float3 color = ShadeLightDirectSH(Lights[lightIndex], probePosition, xiLight, throughput, L);
        AccumulateL2(probe, sampleIndex, color * sampleScale, L);
    }

    float2 xiBounce = SobolBurleySample2D(indexing.x, 1, indexing.y, indexing.z);

    Ray ray;
    ray.P = probePosition;
    ray.D = RandomDirectionSphere(xiBounce);
    ray.tMin = 0;
    ray.tMax = RAY_MAX;

    Hit hit;
    if (TraceRay(ray, hit))
    {
        Surface surface = Surface::Get(hit.primitiveID, hit.bary, hit.frontFace);

        bool backface = !hit.frontFace && !surface.backfaceGI;

        if (!backface)
        {
            float3 bounceEmission = SampleEmission(surface.uv, surface.lightmapGroup).rgb;
            AccumulateL2(probe, sampleIndex, bounceEmission.rgb * 4 * sampleScale, ray.D);

            float3 bounceAlbedo = SampleAlbedo(surface.uv, surface.lightmapGroup).rgb;

            uint compactIndex = GetCompactedIndex(surface.uv, surface.lightmapGroup);
            if (compactIndex != uint::maxValue)
            {
                float3 lightmapDiffuse = float3(CompactedLightmap[compactIndex * 3 + 0],
                                                CompactedLightmap[compactIndex * 3 + 1],
                                                CompactedLightmap[compactIndex * 3 + 2]);
                AccumulateL2(probe, sampleIndex, lightmapDiffuse * bounceAlbedo * 4 * sampleScale, ray.D);
            }
        }
    }
    else
    {
        AccumulateL2(probe, sampleIndex, SampleSkybox(ray.D) * 4 * sampleScale, ray.D);
    }

    if (LIGHTPROBE_DERINGING > 0.0)
    {
        if (Push.sampleIndex == Push.maxSamples - 1)
        {
            DeringSHProbeL2(probe, LIGHTPROBE_DERINGING);
        }
    }

    SHProbes[probeIndex] = probe;
}
