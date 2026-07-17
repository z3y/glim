#include "includes/core.slang"
#include "includes/sobol_burley.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    uint compactedCount;
    uint sampleIndex;
    uint maxSamples;
    uint bounceIndex;
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
    uint bounceIndex = Push.bounceIndex;

    uint sobolIndexMask = ~0u;

    SamplingPattern pattern = SamplingPattern::BlueNoiseFirst;
    uint pixelSeed = RNGPixelInit1D(index);
    uint3 indexing = BlueNoiseIndexing(pixelSeed, sampleIndex, sobolIndexMask, sampleCount, pattern);

    // uint dimension = bounceIndex + 3;
    uint dimension = 3 + bounceIndex * 2;
    float2 xiBounce = SobolBurleySample2D(indexing.x, dimension, indexing.y, indexing.z);

    Ray ray;
    ray.P = positionWS;
    ray.D = RandomDirectionInHemisphere(normalWS, xiBounce);
    ray.tMin = 0;
    ray.tMax = RAY_MAX;

    float4 color = float4(0.0, 0.0, 0.0, 1.0);
    float3 throughput = float3(1.0);
    float4 dominantDirection = 0.0;

    uint bounceAOffset = Push.compactedCount;
    uint bounceBOffset = Push.compactedCount * 2;

    if (LIGHTMAP_MODE == LightmapMode::Directional)
    {
        bounceAOffset += Push.compactedCount;
        bounceBOffset += Push.compactedCount;
    }

    uint previousOffset = 0;
    uint currentOffset = 0;
    if (Push.bounceIndex == 0)
    {
        previousOffset = 0;
        currentOffset = bounceAOffset;
    }
    else
    {
        bool swap = Push.bounceIndex % 2 != 0;
        currentOffset = swap ? bounceBOffset : bounceAOffset;
        previousOffset = swap ? bounceAOffset : bounceBOffset;
    }

    Hit bounceHit;
    if (TraceRay(ray, bounceHit))
    {
        Surface surface = Surface::Get(bounceHit.primitiveID, bounceHit.bary, bounceHit.frontFace);

        if (!bounceHit.frontFace)
        {
            // this is incorrect, the cached bounce doesnt store lighting on the other side
            // if (!surface.backfaceGI)
            // {
            color.a = 0;
            // }
        }

        float3 bounceAlbedo = SampleAlbedo(surface.uv, surface.lightmapGroup).rgb;
        uint compactIndex = GetCompactedIndex(surface.uv, surface.lightmapGroup, true);
        if (compactIndex != uint::maxValue)
        {
            float3 previousDiffuse = float3(CompactedLightmap[(previousOffset + compactIndex) * 3 + 0],
                                            CompactedLightmap[(previousOffset + compactIndex) * 3 + 1],
                                            CompactedLightmap[(previousOffset + compactIndex) * 3 + 2]);

            float3 current = previousDiffuse * bounceAlbedo * color.a;
            color.rgb += current;
            dominantDirection.xyz += ray.D * Luminance(current);
        }
        // else
        // {
        //     color.rgb += float3(1, 0, 1);
        // }
    }

    float4 previous = float4(
        CompactedLightmap[(currentOffset + index) * 3 + 0],
        CompactedLightmap[(currentOffset + index) * 3 + 1],
        CompactedLightmap[(currentOffset + index) * 3 + 2],
        1.0);

    AccumulateColor(previous, sampleIndex, color);
    CompactedLightmap[(currentOffset + index) * 3 + 0] = previous.r;
    CompactedLightmap[(currentOffset + index) * 3 + 1] = previous.g;
    CompactedLightmap[(currentOffset + index) * 3 + 2] = previous.b;

    if (LIGHTMAP_MODE == LightmapMode::Directional)
    {
        uint index = index + Push.compactedCount;
        CompactedLightmap[index * 3 + 0] += dominantDirection.x;
        CompactedLightmap[index * 3 + 1] += dominantDirection.y;
        CompactedLightmap[index * 3 + 2] += dominantDirection.z;
    }

    bool last = Push.sampleIndex == (Push.maxSamples - 1);
    [branch] if (last)
    {
        CompactedLightmap[index * 3 + 0] += previous.r;
        CompactedLightmap[index * 3 + 1] += previous.g;
        CompactedLightmap[index * 3 + 2] += previous.b;
    }
}
