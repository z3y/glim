#include "includes/core.slang"
#include "includes/sobol_burley.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    RayTracingData rt;

    uint sampleIndex;
    uint sampleCount;
    uint lightCount;
    uint bounceCount;

    uint2 resolution;
    Light *lights;
};

[shader("compute")]
[numthreads(8, 8, 1)]
void main(uint3 threadID: SV_DispatchThreadID)
{
    uint2 coords = threadID.xy;

    if (any(coords >= Push.resolution))
    {
        return;
    }

    float4 visibility = VisibilityBuffer[coords];
    uint primitiveID = asuint(visibility.z);
    if (primitiveID == uint::maxValue)
    {
        return;
    }

    float2 bary = visibility.xy;
    Surface surface = Surface::Get(primitiveID, bary);

    uint sampleIndex = Push.sampleIndex;
    uint sampleCount = Push.sampleCount;

    uint pixelSeed = asuint(visibility.w);
    uint3 indexing = BlueNoiseIndexing(pixelSeed, sampleIndex, ~0u, sampleCount, SamplingPattern::BlueNoiseFirst);
    uint dimension = 0;

    float4 color = float4(0.0, 0.0, 0.0, 1.0);
    float3 throughput = float3(1.0, 1.0, 1.0);

    float3 emission = SampleEmission(surface.coords, surface.lightmapGroup).rgb;
    if (!surface.emissive)
    {
        emission = 0.0;
    }
    float3 albedo = SampleAlbedo(surface.coords, surface.lightmapGroup).rgb;
    color.rgb += emission;
    throughput = albedo;

    float3 positionWS = RayOffset(surface.P, surface.Ng);
    float3 normalWS = surface.N;

    for (uint bounceIndex = 0; bounceIndex < (Push.bounceCount + 1); bounceIndex++)
    {
        float2 xiLight = SobolBurleySample2D(indexing.x, dimension++, indexing.y, indexing.z);

        for (uint lightIndex = 0; lightIndex < Push.lightCount; lightIndex++)
        {
            float3 L;
            Light light = Push.lights[lightIndex];
            float3 current = ShadeLightDirect(light, positionWS, normalWS, xiLight, throughput, L, false);
            float NoL = saturate(dot(L, normalWS));
            color.rgb += current * NoL;
        }

        float2 xiBounce = SobolBurleySample2D(indexing.x, dimension++, indexing.y, indexing.z);

        Ray ray;
        ray.tMin = 0;
        ray.tMax = RAY_MAX;
        ray.P = positionWS;
        ray.D = RandomDirectionInHemisphereCosine(normalWS, xiBounce);

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

            float3 bounceAlbedo = SampleAlbedo(surface.coords, surface.lightmapGroup).rgb;
            if (surface.emissive)
            {
                float3 bounceEmission = SampleEmission(surface.coords, surface.lightmapGroup).rgb;
                color.rgb += bounceEmission * throughput;
            }

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
