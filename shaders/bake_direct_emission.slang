#include "includes/core.slang"
#include "includes/sobol_burley.slang"

[[vk::push_constant]]
PushConstants Push;

struct PushConstants
{
    uint compactedCount;
    uint sampleIndex;
    uint maxSamples;
    uint pad0;
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
    // uint x = asuint(visibility.w) & 0xFFFF;
    // uint y = (asuint(visibility.w) >> 16) & 0xFFFF;

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
    float sampleScale = 1.0 / float(sampleCount);

    SamplingPattern pattern = SamplingPattern::BlueNoiseFirst;
    uint pixelSeed = RNGPixelInit1D(index);
    uint sobolIndexMask = ~0u;
    uint3 indexing = BlueNoiseIndexing(pixelSeed, sampleIndex, sobolIndexMask, sampleCount, pattern);

    float2 xiAA = SobolBurleySample2D(indexing.x, 3, indexing.y, indexing.z);
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

    float4 color = float4(0.0, 0.0, 0.0, 1.0);
    float3 throughput = float3(1.0);
    float4 dominantDirection = 0.0;
    SHProbeL2 sh = SHProbeL2::zero();

    float2 xiBounce = SobolBurleySample2D(indexing.x, 1, indexing.y, indexing.z);

    Ray ray;
    ray.P = positionWS;
    ray.D = RandomDirectionInHemisphereCosine(normalWS, xiBounce);
    ray.tMin = 0;
    ray.tMax = RAY_MAX;

    float randomEmissiveTrianglePdf = 1.0 / float(EMISSIVE_TRIANGLES_COUNT);
    float bsdfPdf = 1.0 / float.getPi();

    if (EMISSIVE_TRIANGLES_COUNT > 0)
    {
        float2 xi = SobolBurleySample2D(indexing.x, 2, indexing.y, indexing.z);
        uint triIndex = min(uint(xi.x * float(EMISSIVE_TRIANGLES_COUNT)), EMISSIVE_TRIANGLES_COUNT - 1);
        uint emissivePrimitiveID = EmissiveTriangles[triIndex];

        float3 L;
        float pdf = 0.0;
        float3 current = ShadeTriangleLight(emissivePrimitiveID, positionWS, normalWS, xi.y, throughput, randomEmissiveTrianglePdf, pdf, L, false);
        float3 NoL = saturate(dot(L, normalWS));

        current *= MISWeight(pdf, bsdfPdf);
        color.rgb += current * NoL;
        dominantDirection.xyz += L * Luminance(current * NoL);
        AccumulateL1LightmapDirect(sh, current, L, normalWS);
    }

    Hit hit;
    if (TraceRay(ray, hit))
    {
        Surface surface = Surface::Get(hit.primitiveID, hit.bary, hit.frontFace);

        if (!hit.frontFace)
        {
            if (!surface.backfaceGI)
            {
                color.a = 0;
            }
        }

        if (surface.emissive)
        {
            float3 bounceEmission = SampleEmission(surface.uv, surface.lightmapGroup).rgb;
            float3 emission = bounceEmission * color.a;

            if (any(emission > 0.0))
            {
                if (EMISSIVE_TRIANGLES_COUNT > 0)
                {
                    float triangleArea = CalculateTriangleArea(hit.primitiveID);
                    float3 positionToLight = surface.P - positionWS;
                    float distSq = max(dot(positionToLight, positionToLight), 0.0001);
                    float cosThetaLight = saturate(dot(-normalize(positionToLight), surface.N));

                    if (cosThetaLight > 0.0)
                    {
                        float lightPdf = (randomEmissiveTrianglePdf / triangleArea) * distSq / cosThetaLight;
                        emission *= MISWeight(bsdfPdf, lightPdf);
                        color.rgb += emission;
                        dominantDirection.xyz += ray.D * Luminance(emission);
                        AccumulateL1LightmapIndirect(sh, emission, ray.D, normalWS);
                    }
                }
                else
                {
                    color.rgb += emission;
                    dominantDirection.xyz += ray.D * Luminance(emission);
                    AccumulateL1LightmapIndirect(sh, emission, ray.D, normalWS);
                }
            }
        }
    }
    else
    {
        float3 current = SampleSkybox(ray.D);
        color.rgb += current;
        dominantDirection.xyz += ray.D * Luminance(current);
        AccumulateL1LightmapIndirect(sh, current, ray.D, normalWS);
    }

    // ValidateSampleColor(color);

    CompactedLightmap[index * 3 + 0] += color.x * sampleScale;
    CompactedLightmap[index * 3 + 1] += color.y * sampleScale;
    CompactedLightmap[index * 3 + 2] += color.z * sampleScale;

    // todo
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
}
