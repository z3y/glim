#define PREVIEW_SHADER
#include "includes/core.slang"
#include "includes/sobol_burley.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    uint lightCount;
    uint maxSamples;

    uint sampleIndex;
    uint width;
    uint height;
    uint bounceCount;
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

    float4 color = 0.0;
    float3 throughput = float3(1.0);

    float4 visibility = VisibilityBuffer[coords];

    uint encoded = asuint(visibility.a);
    bool visible = encoded != 0;
    if (!visible)
    {
        LightmapDiffuse[coords] = 0.0;
        return;
    }

    bool hitSkybox = (encoded & (1 << 2)) != 0;
    if (hitSkybox)
    {
        float3 viewDirectionWS = visibility.xyz;
        LightmapDiffuse[coords] = float4(SampleSkybox(viewDirectionWS), 1.0);
        return;
    }

    uint primitiveID = asuint(visibility.z);
    float2 bary = visibility.xy;
    bool useSmoothP = (encoded & (1 << 1)) != 0;

    Surface surface = Surface::Get(primitiveID, bary);

    uint sampleIndex = Push.sampleIndex;
    uint sampleCount = Push.maxSamples;

    uint sobolIndexMask = ~0u;
    SamplingPattern pattern = SamplingPattern::BlueNoiseFirst;
    uint pixelSeed = RNGPixelInit(coords, pattern);
    uint3 indexing = BlueNoiseIndexing(pixelSeed, sampleIndex, sobolIndexMask, sampleCount, pattern);
    uint dimension = 0;

    float3 emission = SampleEmission(surface.uv, surface.lightmapGroup).rgb;
    float3 albedo = SampleAlbedo(surface.uv, surface.lightmapGroup).rgb;
    color.rgb += emission;
    throughput = albedo;

    if (useSmoothP)
    {
        surface.P = surface.smoothP;
    }

    float3 positionWS = RayOffset(surface.P, surface.Ng);
    float3 normalWS = surface.N;

    for (uint bounceIndex = 0; bounceIndex < (Push.bounceCount + 1); bounceIndex++)
    {
        float2 xiLight = SobolBurleySample2D(indexing.x, dimension++, indexing.y, indexing.z);

        for (uint lightIndex = 0; lightIndex < Push.lightCount; lightIndex++)
        {
            float3 L;
            color.rgb += ShadeLightDirect(Lights[lightIndex], positionWS, normalWS, xiLight, throughput, L);
        }

        float2 xiBounce = SobolBurleySample2D(indexing.x, dimension++, indexing.y, indexing.z);

        Ray ray;
        ray.tMin = 0;
        ray.tMax = RAY_MAX;
        ray.P = positionWS;
        ray.D = RandomDirectionInHemisphere(normalWS, xiBounce);

        Hit bounceHit;
        if (TraceRay(ray, bounceHit))
        {
            Surface surface = Surface::Get(bounceHit.primitiveID, bounceHit.bary, bounceHit.frontFace);
            positionWS = RayOffset(surface.P, surface.Ng);
            normalWS = surface.N;

            if (!bounceHit.frontFace)
            {
                if (!surface.backfaceGI)
                {
                    break;
                }
            }

            float3 bounceAlbedo = SampleAlbedo(surface.uv, surface.lightmapGroup).rgb;
            float3 bounceEmission = SampleEmission(surface.uv, surface.lightmapGroup).rgb;

            color.rgb += bounceEmission * throughput;

            throughput *= bounceAlbedo;
        }
        else
        {
            color.rgb += throughput * SampleSkybox(ray.D);
            break;
        }
    }

    float4 previous = LightmapDiffuse[coords];
    AccumulateColor(previous, sampleIndex, color);
    previous.a = 1.0;
    LightmapDiffuse[coords] = previous;
}
