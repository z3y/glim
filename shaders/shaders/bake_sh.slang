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
    uint pad0;
    uint bounceCount;
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

    // SHProbe probe = SHProbes[probeIndex];
    // float3 probePosition = probe.position;

    // Spectrum spectrum = SpectrumZero();
    // float3 throughput = float3(1.0);

    // uint sampleIndex = Push.sampleIndex;
    // uint sampleCount = Push.maxSamples;
    // uint bounceCount = Push.bounceCount;
    // uint lightCount = Push.lightCount;
    // // todo set constant
    // // this is actually incorrect in cycles
    // // uint sobolIndexMask = reversebits(NextPowerOfTwo(sampleCount - 1) - 1);
    // uint sobolIndexMask = NextPowerOfTwo(sampleCount - 1) - 1;

    // SamplingPattern pattern = SamplingPattern::BlueNoisePure;
    // uint pixelSeed = RNGPixelInit(uint2(probeIndex.x, 0), pattern);
    // uint3 indexing = BlueNoiseIndexing(pixelSeed, sampleIndex, sobolIndexMask, sampleCount, pattern);
    // uint dimension = 0;

    // float2 xiLight = SobolBurleySample2D(indexing.x, dimension++, indexing.y, indexing.z);

    // for (uint lightIndex = 0; lightIndex < lightCount; lightIndex++)
    // {
    //     ShadeLightDirect(spectrum, Lights[lightIndex], probePosition, xiLight, throughput);
    // }

    // float2 xiBounce0 = SobolBurleySample2D(indexing.x, dimension++, indexing.y, indexing.z);

    // Ray bounceRay;
    // bounceRay.P = probePosition;
    // bounceRay.D = RandomDirectionSphere(xiBounce0);
    // bounceRay.tMin = 0;
    // bounceRay.tMax = RAY_MAX;
    // float3 wi = bounceRay.D;

    // float3 indirect = 0;

    // // bounces
    // for (uint bounceIndex = 0; bounceIndex < bounceCount; bounceIndex++)
    // {
    //     Hit bounceHit;
    //     if (TraceRay(bounceRay, bounceHit))
    //     {
    //         Surface surfaceBounce = Surface::Get(bounceHit.primitiveID, bounceHit.bary);
    //         bool backfaceGI = surfaceBounce.backfaceGI;

    //         if (!bounceHit.frontFace)
    //         {
    //             if (!backfaceGI)
    //             {
    //                 break;
    //             }

    //             surfaceBounce.N = -surfaceBounce.N;
    //             surfaceBounce.Ng = -surfaceBounce.Ng;
    //         }

    //         float3 bounceEmission = SampleEmission(surfaceBounce.uv, surfaceBounce.lightmapGroup).rgb;
    //         float3 bounceAlbedo = SampleAlbedo(surfaceBounce.uv, surfaceBounce.lightmapGroup).rgb;

    //         indirect += bounceEmission * throughput;

    //         throughput *= bounceAlbedo;

    //         float2 xiLight = SobolBurleySample2D(indexing.x, dimension++, indexing.y, indexing.z);
    //         float2 xiBounce = SobolBurleySample2D(indexing.x, dimension++, indexing.y, indexing.z);

    //         for (uint lightIndex = 0; lightIndex < lightCount; lightIndex++)
    //         {
    //             indirect += ShadeLightIndirect(spectrum, Lights[lightIndex], surfaceBounce, xiLight, throughput);
    //         }

    //         bounceRay.P = RayOffset(surfaceBounce.P, surfaceBounce.Ng);
    //         bounceRay.D = RandomDirectionInHemisphere(surfaceBounce.N, xiBounce);
    //     }
    //     else
    //     {
    //         break;
    //     }
    // }

    // SpectrumAdd(spectrum, indirect * 4.0, wi);

    // AccumulateSH(SHProbes, probeIndex, spectrum);
}
