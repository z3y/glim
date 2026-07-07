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

    uint lightCount;
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

    float4 visibility = VisibilityBuffer[coords];

    if (all(visibility == 0))
    {
        return;
    }

    float3 positionWS = visibility.xyz;
    float3 normalWS = UnpackNormalOctahedron(visibility.w);

    uint sampleIndex = Push.sampleIndex;
    uint sampleCount = Push.maxSamples;
    uint lightCount = Push.lightCount;

    SamplingPattern pattern = SamplingPattern::BlueNoiseFirst;
    uint pixelSeed = RNGPixelInit(coords, pattern);
    uint sobolIndexMask = ~0u;
    uint3 indexing = BlueNoiseIndexing(pixelSeed, sampleIndex, sobolIndexMask, sampleCount, pattern);

    float2 xiLight = SobolBurleySample2D(indexing.x, 0, indexing.y, indexing.z);

    float4 color = float4(0.0, 0.0, 0.0, 1.0);
    float3 throughput = float3(1.0);

    for (uint lightIndex = 0; lightIndex < lightCount; lightIndex++)
    {
        float3 L;
        color.rgb += ShadeLightDirect(Lights[lightIndex], positionWS, normalWS, xiLight, throughput, L);
    }

    float2 xiBounce = SobolBurleySample2D(indexing.x, 1, indexing.y, indexing.z);

    Ray bounceRay;
    bounceRay.P = positionWS;
    bounceRay.D = RandomDirectionInHemisphere(normalWS, xiBounce);
    bounceRay.tMin = 0;
    bounceRay.tMax = RAY_MAX;

    Hit bounceHit;
    if (TraceRay(bounceRay, bounceHit))
    {
        Surface surface = Surface::Get(bounceHit.primitiveID, bounceHit.bary, bounceHit.frontFace);

        if (!bounceHit.frontFace)
        {
            if (!surface.backfaceGI)
            {
                color.a = 0;
            }
        }

        float3 bounceEmission = SampleEmission(surface.uv, surface.lightmapGroup).rgb;
        color.rgb += bounceEmission * color.a;
    }

    float4 previous = LightmapDiffuse[coords];
    color.a = 1.0;
    AccumulateColor(previous, sampleIndex, color);
    LightmapDiffuse[coords] = previous;
}
