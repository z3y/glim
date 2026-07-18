#include "includes/core.slang"
#include "includes/sobol_burley.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    uint compactedCount;
    uint sampleIndex;
    uint maxSamples;
    uint lightCount;
};

[shader("compute")]
    [numthreads(64, 1, 1)] void
    main(uint3 threadID : SV_DispatchThreadID)
{
    uint index = threadID.x;

    if (index >= Push.compactedCount)
    {
        return;
    }

    float4 visibility = CompactedVisibility[index];

    float3 positionWS = visibility.xyz;
    float3 normalWS = UnpackNormalOctahedron(visibility.w);

    uint sampleIndex = Push.sampleIndex;
    uint sampleCount = Push.maxSamples;
    uint lightCount = Push.lightCount;

    SamplingPattern pattern = SamplingPattern::BlueNoiseFirst;
    uint pixelSeed = RNGPixelInit1D(index);
    uint sobolIndexMask = ~0u;
    uint3 indexing = BlueNoiseIndexing(pixelSeed, sampleIndex, sobolIndexMask, sampleCount, pattern);

    float2 xiLight = SobolBurleySample2D(indexing.x, 0, indexing.y, indexing.z);

    float4 color = float4(0.0, 0.0, 0.0, 1.0);
    float3 throughput = float3(1.0);
    float4 dominantDirection = 0.0;

    for (uint lightIndex = 0; lightIndex < lightCount; lightIndex++)
    {
        float3 L;
        float3 current = ShadeLightDirect(Lights[lightIndex], positionWS, normalWS, xiLight, throughput, L);
        color.rgb += current;
        dominantDirection.xyz += L * Luminance(current);
    }

    float2 xiBounce = SobolBurleySample2D(indexing.x, 1, indexing.y, indexing.z);

    Ray ray;
    ray.P = positionWS;
    ray.D = RandomDirectionInHemisphere(normalWS, xiBounce);
    ray.tMin = 0;
    ray.tMax = RAY_MAX;

    Hit bounceHit;
    if (TraceRay(ray, bounceHit))
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
        float3 current = bounceEmission * color.a;
        color.rgb += bounceEmission * color.a;
        dominantDirection.xyz += ray.D * Luminance(current);
    }
    else
    {
        float3 current = SampleSkybox(ray.D);
        color.rgb += current;
        dominantDirection.xyz += ray.D * Luminance(current);
    }

    float4 previous = float4(
        CompactedLightmap[index * 3 + 0],
        CompactedLightmap[index * 3 + 1],
        CompactedLightmap[index * 3 + 2],
        1.0);

    AccumulateColor(previous, sampleIndex, color);
    CompactedLightmap[index * 3 + 0] = previous.x;
    CompactedLightmap[index * 3 + 1] = previous.y;
    CompactedLightmap[index * 3 + 2] = previous.z;

    if (LIGHTMAP_MODE == LightmapMode::Directional)
    {
        uint index = index + Push.compactedCount;
        CompactedLightmap[index * 3 + 0] += dominantDirection.x;
        CompactedLightmap[index * 3 + 1] += dominantDirection.y;
        CompactedLightmap[index * 3 + 2] += dominantDirection.z;
    }
}
