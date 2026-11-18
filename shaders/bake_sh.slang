#define LIGHTPROBE_SH

#include "includes/compaction.slang"
#include "includes/core.slang"
#include "includes/light_tree.slang"
#include "includes/sobol_burley.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    RayTracingData rt;

    uint lightCount;
    uint maxSamples;
    uint sampleIndex;
    uint probeCount;

    SHProbeL2 *shProbes;
    uint mixedStart;
    uint mixedEnd;

    uint2 *compaction;
    LightmapInfo *lightmapsInfo;

    Light *lights;
    float *lightmap;

    float4 *lightTree;
    uint *primitiveToLeaf;
};

[shader("compute")]
[numthreads(64, 1, 1)]
void main(uint3 threadID: SV_DispatchThreadID)
{
    uint probeIndex = threadID.x;

    if (probeIndex >= Push.probeCount)
    {
        return;
    }

    SHProbeL2 *probes = Push.shProbes;

    SHProbeL2 probe = probes[probeIndex];

    uint sampleIndex = Push.sampleIndex;
    uint sampleCount = Push.maxSamples;
    uint lightCount = Push.lightCount;

    float3 probePosition = probe.position;
    float sampleScale = 1.0 / float(sampleCount);

    uint sobolIndexMask = ~0u;
    SamplingPattern pattern = SamplingPattern::BlueNoisePure;
    uint pixelSeed = RNGPixelInit1D(probeIndex);
    uint3 indexing = BlueNoiseIndexing(pixelSeed, sampleIndex, sobolIndexMask, sampleCount, pattern);
    float2 xiLight = SobolBurleySample2D(indexing.x, DIMENSION_LIGHT, indexing.y, indexing.z);
    float2 xiBounce = SobolBurleySample2D(indexing.x, DIMENSION_BOUNCE, indexing.y, indexing.z);

    float radius = probe.radius;
    if (radius > 0.0)
    {
        // own dimensions, reusing xiLight/xiBounce correlates the position with the light sample and ray direction
        float2 xiJitter = SobolBurleySample2D(indexing.x, DIMENSION_PROBE_JITTER_XY, indexing.y, indexing.z);
        float xiJitterZ = SobolBurleySample1D(indexing.x, DIMENSION_PROBE_JITTER_Z, indexing.y, indexing.z);
        float3 jitter = float3(xiJitter, xiJitterZ) * 2.0 - 1.0;
        probePosition += jitter * radius;
    }

    float3 throughput = float3(1.0);

    const uint OCCLUSION_SAMPLE_STRIDE = 16;
    bool traceOcclusion = (sampleIndex % OCCLUSION_SAMPLE_STRIDE) == 0;
    uint occlusionSampleCount = (sampleCount + OCCLUSION_SAMPLE_STRIDE - 1) / OCCLUSION_SAMPLE_STRIDE;
    float4 occlusion = float4(0.0);

    // occlusion only runs every OCCLUSION_SAMPLE_STRIDE samples, every 16th index of the main sequence
    // would land in a single stratum, so it gets its own dense sequence
    float2 xiOcclusionBase = 0.0;
    if (traceOcclusion)
    {
        uint occlusionIndex = sampleIndex / OCCLUSION_SAMPLE_STRIDE;
        uint3 occlusionIndexing = BlueNoiseIndexing(pixelSeed, occlusionIndex, sobolIndexMask, occlusionSampleCount, pattern);
        xiOcclusionBase = SobolBurleySample2D(occlusionIndexing.x, DIMENSION_LIGHT, occlusionIndexing.y, occlusionIndexing.z);
    }

    Light *lights = Push.lights;
    for (uint lightIndex = 0; lightIndex < lightCount; lightIndex++)
    {
        Light light = lights[lightIndex];
        if (light.Mixed())
        {
            uint channel = light.OcclusionChannel();
            if (traceOcclusion && channel <= 3 && light.CastShadows())
            {
                Ray shadowRay;
                float3 L;
                float2 xiOcclusion = DecorrelateSample2D(xiOcclusionBase, lightIndex);
                if (ShadowMaskRay(light, probePosition, xiOcclusion, shadowRay, L))
                {
                    // channels are assigned so lights sharing one never overlap,
                    // the light in range is the only one that can affect this probe on that channel
                    // light index stored in sh[4..7].w as int bits, -1 = no light
                    probe.sh[4 + channel].w = asfloat(int(lightIndex));

                    occlusion[channel] = Luminance(TraceShadowRay(shadowRay));
                }
            }
            continue;
        }
        float3 L;
        float2 xi = DecorrelateSample2D(xiLight, lightIndex);
        float3 color = ShadeLightDirect(light, probePosition, 0.0, xi, throughput, L, true, ShadowTerminator::None());
        AccumulateL2(probe, sampleIndex, color * sampleScale, L);
    }

    occlusion /= float(occlusionSampleCount);
    probe.sh[0].w += occlusion.x;
    probe.sh[1].w += occlusion.y;
    probe.sh[2].w += occlusion.z;
    probe.sh[3].w += occlusion.w;

    float bsdfPdf = 1.0 / (4.0 * float.getPi());

    if (EMISSIVE_TRIANGLES_COUNT > 0)
    {
        float xiPick = SobolBurleySample1D(indexing.x, DIMENSION_LIGHT_PICK, indexing.y, indexing.z);

        uint emissivePrimitiveID;
        float selectionPmf;

        if (LightTreeSample(Push.lightTree, probePosition, 0.0, false, xiPick, emissivePrimitiveID, selectionPmf))
        {
            float3 L;
            float pdf = 0.0;
            // own key, light indices never reach ~0u
            float2 xi = DecorrelateSample2D(xiLight, ~0u);
            float3 current = ShadeTriangleLight(emissivePrimitiveID, probePosition, 0.0, xi, throughput, selectionPmf, pdf, L, true, ShadowTerminator::None());
            current *= MISWeight(pdf, bsdfPdf);
            AccumulateL2(probe, sampleIndex, current * sampleScale, L);
        }
    }

    float *lightmap = Push.lightmap;

    Ray ray;
    ray.P = probePosition;
    ray.D = RandomDirectionInSphere(xiBounce);
    ray.tMin = 0;
    ray.tMax = RAY_MAX;

    Hit hit;
    if (TraceRay(ray, hit))
    {
        Surface surface = Surface::Get(hit.primitiveID, hit.bary, hit.frontFace);

        bool backface = !hit.frontFace && !surface.backfaceGI;

        if (!backface)
        {
            float3 emission = SampleEmission(surface.coords, surface.lightmapGroup).rgb;

            if (surface.emissive && any(emission > 0.0))
            {
                float weight = 1.0;

                if (EMISSIVE_TRIANGLES_COUNT > 0)
                {
                    float selectionPmf = LightTreePmf(Push.lightTree, Push.primitiveToLeaf, hit.primitiveID, probePosition, 0.0, false);

                    if (selectionPmf > 0.0)
                    {
                        float lightPdf = selectionPmf * TriangleLightPdf(hit.primitiveID, probePosition, 0.0, surface.P, true);
                        weight = MISWeight(bsdfPdf, lightPdf);
                    }
                }

                AccumulateL2(probe, sampleIndex, emission.rgb * weight * 4 * sampleScale, ray.D);
            }

            float3 bounceAlbedo = SampleAlbedo(surface.coords, surface.lightmapGroup).rgb;

            uint compactIndex = GetCompactedIndex(surface.coords, surface.lightmapGroup, hit.primitiveID, hit.frontFace);
            if (compactIndex != uint::maxValue)
            {
                float3 lightmapDiffuse = float3(lightmap[compactIndex * 3 + 0],
                                                lightmap[compactIndex * 3 + 1],
                                                lightmap[compactIndex * 3 + 2]);
                AccumulateL2(probe, sampleIndex, lightmapDiffuse * bounceAlbedo * 4 * sampleScale, ray.D);
            }

            // handle mixed lights that were not shaded directly
            // todo spec constant
            if (true)
            {
                float3 positionWS = RayOffset(surface.P, surface.Ng);
                float3 normalWS = surface.Ng;

                for (uint lightIndex = Push.mixedStart; lightIndex < Push.mixedEnd; lightIndex++)
                {
                    Light light = lights[lightIndex];
                    if (!light.Mixed())
                    {
                        continue;
                    }
                    float3 L;
                    float2 xi = DecorrelateSample2D(xiLight, lightIndex);
                    float3 current = ShadeLightDirect(light, positionWS, normalWS, xi, throughput, L, false, ShadowTerminator::None());
                    float NoL = saturate(dot(L, normalWS));
                    current *= NoL;
                    AccumulateL2(probe, sampleIndex, current * bounceAlbedo * 4 * sampleScale, ray.D);
                }
            }
        }
    }
    else
    {
        AccumulateL2(probe, sampleIndex, SampleSkybox(ray.D) * 4 * sampleScale, ray.D);
    }

    if (LIGHTPROBE_DERINGING > 0.0)
    {
        bool lastSample = sampleIndex == (sampleCount - 1);
        if (lastSample)
        {
            DeringSHProbeL2(probe, LIGHTPROBE_DERINGING);
        }
    }

    probes[probeIndex] = probe;
}
