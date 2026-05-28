#include "includes/core.slang"
#include "includes/sobol_burley.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    uint width;
    uint height;
    uint sampleIndex;
    uint maxSamples;

    uint bounceIndex;
    uint pad0;
    uint pad1;
    uint pad2;
};

[shader("compute")]
    [numthreads(8, 8, 1)] void
    main(uint3 threadID : SV_DispatchThreadID)
{
    uint2 coords = threadID.xy;
    uint2 resolution = uint2(Push.width, Push.height);

    if (coords.x >= resolution.x || coords.y >= resolution.y)
    {
        return;
    }

    Spectrum spectrum = SpectrumZero();

    float4 visibility = VisibilityBuffer[coords];

    uint encoded = asuint(visibility.a);

    if (encoded == 0)
    {
        return;
    }

    uint primitiveID = asuint(visibility.z);
    float2 bary = visibility.xy;
    bool useSmoothP = (encoded & (1 << 1)) != 0;

    Surface surface = Surface::Get(primitiveID, bary);

    uint sampleIndex = Push.sampleIndex;
    uint sampleCount = Push.maxSamples;
    uint bounceIndex = Push.bounceIndex;

    uint sobolIndexMask = ~0u;

    SamplingPattern pattern = SamplingPattern::BlueNoiseFirst;
    uint pixelSeed = RNGPixelInit(coords, pattern);
    uint3 indexing = BlueNoiseIndexing(pixelSeed, sampleIndex, sobolIndexMask, sampleCount, pattern);

    float2 xiBounce = SobolBurleySample2D(indexing.x, bounceIndex + 1, indexing.y, indexing.z);

    Ray bounceRay;
    bounceRay.P = RayOffset(surface.P, surface.Ng);
    bounceRay.D = RandomDirectionInHemisphere(surface.N, xiBounce);
    bounceRay.tMin = 0;
    bounceRay.tMax = RAY_MAX;

    Hit bounceHit;
    if (TraceRay(bounceRay, bounceHit))
    {
        Surface previousSurface = surface;
        surface = Surface::Get(bounceHit.primitiveID, bounceHit.bary);
        bool backfaceGI = surface.backfaceGI;

        if (!bounceHit.frontFace)
        {
            if (!backfaceGI)
            {
                return;
            }

            surface.N = -surface.N;
            surface.Ng = -surface.Ng;
        }

        float3 bounceAlbedo = SampleAlbedo(surface.uv, surface.lightmapGroup).rgb;
        if (bounceIndex == 0)
        {
            float3 bounceEmission = SampleEmission(surface.uv, surface.lightmapGroup).rgb;
            SpectrumAdd(spectrum, bounceEmission, 0);
        }

        uint2 coords = uint2(surface.uv * resolution);
        uint index = NonUniformResourceIndex(surface.lightmapGroup);
        float3 previousBounce = PreviousDiffuse[index][coords].rgb;

        SpectrumAdd(spectrum, previousBounce * bounceAlbedo, 0);
    }

    AccumulateColor(LightmapDiffuse, coords, sampleIndex, float4(spectrum.color, 1.0));
}
