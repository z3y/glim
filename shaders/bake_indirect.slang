#include "includes/core.slang"
#include "includes/sobol_burley.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    uint compactedCount;
    uint sampleIndex;
    uint maxSamples;
    uint bounceCount;

    float *bounceBuffer;
    uint pad0;
    uint pad1;
};

[shader("compute")]
[numthreads(64, 1, 1)]
void main(uint3 threadID: SV_DispatchThreadID)
{
    uint index = threadID.x;

    if (index >= Push.compactedCount)
    {
        return;
    }

    [branch]
    if (CompactedVisibility[index].w == asfloat(uint::maxValue))
    {
        return;
    }

    uint sampleIndex = Push.sampleIndex;
    uint sampleCount = Push.maxSamples;
    uint bounceCount = Push.bounceCount;
    float sampleScale = 1.0 / float(sampleCount);

    float3 positionWS;
    float3 normalWS;
    uint3 indexing;
    CreateSample(index, sampleIndex, sampleCount, positionWS, normalWS, indexing);

    float4 color = float4(0.0, 0.0, 0.0, 1.0);
    float3 throughput = float3(1.0);
    float4 dominantDirection = 0.0;
    SHProbeL2 sh = SHProbeL2::zero();

    float ao = 1.0;
    float aoRadius = AO_RANGE;

    float *bounceBuffer = Push.bounceBuffer;

    Ray ray;
    ray.tMin = 0;
    ray.tMax = RAY_MAX;

    [loop]
    for (uint bounceIndex = 0; bounceIndex < bounceCount; bounceIndex++)
    {
        uint dimension = 4 + bounceIndex * 2;
        float2 xiBounce = SobolBurleySample2D(indexing.x, dimension, indexing.y, indexing.z);
        ray.P = positionWS;
        ray.D = RandomDirectionInHemisphereCosine(normalWS, xiBounce);

        Hit bounceHit;
        [branch]
        if (TraceRay(ray, bounceHit))
        {
            Surface surface = Surface::Get(bounceHit.primitiveID, bounceHit.bary, bounceHit.frontFace);

            if (bounceHit.t < aoRadius)
            {
                ao = 0.0; // todo check
            }

            if (!bounceHit.frontFace)
            {
                // this is incorrect, the cached bounce doesnt store lighting on the other side
                // if (!surface.backfaceGI)
                // {
                // color.a = 0;
                // }
                break;
            }

            float3 bounceAlbedo = SampleAlbedo(surface.uv, surface.lightmapGroup).rgb;
            uint compactIndex = GetCompactedIndex(surface.uv, surface.lightmapGroup, bounceHit.primitiveID, true);
            if (compactIndex != uint::maxValue)
            {
                float3 previousDiffuse = float3(CompactedLightmap[compactIndex * 3 + 0],
                                                CompactedLightmap[compactIndex * 3 + 1],
                                                CompactedLightmap[compactIndex * 3 + 2]);

                if (all(isfinite(previousDiffuse)))
                {
                    float3 current = previousDiffuse * bounceAlbedo * color.a * throughput;
                    current *= lerp(1.0, ao, AO_INTENSITY);
                    color.rgb += current;
                    dominantDirection.xyz += ray.D * Luminance(current);

                    AccumulateL1LightmapIndirect(sh, current, ray.D, normalWS);
                }
            }
            else
            {
                // color.rgb += float3(1, 0, 1);
            }

            throughput *= bounceAlbedo;
            positionWS = RayOffset(surface.P, surface.Ng);
            normalWS = surface.N;
        }
        else
        {
            break;
        }
    }

    color *= sampleScale;

    ValidateSampleColor(color);

    bounceBuffer[index * 3 + 0] += color.r;
    bounceBuffer[index * 3 + 1] += color.g;
    bounceBuffer[index * 3 + 2] += color.b;

    if (LIGHTMAP_MODE == LightmapMode::CombinedSH)
    {
        float3 L0 = sh.sh[0].xyz * sampleScale;

        uint indexL0 = index + Push.compactedCount;
        CompactedLightmap[indexL0 * 3 + 0] += L0.x;
        CompactedLightmap[indexL0 * 3 + 1] += L0.y;
        CompactedLightmap[indexL0 * 3 + 2] += L0.z;

        float3 combinedSH = EncodeCombinedSHL1(sh);
        combinedSH *= sampleScale;

        uint indexL1 = indexL0 + Push.compactedCount;
        CompactedLightmap[indexL1 * 3 + 0] += combinedSH.x;
        CompactedLightmap[indexL1 * 3 + 1] += combinedSH.y;
        CompactedLightmap[indexL1 * 3 + 2] += combinedSH.z;
    }
    else if (LIGHTMAP_MODE == LightmapMode::DominantDirection)
    {
        uint indexDir = index + Push.compactedCount;
        CompactedLightmap[indexDir * 3 + 0] += dominantDirection.x;
        CompactedLightmap[indexDir * 3 + 1] += dominantDirection.y;
        CompactedLightmap[indexDir * 3 + 2] += dominantDirection.z;
    }

    bool last = Push.sampleIndex == (Push.maxSamples - 1);
    [branch]
    if (last)
    {
        float3 bounce = float3(bounceBuffer[index * 3 + 0],
                               bounceBuffer[index * 3 + 1],
                               bounceBuffer[index * 3 + 2]);

        bounce.rgb *= INDIRECT_INTENSITY;

        CompactedLightmap[index * 3 + 0] += bounce.r;
        CompactedLightmap[index * 3 + 1] += bounce.g;
        CompactedLightmap[index * 3 + 2] += bounce.b;
    }
}
