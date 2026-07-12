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

    // float4 visibility = VisibilityBuffer[coords];
    float4 visibility = 0;

    if (all(visibility == 0))
    {
        return;
    }

    float3 positionWS = visibility.xyz;
    float3 normalWS = UnpackNormalOctahedron(visibility.w);

    uint sampleIndex = Push.sampleIndex;
    uint sampleCount = Push.maxSamples;
    uint bounceIndex = Push.bounceIndex;

    uint sobolIndexMask = ~0u;

    SamplingPattern pattern = SamplingPattern::BlueNoiseFirst;
    uint pixelSeed = RNGPixelInit(coords, pattern);
    uint3 indexing = BlueNoiseIndexing(pixelSeed, sampleIndex, sobolIndexMask, sampleCount, pattern);

    uint dimension = bounceIndex + 3;
    float2 xiBounce = SobolBurleySample2D(indexing.x, dimension, indexing.y, indexing.z);

    Ray ray;
    ray.P = positionWS;
    ray.D = RandomDirectionInHemisphere(normalWS, xiBounce);
    ray.tMin = 0;
    ray.tMax = RAY_MAX;

    float4 color = 0.0;
    float4 dominantDirection = 0.0;

    Hit bounceHit;
    if (TraceRay(ray, bounceHit))
    {
        Surface surface = Surface::Get(bounceHit.primitiveID, bounceHit.bary, bounceHit.frontFace);

        if (!bounceHit.frontFace)
        {
            if (!surface.backfaceGI)
            {
                return;
            }
        }

        float3 bounceAlbedo = SampleAlbedo(surface.uv, surface.lightmapGroup).rgb;
        float3 previousDiffuse = SamplePreviousDiffuse(surface.uv, surface.lightmapGroup).rgb;
        float3 current = previousDiffuse * bounceAlbedo;
        color.rgb += current;
        dominantDirection.xyz += ray.D * Luminance(current);
    }

    float4 previous = LightmapDiffuse[coords];
    color.a = previous.a;
    AccumulateColor(previous, sampleIndex, color);
    LightmapDiffuse[coords] = previous;

    uint index = (coords.x + coords.y * resolution.x) * 4;
    dominantDirection.z = -dominantDirection.z; // unity coords

    // todo temp
    if (sampleIndex == 0)
    {
        DominantDirection[index + 0] = dominantDirection.x;
        DominantDirection[index + 1] = dominantDirection.y;
        DominantDirection[index + 2] = dominantDirection.z;
    }
    else
    {
        DominantDirection[index + 0] += dominantDirection.x;
        DominantDirection[index + 1] += dominantDirection.y;
        DominantDirection[index + 2] += dominantDirection.z;
    }
}
