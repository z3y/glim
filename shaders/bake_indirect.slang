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

    float4 visibility = CompactedVisibility[index];
    float2 bary = visibility.xy;
    uint primitiveID = asuint(visibility.z);

    uint i0 = Indices[primitiveID * 3 + 0];
    uint i1 = Indices[primitiveID * 3 + 1];
    uint i2 = Indices[primitiveID * 3 + 2];

    float3 p0 = Vertices[i0].GetPosition();
    float3 p1 = Vertices[i1].GetPosition();
    float3 p2 = Vertices[i2].GetPosition();
    float2 uv0 = Vertices[i0].GetUV();
    float2 uv1 = Vertices[i1].GetUV();
    float2 uv2 = Vertices[i2].GetUV();

    float3 edge1 = p1 - p0;
    float3 edge2 = p2 - p0;
    float2 duv1 = uv1 - uv0;
    float2 duv2 = uv2 - uv0;

    float det = duv1.x * duv2.y - duv1.y * duv2.x;
    float invDet = 1.0 / (abs(det) + 1e-9);
    float invDetSigned = abs(det) > 1e-12 ? 1.0 / det : 0.0;

    float3 dPdU = (edge1 * duv2.y - edge2 * duv1.y) * invDet;
    float3 dPdV = (edge2 * duv1.x - edge1 * duv2.x) * invDet;

    float3 worldTexelU = dPdU;
    float3 worldTexelV = dPdV;

    uint sampleIndex = Push.sampleIndex;
    uint sampleCount = Push.maxSamples;
    uint bounceIndex = Push.bounceIndex;
    float sampleScale = 1.0 / float(sampleCount);

    uint sobolIndexMask = ~0u;

    SamplingPattern pattern = SamplingPattern::BlueNoiseFirst;
    uint pixelSeed = RNGPixelInit1D(index);
    uint3 indexing = BlueNoiseIndexing(pixelSeed, sampleIndex, sobolIndexMask, sampleCount, pattern);

    uint dimension = 3 + bounceIndex * 2;
    float2 xiBounce = SobolBurleySample2D(indexing.x, dimension, indexing.y, indexing.z);

    float2 xiAA = SobolBurleySample2D(indexing.x, 1, indexing.y, indexing.z); // todo unique dimension
    float2 jitterTexel = xiAA - 0.5;

    float2 dBary;
    dBary.x = (duv2.y * jitterTexel.x - duv2.x * jitterTexel.y) * invDetSigned;
    dBary.y = (-duv1.y * jitterTexel.x + duv1.x * jitterTexel.y) * invDetSigned;

    float3 w0 = float3(1.0 - bary.x - bary.y, bary.x, bary.y);
    float3 dw = float3(-dBary.x - dBary.y, dBary.x, dBary.y);

    // Clip the jitter segment [bary, bary + dBary] to the triangle instead of
    // projecting the end point. Barycentrics: w = (1 - x - y, x, y)
    float t = 1.0;
    [unroll]
    for (int k = 0; k < 3; k++)
    {
        if (dw[k] < 0.0)
        {
            // largest t where w0[k] + t * dw[k] >= 0
            t = min(t, max(w0[k], 0.0) / -dw[k]);
        }
    }

    float2 jBary = bary + dBary * t;

    // float-error safety only
    jBary = max(jBary, 0.0);
    float s = jBary.x + jBary.y;
    if (s > 1.0)
    {
        jBary /= s;
    }

    Surface surface = Surface::Get(primitiveID, jBary);
    float3 positionWS = RayOffset(surface.P, surface.Ng);
    float3 normalWS = surface.N;

    Ray ray;
    ray.P = positionWS;
    ray.D = RandomDirectionInHemisphereCosine(normalWS, xiBounce);
    ray.tMin = 0;
    ray.tMax = RAY_MAX;

    float4 color = float4(0.0, 0.0, 0.0, 1.0);
    float3 throughput = float3(1.0);
    float4 dominantDirection = 0.0;
    SHProbeL2 sh = SHProbeL2::zero();

    uint currentOffset;
    uint previousOffset;
    if (Push.bounceIndex % 2 == 0)
    {
        currentOffset = 0;
        previousOffset = Push.compactedCount;
    }
    else
    {
        currentOffset = Push.compactedCount;
        previousOffset = 0;
    }

    float ao = 1.0;
    float aoRadius = AO_RANGE;

    float *bounceBuffer = Push.bounceBuffer;

    Hit bounceHit;
    if (TraceRay(ray, bounceHit))
    {
        Surface surface = Surface::Get(bounceHit.primitiveID, bounceHit.bary, bounceHit.frontFace);

        if (bounceHit.t < aoRadius)
        {
            ao = 0.0;
        }

        if (!bounceHit.frontFace)
        {
            // this is incorrect, the cached bounce doesnt store lighting on the other side
            // if (!surface.backfaceGI)
            // {
            color.a = 0;
            // }
        }

        float3 bounceAlbedo = SampleAlbedo(surface.uv, surface.lightmapGroup).rgb;
        uint compactIndex = GetCompactedIndex(surface.uv, surface.lightmapGroup, bounceHit.primitiveID, true);
        if (compactIndex != uint::maxValue)
        {
            float3 previousDiffuse = float3(bounceBuffer[(previousOffset + compactIndex) * 3 + 0],
                                            bounceBuffer[(previousOffset + compactIndex) * 3 + 1],
                                            bounceBuffer[(previousOffset + compactIndex) * 3 + 2]);

            float3 current = previousDiffuse * bounceAlbedo * color.a;
            current *= lerp(1.0, ao, AO_INTENSITY);
            color.rgb += current;
            dominantDirection.xyz += ray.D * Luminance(current);

            AccumulateL1LightmapIndirect(sh, current, ray.D, normalWS);
        }
        else
        {
            // color.rgb += float3(1, 0, 1);
        }
    }

    // ValidateSampleColor(color);

    float4 previous = float4(
        bounceBuffer[(currentOffset + index) * 3 + 0],
        bounceBuffer[(currentOffset + index) * 3 + 1],
        bounceBuffer[(currentOffset + index) * 3 + 2],
        1.0);

    AccumulateColor(previous, sampleIndex, color);
    bounceBuffer[(currentOffset + index) * 3 + 0] = previous.r;
    bounceBuffer[(currentOffset + index) * 3 + 1] = previous.g;
    bounceBuffer[(currentOffset + index) * 3 + 2] = previous.b;

    // if (LIGHTMAP_MODE == LightmapMode::CombinedSH)
    // {
    //     float3 L0 = sh.sh[0].xyz * sampleScale;

    //     // write L0
    //     uint index = index + Push.compactedCount;
    //     CompactedLightmap[index * 3 + 0] += L0.x;
    //     CompactedLightmap[index * 3 + 1] += L0.y;
    //     CompactedLightmap[index * 3 + 2] += L0.z;

    //     float3 combinedSH = EncodeCombinedSHL1(sh);
    //     combinedSH *= sampleScale;

    //     // write L1
    //     index += Push.compactedCount;
    //     CompactedLightmap[index * 3 + 0] += combinedSH.x;
    //     CompactedLightmap[index * 3 + 1] += combinedSH.y;
    //     CompactedLightmap[index * 3 + 2] += combinedSH.z;
    // }
    // else if (LIGHTMAP_MODE == LightmapMode::DominantDirection)
    // {
    //     uint index = index + Push.compactedCount;
    //     CompactedLightmap[index * 3 + 0] += dominantDirection.x;
    //     CompactedLightmap[index * 3 + 1] += dominantDirection.y;
    //     CompactedLightmap[index * 3 + 2] += dominantDirection.z;
    // }

    bool last = Push.sampleIndex == (Push.maxSamples - 1);
    [branch]
    if (last)
    {
        previous.rgb *= INDIRECT_INTENSITY;

        CompactedLightmap[index * 3 + 0] += previous.r;
        CompactedLightmap[index * 3 + 1] += previous.g;
        CompactedLightmap[index * 3 + 2] += previous.b;
    }
}
